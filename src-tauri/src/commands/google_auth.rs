use crate::auth::google::GoogleAuth;
use crate::db::Database;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn google_connect(auth: State<'_, Arc<GoogleAuth>>, db: State<'_, Arc<Database>>) -> Result<String, String> {
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
pub fn google_status(auth: State<Arc<GoogleAuth>>) -> bool {
    auth.is_authenticated()
}
