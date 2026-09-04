pub mod bedrock;
pub mod claude;
pub mod cost;
pub mod gemini;
pub mod local;
pub mod openai;
pub mod routing;
pub mod tools;
pub use routing::{classify_workload, Workload};

use crate::voice::tts::TtsCommand;
use local::backend::{BackendType, LocalEndpoint};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderChainEntry {
    pub position: i32,
    pub provider_type: String, // "gemini", "claude", "openai", "bedrock", "local"
    pub endpoint_id: Option<String>,
    pub model_id: Option<String>,
    pub enabled: bool,
}

/// Gemini model selection: a user-configured provider-chain override wins;
/// otherwise pick by workload tier. Used by both provider_name display and
/// dispatch so the emitted label never diverges from the model actually sent.
fn gemini_model_for(model_override: Option<&str>, workload: Workload) -> &str {
    model_override.unwrap_or(match workload {
        Workload::Premium => gemini::MODEL_PRO,
        _ => gemini::MODEL_FLASH,
    })
}

/// Geography prefixes used by Bedrock cross-region inference profile ids.
/// `global.` is deliberately absent -- it is geography-agnostic and must not be
/// rewritten.
const INFERENCE_GEOGRAPHIES: [&str; 3] = ["apac", "us", "eu"];

/// Retarget a Bedrock model id at the geography the credentials were saved for.
///
/// The `workload_routing` seed pins the volume tier to `apac.amazon.nova-micro-v1:0`
/// (V13), so a user on `us-east-1` would otherwise send an APAC inference
/// profile to a US endpoint and get a ValidationException on every volume-tier
/// request. Ids prefixed `global.`, and ids with no geography prefix at all,
/// are left untouched.
fn apply_inference_profile(model: &str, profile: &str) -> String {
    match model.split_once('.') {
        Some((head, rest)) if INFERENCE_GEOGRAPHIES.contains(&head) && head != profile => {
            format!("{}.{}", profile, rest)
        }
        _ => model.to_string(),
    }
}

/// Keychain service holding the provider API keys the user pastes in
/// onboarding or Settings.
pub const KEY_SERVICE: &str = "com.jarvis.ai";

/// Credential lookup order: process env first (dev workflow and
/// `~/Library/Application Support/jarvis/.env`), then the Keychain. An empty
/// env var counts as absent -- otherwise a stray `GEMINI_API_KEY=` in a
/// dotfile would mask a key the user actually saved.
fn resolve_key(env_var: &str, fallback: Option<String>) -> Option<String> {
    match std::env::var(env_var) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => fallback.filter(|v| !v.trim().is_empty()),
    }
}

/// Position and length of the next SSE event delimiter, whichever of
/// `\r\n\r\n` and `\n\n` comes first.
fn find_event_delimiter(bytes: &[u8]) -> Option<(usize, usize)> {
    let crlf = bytes.windows(4).position(|w| w == b"\r\n\r\n");
    let lf = bytes.windows(2).position(|w| w == b"\n\n");
    match (crlf, lf) {
        (Some(c), Some(l)) if c <= l => Some((c, 4)),
        (Some(_), Some(l)) => Some((l, 2)),
        (Some(c), None) => Some((c, 4)),
        (None, Some(l)) => Some((l, 2)),
        (None, None) => None,
    }
}

/// Byte-level SSE framing shared by every streaming provider.
///
/// Chunk boundaries from `bytes_stream()` are transport boundaries, not
/// character boundaries. Decoding each chunk on its own with
/// `from_utf8_lossy` destroys any multi-byte sequence that straddles two
/// chunks -- the truncated tail of chunk N and the orphaned continuation bytes
/// of chunk N+1 both collapse to U+FFFD, and the character is gone before the
/// SSE reassembly ever sees it. Every CJK character is three bytes, so that is
/// a direct hit on Cantonese and Chinese replies. Accumulating raw bytes and
/// decoding only whole event blocks -- which always end on an ASCII delimiter,
/// and are therefore complete UTF-8 -- carries the partial sequence forward.
pub(crate) struct SseByteBuffer {
    bytes: Vec<u8>,
}

impl SseByteBuffer {
    pub(crate) fn new() -> Self {
        SseByteBuffer { bytes: Vec::new() }
    }

    pub(crate) fn extend(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
    }

    /// Pop the next complete event block, or `None` while the buffer still
    /// holds only a partial event.
    pub(crate) fn next_event(&mut self) -> Option<String> {
        let (pos, delim_len) = find_event_delimiter(&self.bytes)?;
        let block = match std::str::from_utf8(&self.bytes[..pos]) {
            Ok(s) => s.to_string(),
            Err(e) => {
                log::warn!("[sse] event block is not valid UTF-8: {e}");
                String::from_utf8_lossy(&self.bytes[..pos]).into_owned()
            }
        };
        self.bytes.drain(..pos + delim_len);
        Some(block)
    }
}

/// Identity of a tool call inside one batch. The name alone is not enough:
/// three `read_email` calls with different ids are a legitimate fan-out, and
/// collapsing them makes the model answer about one email while believing it
/// read three. Only an identical name *and* identical arguments is a repeat.
pub(crate) fn tool_call_key(name: &str, args_str: &str) -> String {
    format!("{}:{}", name, args_str)
}

