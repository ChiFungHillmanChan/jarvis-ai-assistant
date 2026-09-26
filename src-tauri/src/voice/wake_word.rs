use crate::ai::{AiRouter, Workload};
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::voice::model_manager;
use crate::voice::transcribe::LocalTranscriber;
use crate::voice::tts::StreamingTts;
use crate::voice::{JarvisAppHandle, VoiceEngine, VoiceState};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::async_runtime::JoinHandle;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

/// Deliberately holds no `AiRouter`. It used to keep a by-value clone taken at
/// startup, so an API key pasted or a provider chain edited afterwards never
/// reached "Hey Jarvis" -- the wake word kept talking to the snapshot from
/// launch and answered "No API key configured" long after the key was saved.
/// The live router is read out of managed state per turn instead; see
/// `current_router`.
pub struct WakeWordService {
    task: AsyncMutex<Option<JoinHandle<()>>>,
    cancellation_token: Mutex<CancellationToken>,
    voice_engine: Arc<VoiceEngine>,
    db: Arc<Database>,
    auth: Arc<GoogleAuth>,
    app_handle: JarvisAppHandle,
}

impl WakeWordService {
    pub fn new(
        voice_engine: Arc<VoiceEngine>,
        db: Arc<Database>,
        auth: Arc<GoogleAuth>,
        app_handle: JarvisAppHandle,
    ) -> Self {
        Self {
            task: AsyncMutex::new(None),
            cancellation_token: Mutex::new(CancellationToken::new()),
            voice_engine,
            db,
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

        // Loading ggml-base.bin is a synchronous ~148 MB read plus whisper
        // context init -- keep it off the async runtime.
        let model_path = model_manager::model_path();
        let local_transcriber =
            tauri::async_runtime::spawn_blocking(move || LocalTranscriber::new(&model_path))
                .await
                .map_err(|e| format!("Wake-word model load task failed: {}", e))??;

        {
            let mut router = self
                .voice_engine
                .audio_router
                .lock()
                .map_err(|e| e.to_string())?;
            if !router.is_alive() {
                router.start()?;
            }
            router.set_wake_word_resting(true);
            router.unmute();
        }

        let token = {
            let mut guard = self.cancellation_token.lock().map_err(|e| e.to_string())?;
            *guard = CancellationToken::new();
            guard.clone()
        };

        self.voice_engine
            .set_state_and_emit(VoiceState::WakeWordListening);

        let voice_engine = Arc::clone(&self.voice_engine);
        let db = Arc::clone(&self.db);
        let auth = Arc::clone(&self.auth);
        let app_handle = self.app_handle.clone();

        let handle = tauri::async_runtime::spawn(async move {
            supervisor_loop(
                voice_engine,
                db,
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

        let ptt_active = matches!(self.voice_engine.get_state(), VoiceState::Listening);
        {
            let mut router = self
                .voice_engine
                .audio_router
                .lock()
                .map_err(|e| e.to_string())?;
            router.set_wake_word_resting(false);
            // Close the stream, not just the routing, so the macOS recording
            // indicator goes out. Skipped while push-to-talk is capturing --
            // stop_listening releases the stream when that session ends.
            if !ptt_active {
                router.stop();
            }
        }

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

/// Clone the live `AiRouter` out of managed state for one turn.
///
/// Cloning out of the lock, never holding it across an `.await`, is the rule
/// every other caller of the managed router follows.
fn current_router(app_handle: &JarvisAppHandle) -> Result<AiRouter, String> {
    let managed = app_handle
        .try_state::<Mutex<AiRouter>>()
        .ok_or_else(|| "AI router is not available yet".to_string())?;
    let router = managed.lock().map_err(|e| e.to_string())?.clone();
    Ok(router)
}

async fn supervisor_loop(
    voice_engine: Arc<VoiceEngine>,
    db: Arc<Database>,
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
                            log::error!("Wake-word audio reconnect failed: {}", reconnect_err);
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
    auth: Arc<GoogleAuth>,
    app_handle: JarvisAppHandle,
    token: CancellationToken,
    local_transcriber: LocalTranscriber,
) -> Result<(), String> {
    voice_engine.set_state_and_emit(VoiceState::WakeWordListening);
    let mut gate = SpeechGate::new();

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

                let audio_window = {
                    let audio_router = voice_engine
                        .audio_router
                        .lock()
                        .map_err(|e| e.to_string())?;
                    // A cpal Stream outlives its device: when the AirPods go
                    // away the callback stops firing, the ring's write position
                    // freezes and read_ring returns the same stale 2 s window
                    // forever. Surfacing it as an error is what makes the
                    // supervisor's reconnect branch reachable at all.
                    if !audio_router.is_alive() {
                        return Err(
                            "Audio capture stopped -- input device changed or unavailable"
                                .to_string(),
                        );
                    }
                    audio_router.read_ring(2.0)
                };

                if audio_window.is_empty() {
                    log::debug!("Wake word: empty audio window -- stream may not be active");
                    gate.reset();
                    continue;
                }

                // Judged in short sub-frames rather than as one 2 s average: a
                // wake phrase fills only ~0.7 s of the window, so averaging it
                // against 1.3 s of room noise pulls it back down to the floor.
                let levels = sub_frame_levels(&audio_window);
                let peak_rms = levels.iter().copied().fold(0.0f32, f32::max);
                if !gate.admit(&levels, Instant::now()) {
                    continue;
                }

                let transcript = transcribe_local_window(local_transcriber.clone(), audio_window).await?;

                // Whisper ran for a few hundred milliseconds; a push-to-talk
                // session may have started in that window. Acting now would
                // reset its recording buffer.
                if !matches!(voice_engine.get_state(), VoiceState::WakeWordListening | VoiceState::Idle) {
                    continue;
                }

                if transcript.is_empty() {
                    continue;
                }
                log::debug!("Wake word check (peak RMS {:.4}): {} characters", peak_rms, transcript.chars().count());
                if !is_wake_phrase(&transcript) {
                    continue;
                }

                handle_wake_word_detection(
                    Arc::clone(&voice_engine),
                    Arc::clone(&db),
                    Arc::clone(&auth),
                    app_handle.clone(),
                    token.clone(),
                    local_transcriber.clone(),
                ).await?;

                // Capture was muted for the whole exchange, so the ring still
                // holds the phrase that fired it. Hold off until it has rolled
                // over rather than waking on the user's own words again.
                gate.note_detection(Instant::now());
                voice_engine.set_state_and_emit(VoiceState::WakeWordListening);
            }
        }
    }
}

async fn handle_wake_word_detection(
    voice_engine: Arc<VoiceEngine>,
    db: Arc<Database>,
    auth: Arc<GoogleAuth>,
    app_handle: JarvisAppHandle,
    token: CancellationToken,
    local_transcriber: LocalTranscriber,
) -> Result<(), String> {
    let _assistant_guard = match crate::chat_session::FOREGROUND.try_lock() {
        Ok(guard) => guard,
        Err(_) => return Ok(()),
    };
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
    let _ = app_handle.emit("chat-voice-active", json!({"active": true}));
    let _ = app_handle.emit(
        "chat-status",
        json!({
            "status": "Transcribing your request...",
            "phase": "transcribing"
        }),
    );

    let user_text =
        transcribe_with_fallback(local_transcriber, &command_audio, Arc::clone(&db)).await?;
    if user_text.trim().is_empty() {
        let _ = app_handle.emit("chat-voice-active", json!({"active": false}));
        voice_engine
            .audio_router
            .lock()
            .map_err(|e| e.to_string())?
            .unmute();
        return Ok(());
    }

    // Show transcribed text in chat and signal AI is thinking
    let _ = app_handle.emit(
        "chat-new-message",
        json!({
            "role": "user", "content": &user_text
        }),
    );
    let _ = app_handle.emit("chat-state", json!({"state": "thinking"}));
    let _ = app_handle.emit(
        "chat-status",
        json!({
            "status": "Understanding your request...",
            "phase": "thinking"
        }),
    );

    save_message(&db, "user", &user_text)?;
    let messages = load_recent_messages(&db)?;

    // Create streaming TTS -- speaks sentences as AI tokens arrive
    voice_engine.set_state_and_emit(VoiceState::WakeWordSpeaking);
    let streaming_tts = {
        let tts = voice_engine.tts.lock().map_err(|e| e.to_string())?.clone();
        StreamingTts::new(tts, app_handle.clone())
    };
    let tts_tx = Some(streaming_tts.sender());

    // Read per turn, so a key or chain saved after launch is in force on the
    // very next "Hey Jarvis" rather than after a relaunch.
    let router = current_router(&app_handle)?;
    let response = match router.send(messages, &db, &auth, &app_handle, tts_tx, Workload::Tools).await {
        Ok(r) => r,
        Err(e) => {
            let _ = app_handle.emit("chat-voice-active", json!({"active": false}));
            let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
            return Err(e);
        }
    };
    // The assistant turn goes through the shared guard, never the raw insert:
    // a provider that returns no text still yields Ok("") here, and a blank row
    // renders as an empty bubble and replays as an empty message part.
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        crate::commands::chat::persist_assistant_reply(&conn, &response)
            .map_err(|e| e.to_string())?;
    }

    // Show assistant response in chat
    let _ = app_handle.emit(
        "chat-new-message",
        json!({
            "role": "assistant", "content": &response
        }),
    );

    // Finish streaming TTS -- speaks any remaining buffered text
    streaming_tts.finish().await;
    // Unconditional: the HUD must return to idle here regardless of whether
    // the streaming consumer's own "idle" emit ran.
    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));

