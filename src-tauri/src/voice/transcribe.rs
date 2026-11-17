use base64::Engine;
use std::path::Path;
use std::sync::Arc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::ai::gemini;

fn read_stt_language(db: &crate::db::Database) -> String {
    let conn = db.conn.lock().ok();
    conn.and_then(|c| {
        c.query_row(
            "SELECT value FROM user_preferences WHERE key = 'stt_language'",
            [],
            |r| r.get::<_, String>(0),
        ).ok()
    }).unwrap_or_else(|| "auto".into())
}

/// Vocabulary size of large-v3 (and large-v3-turbo), the first Whisper models
/// with a 100th language slot: Cantonese.
const LARGE_V3_VOCAB: i32 = 51866;

/// Whether the loaded model has a Cantonese language token. whisper.cpp takes
/// language ids from one global table, where "yue" is 99, but ggml-base and
/// every model before large-v3 have only 99 slots (ids 0-98). Asking those for
/// "yue" selects the token after the last language, <|translate|>, and
/// Cantonese came out as English nonsense.
fn has_yue_token(n_vocab: i32) -> bool {
    n_vocab >= LARGE_V3_VOCAB
}

/// Without it, "zh" rewrites Cantonese in Mandarin wording (明天下午 for
/// 聽日下晝); with it, ggml-base keeps the speaker's words, in Traditional
/// characters.
const CANTONESE_PROMPT: &str = "以下係香港廣東話，用繁體中文書寫。";

/// Whisper's language and initial prompt for the dictation setting, given the
/// loaded model's vocabulary size. Anything but a fixed language ("auto",
/// "bilingual") lets Whisper detect it.
pub(crate) fn whisper_language(setting: &str, n_vocab: i32) -> (Option<&'static str>, Option<&'static str>) {
    match setting {
        "yue" if has_yue_token(n_vocab) => (Some("yue"), None),
        "yue" => (Some("zh"), Some(CANTONESE_PROMPT)),
        "en" => (Some("en"), None),
        _ => (None, None),
    }
}

#[derive(Clone)]
pub struct Transcriber {
    api_key: String,
}

impl Transcriber {
    pub fn new(api_key: String) -> Result<Self, String> {
        if api_key.is_empty() {
            return Err("Gemini API key required for speech-to-text".to_string());
        }
        Ok(Transcriber { api_key })
    }

    pub async fn transcribe(&self, audio_data: &[f32], db: &crate::db::Database) -> Result<String, String> {
        if audio_data.len() < 1600 {
            return Ok(String::new());
        }

        let wav_bytes = super::recordings::wav(audio_data)?;
        let base64_audio =
            base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

        let lang = read_stt_language(db);
        let prompt = match lang.as_str() {
            "yue" => "Transcribe this Cantonese audio to written Chinese (use traditional characters and Hong Kong colloquial style 廣東話, NOT 書面語). Return ONLY the transcription, nothing else. If the audio is silent or unintelligible, return an empty string.",
            "en"  => "Transcribe this audio to text in English. Return ONLY the transcription, nothing else. If the audio is silent or unintelligible, return an empty string.",
            _     => "Transcribe this audio to text in the speaker's language (auto-detect English or Cantonese). Return ONLY the transcription, nothing else. If the audio is silent or unintelligible, return an empty string.",
        };

        let request_body = serde_json::json!({
            "contents": [{
                "parts": [
                    {
                        "inline_data": {
                            "mime_type": "audio/wav",
                            "data": base64_audio
                        }
                    },
                    {
                        "text": prompt
                    }
                ]
            }],
            "generationConfig": {
                "maxOutputTokens": crate::ai::models::limits("gemini").stt_max_output_tokens.unwrap_or_default()
            }
        });
        let model = crate::ai::models::default_for("stt", "gemini");

        let url = format!(
            "{}/models/{}:generateContent",
            gemini::API_BASE,
            model
        );

        // Bounded, because this call sits between the user speaking and any
        // reply: reqwest's default is no timeout at all, so an unreachable or
        // stalled endpoint left the assistant silent indefinitely with no error
        // and no way back. 90 s total is well beyond a normal transcription.
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .map_err(|e| format!("Gemini STT client: {}", e))?;
        let started = std::time::Instant::now();
        let resp = client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Gemini STT error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Gemini STT error {}: {}", status, body));
        }

