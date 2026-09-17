use crate::db::Database;
use crate::integrations::notion;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct NotionPageView {
    pub id: i64,
    pub notion_id: String,
    pub title: String,
    pub url: Option<String>,
    pub parent_type: Option<String>,
    pub last_edited: Option<String>,
}

#[tauri::command]
pub fn get_notion_pages(
    db: State<Arc<Database>>,
    limit: Option<u32>,
) -> Result<Vec<NotionPageView>, String> {
    let limit = limit.unwrap_or(50);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, notion_id, title, url, parent_type, last_edited FROM notion_pages ORDER BY last_edited DESC LIMIT ?1"
    ).map_err(|e| e.to_string())?;
    let pages = stmt
        .query_map(rusqlite::params![limit], |row| {
            Ok(NotionPageView {
                id: row.get(0)?,
                notion_id: row.get(1)?,
                title: row.get(2)?,
                url: row.get(3)?,
                parent_type: row.get(4)?,
                last_edited: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(pages)
}

#[tauri::command]
pub async fn sync_notion(db: State<'_, Arc<Database>>) -> Result<String, String> {
    notion::sync_connected(&db).await
}

#[tauri::command]
pub async fn save_notion_token(db: State<'_, Arc<Database>>, token: String) -> Result<(), String> {
    // Keychain, not user_preferences -- the row keeps only a marker.
    crate::integrations::notion_mcp::save_rest_token(&db, &token).await
}

#[tauri::command]
pub async fn notion_connect(
    _app: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
) -> Result<crate::integrations::notion_mcp::NotionConnectionStatus, String> {
    crate::integrations::notion_mcp::connect(&db).await
}

#[tauri::command]
pub fn notion_cancel_connect() -> Result<(), String> {
    crate::integrations::notion_mcp::cancel_connect()
}

#[tauri::command]
pub async fn notion_connection_status(
    db: State<'_, Arc<Database>>,
) -> Result<crate::integrations::notion_mcp::NotionConnectionStatus, String> {
    crate::integrations::notion_mcp::connection_status(&db).await
}

#[tauri::command]
pub async fn notion_disconnect(db: State<'_, Arc<Database>>) -> Result<(), String> {
    crate::integrations::notion_mcp::disconnect(&db).await
}

#[tauri::command]
pub fn get_notion_stats(db: State<Arc<Database>>) -> Result<i64, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM notion_pages", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}
