pub mod bedrock;
pub mod capability;
pub mod consent;
pub mod failure;
pub mod notify;
pub mod claude;
pub mod compat;
pub mod mail_policy;
pub mod models;
pub mod complete;
pub mod cost;
pub mod disabled;
pub mod usage;
pub mod gemini;
pub mod local;
pub mod openai;
pub(crate) mod openai_responses;
pub mod persona;
pub(crate) mod provider;
pub mod registry;
pub mod routing;
pub mod tools;
#[cfg(test)]
pub(crate) mod wire_tests;
pub use routing::{classify_workload, Workload};

use crate::voice::tts::TtsCommand;
use local::backend::{BackendType, LocalEndpoint};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use crate::chat_session::ScopedEmitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderChainEntry {
    pub position: i32,
    pub provider_type: String, // "gemini", "claude", "openai", "bedrock", "local"
    pub endpoint_id: Option<String>,
    pub model_id: Option<String>,
    pub enabled: bool,
    /// Only for `openai` rows: an OpenAI-compatible service such as Kimi or
    /// Qwen. None is api.openai.com.
    #[serde(default)]
    pub base_url: Option<String>,
}

/// Gemini model selection: a user-configured provider-chain override wins;
/// otherwise pick by workload tier. Used by both provider_name display and
/// dispatch so the emitted label never diverges from the model actually sent.
/// The tiers are the registry's `premium` and `chat` defaults for Gemini.
fn gemini_model_for(model_override: Option<&str>, workload: Workload) -> String {
    match (model_override, workload) {
        (Some(model), _) => model.to_string(),
        (None, Workload::Premium) => crate::ai::models::gemini_pro(),
        (None, _) => crate::ai::models::gemini_flash(),
    }
}

/// Retarget a Bedrock model id at the geography the credentials were saved for.
///
/// A saved model id can carry a fixed geography (the V13 seed used
/// `apac.amazon.nova-micro-v1:0`), so a user on `us-east-1` would otherwise
/// send an APAC inference profile to a US endpoint and get a
/// ValidationException on every request. Ids prefixed `global.`, and ids
/// with no geography prefix at all, are left untouched.
///
/// The geography prefixes are the registry's `inference_geographies` for
/// Bedrock. `global.` is deliberately absent: it is geography-agnostic and
/// must not be rewritten.
pub(crate) fn apply_inference_profile(model: &str, profile: &str) -> String {
    let registry = registry::current();
    let geographies = registry.provider("bedrock").map(|p| p.inference_geographies.as_slice()).unwrap_or_default();
    match model.split_once('.') {
        Some((head, rest)) if geographies.iter().any(|g| g == head) && head != profile => {
            format!("{}.{}", profile, rest)
        }
        _ => model.to_string(),
    }
}

/// The Bedrock model a chain row runs: the one saved with it, else the
/// default. Chat and plain completions both use this.
fn bedrock_model(entry: &ProviderChainEntry) -> String {
    entry.model_id.clone().filter(|m| !m.trim().is_empty()).unwrap_or_else(bedrock::default_model)
}

/// The Bedrock model id typed in Settings, as it is saved: trimmed, and None
/// (the default model) when empty. Both forms the Bedrock console shows are
/// kept as typed: a model id (`anthropic.claude-haiku-4-5-20251001-v1:0`) or
/// an inference profile id (`us.`, `eu.`, `apac.` or `global.` in front).
/// When a request is sent, a `us.`, `eu.` or `apac.` prefix is pointed at
/// the saved region (`apply_inference_profile`), as for every saved id.
pub(crate) fn bedrock_model_to_save(typed: Option<&str>, language: bedrock::UiLanguage) -> Result<Option<String>, String> {
    let Some(model) = typed.map(str::trim).filter(|m| !m.is_empty()) else {
        return Ok(None);
    };
    // AWS takes up to 2,048 characters, none of them spaces.
    if model.len() > 2048 || model.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(language.pick(
            "This is not a Bedrock model ID. Copy the model ID or inference profile ID from the Bedrock console.",
            "這不是 Bedrock 模型 ID，請從 Bedrock 主控台複製模型 ID 或推論設定檔 ID。",
        ));
    }
    Ok(Some(model.to_string()))
}

/// Keychain service holding the provider API keys the user pastes in
/// onboarding or Settings.
pub const KEY_SERVICE: &str = "com.jarvis.ai";

pub const NO_AI_CHOSEN: &str = "尚未選擇 AI。請按上方「設定導引」，選擇你使用的 AI（例如 Ollama、OpenAI、Gemini、Claude、Kimi 或 Qwen）。";

/// Credential lookup order: process env first (dev workflow and
/// `~/Library/Application Support/jarvis/.env`), then the Keychain. An empty
/// env var counts as absent -- otherwise a stray `GEMINI_API_KEY=` in a
/// dotfile would mask a key the user actually saved.
fn resolve_key(env_var: &str, fallback: Option<String>) -> Option<String> {
    resolve_key_with(env_var, || fallback)
}

/// Like `resolve_key`, but only asks the fallback when the environment has no
/// key. The Keychain read behind it can show a password prompt and runs on the
/// main thread at startup, so an eager read both asked for a secret `.env`
/// already supplied and kept the window from opening until it was answered.
fn resolve_key_with(env_var: &str, fallback: impl FnOnce() -> Option<String>) -> Option<String> {
    match std::env::var(env_var) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => fallback().filter(|v| !v.trim().is_empty()),
    }
}

/// A chat turn's request, the same for every provider.
fn chat_request(db: &crate::db::Database, messages: Vec<(String, String)>, deeper: Option<capability::Deeper>) -> provider::ChatRequest {
    provider::ChatRequest { system: provider::SystemPrompt::for_request(db), messages, tools: tools::get_tool_definitions(), deeper }
}

/// The address a provider's own API is reached at: `official`, except in a
/// test, which points it at a loopback server (`wire_tests::serve`) and is
/// refused the provider's own address (`wire_tests::refuse_outside`).
pub(crate) fn api_base(official: &str) -> String {
    #[cfg(test)]
    {
        if let Some(base) = wire_tests::API_BASE.with(|base| base.borrow().clone()) {
            return base;
        }
        let wire = if official.contains("openai.com") { "OpenAI" } else if official.contains("anthropic.com") { "Claude" } else if official.contains("googleapis.com") { "Gemini" } else { "A provider" };
        wire_tests::refuse_outside(wire, official);
    }
    official.to_string()
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
/// A resend, or a "send again" picked in the error card, replays the whole
/// turn from the original messages, so anything in this list would be
/// carried out twice: neither happens once one ran (P06). Every other tool
/// is read-only (`tests::every_tool_is_listed_as_an_action_or_as_read_only`).
const SIDE_EFFECTING_TOOLS: &[&str] = &[
    // On the Mac: an app or a file opened twice, a second notification.
    "open_app",
    "open_file",
    "send_notification",
    "create_event",
    "update_event",
    "delete_event",
    "draft_email",
    "archive_email",
    "create_github_issue",
    "create_notion_page",
    "create_task",
    // Local only, but a fallback replay would still flip it twice.
    "update_follow_up",
    "remember",
    "forget",
];

/// Tools that pull text into the model's context that nobody on this device
/// wrote. Whatever a stranger put in an email, a shared Notion page or a synced
/// note arrives through one of these.
const UNTRUSTED_READ_TOOLS: &[&str] = &[
    "read_email",
    "search_emails",
    "read_note",
    "search_notes",
    "read_notion_page",
    "search_notion",
    // An invitation's title, description and location are written by whoever
    // sent it, and Google drops invitations straight onto the primary calendar
    // `list_events` reads. The `update_event` guard already treats a shared
    // event as attacker-reachable; reading one is the same text.
    "list_events",
    // Issue and PR titles are written by whoever opened them.
    "list_github_items",
    // JARVIS's synced mail data: titles, summaries and evidence are derived
    // from email a stranger wrote.
    "list_follow_ups",
    "list_inbox_triage",
];

/// Per-turn tally of side-effecting tool calls, shared between the router and
/// whichever provider is currently running. Providers bump it as they execute;
/// the router reads it to decide whether a fallback would duplicate work.
///
/// It also records whether the turn has read untrusted content, after which
/// `ai::tools` asks the user before running the tools in
/// `tool_approval::NEEDS_APPROVAL_AFTER_UNTRUSTED_READ`: injected text can ask
/// for exactly those calls.
#[derive(Clone, Default)]
pub struct SideEffectLedger {
    side_effects: Arc<AtomicUsize>,
    untrusted_read: Arc<AtomicBool>,
    /// Why a tool call was refused Gmail or Calendar content this turn
    /// (P06, Google Limited Use), if one was.
    google_content_refused: Arc<std::sync::Mutex<Option<String>>>,
}

impl SideEffectLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `tool_name` if it mutates state outside JARVIS, or if it brings
    /// content from outside this device into the turn.
    pub fn note(&self, tool_name: &str) {
        if SIDE_EFFECTING_TOOLS.contains(&tool_name) {
            self.side_effects.fetch_add(1, Ordering::Relaxed);
        }
        if UNTRUSTED_READ_TOOLS.contains(&tool_name) {
            self.untrusted_read.store(true, Ordering::Relaxed);
        }
    }

    /// A ledger for a turn that starts out tainted, because its prompt already
    /// carries text authored off this device -- mirrored email subjects,
    /// calendar titles, Obsidian note bodies pasted in as context rather than
    /// fetched by a tool call. Nothing calls `note` on those paths, so without
    /// this the turn looks clean and the tools that wait for a click after an
    /// untrusted read would run without one.
    pub fn untrusted() -> Self {
        let ledger = Self::default();
        ledger.untrusted_read.store(true, Ordering::Relaxed);
        ledger
    }

    pub fn count(&self) -> usize {
        self.side_effects.load(Ordering::Relaxed)
    }

    /// True once this turn has read content authored off this device.
    pub fn saw_untrusted_read(&self) -> bool {
        self.untrusted_read.load(Ordering::Relaxed)
    }

    /// A tool call needed Gmail or Calendar content the model may not
    /// receive; `message` says why. The turn ends on the error card
    /// ([`run_chosen`]).
    pub fn refuse_google_content(&self, message: String) {
        if let Ok(mut refused) = self.google_content_refused.lock() {
            refused.get_or_insert(message);
        }
    }

    pub fn google_content_refused(&self) -> Option<String> {
        self.google_content_refused.lock().ok().and_then(|refused| refused.clone())
    }
}

/// The model a request runs on (P06): the first enabled row of the chain the
/// user saved. The other rows are never tried in its place; they are offered
/// in the error card. For the built-in chain nobody saved, see
/// [`unsaved_chain_choice`]. None: no AI chosen.
pub(crate) fn chosen_entry(chain: &[ProviderChainEntry], saved: bool, configured: &[ProviderChainEntry]) -> Option<ProviderChainEntry> {
    if saved {
        return chain.iter().find(|entry| entry.enabled).cloned();
    }
    unsaved_chain_choice(chain, configured)
}

/// Which of the router's own keys exist (never the keys themselves), as
/// the live router last published them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct KeysFound {
    pub gemini: bool,
    pub claude: bool,
    pub openai: bool,
}

static KEYS_FOUND: std::sync::RwLock<Option<KeysFound>> = std::sync::RwLock::new(None);

/// The keys the live router has, as last published. The mail gate uses
/// them to work out which model a chain nobody saved runs on
/// ([`unsaved_choice`]) without a Keychain read of its own. None until the
/// launch publishes them.
pub(crate) fn keys_found() -> Option<KeysFound> {
    #[cfg(test)]
    return keys_found::get();
    #[cfg(not(test))]
    process_wide_keys_found()
}

/// What release builds read.
fn process_wide_keys_found() -> Option<KeysFound> {
    KEYS_FOUND.read().ok().and_then(|keys| *keys)
}

/// Written only for the live router ([`AiRouter::publish_keys`],
/// [`install_live`]): a router built for one job reads the Keychain for
/// itself, and a read that failed for it alone must not change what the
/// gate judges (review B, r4-m3).
fn publish_keys_found(keys: KeysFound) {
    if let Ok(mut found) = KEYS_FOUND.write() {
        *found = Some(keys);
    }
    #[cfg(test)]
    keys_found::publish(keys);
}

/// Make `next` the live router, the one the chat and the jobs started from
/// now on clone, and publish its keys, both under the router's lock.
pub(crate) fn install_live(live: &std::sync::Mutex<AiRouter>, next: AiRouter) -> Result<(), String> {
    let mut router = live.lock().map_err(|e| e.to_string())?;
    next.publish_keys();
    *router = next;
    Ok(())
}

