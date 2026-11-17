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
        matches!(cloud_speech_for(&self.db), CloudSpeech::Ready(..)) || model_manager::is_downloaded()
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

    /// A spoken request: at most one cloud attempt, on the chosen AI's
    /// speech-to-text, then Whisper on this Mac if it is downloaded (DECISIONS
    /// P06, 2026-11-17). Never another cloud service, never from this Mac to
    /// the cloud, and never to a service whose terms wait for the user's
    /// agreement.
    pub async fn transcribe_command(&self, audio_data: &[f32]) -> Result<String, String> {
        let whisper = model_manager::is_downloaded();
        let cloud = cloud_speech_for(&self.db);
        match first_step(&cloud, whisper, crate::ai::bedrock::UiLanguage::read(&self.db)) {
            Step::Fail(reason) => {
                // Nothing on this Mac can transcribe it: the card, with the reason.
                if let CloudSpeech::WaitsForConsent(_, request) = &cloud {
                    request.raise("stt");
                }
                return Err(reason);
            }
            Step::Local => {}
            Step::Cloud => {
                if let CloudSpeech::Ready(route, key) = cloud {
                    let result = match Transcriber::new(route.wire, key, route.model) {
                        Ok(transcriber) => transcriber.transcribe(audio_data, &self.db).await,
                        Err(e) => Err(e),
                    };
                    match &result {
                        Err(e) => log::warn!("{}", cloud_failure_line(&route.label, e, whisper)),
                        Ok(text) if text.is_empty() && whisper => log::info!("[voice] cloud transcription on {} returned no text; transcribing on this Mac", route.label),
                        Ok(_) => {}
                    }
                    if let Some(text) = after_cloud(result, whisper)? {
                        return Ok(text);
                    }
                }
            }
        }

        if whisper {
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

/// Where a spoken request may go, for the AI the user chose.
enum CloudSpeech {
    /// Its speech-to-text, with the key, and its data terms safe or agreed to.
    Ready(SpeechRoute, String),
    /// Its data terms wait for the user's agreement: the recording is not sent.
    WaitsForConsent(SpeechRoute, Box<crate::ai::consent::Request>),
    /// No cloud speech-to-text for this AI, or no key.
    None,
}

/// Resolved on every use rather than once at startup: users paste the key into
/// Settings after first launch, which writes it to the Keychain only.
fn cloud_speech_for(db: &crate::db::Database) -> CloudSpeech {
    let Ok(chain) = crate::commands::ai_choice::saved_chain(db) else { return CloudSpeech::None };
    cloud_speech_from(db, &chain, |wire| match wire {
        transcribe::SpeechWire::Gemini => resolve_key("GEMINI_API_KEY", "gemini_api_key"),
        transcribe::SpeechWire::OpenAi => resolve_key("OPENAI_API_KEY", "openai_api_key"),
    })
}

/// [`cloud_speech_for`] with the saved chain and the key lookup given.
fn cloud_speech_from(db: &crate::db::Database, chain: &[crate::ai::ProviderChainEntry], key_for: impl FnOnce(transcribe::SpeechWire) -> Option<String>) -> CloudSpeech {
    let Some(route) = speech_route(crate::commands::ai_choice::describe(chain).as_ref()) else {
        return CloudSpeech::None;
    };
    let Some(key) = key_for(route.wire) else { return CloudSpeech::None };
    match speech_gate(db, route.wire) {
        Ok(()) => CloudSpeech::Ready(route, key),
        Err(request) => CloudSpeech::WaitsForConsent(route, request),
    }
}

/// The data gate for the recording, as for email and calendar content
/// (ai::mail_policy::gate): the provider's terms are safe, or the user
/// agreed to them.
fn speech_gate(db: &crate::db::Database, wire: transcribe::SpeechWire) -> Result<(), Box<crate::ai::consent::Request>> {
    crate::ai::mail_policy::gate(db, crate::ai::consent::Subject { provider: speech_provider(wire).into(), base_url: String::new() })
}

fn speech_provider(wire: transcribe::SpeechWire) -> &'static str {
    match wire {
        transcribe::SpeechWire::Gemini => "gemini",
        transcribe::SpeechWire::OpenAi => "openai",
    }
}

/// Whether a consent to `provider` can let a voice recording through: it
/// is one of the services a recording is sent to.
pub(crate) fn is_speech_provider(provider: &str) -> bool {
    [transcribe::SpeechWire::Gemini, transcribe::SpeechWire::OpenAi].into_iter().any(|wire| speech_provider(wire) == provider)
}

/// The first step for a spoken request.
#[derive(Debug, PartialEq)]
enum Step {
    /// The one cloud attempt.
    Cloud,
    /// Whisper on this Mac.
    Local,
    /// Neither can run: why, shown as it is.
    Fail(String),
}

const NEEDS_LOCAL_MODEL: &str = "語音輸入需要本機語音模型：請到「設定 → 語音」下載（約 148 MB，語音只在此 Mac 處理）。";

fn first_step(cloud: &CloudSpeech, whisper: bool, language: crate::ai::bedrock::UiLanguage) -> Step {
    match cloud {
        CloudSpeech::Ready(..) => Step::Cloud,
        _ if whisper => Step::Local,
        CloudSpeech::WaitsForConsent(route, _) => Step::Fail(language.pick(
            format!("Your voice goes to {} only after you agree to its data terms. Agree on the card in Settings → Your AI, or download the speech model for this Mac in Settings → Voice.", route.label),
            format!("你的語音只會在你同意 {} 的資料條款後才傳送。請在「設定 → 你的 AI」的卡片按同意，或在「設定 → 語音」下載此 Mac 的語音模型。", route.label),
        )),
        CloudSpeech::None => Step::Fail(NEEDS_LOCAL_MODEL.into()),
    }
}

/// After the one cloud attempt: its text (Some), Whisper on this Mac
/// (None, only when the model is downloaded), or the cloud error as it is.
fn after_cloud(result: Result<String, String>, whisper: bool) -> Result<Option<String>, String> {
    match result {
        Ok(text) if !text.is_empty() || !whisper => Ok(Some(text)),
        Ok(_) => Ok(None),
        Err(_) if whisper => Ok(None),
        Err(error) => Err(error),
    }
}

/// The log line for a failed cloud transcription: the service and the kind
/// of failure, never the provider's reply, the audio or a transcript.
pub(crate) fn cloud_failure_line(label: &str, error: &str, whisper: bool) -> String {
    let next = if whisper { "transcribing on this Mac" } else { "no speech model on this Mac, the error is shown" };
    format!("[voice] cloud transcription on {label} failed ({:?}); {next}", crate::ai::failure::code_of(error))
}

/// The cloud speech-to-text transcriber for the AI the user chose, when the
/// recording may go to it ([`CloudSpeech::Ready`]). The wake word, which
/// always has Whisper, uses it first.
pub fn cloud_transcriber(db: &crate::db::Database) -> Option<(Transcriber, String)> {
    let CloudSpeech::Ready(route, key) = cloud_speech_for(db) else { return None };
    match Transcriber::new(route.wire, key, route.model) {
        Ok(t) => Some((t, route.label)),
        Err(e) => {
            log::warn!("STT init failed: {}", e);
            None
        }
    }
}

/// The cloud speech-to-text a spoken request goes to, for Settings.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CloudSpeechView {
    /// "Gemini 3.7 Flash (Google Gemini)".
    pub label: String,
    /// True while its data terms wait for the user's agreement: until then
    /// the recording stays on this Mac.
    pub consent_needed: bool,
}

