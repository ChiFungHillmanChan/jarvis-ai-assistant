use crate::db::Database;
use crate::voice::model_manager;
use crate::voice::wake_word::WakeWordService;
use crate::voice::{VoiceEngine, VoiceState};
use std::sync::Arc;
use tauri::State;

#[derive(serde::Serialize)]
pub struct WakeWordStatus {
    pub enabled: bool,
    pub running: bool,
    pub model_downloaded: bool,
    pub voice_state: VoiceState,
}

#[tauri::command]
pub async fn get_wake_word_status(
    service: State<'_, Arc<WakeWordService>>,
    engine: State<'_, Arc<VoiceEngine>>,
    db: State<'_, Arc<Database>>,
) -> Result<WakeWordStatus, String> {
    Ok(WakeWordStatus {
        enabled: get_pref(&db, "wake_word_enabled", "false")? == "true",
        running: service.is_running().await,
        model_downloaded: model_manager::is_downloaded(),
        voice_state: engine.get_state(),
    })
}

#[tauri::command]
pub async fn enable_wake_word(
    service: State<'_, Arc<WakeWordService>>,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    service.enable().await?;
    set_pref(&db, "wake_word_enabled", "true")
}

#[tauri::command]
pub async fn disable_wake_word(
    service: State<'_, Arc<WakeWordService>>,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    service.disable().await?;
    set_pref(&db, "wake_word_enabled", "false")
}

#[tauri::command]
pub fn is_model_downloaded() -> bool {
    model_manager::is_downloaded()
}

#[tauri::command]
pub async fn download_model(engine: State<'_, Arc<VoiceEngine>>) -> Result<bool, String> {
    let app_handle = engine
        .app_handle
        .clone()
        .ok_or("Tauri app handle not available")?;
    model_manager::download(app_handle).await?;
    Ok(true)
}

fn get_pref(db: &Database, key: &str, default: &str) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    Ok(conn
        .query_row(
            "SELECT value FROM user_preferences WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| default.to_string()))
}

fn set_pref(db: &Database, key: &str, value: &str) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