    voice_engine
        .audio_router
        .lock()
        .map_err(|e| e.to_string())?
        .unmute();
    let _ = app_handle.emit("chat-voice-active", json!({"active": false}));

    Ok(())
}

/// "Hey Jarvis" is an English phrase, so the wake window is transcribed as
/// English whatever the user dictates in; see the test of the same name.
const WAKE_PHRASE_LANGUAGE: Option<&str> = Some("en");

async fn transcribe_local_window(
    local_transcriber: LocalTranscriber,
    audio_window: Vec<f32>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || local_transcriber.transcribe_as(&audio_window, WAKE_PHRASE_LANGUAGE))
        .await
        .map_err(|e| format!("Wake-word transcription task failed: {}", e))?
}

async fn transcribe_with_fallback(
    local_transcriber: LocalTranscriber,
    audio_data: &[f32],
    db: Arc<crate::db::Database>,
) -> Result<String, String> {
    if let Some(transcriber) = crate::voice::cloud_transcriber(&db) {
        match transcriber.transcribe(audio_data, &db).await {
            Ok(text) if !text.is_empty() => return Ok(text),
            Ok(_) => log::info!("Wake-word cloud STT returned empty text, using local fallback"),
            Err(e) => log::warn!("Wake-word cloud STT failed, using local fallback: {}", e),
        }
    }

    let samples = audio_data.to_vec();
    tauri::async_runtime::spawn_blocking(move || local_transcriber.transcribe(&samples, &db))
        .await
        .map_err(|e| format!("Wake-word local STT task failed: {}", e))?
}

