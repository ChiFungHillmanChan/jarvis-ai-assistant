use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize, Deserialize)]
pub struct Settings {
    pub values: HashMap<String, String>,
}

/// Preference keys that hold a live third-party credential. The webview never
/// needs the secret itself, only whether one is set, so these are replaced by a
/// `<key>_configured` flag on the way out -- the same shape `get_api_key_status`
/// uses for the provider keys.
///
/// Keyed on the preference name, not on where the value is stored, so the filter
/// stays correct once these credentials move to the Keychain.
const SECRET_PREFERENCE_KEYS: [&str; 6] = [
    "github_token",
    "notion_api_key",
    "obsidian_api_key",
    "minimax_api_key",
    "typesafe_api_key",
    "google_refresh_token",
];

fn is_secret_preference(key: &str) -> bool {
    SECRET_PREFERENCE_KEYS.contains(&key)
}

/// Replace every credential-bearing row with a boolean `<key>_configured` flag.
fn redact_secret_preferences(values: HashMap<String, String>) -> HashMap<String, String> {
    values
        .into_iter()
        .map(|(key, value)| {
            if is_secret_preference(&key) {
                let configured = !value.trim().is_empty();
                (format!("{}_configured", key), configured.to_string())
            } else {
                (key, value)
            }
        })
        .collect()
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
    Ok(Settings { values: redact_secret_preferences(values) })
}

