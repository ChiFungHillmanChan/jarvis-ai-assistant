use base64::Engine;
use hound::{WavSpec, WavWriter};
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::ai::gemini;

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

    pub async fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        if audio_data.len() < 1600 {
            return Ok(String::new());
        }

        let wav_bytes = self.to_wav(audio_data)?;
        let base64_audio =
            base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

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
                        "text": "Transcribe this audio to text. Return ONLY the transcription, nothing else. If the audio is silent or unintelligible, return an empty string."
                    }
                ]
            }],
            "generationConfig": {
                "maxOutputTokens": 512
            }
        });

        let url = format!(
            "{}/models/{}:generateContent",
            gemini::API_BASE,
            gemini::MODEL_FLASH
        );

        let client = reqwest::Client::new();
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

        let text = result["candidates"]
            .as_array()
            .and_then(|c| c.first())
            .and_then(|c| c["content"]["parts"].as_array())
            .and_then(|p| p.first())
            .and_then(|p| p["text"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();

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
                writer
                    .write_sample(s16)
                    .map_err(|e| format!("WAV write error: {}", e))?;
            }
            writer
                .finalize()
                .map_err(|e| format!("WAV finalize error: {}", e))?;
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
