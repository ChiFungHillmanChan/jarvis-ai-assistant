pub mod capture;
pub mod commands;
pub mod transcribe;
pub mod tts;

use capture::AudioCapture;
use transcribe::Transcriber;
use tts::TextToSpeech;
use std::sync::Mutex;

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub enum VoiceState {
    Idle,
    Listening,
    Processing,
    Speaking,
    Error(String),
    Disabled,
}

pub struct VoiceEngine {
    pub capture: Mutex<AudioCapture>,
    pub transcriber: Mutex<Option<Transcriber>>,
    pub tts: Mutex<TextToSpeech>,
    pub state: Mutex<VoiceState>,
    pub muted: Mutex<bool>,
}

impl VoiceEngine {
    pub fn new() -> Self {
        // AudioCapture::new() constructs empty state and cannot fail in practice
        let capture = AudioCapture::new().expect("AudioCapture init should not fail");

        let transcriber = match std::env::var("OPENAI_API_KEY") {
            Ok(key) if !key.is_empty() => {
                match Transcriber::new(key) {
                    Ok(t) => { log::info!("Voice STT ready (OpenAI Whisper API)"); Some(t) }
                    Err(e) => { log::warn!("STT init failed: {}", e); None }
                }
            }
            _ => { log::info!("No OPENAI_API_KEY set. Voice STT disabled."); None }
        };

        VoiceEngine {
            capture: Mutex::new(capture),
            transcriber: Mutex::new(transcriber),
            tts: Mutex::new(TextToSpeech::new()),
            state: Mutex::new(VoiceState::Idle),
            muted: Mutex::new(false),
        }
    }

    pub fn set_state(&self, state: VoiceState) { *self.state.lock().unwrap() = state; }
    pub fn get_state(&self) -> VoiceState { self.state.lock().unwrap().clone() }
    pub fn is_available(&self) -> bool { self.transcriber.lock().unwrap().is_some() }

    pub fn toggle_mute(&self) -> bool {
        let mut muted = self.muted.lock().unwrap();
        *muted = !*muted;
        if *muted { self.set_state(VoiceState::Disabled); }
        else { self.set_state(VoiceState::Idle); }
        *muted
    }
}
