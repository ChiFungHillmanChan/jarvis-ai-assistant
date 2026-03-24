use crate::ai::AiRouter;
use crate::db::Database;
use crate::voice::{VoiceEngine, VoiceState};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn start_listening(engine: State<'_, Arc<VoiceEngine>>) -> Result<String, String> {
    if !engine.is_available() {
        let error = "Voice not available. Configure OPENAI_API_KEY or download the local Whisper model.".to_string();
        engine.set_state_and_emit(VoiceState::Error(error.clone()));
        return Err(error);
    }
    if *engine.muted.lock().unwrap() {
        let error = "Voice is muted".to_string();
        engine.set_state_and_emit(VoiceState::Error(error.clone()));
        return Err(error);
    }
    engine.set_state_and_emit(VoiceState::Listening);
    engine.audio_router.lock().map_err(|e| e.to_string())?.start_ptt();
    Ok("Listening...".to_string())
}

#[tauri::command]
pub async fn stop_listening(
    engine: State<'_, Arc<VoiceEngine>>,
    router: State<'_, AiRouter>,
    db: State<'_, Arc<Database>>,
    google_auth: State<'_, Arc<crate::auth::google::GoogleAuth>>,
) -> Result<String, String> {
    let samples = engine.audio_router.lock().map_err(|e| e.to_string())?.stop_ptt();
    if samples.is_empty() {
        engine.set_state_and_emit(VoiceState::Idle);
        return Ok(String::new());
    }

    engine.set_state_and_emit(VoiceState::Processing);

    let text = match engine.transcribe_command(&samples).await {
        Ok(text) => text,
        Err(e) => {
            engine.set_state_and_emit(VoiceState::Error(e.clone()));
            return Err(e);
        }
    };

    if text.is_empty() {
        engine.set_state_and_emit(VoiceState::Idle);
        return Ok(String::new());
    }

    // Save user message
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('user', ?1)", rusqlite::params![text])
            .map_err(|e| e.to_string())?;
    }

    // Get AI response
    let messages = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT role, content FROM conversations ORDER BY id DESC LIMIT 20")
            .map_err(|e| e.to_string())?;
        let mut msgs: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        msgs.reverse();
        msgs
    };

    let response = match router.send(messages, &db, &google_auth).await {
        Ok(response) => response,
        Err(e) => {
            engine.set_state_and_emit(VoiceState::Error(e.clone()));
            return Err(e);
        }
    };

    // Save assistant response
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('assistant', ?1)", rusqlite::params![response])
            .map_err(|e| e.to_string())?;
    }

    // Speak response — mute capture during TTS to prevent audio feedback
    engine.set_state_and_emit(VoiceState::Speaking);
    engine.audio_router.lock().map_err(|e| e.to_string())?.mute();
    {
        let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
        if let Err(e) = tts.speak(&response).await {
            log::warn!("TTS failed: {}", e);
        }
    }
    engine.audio_router.lock().map_err(|e| e.to_string())?.unmute();

    engine.set_state_and_emit(VoiceState::Idle);
    Ok(response)
}

#[tauri::command]
pub fn get_voice_state(engine: State<Arc<VoiceEngine>>) -> VoiceState {
    engine.get_state()
}

#[tauri::command]
pub fn toggle_mute(engine: State<Arc<VoiceEngine>>) -> bool {
    engine.toggle_mute()
}

#[tauri::command]
pub fn get_voice_settings(db: State<Arc<Database>>) -> Result<VoiceSettings, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let get = |key: &str, default: &str| -> String {
        conn.query_row("SELECT value FROM user_preferences WHERE key = ?1", rusqlite::params![key], |row| row.get(0))
            .unwrap_or_else(|_| default.to_string())
    };
    Ok(VoiceSettings {
        enabled: get("voice_enabled", "true") == "true",
        tts_voice: get("tts_voice", "Samantha"),
        tts_rate: get("tts_rate", "200").parse().unwrap_or(200),
        tts_enabled: get("tts_enabled", "true") == "true",
    })
}

#[tauri::command]
pub fn set_voice_setting(
    db: State<Arc<Database>>,
    engine: State<Arc<VoiceEngine>>,
    key: String,
    value: String,
) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
        rusqlite::params![key, value],
    ).map_err(|e| e.to_string())?;
    drop(conn);

    // Sync in-memory TTS with the new setting
    match key.as_str() {
        "tts_voice" => engine.tts.lock().map_err(|e| e.to_string())?.set_voice(value),
        "tts_rate" => engine.tts.lock().map_err(|e| e.to_string())?.set_rate(value.parse().unwrap_or(200)),
        "tts_enabled" => engine.tts.lock().map_err(|e| e.to_string())?.set_enabled(value == "true"),
        _ => {}
    }
    Ok(())
}

#[tauri::command]
pub async fn list_tts_voices() -> Result<Vec<String>, String> {
    crate::voice::tts::TextToSpeech::list_voices().await
}

#[derive(serde::Serialize)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub tts_voice: String,
    pub tts_rate: u32,
    pub tts_enabled: bool,
}
