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
    /// "compatible" and "ollama".
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
        "openai" => ("openai", None, None, None),
        "gemini" => ("gemini", None, None, None),
        "claude" => ("claude", None, None, None),
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
fn describe(chain: &[ProviderChainEntry]) -> Option<CurrentAi> {
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

fn saved_chain(db: &Database) -> Result<Vec<ProviderChainEntry>, String> {
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

fn rebuilt_router(db: &Database) -> AiRouter {
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
        complete::Request { system: "", user: "Reply with the single word OK.", json: false, max_tokens: 1_024 },
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

/// Models the chosen service offers: an OpenAI-compatible service's own
/// list, or the models installed in Ollama.
#[tauri::command]
pub async fn list_ai_models(provider: String, base_url: Option<String>, api_key: Option<String>) -> Result<Vec<String>, String> {
    match provider.as_str() {
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
            let mut models: Vec<String> = body
                .get("models")
                .and_then(|m| m.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(str::to_string))
                .collect();
            models.sort();
            Ok(models)
        }
        _ => Err("這個 AI 不需要選擇模型。".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(provider: &str, base_url: Option<&str>, model: Option<&str>) -> AiChoice {
        AiChoice { provider: provider.into(), api_key: None, base_url: base_url.map(Into::into), model: model.map(Into::into) }
    }

    #[test]
    fn each_choice_becomes_exactly_one_chain_row() {
        let kimi = entry_for(&choice("compatible", Some("https://api.moonshot.ai/v1/"), Some("kimi-k2"))).unwrap();
        assert_eq!((kimi.provider_type.as_str(), kimi.base_url.as_deref(), kimi.model_id.as_deref()), ("openai", Some(compat::KIMI_GLOBAL), Some("kimi-k2")));
        let ollama = entry_for(&choice("ollama", None, Some("qwen3:8b"))).unwrap();
        assert_eq!((ollama.provider_type.as_str(), ollama.endpoint_id.as_deref()), ("local", Some(OLLAMA_ENDPOINT)));
        assert_eq!(entry_for(&choice("gemini", None, None)).unwrap().model_id, None, "Gemini picks Flash or Pro per task");
        assert!(entry_for(&choice("compatible", Some("https://api.moonshot.ai/v1"), None)).is_err(), "a compatible service needs a model");
        assert!(entry_for(&choice("compatible", Some("http://evil.example/v1"), Some("m"))).is_err());
        assert!(entry_for(&choice("ollama", None, Some(" "))).is_err());
        assert!(entry_for(&choice("bedrock", None, None)).is_err());
    }

    #[test]
    fn saving_replaces_the_chain_and_points_mail_at_the_same_ai() {
        let db = crate::db::test_database();
        db.conn.lock().unwrap().execute_batch("INSERT INTO provider_chain (position, provider_type) VALUES (0,'gemini'),(1,'claude');").unwrap();
        let kimi = choice("compatible", Some(compat::KIMI_GLOBAL), Some("kimi-k2"));
        save(&db, &kimi, &entry_for(&kimi).unwrap()).unwrap();
        let chain = saved_chain(&db).unwrap();
        assert_eq!(chain.len(), 1, "nothing else is tried behind the user's back");
        assert_eq!(describe(&chain), Some(CurrentAi { provider: "compatible".into(), label: "Kimi".into(), model: Some("kimi-k2".into()), base_url: Some(compat::KIMI_GLOBAL.into()) }));
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
}