/// Which cloud speech-to-text a spoken request would use, for Settings, or
/// None (local Whisper; the download is offered).
#[tauri::command]
pub fn cloud_speech(db: tauri::State<'_, std::sync::Arc<crate::db::Database>>) -> Option<CloudSpeechView> {
    match cloud_speech_for(&db) {
        CloudSpeech::Ready(route, _) => Some(CloudSpeechView { label: route.label, consent_needed: false }),
        CloudSpeech::WaitsForConsent(route, _) => Some(CloudSpeechView { label: route.label, consent_needed: true }),
        CloudSpeech::None => None,
    }
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

    /// The recording waits for the consent its terms need: Gemini's card
    /// until the user agrees, sent once they do, held back again after they
    /// withdraw it (review B, m1). The key is given here, not read from the
    /// Keychain; nothing is sent.
    #[test]
    fn a_recording_waits_for_the_consent_its_terms_need() {
        use super::{cloud_speech_from, CloudSpeech};
        use crate::ai::mail_policy::test_support::{agree_to_gemini, use_ai};
        let db = crate::db::test_database();
        use_ai(&db, "gemini", None);
        let chain = crate::commands::ai_choice::saved_chain(&db).unwrap();
        let speech = |db: &crate::db::Database| cloud_speech_from(db, &chain, |_| Some("test-gemini".to_string()));
        assert!(matches!(speech(&db), CloudSpeech::WaitsForConsent(_, request) if request.subject().provider == "gemini"));
        agree_to_gemini(&db);
        assert!(matches!(speech(&db), CloudSpeech::Ready(_, key) if key == "test-gemini"));
        crate::ai::consent::revoke(&db, &crate::ai::consent::Subject { provider: "gemini".into(), base_url: String::new() }).unwrap();
        assert!(matches!(speech(&db), CloudSpeech::WaitsForConsent(..)));
        // No key: no cloud speech at all.
        assert!(matches!(cloud_speech_from(&db, &chain, |_| None), CloudSpeech::None));
    }

    /// The recording goes to at most one cloud service, the chosen AI's,
    /// and falls back only to Whisper on this Mac; with no Whisper model
    /// the cloud error is shown as it is.
    #[test]
    fn the_recording_goes_to_one_cloud_service_at_most_and_falls_back_only_to_this_mac() {
        use super::{after_cloud, first_step, CloudSpeech, SpeechRoute, Step};
        use crate::ai::bedrock::UiLanguage;
        let route = SpeechRoute { wire: super::transcribe::SpeechWire::Gemini, model: "m".into(), label: "Gemini STT".into() };
        let ready = CloudSpeech::Ready(route.clone(), "key".into());
        assert_eq!(first_step(&ready, true, UiLanguage::En), Step::Cloud);
        assert_eq!(first_step(&ready, false, UiLanguage::En), Step::Cloud);
        // This Mac never goes on to the cloud: without a cloud route it is
        // Whisper or the reason, nothing else.
        assert_eq!(first_step(&CloudSpeech::None, true, UiLanguage::En), Step::Local);
        assert_eq!(first_step(&CloudSpeech::None, false, UiLanguage::En), Step::Fail(super::NEEDS_LOCAL_MODEL.into()));

        assert_eq!(after_cloud(Ok("hello".into()), true), Ok(Some("hello".into())));
        assert_eq!(after_cloud(Err("Gemini API error 503: busy".into()), true), Ok(None), "to Whisper on this Mac");
        assert_eq!(after_cloud(Err("Gemini API error 503: busy".into()), false), Err("Gemini API error 503: busy".into()), "shown as it is");
        assert_eq!(after_cloud(Ok(String::new()), true), Ok(None));
        assert_eq!(after_cloud(Ok(String::new()), false), Ok(Some(String::new())), "nothing heard, as from Whisper");
    }

    /// A provider whose data terms wait for the user's agreement is never
    /// sent the recording: Whisper on this Mac, or the reason with the card.
    /// Agreeing lets it through; withdrawing stops it again.
    #[test]
    fn a_provider_whose_terms_wait_for_consent_is_never_sent_the_recording() {
        use super::{first_step, speech_gate, CloudSpeech, SpeechRoute, Step};
        use super::transcribe::SpeechWire;
        use crate::ai::bedrock::UiLanguage;
        let db = crate::db::test_database();
        let request = speech_gate(&db, SpeechWire::Gemini).expect_err("Gemini's terms need billing confirmed");
        assert_eq!(request.provider, "gemini");
        let route = SpeechRoute { wire: SpeechWire::Gemini, model: "m".into(), label: "Gemini STT".into() };
        let waiting = CloudSpeech::WaitsForConsent(route, request);
        assert_eq!(first_step(&waiting, true, UiLanguage::En), Step::Local);
        let Step::Fail(reason) = first_step(&waiting, false, UiLanguage::En) else { panic!("the reason is shown") };
        assert!(reason.starts_with("Your voice goes to Gemini STT only after you agree"), "{reason}");

        let subject = crate::ai::consent::Subject { provider: "gemini".into(), base_url: String::new() };
        crate::ai::consent::give(&db, &subject, crate::ai::registry::DataTerms::NeedsBillingConfirmation).unwrap();
        assert!(speech_gate(&db, SpeechWire::Gemini).is_ok());
        crate::ai::consent::revoke(&db, &subject).unwrap();
        assert!(speech_gate(&db, SpeechWire::Gemini).is_err());
        assert!(speech_gate(&db, SpeechWire::OpenAi).is_ok(), "OpenAI's terms are safe");
    }

    /// The log line names the service and the kind of failure, and nothing
    /// of the provider's reply, the audio or a transcript.
    #[test]
    fn a_failed_cloud_transcription_is_logged_without_its_content() {
        // voice/transcribe.rs's own error shapes.
        let error = "Gemini STT error 400 Bad Request: {\"error\":{\"message\":\"PRIVATE-TRANSCRIPT-CANARY\"}}";
        let line = super::cloud_failure_line("Gemini STT", error, true);
        assert_eq!(line, "[voice] cloud transcription on Gemini STT failed (BadRequest); transcribing on this Mac");
        assert!(!line.contains("CANARY"));
        let busy = super::cloud_failure_line("GPT STT", "OpenAI STT error 503 Service Unavailable: PRIVATE-TRANSCRIPT-CANARY", false);
        assert_eq!(busy, "[voice] cloud transcription on GPT STT failed (ServerError); no speech model on this Mac, the error is shown");
        let offline = super::cloud_failure_line("GPT STT", "OpenAI STT error: error sending request for url", true);
        assert!(offline.contains("(Unreachable)"), "{offline}");
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
