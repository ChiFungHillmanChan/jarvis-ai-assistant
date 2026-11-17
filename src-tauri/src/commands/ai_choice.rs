//! Setup step 1: the user chooses the one AI JARVIS uses. The choice is
//! tested with a short request before anything is saved; saving writes a
//! one-row provider chain (so no other provider is tried behind the user's
//! back) and points mail analysis at the same AI.
//! docs/superpowers/plans/2026-09-26-choose-one-ai.md
use crate::ai::{compat, complete, AiRouter, ProviderChainEntry, KEY_SERVICE};
use crate::db::Database;
use crate::security::keychain::KeychainStore;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::State;

const OLLAMA_ENDPOINT: &str = "ollama-local";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChoice {
    /// "ollama", "openai", "gemini", "claude" or "compatible".
    pub provider: String,
    /// A new key; None keeps the one already saved.
    pub api_key: Option<String>,
    /// "compatible" only.
    pub base_url: Option<String>,
    /// Required for "compatible" and "ollama". Optional for "openai",
    /// "gemini" and "claude": None follows the latest model JARVIS ships with.
    pub model: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct CurrentAi {
    pub provider: String,
    pub label: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

/// A pasted key without surrounding spaces or quotes; keys never contain them.
fn clean_key(key: Option<&str>) -> Option<String> {
    key.map(|k| k.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`').trim().to_string()).filter(|k| !k.is_empty())
}

/// The chain row a choice becomes. Validation only; nothing is saved.
fn entry_for(choice: &AiChoice) -> Result<ProviderChainEntry, String> {
    let model = choice.model.as_deref().map(str::trim).filter(|m| !m.is_empty()).map(str::to_string);
    let (provider_type, endpoint_id, model_id, base_url) = match choice.provider.as_str() {
        "openai" => ("openai", None, model, None),
        "gemini" => ("gemini", None, model, None),
        "claude" => ("claude", None, model, None),
        "compatible" => {
            let base = compat::normalize_base_url(choice.base_url.as_deref().unwrap_or_default())?;
            ("openai", None, Some(model.ok_or("請選擇模型。")?), Some(base))
        }
        "ollama" => ("local", Some(OLLAMA_ENDPOINT.to_string()), Some(model.ok_or("請選擇本機模型。")?), None),
        _ => return Err("請選擇一個 AI。".into()),
    };
    Ok(ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id, model_id, enabled: true, base_url })
}

fn keychain_account(choice: &AiChoice, entry: &ProviderChainEntry) -> Option<String> {
    match choice.provider.as_str() {
        "openai" => Some("openai_api_key".into()),
        "gemini" => Some("gemini_api_key".into()),
        "claude" => Some("claude_api_key".into()),
        "compatible" => entry.base_url.as_deref().map(compat::key_account),
        _ => None,
    }
}

/// Describe a chain as the one AI in use, for setup and Settings.
pub(crate) fn describe(chain: &[ProviderChainEntry]) -> Option<CurrentAi> {
    let entry = chain.iter().find(|e| e.enabled)?;
    let (provider, label) = match (entry.provider_type.as_str(), entry.base_url.as_deref()) {
        ("openai", Some(base)) => ("compatible", compat::label(base)),
        ("openai", None) => ("openai", "OpenAI".to_string()),
        ("gemini", _) => ("gemini", "Gemini".to_string()),
        ("claude", _) => ("claude", "Claude".to_string()),
        ("local", _) => ("ollama", "Ollama".to_string()),
        ("bedrock", _) => ("bedrock", "AWS Bedrock".to_string()),
        (other, _) => (other, other.to_string()),
    };
    Some(CurrentAi { provider: provider.into(), label, model: entry.model_id.clone(), base_url: entry.base_url.clone() })
}

pub(crate) fn saved_chain(db: &Database) -> Result<Vec<ProviderChainEntry>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT position, provider_type, endpoint_id, model_id, enabled, base_url FROM provider_chain ORDER BY position")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProviderChainEntry {
                position: r.get(0)?,
                provider_type: r.get(1)?,
                endpoint_id: r.get(2)?,
                model_id: r.get(3)?,
                enabled: r.get::<_, i32>(4)? != 0,
                base_url: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Persist a tested choice: key, one-row chain, mail on the same AI.
fn save(db: &Database, choice: &AiChoice, entry: &ProviderChainEntry) -> Result<(), String> {
    if let (Some(account), Some(key)) = (keychain_account(choice, entry), clean_key(choice.api_key.as_deref())) {
        KeychainStore::put(KEY_SERVICE, &account, &key)?;
    }
    let mut conn = db.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM provider_chain", []).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO provider_chain (position, provider_type, endpoint_id, model_id, enabled, base_url) VALUES (0, ?1, ?2, ?3, 1, ?4)",
        rusqlite::params![entry.provider_type, entry.endpoint_id, entry.model_id, entry.base_url],
    )
    .map_err(|e| e.to_string())?;
    // Ollama keeps the mail path that checks the model's context size.
    match choice.provider.as_str() {
        "ollama" => tx.execute("UPDATE mail_settings SET processing_mode='local', local_model=?1 WHERE id=1", [entry.model_id.as_deref().unwrap_or_default()]),
        _ => tx.execute("UPDATE mail_settings SET processing_mode='assistant' WHERE id=1", []),
    }
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub(crate) fn rebuilt_router(db: &Database) -> AiRouter {
    let provider = db
        .conn
        .lock()
        .ok()
        .and_then(|conn| conn.query_row("SELECT value FROM user_preferences WHERE key='ai_provider'", [], |r| r.get::<_, String>(0)).ok())
        .unwrap_or_else(|| "gemini_primary".into());
    let mut router = AiRouter::new(&provider);
    router.load_from_db(db);
    router.reload_keys();
    router.append_default_fallbacks();
    router
}

/// Test the choice with one short request, then save it.
#[tauri::command]
pub async fn choose_ai(
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    choice: AiChoice,
) -> Result<CurrentAi, String> {
    let entry = entry_for(&choice)?;
    if choice.provider == "ollama" {
        // Ollama may have started after JARVIS did.
        db.conn
            .lock()
            .map_err(|e| e.to_string())?
            .execute(
                "INSERT OR IGNORE INTO local_endpoints (id, name, url, backend_type, is_active) VALUES (?1, 'Ollama (local)', 'http://localhost:11434', 'ollama', 1)",
                [OLLAMA_ENDPOINT],
            )
            .map_err(|e| e.to_string())?;
    }
    let mut candidate = rebuilt_router(&db);
    if let Some(key) = clean_key(choice.api_key.as_deref()) {
        match (choice.provider.as_str(), entry.base_url.as_deref()) {
            ("openai", _) => candidate.set_openai_key(&key),
            ("gemini", _) => candidate.set_gemini_key(&key),
            ("claude", _) => candidate.set_claude_key(&key),
            ("compatible", Some(base)) => candidate.set_compat_key(base, &key),
            _ => {}
        }
    }
    candidate.set_provider_chain(vec![entry.clone()]);
    complete::complete(
        &candidate,
        &db,
        complete::Request { system: "", user: "Reply with the single word OK.", json: false, max_tokens: 1_024, feature: crate::ai::cost::Feature::Other },
    )
    .await?;
    save(&db, &choice, &entry)?;
    *router.lock().map_err(|e| e.to_string())? = rebuilt_router(&db);
    describe(&[entry]).ok_or_else(|| "未能讀取所選 AI。".into())
}

/// The AI in use, or None when setup has not chosen one.
#[tauri::command]
pub fn current_ai(db: State<'_, Arc<Database>>) -> Result<Option<CurrentAi>, String> {
    Ok(describe(&saved_chain(&db)?))
}

/// Whether a model id ends in a dated snapshot ("-2025-08-07", "-0125").
fn is_snapshot(id: &str) -> bool {
    let tail: Vec<&str> = id.rsplit('-').take(3).collect();
    let digits = |s: &str, n: usize| s.len() == n && s.chars().all(|c| c.is_ascii_digit());
    digits(tail[0], 4) || (tail.len() == 3 && digits(tail[0], 2) && digits(tail[1], 2) && digits(tail[2], 4)) || digits(tail[0], 8)
}

/// The models a provider lists that can hold a conversation, newest name
/// first. The registry decides: the provider's `model_id_pattern`, and the
/// `chat_model_filter` words and dated snapshots left out (image, audio,
/// embedding, coding and the like).
fn chat_models(provider: &str, ids: Vec<String>) -> Vec<String> {
    let registry = crate::ai::registry::current();
    let filter = &registry.chat_model_filter;
    let pattern = registry.provider(provider).and_then(|p| p.model_id_pattern.as_deref()).and_then(|pattern| match regex::Regex::new(pattern) {
        Ok(re) => Some(re),
        Err(_) => {
            log::warn!("registry: model_id_pattern for {provider} is not a valid pattern; listing every model");
            None
        }
    });
    let mut models: Vec<String> = ids
        .into_iter()
        .map(|id| id.trim_start_matches(filter.strip_prefix.as_str()).to_string())
        .filter(|id| pattern.as_ref().is_none_or(|re| re.is_match(id)))
        .filter(|id| !filter.exclude_words.iter().any(|word| id.contains(word.as_str())) && !(filter.exclude_dated_snapshots && is_snapshot(id)))
        .collect();
    models.sort_by(|a, b| b.cmp(a));
    models.dedup();
    models
}

/// The pasted key, or the one saved for this provider.
async fn provider_key(account: &'static str, pasted: Option<&str>, name: &str) -> Result<String, String> {
    if let Some(key) = clean_key(pasted) {
        return Ok(key);
    }
    tokio::task::spawn_blocking(move || crate::integration_keys::resolve(account))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| format!("請先輸入 {name} API 密鑰。"))
}

/// GET a provider's model list and pull the ids out of `list`/`field`.
async fn fetch_models(name: &str, request: reqwest::RequestBuilder, list: &str, field: &str) -> Result<Vec<String>, String> {
    let response = request.send().await.map_err(|_| format!("未能連接 {name}，請檢查網絡。"))?;
    match response.status().as_u16() {
        200 => {}
        400 | 401 | 403 => return Err(format!("{name} 拒絕了這個 API 密鑰，請檢查後重試。")),
        status => return Err(format!("{name} 暫時未能列出模型（{status}）。")),
    }
    let body: serde_json::Value = response.json().await.map_err(|_| format!("{name} 回覆的模型清單格式無效。"))?;
    Ok(body
        .get(list)
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| m.get(field).and_then(|id| id.as_str()).map(str::to_string))
        .collect())
}

/// Models the chosen service offers, read live from the provider so new
/// models appear without an app update: OpenAI, Gemini, Claude, an
/// OpenAI-compatible service, or the models installed in Ollama.
#[tauri::command]
pub async fn list_ai_models(provider: String, base_url: Option<String>, api_key: Option<String>) -> Result<Vec<String>, String> {
    match provider.as_str() {
        "openai" => {
            let key = provider_key("openai_api_key", api_key.as_deref(), "OpenAI").await?;
            let ids = fetch_models("OpenAI", compat::client()?.get("https://api.openai.com/v1/models").bearer_auth(key), "data", "id").await?;
            Ok(chat_models("openai", ids))
        }
        "gemini" => {
            let key = provider_key("gemini_api_key", api_key.as_deref(), "Gemini").await?;
            let request = compat::client()?.get(format!("{}/models?pageSize=1000", crate::ai::gemini::API_BASE)).header("x-goog-api-key", key);
            let ids = fetch_models("Gemini", request, "models", "name").await?;
            Ok(chat_models("gemini", ids))
        }
        "claude" => {
            let key = provider_key("claude_api_key", api_key.as_deref(), "Claude").await?;
            let request = compat::client()?.get("https://api.anthropic.com/v1/models?limit=1000").header("x-api-key", key).header("anthropic-version", crate::ai::models::anthropic_version());
            let ids = fetch_models("Claude", request, "data", "id").await?;
            Ok(chat_models("claude", ids))
        }
        "compatible" => {
            let base = compat::normalize_base_url(base_url.as_deref().unwrap_or_default())?;
            let key = match clean_key(api_key.as_deref()) {
                Some(key) => key,
                None => {
                    let lookup = base.clone();
                    tokio::task::spawn_blocking(move || compat::resolve_key(&lookup))
                        .await
                        .ok()
                        .flatten()
                        .ok_or_else(|| format!("請先輸入 {} API 密鑰。", compat::label(&base)))?
                }
            };
            compat::list_models(&base, &key).await
        }
        "ollama" => {
            let response = compat::client()?
                .get("http://localhost:11434/api/tags")
                .send()
                .await
                .map_err(|_| "未偵測到 Ollama。請先開啟 Ollama App。".to_string())?;
            let body: serde_json::Value = response.json().await.map_err(|_| "Ollama 回覆格式無效。".to_string())?;
            Ok(crate::ai::local::ollama::models_on_this_mac(&body))
        }
        _ => Err("這個 AI 不需要選擇模型。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIMI_GLOBAL: &str = "https://api.moonshot.ai/v1";

    fn choice(provider: &str, base_url: Option<&str>, model: Option<&str>) -> AiChoice {
        AiChoice { provider: provider.into(), api_key: None, base_url: base_url.map(Into::into), model: model.map(Into::into) }
    }

    #[test]
    fn each_choice_becomes_exactly_one_chain_row() {
        let kimi = entry_for(&choice("compatible", Some("https://api.moonshot.ai/v1/"), Some("kimi-k2"))).unwrap();
        assert_eq!((kimi.provider_type.as_str(), kimi.base_url.as_deref(), kimi.model_id.as_deref()), ("openai", Some(KIMI_GLOBAL), Some("kimi-k2")));
        let ollama = entry_for(&choice("ollama", None, Some("qwen3:8b"))).unwrap();
        assert_eq!((ollama.provider_type.as_str(), ollama.endpoint_id.as_deref()), ("local", Some(OLLAMA_ENDPOINT)));
        assert_eq!(entry_for(&choice("gemini", None, None)).unwrap().model_id, None, "Gemini picks Flash or Pro per task");
        assert!(entry_for(&choice("compatible", Some("https://api.moonshot.ai/v1"), None)).is_err(), "a compatible service needs a model");
        assert_eq!(entry_for(&choice("openai", None, None)).unwrap().model_id, None, "no model follows the latest");
        assert_eq!(entry_for(&choice("claude", None, Some(" claude-opus-5-5 "))).unwrap().model_id.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(entry_for(&choice("openai", None, Some(""))).unwrap().model_id, None);
        assert!(entry_for(&choice("compatible", Some("http://evil.example/v1"), Some("m"))).is_err());
        assert!(entry_for(&choice("ollama", None, Some(" "))).is_err());
        assert!(entry_for(&choice("bedrock", None, None)).is_err());
    }

    #[test]
    fn saving_replaces_the_chain_and_points_mail_at_the_same_ai() {
        let db = crate::db::test_database();
        db.conn.lock().unwrap().execute_batch("INSERT INTO provider_chain (position, provider_type) VALUES (0,'gemini'),(1,'claude');").unwrap();
        let kimi = choice("compatible", Some(KIMI_GLOBAL), Some("kimi-k2"));
        save(&db, &kimi, &entry_for(&kimi).unwrap()).unwrap();
        let chain = saved_chain(&db).unwrap();
        assert_eq!(chain.len(), 1, "nothing else is tried behind the user's back");
        assert_eq!(describe(&chain), Some(CurrentAi { provider: "compatible".into(), label: "Kimi".into(), model: Some("kimi-k2".into()), base_url: Some(KIMI_GLOBAL.into()) }));
        assert_eq!(crate::mail::store::settings(&db).unwrap().processing_mode, "assistant");
        let ollama = choice("ollama", None, Some("qwen3:8b"));
        db.conn.lock().unwrap().execute("INSERT INTO local_endpoints (id, name, url, backend_type) VALUES ('ollama-local','Ollama','http://localhost:11434','ollama')", []).unwrap();
        save(&db, &ollama, &entry_for(&ollama).unwrap()).unwrap();
        let mail = crate::mail::store::settings(&db).unwrap();
        assert_eq!((mail.processing_mode.as_str(), mail.local_model.as_str()), ("local", "qwen3:8b"));
    }

    #[test]
    fn pasted_keys_lose_spaces_and_quotes() {
        assert_eq!(clean_key(Some(" 'AIzaSy-abc' ")).as_deref(), Some("AIzaSy-abc"));
        assert_eq!(clean_key(Some("\"sk-123\"\n")).as_deref(), Some("sk-123"));
        assert_eq!(clean_key(Some("  ")), None);
        assert_eq!(clean_key(None), None);
    }

    #[test]
    fn no_chain_means_no_ai_chosen() {
        assert_eq!(describe(&[]), None);
    }

    #[test]
    fn model_lists_keep_only_chat_models() {
        let ids = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            chat_models("openai", ids(&["gpt-6-luna", "o4-mini", "gpt-4o-audio-preview", "text-embedding-3-small", "dall-e-3", "whisper-1", "omni-moderation-latest", "gpt-6-luna", "gpt-5-2025-08-07", "gpt-3.5-turbo-0125", "gpt-5.3-codex", "gpt-live-1", "gpt-6.1-sol"])),
            ["o4-mini", "gpt-6.1-sol", "gpt-6-luna"]
        );
        assert_eq!(
            chat_models("gemini", ids(&["models/gemini-3.7-flash", "models/gemini-embedding-001", "models/imagen-4", "models/gemini-3.1-pro-preview"])),
            ["gemini-3.7-flash", "gemini-3.1-pro-preview"]
        );
        assert_eq!(chat_models("claude", ids(&["claude-sonnet-5-5", "claude-opus-5-5", "claude-3-5-sonnet-20241022"])), ["claude-sonnet-5-5", "claude-opus-5-5"]);
    }

    /// The filter is the registry's: a copy that changes it changes the lists.
    #[test]
    fn the_chat_model_filter_follows_the_registry() {
        let ids = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let _registry = crate::ai::registry::test_override::with(|r| {
            r.chat_model_filter.exclude_words.push("-mini".into());
            r.chat_model_filter.exclude_dated_snapshots = false;
            r.providers.iter_mut().find(|p| p.id == "openai").unwrap().model_id_pattern = Some("^gpt-6".into());
        });
        assert_eq!(chat_models("openai", ids(&["gpt-6-luna", "gpt-6-mini", "gpt-5-2025-08-07", "o4"])), ["gpt-6-luna"]);
        assert_eq!(chat_models("claude", ids(&["claude-3-5-sonnet-20241022"])), ["claude-3-5-sonnet-20241022"], "snapshots stay when the copy says so");
    }
}
