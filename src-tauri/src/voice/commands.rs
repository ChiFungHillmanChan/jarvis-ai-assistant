use crate::ai::AiRouter;
use crate::db::Database;
use crate::voice::{VoiceEngine, VoiceState};
use crate::voice::tts::StreamingTts;
use serde_json::json;
use std::sync::Arc;
use tauri::{Emitter, State};

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

    // Ensure audio stream is alive -- reconnect if it died or failed at startup
    {
        let mut router = engine.audio_router.lock().map_err(|e| e.to_string())?;
        if !router.is_alive() {
            log::warn!("AudioRouter stream not alive, reconnecting...");
            router.reconnect().map_err(|e| {
                let error = format!("Microphone unavailable: {}", e);
                engine.set_state_and_emit(VoiceState::Error(error.clone()));
                error
            })?;
        }
    }

    engine.set_state_and_emit(VoiceState::Listening);
    engine.audio_router.lock().map_err(|e| e.to_string())?.start_ptt();
    engine.start_mic_emitter();
    Ok("Listening...".to_string())
}

#[tauri::command]
pub async fn stop_listening(
    app_handle: tauri::AppHandle,
    engine: State<'_, Arc<VoiceEngine>>,
    router: State<'_, AiRouter>,
    db: State<'_, Arc<Database>>,
    google_auth: State<'_, Arc<crate::auth::google::GoogleAuth>>,
) -> Result<String, String> {
    // Guard: only proceed if currently Listening (prevents concurrent calls)
    {
        let current = engine.state.lock().map_err(|e| e.to_string())?;
        if *current != VoiceState::Listening {
            log::warn!("stop_listening called but state is {:?}, ignoring", *current);
            return Ok(String::new());
        }
    }
    // Immediately transition to Processing so duplicate calls are rejected
    engine.set_state_and_emit(VoiceState::Processing);
    engine.stop_mic_emitter();

    let samples = engine.audio_router.lock().map_err(|e| e.to_string())?.stop_ptt();

    let duration_secs = samples.len() as f32 / 16000.0;
    let rms = if samples.is_empty() { 0.0 } else {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    };

    log::info!("PTT captured: {:.2}s, {} samples, RMS {:.6}", duration_secs, samples.len(), rms);

    if samples.is_empty() {
        log::warn!("PTT returned empty buffer -- audio stream may have died");
        engine.set_state_and_emit(VoiceState::Idle);
        return Ok(String::new());
    }

    if samples.len() < 8000 {
        log::warn!("Audio too short ({:.2}s), skipping transcription", duration_secs);
        engine.set_state_and_emit(VoiceState::Idle);
        return Ok(String::new());
    }

    if rms < 0.001 {
        log::warn!("Audio is silence (RMS {:.6}), skipping transcription", rms);
        if rms == 0.0 && samples.len() > 16000 {
            let error = "Microphone permission denied. Grant access in System Settings > Privacy & Security > Microphone.".to_string();
            engine.set_state_and_emit(VoiceState::Error(error.clone()));
            let _ = app_handle.emit("voice-error", json!({"error": error}));
            return Err(error);
        }
        engine.set_state_and_emit(VoiceState::Idle);
        return Ok(String::new());
    }

    // Signal that voice initiated this AI request (frontend uses this to skip token events)
    let _ = app_handle.emit("chat-voice-active", json!({"active": true}));

    let text = match engine.transcribe_command(&samples).await {
        Ok(text) => text,
        Err(e) => {
            let _ = app_handle.emit("chat-voice-active", json!({"active": false}));
            engine.set_state_and_emit(VoiceState::Idle);
            return Err(e);
        }
    };

    if text.is_empty() {
        let _ = app_handle.emit("chat-voice-active", json!({"active": false}));
        engine.set_state_and_emit(VoiceState::Idle);
        return Ok(String::new());
    }

    // Save user message and notify chat panel
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('user', ?1)", rusqlite::params![text])
            .map_err(|e| e.to_string())?;
    }
    let _ = app_handle.emit("chat-new-message", json!({
        "role": "user", "content": text
    }));
    let _ = app_handle.emit("chat-state", json!({"state": "thinking"}));

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

    // Mute capture before AI call -- streaming TTS will speak during processing
    engine.audio_router.lock().map_err(|e| e.to_string())?.mute();
    engine.set_state_and_emit(VoiceState::Speaking);

    // Create streaming TTS -- speaks sentences as AI tokens arrive
    let streaming_tts = {
        let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
        StreamingTts::new(tts, app_handle.clone())
    };
    let tts_tx = Some(streaming_tts.sender());

    let response = match router.send(messages, &db, &google_auth, &app_handle, tts_tx).await {
        Ok(response) => response,
        Err(e) => {
            // Always clean up on error -- unmute, reset state, signal frontend
            let _ = engine.audio_router.lock().map(|r| r.unmute());
            let _ = app_handle.emit("chat-voice-active", json!({"active": false}));
            let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
            engine.set_state_and_emit(VoiceState::Idle);
            return Err(e);
        }
    };

    // Save assistant response and notify chat panel
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('assistant', ?1)", rusqlite::params![response])
            .map_err(|e| e.to_string())?;
    }
    let _ = app_handle.emit("chat-new-message", json!({
        "role": "assistant", "content": response
    }));

    // Finish streaming TTS -- speaks any remaining buffered text
    streaming_tts.finish().await;

    // Clean up -- always restore audio and state
    let _ = engine.audio_router.lock().map(|r| r.unmute());
    let _ = app_handle.emit("chat-voice-active", json!({"active": false}));
    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
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