#[tauri::command]
pub fn update_setting(
    db: State<Arc<Database>>,
    router: State<'_, std::sync::Mutex<crate::ai::AiRouter>>,
    key: String,
    value: String,
) -> Result<(), String> {
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
            rusqlite::params![key, value],
        ).map_err(|e| e.to_string())?;
    }

    // The live router is constructed from `ai_provider` at startup and nothing
    // else recomputes it, so changing the provider here used to take effect
    // only after a relaunch. Rebuild it with the same sequence lib.rs uses, so
    // the replacement is not missing its chain or its keys.
    if key == "ai_provider" {
        let mut rebuilt = crate::ai::AiRouter::new(&value);
        rebuilt.load_from_db(&db);
        rebuilt.reload_keys();
        rebuilt.append_default_fallbacks();
        *router.lock().map_err(|e| e.to_string())? = rebuilt;
        log::info!("AI provider changed to {}, router rebuilt", value);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_user_preference(
    key: String,
    db: tauri::State<'_, std::sync::Arc<crate::db::Database>>,
) -> Result<String, String> {
    if is_secret_preference(&key) {
        return Err(format!(
            "'{key}' is a credential and cannot be read back. Use the '{key}_configured' flag from get_settings."
        ));
    }
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

/// What a `set_api_key` call is asking for.
#[derive(Debug, PartialEq)]
enum KeyChange {
    Save(String),
    Remove,
}

/// Decide between saving and removing. Removal has to be asked for explicitly.
///
/// A blank value used to mean "delete the Keychain item", which made an
/// accidental SAVE on the permanently-empty Settings field destroy a working
/// credential with no undo. A blank value now saves nothing and removes
/// nothing; only `remove = true` deletes.
fn plan_key_change(value: &str, remove: Option<bool>) -> Result<KeyChange, String> {
    if remove == Some(true) {
        return Ok(KeyChange::Remove);
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(
            "No key was entered, so nothing was saved. Type a key to replace the saved one, \
             or use CLEAR to remove it."
                .to_string(),
        );
    }
    Ok(KeyChange::Save(trimmed.to_string()))
}

/// Store a provider API key in the macOS Keychain and refresh the live router
/// so the key works immediately. The secret never touches `user_preferences`.
///
/// `remove = true` deletes the saved key instead; see `plan_key_change`.
#[tauri::command]
pub fn set_api_key(
    provider: String,
    value: String,
    remove: Option<bool>,
    db: State<'_, Arc<Database>>,
    router: State<'_, std::sync::Mutex<crate::ai::AiRouter>>,
) -> Result<(), String> {
    let change = plan_key_change(&value, remove)?;
    // MiniMax is an integration credential, not one of the chain providers: it
    // is read back through `integration_keys::resolve`, which expects the
    // Keychain item plus the marker row that `store` writes. Routing it through
    // the provider branch below would save the secret but leave
    // `minimax_api_key_configured` false forever.
    if provider == "minimax" || provider == "typesafe" {
        let secret = match change {
            KeyChange::Save(ref s) => s.as_str(),
            KeyChange::Remove => "",
        };
        let key = if provider == "minimax" { "minimax_api_key" } else { "typesafe_api_key" };
        return crate::integration_keys::store(&db, key, secret);
    }
    let account = match provider.as_str() {
        "gemini" => "gemini_api_key",
        "claude" => "claude_api_key",
        "openai" => "openai_api_key",
        other => return Err(format!("unknown provider: {other}")),
    };
    match change {
        KeyChange::Remove => {
            crate::security::keychain::KeychainStore::delete(crate::ai::KEY_SERVICE, account).ok();
        }
        KeyChange::Save(secret) => {
            crate::security::keychain::KeychainStore::put(
                crate::ai::KEY_SERVICE,
                account,
                &secret,
            )?;
        }
    }
    let mut router = router.lock().map_err(|e| e.to_string())?;
    router.reload_keys();
    router.append_default_fallbacks();
    Ok(())
}

/// Report which provider keys are configured, without returning any secret.
#[tauri::command]
pub fn get_api_key_status() -> Result<HashMap<String, bool>, String> {
    let configured = |env_var: &str, account: &str| {
        if std::env::var(env_var).map(|v| !v.trim().is_empty()).unwrap_or(false) {
            return true;
        }
        // A read that fails for any reason other than "no such item" reports
        // the key as unconfigured, which is how a locked keychain shows up in
        // Settings as a key the user never saved. Log the cause, account name
        // only -- never the value.
        match crate::security::keychain::KeychainStore::get(crate::ai::KEY_SERVICE, account) {
            Ok(_) => true,
            Err(e) => {
                if !crate::security::keychain::KeychainStore::is_not_found(&e) {
                    log::warn!("Keychain read failed for {account}: {e}");
                }
                false
            }
        }
    };
    Ok(HashMap::from([
        ("gemini".to_string(), configured("GEMINI_API_KEY", "gemini_api_key")),
        ("claude".to_string(), configured("ANTHROPIC_API_KEY", "claude_api_key")),
        ("openai".to_string(), configured("OPENAI_API_KEY", "openai_api_key")),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every credential the app keeps in `user_preferences` must be on the
    /// deny-list. If a new integration adds one, add it here too.
    #[test]
    fn every_known_credential_key_is_denied() {
        for key in [
            "github_token",
            "notion_api_key",
            "obsidian_api_key",
            "minimax_api_key",
            "google_refresh_token",
        ] {
            assert!(is_secret_preference(key), "{key} must be denied");
        }
    }

    #[test]
    fn ordinary_preferences_are_not_denied() {
        for key in ["ai_provider", "language", "auto_briefing", "user_name", "tts_offline_mode"] {
            assert!(!is_secret_preference(key), "{key} must stay readable");
        }
    }

    /// get_settings hands its result straight to the webview, so no secret
    /// value may survive the trip -- only a boolean flag.
    #[test]
    fn get_settings_returns_flags_instead_of_secrets() {
        let raw = HashMap::from([
            ("ai_provider".to_string(), "gemini_primary".to_string()),
            ("github_token".to_string(), "ghp_realsecret".to_string()),
            ("minimax_api_key".to_string(), "".to_string()),
        ]);

        let out = redact_secret_preferences(raw);

        assert_eq!(out.get("ai_provider"), Some(&"gemini_primary".to_string()));
        assert!(out.get("github_token").is_none(), "raw token must not be returned");
        assert_eq!(out.get("github_token_configured"), Some(&"true".to_string()));
        assert_eq!(out.get("minimax_api_key_configured"), Some(&"false".to_string()));
        assert!(
            !out.values().any(|v| v.contains("ghp_realsecret")),
            "no secret may appear in any returned value"
        );
    }

    /// The Settings key field is blank by design for an already-configured
    /// provider, so a SAVE with nothing typed must not be able to destroy the
    /// saved credential. Blank is refused, not treated as a delete.
    #[test]
    fn a_blank_value_never_removes_a_saved_key() {
        for (value, remove) in [("", None), ("   ", None), ("", Some(false)), ("\t\n", Some(false))]
        {
            let planned = plan_key_change(value, remove);
            assert!(
                planned.is_err(),
                "value {value:?} with remove={remove:?} must be refused, got {planned:?}"
            );
        }
    }

    #[test]
    fn removal_requires_the_explicit_flag() {
        assert_eq!(plan_key_change("", Some(true)), Ok(KeyChange::Remove));
        // The flag wins outright, so the caller never has to send the secret
        // back over IPC just to delete it.
        assert_eq!(plan_key_change("sk-live", Some(true)), Ok(KeyChange::Remove));
    }

    #[test]
    fn a_real_key_is_saved_trimmed() {
        assert_eq!(
            plan_key_change("  sk-live-123  ", None),
            Ok(KeyChange::Save("sk-live-123".to_string()))
        );
        assert_eq!(
            plan_key_change("sk-live-123", Some(false)),
            Ok(KeyChange::Save("sk-live-123".to_string()))
        );
    }
}