/// Tests see the keys their own thread published, so tests running side by
/// side do not see each other's; or the keys a test sets.
#[cfg(test)]
pub(crate) mod keys_found {
    use super::KeysFound;
    use std::cell::Cell;

    /// Held by a test that publishes, so the process-wide copy one of them
    /// checks is not written by another meanwhile.
    pub(crate) static PUBLISHING: std::sync::Mutex<()> = std::sync::Mutex::new(());

    thread_local! {
        static PUBLISHED: Cell<Option<KeysFound>> = const { Cell::new(None) };
        static OVERRIDE: Cell<Option<Option<KeysFound>>> = const { Cell::new(None) };
    }

    pub(crate) fn get() -> Option<KeysFound> {
        OVERRIDE.with(Cell::get).unwrap_or_else(|| PUBLISHED.with(Cell::get))
    }

    pub(super) fn publish(keys: KeysFound) {
        PUBLISHED.with(|p| p.set(Some(keys)));
    }

    pub(crate) struct Guard(Option<Option<KeysFound>>);

    impl Drop for Guard {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }

    pub(crate) fn with(keys: KeysFound) -> Guard {
        Guard(OVERRIDE.with(|o| o.replace(Some(Some(keys)))))
    }

    /// As before the first Keychain read.
    pub(crate) fn without() -> Guard {
        Guard(OVERRIDE.with(|o| o.replace(Some(None))))
    }
}

/// The chain a request runs on when nobody saved one: the preset's rows,
/// then Claude and then OpenAI when their keys exist and the preset has no
/// row for them. Worked out when it is used, from the registry in force, so
/// the router and the mail gate agree.
pub(crate) fn unsaved_chain(preset: &str, keys: KeysFound) -> Vec<ProviderChainEntry> {
    let mut chain = preset_chain(preset);
    for (provider_type, keyed) in [("claude", keys.claude), ("openai", keys.openai)] {
        if keyed && !chain.iter().any(|entry| entry.provider_type == provider_type) {
            chain.push(ProviderChainEntry { position: chain.len() as i32, provider_type: provider_type.into(), endpoint_id: None, model_id: None, enabled: true, base_url: None });
        }
    }
    chain
}

/// The model a request with no pick runs on when nobody saved a chain, by
/// the rule in [`unsaved_chain_choice`]. The mail gate asks this
/// (`mail_policy::check`); the router decides with the same two functions
/// (`AiRouter::chosen`). `bedrock`: AWS credentials are saved.
pub(crate) fn unsaved_choice(preset: &str, keys: KeysFound, bedrock: bool) -> Option<ProviderChainEntry> {
    let keyed = [("gemini", keys.gemini), ("claude", keys.claude), ("openai", keys.openai), ("bedrock", bedrock)];
    let configured: Vec<ProviderChainEntry> = keyed
        .into_iter()
        .filter(|(_, keyed)| *keyed)
        .map(|(provider_type, _)| ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id: None, model_id: None, enabled: true, base_url: None })
        .collect();
    unsaved_chain_choice(&unsaved_chain(preset, keys), &configured)
}

/// Whether AWS credentials are saved, as the router counts Bedrock keyed.
pub(crate) fn bedrock_saved(db: &crate::db::Database) -> bool {
    db.conn.lock().ok().is_some_and(|conn| conn.query_row("SELECT 1 FROM aws_credentials WHERE id = 1", [], |_| Ok(())).is_ok())
}

/// The choice for a chain nobody saved (DECISIONS, P06 "Failure handling",
/// point 2): the first enabled row that has credentials counts as the
/// user's choice, since nobody chose and there is nothing to switch away
/// from. These are installs with no provider_chain rows: betas from before
/// setup asked for an AI, where Settings → Provider priority was never
/// saved, and new installs with a key but setup's AI step not finished.
fn unsaved_chain_choice(chain: &[ProviderChainEntry], configured: &[ProviderChainEntry]) -> Option<ProviderChainEntry> {
    let has_credentials = |entry: &&ProviderChainEntry| configured.iter().any(|c| c.provider_type == entry.provider_type && c.base_url == entry.base_url && c.endpoint_id == entry.endpoint_id);
    chain.iter().filter(|entry| entry.enabled).find(has_credentials).cloned()
}

/// The wait before the one resend to the same model after a transient
/// failure.
#[cfg(not(test))]
const RESEND_DELAY: std::time::Duration = std::time::Duration::from_secs(1);
#[cfg(test)]
const RESEND_DELAY: std::time::Duration = std::time::Duration::from_millis(0);

/// Whether the chat wire `row` is sent on resends a 429 or 5xx once itself
/// ([`provider::Wire::resends_transient`]): the OpenAI wire does for
/// api.openai.com and the compatible services, and not for a local server
/// (`AiRouter::send_one` sends "openai" rows to it, "local" rows to its
/// local mode). No other wire resends.
pub(crate) fn resends_inside(row: &ProviderChainEntry) -> bool {
    row.provider_type == "openai"
}

/// One request on the chosen model, with no other model tried in its place
/// (DECISIONS.md section 5, "Failure handling"). Before anything is sent the
/// model must support the feature (`capability::resolve`). A transient
/// failure (rate limit, server error, no connection, an empty reply) is sent
/// once more to the same model, unless an action already ran. Otherwise the
/// request fails with the error card's data, or, for a background job, the
/// sentence its run records ([`failure::Failure::surface`]).
///
/// `send` sends to one row; the router and `complete` pass the provider
/// calls, the tests pass their own. `emit` gets the HUD events.
/// `resends_inside`: whether the wire a row goes to already sent a 429 or
/// 5xx once more itself ([`provider::Wire::resends_transient`]), so this does
/// not send it again.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_chosen<T, F, Fut>(
    chosen: Option<ProviderChainEntry>,
    picked: Option<capability::Pick>,
    configured: &[ProviderChainEntry],
    feature: cost::Feature,
    workload: Workload,
    ledger: &SideEffectLedger,
    language: bedrock::UiLanguage,
    resends_inside: &(dyn Fn(&ProviderChainEntry) -> bool + Sync),
    may_receive_google_content: &(dyn Fn(&ProviderChainEntry) -> bool + Sync),
    mut emit: impl FnMut(&str, serde_json::Value),
    mut send: F,
) -> Result<T, String>
where
    F: FnMut(ProviderChainEntry) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let key = capability::feature_key(feature, workload);
    // A model picked in the error card runs this one request, and only if
    // the card could have offered it: the frontend cannot name a model or
    // an address of its own.
    let chosen = match picked {
        Some(pick) if capability::is_offered(key, &pick, configured) => Some(pick.entry()),
        Some(_) => return Err(language.pick("That model cannot be used for this request. Choose another one.", "這個模型不能用於這個請求，請另選模型。")),
        None => chosen,
    };
    let Some(entry) = chosen else { return Err(NO_AI_CHOSEN.into()) };
    let foreground = disabled::is_foreground(feature);
    let target = match capability::resolve(key, &entry, configured) {
        capability::Resolved::Ready(target) => target,
        // A background job on a disabled model keeps P05's notice, which
        // saves the replacement once the user confirms it.
        capability::Resolved::Unsupported(capability::Unsupported { reason: capability::Reason::Disabled { replacement }, .. }) if !foreground => {
            let provider = mail_policy::registry_provider(&entry).unwrap_or_default().to_string();
            let model = entry.model_id.clone().unwrap_or_default();
            return Err(disabled::stop(disabled::Disabled { provider, model, replacement }, false, language));
        }
        capability::Resolved::Unsupported(unsupported) => return Err(failure::Failure::unsupported(key, unsupported, language).surface(foreground, language)),
    };
    // The model resolved is the one sent, so the card and the usage row name
    // what ran. Bedrock without a saved model keeps bedrock::default_model.
    let mut row = entry.clone();
    if !(row.provider_type == "bedrock" && row.model_id.is_none()) {
        row.model_id = target.model.clone();
    }
    let label = target.label.clone();
    emit("chat-provider", json!({"provider": &label, "status": "trying"}));
    let mut resent = false;
    loop {
        match send(row.clone()).await {
            Ok(response) => {
                emit("chat-provider", json!({"provider": &label, "status": "active"}));
                // Sending this run again (the consent card's "Send again")
                // would carry its actions out twice.
                if ledger.count() > 0 {
                    failure::note_current_run_acted();
                }
                // A tool call was refused Gmail or Calendar content (P06,
                // Google Limited Use): the turn ends on the card, which
                // offers the models that may receive it, rather than on an
                // answer given without it.
                if let Some(message) = ledger.google_content_refused() {
                    emit("chat-token", json!({"token": "", "done": true, "discard": true}));
                    let alternatives = capability::offered_for(key, configured).into_iter().filter(|a| may_receive_google_content(&a.pick.entry())).collect();
                    return Err(failure::Failure::google_content(key, target, message, ledger.count(), alternatives).surface(foreground, language));
                }
                return Ok(response);
            }
            Err(error) => {
                log::warn!("{}", failure::log_line(&label, &error));
                let code = failure::code_of(&error);
                let resent_inside = resends_inside(&row) && matches!(code, failure::Code::RateLimited | failure::Code::ServerError);
                let again = !resent && code.transient() && !resent_inside && ledger.count() == 0;
                // `discard` drops what this attempt streamed, only when the
                // resend is about to replace it.
                emit("chat-token", json!({"token": "", "done": true, "discard": again}));
                if again {
                    resent = true;
                    emit("chat-status", json!({"status": "Trying the same model again...", "phase": "thinking"}));
                    tokio::time::sleep(RESEND_DELAY).await;
                    continue;
                }
                emit("chat-provider", json!({"provider": &label, "status": "failed"}));
                if ledger.count() > 0 {
                    log::warn!("{} action(s) already ran this turn; the request is not offered again", ledger.count());
                    failure::note_current_run_acted();
                }
                let alternatives = capability::offered_for(key, configured);
                return Err(failure::Failure::failed(key, target, &error, ledger.count(), alternatives, language).surface(foreground, language));
            }
        }
    }
}

// Briefing and other non-chat callers propagate router errors directly. Finish
// the HUD here on every terminal error and every response without streaming
// speech; successful streaming speech keeps ownership until its audio finishes.
async fn finish_request_with_hud<T>(
    request: impl std::future::Future<Output = Result<T, String>>,
    streaming_tts: bool,
    mut emit: impl FnMut(&str, serde_json::Value),
) -> Result<T, String> {
    let result = request.await;
    if result.is_err() || !streaming_tts {
        emit("chat-state", json!({"state": "idle"}));
    }
    result
}

#[derive(Clone)]
pub struct AiRouter {
    gemini_key: Option<String>,
    claude_key: Option<String>,
    openai_key: Option<String>,
    provider_chain: Vec<ProviderChainEntry>,
    local_endpoints: Vec<LocalEndpoint>,
    /// True once `load_from_db` has replaced `provider_chain` with rows the
    /// user actually persisted. `append_default_fallbacks` only touches the
    /// built-in default, so a chain the user deliberately edited (even down
    /// to zero rows re-added by hand) is never appended to behind their back.
    chain_loaded_from_db: bool,
    /// The preset the router was built from (`ai_provider`): a chain nobody
    /// saved is worked out from it ([`unsaved_chain`]).
    preset: String,
    /// Keys for OpenAI-compatible services by host, for a router built to
    /// test a choice before it is saved. Otherwise the Keychain is read when
    /// the service is used.
    compat_keys: HashMap<String, String>,
}

/// Endpoints the router may send prompts, which can carry email, and keys to:
/// https, or plain http only to this Mac. An endpoint saved with plain http to
/// another machine is skipped rather than sent in cleartext across a network.
fn only_secure_endpoints(endpoints: Vec<LocalEndpoint>) -> Vec<LocalEndpoint> {
    endpoints
        .into_iter()
        .filter(|endpoint| {
            let secure = compat::normalize_base_url(&endpoint.url).is_ok();
            if !secure {
                log::warn!("AI endpoint {} skipped: not https and not on this Mac", endpoint.id);
            }
            secure
        })
        .collect()
}

/// The provider chain a preset names (Settings, `ai_provider`), from the
/// registry; an unknown name is the registry's default preset. In the
/// seeded registry the default leaves position 0 unpinned so the workload
/// tier picks the model (Pro for premium work, Flash otherwise) and pins
/// position 1 to Pro, so a Flash outage degrades to a *different* model.
pub(crate) fn preset_chain(name: &str) -> Vec<ProviderChainEntry> {
    let registry = registry::current();
    let preset = registry.preset(name).or_else(|| registry.preset(&registry.presets.default));
    preset
        .map(|preset| preset.entries.as_slice())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(position, entry)| ProviderChainEntry {
            position: position as i32,
            provider_type: entry.provider.clone(),
            endpoint_id: None,
            model_id: entry.model.clone(),
            enabled: true,
            base_url: None,
        })
        .collect()
}

