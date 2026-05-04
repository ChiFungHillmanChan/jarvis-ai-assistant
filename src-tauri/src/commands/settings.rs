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
