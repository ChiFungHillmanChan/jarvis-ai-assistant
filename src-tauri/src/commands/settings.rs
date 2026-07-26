use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize, Deserialize)]
pub struct Settings {
    pub values: HashMap<String, String>,
}

#[tauri::command]
pub fn get_settings(db: State<Arc<Database>>) -> Result<Settings, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT key, value FROM user_preferences").map_err(|e| e.to_string())?;
    let values: HashMap<String, String> = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(Settings { values })
}

#[tauri::command]
pub fn update_setting(db: State<Arc<Database>>, key: String, value: String) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
        rusqlite::params![key, value],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_user_preference(
    key: String,
    db: tauri::State<'_, std::sync::Arc<crate::db::Database>>,
) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
    conn.query_row(
        "SELECT value FROM user_preferences WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    ).map_err(|e| format!("db read: {e}"))
}

#[tauri::command]
pub async fn set_user_preference(
    key: String,
    value: String,
    db: tauri::State<'_, std::sync::Arc<crate::db::Database>>,
) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
    conn.execute(
        "INSERT INTO user_preferences (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
        rusqlite::params![key, value],
    ).map_err(|e| format!("db write: {e}"))?;
    Ok(())
}

/// Store a provider API key in the macOS Keychain and refresh the live router
/// so the key works immediately. The secret never touches `user_preferences`.
#[tauri::command]
pub fn set_api_key(
    provider: String,
    value: String,
    db: State<'_, Arc<Database>>,
    router: State<'_, std::sync::Mutex<crate::ai::AiRouter>>,
) -> Result<(), String> {
    let account = match provider.as_str() {
        "gemini" => "gemini_api_key",
        "claude" => "claude_api_key",
        "openai" => "openai_api_key",
        other => return Err(format!("unknown provider: {other}")),
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        crate::security::keychain::KeychainStore::delete(crate::ai::KEY_SERVICE, account).ok();
    } else {
        crate::security::keychain::KeychainStore::put(crate::ai::KEY_SERVICE, account, trimmed)?;
    }
    let mut router = router.lock().map_err(|e| e.to_string())?;
    router.load_keys_from_db(&db);
    Ok(())
}

/// Report which provider keys are configured, without returning any secret.
#[tauri::command]
pub fn get_api_key_status() -> Result<HashMap<String, bool>, String> {
    let configured = |env_var: &str, account: &str| {
        std::env::var(env_var).map(|v| !v.trim().is_empty()).unwrap_or(false)
            || crate::security::keychain::KeychainStore::get(crate::ai::KEY_SERVICE, account).is_ok()
    };
    Ok(HashMap::from([
        ("gemini".to_string(), configured("GEMINI_API_KEY", "gemini_api_key")),
        ("claude".to_string(), configured("ANTHROPIC_API_KEY", "claude_api_key")),
        ("openai".to_string(), configured("OPENAI_API_KEY", "openai_api_key")),
    ]))
}