impl AiRouter {
    pub fn new(provider_setting: &str) -> Self {
        // Keys start env-only; `reload_keys` layers the Keychain in
        // afterwards, because construction has no database handle.
        let gemini_key = resolve_key("GEMINI_API_KEY", None);
        let claude_key = resolve_key("ANTHROPIC_API_KEY", None);
        let openai_key = resolve_key("OPENAI_API_KEY", None);
        let chain = preset_chain(provider_setting);

        AiRouter {
            gemini_key,
            claude_key,
            openai_key,
            provider_chain: chain,
            local_endpoints: Vec::new(),
            chain_loaded_from_db: false,
            preset: provider_setting.to_string(),
            compat_keys: HashMap::new(),
        }
    }

    /// The router with no key of its own, whatever the environment holds:
    /// a request a test lets through then fails on this Mac.
    #[cfg(test)]
    pub(crate) fn without_keys(mut self) -> Self {
        self.gemini_key = None;
        self.claude_key = None;
        self.openai_key = None;
        self
    }

    /// Which of the router's own keys exist.
    pub(crate) fn keys_found(&self) -> KeysFound {
        KeysFound { gemini: self.gemini_key.is_some(), claude: self.claude_key.is_some(), openai: self.openai_key.is_some() }
    }

    /// The model a request with no pick runs on (P06): the first enabled row
    /// of a saved chain, or for a chain nobody saved the one
    /// [`unsaved_chain`] and the keys give now. None: no AI chosen.
    pub(crate) fn chosen(&self, configured: &[ProviderChainEntry]) -> Option<ProviderChainEntry> {
        if self.chain_loaded_from_db {
            return chosen_entry(&self.provider_chain, true, configured);
        }
        chosen_entry(&unsaved_chain(&self.preset, self.keys_found()), false, configured)
    }

