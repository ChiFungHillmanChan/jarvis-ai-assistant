use reqwest::Client;
use tokio::sync::mpsc;
use futures_util::StreamExt;

pub struct MinimaxClient {
    api_key: String,
}

impl MinimaxClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    /// Returns a channel that yields streaming MP3 byte chunks.
    pub async fn stream_speak(
        &self,
        text: &str,
        voice_id: &str,
    ) -> Result<mpsc::Receiver<Vec<u8>>, String> {
        let url = "https://api.minimaxi.chat/v1/t2a_v2";
        let body = serde_json::json!({
            "model": "speech-02-hd",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn cantonese_tts_returns_mp3_bytes() {
        let key = std::env::var("MINIMAX_API_KEY").unwrap_or_default();
        if key.is_empty() {
            eprintln!("Skipping: no MINIMAX_API_KEY in env");
            return;
        }
        let client = MinimaxClient::new(key);
        let mut rx = client
            .stream_speak("你好，我係 Jarvis", "Cantonese_PrettyGirl")
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
