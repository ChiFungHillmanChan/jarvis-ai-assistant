use crate::ai::AiRouter;
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::voice::model_manager;
use crate::voice::transcribe::LocalTranscriber;
use crate::voice::{JarvisAppHandle, VoiceEngine, VoiceState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;
use tokio::sync::Mutex as AsyncMutex;
use tauri::async_runtime::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct WakeWordService {
    task: AsyncMutex<Option<JoinHandle<()>>>,
    cancellation_token: Mutex<CancellationToken>,
    voice_engine: Arc<VoiceEngine>,
    db: Arc<Database>,
    router: AiRouter,
    auth: Arc<GoogleAuth>,
    app_handle: JarvisAppHandle,
}

impl WakeWordService {
    pub fn new(
        voice_engine: Arc<VoiceEngine>,
        db: Arc<Database>,
        router: AiRouter,
        auth: Arc<GoogleAuth>,
        app_handle: JarvisAppHandle,
    ) -> Self {
        Self {
            task: AsyncMutex::new(None),
            cancellation_token: Mutex::new(CancellationToken::new()),
            voice_engine,
            db,
            router,
            auth,
            app_handle,
        }
    }

    pub async fn enable(&self) -> Result<(), String> {
        if *self.voice_engine.muted.lock().map_err(|e| e.to_string())? {
            return Err("Voice is muted".to_string());
        }

        if !model_manager::is_downloaded() {
            return Err("Wake-word model not downloaded".to_string());
        }

        {
            let mut task_guard = self.task.lock().await;
            if let Some(handle) = task_guard.as_ref() {
                if !handle.inner().is_finished() {
                    return Ok(());
                }
                task_guard.take();
            }
        }

        let local_transcriber = LocalTranscriber::new(&model_manager::model_path())?;

        {
            let mut router = self
                .voice_engine
                .audio_router
                .lock()
                .map_err(|e| e.to_string())?;
            if !router.is_alive() {
                router.start()?;
            }
            router.unmute();
        }

        let token = {
            let mut guard = self
                .cancellation_token
                .lock()
                .map_err(|e| e.to_string())?;
            *guard = CancellationToken::new();
            guard.clone()
        };

        self.voice_engine
            .set_state_and_emit(VoiceState::WakeWordListening);

        let voice_engine = Arc::clone(&self.voice_engine);
        let db = Arc::clone(&self.db);
        let router = self.router.clone();
        let auth = Arc::clone(&self.auth);
        let app_handle = self.app_handle.clone();

        let handle = tauri::async_runtime::spawn(async move {
            supervisor_loop(
                voice_engine,
                db,
                router,
                auth,
                app_handle,
                token,
                local_transcriber,
            )
            .await;
        });

        *self.task.lock().await = Some(handle);
        Ok(())
    }

    pub async fn disable(&self) -> Result<(), String> {
        self.cancellation_token
            .lock()
            .map_err(|e| e.to_string())?
            .cancel();

        if let Some(handle) = self.task.lock().await.take() {
            let _ = handle.await;
        }

        self.voice_engine
            .audio_router
            .lock()
            .map_err(|e| e.to_string())?
            .deactivate();

        if *self.voice_engine.muted.lock().map_err(|e| e.to_string())? {
            self.voice_engine.set_state_and_emit(VoiceState::Disabled);
        } else {
            self.voice_engine.set_state_and_emit(VoiceState::Idle);
        }

        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        let mut guard = self.task.lock().await;
        let is_running = guard
            .as_ref()
            .map(|handle| !handle.inner().is_finished())
            .unwrap_or(false);
        if !is_running {
            guard.take();
        }
        is_running
    }
}

async fn supervisor_loop(
    voice_engine: Arc<VoiceEngine>,
    db: Arc<Database>,
    router: AiRouter,
    auth: Arc<GoogleAuth>,
    app_handle: JarvisAppHandle,
    token: CancellationToken,
    local_transcriber: LocalTranscriber,
) {
    loop {
        if token.is_cancelled() {
            break;
        }

        match detection_loop(
            Arc::clone(&voice_engine),
            Arc::clone(&db),
            router.clone(),
            Arc::clone(&auth),
            app_handle.clone(),
            token.clone(),
            local_transcriber.clone(),
        )
        .await
        {
            Ok(()) => break,
            Err(e) => {
                log::error!("Wake-word supervisor error: {}", e);
                voice_engine.set_state_and_emit(VoiceState::Error(e));

                if token.is_cancelled() {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(3)).await;
                if token.is_cancelled() {
                    break;
                }

                match voice_engine.audio_router.lock() {
                    Ok(mut audio_router) => {
                        if let Err(reconnect_err) = audio_router.reconnect() {
                            log::error!(
                                "Wake-word audio reconnect failed: {}",
                                reconnect_err
                            );
                            continue;
                        }
                        audio_router.unmute();
                    }
                    Err(lock_err) => {
                        log::error!("Wake-word audio router lock failed: {}", lock_err);
                        continue;
                    }
                }

                voice_engine.set_state_and_emit(VoiceState::WakeWordListening);
            }
        }
    }
}

async fn detection_loop(
    voice_engine: Arc<VoiceEngine>,
    db: Arc<Database>,
    router: AiRouter,
    auth: Arc<GoogleAuth>,
    app_handle: JarvisAppHandle,
    token: CancellationToken,
    local_transcriber: LocalTranscriber,
) -> Result<(), String> {
    voice_engine.set_state_and_emit(VoiceState::WakeWordListening);

    loop {
        tokio::select! {
            _ = token.cancelled() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(800)) => {
                if *voice_engine.muted.lock().map_err(|e| e.to_string())? {
                    continue;
                }

                if !matches!(voice_engine.get_state(), VoiceState::WakeWordListening | VoiceState::Idle) {
                    continue;
                }

                let audio_window = voice_engine
                    .audio_router
                    .lock()
                    .map_err(|e| e.to_string())?
                    .read_ring(2.0);

                if !has_speech(&audio_window, 0.015) {
                    continue;
                }

                let transcript = transcribe_local_window(local_transcriber.clone(), audio_window).await?;
                if transcript.is_empty() || !is_wake_phrase(&transcript) {
                    continue;
                }

                handle_wake_word_detection(
                    Arc::clone(&voice_engine),
                    Arc::clone(&db),
                    router.clone(),
                    Arc::clone(&auth),
                    app_handle.clone(),
                    token.clone(),
                    local_transcriber.clone(),
                ).await?;

                voice_engine.set_state_and_emit(VoiceState::WakeWordListening);
            }
        }
    }
}