        let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        record_gemini_stt(db, &model, &result, started.elapsed().as_millis() as i64);

        let text = result["candidates"]
            .as_array()
            .and_then(|c| c.first())
            .and_then(|c| c["content"]["parts"].as_array())
            .and_then(|p| p.first())
            .and_then(|p| p["text"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        log::info!("Transcribed {} characters", text.chars().count());
        Ok(text)
    }
}

/// Store one Gemini transcription. The audio is billed at Gemini's audio
/// input rate, which ai/models.rs does not carry, so the tokens are kept and
/// the cost is left unknown rather than priced as text.
fn record_gemini_stt(db: &crate::db::Database, model: &str, result: &serde_json::Value, latency_ms: i64) {
    let count = |field: &str| result["usageMetadata"][field].as_i64().unwrap_or(0);
    // Thinking is billed as output; candidatesTokenCount leaves it out.
    let output = count("candidatesTokenCount") + count("thoughtsTokenCount");
    crate::ai::cost::record_unpriced(db, "gemini", crate::ai::cost::Feature::Stt, model, count("promptTokenCount"), output, latency_ms);
}

/// What the speaker said, without Whisper's notes about sound that is not
/// speech: "[BLANK_AUDIO]" for silence, "(music)", "*outro music*". Left in,
/// a note was saved as the user's message and answered by the AI.
fn spoken_words(transcript: &str) -> String {
    let mut words = String::new();
    let mut note_ends_at = None;
    for c in transcript.chars() {
        match note_ends_at {
            Some(end) if c == end => note_ends_at = None,
            Some(_) => {}
            None => match c {
                '[' => note_ends_at = Some(']'),
                '(' => note_ends_at = Some(')'),
                '*' => note_ends_at = Some('*'),
                _ => words.push(c),
            },
        }
    }
    words.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How hard local Whisper works on one pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecodeProfile {
    pub threads: Option<i32>,
    pub audio_ctx: Option<i32>,
    /// Re-decode at higher temperatures when a pass is low-confidence.
    pub fallback: bool,
    pub max_tokens: Option<i32>,
}

impl DecodeProfile {
    /// A spoken request, up to 28 s: whisper.cpp's defaults.
    pub const REQUEST: Self = Self { threads: None, audio_ctx: None, fallback: true, max_tokens: None };
    /// The 2 s wake window, checked about once a second whenever someone is
    /// talking near the Mac. The defaults encode a full 30 s context (1500
    /// positions) on 4 threads and may decode six times; 256 positions still
    /// cover about 5 s. Measured on an M5 with ggml-base: 0.7-1.3 CPU-seconds
    /// per pass with the defaults, 72-91 ms with this profile.
    pub const WAKE: Self = Self { threads: Some(2), audio_ctx: Some(256), fallback: false, max_tokens: Some(16) };
}

#[derive(Clone)]
pub struct LocalTranscriber {
    ctx: Arc<WhisperContext>,
}

impl LocalTranscriber {
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let path = model_path
            .to_str()
            .ok_or("Model path is not valid UTF-8")?;
        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(path, params)
            .map_err(|e| format!("Failed to load Whisper model: {}", e))?;
        log::info!("Loaded local Whisper model from {}", path);
        Ok(Self { ctx: Arc::new(ctx) })
    }

    pub fn transcribe(&self, audio_data: &[f32], db: &crate::db::Database) -> Result<String, String> {
        let (language, prompt) = whisper_language(&read_stt_language(db), self.ctx.n_vocab());
        self.transcribe_with(audio_data, language, prompt, DecodeProfile::REQUEST)
    }

    /// Transcribe in a fixed language, or let Whisper detect it when `None`.
    pub fn transcribe_as(&self, audio_data: &[f32], language: Option<&str>, profile: DecodeProfile) -> Result<String, String> {
        self.transcribe_with(audio_data, language, None, profile)
    }

    fn transcribe_with(&self, audio_data: &[f32], language: Option<&str>, prompt: Option<&str>, profile: DecodeProfile) -> Result<String, String> {
        if audio_data.len() < 1600 {
            return Ok(String::new());
        }

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(language);
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_single_segment(true);
        if let Some(prompt) = prompt {
            params.set_initial_prompt(prompt);
        }
        if let Some(threads) = profile.threads {
            params.set_n_threads(threads);
        }
        if let Some(audio_ctx) = profile.audio_ctx {
            params.set_audio_ctx(audio_ctx);
        }
        if !profile.fallback {
            params.set_temperature_inc(0.0);
        }
        if let Some(max_tokens) = profile.max_tokens {
            // Timestamp tokens would count against a cap this small.
            params.set_max_tokens(max_tokens);
            params.set_no_timestamps(true);
        }

        state
            .full(params, audio_data)
            .map_err(|e| format!("Local Whisper transcription failed: {}", e))?;

        let segments = state.full_n_segments();

        let mut text = String::new();
        for idx in 0..segments {
            let Some(segment) = state.get_segment(idx) else {
                continue;
            };
            let segment = segment
                .to_str_lossy()
                .map_err(|e| format!("Failed to read Whisper segment: {}", e))?;
            text.push_str(segment.trim());
            text.push(' ');
        }

        let text = spoken_words(&text);
        log::info!("Locally transcribed {} characters", text.chars().count());
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Gemini transcription is stored as speech-to-text with its tokens,
    /// thinking included, and the cost unknown: audio input has its own rate.
    #[test]
    fn a_gemini_transcription_is_recorded_as_stt() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let reply = serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "聽日三點開會"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 480, "candidatesTokenCount": 12, "thoughtsTokenCount": 30}
        });
        record_gemini_stt(&db, crate::ai::models::GEMINI_FLASH, &reply, 900);
        assert_eq!(
            crate::ai::cost::tests::usage_rows(&db),
            vec![crate::ai::cost::tests::UsageRow {
                provider: "gemini".into(),
                feature: "stt".into(),
                model_id: crate::ai::models::GEMINI_FLASH.into(),
                input_tokens: 480,
                output_tokens: 42,
                usd_cost_micro: None,
                price_unknown: true,
            }]
        );
    }

    #[test]
    fn the_wake_profile_is_light_and_requests_keep_the_defaults() {
        assert_eq!(DecodeProfile::REQUEST, DecodeProfile { threads: None, audio_ctx: None, fallback: true, max_tokens: None });
        // audio_ctx must stay at 256 or more: at 128 the base model repeated itself in testing.
        assert_eq!(DecodeProfile::WAKE, DecodeProfile { threads: Some(2), audio_ctx: Some(256), fallback: false, max_tokens: Some(16) });
    }

    #[test]
    fn whisper_notes_about_sound_are_not_the_speakers_words() {
        for note in ["[BLANK_AUDIO]", "*outro music*", "(music)", "[ Silence ]", " [BLANK_AUDIO] [MUSIC] "] {
            assert_eq!(spoken_words(note), "", "{note:?} is not something the speaker said");
        }
        assert_eq!(spoken_words("[MUSIC] what is on my calendar"), "what is on my calendar");
        assert_eq!(spoken_words("我聽日下晝三點要同陳生開會"), "我聽日下晝三點要同陳生開會");
        // A note cut off by the token cap still is not speech.
        assert_eq!(spoken_words("hey jarvis [BLANK_AUD"), "hey jarvis");
    }

    /// Run by hand against the real model:
    /// `JARVIS_WHISPER_MODEL=.../ggml-base.bin cargo test --release --lib
    /// a_quiet_room_transcribes_to_nothing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn a_quiet_room_transcribes_to_nothing() {
        let model = std::env::var("JARVIS_WHISPER_MODEL").expect("JARVIS_WHISPER_MODEL");
        let transcriber = LocalTranscriber::new(std::path::Path::new(&model)).unwrap();
        // Hiss just above the level push-to-talk already discards as silence (RMS 0.001).
        let mut seed = 12345u32;
        let hiss: Vec<f32> = (0..16_000 * 3)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                ((seed >> 8) as f32 / 8388608.0 - 1.0) * 0.003
            })
            .collect();
        for (language, prompt) in [(Some("en"), None), (Some("zh"), Some(CANTONESE_PROMPT)), (None, None)] {
            let text = transcriber.transcribe_with(&hiss, language, prompt, DecodeProfile::REQUEST).unwrap();
            assert_eq!(text, "", "{language:?}: three seconds of room hiss came back as {text:?}");
        }
    }

    /// Run by hand against the real model and a 16 kHz mono clip of someone saying "Hey Jarvis":
    /// `JARVIS_WHISPER_MODEL=.../ggml-base.bin JARVIS_WAKE_SAMPLE=.../hey.wav cargo test
    /// --release --lib the_wake_profile_still_hears_hey_jarvis -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn the_wake_profile_still_hears_hey_jarvis() {
        let model = std::env::var("JARVIS_WHISPER_MODEL").expect("JARVIS_WHISPER_MODEL");
        let sample = std::env::var("JARVIS_WAKE_SAMPLE").expect("JARVIS_WAKE_SAMPLE");
        let mut reader = hound::WavReader::open(sample).unwrap();
        assert_eq!((reader.spec().sample_rate, reader.spec().channels), (16_000, 1));
        let audio: Vec<f32> = reader.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
        let transcriber = LocalTranscriber::new(std::path::Path::new(&model)).unwrap();
        for profile in [DecodeProfile::REQUEST, DecodeProfile::WAKE] {
            let started = std::time::Instant::now();
            let text = transcriber.transcribe_as(&audio, Some("en"), profile).unwrap();
            println!("{profile:?}: {:?} in {:?}", text, started.elapsed());
            assert!(text.to_lowercase().contains("jarvis"), "{profile:?} heard {text:?}");
        }
    }

    #[test]
    fn whisper_language_follows_the_dictation_setting() {
        assert_eq!(whisper_language("yue", LARGE_V3_VOCAB), (Some("yue"), None));
        assert_eq!(whisper_language("en", BASE_VOCAB), (Some("en"), None));
        assert_eq!(whisper_language("bilingual", BASE_VOCAB), (None, None));
        assert_eq!(whisper_language("auto", BASE_VOCAB), (None, None));
    }

    const BASE_VOCAB: i32 = 51865;

    /// ggml-base has 99 language slots, so "yue" (id 99) would select
    /// <|translate|> and Cantonese came out as English nonsense.
    #[test]
    fn cantonese_on_a_model_without_a_yue_token_decodes_as_chinese_with_a_cantonese_prompt() {
        assert_eq!(whisper_language("yue", BASE_VOCAB), (Some("zh"), Some(CANTONESE_PROMPT)));
        assert!(CANTONESE_PROMPT.contains("廣東話"));
    }

    /// whisper.cpp's own count: `num_languages() = n_vocab - 51765 - 1` for a
    /// multilingual model, and "yue" needs id 99 to be inside it.
    #[test]
    fn only_a_large_v3_sized_vocabulary_has_the_yue_token() {
        assert!(!has_yue_token(BASE_VOCAB));
        assert!(has_yue_token(LARGE_V3_VOCAB));
        assert_eq!(whisper_rs::get_lang_id("yue"), Some(99));
    }

    /// Run by hand against the real model and a 16 kHz mono Cantonese clip, e.g.
    /// `say -v Sinji -o /tmp/yue.wav --file-format=WAVE --data-format=LEI16@16000 '我聽日下晝三點要同陳生開會'`, then
    /// `JARVIS_WHISPER_MODEL=.../ggml-base.bin JARVIS_CANTONESE_SAMPLE=/tmp/yue.wav cargo test
    /// --release --lib the_cantonese_setting_writes_chinese -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn the_cantonese_setting_writes_chinese() {
        let model = std::env::var("JARVIS_WHISPER_MODEL").expect("JARVIS_WHISPER_MODEL");
        let sample = std::env::var("JARVIS_CANTONESE_SAMPLE").expect("JARVIS_CANTONESE_SAMPLE");
        let mut reader = hound::WavReader::open(sample).unwrap();
        assert_eq!((reader.spec().sample_rate, reader.spec().channels), (16_000, 1));
        let audio: Vec<f32> = reader.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
        let transcriber = LocalTranscriber::new(std::path::Path::new(&model)).unwrap();
        let (language, prompt) = whisper_language("yue", transcriber.ctx.n_vocab());
        let text = transcriber.transcribe_with(&audio, language, prompt, DecodeProfile::REQUEST).unwrap();
        println!("{language:?}: {text:?}");
        let han = text.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count();
        assert!(han * 2 >= text.chars().filter(|c| !c.is_whitespace()).count(), "mostly Chinese expected, heard {text:?}");
    }
}
