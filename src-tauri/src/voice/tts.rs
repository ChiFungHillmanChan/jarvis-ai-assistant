use tokio::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;
use serde_json::json;

#[derive(Clone)]
pub struct TextToSpeech {
    voice: String,
    rate: u32,
    enabled: bool,
    cancel_flag: Arc<AtomicBool>,
}

impl TextToSpeech {
    pub fn new() -> Self {
        TextToSpeech {
            voice: "Samantha".to_string(),
            rate: 200,
            enabled: true,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a TTS instance with settings loaded from user_preferences DB.
    pub fn from_db(db: &crate::db::Database) -> Self {
        let conn = db.conn.lock().unwrap();
        let get = |key: &str, default: &str| -> String {
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = ?1",
                rusqlite::params![key],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| default.to_string())
        };
        TextToSpeech {
            voice: get("tts_voice", "Samantha"),
            rate: get("tts_rate", "200").parse().unwrap_or(200),
            enabled: get("tts_enabled", "true") == "true",
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_voice(&mut self, voice: String) { self.voice = voice; }
    pub fn set_rate(&mut self, rate: u32) { self.rate = rate; }
    pub fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }

    pub async fn speak(&self, text: &str) -> Result<(), String> {
        if !self.enabled || text.is_empty() { return Ok(()); }
        let status = Command::new("say")
            .arg("-v").arg(&self.voice)
            .arg("-r").arg(self.rate.to_string())
            .arg(text)
            .status().await.map_err(|e| format!("TTS error: {}", e))?;
        if !status.success() { return Err("TTS command failed".to_string()); }
        Ok(())
    }

    pub async fn speak_queued(&self, text: &str, app_handle: &tauri::AppHandle) -> Result<(), String> {
        if !self.enabled || text.is_empty() { return Ok(()); }
        self.cancel_flag.store(false, Ordering::SeqCst);
        let sentences = split_sentences(text);
        let total = sentences.len();
        for (i, sentence) in sentences.iter().enumerate() {
            if self.cancel_flag.load(Ordering::SeqCst) {
                let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                return Ok(());
            }
            let remaining = total - i - 1;
            if i == 0 {
                let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
            }
            self.speak_sentence(sentence, remaining, app_handle).await?;
            if remaining == 0 {
                let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
            }
        }
        Ok(())
    }

    pub async fn speak_sentence(&self, sentence: &str, remaining: usize, app_handle: &tauri::AppHandle) -> Result<(), String> {
        if !self.enabled || sentence.is_empty() || self.cancel_flag.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _ = app_handle.emit("tts-speaking", json!({
            "sentence": sentence, "remaining": remaining
        }));
        // Amplitude simulation in background
        let cancel = self.cancel_flag.clone();
        let ah = app_handle.clone();
        let word_count = sentence.split_whitespace().count();
        let est_duration_ms = (word_count as u64) * 300;
        let amp_handle = tokio::spawn(async move {
            let start = std::time::Instant::now();
            let interval = std::time::Duration::from_millis(66);
            loop {
                if cancel.load(Ordering::SeqCst) { break; }
                let elapsed = start.elapsed().as_millis() as f64;
                if elapsed > est_duration_ms as f64 * 1.5 { break; }
                let t = elapsed / 1000.0;
                let amp = (0.4 + 0.3 * (t * 5.0).sin() + 0.2 * (t * 8.3).sin() + 0.1 * (t * 12.7).sin()).clamp(0.0, 1.0);
                let _ = ah.emit("tts-amplitude", json!({"amplitude": amp}));
                tokio::time::sleep(interval).await;
            }
        });
        let status = Command::new("say")
            .arg("-v").arg(&self.voice)
            .arg("-r").arg(self.rate.to_string())
            .arg(sentence)
            .status().await.map_err(|e| format!("TTS error: {}", e))?;
        amp_handle.abort();
        let _ = app_handle.emit("tts-amplitude", json!({"amplitude": 0.0}));
        if !status.success() {
            log::warn!("TTS sentence failed: {}", &sentence[..sentence.len().min(50)]);
        }
        let _ = app_handle.emit("tts-sentence-done", json!({"remaining": remaining}));
        Ok(())
    }

    pub async fn list_voices() -> Result<Vec<String>, String> {
        let output = Command::new("say").arg("-v").arg("?")
            .output().await.map_err(|e| format!("Failed to list voices: {}", e))?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text.lines().filter_map(|line| line.split_whitespace().next().map(|s| s.to_string())).collect())
    }
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut buffer = String::new();
    let mut prev_was_delimiter = false;
    for ch in text.chars() {
        buffer.push(ch);
        if prev_was_delimiter && ch == ' ' && buffer.len() >= 20 {
            let trimmed = buffer.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(trimmed);
            }
            buffer.clear();
            prev_was_delimiter = false;
            continue;
        }
        prev_was_delimiter = matches!(ch, '.' | '!' | '?');
        if buffer.len() >= 200 {
            if let Some(last_space) = buffer.rfind(' ') {
                let (sentence, remainder) = buffer.split_at(last_space);
                let trimmed = sentence.trim().to_string();
                let leftover = remainder.trim().to_string();
                if !trimmed.is_empty() {
                    sentences.push(trimmed);
                }
                buffer = leftover;
            } else {
                let trimmed = buffer.trim().to_string();
                if !trimmed.is_empty() {
                    sentences.push(trimmed);
                }
                buffer.clear();
            }
        }
    }
    let trimmed = buffer.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }
    sentences
}
