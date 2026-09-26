pub mod audio_router;
pub mod commands;
pub mod cosyvoice;
pub mod cosyvoice_download;
pub mod minimax;
pub mod model_manager;
pub mod transcribe;
pub mod tts;
pub mod tts_router;
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
    /// Cached local Whisper context. Loading `ggml-base.bin` costs seconds, so
    /// it is built once and shared into every `spawn_blocking` call.
    local_transcriber: Mutex<Option<LocalTranscriber>>,
    pub tts: Mutex<TextToSpeech>,
    pub state: Mutex<VoiceState>,
    pub muted: Mutex<bool>,
    pub app_handle: Option<JarvisAppHandle>,
    pub db: Arc<crate::db::Database>,
    mic_emitter_active: Arc<AtomicBool>,
}

impl VoiceEngine {
    pub fn new(db: &Arc<crate::db::Database>, app_handle: Option<JarvisAppHandle>) -> Self {
        // Do NOT open the mic here. Starting the input stream at construction
        // triggered the macOS microphone prompt on first launch and left the
        // recording indicator lit for the whole session, even though
        // wake_word_enabled defaults to false. WakeWordService::enable and the
        // push-to-talk path both start the router on demand.
        let router = AudioRouter::new();

        let tts = TextToSpeech::from_db(&**db);
        log::info!("TTS initialized from DB preferences");

        VoiceEngine {
            audio_router: Mutex::new(router),
            local_transcriber: Mutex::new(None),
            tts: Mutex::new(tts),
            state: Mutex::new(VoiceState::Idle),
            muted: Mutex::new(false),
            app_handle,
            db: Arc::clone(db),
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
        cloud_transcriber(&self.db).is_some() || model_manager::is_downloaded()
    }

    /// Drop the cached local Whisper context so the next transcription reloads
    /// the model file. Called after the model is (re-)downloaded.
    pub fn invalidate_local_transcriber(&self) {
        if let Ok(mut guard) = self.local_transcriber.lock() {
            *guard = None;
        }
    }

    /// Load the local Whisper model once and reuse it for every utterance.
    async fn local_transcriber(&self) -> Result<LocalTranscriber, String> {
        let cached = self
            .local_transcriber
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        if let Some(transcriber) = cached {
            return Ok(transcriber);
        }

        let model_path = model_manager::model_path();
        let built = tauri::async_runtime::spawn_blocking(move || LocalTranscriber::new(&model_path))
            .await
            .map_err(|e| format!("Local Whisper load task failed: {}", e))??;
        *self.local_transcriber.lock().map_err(|e| e.to_string())? = Some(built.clone());
        Ok(built)
    }

    pub async fn transcribe_command(&self, audio_data: &[f32]) -> Result<String, String> {
        if let Some(transcriber) = cloud_transcriber(&self.db) {
            match transcriber.transcribe(audio_data, &self.db).await {
                Ok(text) if !text.is_empty() => return Ok(text),
                Ok(_) => log::info!("Cloud transcription returned empty text, trying local fallback"),
                Err(e) => log::warn!("Cloud transcription failed, trying local fallback: {}", e),
            }
        }

        if model_manager::is_downloaded() {
            let transcriber = self.local_transcriber().await?;
            let samples = audio_data.to_vec();
            let db = Arc::clone(&self.db);
            let local_result = tauri::async_runtime::spawn_blocking(move || {
                transcriber.transcribe(&samples, &db)
            })
            .await
            .map_err(|e| format!("Local Whisper task failed: {}", e))?;

            return local_result;
        }

        Err("語音輸入需要本機語音模型：請到「設定 → 語音」下載（約 148 MB，語音只在此 Mac 處理）。".to_string())
    }

    pub fn toggle_mute(&self) -> bool {
        let mut muted = self.muted.lock().unwrap();
        *muted = !*muted;
        if *muted {
            if let Ok(mut router) = self.audio_router.lock() {
                // Close the stream, not just the routing: leaving it open keeps
                // the macOS recording indicator lit while the user thinks the
                // microphone is off.
                router.stop();
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

/// Gemini hears the user only when Gemini is the AI they chose, or when they
/// have not chosen one (installs from before setup asked). Anyone else gets
/// the local Whisper model, so no other AI receives their voice.
fn speech_allowed_for(chosen: Option<&crate::commands::ai_choice::CurrentAi>) -> bool {
    chosen.is_none_or(|ai| ai.provider == "gemini")
}

/// Build the cloud speech-to-text transcriber from the currently configured
/// Gemini key, or return None when there is none or Gemini is not the chosen AI.
///
/// Resolved on every use rather than once at startup: users paste the key into
/// Settings after first launch, which writes it to the Keychain only.
pub fn cloud_transcriber(db: &crate::db::Database) -> Option<Transcriber> {
    let chain = crate::commands::ai_choice::saved_chain(db).ok()?;
    if !speech_allowed_for(crate::commands::ai_choice::describe(&chain).as_ref()) {
        return None;
    }
    let key = resolve_gemini_key()?;
    match Transcriber::new(key) {
        Ok(t) => Some(t),
        Err(e) => {
            log::warn!("STT init failed: {}", e);
            None
        }
    }
}

/// Same lookup order as `ai::resolve_key`: process env first, then the
/// Keychain the Settings screen writes to.
fn resolve_gemini_key() -> Option<String> {
    pick_key(
        std::env::var("GEMINI_API_KEY").ok(),
        crate::security::keychain::KeychainStore::get(crate::ai::KEY_SERVICE, "gemini_api_key")
            .ok(),
    )
}

/// Env value wins over the Keychain value; a blank value counts as absent.
/// Split out from the Keychain call so the precedence rule is testable.
fn pick_key(env_value: Option<String>, keychain_value: Option<String>) -> Option<String> {
    match env_value {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => keychain_value.filter(|v| !v.trim().is_empty()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_gemini_choice_or_no_choice_sends_speech_to_gemini() {
        use super::speech_allowed_for;
        use crate::commands::ai_choice::CurrentAi;
        let ai = |provider: &str| CurrentAi { provider: provider.into(), label: provider.into(), model: None, base_url: None };
        assert!(speech_allowed_for(None), "installs from before setup asked keep Gemini speech");
        assert!(speech_allowed_for(Some(&ai("gemini"))));
        for other in ["ollama", "openai", "claude", "compatible"] {
            assert!(!speech_allowed_for(Some(&ai(other))), "{other} users get local Whisper");
        }
    }

    use super::pick_key;

    #[test]
    fn keychain_key_is_used_when_no_env_var_is_set() {
        // The shipped-app case: no .env, key pasted into Settings.
        assert_eq!(
            pick_key(None, Some("from-keychain".to_string())),
            Some("from-keychain".to_string())
        );
    }

    #[test]
    fn env_key_wins_over_keychain_and_blanks_are_ignored() {
        assert_eq!(
            pick_key(Some("from-env".to_string()), Some("from-keychain".to_string())),
            Some("from-env".to_string())
        );
        assert_eq!(
            pick_key(Some("  ".to_string()), Some("from-keychain".to_string())),
            Some("from-keychain".to_string())
        );
        assert_eq!(pick_key(None, Some("  ".to_string())), None);
        assert_eq!(pick_key(None, None), None);
    }
}