    /// [`chosen`](Self::chosen) as a request would take it now. The
    /// configured rows are looked up (a Keychain read for each compatible
    /// service) only for a chain nobody saved, the one case they decide.
    pub(crate) async fn chosen_now(&self, db: &crate::db::Database) -> Option<ProviderChainEntry> {
        if self.chain_loaded_from_db {
            return self.chosen(&[]);
        }
        self.chosen(&self.configured(db).await)
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
                    "SELECT position, provider_type, endpoint_id, model_id, enabled, base_url \
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
                            base_url: row.get(5)?,
                        })
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();

                if !chain.is_empty() {
                    self.provider_chain = chain;
                    self.chain_loaded_from_db = true;
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
            self.local_endpoints = only_secure_endpoints(endpoints);
        }
    }

    /// Layer Keychain-stored provider keys under the env vars. Called at
    /// startup and again whenever a key is saved in Settings, so a freshly
    /// pasted key takes effect without restarting the app.
    pub fn reload_keys(&mut self) {
        // A locked keychain, or an access prompt nobody answered, fails here
        // exactly like an unsaved key, so the user gets "No API key configured"
        // for a key they can see in Keychain Access. Say so in the log. The
        // account name only -- never the value.
        let from_keychain = |account: &str| {
            match crate::security::keychain::KeychainStore::get(KEY_SERVICE, account) {
                Ok(value) => Some(value),
                Err(e) => {
                    if !crate::security::keychain::KeychainStore::is_not_found(&e) {
                        log::warn!("Keychain read failed for {account}: {e}");
                    }
                    None
                }
            }
        };
        self.gemini_key = resolve_key_with("GEMINI_API_KEY", || from_keychain("gemini_api_key"));
        self.claude_key = resolve_key_with("ANTHROPIC_API_KEY", || from_keychain("claude_api_key"));
        self.openai_key = resolve_key_with("OPENAI_API_KEY", || from_keychain("openai_api_key"));
    }

    /// Publish which of this router's keys exist, for the mail gate
    /// ([`keys_found`]). For the live router only: at launch, and when a
    /// key is saved or removed in Settings; a new live router publishes
    /// through [`install_live`].
    pub(crate) fn publish_keys(&self) {
        publish_keys_found(self.keys_found());
    }

    /// Add Claude and/or OpenAI to the built-in default chain when the user
    /// has a key for them but has never persisted a provider chain of their
    /// own. Onboarding and Settings both promise "JARVIS uses whichever is
    /// available, falling back automatically", but every default chain is
    /// Gemini-only -- a tester who only pastes an OpenAI or Claude key gets
    /// "No Gemini API key configured" on every message instead of the
    /// fallback the copy describes. Only ever touches the built-in default
    /// (`chain_loaded_from_db == false`); a chain the user persisted, even one
    /// with zero rows, is left exactly as they made it. Safe to call more than
    /// once -- an entry already present for a provider is never duplicated.
    pub fn append_default_fallbacks(&mut self) {
        if self.chain_loaded_from_db {
            return;
        }
        // Rebuilt rather than appended to, so the rows come in the same
        // order whichever key was added first ([`unsaved_chain`]).
        self.provider_chain = unsaved_chain(&self.preset, self.keys_found());
    }

    pub fn provider_chain(&self) -> &[ProviderChainEntry] {
        &self.provider_chain
    }

    pub fn local_endpoints(&self) -> &[LocalEndpoint] {
        &self.local_endpoints
    }

    /// Replace the chain with the one the user just saved in Settings. That
    /// save has already written the rows, so this chain is a persisted one and
    /// `append_default_fallbacks` has to leave it alone from here. Without the
    /// flag the very next API key saved in Settings re-added the claude and
    /// openai entries the user had just deleted, and the live router diverged
    /// from both the database and the Settings screen until the next relaunch
    /// -- on a chain deliberately trimmed to a local endpoint, that put the
    /// next failed message on OpenAI.
    pub fn set_provider_chain(&mut self, chain: Vec<ProviderChainEntry>) {
        self.provider_chain = chain;
        self.chain_loaded_from_db = true;
    }

    pub fn set_compat_key(&mut self, base_url: &str, key: &str) {
        self.compat_keys.insert(compat::host(base_url), key.trim().to_string());
    }

    /// Key for an OpenAI-compatible service: one given to this router, else
    /// the Keychain (off the async threads; it can block on a prompt).
    pub(crate) async fn compat_key(&self, base_url: &str) -> Option<String> {
        if let Some(key) = self.compat_keys.get(&compat::host(base_url)) {
            return Some(key.clone());
        }
        let base = base_url.to_string();
        tokio::task::spawn_blocking(move || compat::resolve_key(&base)).await.ok().flatten()
    }

    pub fn set_gemini_key(&mut self, key: &str) {
        self.gemini_key = Some(key.trim().to_string());
    }

    pub fn set_claude_key(&mut self, key: &str) {
        self.claude_key = Some(key.trim().to_string());
    }

    pub fn set_openai_key(&mut self, key: &str) {
        self.openai_key = Some(key.trim().to_string());
    }

    pub fn set_local_endpoints(&mut self, endpoints: Vec<LocalEndpoint>) {
        self.local_endpoints = only_secure_endpoints(endpoints);
    }

    pub fn find_endpoint(&self, endpoint_id: &str) -> Option<&LocalEndpoint> {
        self.local_endpoints.iter().find(|e| e.id == endpoint_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send(
        &self,
        messages: Vec<(String, String)>,
        db: &crate::db::Database,
        google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
        app_handle: &tauri::AppHandle,
        tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
        workload: Workload,
        feature: cost::Feature,
    ) -> Result<String, String> {
        self.send_with_ledger(
            messages,
            db,
            google_auth,
            app_handle,
            tts_tx,
            workload,
            feature,
            SideEffectLedger::new(),
        )
        .await
    }

    /// `send` for a turn whose prompt already carries text nobody on this
    /// device wrote: the briefing pastes unread senders and subjects in,
    /// `ask_jarvis` pastes the same block plus today's event titles, and the
    /// chat [CONTEXT] and [OBSIDIAN NOTES] blocks do the same. None of that
    /// arrives through a tool call, so the ledger would otherwise report a
    /// clean turn and a stranger's subject line could move a meeting or wipe
    /// a memory without the user's click.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_with_untrusted_context(
        &self,
        messages: Vec<(String, String)>,
        db: &crate::db::Database,
        google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
        app_handle: &tauri::AppHandle,
        tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
        workload: Workload,
        feature: cost::Feature,
    ) -> Result<String, String> {
        self.send_with_ledger(
            messages,
            db,
            google_auth,
            app_handle,
            tts_tx,
            workload,
            feature,
            SideEffectLedger::untrusted(),
        )
        .await
    }

    async fn send_with_ledger(
        &self,
        messages: Vec<(String, String)>,
        db: &crate::db::Database,
        google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
        app_handle: &tauri::AppHandle,
        tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
        workload: Workload,
        feature: cost::Feature,
        ledger: SideEffectLedger,
    ) -> Result<String, String> {
        let streaming_tts = tts_tx.is_some();
        // The providers record usage deep in their loops; this is how the
        // row learns what the request was for.
        cost::with_feature(
            feature,
            finish_request_with_hud(
                self.run_provider_chain(
                    messages,
                    db,
                    google_auth,
                    app_handle,
                    tts_tx,
                    workload,
                    feature,
                    ledger,
                ),
                streaming_tts,
                |event, payload| {
                    let _ = app_handle.emit(event, payload);
                },
            ),
        )
        .await
    }

    /// The rows the user has credentials for: what the error card offers,
    /// and what a model picked in it is checked against. Each service with a
    /// key, on the registry's default for the feature, and every saved row
    /// whose credential is there (a compatible service or a local model is
    /// offered only on the model the user saved for it).
    pub(crate) async fn configured(&self, db: &crate::db::Database) -> Vec<ProviderChainEntry> {
        let bedrock = bedrock_saved(db);
        let keyed = |provider_type: &str| match provider_type {
            "gemini" => self.gemini_key.is_some(),
            "claude" => self.claude_key.is_some(),
            "openai" => self.openai_key.is_some(),
            "bedrock" => bedrock,
            _ => false,
        };
        let mut rows: Vec<ProviderChainEntry> = ["gemini", "claude", "openai", "bedrock"]
            .into_iter()
            .filter(|provider_type| keyed(provider_type))
            .map(|provider_type| ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id: None, model_id: None, enabled: true, base_url: None })
            .collect();
        for entry in &self.provider_chain {
            let present = match (entry.provider_type.as_str(), entry.base_url.as_deref()) {
                ("openai", Some(base)) => self.compat_key(base).await.is_some(),
                ("local", _) => entry.endpoint_id.as_deref().is_some_and(|id| self.find_endpoint(id).is_some()),
                (provider_type, _) => keyed(provider_type),
            };
            if present {
                rows.push(ProviderChainEntry { enabled: true, ..entry.clone() });
            }
        }
        rows
    }

    /// The error card for a request the user waits on that needs Gmail or
    /// Calendar content the model it would run on may not receive (P06,
    /// Google Limited Use): the model, why (`message`), and the configured
    /// models that may receive the content. Nothing is sent.
    pub(crate) async fn google_content_card(&self, db: &crate::db::Database, feature: cost::Feature, message: String) -> String {
        let configured = self.configured(db).await;
        let key = capability::feature_key(feature, Workload::Volume);
        let entry = capability::picked().map(|pick| pick.entry()).or_else(|| self.chosen(&configured));
        let Some(entry) = entry else { return message };
        let target = match capability::resolve(key, &entry, &configured) {
            capability::Resolved::Ready(target) => target,
            capability::Resolved::Unsupported(unsupported) => unsupported.target,
        };
        let alternatives = capability::offered_for(key, &configured).into_iter().filter(|a| mail_policy::may_receive(db, &a.pick.entry())).collect();
        failure::Failure::google_content(key, target, message, 0, alternatives).to_error()
    }

    /// The request on the chosen model ([`run_chosen`]): the providers'
    /// calls for each row type, nothing else tried in its place.
    #[allow(clippy::too_many_arguments)]
    async fn run_provider_chain(
        &self,
        messages: Vec<(String, String)>,
        db: &crate::db::Database,
        google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
        app_handle: &tauri::AppHandle,
        tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
        workload: Workload,
        feature: cost::Feature,
        ledger: SideEffectLedger,
    ) -> Result<String, String> {
        let configured = self.configured(db).await;
        let chosen = self.chosen(&configured);
        let (messages, tts_tx, ledger_ref) = (&messages, &tts_tx, &ledger);
        run_chosen(
            chosen,
            capability::picked(),
            &configured,
            feature,
            workload,
            &ledger,
            bedrock::UiLanguage::read(db),
            &resends_inside,
            &|row| mail_policy::may_receive(db, row),
            |event, payload| {
                let _ = app_handle.emit(event, payload);
            },
            move |row| self.send_one(row, messages.clone(), db, google_auth, app_handle, tts_tx.clone(), workload, ledger_ref),
        )
        .await
    }

    /// Send the request to one row.
    #[allow(clippy::too_many_arguments)]
    async fn send_one(
        &self,
        entry: ProviderChainEntry,
        messages: Vec<(String, String)>,
        db: &crate::db::Database,
        google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
        app_handle: &(impl ScopedEmitter + Sync),
        tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
        workload: Workload,
        ledger: &SideEffectLedger,
    ) -> Result<String, String> {
        // "Think harder", as far as the model takes it; nothing extra for a
        // model whose button is hidden.
        let deeper = capability::deeper_for(&entry, workload);
        let turn = provider::Turn { db, google_auth, window: app_handle, voice: tts_tx.clone(), workload, ledger };
        match entry.provider_type.as_str() {
            "gemini" => {
                let Some(ref key) = self.gemini_key else { return Err("No Gemini API key configured".to_string()) };
                let model = gemini_model_for(entry.model_id.as_deref(), workload);
                provider::chat(&gemini::GeminiWire::new(key, &model, deeper), chat_request(db, messages, deeper), turn).await
            }
            "claude" => {
                let Some(ref key) = self.claude_key else { return Err("No Claude API key configured".to_string()) };
                let model = entry.model_id.clone().unwrap_or_else(crate::ai::models::claude_default);
                provider::chat(&claude::ClaudeWire::new(key, &model), chat_request(db, messages, deeper), turn).await
            }
            "openai" if entry.base_url.is_some() => {
                let base = entry.base_url.clone().unwrap_or_default();
                let Some(key) = self.compat_key(&base).await else { return Err(format!("No {} API key configured", compat::label(&base))) };
                let wire = openai::OpenAiWire::new(&key, openai::Target { base_url: Some(&base), model: entry.model_id.as_deref() })?;
                provider::chat(&wire, chat_request(db, messages, deeper), turn).await
            }
            "openai" => {
                let Some(ref key) = self.openai_key else { return Err("No OpenAI API key configured".to_string()) };
                let wire = openai_responses::ResponsesWire::new(key, entry.model_id.as_deref(), deeper);
                provider::chat(&wire, chat_request(db, messages, deeper), turn).await
            }
            "bedrock" => {
                // Load AwsCredentials directly from sqlite + keychain.  We do the
                // lookups inline rather than going through `AwsCredsRepo::new(Arc)`
                // because `AiRouter::send` only holds `&Database` -- routing one
                // borrow through an `Arc` would require restructuring every caller
                // of `AiRouter`.  This is a thin equivalent of `AwsCredsRepo::load`.
                let creds_row = {
                    let conn = db.conn.lock().map_err(|e| format!("bedrock db lock: {e}"))?;
                    conn.query_row(
                        "SELECT access_key_id, region, inference_profile \
                         FROM aws_credentials WHERE id = 1",
                        [],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
                    )
                    .ok()
                };
                let Some((akid, region, profile)) = creds_row else {
                    return Err("bedrock: AWS credentials not configured in Settings".to_string());
                };
                let secret = crate::security::keychain::KeychainStore::get("com.jarvis.aws", "secret_access_key").map_err(|e| format!("bedrock keychain load: {e}"))?;
                let creds = crate::aws::credentials::AwsCredentials { access_key_id: akid, secret_access_key: secret, region, inference_profile: profile };

                // The model the user saved in the chain, as for every other
                // provider. The V13 `workload_routing` table (Nova Micro /
                // Haiku 4.5 / Sonnet 4.6 per tier) used to override it here,
                // so Bedrock silently ran a different model from the one
                // chosen. The table is kept as history and no longer read;
                // per-feature defaults come from the model registry (P05).
                let model = bedrock_model(&entry);
                // A saved id can carry another geography's prefix; point it
                // at the region these credentials were saved for.
                let model = apply_inference_profile(&model, &creds.inference_profile);
                log::info!("Bedrock workload={:?} -> model={} (region={})", workload, model, creds.region);
                provider::chat(&bedrock::BedrockWire::new(creds, &model), chat_request(db, messages, None), turn).await
            }
            "local" => {
                let Some(endpoint_id) = entry.endpoint_id.as_deref() else { return Err("Local provider missing endpoint_id".to_string()) };
                let Some(model_id) = entry.model_id.as_deref() else { return Err("Local provider missing model_id".to_string()) };
                let Some(endpoint) = self.find_endpoint(endpoint_id) else { return Err(format!("Endpoint '{}' not found or inactive", endpoint_id)) };

                let (context_length, tool_override) = {
                    let conn = db.conn.lock().map_err(|e| e.to_string())?;
                    let ctx_len: u32 = conn
                        .query_row(
                            "SELECT context_length FROM local_model_overrides WHERE endpoint_id = ?1 AND model_id = ?2",
                            rusqlite::params![endpoint_id, model_id],
                            |row| row.get(0),
                        )
                        .unwrap_or_else(|_| crate::ai::models::limits("ollama").default_context_tokens.unwrap_or_default());
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
                let detected_cap = backend.detect_tool_capability(&endpoint.url, model_id).await;

                let chat = local::LocalChat::new(endpoint.clone(), detected_cap, context_length, tool_override.as_deref());
                provider::chat(&openai::OpenAiWire::local(chat, model_id), chat_request(db, messages, None), turn).await
            }
            unknown => Err(format!("Unknown provider type: {}", unknown)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A chain nobody saved is worked out when the request is sent, from the
    /// preset, the keys and the registry in force: the order keys were added
    /// in does not change which model answers, and the mail gate gets the
    /// same answer from the same function (review B, I-1).
    #[tokio::test]
    async fn the_model_an_unsaved_chain_runs_on_does_not_depend_on_the_order_keys_were_added() {
        let db = crate::db::test_database();
        let mut router = AiRouter::new("");
        router.set_openai_key("test-openai");
        router.append_default_fallbacks();
        router.set_claude_key("test-claude");
        router.append_default_fallbacks();
        let configured = router.configured(&db).await;
        let keys = KeysFound { gemini: false, claude: true, openai: true };
        assert_eq!(router.keys_found(), keys);
        let chosen = router.chosen(&configured).unwrap();
        assert_eq!(chosen.provider_type, "claude");
        let row = |e: &ProviderChainEntry| (e.provider_type.clone(), e.model_id.clone(), e.base_url.clone(), e.endpoint_id.clone());
        assert_eq!(unsaved_choice("", keys, false).as_ref().map(row), Some(row(&chosen)));
        let tail: Vec<_> = router.provider_chain().iter().rev().take(2).map(|e| e.provider_type.clone()).collect();
        assert_eq!(tail, ["openai", "claude"]);
        // Decided when asked, from the keys the router holds then, as the
        // mail gate decides: not from a chain built when an earlier key was
        // added (or before any was).
        let mut not_rebuilt = AiRouter::new("");
        not_rebuilt.set_claude_key("test-claude");
        assert_eq!(not_rebuilt.chosen(&configured).map(|e| e.provider_type), Some("claude".to_string()));
        // A saved chain is followed as saved.
        router.set_provider_chain(vec![ProviderChainEntry { position: 0, provider_type: "openai".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None }]);
        assert_eq!(router.chosen(&configured).unwrap().provider_type, "openai");
    }

    /// Only the live router publishes which keys exist (review B, r4-m3):
    /// a router built for one job (a sort run, a choice being tested) reads
    /// the Keychain for itself and publishes nothing, so a read that failed
    /// for it alone cannot change what the gate judges. The live router's
    /// keys are published as they are, to this thread's copy (test builds)
    /// and to the process-wide copy release builds read.
    #[test]
    fn only_the_live_router_publishes_its_keys() {
        let db = crate::db::test_database();
        let _job = crate::commands::ai_choice::rebuilt_router(&db);
        assert_eq!(keys_found::get(), None, "a router built for one job publishes nothing");

        let live = std::sync::Mutex::new(AiRouter::new("").without_keys());
        let mut next = AiRouter::new("").without_keys();
        next.set_gemini_key("test-g");
        next.set_claude_key("test-c");
        let expected = KeysFound { gemini: true, claude: true, openai: false };
        let _one_at_a_time = keys_found::PUBLISHING.lock().unwrap_or_else(|e| e.into_inner());
        install_live(&live, next).unwrap();
        assert_eq!(live.lock().unwrap().keys_found(), expected);
        assert_eq!(keys_found::get(), Some(expected));
        assert_eq!(process_wide_keys_found(), Some(expected));
    }

    /// Every tool the app defines is either an action (a resend or a
    /// picked "send again" would carry it out twice, so neither happens
    /// after one ran) or read-only. A new tool cannot be left out of both
    /// (review A, M-5).
    #[test]
    fn every_tool_is_listed_as_an_action_or_as_read_only() {
        const READ_ONLY: &[&str] = &[
            "run_command", "find_files", "list_tasks", "system_info", "search_emails", "read_email", "list_follow_ups", "list_inbox_triage", "list_events",
            "search_notion", "read_notion_page", "list_github_items", "search_notes", "read_note", "render_chart", "render_status", "list_memories",
        ];
        let defined: Vec<String> = tools::get_tool_definitions().into_iter().map(|tool| tool.name).collect();
        for name in &defined {
            let action = SIDE_EFFECTING_TOOLS.contains(&name.as_str());
            let read_only = READ_ONLY.contains(&name.as_str());
            assert!(action != read_only, "{name}: listed as an action {action}, as read-only {read_only}");
        }
        for name in SIDE_EFFECTING_TOOLS.iter().chain(READ_ONLY) {
            assert!(defined.iter().any(|d| d == name), "{name} is listed but no longer defined");
        }
        for action in ["open_app", "open_file", "send_notification"] {
            assert!(SIDE_EFFECTING_TOOLS.contains(&action), "{action} acts on the Mac");
        }
    }

    fn bedrock_entry(model_id: Option<&str>) -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "bedrock".into(), endpoint_id: None, model_id: model_id.map(Into::into), enabled: true, base_url: None }
    }

    /// A typed id is saved trimmed, or refused when it cannot be a Bedrock
    /// id; what goes out for it follows the saved region only for a us.,
    /// eu. or apac. profile.
    #[test]
    fn a_typed_bedrock_model_id_is_saved_trimmed_and_sent_for_the_saved_region() {
        use super::bedrock::UiLanguage::En;
        assert_eq!(bedrock_model_to_save(Some("  anthropic.claude-haiku-4-5-20251001-v1:0\n"), En), Ok(Some("anthropic.claude-haiku-4-5-20251001-v1:0".into())));
        assert_eq!(bedrock_model_to_save(Some("   "), En), Ok(None), "empty is the default model");
        assert_eq!(bedrock_model_to_save(None, En), Ok(None));
        assert!(bedrock_model_to_save(Some("claude haiku"), En).is_err());
        assert!(bedrock_model_to_save(Some(&"a".repeat(2_049)), En).is_err());
        let arn = "arn:aws:bedrock:ap-southeast-1:123456789012:application-inference-profile/abc123";
        assert_eq!(bedrock_model_to_save(Some(arn), En), Ok(Some(arn.into())));

        // What goes out for each, with credentials saved for Singapore.
        let sent = |typed: &str| apply_inference_profile(&bedrock_model(&bedrock_entry(bedrock_model_to_save(Some(typed), En).unwrap().as_deref())), "apac");
        assert_eq!(sent("anthropic.claude-haiku-4-5-20251001-v1:0"), "anthropic.claude-haiku-4-5-20251001-v1:0", "a bare model id as typed");
        assert_eq!(sent("us.anthropic.claude-haiku-4-5-20251001-v1:0"), "apac.anthropic.claude-haiku-4-5-20251001-v1:0", "a profile id follows the saved region");
        assert_eq!(sent("global.anthropic.claude-sonnet-4-6"), "global.anthropic.claude-sonnet-4-6");
        assert_eq!(sent(arn), arn);
    }

    /// The model saved in the chain is the one Bedrock runs; the V13
    /// workload_routing rows (Nova Micro, Haiku 4.5, Sonnet 4.6) no longer
    /// replace it.
    #[test]
    fn bedrock_runs_the_model_saved_in_the_chain() {
        assert_eq!(bedrock_model(&bedrock_entry(Some("amazon.nova-pro-v1:0"))), "amazon.nova-pro-v1:0");
        assert_eq!(bedrock_model(&bedrock_entry(Some("  "))), bedrock::default_model());
        assert_eq!(bedrock_model(&bedrock_entry(None)), bedrock::default_model());
        assert_eq!(apply_inference_profile(&bedrock_model(&bedrock_entry(Some("anthropic.claude-sonnet-4-6"))), "apac"), apply_inference_profile("anthropic.claude-sonnet-4-6", "apac"));
    }

    /// Every chat, voice and briefing row gets its feature from this wrapper:
    /// the providers record deep in the chain, where only the task-local is in
    /// reach. A refactor that dropped it would store them all as "other" and
    /// leave every other test green.
    #[test]
    fn send_with_ledger_runs_the_chain_under_its_feature() {
        let source = include_str!("mod.rs");
        let start = source.find("async fn send_with_ledger(").expect("send_with_ledger exists");
        let body = &source[start..];
        let body = &body[..body.find("\n    }\n").expect("end of send_with_ledger")];
        let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            body.contains("cost::with_feature( feature, finish_request_with_hud( self.run_provider_chain("),
            "send_with_ledger must run the provider chain inside cost::with_feature: {body}"
        );
    }

    fn chosen_row(provider_type: &str, model: Option<&str>) -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id: None, model_id: model.map(str::to_string), enabled: true, base_url: None }
    }

    type Events = Vec<(String, serde_json::Value)>;

    /// `run_chosen` with a fake provider: each send returns the next of
    /// `replies`. Returns the result, the rows sent to and the HUD events.
    async fn run_fake(
        chosen: Option<ProviderChainEntry>,
        picked: Option<capability::Pick>,
        configured: &[ProviderChainEntry],
        feature: cost::Feature,
        ledger: &SideEffectLedger,
        replies: Vec<Result<&str, &str>>,
    ) -> (Result<String, String>, Vec<ProviderChainEntry>, Events) {
        let sent = std::cell::RefCell::new(Vec::new());
        let replies = std::cell::RefCell::new(replies.into_iter());
        let mut events = Vec::new();
        let result = run_chosen(chosen, picked, configured, feature, Workload::Volume, ledger, bedrock::UiLanguage::En, &resends_inside, &|row| row.base_url.is_none(), |event, payload| events.push((event.to_string(), payload)), |row| {
            sent.borrow_mut().push(row);
            let reply = replies.borrow_mut().next().expect("sent more often than expected").map(str::to_string).map_err(str::to_string);
            async move { reply }
        })
        .await;
        (result, sent.into_inner(), events)
    }

    fn providers(rows: &[ProviderChainEntry]) -> Vec<&str> {
        rows.iter().map(|r| r.provider_type.as_str()).collect()
    }

    fn offered(card: &serde_json::Value) -> Vec<String> {
        card["alternatives"].as_array().unwrap().iter().map(|a| a["pick"]["provider_type"].as_str().unwrap().to_string()).collect()
    }

    const BAD_REQUEST: &str = "Gemini API error 400 Bad Request: {\"message\":\"Request contains an invalid argument\"}";
    const OVERLOADED: &str = "Gemini API error 503 Service Unavailable: overloaded";

    /// P06: a failed request ends on the error card. The next row of the
    /// chain is not tried in its place; it is offered.
    #[tokio::test]
    async fn a_failed_request_shows_the_error_card_and_no_other_model_is_tried() {
        let chain = [chosen_row("gemini", None), chosen_row("claude", None)];
        let chosen = chosen_entry(&chain, true, &chain);
        let (result, sent, events) = run_fake(chosen, None, &chain, cost::Feature::Chat, &SideEffectLedger::new(), vec![Err(BAD_REQUEST)]).await;
        assert_eq!(providers(&sent), ["gemini"], "the next row is not tried");
        assert_eq!(sent[0].model_id.as_deref(), Some(crate::ai::models::GEMINI_FLASH), "the model resolved for the feature is the one sent");
        let card = failure::parse(&result.unwrap_err()).expect("the error card");
        assert_eq!(card["code"], "bad_request");
        assert_eq!(card["model"]["label"], "Gemini 3.7 Flash (Google Gemini)");
        assert_eq!(offered(&card), ["claude"], "the other configured model is offered, not used");
        assert!(events.contains(&("chat-token".to_string(), json!({"token": "", "done": true, "discard": false}))), "what streamed stays on screen");
        assert!(events.contains(&("chat-provider".to_string(), json!({"provider": "Gemini 3.7 Flash (Google Gemini)", "status": "failed"}))));
    }

    /// Every replacement of the live router goes through `install_live`,
    /// which publishes its keys, and the launch publishes before any job
    /// starts (review B, r5-m1). These run only in the Tauri app, so the
    /// source is read: the three callers use `install_live`, no code
    /// assigns a router through a guard anywhere else, and `lib.rs` calls
    /// `publish_keys` after the fallbacks are added and before the first
    /// task is spawned.
    #[test]
    fn the_live_router_is_replaced_only_through_install_live() {
        let callers = [
            ("commands/ai_choice.rs", include_str!("../commands/ai_choice.rs"), "crate::ai::install_live(&router, rebuilt_router(&db))?;"),
            ("commands/settings.rs", include_str!("../commands/settings.rs"), "crate::ai::install_live(&router, rebuilt)?;"),
            ("ai/disabled.rs", include_str!("disabled.rs"), "super::install_live(&router, crate::commands::ai_choice::rebuilt_router(&db))?;"),
        ];
        for (file, source, call) in callers {
            assert!(source.contains(call), "{file} replaces the live router through install_live");
        }
        let holders = [
            ("ai/mod.rs", include_str!("mod.rs")),
            ("ai/capability.rs", include_str!("capability.rs")),
            ("commands/assistant.rs", include_str!("../commands/assistant.rs")),
            ("commands/chat.rs", include_str!("../commands/chat.rs")),
            ("commands/cron.rs", include_str!("../commands/cron.rs")),
            ("commands/local_llm.rs", include_str!("../commands/local_llm.rs")),
            ("commands/mail.rs", include_str!("../commands/mail.rs")),
            ("commands/scheduler.rs", include_str!("../commands/scheduler.rs")),
            ("commands/setup.rs", include_str!("../commands/setup.rs")),
            ("lib.rs", include_str!("../lib.rs")),
            ("scheduler/daily_briefing.rs", include_str!("../scheduler/daily_briefing.rs")),
            ("scheduler/scheduled_briefing.rs", include_str!("../scheduler/scheduled_briefing.rs")),
            ("voice/commands.rs", include_str!("../voice/commands.rs")),
            ("voice/wake_word.rs", include_str!("../voice/wake_word.rs")),
        ]
        .into_iter()
        .chain(callers.iter().map(|(file, source, _)| (*file, *source)));
        for (file, source) in holders {
            let assigned: Vec<&str> = source
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with('*') && line.contains(" = ") && !line.contains("=="))
                .filter(|line| line.split(" = ").next().is_some_and(|target| target.contains("router")) || line.contains("rebuilt"))
                .collect();
            let allowed: &[&str] = if file == "ai/mod.rs" { &["*router = next;"] } else { &[] };
            assert_eq!(assigned, allowed, "{file}: a router assigned outside install_live");
        }
        let lib = include_str!("../lib.rs");
        let at = |needle: &str| lib.find(needle).unwrap_or_else(|| panic!("lib.rs: {needle}"));
        let publish = at("router.publish_keys();");
        assert!(at("router.append_default_fallbacks();") < publish, "published after the fallbacks are added");
        assert!(publish < at("tauri::async_runtime::spawn("), "published before any task is spawned");
        assert!(publish < at("scheduler::Scheduler::new("), "published before the scheduler starts");
    }

    /// A failed request is logged by `failure::log_line`: the service, the
    /// failure class, the status and the provider's code, never the body
    /// (review A, M-6 and N-2).
    #[tokio::test]
    async fn a_failed_request_is_logged_without_its_body() {
        let chain = [chosen_row("gemini", None)];
        let chosen = chosen_entry(&chain, true, &chain);
        let body = r#"Gemini API error 400 Bad Request: {"error":{"message":"MAIL-CANARY Kenny owes","status":"INVALID_ARGUMENT"}}"#;
        let logged = crate::test_log::capture(|| async { run_fake(chosen, None, &chain, cost::Feature::Chat, &SideEffectLedger::new(), vec![Err(body)]).await }).await;
        let label = "Gemini 3.7 Flash (Google Gemini)";
        assert!(logged.contains(&failure::log_line(label, body)), "{logged:?}");
        assert!(!logged.iter().any(|line| line.contains("CANARY")), "{logged:?}");
    }

    /// The card names the model a chain nobody saved runs on (`chosen`),
    /// not the preset's first row (review A, R48). Only Claude has a key,
    /// and a fetched copy marks Claude's terms unknown.
    #[tokio::test]
    async fn the_google_content_card_names_the_model_an_unsaved_chain_runs_on() {
        use crate::ai::registry::DataTerms;
        let db = crate::db::test_database();
        db.conn.lock().unwrap().execute("DELETE FROM provider_chain", []).unwrap();
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "claude").unwrap().data_terms = DataTerms::Unknown);
        let mut router = AiRouter::new("gemini_primary").without_keys();
        router.set_claude_key("test-c");
        router.append_default_fallbacks();
        assert_eq!(router.chosen(&router.configured(&db).await).map(|e| e.provider_type), Some("claude".into()));
        let card = failure::parse(&router.google_content_card(&db, cost::Feature::MailAnalysis, "not sent".into()).await).expect("the card");
        assert_eq!(card["model"]["pick"]["provider_type"], "claude", "{card}");
    }

    /// A transient failure is sent once more to the same model, and only
    /// once; anything else ends on the card at once. OpenAI's own wire
    /// already resends a 429 or 5xx.
    #[tokio::test]
    async fn a_transient_failure_is_sent_once_more_to_the_same_model_only() {
        let chain = [chosen_row("gemini", None), chosen_row("claude", None)];
        let chosen = || chosen_entry(&chain, true, &chain);
        let ledger = SideEffectLedger::new();

        let (result, sent, events) = run_fake(chosen(), None, &chain, cost::Feature::Chat, &ledger, vec![Err(OVERLOADED), Ok("answer")]).await;
        assert_eq!(result.unwrap(), "answer");
        assert_eq!(providers(&sent), ["gemini", "gemini"]);
        assert!(events.contains(&("chat-token".to_string(), json!({"token": "", "done": true, "discard": true}))), "the resend replaces what streamed");

        let (result, sent, _) = run_fake(chosen(), None, &chain, cost::Feature::Chat, &ledger, vec![Err(OVERLOADED), Err(OVERLOADED)]).await;
        assert_eq!(providers(&sent), ["gemini", "gemini"], "one resend, then the card");
        assert_eq!(failure::parse(&result.unwrap_err()).unwrap()["code"], "server_error");

        let (_, sent, _) = run_fake(chosen(), None, &chain, cost::Feature::Chat, &ledger, vec![Err(BAD_REQUEST)]).await;
        assert_eq!(sent.len(), 1, "a refused request is not sent again");

        let openai = [chosen_row("openai", None)];
        let (_, sent, _) = run_fake(Some(openai[0].clone()), None, &openai, cost::Feature::Chat, &ledger, vec![Err("OpenAI API error 503 Service Unavailable: x")]).await;
        assert_eq!(sent.len(), 1, "openai::send_retrying already resent it");
    }

    /// A turn whose tool call was refused Gmail or Calendar content (P06,
    /// Google Limited Use) ends on the card, not on the answer given without
    /// it. The card offers only the models that may receive the content (the
    /// fake's rule: no address of its own), and none after an action ran.
    #[tokio::test]
    async fn a_turn_refused_google_content_ends_on_the_card_with_the_models_that_may_receive_it() {
        let at = |base_url: &str| ProviderChainEntry { base_url: Some(base_url.into()), model_id: Some("m".into()), ..chosen_row("openai", None) };
        let kimi = at("https://api.moonshot.ai/v1");
        let chain = [kimi.clone(), at("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"), chosen_row("claude", None)];
        let ledger = SideEffectLedger::new();
        ledger.refuse_google_content("Gmail and Calendar content is not sent to Kimi: ...".into());
        let (result, sent, events) = run_fake(Some(kimi.clone()), None, &chain, cost::Feature::Chat, &ledger, vec![Ok("Reading your email needs another AI.")]).await;
        assert_eq!(sent.len(), 1);
        let card = failure::parse(&result.unwrap_err()).expect("the error card");
        assert_eq!(card["code"], "google_content");
        assert_eq!(card["message"], "Gmail and Calendar content is not sent to Kimi: ...");
        assert_eq!(offered(&card), ["claude"], "Qwen may not receive it either");
        assert!(events.contains(&("chat-token".to_string(), json!({"token": "", "done": true, "discard": true}))), "the answer without it is not kept");

        ledger.note("create_event");
        let (result, _, _) = run_fake(Some(kimi), None, &chain, cost::Feature::Chat, &ledger, vec![Ok("Done.")]).await;
        let card = failure::parse(&result.unwrap_err()).unwrap();
        assert_eq!((card["code"].as_str(), card["actions_ran"].as_u64()), (Some("google_content"), Some(1)));
        assert!(offered(&card).is_empty(), "no model is offered to run it again");

        let (result, _, _) = run_fake(Some(chosen_row("claude", None)), None, &chain, cost::Feature::Chat, &SideEffectLedger::new(), vec![Ok("answer")]).await;
        assert_eq!(result.unwrap(), "answer", "a turn that was refused nothing keeps its answer");
    }

    /// After an action tool ran (a calendar event, a draft), the request is
    /// never sent again: not to the same model, and the card offers none.
    #[tokio::test]
    async fn no_request_is_sent_again_after_an_action_ran() {
        let chain = [chosen_row("gemini", None), chosen_row("claude", None)];
        let ledger = SideEffectLedger::new();
        ledger.note("create_event");
        let (result, sent, events) = run_fake(chosen_entry(&chain, true, &chain), None, &chain, cost::Feature::Chat, &ledger, vec![Err(OVERLOADED)]).await;
        assert_eq!(sent.len(), 1, "no resend after an action");
        let card = failure::parse(&result.unwrap_err()).unwrap();
        assert_eq!(card["actions_ran"], 1);
        assert!(offered(&card).is_empty(), "no model is offered to run it again");
        assert!(events.contains(&("chat-token".to_string(), json!({"token": "", "done": true, "discard": false}))));
    }

    /// A chat run that carried out an action, whether its last model call
    /// failed or the run answered (the consent card's "Send again"), is
    /// recorded, so send_message refuses to send it again; a run that did
    /// nothing may be sent again.
    #[tokio::test]
    async fn a_run_that_carried_out_an_action_is_never_sent_again() {
        let chain = [chosen_row("gemini", None)];
        let acted = |replies: Vec<Result<&'static str, &'static str>>| {
            let chain = chain.clone();
            async move {
                let run = uuid::Uuid::new_v4().to_string();
                let ledger = SideEffectLedger::new();
                ledger.note("create_event");
                let _ = failure::TEST_RUN.scope(run.clone(), run_fake(chosen_entry(&chain, true, &chain), None, &chain, cost::Feature::Chat, &ledger, replies)).await;
                run
            }
        };
        assert!(!failure::may_retry(&acted(vec![Err(OVERLOADED)]).await), "failed after an action");
        assert!(!failure::may_retry(&acted(vec![Ok("Done.")]).await), "answered after an action");
        let clean = uuid::Uuid::new_v4().to_string();
        let _ = failure::TEST_RUN.scope(clean.clone(), run_fake(chosen_entry(&chain, true, &chain), None, &chain, cost::Feature::Chat, &SideEffectLedger::new(), vec![Err(BAD_REQUEST)])).await;
        assert!(failure::may_retry(&clean), "nothing ran");
    }

    /// A saved model the registry disabled is not sent. A request the user
    /// waits on gets the card with the replacement first (choosing it saves
    /// it, as P05's notice did); a background job keeps P05's notice.
    #[tokio::test]
    async fn a_disabled_model_is_not_sent_and_the_card_offers_its_replacement() {
        let _registry = crate::ai::registry::test_override::with(|r| {
            let entry = r.models.iter_mut().find(|m| m.id == "gpt-5.6-luna").unwrap();
            entry.status = crate::ai::registry::Status::Disabled;
            entry.replacement = Some("gpt-6-luna".into());
        });
        let saved = ProviderChainEntry { position: 3, ..chosen_row("openai", Some("gpt-5.6-luna")) };
        let configured = [saved.clone(), chosen_row("claude", None)];

        let (result, sent, _) = run_fake(Some(saved.clone()), None, &configured, cost::Feature::Chat, &SideEffectLedger::new(), vec![]).await;
        assert!(sent.is_empty(), "nothing is sent to a disabled model");
        let card = failure::parse(&result.unwrap_err()).unwrap();
        assert_eq!(card["code"], "disabled");
        assert_eq!(card["alternatives"][0]["pick"]["model_id"], "gpt-6-luna");
        assert_eq!(card["alternatives"][0]["saves_at"], 3);
        assert_eq!(offered(&card), ["openai", "claude"]);

        let (result, sent, _) = run_fake(Some(saved), None, &configured, cost::Feature::MailSort, &SideEffectLedger::new(), vec![]).await;
        assert!(sent.is_empty());
        let error = result.unwrap_err();
        assert!(failure::parse(&error).is_none(), "a background job does not prompt");
        assert!(error.starts_with("The model gpt-5.6-luna has been turned off"), "{error}");
    }

    /// The capability check runs before anything is sent: a model without
    /// what the feature needs is never asked.
    #[tokio::test]
    async fn a_model_without_the_capability_the_feature_needs_is_not_sent() {
        let _registry = crate::ai::registry::test_override::with(|r| {
            let flash = r.models.iter_mut().find(|m| m.id == crate::ai::models::GEMINI_FLASH).unwrap();
            flash.capabilities.tools = false;
            flash.capabilities.json = false;
        });
        let configured = [chosen_row("gemini", None), chosen_row("claude", None)];
        let (result, sent, _) = run_fake(Some(configured[0].clone()), None, &configured, cost::Feature::Chat, &SideEffectLedger::new(), vec![]).await;
        assert!(sent.is_empty());
        let card = failure::parse(&result.unwrap_err()).unwrap();
        assert_eq!((card["code"].as_str(), card["capability"].as_str()), (Some("unsupported"), Some("tools")));
        assert_eq!(offered(&card), ["claude"]);

        // A background job records what it needs; no card.
        let (result, sent, _) = run_fake(Some(configured[0].clone()), None, &configured, cost::Feature::MailSort, &SideEffectLedger::new(), vec![]).await;
        assert!(sent.is_empty());
        let recorded = result.unwrap_err();
        assert!(failure::parse(&recorded).is_none());
        assert!(recorded.starts_with("Needs a model that supports mail sorting: "), "{recorded}");
    }

    /// A model picked in the card runs that request, only when the card
    /// could have offered it.
    #[tokio::test]
    async fn a_picked_model_runs_only_when_the_card_offered_it() {
        let configured = [chosen_row("gemini", None), chosen_row("claude", None)];
        let claude = capability::Pick::of(&chosen_row("claude", None));
        let (result, sent, _) = run_fake(Some(configured[0].clone()), Some(claude), &configured, cost::Feature::Chat, &SideEffectLedger::new(), vec![Ok("from claude")]).await;
        assert_eq!(result.unwrap(), "from claude");
        assert_eq!(providers(&sent), ["claude"]);

        let elsewhere = capability::Pick { provider_type: "openai".into(), base_url: Some("https://attacker.example/v1".into()), endpoint_id: None, model_id: Some("x".into()) };
        let (result, sent, _) = run_fake(Some(configured[0].clone()), Some(elsewhere), &configured, cost::Feature::Chat, &SideEffectLedger::new(), vec![]).await;
        assert!(sent.is_empty(), "a pick the card did not offer is refused");
        assert!(result.unwrap_err().starts_with("That model cannot be used"));
    }

    /// The model is the first enabled row of the saved chain, with or
    /// without credentials (no key ends on the card naming it). Only the
    /// built-in chain nobody saved starts at the first row with credentials.
    #[tokio::test]
    async fn the_chosen_model_is_the_first_enabled_saved_row() {
        let off = ProviderChainEntry { enabled: false, ..chosen_row("gemini", None) };
        let saved = [off, chosen_row("claude", None), chosen_row("openai", None)];
        let keys = [chosen_row("openai", None)];
        assert_eq!(chosen_entry(&saved, true, &keys).map(|r| r.provider_type), Some("claude".into()));
        let (result, _, _) = run_fake(chosen_entry(&saved, true, &keys), None, &keys, cost::Feature::Chat, &SideEffectLedger::new(), vec![Err("No Claude API key configured")]).await;
        assert_eq!(failure::parse(&result.unwrap_err()).unwrap()["code"], "no_key");

        let built_in = [chosen_row("gemini", None), chosen_row("gemini", Some(crate::ai::models::GEMINI_PRO)), chosen_row("openai", None)];
        assert_eq!(chosen_entry(&built_in, false, &keys).map(|r| r.provider_type), Some("openai".into()));
        assert!(chosen_entry(&built_in, false, &[]).is_none());
        let (result, sent, _) = run_fake(None, None, &[], cost::Feature::Chat, &SideEffectLedger::new(), vec![]).await;
        assert!(sent.is_empty());
        assert_eq!(result.unwrap_err(), NO_AI_CHOSEN);
    }

    /// A chain nobody saved: the first enabled row with credentials, matched
    /// on provider, address and endpoint, in the chain's order; none
    /// without credentials.
    #[test]
    fn a_chain_nobody_saved_uses_its_first_enabled_row_with_credentials() {
        let row = |provider: &str, base: Option<&str>, endpoint: Option<&str>| ProviderChainEntry { base_url: base.map(str::to_string), endpoint_id: endpoint.map(str::to_string), ..chosen_row(provider, None) };
        let chain = [
            ProviderChainEntry { enabled: false, ..row("claude", None, None) },
            row("gemini", None, None),
            row("openai", Some("https://api.moonshot.ai/v1"), None),
            row("local", None, Some("lan")),
            row("openai", None, None),
        ];
        let choice = |configured: &[ProviderChainEntry]| unsaved_chain_choice(&chain, configured).map(|r| (r.provider_type, r.base_url, r.endpoint_id));
        assert_eq!(choice(&[row("claude", None, None)]), None, "a disabled row is never the choice");
        assert_eq!(choice(&[row("openai", None, None), row("gemini", None, None)]), Some(("gemini".into(), None, None)), "the chain's order, not the keys'");
        assert_eq!(choice(&[row("openai", None, None)]), Some(("openai".into(), None, None)), "an OpenAI key is not a Kimi key");
        assert_eq!(choice(&[row("openai", Some("https://api.moonshot.ai/v1"), None)]), Some(("openai".into(), Some("https://api.moonshot.ai/v1".into()), None)));
        assert_eq!(choice(&[row("local", None, Some("other"))]), None, "another endpoint is not this one");
        assert_eq!(choice(&[row("local", None, Some("lan"))]), Some(("local".into(), None, Some("lan".into()))));
        assert_eq!(choice(&[]), None);
    }

    /// The table stays as history (P05 replaces it with the registry), but
    /// the request paths no longer read it, directly or through
    /// routing::WorkloadRouter (constructed only in its own tests).
    #[test]
    fn no_request_path_reads_workload_routing() {
        let sources = [
            ("ai/mod.rs", include_str!("mod.rs")),
            ("ai/bedrock.rs", include_str!("bedrock.rs")),
            ("ai/complete.rs", include_str!("complete.rs")),
        ];
        for (name, source) in sources {
            let runtime = source.split("#[cfg(test)]").next().unwrap();
            for read in [["FROM ", "workload_routing"].concat(), ["Workload", "Router"].concat()] {
                assert!(!runtime.contains(&read), "{name} reads workload_routing at runtime ({read})");
            }
        }
    }

    #[test]
    fn plain_http_endpoints_off_this_mac_are_not_used() {
        let endpoint = |id: &str, url: &str| LocalEndpoint {
            id: id.to_string(),
            name: id.to_string(),
            url: url.to_string(),
            backend_type: BackendType::Ollama,
            api_key: None,
            use_tls: false,
            connection_timeout_ms: 5000,
            keep_alive_minutes: 30,
            is_active: true,
            last_health_check: None,
            last_health_status: None,
        };
        let kept: Vec<String> = only_secure_endpoints(vec![
            endpoint("local", "http://localhost:11434"),
            endpoint("loopback", "http://127.0.0.1:8080/v1"),
            endpoint("tls", "https://llm.example.com/v1"),
            endpoint("lan", "http://192.168.1.20:11434"),
            endpoint("remote", "http://llm.example.com/v1"),
        ])
        .into_iter()
        .map(|e| e.id)
        .collect();
        assert_eq!(kept, ["local", "loopback", "tls"]);
    }
    use rusqlite::Connection;

    #[tokio::test]
    async fn failed_request_clears_hud_for_briefing_and_streaming_chat() {
        for streaming_tts in [false, true] {
            let mut events = Vec::new();
            let result: Result<String, String> = finish_request_with_hud(
                async { Err("All providers failed: No Gemini API key configured".into()) },
                streaming_tts,
                |event, payload| events.push((event.to_string(), payload)),
            )
            .await;

            assert_eq!(result.unwrap_err(), "All providers failed: No Gemini API key configured");
            assert_eq!(events, vec![("chat-state".to_string(), json!({"state": "idle"}))],
                "a terminal failure must clear the global HUD even when the caller only propagates the error");
        }
    }

    #[tokio::test]
    async fn completed_briefing_clears_hud_without_requiring_a_chat_command() {
        let mut events = Vec::new();
        let result = finish_request_with_hud(
            async { Ok("今日摘要".to_string()) },
            false,
            |event, payload| events.push((event.to_string(), payload)),
        )
        .await;

        assert_eq!(result.unwrap(), "今日摘要");
        assert_eq!(events, vec![("chat-state".to_string(), json!({"state": "idle"}))]);
    }

    #[tokio::test]
    async fn completed_streaming_request_keeps_hud_until_tts_finishes() {
        let mut events = Vec::new();
        let result = finish_request_with_hud(
            async { Ok("still speaking".to_string()) },
            true,
            |event, payload| events.push((event.to_string(), payload)),
        )
        .await;

        assert_eq!(result.unwrap(), "still speaking");
        assert!(events.is_empty(), "model completion must not reset a still-speaking HUD");
    }

    #[test]
    fn an_environment_key_never_touches_the_keychain() {
        std::env::set_var("JARVIS_TEST_KEY_LAZY", "from-env");
        assert_eq!(
            resolve_key_with("JARVIS_TEST_KEY_LAZY", || panic!("Keychain read although the env var is set")),
            Some("from-env".to_string())
        );
        std::env::remove_var("JARVIS_TEST_KEY_LAZY");
        assert_eq!(resolve_key_with("JARVIS_TEST_KEY_LAZY", || Some("saved".into())), Some("saved".to_string()));
    }

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
        assert!(!ledger.saw_untrusted_read());
        ledger.note("read_email");
        ledger.note("search_emails");
        ledger.note("list_events");
        assert_eq!(ledger.count(), 0);
        // Reading is not a side effect, but it does taint the turn.
        assert!(ledger.saw_untrusted_read());
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

    /// Every tool that pulls text somebody else wrote into the turn has to
    /// taint it. `list_events` and `list_github_items` were missing: a Google
    /// invitation's title is the organiser's text and Google files invitations
    /// on the primary calendar by default, and an issue or PR title is written
    /// by whoever opened it. Without them "what's on today" was enough to get
    /// an injected instruction into `remember`, and from there into every
    /// later system prompt.
    #[test]
    fn every_reader_of_other_peoples_text_taints_the_turn() {
        for reader in [
            "read_email",
            "search_emails",
            "read_note",
            "search_notes",
            "read_notion_page",
            "search_notion",
            "list_events",
            "list_github_items",
            "list_follow_ups",
            "list_inbox_triage",
        ] {
            let ledger = SideEffectLedger::new();
            ledger.note(reader);
            assert!(
                ledger.saw_untrusted_read(),
                "{reader} returns text this device did not author"
            );
        }
        // Reading the device's own state does not taint anything.
        for local in ["list_tasks", "system_info", "list_memories"] {
            let ledger = SideEffectLedger::new();
            ledger.note(local);
            assert!(
                !ledger.saw_untrusted_read(),
                "{local} only reads the user's own data"
            );
        }
    }

    #[test]
    fn a_pasted_context_block_taints_the_turn_before_any_tool_runs() {
        let ledger = SideEffectLedger::untrusted();
        assert!(ledger.saw_untrusted_read());
        assert_eq!(
            ledger.count(),
            0,
            "marking the context untrusted is not itself a side effect"
        );
    }

    /// The taint gate is structurally bypassable: three paths paste mirrored
    /// email subjects, calendar titles and note bodies straight into the
    /// prompt, so no tool call ever runs and the ledger stays clean unless the
    /// call site says otherwise. The compiler cannot see that, so the wiring is
    /// checked as a property of the source text, bounded to the function that
    /// builds each prompt so the assertions cannot satisfy themselves.
    #[test]
    fn every_pasted_context_site_marks_the_turn_untrusted() {
        fn between<'a>(source: &'a str, from: &str, to: &str, what: &str) -> &'a str {
            let start = source
                .find(from)
                .unwrap_or_else(|| panic!("{what}: '{from}' must exist"));
            let len = source[start..]
                .find(to)
                .unwrap_or_else(|| panic!("{what}: '{to}' must follow '{from}'"));
            &source[start..start + len]
        }

        // The whole briefing prompt is DayContext::to_prompt, which carries the
        // sender and subject of the three newest unread mails.
        let briefing = include_str!("../assistant/briefing.rs");
        assert!(
            briefing.contains("send_with_untrusted_context"),
            "the briefing turn reads unread subjects with no tool call and must be tainted"
        );
        assert!(
            !briefing.contains(".send("),
            "no briefing path may take the untainted send"
        );

        // ask_jarvis pastes the same block plus today's event titles.
        let ask = between(
            include_str!("../commands/assistant.rs"),
            "pub async fn ask_jarvis(",
            "pub async fn search_conversations(",
            "ask_jarvis",
        );
        assert!(
            ask.contains("send_with_untrusted_context"),
            "ask_jarvis pastes the context block in and must be tainted"
        );
        assert!(
            !ask.contains(".send("),
            "ask_jarvis must not take the untainted send"
        );

        // send_message adds the [CONTEXT] and [OBSIDIAN NOTES] blocks, both
        // optional -- so here the untainted send is legitimate for a turn that
        // carries neither, and what has to exist is the tainted alternative.
        let chat = between(
            include_str!("../commands/chat.rs"),
            "let mut all_messages = context_messages;",
            "streaming_tts.finish()",
            "send_message",
        );
        assert!(
            chat.contains("send_with_untrusted_context"),
            "a chat turn carrying [CONTEXT] or [OBSIDIAN NOTES] must be tainted"
        );

        // The daily email briefing pastes the senders, subjects and snippets
        // of up to 50 inbox messages and only needs text back, so every call
        // is tainted and runs with tools switched off.
        let daily = include_str!("../scheduler/daily_briefing.rs");
        let daily = &daily[..daily.find("#[cfg(test)]").expect("daily_briefing has tests")];
        assert!(!daily.contains(".send("), "no daily briefing call may take the untainted send");
        let daily = daily.split_whitespace().collect::<Vec<_>>().join(" ");
        let tainted = daily.matches("send_with_untrusted_context(").count();
        assert!(tainted >= 2, "classify and summarise must both be tainted");
        assert_eq!(
            daily.matches("TOOL_EXECUTION_DISABLED .scope(true, router.send_with_untrusted_context(").count(),
            tainted,
            "every daily briefing call must run with tools switched off"
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

    /// The presets, their order and the default are the registry's: a copy
    /// that adds a preset or changes the tiers is what the router builds.
    #[test]
    fn presets_and_the_gemini_tiers_follow_the_registry() {
        use crate::ai::registry::doc::{Preset, PresetEntry, Text};
        let _registry = crate::ai::registry::test_override::with(|r| {
            r.presets.chains.push((
                "claude_then_gemini".into(),
                Preset { label: Text { en: "x".into(), zh_hk: "x".into() }, entries: vec![PresetEntry { provider: "claude".into(), model: Some("claude-opus-5-5".into()) }, PresetEntry { provider: "gemini".into(), model: None }] },
            ));
            r.presets.default = "claude_then_gemini".into();
            r.features.get_mut("premium").unwrap().insert("gemini".into(), "gemini-3.5-flash".into());
        });
        let rows = |name: &str| AiRouter::new(name).provider_chain().iter().map(|e| (e.position, e.provider_type.clone(), e.model_id.clone())).collect::<Vec<_>>();
        let expected = vec![(0, "claude".to_string(), Some("claude-opus-5-5".to_string())), (1, "gemini".to_string(), None)];
        assert_eq!(rows("claude_then_gemini"), expected);
        assert_eq!(rows("not-a-preset"), expected, "an unknown name is the registry's default preset");
        assert_eq!(rows("gemini_pro_only"), vec![(0, "gemini".to_string(), Some(crate::ai::models::GEMINI_PRO.to_string()))]);
        assert_eq!(gemini_model_for(None, Workload::Premium), "gemini-3.5-flash");
        assert_eq!(gemini_model_for(None, Workload::Volume), crate::ai::models::GEMINI_FLASH);
        assert_eq!(gemini_model_for(Some("pinned"), Workload::Premium), "pinned");
    }

    /// The geographies a saved Bedrock id is moved between are the registry's.
    #[test]
    fn inference_geographies_follow_the_registry() {
        assert_eq!(apply_inference_profile("apac.amazon.nova-micro-v1:0", "us"), "us.amazon.nova-micro-v1:0");
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "bedrock").unwrap().inference_geographies = vec!["us".into(), "eu".into(), "jp".into()]);
        assert_eq!(apply_inference_profile("apac.amazon.nova-micro-v1:0", "us"), "apac.amazon.nova-micro-v1:0", "apac is no longer a geography");
        assert_eq!(apply_inference_profile("jp.amazon.nova-micro-v1:0", "eu"), "eu.amazon.nova-micro-v1:0");
    }

    #[test]
    fn default_chain_lets_the_workload_tier_pick_the_first_model() {
        let router = AiRouter::new("gemini_primary");
        let chain = router.provider_chain();
        assert_eq!(chain.len(), 2);
        assert!(chain[0].model_id.is_none());
        assert_eq!(
            gemini_model_for(chain[0].model_id.as_deref(), Workload::Volume),
            crate::ai::models::GEMINI_FLASH
        );
        assert_eq!(
            gemini_model_for(chain[0].model_id.as_deref(), Workload::Premium),
            crate::ai::models::GEMINI_PRO
        );
        // The pinned fallback keeps its explicit model, as the named presets do.
        assert_eq!(chain[1].model_id.as_deref(), Some(crate::ai::models::GEMINI_PRO));
        let pro_only = AiRouter::new("gemini_pro_only");
        assert_eq!(
            gemini_model_for(pro_only.provider_chain()[0].model_id.as_deref(), Workload::Volume),
            crate::ai::models::GEMINI_PRO
        );
    }

    /// Position 1 is what the error card offers when position 0's model is
    /// unavailable (P06: it is no longer tried by itself). If both positions
    /// resolved to the same id, the card would offer the model that just
    /// failed -- which is what shipped while `gemini-3-flash-preview` was
    /// pinned in both slots.
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

    /// The built-in default chain is Gemini-only. A tester who pastes only a
    /// Claude or an OpenAI key, and never saved a chain, must still get an
    /// answer instead of "No Gemini API key configured" on every message: the
    /// first row with a key is the model used (`chosen_entry`).
    #[test]
    fn default_chain_gains_claude_and_openai_when_keys_exist() {
        let mut router = AiRouter::new("gemini_primary");
        router.claude_key = Some("sk-ant-test".to_string());
        router.openai_key = Some("sk-oai-test".to_string());

        router.append_default_fallbacks();
        // Idempotent: a second call (e.g. from a second reload_keys() site)
        // must not duplicate the entries it already appended.
        router.append_default_fallbacks();

        let chain = router.provider_chain();
        let types: Vec<&str> = chain.iter().map(|e| e.provider_type.as_str()).collect();
        assert_eq!(types, vec!["gemini", "gemini", "claude", "openai"]);
        assert!(chain[2].enabled);
        assert!(chain[2].model_id.is_none());
        assert!(chain[3].enabled);
        assert!(chain[3].model_id.is_none());
    }

    /// A chain the user actually persisted -- even one they trimmed down
    /// themselves -- must never be appended to behind their back.
    #[test]
    fn persisted_chain_is_left_alone() {
        let mut router = AiRouter::new("gemini_primary");
        router.claude_key = Some("sk-ant-test".to_string());
        router.openai_key = Some("sk-oai-test".to_string());
        // Simulate what `load_from_db` sets once real provider_chain rows
        // were found, without needing a live Database handle.
        router.chain_loaded_from_db = true;

        router.append_default_fallbacks();

        let chain = router.provider_chain();
        assert_eq!(chain.len(), 2, "persisted chain must not gain new entries");
        let types: Vec<&str> = chain.iter().map(|e| e.provider_type.as_str()).collect();
        assert_eq!(types, vec!["gemini", "gemini"]);
    }

    /// The Settings save path reaches the router through `set_provider_chain`,
    /// never through `load_from_db`, so it has to mark the chain persisted too.
    /// It did not, and `set_api_key` -- which calls `append_default_fallbacks`
    /// on every save -- put back the cloud entries the user had just removed.
    #[test]
    fn a_chain_saved_in_settings_is_not_appended_to() {
        let mut router = AiRouter::new("gemini_primary");
        router.claude_key = Some("sk-ant-test".to_string());
        router.openai_key = Some("sk-oai-test".to_string());

        // What `commands::local_llm::update_provider_chain` does once it has
        // written the rows: hand the router the chain the user saved. Here the
        // user deliberately trimmed inference down to one local endpoint.
        router.set_provider_chain(vec![ProviderChainEntry {
            position: 0,
            provider_type: "local".into(),
            endpoint_id: Some("ollama-1".into()),
            model_id: Some("llama3".into()),
            enabled: true,
            base_url: None,
        }]);

        // What `commands::settings::set_api_key` does on the next key save.
        router.append_default_fallbacks();

        let types: Vec<&str> = router
            .provider_chain()
            .iter()
            .map(|e| e.provider_type.as_str())
            .collect();
        assert_eq!(
            types,
            vec!["local"],
            "a chain saved in Settings must not gain cloud entries behind the user's back"
        );
    }

    /// The shipped migration, not a copy of it -- a hand-written mirror of the
    /// V20 SQL could drift from what refinery actually runs and this test would
    /// never notice.
    const V20_SQL: &str = include_str!("../../migrations/V20__default_chain_fallback.sql");

    /// `provider_chain` as of V10 (the current shape): a hand-written subset
    /// with the columns and CHECK constraint V20 runs against, like the
    /// `aws_credentials` fixture in `aws/credentials.rs` uses, rather than
    /// replaying V7/V8/V10 in order.
    fn fresh_provider_chain_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE provider_chain (
                position INTEGER NOT NULL PRIMARY KEY,
                provider_type TEXT NOT NULL
                    CHECK(provider_type IN ('claude', 'openai', 'local', 'gemini', 'bedrock')),
                endpoint_id TEXT,
                model_id TEXT,
                enabled INTEGER NOT NULL DEFAULT 1
            );",
        )
        .expect("create provider_chain table");
        conn
    }

    fn seed_chain_row(conn: &Connection, position: i32, provider_type: &str, model_id: Option<&str>) {
        conn.execute(
            "INSERT INTO provider_chain (position, provider_type, endpoint_id, model_id, enabled)
             VALUES (?1, ?2, NULL, ?3, 1)",
            rusqlite::params![position, provider_type, model_id],
        )
        .expect("seed chain row");
    }

    fn model_id_at(conn: &Connection, position: i32) -> Option<String> {
        conn.query_row(
            "SELECT model_id FROM provider_chain WHERE position = ?1",
            rusqlite::params![position],
            |r| r.get(0),
        )
        .expect("read back position")
    }

    /// V20 exists because Settings.tsx loads `get_provider_chain`'s response
    /// into React state and writes the whole array back verbatim on the first
    /// edit -- add/remove/reorder/toggle an entry, or remove a local endpoint.
    /// Anyone who touched their chain before this fix shipped has the pre-fix
    /// default persisted, with position 1 pinned to the same Flash id position
    /// 0 resolves to; `load_from_db` then replaces the corrected
    /// `AiRouter::new` default with those stale rows, so the fix never reaches
    /// them without this migration.
    ///
    /// The guards must stay exact: Settings.tsx has a literal "+ Gemini Flash"
    /// button (`handleAddToChain`) that appends at `position: chain.length`. A
    /// user who trimmed their chain down and clicked it has that row as a
    /// DELIBERATE cheap choice -- rewriting it to Pro would quadruple the cost
    /// of the model they picked for being cheap.
    #[test]
    fn v20_rewrites_only_the_byte_for_byte_pre_fix_default() {
        // These are HISTORICAL literals, deliberately not crate::ai::models::GEMINI_FLASH
        // and crate::ai::models::GEMINI_PRO. V20 repairs rows that were written when
        // MODEL_FLASH held this exact value; the constant is free to move to
        // a newer id afterwards and the migration must keep targeting the old
        // one. Binding this test to the constants made it fail the moment
        // MODEL_FLASH was bumped, and the tempting "fix" -- editing the
        // migration to match -- would silently stop repairing real installs.
        const STALE_FLASH: &str = "gemini-3-flash-preview";
        const REPAIRED_PRO: &str = "gemini-3.1-pro-preview";

        // The stale pre-fix default: position 0 unpinned (gemini, no
        // model_id), position 1 pinned to Flash. This IS rewritten to Pro.
        let conn = fresh_provider_chain_conn();
        seed_chain_row(&conn, 0, "gemini", None);
        seed_chain_row(&conn, 1, "gemini", Some(STALE_FLASH));
        conn.execute_batch(V20_SQL).expect("run V20");
        assert_eq!(model_id_at(&conn, 1).as_deref(), Some(REPAIRED_PRO));

        // [claude, gemini flash]: position 0 is not the unpinned-gemini
        // marker, so the EXISTS guard fails. NOT rewritten.
        let conn = fresh_provider_chain_conn();
        seed_chain_row(&conn, 0, "claude", None);
        seed_chain_row(&conn, 1, "gemini", Some(STALE_FLASH));
        conn.execute_batch(V20_SQL).expect("run V20");
        assert_eq!(model_id_at(&conn, 1).as_deref(), Some(STALE_FLASH));

        // A 3-entry chain: the COUNT(*) = 2 guard fails. NOT rewritten.
        let conn = fresh_provider_chain_conn();
        seed_chain_row(&conn, 0, "gemini", None);
        seed_chain_row(&conn, 1, "gemini", Some(STALE_FLASH));
        seed_chain_row(&conn, 2, "claude", None);
        conn.execute_batch(V20_SQL).expect("run V20");
        assert_eq!(model_id_at(&conn, 1).as_deref(), Some(STALE_FLASH));

        // An empty table: no rows match, no-op, does not error.
        let conn = fresh_provider_chain_conn();
        conn.execute_batch(V20_SQL).expect("run V20 on an empty table");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM provider_chain", [], |r| r.get(0))
            .expect("count");
        assert_eq!(rows, 0);
    }

    /// The shipped V21 migration -- see the V20 comment above for why this is
    /// `include_str!`ed rather than a hand-copied string that could drift.
    const V21_SQL: &str = include_str!("../../migrations/V21__beta_cleanup.sql");

    /// Minimal subset of the four tables V21's statements touch, not a
    /// replay of the full migration chain -- same approach as
    /// `fresh_provider_chain_conn` above.
    fn fresh_v21_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE emails (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gmail_id TEXT UNIQUE NOT NULL,
                sender TEXT NOT NULL,
                received_at TEXT NOT NULL
            );
            CREATE TABLE user_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE provider_chain (
                position INTEGER NOT NULL PRIMARY KEY,
                provider_type TEXT NOT NULL
                    CHECK(provider_type IN ('claude', 'openai', 'local', 'gemini', 'bedrock')),
                endpoint_id TEXT,
                model_id TEXT,
                enabled INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE cron_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                schedule TEXT NOT NULL,
                action_type TEXT NOT NULL,
                parameters TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                last_run TEXT,
                next_run TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                description TEXT
            );",
        )
        .expect("create V21 subset tables");
        conn
    }

    /// Pre-fix rows stored the raw RFC 2822 `Date` header; `received_at` is
    /// compared with bytewise TEXT collation everywhere, so those rows sort
    /// wrong and cannot be repaired in place -- they must be dropped. A row
    /// already in the RFC 3339 shape the gmail fix now writes must survive.
    #[test]
    fn v21_deletes_only_pre_fix_email_rows() {
        let conn = fresh_v21_conn();
        conn.execute(
            "INSERT INTO emails (gmail_id, sender, received_at) VALUES ('old', 'a@b.c', ?1)",
            rusqlite::params!["Thu, 4 Sep 2026 10:15:00 +0800"],
        )
        .expect("seed legacy row");
        conn.execute(
            "INSERT INTO emails (gmail_id, sender, received_at) VALUES ('new', 'a@b.c', ?1)",
            rusqlite::params!["2026-09-04T02:15:00Z"],
        )
        .expect("seed ISO row");

        conn.execute_batch(V21_SQL).expect("run V21");

        let ids: Vec<String> = conn
            .prepare("SELECT gmail_id FROM emails ORDER BY gmail_id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(ids, vec!["new".to_string()], "only the ISO-format row must survive");
    }

    /// The V1 seed wrote `ai_provider = 'claude_primary'`, a value no
    /// provider-selection code recognises. It must be rewritten to the
    /// actual default, and only that exact key/value pair -- an install that
    /// already picked `gemini_primary`, or an unrelated preference, must be
    /// left untouched.
    #[test]
    fn v21_rewrites_claude_primary_ai_provider_only() {
        let conn = fresh_v21_conn();
        conn.execute(
            "INSERT INTO user_preferences (key, value) VALUES ('ai_provider', 'claude_primary')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_preferences (key, value) VALUES ('language', 'en')",
            [],
        )
        .unwrap();

        conn.execute_batch(V21_SQL).expect("run V21");

        let provider: String = conn
            .query_row(
                "SELECT value FROM user_preferences WHERE key = 'ai_provider'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(provider, "gemini_primary");
        let language: String = conn
            .query_row(
                "SELECT value FROM user_preferences WHERE key = 'language'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(language, "en", "an unrelated preference must not be touched");
    }

    /// Rows persisted before the model id moved on are still pinned to the
    /// retired `gemini-3-flash-preview` id. The rewrite must key off
    /// `provider_type = 'gemini'` -- a row of a different provider type that
    /// happens to carry the same string is not this defect and must be left
    /// alone.
    #[test]
    fn v21_rewrites_flash_id_only_for_gemini_provider_type() {
        const STALE_FLASH: &str = "gemini-3-flash-preview";
        let conn = fresh_v21_conn();
        conn.execute(
            "INSERT INTO provider_chain (position, provider_type, model_id) VALUES (0, 'gemini', ?1)",
            rusqlite::params![STALE_FLASH],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO provider_chain (position, provider_type, model_id) VALUES (1, 'claude', ?1)",
            rusqlite::params![STALE_FLASH],
        )
        .unwrap();

        conn.execute_batch(V21_SQL).expect("run V21");

        assert_eq!(model_id_at(&conn, 0).as_deref(), Some("gemini-3.7-flash"));
        assert_eq!(
            model_id_at(&conn, 1).as_deref(),
            Some(STALE_FLASH),
            "a non-gemini row must not be rewritten"
        );
    }

    /// The V14 seed wrote `tts_voice_yue = 'Sin-ji'`, but the installed
    /// macOS voice is `Sinji` (no hyphen), so `say -v` never matches it.
    #[test]
    fn v21_rewrites_sin_ji_voice_pref_to_sinji() {
        let conn = fresh_v21_conn();
        conn.execute(
            "INSERT INTO user_preferences (key, value) VALUES ('tts_voice_yue', 'Sin-ji')",
            [],
        )
        .unwrap();

        conn.execute_batch(V21_SQL).expect("run V21");

        let voice: String = conn
            .query_row(
                "SELECT value FROM user_preferences WHERE key = 'tts_voice_yue'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(voice, "Sinji");
    }

    /// Accepted email rules (`email_rules`) never ran because no cron job
    /// dispatched `auto_archive_emails`. The seed must land once on a fresh
    /// install and must not duplicate if the migration body is ever re-run
    /// against a database that already has it.
    #[test]
    fn v21_seeds_auto_archive_job_once_not_twice() {
        let conn = fresh_v21_conn();
        conn.execute_batch(V21_SQL).expect("run V21 the first time");
        conn.execute_batch(V21_SQL).expect("run V21 the second time");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM cron_jobs WHERE action_type = 'auto_archive_emails'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "re-running the migration must not duplicate the job");

        let (schedule, status, description): (String, String, Option<String>) = conn
            .query_row(
                "SELECT schedule, status, description FROM cron_jobs WHERE action_type = 'auto_archive_emails'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(schedule, "0 */10 * * * *");
        assert_eq!(status, "active");
        assert_eq!(
            description.as_deref(),
            Some("Archives new mail from senders whose rules you accepted")
        );
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
            "local_endpoint_api_key:r2-01-keychain", // gitleaks:allow (test account name)
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

        KeychainStore::delete(KEY_SERVICE, "local_endpoint_api_key:r2-01-keychain").ok(); // gitleaks:allow (test account name)
    }
}