/// Tools whose execution changes state the user can see outside JARVIS.
/// Falling through to the next provider replays the whole turn from the
/// original messages, so anything in this list would be carried out twice.
const SIDE_EFFECTING_TOOLS: &[&str] = &[
    "create_event",
    "update_event",
    "delete_event",
    "draft_email",
    "archive_email",
    "create_github_issue",
    "create_notion_page",
    "create_task",
    "remember",
    "forget",
];

/// Per-turn tally of side-effecting tool calls, shared between the router and
/// whichever provider is currently running. Providers bump it as they execute;
/// the router reads it to decide whether a fallback would duplicate work.
#[derive(Clone, Default)]
pub struct SideEffectLedger(Arc<AtomicUsize>);

impl SideEffectLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `tool_name` if it mutates state outside JARVIS.
    pub fn note(&self, tool_name: &str) {
        if SIDE_EFFECTING_TOOLS.contains(&tool_name) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn count(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}

/// What the router should do with a failed chain entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorDisposition {
    /// Transport fault, rate limit or server-side error: the next entry is
    /// worth trying.
    Retry,
    /// The credential this provider type uses was rejected. Later entries of
    /// the same provider type read the same key, so they would fail
    /// identically; a different vendor may still work.
    SkipSameProvider,
    /// The request itself cannot be accepted. Replaying it anywhere is a
    /// guaranteed second failure.
    StopChain,
}

