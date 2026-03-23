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
pub fn get_notion_pages(db: State<Arc<Database>>, limit: Option<u32>) -> Result<Vec<NotionPageView>, String> {
    let limit = limit.unwrap_or(50);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, notion_id, title, url, parent_type, last_edited FROM notion_pages ORDER BY last_edited DESC LIMIT ?1"
    ).map_err(|e| e.to_string())?;
    let pages = stmt.query_map(rusqlite::params![limit], |row| {
        Ok(NotionPageView { id: row.get(0)?, notion_id: row.get(1)?, title: row.get(2)?, url: row.get(3)?, parent_type: row.get(4)?, last_edited: row.get(5)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(pages)
}

#[tauri::command]
pub async fn sync_notion(db: State<'_, Arc<Database>>) -> Result<String, String> {
    let token = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM user_preferences WHERE key = 'notion_api_key'", [], |row| row.get::<_, String>(0))
            .map_err(|_| "Notion API key not configured".to_string())?
    };
    let pages = notion::search_pages(&token, None).await?;
    let count = pages.len();
    notion::save_to_db(&db, &pages)?;
    Ok(format!("Synced {} pages", count))
}

#[tauri::command]
pub fn save_notion_token(db: State<Arc<Database>>, token: String) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES ('notion_api_key', ?1, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
        rusqlite::params![token],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_notion_stats(db: State<Arc<Database>>) -> Result<i64, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM notion_pages", [], |r| r.get(0)).map_err(|e| e.to_string())
}
