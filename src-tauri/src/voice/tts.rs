use tokio::process::Command;
use tokio::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;
use serde_json::json;

// ---------------------------------------------------------------------------
// Streaming TTS -- speaks sentences as AI tokens arrive
// ---------------------------------------------------------------------------

pub enum TtsCommand {
    /// A text chunk from the AI streaming response -- accumulated into sentences
    TextChunk(String),
    /// A narration to speak immediately (e.g. tool status like "Checking calendar...")
    Narrate(String),
    /// AI finished -- speak remaining buffer, then stop
    Done,
}

/// Streaming TTS consumer that speaks sentences in real-time as tokens arrive.
pub struct StreamingTts {
    tx: mpsc::Sender<TtsCommand>,
    handle: tokio::task::JoinHandle<()>,
}

impl StreamingTts {
    pub fn new(tts: TextToSpeech, app_handle: tauri::AppHandle) -> Self {
        let (tx, rx) = mpsc::channel::<TtsCommand>(200);
        let handle = tokio::spawn(streaming_tts_consumer(tts, app_handle, rx));
        StreamingTts { tx, handle }
    }

    /// Get a clone of the sender for passing into AI providers.
    pub fn sender(&self) -> mpsc::Sender<TtsCommand> {
        self.tx.clone()
    }

    /// Drop the sender and wait for the consumer to finish speaking everything.
    pub async fn finish(self) {
        let _ = self.tx.send(TtsCommand::Done).await;
        drop(self.tx);
        let _ = self.handle.await;
    }
}

async fn streaming_tts_consumer(
    tts: TextToSpeech,
    app_handle: tauri::AppHandle,
    mut rx: mpsc::Receiver<TtsCommand>,
) {
    if !tts.enabled {
        // Drain channel without speaking
        while rx.recv().await.is_some() {}
        return;
    }

    tts.cancel_flag.store(false, Ordering::SeqCst);

    let mut buffer = String::new();
    let mut sentence_count: usize = 0;
    let mut prev_was_delimiter = false;

    loop {
        let cmd = match rx.recv().await {
            Some(cmd) => cmd,
            None => break, // channel closed
        };

        if tts.is_cancelled() {
            let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
            // Drain remaining messages
            while rx.recv().await.is_some() {}
            return;
        }

        match cmd {
            TtsCommand::TextChunk(text) => {
                for ch in text.chars() {
                    buffer.push(ch);

                    // Sentence boundary: delimiter followed by space, buffer >= 10 chars
                    if prev_was_delimiter && ch == ' ' && buffer.len() >= 10 {
                        let sentence = buffer.trim().to_string();
                        if !sentence.is_empty() {
                            if sentence_count == 0 {
                                let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
                            }
                            let _ = tts.speak_sentence(&sentence, 1, &app_handle).await;
                            sentence_count += 1;
                        }
                        buffer.clear();
                        prev_was_delimiter = false;
                        continue;
                    }

                    prev_was_delimiter = matches!(ch, '.' | '!' | '?' | ':');

                    // Force-split at 150 chars for faster delivery
                    if buffer.len() >= 150 {
                        let sentence = if let Some(last_space) = buffer.rfind(' ') {
                            let (s, remainder) = buffer.split_at(last_space);
                            let sentence = s.trim().to_string();
                            buffer = remainder.trim().to_string();
                            sentence
                        } else {
                            let sentence = buffer.trim().to_string();
                            buffer.clear();
                            sentence
                        };
                        if !sentence.is_empty() {
                            if sentence_count == 0 {
                                let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
                            }
                            let _ = tts.speak_sentence(&sentence, 1, &app_handle).await;
                            sentence_count += 1;
                        }
                    }
                }
            }
            TtsCommand::Narrate(text) => {
                // Flush any buffered text FIRST so it's spoken before the narration
                let pending = buffer.trim().to_string();
                if !pending.is_empty() {
                    if sentence_count == 0 {
                        let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
                    }
                    let _ = tts.speak_sentence(&pending, 1, &app_handle).await;
                    sentence_count += 1;
                    buffer.clear();
                    prev_was_delimiter = false;
                }
                // Then speak the narration (e.g. "Checking calendar...")
                if !text.is_empty() {
                    if sentence_count == 0 {
                        let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
                    }
                    let _ = tts.speak_sentence(&text, 1, &app_handle).await;
                    sentence_count += 1;
                }
            }
            TtsCommand::Done => {
                // Speak any remaining buffered text
                let remaining = buffer.trim().to_string();
                if !remaining.is_empty() {
                    if sentence_count == 0 {
                        let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
                    }
                    let _ = tts.speak_sentence(&remaining, 0, &app_handle).await;
                }
                let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                return;
            }
        }
    }

    // Channel closed without Done -- speak remaining buffer and clean up
    let remaining = buffer.trim().to_string();
    if !remaining.is_empty() {
        if sentence_count == 0 {
            let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
        }
        let _ = tts.speak_sentence(&remaining, 0, &app_handle).await;
    }
    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
}

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
        // Sanitize text for natural speech -- strip markdown and formatting
        let clean = sanitize_for_speech(sentence);
        if clean.is_empty() {
            return Ok(());
        }
        let _ = app_handle.emit("tts-speaking", json!({
            "sentence": &clean, "remaining": remaining
        }));
        // Amplitude simulation in background
        let cancel = self.cancel_flag.clone();
        let ah = app_handle.clone();
        let word_count = clean.split_whitespace().count();
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
            .arg(&clean)
            .status().await.map_err(|e| format!("TTS error: {}", e))?;
        amp_handle.abort();
        let _ = app_handle.emit("tts-amplitude", json!({"amplitude": 0.0}));
        if !status.success() {
            log::warn!("TTS sentence failed: {}", &clean[..clean.len().min(50)]);
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

/// Strip markdown, URLs, and formatting so macOS `say` reads clean natural speech.
fn sanitize_for_speech(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Strip markdown bold/italic markers: * and _
            '*' | '_' => continue,
            // Strip backticks (inline code)
            '`' => continue,
            // Strip hash headers: "## Title" → "Title"
            '#' => {
                while chars.peek() == Some(&'#') { chars.next(); }
                if chars.peek() == Some(&' ') { chars.next(); }
                continue;
            }
            // Convert markdown links [text](url) → "text"
            '[' => {
                let mut link_text = String::new();
                for c in chars.by_ref() {
                    if c == ']' { break; }
                    link_text.push(c);
                }
                // Skip the (url) part if present
                if chars.peek() == Some(&'(') {
                    chars.next();
                    let mut depth = 1;
                    for c in chars.by_ref() {
                        if c == '(' { depth += 1; }
                        if c == ')' { depth -= 1; if depth == 0 { break; } }
                    }
                }
                out.push_str(&link_text);
                continue;
            }
            // Strip bullet-point dashes at start of line: "- item" → "item"
            '-' if out.is_empty() || out.ends_with('\n') => {
                if chars.peek() == Some(&' ') { chars.next(); }
                continue;
            }
            // Collapse newlines into spaces
            '\n' => {
                if !out.ends_with(' ') && !out.is_empty() {
                    out.push(' ');
                }
                continue;
            }
            _ => out.push(ch),
        }
    }

    // Collapse multiple spaces
    let mut result = String::with_capacity(out.len());
    let mut prev_space = false;
    for ch in out.chars() {
        if ch == ' ' {
            if !prev_space { result.push(' '); }
            prev_space = true;
        } else {
            result.push(ch);
            prev_space = false;
        }
    }

    result.trim().to_string()
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
