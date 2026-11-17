pub mod audio_router;
pub mod commands;
pub mod cosyvoice;
pub mod cosyvoice_download;
pub mod minimax;
pub mod model_manager;
pub mod recordings;
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

/// Where a spoken request is transcribed in the cloud (P06): the registry's
/// `stt` model for the provider of the AI the user chose, on a wire this
/// build has. No choice yet (installs from before setup asked) counts as
/// Gemini, as before. Any other choice, or a provider the registry names no
/// `stt` model for (OpenAI today), gets None: local Whisper, so no other AI
/// receives the user's voice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeechRoute {
    pub wire: transcribe::SpeechWire,
    pub model: String,
    /// "Gemini 3.7 Flash (Google Gemini)", for Settings.
    pub label: String,
}

fn speech_route(chosen: Option<&crate::commands::ai_choice::CurrentAi>) -> Option<SpeechRoute> {
    let (provider, wire) = match chosen {
        None => ("gemini", transcribe::SpeechWire::Gemini),
        Some(ai) if ai.provider == "gemini" => ("gemini", transcribe::SpeechWire::Gemini),
        Some(ai) if ai.provider == "openai" && ai.base_url.is_none() => ("openai", transcribe::SpeechWire::OpenAi),
        Some(_) => return None,
    };
    let entry = crate::ai::ProviderChainEntry { position: 0, provider_type: provider.into(), endpoint_id: None, model_id: None, enabled: true, base_url: None };
    let target = crate::ai::capability::resolve("stt", &entry, &[]).ready()?;
    Some(SpeechRoute { wire, model: target.model?, label: target.label })
}

/// The cloud route for the AI the user chose, when its key is in place.
fn cloud_route(db: &crate::db::Database) -> Option<(SpeechRoute, String)> {
    let chain = crate::commands::ai_choice::saved_chain(db).ok()?;
    let route = speech_route(crate::commands::ai_choice::describe(&chain).as_ref())?;
    let key = match route.wire {
        transcribe::SpeechWire::Gemini => resolve_key("GEMINI_API_KEY", "gemini_api_key"),
        transcribe::SpeechWire::OpenAi => resolve_key("OPENAI_API_KEY", "openai_api_key"),
    }?;
    Some((route, key))
}

/// Build the cloud speech-to-text transcriber for the AI the user chose
/// ([`speech_route`]), or None when there is none or its key is missing.
///
/// Resolved on every use rather than once at startup: users paste the key into
/// Settings after first launch, which writes it to the Keychain only.
pub fn cloud_transcriber(db: &crate::db::Database) -> Option<Transcriber> {
    let (route, key) = cloud_route(db)?;
    match Transcriber::new(route.wire, key, route.model) {
        Ok(t) => Some(t),
        Err(e) => {
            log::warn!("STT init failed: {}", e);
            None
        }
    }
}

/// Which cloud speech-to-text a spoken request would use, for Settings:
/// the model's label, or None (local Whisper; the download is offered).
#[tauri::command]
pub fn cloud_speech(db: tauri::State<'_, std::sync::Arc<crate::db::Database>>) -> Option<String> {
    cloud_route(&db).map(|(route, _)| route.label)
}

/// Same lookup order as `ai::resolve_key`: process env first, then the
/// Keychain the Settings screen writes to.
fn resolve_key(env_var: &str, account: &str) -> Option<String> {
    pick_key(std::env::var(env_var).ok(), crate::security::keychain::KeychainStore::get(crate::ai::KEY_SERVICE, account).ok())
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
    /// The voice goes to the registry's `stt` model of the chosen AI's
    /// provider and to no other AI: Gemini (or no choice yet) to Gemini's;
    /// OpenAI only once the registry names an OpenAI model; everyone else
    /// to local Whisper.
    #[test]
    fn speech_goes_to_the_stt_model_of_the_chosen_ai_or_stays_on_this_mac() {
        use super::{speech_route, transcribe::SpeechWire};
        use crate::commands::ai_choice::CurrentAi;
        let ai = |provider: &str, base_url: Option<&str>| CurrentAi { provider: provider.into(), label: provider.into(), model: None, base_url: base_url.map(str::to_string) };
        let gemini_stt = crate::ai::registry::current().feature_default("stt", "gemini").unwrap().to_string();
        for chosen in [None, Some(ai("gemini", None))] {
            let route = speech_route(chosen.as_ref()).expect("Gemini transcribes");
            assert_eq!((route.wire, route.model.as_str()), (SpeechWire::Gemini, gemini_stt.as_str()), "installs from before setup asked keep Gemini speech");
        }
        for other in [ai("ollama", None), ai("openai", None), ai("claude", None), ai("compatible", Some("https://api.moonshot.ai/v1"))] {
            assert_eq!(speech_route(Some(&other)), None, "{} users get local Whisper", other.provider);
        }

        let _registry = crate::ai::registry::test_override::with(|r| {
            r.features.get_mut("stt").unwrap().insert("openai".into(), "transcribe-model".into());
        });
        let route = speech_route(Some(&ai("openai", None))).expect("a registry that names an OpenAI model turns it on");
        assert_eq!((route.wire, route.model.as_str()), (SpeechWire::OpenAi, "transcribe-model"));
        assert_eq!(speech_route(Some(&ai("openai", Some("https://api.moonshot.ai/v1")))), None, "a compatible service is not OpenAI");
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
