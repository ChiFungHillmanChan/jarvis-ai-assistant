use crate::auth::google::GoogleAuth;
use crate::db::Database;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn google_connect(auth: State<'_, Arc<GoogleAuth>>, db: State<'_, Arc<Database>>) -> Result<String, String> {
    if auth.is_linked() {
        if let Ok(_token) = auth.ensure_access_token().await {
            return Ok("Already connected to Google".to_string());
        }
    }

    let scopes = vec![
        "https://www.googleapis.com/auth/gmail.readonly".to_string(),
        "https://www.googleapis.com/auth/gmail.modify".to_string(),
        "https://www.googleapis.com/auth/calendar".to_string(),
    ];
    auth.start_auth_flow(scopes).await?;
    auth.save_to_db(&db);
    // Learn who the user is, so greetings and the system prompt address them
    // rather than whoever built the app. Best-effort: a failure here must not
    // fail an otherwise successful connection.
    if let Ok(token) = auth.ensure_access_token().await {
        if let Err(e) = store_profile_name(&token, &db).await {
            log::warn!("Could not read Google profile name: {}", e);
        }
    }
    Ok("Connected to Google".to_string())
}

/// Fetch the signed-in user's given name and persist it as `user_name`.
async fn store_profile_name(access_token: &str, db: &Arc<Database>) -> Result<(), String> {
    let resp = crate::util::http_client()
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("userinfo HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let name = body
        .get("given_name")
        .or_else(|| body.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if name.is_empty() {
        return Err("userinfo returned no name".to_string());
    }
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES ('user_name', ?1, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
        rusqlite::params![name],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn google_status(auth: State<'_, Arc<GoogleAuth>>) -> Result<bool, String> {
    if auth.is_authenticated() {
        return Ok(true);
    }
    if auth.has_refresh_token() {
        match auth.refresh_access_token().await {
            Ok(()) => return Ok(true),
            Err(e) => {
                log::warn!("Google token refresh failed during status check: {}", e);
                return Ok(false);
            }
        }
    }
    Ok(false)
}