async fn handle_wake_word_detection(
    voice_engine: Arc<VoiceEngine>,
    db: Arc<Database>,
    router: AiRouter,
    auth: Arc<GoogleAuth>,
    app_handle: JarvisAppHandle,
    token: CancellationToken,
    local_transcriber: LocalTranscriber,
) -> Result<(), String> {
    voice_engine.set_state_and_emit(VoiceState::WakeWordDetected);
    show_main_window(&app_handle);

    voice_engine
        .audio_router
        .lock()
        .map_err(|e| e.to_string())?
        .mute();

    voice_engine.set_state_and_emit(VoiceState::WakeWordSpeaking);
    let tts = voice_engine.tts.lock().map_err(|e| e.to_string())?.clone();
    if let Err(e) = tts.speak("Yes?").await {
        log::warn!("Wake-word acknowledgement TTS failed: {}", e);
    }

    voice_engine
        .audio_router
        .lock()
        .map_err(|e| e.to_string())?
        .unmute();

    voice_engine.set_state_and_emit(VoiceState::WakeWordProcessing);
    let command_audio = record_command_audio(Arc::clone(&voice_engine), token.clone()).await?;
    if command_audio.is_empty() {
        voice_engine
            .audio_router
            .lock()
            .map_err(|e| e.to_string())?
            .unmute();
        return Ok(());
    }

    voice_engine
        .audio_router
        .lock()
        .map_err(|e| e.to_string())?
        .mute();

    let user_text =
        transcribe_with_fallback(Arc::clone(&voice_engine), local_transcriber, &command_audio).await?;
    if user_text.trim().is_empty() {
        voice_engine
            .audio_router
            .lock()
            .map_err(|e| e.to_string())?
            .unmute();
        return Ok(());
    }

    save_message(&db, "user", &user_text)?;
    let messages = load_recent_messages(&db)?;
    let response = router.send(messages, &db, &auth, &app_handle).await?;
    save_message(&db, "assistant", &response)?;

    voice_engine.set_state_and_emit(VoiceState::WakeWordSpeaking);
    let tts = voice_engine.tts.lock().map_err(|e| e.to_string())?.clone();
    if let Err(e) = tts.speak(&response).await {
        log::warn!("Wake-word response TTS failed: {}", e);
    }

    voice_engine
        .audio_router
        .lock()
        .map_err(|e| e.to_string())?
        .unmute();

    Ok(())
}

