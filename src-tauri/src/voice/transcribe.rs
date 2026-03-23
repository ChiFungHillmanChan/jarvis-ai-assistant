use hound::{WavSpec, WavWriter};
use reqwest::multipart;
use std::io::Cursor;

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