/// Longest request after "Yes?". Just inside the push-to-talk buffer, which
/// keeps only its newest 30 s at 48 kHz (`AudioRouter::new`).
const COMMAND_MAX_DURATION: Duration = Duration::from_secs(28);
/// Give up when nothing is said after "Yes?".
const COMMAND_NO_SPEECH_TIMEOUT: Duration = Duration::from_secs(8);
/// Quiet 250 ms polls in a row that end a request: 1.5 s, so a pause to think
/// mid-sentence does not send half of it.
const COMMAND_END_QUIET_POLLS: u32 = 6;

fn command_finished(elapsed: Duration, heard_speech: bool, quiet_polls: u32) -> bool {
    if !heard_speech {
        return elapsed >= COMMAND_NO_SPEECH_TIMEOUT;
    }
    quiet_polls >= COMMAND_END_QUIET_POLLS || elapsed >= COMMAND_MAX_DURATION
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

                if command_finished(start.elapsed(), heard_speech, silent_windows) {
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
        .prepare("SELECT role, content FROM conversations WHERE session_id = 'legacy' ORDER BY id DESC LIMIT 20")
        .map_err(|e| e.to_string())?;
    let mut messages = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    messages.reverse();
    Ok(messages)
}

/// Sub-frames quieter than this are never transcribed, whatever the tracked
/// noise floor has settled on -- the floor of a silent room is not a speech
/// reference.
const ABSOLUTE_SPEECH_FLOOR: f32 = 0.008;
/// How far above the tracked noise floor a sub-frame has to stand to count as
/// speech. Steady noise sits *at* the floor by definition, so it never clears
/// this; a phrase spoken into the microphone is several times the floor.
const SPEECH_OVER_NOISE: f32 = 2.0;
/// Length of the sub-frames a window is judged in. Short enough that a ~0.7 s
/// wake phrase fills at least two of them whole, long enough that one frame's
/// RMS is a level rather than a sample of the waveform. `read_ring` always
/// returns 16 kHz audio, so this is a fixed sample count.
const SUB_FRAME_SAMPLES: usize = 4_000;
/// Floor estimate used before any window has been observed. Chosen so the very
/// first window is judged against `ABSOLUTE_SPEECH_FLOOR` alone: a gate that
/// cannot fire on its first window misses a wake word spoken the moment it is
/// switched on.
const INITIAL_NOISE_FLOOR: f32 = ABSOLUTE_SPEECH_FLOOR / SPEECH_OVER_NOISE;
/// Weight given to a window whose quiet level is above the current estimate.
/// Deliberately small, so a spoken phrase does not drag the floor up over
/// itself; a fan that switches on is sustained and is still tracked within a
/// few seconds.
const NOISE_FLOOR_ATTACK: f32 = 0.12;
/// Weight given to a window whose quiet level is below the estimate. Larger
/// than the attack so the gate re-arms quickly once a room goes quiet again.
const NOISE_FLOOR_DECAY: f32 = 0.4;
/// Suppression after a *confirmed* wake word, long enough for the 2 s ring to
/// refill with audio recorded after the exchange. Nothing else in the gate
/// blocks on elapsed time.
const POST_DETECTION_COOLDOWN: Duration = Duration::from_millis(2000);

/// Cheap pre-gate in front of the Whisper pass.
///
/// Running base-model inference on every 800 ms window burns CPU continuously
/// in any room that is not silent, and a fixed RMS threshold cannot tell a fan
/// from a voice: the fan simply sits above it forever.
///
/// So the gate tracks the room's own noise floor and admits a window only when
/// it stands clear of that floor. Steady noise stops producing passes entirely
/// once the floor has adapted to it, which takes a few seconds.
///
/// Admitting a window deliberately changes nothing that can block the next one.
/// The previous gate counted consecutive windows and reset that count on every
/// pass, so under sustained noise it alternated pass/block -- and a wake phrase
/// landing on a blocked window was never transcribed at all. The only state
/// that can block is the cooldown after a *confirmed* detection.
///
/// A window is judged sub-frame by sub-frame, not by its overall RMS. The
/// window is 2 s and a wake phrase is about 0.7 s of it, so its energy averaged
/// over the whole window comes back down towards the floor: a phrase three
/// times the room noise averages to roughly 1.9x it, under the 2x the gate
/// asks for. Judged per sub-frame the phrase stands at its own level, and the
/// floor learns from the window's quietest sub-frame -- the part that is *not*
/// speech -- so a phrase can never teach the floor to ignore itself.
struct SpeechGate {
    noise_floor: Option<f32>,
    suppressed_until: Option<Instant>,
}

impl SpeechGate {
    fn new() -> Self {
        SpeechGate {
            noise_floor: None,
            suppressed_until: None,
        }
    }

    /// Forget the adapted floor. Used when capture is interrupted: the room the
    /// estimate describes is no longer the room being recorded.
    fn reset(&mut self) {
        self.noise_floor = None;
    }

    /// Suppress transcription briefly after a confirmed wake word. The ring
    /// still holds the phrase that just fired -- capture is muted for the whole
    /// exchange, so its contents do not advance -- and re-reading it would
    /// re-trigger the detector on the user's own words.
    fn note_detection(&mut self, now: Instant) {
        self.suppressed_until = Some(now + POST_DETECTION_COOLDOWN);
    }

    /// Returns true when this window should be transcribed, given the RMS of
    /// each of its sub-frames (see `sub_frame_levels`).
    fn admit(&mut self, levels: &[f32], now: Instant) -> bool {
        if levels.is_empty() {
            return false;
        }
        if let Some(until) = self.suppressed_until {
            if now < until {
                // Stale audio from the exchange that just finished. Do not let
                // it teach the noise floor either.
                return false;
            }
            self.suppressed_until = None;
        }

        // The loudest sub-frame decides: that is where the phrase is, if there
        // is one. The quietest decides what the room sounds like.
        let peak = levels.iter().copied().fold(0.0f32, f32::max);
        let quiet = levels.iter().copied().fold(f32::INFINITY, f32::min);

        let floor = self.noise_floor.unwrap_or(INITIAL_NOISE_FLOOR);
        let speech = peak >= ABSOLUTE_SPEECH_FLOOR && peak > floor * SPEECH_OVER_NOISE;

        // The floor tracks the room whether or not the window was admitted --
        // otherwise noise loud enough to pass would never be learned -- but it
        // learns from the quietest sub-frame, never from the peak, so it does
        // not climb over the speech it exists to detect.
        let weight = if quiet > floor {
            NOISE_FLOOR_ATTACK
        } else {
            NOISE_FLOOR_DECAY
        };
        self.noise_floor = Some(floor + (quiet - floor) * weight);

        speech
    }
}

/// RMS of each sub-frame of `samples`.
///
/// A trailing partial frame shorter than half a frame is dropped: too few
/// samples to level-detect on, and a short frame's RMS swings wildly. The first
/// frame is always full length, so the result is never empty for non-empty
/// input.
fn sub_frame_levels(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let frame = SUB_FRAME_SAMPLES.min(samples.len());
    samples
        .chunks(frame)
        .filter(|chunk| chunk.len() * 2 >= frame)
        .map(|chunk| rms(chunk))
        .collect()
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

    let energy = samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
    energy.sqrt()
}

fn is_wake_phrase(transcript: &str) -> bool {
    let normalized = transcript
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() || c.is_ascii_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.contains("hey jarvis") || compact.contains("hi jarvis") {
        return true;
    }

    let tokens: Vec<&str> = compact.split_whitespace().collect();
    let has_greeting = tokens
        .iter()
        .any(|token| matches!(*token, "hey" | "hi" | "hay"));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2 s window at 16 kHz, `noise` throughout, with `burst_secs` of `burst`
    /// in the middle -- the shape of a wake phrase spoken in a room with a fan.
    fn synthetic_window(noise: f32, burst: f32, burst_secs: f32) -> Vec<f32> {
        let mut samples = vec![noise; 32_000];
        let len = (burst_secs * 16_000.0) as usize;
        let start = (samples.len() - len) / 2;
        for sample in samples[start..start + len].iter_mut() {
            *sample = burst;
        }
        samples
    }

    /// Feed `windows` identical 800 ms windows at `level` and count the passes.
    fn feed(gate: &mut SpeechGate, level: f32, windows: u32, start: Instant) -> u32 {
        (0..windows)
            .filter(|k| gate.admit(&[level; 8], start + Duration::from_millis(800 * u64::from(*k))))
            .count() as u32
    }

    #[test]
    fn steady_noise_stops_driving_whisper_passes() {
        // F075: room noise above the old fixed 0.008 threshold ran a full
        // ggml-base pass on every cycle, forever. The floor has to learn the
        // room and then stay quiet -- halving the rate is not a fix.
        let t0 = Instant::now();
        let mut gate = SpeechGate::new();

        let early = feed(&mut gate, 0.02, 10, t0);
        assert!(
            early <= 6,
            "the floor took too long to adapt: {} passes in the first 10 windows",
            early
        );

        let settled = feed(&mut gate, 0.02, 30, t0 + Duration::from_secs(8));
        assert_eq!(
            settled, 0,
            "steady room noise must stop producing transcriptions entirely"
        );
    }

    #[test]
    fn a_pass_never_blocks_the_window_after_it() {
        // REG09: the old gate reset its consecutive counter on every pass, so
        // under sustained noise it alternated pass/block, and a wake phrase
        // landing on a blocked window was silently never transcribed. Both
        // windows of a two-window phrase must be admitted, every time.
        let t0 = Instant::now();
        let mut gate = SpeechGate::new();
        let mut now = t0;
        let step = Duration::from_millis(800);

        // Let the floor settle on the room first.
        for _ in 0..30 {
            gate.admit(&[0.02; 8], now);
            now += step;
        }

        for round in 0..5 {
            assert!(
                gate.admit(&[0.10; 8], now),
                "round {}: first window of the phrase was blocked",
                round
            );
            now += step;
            assert!(
                gate.admit(&[0.10; 8], now),
                "round {}: second window blocked purely because the first passed",
                round
            );
            now += step;
            for _ in 0..4 {
                gate.admit(&[0.02; 8], now);
                now += step;
            }
        }
    }

    #[test]
    fn near_silence_never_reaches_whisper() {
        // However low the floor decays, a window under the absolute floor is
        // not speech.
        let t0 = Instant::now();
        let mut gate = SpeechGate::new();
        for k in 0..20u32 {
            assert!(!gate.admit(&[0.0005; 8], t0 + Duration::from_millis(800 * u64::from(k))));
        }
    }

    #[test]
    fn a_confirmed_detection_suppresses_the_stale_ring() {
        let t0 = Instant::now();
        let mut gate = SpeechGate::new();
        assert!(gate.admit(&[0.10; 8], t0));
        gate.note_detection(t0);

        assert!(
            !gate.admit(&[0.10; 8], t0 + Duration::from_millis(800)),
            "the ring still holds the phrase that just fired"
        );
        assert!(!gate.admit(&[0.10; 8], t0 + Duration::from_millis(1600)));
        assert!(
            gate.admit(
                &[0.10; 8],
                t0 + POST_DETECTION_COOLDOWN + Duration::from_millis(1)
            ),
            "the cooldown must expire on its own"
        );
    }

    #[test]
    fn sub_frame_levels_isolate_a_short_burst() {
        // The measurement the gate now depends on: the burst has to show up as
        // its own level, not be averaged into the window it sits in.
        let window = synthetic_window(0.001, 0.06, 0.5);
        let levels = sub_frame_levels(&window);
        assert_eq!(levels.len(), 8, "2 s of 16 kHz audio is eight 0.25 s frames");

        let peak = levels.iter().copied().fold(0.0f32, f32::max);
        let quiet = levels.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(
            peak > 0.05,
            "the burst must survive as a sub-frame level: {}",
            peak
        );
        assert!(
            quiet < 0.002,
            "the quiet part must stay quiet so the floor can learn it: {}",
            quiet
        );
        assert!(
            rms(&window) < 0.04,
            "the whole-window average is what hid the burst: {}",
            rms(&window)
        );
    }

    #[test]
    fn a_short_phrase_in_a_mostly_quiet_window_is_admitted() {
        // R2-04: a wake phrase is ~0.7 s of the 2 s window. Judged as one
        // average, 0.7 s at 0.06 inside 2 s of 0.02 room noise comes out at
        // 0.039 -- under 2x the 0.02 floor -- so the gate rejected exactly the
        // windows it exists to catch, at every tick, for as long as the fan ran.
        let t0 = Instant::now();
        let mut gate = SpeechGate::new();
        let mut now = t0;
        let step = Duration::from_millis(800);
        let room = vec![0.02f32; 32_000];
        let phrase = synthetic_window(0.02, 0.06, 0.7);

        assert!(
            rms(&phrase) < 0.02 * SPEECH_OVER_NOISE,
            "precondition: the whole-window average does not clear the gate ({} vs {})",
            rms(&phrase),
            0.02 * SPEECH_OVER_NOISE
        );

        // Let the floor settle on the room first.
        for _ in 0..30 {
            gate.admit(&sub_frame_levels(&room), now);
            now += step;
        }

        for round in 0..5 {
            assert!(
                gate.admit(&sub_frame_levels(&phrase), now),
                "round {}: the phrase was averaged away against the room",
                round
            );
            now += step;
            // Repeated because the floor must learn from the quiet part of the
            // window: if it learned from the phrase it would climb over it and
            // later rounds would start failing.
            for _ in 0..3 {
                gate.admit(&sub_frame_levels(&room), now);
                now += step;
            }
        }
    }

    #[test]
    fn uniform_room_noise_is_never_admitted() {
        // The other half of R2-04's fix: judging on the loudest sub-frame must
        // not turn every noisy window into a Whisper pass. Steady noise has no
        // sub-frame that stands out from the rest of the window.
        let t0 = Instant::now();
        let mut gate = SpeechGate::new();
        let mut now = t0;
        let step = Duration::from_millis(800);
        let room = vec![0.02f32; 32_000];

        for _ in 0..30 {
            gate.admit(&sub_frame_levels(&room), now);
            now += step;
        }

        for k in 0..10 {
            assert!(
                !gate.admit(&sub_frame_levels(&room), now),
                "window {}: uniform room noise must not reach whisper",
                k
            );
            now += step;
        }
    }

    /// The wake-word turn is the third assistant-reply persist site, and it is
    /// the one that reaches every provider in the chain through `Workload::Tools`:
    /// Claude, OpenAI, Bedrock and the local backends can all return `Ok("")`,
    /// which used to be inserted verbatim and render as an empty chat bubble.
    /// The compiler cannot see that this call site skipped the shared guard, so
    /// the wiring is checked as a property of the source text. The search is
    /// bounded to this one function's body, so the assertions below cannot
    /// satisfy themselves.
    /// "Hey Jarvis" is English whatever language the user dictates in. Whisper
    /// told to expect Cantonese writes the phrase in Chinese characters, which
    /// `is_wake_phrase` (Latin letters only) never matches, so the wake check
    /// must not follow `stt_language`. Checked on the source like the guard
    /// test below: the model is not available to a unit test.
    #[test]
    fn the_wake_phrase_is_transcribed_as_english_whatever_the_speech_language() {
        assert_eq!(WAKE_PHRASE_LANGUAGE, Some("en"));
        let source = include_str!("wake_word.rs");
        let start = source
            .find("async fn transcribe_local_window(")
            .expect("transcribe_local_window must exist");
        let len = source[start..]
            .find("\nasync fn transcribe_with_fallback(")
            .expect("transcribe_with_fallback must follow transcribe_local_window");
        let body = &source[start..start + len];
        assert!(
            body.contains("transcribe_as(") && body.contains("WAKE_PHRASE_LANGUAGE"),
            "the wake window must be transcribed in WAKE_PHRASE_LANGUAGE, not the dictation language"
        );
    }

    #[test]
    fn a_pause_shorter_than_one_and_a_half_seconds_does_not_end_the_request() {
        let elapsed = Duration::from_secs(4);
        assert!(!command_finished(elapsed, true, 5));
        assert!(command_finished(elapsed, true, 6));
    }

    #[test]
    fn a_request_longer_than_eight_seconds_is_not_cut_off() {
        assert!(!command_finished(Duration::from_secs(20), true, 0));
        assert!(command_finished(COMMAND_MAX_DURATION, true, 0));
    }

    #[test]
    fn saying_nothing_after_the_prompt_still_gives_up_after_eight_seconds() {
        assert!(!command_finished(Duration::from_secs(7), false, 0));
        assert!(command_finished(Duration::from_secs(8), false, 0));
    }

    #[test]
    fn the_wake_word_assistant_turn_goes_through_the_shared_guard() {
        let source = include_str!("wake_word.rs");
        let start = source
            .find("async fn handle_wake_word_detection(")
            .expect("handle_wake_word_detection must exist");
        let len = source[start..]
            .find("\nasync fn transcribe_local_window(")
            .expect("transcribe_local_window must follow handle_wake_word_detection");
        let body = &source[start..start + len];

        assert!(
            body.contains("persist_assistant_reply"),
            "the wake-word flow must persist the assistant turn through persist_assistant_reply"
        );
        assert!(
            !body.contains("save_message(&db, \"assistant\""),
            "the wake-word flow must not insert the assistant turn with the unguarded save_message"
        );
    }
}
