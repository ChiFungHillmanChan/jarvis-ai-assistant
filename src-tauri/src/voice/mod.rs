pub mod audio_router;
pub mod commands;
pub mod model_manager;
pub mod transcribe;
pub mod tts;
pub mod wake_commands;
pub mod wake_word;

use audio_router::AudioRouter;
use transcribe::{LocalTranscriber, Transcriber};
use tts::TextToSpeech;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub enum VoiceState {
    Idle,
    Listening,
    Processing,
    Speaking,
    WakeWordListening,
    WakeWordDetected,
    WakeWordProcessing,
    WakeWordSpeaking,
    ModelDownloading(u32),
    Error(String),
    Disabled,
}

pub type JarvisAppHandle = tauri::AppHandle<tauri::Wry>;

pub struct VoiceEngine {
    pub audio_router: Mutex<AudioRouter>,
    pub transcriber: Mutex<Option<Transcriber>>,
    pub tts: Mutex<TextToSpeech>,
    pub state: Mutex<VoiceState>,
    pub muted: Mutex<bool>,
    pub app_handle: Option<JarvisAppHandle>,
    mic_emitter_active: Arc<AtomicBool>,
}

impl VoiceEngine {
    pub fn new(db: &crate::db::Database, app_handle: Option<JarvisAppHandle>) -> Self {
        let mut router = AudioRouter::new();
        if let Err(e) = router.start() {
            log::warn!("AudioRouter start failed (will retry on first use): {}", e);
        }

        let transcriber = match std::env::var("GEMINI_API_KEY") {
            Ok(key) if !key.is_empty() => {
                match Transcriber::new(key) {
                    Ok(t) => { log::info!("Voice STT ready (Gemini API)"); Some(t) }
                    Err(e) => { log::warn!("STT init failed: {}", e); None }
                }
            }
            _ => { log::info!("No GEMINI_API_KEY set. Voice STT disabled."); None }
        };

        let tts = TextToSpeech::from_db(db);
        log::info!("TTS initialized from DB preferences");

        VoiceEngine {
            audio_router: Mutex::new(router),
            transcriber: Mutex::new(transcriber),
            tts: Mutex::new(tts),
            state: Mutex::new(VoiceState::Idle),
            muted: Mutex::new(false),
            app_handle,
            mic_emitter_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_state_and_emit(&self, state: VoiceState) {
        *self.state.lock().unwrap() = state.clone();
        if let Some(app_handle) = &self.app_handle {
            if let Err(e) = app_handle.emit("voice-state", state) {
                log::warn!("Failed to emit voice-state event: {}", e);
            }
        }
    }

    pub fn get_state(&self) -> VoiceState { self.state.lock().unwrap().clone() }

    pub fn is_available(&self) -> bool {
        self.transcriber.lock().unwrap().is_some() || model_manager::is_downloaded()
    }

    pub async fn transcribe_command(&self, audio_data: &[f32]) -> Result<String, String> {
        let cloud_transcriber = {
            let guard = self.transcriber.lock().map_err(|e| e.to_string())?;
            guard.clone()
        };

        if let Some(transcriber) = cloud_transcriber {
            match transcriber.transcribe(audio_data).await {
                Ok(text) if !text.is_empty() => return Ok(text),
                Ok(_) => log::info!("Cloud transcription returned empty text, trying local fallback"),
                Err(e) => log::warn!("Cloud transcription failed, trying local fallback: {}", e),
            }
        }

        if model_manager::is_downloaded() {
            let model_path = model_manager::model_path();
            let samples = audio_data.to_vec();
            let local_result = tauri::async_runtime::spawn_blocking(move || {
                let transcriber = LocalTranscriber::new(&model_path)?;
                transcriber.transcribe(&samples)
            })
            .await
            .map_err(|e| format!("Local Whisper task failed: {}", e))?;

            return local_result;
        }

        Err("Speech-to-text not available. Configure GEMINI_API_KEY or download the local Whisper model.".to_string())
    }

    pub fn toggle_mute(&self) -> bool {
        let mut muted = self.muted.lock().unwrap();
        *muted = !*muted;
        if *muted {
            if let Ok(router) = self.audio_router.lock() {
                router.deactivate();
            }
            self.set_state_and_emit(VoiceState::Disabled);
        } else {
            self.set_state_and_emit(VoiceState::Idle);
        }
        *muted
    }

    /// Start emitting mic-amplitude events at ~20 Hz for frontend visualization.
    pub fn start_mic_emitter(&self) {
        if self.mic_emitter_active.swap(true, Ordering::Relaxed) {
            return; // already running
        }
        let active = self.mic_emitter_active.clone();
        let mic_amp = self.audio_router.lock().unwrap().mic_amplitude.clone();
        let app_handle = self.app_handle.clone();

        tokio::spawn(async move {
            while active.load(Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let amp = f32::from_bits(mic_amp.load(Ordering::Relaxed));
                if let Some(ref h) = app_handle {
                    let _ = h.emit("mic-amplitude", serde_json::json!({ "amplitude": amp }));
                }
            }
        });
    }

    /// Stop emitting mic-amplitude events.
    pub fn stop_mic_emitter(&self) {
        self.mic_emitter_active.store(false, Ordering::Relaxed);
    }
}
