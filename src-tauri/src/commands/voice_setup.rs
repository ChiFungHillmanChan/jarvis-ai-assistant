use crate::db::Database;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// Tauri command: download the CosyVoice 2 model and enable offline voice mode.
/// Progress events are emitted as `cosyvoice_progress` on the app handle.
/// The frontend should listen for these events before calling this command.
#[tauri::command]
pub async fn enable_offline_voice(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let app_handle = app.clone();

    // Forward download progress events to the frontend
    tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            let _ = app_handle.emit("cosyvoice_progress", &p);
        }
    });

    let dir = crate::voice::cosyvoice_download::ensure_downloaded(tx).await?;

    let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
    conn.execute(
        "INSERT INTO user_preferences (key, value) VALUES ('cosyvoice_model_path', ?1)
         ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
        rusqlite::params![dir.to_str().unwrap()],
    )
    .map_err(|e| format!("db save model_path: {e}"))?;
    conn.execute(
        "INSERT INTO user_preferences (key, value) VALUES ('tts_offline_mode', '1')
         ON CONFLICT(key) DO UPDATE SET value = '1', updated_at = datetime('now')",
        [],
    )
    .map_err(|e| format!("db enable offline: {e}"))?;
    Ok(())
}

/// Tauri command: check whether the speech-swift CosyVoice binary is reachable.
/// Returns true if found via JARVIS_COSYVOICE_BIN env var or on PATH.
#[tauri::command]
pub fn check_cosyvoice_binary() -> bool {
    crate::voice::cosyvoice::resolve_binary().is_some()
}
