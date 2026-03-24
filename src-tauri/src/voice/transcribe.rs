use hound::{WavSpec, WavWriter};
use reqwest::multipart;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

#[derive(Clone)]
pub struct Transcriber {
    api_key: String,
}

impl Transcriber {
    pub fn new(api_key: String) -> Result<Self, String> {
        if api_key.is_empty() {
            return Err("OpenAI API key required for speech-to-text".to_string());
        }
        Ok(Transcriber { api_key })
    }

    pub async fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        if audio_data.len() < 1600 {
            return Ok(String::new());
        }

        // Convert f32 PCM to WAV bytes
        let wav_bytes = self.to_wav(audio_data)?;

        // Send to OpenAI Whisper API
        let client = reqwest::Client::new();
        let file_part = multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| e.to_string())?;

        let form = multipart::Form::new()
            .text("model", "whisper-1")
            .text("language", "en")
            .part("file", file_part);

        let resp = client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Whisper API error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Whisper API error {}: {}", status, body));
        }

        let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let text = result["text"].as_str().unwrap_or("").trim().to_string();

        log::info!("Transcribed: \"{}\"", text);
        Ok(text)
    }

    fn to_wav(&self, audio_data: &[f32]) -> Result<Vec<u8>, String> {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(&mut cursor, spec)
                .map_err(|e| format!("WAV writer error: {}", e))?;

            for &sample in audio_data {
                let s16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                writer.write_sample(s16).map_err(|e| format!("WAV write error: {}", e))?;
            }
            writer.finalize().map_err(|e| format!("WAV finalize error: {}", e))?;
        }

        Ok(cursor.into_inner())
    }
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

    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        if audio_data.len() < 1600 {
            return Ok(String::new());
        }

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_single_segment(true);

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

        let text = text.trim().to_string();
        log::info!("Locally transcribed: \"{}\"", text);
        Ok(text)
    }
}
