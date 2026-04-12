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
    Ok("Connected to Google".to_string())
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