async fn transcribe_local_window(
    local_transcriber: LocalTranscriber,
    audio_window: Vec<f32>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || local_transcriber.transcribe(&audio_window))
        .await
        .map_err(|e| format!("Wake-word transcription task failed: {}", e))?
}

async fn transcribe_with_fallback(
    voice_engine: Arc<VoiceEngine>,
    local_transcriber: LocalTranscriber,
    audio_data: &[f32],
) -> Result<String, String> {
    let cloud_transcriber = {
        let guard = voice_engine.transcriber.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };

    if let Some(transcriber) = cloud_transcriber {
        match transcriber.transcribe(audio_data).await {
            Ok(text) if !text.is_empty() => return Ok(text),
            Ok(_) => log::info!("Wake-word cloud STT returned empty text, using local fallback"),
            Err(e) => log::warn!("Wake-word cloud STT failed, using local fallback: {}", e),
        }
    }

    let samples = audio_data.to_vec();
    tauri::async_runtime::spawn_blocking(move || local_transcriber.transcribe(&samples))
        .await
        .map_err(|e| format!("Wake-word local STT task failed: {}", e))?
}

async fn record_command_audio(
    voice_engine: Arc<VoiceEngine>,
    token: CancellationToken,
) -> Result<Vec<f32>, String> {
    voice_engine
        .audio_router
        .lock()
        .map_err(|e| e.to_string())?
        .start_ptt();

    let start = Instant::now();
    let mut heard_speech = false;
    let mut silent_windows = 0u32;

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                return Ok(voice_engine.audio_router.lock().map_err(|e| e.to_string())?.stop_ptt());
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                let samples = voice_engine
                    .audio_router
                    .lock()
                    .map_err(|e| e.to_string())?
                    .peek_ptt();

                let duration_secs = samples.len() as f32 / 16_000.0;
                if duration_secs < 0.35 {
                    continue;
                }

                let trailing_rms = trailing_rms(&samples, 0.35);
                if trailing_rms > 0.02 {
                    heard_speech = true;
                    silent_windows = 0;
                } else if heard_speech && duration_secs > 0.8 {
                    silent_windows += 1;
                }

                if start.elapsed() >= Duration::from_secs(8) || (heard_speech && silent_windows >= 3) {
                    break;
                }
            }
        }
    }

    Ok(voice_engine
        .audio_router
        .lock()
        .map_err(|e| e.to_string())?
        .stop_ptt())
}

fn show_main_window(app_handle: &JarvisAppHandle) {
    if crate::wallpaper::is_active() {
        let _ = crate::wallpaper::raise_for_interaction(app_handle);
    } else {
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

fn save_message(db: &Database, role: &str, content: &str) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO conversations (role, content) VALUES (?1, ?2)",
        rusqlite::params![role, content],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn load_recent_messages(db: &Database) -> Result<Vec<(String, String)>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT role, content FROM conversations ORDER BY id DESC LIMIT 20")
        .map_err(|e| e.to_string())?;
    let mut messages = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    messages.reverse();
    Ok(messages)
}

fn has_speech(samples: &[f32], threshold: f32) -> bool {
    rms(samples) > threshold
}

fn trailing_rms(samples: &[f32], duration_secs: f32) -> f32 {
    let window = (duration_secs * 16_000.0) as usize;
    let start = samples.len().saturating_sub(window);
    rms(&samples[start..])
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let energy = samples
        .iter()
        .map(|sample| sample * sample)
        .sum::<f32>()
        / samples.len() as f32;
    energy.sqrt()
}

fn is_wake_phrase(transcript: &str) -> bool {
    let normalized = transcript
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphabetic() || c.is_ascii_whitespace() { c } else { ' ' })
        .collect::<String>();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.contains("hey jarvis") || compact.contains("hi jarvis") {
        return true;
    }

    let tokens: Vec<&str> = compact.split_whitespace().collect();
    let has_greeting = tokens.iter().any(|token| matches!(*token, "hey" | "hi" | "hay"));
    let has_name = tokens
        .iter()
        .any(|token| levenshtein(token, "jarvis") <= 1 || token.contains("jarvis"));

    has_greeting && has_name
}

fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }

    let b_len = b.chars().count();
    let mut previous: Vec<usize> = (0..=b_len).collect();
    let mut current = vec![0; b_len + 1];

    for (i, a_char) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b.chars().enumerate() {
            let cost = usize::from(a_char != b_char);
            current[j + 1] = (current[j] + 1)
                .min(previous[j + 1] + 1)
                .min(previous[j] + cost);
        }
        previous.clone_from(&current);
    }

    previous[b_len]
}