/// Pull the HTTP status out of the `"<provider> API error <status>: <body>"`
/// strings the providers build. Transport errors, SDK errors and the router's
/// own messages carry no status and return `None`.
fn http_status_in(err: &str) -> Option<u16> {
    let rest = err.split(" API error ").nth(1)?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// Decide whether a provider error is worth replaying against the rest of the
/// chain. Anything unrecognised stays retryable, which is the pre-existing
/// behaviour and the safe direction.
pub(crate) fn classify_provider_error(err: &str) -> ErrorDisposition {
    let lower = err.to_lowercase();
    match http_status_in(err) {
        // Gemini reports a revoked or mistyped key as 400 API_KEY_INVALID;
        // any other 400 is a malformed or oversized request.
        Some(400) => {
            if lower.contains("api_key_invalid") || lower.contains("api key not valid") {
                ErrorDisposition::SkipSameProvider
            } else {
                ErrorDisposition::StopChain
            }
        }
        Some(401) | Some(403) => ErrorDisposition::SkipSameProvider,
        Some(413) | Some(422) => ErrorDisposition::StopChain,
        _ => ErrorDisposition::Retry,
    }
}

/// Key identifying the credential a chain entry authenticates with.
///
/// `gemini`, `claude` and `openai` each read one shared key off the router, and
/// `bedrock` reads one credential row, so a rejected credential really does kill
/// every remaining entry of that provider type. `local` is different: each
/// chain entry names its own `LocalEndpoint`, which carries its own `api_key`
/// and is sent as a per-endpoint bearer token, so one unreachable or
/// misconfigured server must not disable the other local endpoints.
fn credential_scope(entry: &ProviderChainEntry) -> String {
    match entry.provider_type.as_str() {
        "local" => format!("local:{}", entry.endpoint_id.as_deref().unwrap_or("")),
        other => other.to_string(),
    }
}

/// Whether the router still has somewhere to go after `idx`: a later entry that
/// is enabled and whose credential has not already been rejected this turn.
fn has_live_entry_after(
    chain: &[ProviderChainEntry],
    idx: usize,
    dead_credentials: &HashSet<String>,
) -> bool {
    chain
        .iter()
        .skip(idx + 1)
        .any(|next| next.enabled && !dead_credentials.contains(&credential_scope(next)))
}

#[derive(Clone)]
pub struct AiRouter {
    gemini_key: Option<String>,
    claude_key: Option<String>,
    openai_key: Option<String>,
    provider_chain: Vec<ProviderChainEntry>,
    local_endpoints: Vec<LocalEndpoint>,
}

impl AiRouter {
    pub fn new(provider_setting: &str) -> Self {
        // Keys start env-only; `reload_keys` layers the Keychain in
        // afterwards, because construction has no database handle.
        let gemini_key = resolve_key("GEMINI_API_KEY", None);
        let claude_key = resolve_key("ANTHROPIC_API_KEY", None);
        let openai_key = resolve_key("OPENAI_API_KEY", None);
        let chain = match provider_setting {
            "gemini_flash" => vec![
                ProviderChainEntry {
                    position: 0,
                    provider_type: "gemini".into(),
                    endpoint_id: None,
                    model_id: Some(gemini::MODEL_FLASH.into()),
                    enabled: true,
                },
                ProviderChainEntry {
                    position: 1,
                    provider_type: "gemini".into(),
                    endpoint_id: None,
                    model_id: Some(gemini::MODEL_PRO.into()),
                    enabled: true,
                },
            ],
            "gemini_pro_only" => vec![ProviderChainEntry {
                position: 0,
                provider_type: "gemini".into(),
                endpoint_id: None,
                model_id: Some(gemini::MODEL_PRO.into()),
                enabled: true,
            }],
            "gemini_flash_only" => vec![ProviderChainEntry {
                position: 0,
                provider_type: "gemini".into(),
                endpoint_id: None,
                model_id: Some(gemini::MODEL_FLASH.into()),
                enabled: true,
            }],
            _ => vec![
                // gemini_primary (default): position 0 is left unpinned so the
                // workload tier picks the model -- Pro for premium work, Flash
                // for read-only volume and tool turns. Position 1 is pinned to
                // Pro so a Flash outage degrades to a *different* model; pinning
                // Flash here made the fallback a retry of the failing call.
                // Pro is the id with a confirmed public price (cost.rs), so the
                // chain degrades from the doubtful id to the confirmed one.
                ProviderChainEntry {
                    position: 0,
                    provider_type: "gemini".into(),
                    endpoint_id: None,
                    model_id: None,
                    enabled: true,
                },
                ProviderChainEntry {
                    position: 1,
                    provider_type: "gemini".into(),
                    endpoint_id: None,
                    model_id: Some(gemini::MODEL_PRO.into()),
                    enabled: true,
                },
            ],
        };

        AiRouter {
            gemini_key,
            claude_key,
            openai_key,
            provider_chain: chain,
            local_endpoints: Vec::new(),
        }
    }

    /// Load provider chain from database (call after construction)
    pub fn load_from_db(&mut self, db: &crate::db::Database) {
        // Rows first, Keychain second. Resolving an endpoint's api_key is a
        // blocking macOS Keychain lookup, so doing it inside the row mapper
        // held the one global database mutex for as long as a locked keychain
        // -- or an access prompt nobody has answered yet -- takes to return,
        // stalling every other database user during startup. Same rule
        // commands::local_llm and commands::system_check already follow: never
        // Keychain FFI while the DB lock is held.
        let mut endpoints: Option<Vec<LocalEndpoint>> = None;

        if let Ok(conn) = db.conn.lock() {
            let mut stmt = conn
                .prepare(
                    "SELECT id, name, url, backend_type, api_key, use_tls, \
                     connection_timeout_ms, keep_alive_minutes, is_active, \
                     last_health_check, last_health_status FROM local_endpoints WHERE is_active = 1",
                )
                .ok();

            if let Some(ref mut stmt) = stmt {
                let collected: Vec<LocalEndpoint> = stmt
                    .query_map([], |row| {
                        Ok(LocalEndpoint {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            url: row.get(2)?,
                            backend_type: BackendType::from_str(
                                &row.get::<_, String>(3).unwrap_or_default(),
                            ),
                            // Still the raw column value -- a Keychain marker on
                            // any row this build wrote. Exchanged for the secret
                            // below, once the lock is gone.
                            api_key: row.get(4)?,
                            use_tls: row.get::<_, i32>(5).unwrap_or(0) != 0,
                            connection_timeout_ms: row.get::<_, u32>(6).unwrap_or(5000),
                            keep_alive_minutes: row.get::<_, u32>(7).unwrap_or(30),
                            is_active: row.get::<_, i32>(8).unwrap_or(1) != 0,
                            last_health_check: row.get(9)?,
                            last_health_status: row
                                .get::<_, Option<i32>>(10)?
                                .map(|v| v != 0),
                        })
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();
                endpoints = Some(collected);
            }

            let mut chain_stmt = conn
                .prepare(
                    "SELECT position, provider_type, endpoint_id, model_id, enabled \
                     FROM provider_chain ORDER BY position ASC",
                )
                .ok();

            if let Some(ref mut stmt) = chain_stmt {
                let chain: Vec<ProviderChainEntry> = stmt
                    .query_map([], |row| {
                        Ok(ProviderChainEntry {
                            position: row.get(0)?,
                            provider_type: row.get(1)?,
                            endpoint_id: row.get(2)?,
                            model_id: row.get(3)?,
                            enabled: row.get::<_, i32>(4).unwrap_or(1) != 0,
                        })
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();

                if !chain.is_empty() {
                    self.provider_chain = chain;
                }
            }
        }

        // Lock released. The marker in the api_key column is not a credential,
        // so it has to be exchanged for the real secret before the router can
        // send it as a bearer token.
        if let Some(mut endpoints) = endpoints {
            for endpoint in &mut endpoints {
                let resolved = crate::integration_keys::resolve_local_endpoint_key(
                    &endpoint.id,
                    endpoint.api_key.as_deref(),
                );
                endpoint.api_key = resolved;
            }
            self.local_endpoints = endpoints;
        }
    }

    /// Layer Keychain-stored provider keys under the env vars. Called at
    /// startup and again whenever a key is saved in Settings, so a freshly
    /// pasted key takes effect without restarting the app.
    pub fn reload_keys(&mut self) {
        let from_keychain = |account: &str| {
            crate::security::keychain::KeychainStore::get(KEY_SERVICE, account).ok()
        };
        self.gemini_key = resolve_key("GEMINI_API_KEY", from_keychain("gemini_api_key"));
        self.claude_key = resolve_key("ANTHROPIC_API_KEY", from_keychain("claude_api_key"));
        self.openai_key = resolve_key("OPENAI_API_KEY", from_keychain("openai_api_key"));
    }

    pub fn provider_chain(&self) -> &[ProviderChainEntry] {
        &self.provider_chain
    }

    pub fn local_endpoints(&self) -> &[LocalEndpoint] {
        &self.local_endpoints
    }

    pub fn set_provider_chain(&mut self, chain: Vec<ProviderChainEntry>) {
        self.provider_chain = chain;
    }

    pub fn set_local_endpoints(&mut self, endpoints: Vec<LocalEndpoint>) {
        self.local_endpoints = endpoints;
    }

    pub fn find_endpoint(&self, endpoint_id: &str) -> Option<&LocalEndpoint> {
        self.local_endpoints.iter().find(|e| e.id == endpoint_id)
    }

    pub async fn send(
        &self,
        messages: Vec<(String, String)>,
        db: &crate::db::Database,
        google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
        app_handle: &tauri::AppHandle,
        tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
        workload: Workload,
    ) -> Result<String, String> {
        let mut errors: Vec<String> = Vec::new();
        let ledger = SideEffectLedger::new();
        // Credentials already rejected this turn, keyed by `credential_scope`
        // so a dead local endpoint does not take the other local endpoints
        // down with it.
        let mut dead_credentials: HashSet<String> = HashSet::new();

        for (idx, entry) in self.provider_chain.iter().enumerate() {
            if !entry.enabled {
                continue;
            }
            if dead_credentials.contains(&credential_scope(entry)) {
                continue;
            }

            let provider_name = match entry.provider_type.as_str() {
                "gemini" => {
                    let model = gemini_model_for(entry.model_id.as_deref(), workload);
                    format!("gemini ({})", model)
                }
                "local" => {
                    let model = entry.model_id.as_deref().unwrap_or("unknown");
                    format!("{} (local)", model)
                }
                "bedrock" => {
                    let model = entry
                        .model_id
                        .clone()
                        .unwrap_or_else(bedrock::default_model);
                    format!("bedrock ({})", model)
                }
                other => other.to_string(),
            };

            let _ = app_handle.emit(
                "chat-provider",
                json!({
                    "provider": &provider_name,
                    "status": "trying"
                }),
            );

            let result = match entry.provider_type.as_str() {
                "gemini" => {
                    if let Some(ref key) = self.gemini_key {
                        let model = gemini_model_for(entry.model_id.as_deref(), workload);
                        gemini::send(
                            key,
                            model,
                            messages.clone(),
                            db,
                            google_auth,
                            app_handle,
                            tts_tx.clone(),
                            workload,
                            &ledger,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    } else {
                        Err("No Gemini API key configured".to_string())
                    }
                }
                "claude" => {
                    if let Some(ref key) = self.claude_key {
                        claude::send(
                            key,
                            messages.clone(),
                            db,
                            google_auth,
                            app_handle,
                            tts_tx.clone(),
                            workload,
                            &ledger,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    } else {
                        Err("No Claude API key configured".to_string())
                    }
                }
                "openai" => {
                    if let Some(ref key) = self.openai_key {
                        openai::send(
                            key,
                            messages.clone(),
                            db,
                            google_auth,
                            app_handle,
                            tts_tx.clone(),
                            workload,
                            &ledger,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    } else {
                        Err("No OpenAI API key configured".to_string())
                    }
                }
                "bedrock" => {
                    // Load AwsCredentials directly from sqlite + keychain.  We do the
                    // lookups inline rather than going through `AwsCredsRepo::new(Arc)`
                    // because `AiRouter::send` only holds `&Database` -- routing one
                    // borrow through an `Arc` would require restructuring every caller
                    // of `AiRouter`.  This is a thin equivalent of `AwsCredsRepo::load`.
                    let creds_row = {
                        let conn = match db.conn.lock() {
                            Ok(c) => c,
                            Err(e) => {
                                errors.push(format!("bedrock db lock: {e}"));
                                continue;
                            }
                        };
                        conn.query_row(
                            "SELECT access_key_id, region, inference_profile \
                             FROM aws_credentials WHERE id = 1",
                            [],
                            |r| {
                                Ok((
                                    r.get::<_, String>(0)?,
                                    r.get::<_, String>(1)?,
                                    r.get::<_, String>(2)?,
                                ))
                            },
                        )
                        .ok()
                    };
                    let Some((akid, region, profile)) = creds_row else {
                        errors.push(
                            "bedrock: AWS credentials not configured in Settings".to_string(),
                        );
                        continue;
                    };
                    let secret = match crate::security::keychain::KeychainStore::get(
                        "com.jarvis.aws",
                        "secret_access_key",
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            errors.push(format!("bedrock keychain load: {e}"));
                            continue;
                        }
                    };
                    let creds = crate::aws::credentials::AwsCredentials {
                        access_key_id: akid,
                        secret_access_key: secret,
                        region,
                        inference_profile: profile,
                    };

                    // Look up the model id for this workload tier.  Falls back to
                    // the chain-entry model_id (or the legacy default) if the
                    // workload_routing table is missing the requested row.
                    let workload_key = match workload {
                        Workload::Volume => "volume",
                        Workload::Tools => "tools",
                        Workload::Premium => "premium",
                    };
                    let model = {
                        let conn = match db.conn.lock() {
                            Ok(c) => c,
                            Err(e) => {
                                errors.push(format!("bedrock db lock: {e}"));
                                continue;
                            }
                        };
                        conn.query_row(
                            "SELECT model_id FROM workload_routing \
                             WHERE workload = ?1 AND enabled = 1",
                            rusqlite::params![workload_key],
                            |r| r.get::<_, String>(0),
                        )
                        .ok()
                        .or_else(|| entry.model_id.clone())
                        .unwrap_or_else(bedrock::default_model)
                    };
                    // The routing rows carry a fixed geography prefix; point it
                    // at the region these credentials were saved for.
                    let model = apply_inference_profile(&model, &creds.inference_profile);
                    log::info!(
                        "Bedrock workload={:?} -> model={} (region={})",
                        workload,
                        model,
                        creds.region
                    );
                    bedrock::send(
                        &creds,
                        &model,
                        workload,
                        messages.clone(),
                        db,
                        google_auth,
                        app_handle,
                        tts_tx.clone(),
                        &ledger,
                    )
                    .await
                    .map_err(|e| e.to_string())
                }
                "local" => {
                    let endpoint_id = match &entry.endpoint_id {
                        Some(id) => id,
                        None => {
                            errors.push("Local provider missing endpoint_id".to_string());
                            continue;
                        }
                    };
                    let model_id = match &entry.model_id {
                        Some(id) => id,
                        None => {
                            errors.push("Local provider missing model_id".to_string());
                            continue;
                        }
                    };
                    let endpoint = match self.find_endpoint(endpoint_id) {
                        Some(ep) => ep,
                        None => {
                            errors.push(format!(
                                "Endpoint '{}' not found or inactive",
                                endpoint_id
                            ));
                            continue;
                        }
                    };

                    let (context_length, tool_override) = {
                        let conn = db.conn.lock().map_err(|e| e.to_string())?;
                        let ctx_len: u32 = conn
                            .query_row(
                                "SELECT context_length FROM local_model_overrides WHERE endpoint_id = ?1 AND model_id = ?2",
                                rusqlite::params![endpoint_id, model_id],
                                |row| row.get(0),
                            )
                            .unwrap_or(4096);
                        let tool_ov: Option<String> = conn
                            .query_row(
                                "SELECT tool_capability FROM local_model_overrides WHERE endpoint_id = ?1 AND model_id = ?2",
                                rusqlite::params![endpoint_id, model_id],
                                |row| row.get(0),
                            )
                            .ok();
                        (ctx_len, tool_ov)
                    };

                    let backend = local::get_backend(&endpoint.backend_type);
                    let detected_cap = backend
                        .detect_tool_capability(&endpoint.url, model_id)
                        .await;

                    local::send_local(
                        endpoint,
                        model_id,
                        messages.clone(),
                        detected_cap,
                        context_length,
                        tool_override.as_deref(),
                        db,
                        google_auth,
                        app_handle,
                        tts_tx.clone(),
                        workload,
                        &ledger,
                    )
                    .await
                    .map_err(|e| e.to_string())
                }
                unknown => Err(format!("Unknown provider type: {}", unknown)),
            };

            match result {
                Ok(response) => {
                    let _ = app_handle.emit(
                        "chat-provider",
                        json!({
                            "provider": &provider_name,
                            "status": "active"
                        }),
                    );
                    return Ok(response);
                }
                Err(e) => {
                    log::warn!("{} failed: {}", provider_name, e);
                    let _ = app_handle.emit(
                        "chat-provider",
                        json!({
                            "provider": &provider_name,
                            "status": "failed"
                        }),
                    );
                    errors.push(format!("{}: {}", provider_name, e));

                    let disposition = classify_provider_error(&e);
                    if disposition == ErrorDisposition::SkipSameProvider {
                        log::warn!(
                            "{} rejected the credential; skipping remaining entries using '{}'",
                            provider_name,
                            credential_scope(entry)
                        );
                        dead_credentials.insert(credential_scope(entry));
                    }

                    // `discard` tells the frontend to throw away whatever this
                    // provider already streamed. Without it the next provider's
                    // tokens are appended to half an answer from this one -- but
                    // when no other provider is going to run, that half answer is
                    // all the user gets, so discarding it just deletes what they
                    // were reading.
                    let will_retry = ledger.count() == 0
                        && disposition != ErrorDisposition::StopChain
                        && has_live_entry_after(&self.provider_chain, idx, &dead_credentials);
                    let _ = app_handle.emit(
                        "chat-token",
                        json!({"token": "", "done": true, "discard": will_retry}),
                    );

                    // The next provider would replay this turn from the original
                    // messages, which carry no record of what already ran. If a
                    // side-effecting tool has fired, replaying it creates a second
                    // calendar event or a second draft, so stop and show the error.
                    if ledger.count() > 0 {
                        log::warn!(
                            "halting provider fallback: {} side-effecting tool call(s) already executed this turn",
                            ledger.count()
                        );
                        return Err(format!(
                            "Stopped after {} completed action(s) so they are not repeated: {}",
                            ledger.count(),
                            errors.join("; ")
                        ));
                    }

                    if disposition == ErrorDisposition::StopChain {
                        log::warn!(
                            "{} returned a terminal error; not trying further providers",
                            provider_name
                        );
                        break;
                    }

                    let _ = app_handle.emit(
                        "chat-status",
                        json!({
                            "status": "Retrying with next provider...",
                            "phase": "thinking"
                        }),
                    );
                }
            }
        }

        Err(format!(
            "All providers failed: {}",
            errors.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_key_prefers_env_then_falls_back() {
        std::env::set_var("JARVIS_TEST_KEY_A", "from-env");
        assert_eq!(
            resolve_key("JARVIS_TEST_KEY_A", None),
            Some("from-env".to_string())
        );

        // An empty env var is treated as absent so it cannot mask a saved key.
        std::env::set_var("JARVIS_TEST_KEY_B", "");
        assert_eq!(
            resolve_key("JARVIS_TEST_KEY_B", Some("from-keychain".to_string())),
            Some("from-keychain".to_string())
        );

        std::env::remove_var("JARVIS_TEST_KEY_C");
        assert_eq!(resolve_key("JARVIS_TEST_KEY_C", None), None);
        assert_eq!(
            resolve_key("JARVIS_TEST_KEY_C", Some("  ".to_string())),
            None
        );
    }

    #[test]
    fn sse_buffer_survives_multibyte_split_across_chunks() {
        // Every CJK character is three bytes, so a transport split lands inside
        // one routinely. Decoding per chunk turns it into U+FFFD; the buffer
        // must reassemble it byte-for-byte.
        let text = "總結我啲email，今日有咩未讀郵件";
        let event = format!("data: {}\n\n", text);
        let bytes = event.as_bytes();

        // "data: " is 6 ASCII bytes, so byte 7 is a continuation byte of 總.
        let split = 7;
        assert_eq!(bytes[split] & 0b1100_0000, 0b1000_0000);

        let mut buf = SseByteBuffer::new();
        buf.extend(&bytes[..split]);
        assert!(buf.next_event().is_none(), "partial event must not emit");
        buf.extend(&bytes[split..]);

        let block = buf.next_event().expect("complete event");
        assert_eq!(block, format!("data: {}", text));
        assert_eq!(block.as_bytes(), &bytes[..bytes.len() - 2]);
        assert!(!block.contains('\u{FFFD}'));
        assert!(buf.next_event().is_none());
    }

    #[test]
    fn sse_buffer_emits_events_in_order_and_handles_crlf() {
        let mut buf = SseByteBuffer::new();
        buf.extend(b"data: one\n\ndata: two\r\n\r\ndata: thr");
        assert_eq!(buf.next_event().as_deref(), Some("data: one"));
        assert_eq!(buf.next_event().as_deref(), Some("data: two"));
        assert!(buf.next_event().is_none());
        buf.extend(b"ee\n\n");
        assert_eq!(buf.next_event().as_deref(), Some("data: three"));
    }

    #[test]
    fn tool_call_key_separates_same_tool_with_different_args() {
        let mut executed: HashSet<String> = HashSet::new();
        // Three parallel read_email calls on different ids must all execute.
        assert!(executed.insert(tool_call_key("read_email", r#"{"id":"A"}"#)));
        assert!(executed.insert(tool_call_key("read_email", r#"{"id":"B"}"#)));
        assert!(executed.insert(tool_call_key("read_email", r#"{"id":"C"}"#)));
        // The pathology the guard was written for -- an identical repeat -- is
        // still suppressed.
        assert!(!executed.insert(tool_call_key("read_email", r#"{"id":"A"}"#)));
        // Different tools never collide.
        assert!(executed.insert(tool_call_key("archive_email", r#"{"id":"A"}"#)));
    }

    #[test]
    fn ledger_counts_only_side_effecting_tools() {
        let ledger = SideEffectLedger::new();
        assert_eq!(ledger.count(), 0);
        ledger.note("read_email");
        ledger.note("search_emails");
        ledger.note("list_events");
        assert_eq!(ledger.count(), 0);
        ledger.note("create_event");
        assert_eq!(ledger.count(), 1);
        ledger.note("draft_email");
        assert_eq!(ledger.count(), 2);
        // Clones share the same tally, so a provider's bumps are visible to
        // the router.
        let copy = ledger.clone();
        copy.note("archive_email");
        assert_eq!(ledger.count(), 3);
    }

    #[test]
    fn error_classifier_separates_terminal_from_retryable() {
        // Retryable: transport, rate limit, server-side.
        assert_eq!(
            classify_provider_error("Stream error: error decoding response body"),
            ErrorDisposition::Retry
        );
        assert_eq!(
            classify_provider_error("Gemini API error 429 Too Many Requests: {\"error\":{}}"),
            ErrorDisposition::Retry
        );
        assert_eq!(
            classify_provider_error("Claude API error 503 Service Unavailable: overloaded"),
            ErrorDisposition::Retry
        );
        assert_eq!(
            classify_provider_error("Local LLM API error 500 Internal Server Error: "),
            ErrorDisposition::Retry
        );
        // A missing key is provider-scoped, not request-scoped: another vendor
        // in the chain may still be configured.
        assert_eq!(
            classify_provider_error("No Gemini API key configured"),
            ErrorDisposition::Retry
        );

        // Rejected credential: every later entry of the same provider type
        // reads the same key.
        assert_eq!(
            classify_provider_error(
                "Gemini API error 400 Bad Request: {\"error\":{\"status\":\"INVALID_ARGUMENT\",\"message\":\"API_KEY_INVALID\"}}"
            ),
            ErrorDisposition::SkipSameProvider
        );
        assert_eq!(
            classify_provider_error("Claude API error 401 Unauthorized: invalid x-api-key"),
            ErrorDisposition::SkipSameProvider
        );
        assert_eq!(
            classify_provider_error("OpenAI API error 403 Forbidden: country not supported"),
            ErrorDisposition::SkipSameProvider
        );

        // The request itself cannot be accepted anywhere.
        assert_eq!(
            classify_provider_error("OpenAI API error 400 Bad Request: context_length_exceeded"),
            ErrorDisposition::StopChain
        );
        assert_eq!(
            classify_provider_error("Claude API error 413 Payload Too Large: "),
            ErrorDisposition::StopChain
        );
    }

    #[test]
    fn inference_profile_retargets_only_a_geography_prefix() {
        // The V13 volume seed is pinned to apac; a us-east-1 user must not send
        // an APAC inference profile id to a US endpoint.
        assert_eq!(
            apply_inference_profile("apac.amazon.nova-micro-v1:0", "us"),
            "us.amazon.nova-micro-v1:0"
        );
        assert_eq!(
            apply_inference_profile("us.amazon.nova-micro-v1:0", "eu"),
            "eu.amazon.nova-micro-v1:0"
        );

        // No-ops: the prefix already matches the profile.
        assert_eq!(
            apply_inference_profile("apac.amazon.nova-micro-v1:0", "apac"),
            "apac.amazon.nova-micro-v1:0"
        );
        // `global.` is geography-agnostic -- rewriting it would break the tools
        // and premium tiers, which are the two that work today.
        assert_eq!(
            apply_inference_profile("global.anthropic.claude-sonnet-4-6", "us"),
            "global.anthropic.claude-sonnet-4-6"
        );
        assert_eq!(
            apply_inference_profile("global.anthropic.claude-haiku-4-5-20251001-v1:0", "eu"),
            "global.anthropic.claude-haiku-4-5-20251001-v1:0"
        );
        // A plain model id has no geography segment to replace.
        assert_eq!(
            apply_inference_profile("anthropic.claude-3-5-sonnet-20241022-v2:0", "us"),
            "anthropic.claude-3-5-sonnet-20241022-v2:0"
        );
        assert_eq!(apply_inference_profile("nova-micro", "us"), "nova-micro");
    }

    #[test]
    fn default_chain_lets_the_workload_tier_pick_the_first_model() {
        let router = AiRouter::new("gemini_primary");
        let chain = router.provider_chain();
        assert_eq!(chain.len(), 2);
        assert!(chain[0].model_id.is_none());
        assert_eq!(
            gemini_model_for(chain[0].model_id.as_deref(), Workload::Volume),
            gemini::MODEL_FLASH
        );
        assert_eq!(
            gemini_model_for(chain[0].model_id.as_deref(), Workload::Premium),
            gemini::MODEL_PRO
        );
        // The pinned fallback keeps its explicit model, as the named presets do.
        assert_eq!(chain[1].model_id.as_deref(), Some(gemini::MODEL_PRO));
        let pro_only = AiRouter::new("gemini_pro_only");
        assert_eq!(
            gemini_model_for(pro_only.provider_chain()[0].model_id.as_deref(), Workload::Volume),
            gemini::MODEL_PRO
        );
    }

    /// Position 1 exists to survive position 0's model being unavailable. If both
    /// positions resolve to the same id with the same key, a retired model id is a
    /// total outage rather than a degraded one -- which is exactly what shipped
    /// while `gemini-3-flash-preview` was pinned in both slots.
    #[test]
    fn default_chain_fallback_is_a_different_model_than_the_volume_tier_uses() {
        let router = AiRouter::new("gemini_primary");
        let chain = router.provider_chain();
        let first = gemini_model_for(chain[0].model_id.as_deref(), Workload::Volume);
        let second = gemini_model_for(chain[1].model_id.as_deref(), Workload::Volume);
        assert_ne!(
            first, second,
            "the default chain must degrade to a different model, not retry the same one"
        );
    }

    fn entry(position: i32, provider_type: &str, endpoint_id: Option<&str>) -> ProviderChainEntry {
        ProviderChainEntry {
            position,
            provider_type: provider_type.to_string(),
            endpoint_id: endpoint_id.map(|s| s.to_string()),
            model_id: None,
            enabled: true,
        }
    }

    /// The router loop skips an entry whose `credential_scope` is already dead
    /// and marks that scope on a `SkipSameProvider` failure. Two local chain
    /// entries point at two different `LocalEndpoint`s, each carrying its own
    /// bearer token, so a 401 from the first must not disable the second.
    /// (`send` itself needs an `AppHandle`, so the loop's two decisions are
    /// asserted through the helpers it calls.)
    #[test]
    fn a_dead_local_endpoint_does_not_disable_the_other_local_endpoints() {
        let chain = vec![
            entry(0, "local", Some("endpoint-a")),
            entry(1, "local", Some("endpoint-b")),
        ];
        assert_eq!(
            classify_provider_error("Local LLM API error 401 Unauthorized: invalid api key"),
            ErrorDisposition::SkipSameProvider
        );

        let mut dead: HashSet<String> = HashSet::new();
        dead.insert(credential_scope(&chain[0]));

        assert!(dead.contains(&credential_scope(&chain[0])));
        assert!(
            !dead.contains(&credential_scope(&chain[1])),
            "endpoint-b has its own credential and must still be attempted"
        );
        assert!(
            has_live_entry_after(&chain, 0, &dead),
            "the router must fall through to the second local endpoint"
        );
    }

    /// The other half: gemini, claude, openai and bedrock all read one shared
    /// credential, so a rejected key really does kill the rest of that type.
    #[test]
    fn a_rejected_shared_key_still_kills_the_rest_of_that_provider() {
        let chain = vec![entry(0, "gemini", None), entry(1, "gemini", None)];
        let mut dead: HashSet<String> = HashSet::new();
        dead.insert(credential_scope(&chain[0]));

        assert!(dead.contains(&credential_scope(&chain[1])));
        assert!(!has_live_entry_after(&chain, 0, &dead));

        // Two entries of different types stay independent.
        let mixed = vec![entry(0, "gemini", None), entry(1, "claude", None)];
        let mut dead_mixed: HashSet<String> = HashSet::new();
        dead_mixed.insert(credential_scope(&mixed[0]));
        assert!(has_live_entry_after(&mixed, 0, &dead_mixed));
    }

    /// `discard` wipes the partial answer already on screen, which is only
    /// justified when another provider is about to overwrite it.
    #[test]
    fn no_provider_is_left_to_run_after_the_last_enabled_entry() {
        let dead: HashSet<String> = HashSet::new();
        let single = vec![entry(0, "gemini", None)];
        assert!(!has_live_entry_after(&single, 0, &dead));

        // A later entry that the user disabled is not a fallback either.
        let mut disabled_tail = vec![entry(0, "gemini", None), entry(1, "claude", None)];
        disabled_tail[1].enabled = false;
        assert!(!has_live_entry_after(&disabled_tail, 0, &dead));

        // An enabled, live entry is.
        let two = vec![entry(0, "gemini", None), entry(1, "claude", None)];
        assert!(has_live_entry_after(&two, 0, &dead));
        assert!(!has_live_entry_after(&two, 1, &dead));
    }

    /// R2-01: the Keychain lookup moved out of the row mapper, which must not
    /// change what ends up on the router -- a marker row still has to resolve to
    /// the secret, and a legacy plaintext row still has to keep working. The
    /// lock-ordering half of the fix is structural (the mapper no longer has a
    /// way to call the Keychain), so this pins the semantics it must preserve.
    #[test]
    fn load_from_db_resolves_endpoint_keys_after_releasing_the_lock() {
        use crate::security::keychain::KeychainStore;

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE local_endpoints (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 url TEXT NOT NULL,
                 backend_type TEXT NOT NULL DEFAULT 'ollama',
                 api_key TEXT,
                 use_tls INTEGER DEFAULT 0,
                 connection_timeout_ms INTEGER DEFAULT 5000,
                 keep_alive_minutes INTEGER DEFAULT 30,
                 is_active INTEGER DEFAULT 1,
                 last_health_check TEXT,
                 last_health_status INTEGER);
             CREATE TABLE provider_chain (
                 position INTEGER PRIMARY KEY,
                 provider_type TEXT NOT NULL,
                 endpoint_id TEXT,
                 model_id TEXT,
                 enabled INTEGER DEFAULT 1);
             INSERT INTO local_endpoints (id, name, url, backend_type, api_key) VALUES
                 ('r2-01-keychain', 'remote', 'https://x', 'openai_compat', '__keychain__'),
                 ('r2-01-legacy', 'legacy', 'https://y', 'openai_compat', 'sk-plaintext'),
                 ('r2-01-none', 'ollama', 'http://z', 'ollama', NULL);",
        )
        .unwrap();
        let db = crate::db::Database {
            conn: std::sync::Mutex::new(conn),
        };
        KeychainStore::put(
            KEY_SERVICE,
            "local_endpoint_api_key:r2-01-keychain",
            "sk-from-keychain",
        )
        .unwrap();

        let mut router = AiRouter::new("gemini_primary");
        router.load_from_db(&db);

        let key_for = |id: &str| {
            router
                .local_endpoints()
                .iter()
                .find(|e| e.id == id)
                .and_then(|e| e.api_key.clone())
        };
        assert_eq!(key_for("r2-01-keychain").as_deref(), Some("sk-from-keychain"));
        assert_eq!(key_for("r2-01-legacy").as_deref(), Some("sk-plaintext"));
        assert_eq!(key_for("r2-01-none"), None);

        // Nothing is still holding the connection once the call returns.
        assert!(db.conn.try_lock().is_ok());

        KeychainStore::delete(KEY_SERVICE, "local_endpoint_api_key:r2-01-keychain").ok();
    }
}
