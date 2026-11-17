use reqwest::Client;
use tokio::sync::mpsc;
use futures_util::StreamExt;

/// The MiniMax speech model JARVIS asks for: the registry's `tts.minimax`.
fn model() -> String {
    crate::ai::models::default_for("tts", "minimax")
}

pub struct MinimaxClient {
    api_key: String,
}

impl MinimaxClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    /// Returns a channel that yields streaming MP3 byte chunks. An accepted
    /// request is stored in the usage table: MiniMax bills per character.
    pub async fn stream_speak(
        &self,
        text: &str,
        voice_id: &str,
        db: &crate::db::Database,
    ) -> Result<mpsc::Receiver<Vec<u8>>, String> {
        let url = "https://api.minimaxi.chat/v1/t2a_v2";
        let started = std::time::Instant::now();
        let model = model();
        let body = serde_json::json!({
            "model": model,
            "text": text,
            "stream": true,
            "voice_setting": {
                "voice_id": voice_id,
                "speed": 1.0,
                "vol": 1.0,
                "pitch": 0,
            },
            "audio_setting": {
                "sample_rate": 32000,
                "bitrate": 128000,
                "format": "mp3",
                "channel": 1,
            }
        });

        let client = Client::new();
        let resp = client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("minimax post: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "minimax http {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        record_speech(db, &model, text, started.elapsed().as_millis() as i64);

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut byte_stream = resp.bytes_stream();
            while let Some(item) = byte_stream.next().await {
                match item {
                    Ok(bytes) => {
                        if tx.send(bytes.to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(rx)
    }
}

/// MiniMax prices per character, which the token price list cannot express:
/// the characters sent go in `input_tokens`, with the cost unknown.
fn record_speech(db: &crate::db::Database, model: &str, text: &str, latency_ms: i64) {
    crate::ai::cost::record_unpriced(db, "minimax", crate::ai::cost::Feature::Tts, model, text.chars().count() as i64, 0, latency_ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MiniMax bills per character: the characters are stored, the cost is
    /// unknown.
    #[test]
    fn speech_is_recorded_as_tts_by_character() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        record_speech(&db, &model(), "你好，我係 Jarvis", 300);
        assert_eq!(
            crate::ai::cost::tests::usage_rows(&db),
            vec![crate::ai::cost::tests::UsageRow {
                provider: "minimax".into(),
                feature: "tts".into(),
                model_id: "speech-02-hd".into(),
                input_tokens: 12,
                output_tokens: 0,
                usd_cost_micro: None,
                price_unknown: true,
            }]
        );
    }

    #[tokio::test]
    #[ignore]
    async fn cantonese_tts_returns_mp3_bytes() {
        let key = std::env::var("MINIMAX_API_KEY").unwrap_or_default();
        if key.is_empty() {
            eprintln!("Skipping: no MINIMAX_API_KEY in env");
            return;
        }
        let client = MinimaxClient::new(key);
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let mut rx = client
            .stream_speak("你好，我係 Jarvis", "Cantonese_PrettyGirl", &db)
            .await
            .unwrap();
        let mut total = 0usize;
        while let Some(chunk) = rx.recv().await {
            total += chunk.len();
        }
        assert!(
            total > 1000,
            "expected non-trivial mp3 stream, got {total} bytes"
        );
    }
}
