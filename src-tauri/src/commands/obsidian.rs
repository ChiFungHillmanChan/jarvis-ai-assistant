use crate::db::Database;
use crate::integrations::obsidian;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct ObsidianNoteView {
    pub path: String,
    pub content: Option<String>,
}

fn get_obsidian_key(db: &Arc<Database>) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT value FROM user_preferences WHERE key = 'obsidian_api_key'",
        [],
        |row| row.get::<_, String>(0),
    )
    .map_err(|_| "Obsidian API key not configured. Set it in Settings.".to_string())
}

#[tauri::command]
pub async fn search_obsidian(
    db: State<'_, Arc<Database>>,
    query: String,
) -> Result<Vec<ObsidianNoteView>, String> {
    let key = get_obsidian_key(&db)?;
    let notes = obsidian::search_vault(&key, &query).await?;
    Ok(notes
        .into_iter()
        .map(|n| ObsidianNoteView {
            path: n.path,
            content: n.content,
        })
        .collect())
}

#[tauri::command]
pub async fn get_obsidian_note(
    db: State<'_, Arc<Database>>,
    path: String,
) -> Result<String, String> {
    let key = get_obsidian_key(&db)?;
    obsidian::get_note(&key, &path).await
}

#[tauri::command]
pub async fn save_obsidian_note(
    db: State<'_, Arc<Database>>,
    path: String,
    content: String,
) -> Result<(), String> {
    let key = get_obsidian_key(&db)?;
    obsidian::save_note(&key, &path, &content).await
}

#[tauri::command]
pub async fn list_obsidian_files(db: State<'_, Arc<Database>>) -> Result<Vec<String>, String> {
    let key = get_obsidian_key(&db)?;
    obsidian::list_files(&key).await
}

#[tauri::command]
pub fn save_obsidian_key(db: State<Arc<Database>>, key: String) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES ('obsidian_api_key', ?1, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
        rusqlite::params![key],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
