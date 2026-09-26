use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use crate::chat_session::ScopedEmitter;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum DelimiterStrength {
    Hard,
    Soft,
}

/// Characters after a hard delimiter before a chunk is spoken.
const HARD_MIN_CHARS: usize = 10;
/// Characters after a soft delimiter before a chunk is spoken.
const SOFT_MIN_CHARS: usize = 28;
/// Characters after which a chunk is spoken with no delimiter at all.
const FORCE_SPLIT_CHARS: usize = 90;

/// Process-wide playback lock.
///
/// Push-to-talk, wake word and the daily briefing each build their own
/// `TextToSpeech` and spawn `say` independently. Without this they talk over
/// each other. It is a tokio mutex because it is held across the `.await` on
/// the child process; acquire it only around playback, only ever after
/// `utterance_lock` and never while holding a std mutex.
pub fn playback_lock() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

/// Whether anything is being spoken right now: every playback path holds `playback_lock`.
pub fn is_playing() -> bool {
    playback_lock().try_lock().is_err()
}

/// Process-wide utterance lock.
///
/// `playback_lock` is taken per `say` child so a cancel can land between
/// sentences. On its own that also means two live utterances *alternate*:
/// tokio's mutex is FIFO-fair, so a second speaker waiting while sentence N
/// plays gets the lock ahead of the first speaker's sentence N+1, and the user
/// hears briefing / reply / briefing / reply. Two voices swapping sentences is
/// worse than one waiting for the other.
///
/// So an utterance holds this for its whole length and a second speaker waits
/// for the first to finish or be cancelled. Cancellation is unaffected: it is
/// still checked between sentences and still kills the running child, so the
/// wait is bounded by one sentence rather than by the whole utterance.
///
/// Lock order is always utterance_lock -> playback_lock. Nothing takes them the
/// other way round, and no caller of the public methods holds either.
fn utterance_lock() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

/// Incremental sentence segmenter for the streaming TTS consumer.
///
/// Handles ASCII punctuation (delimiter followed by a space) and CJK
/// punctuation, which has no trailing space -- there the delimiter itself ends
/// the chunk. Thresholds count characters, not bytes: a Han character is three
/// bytes, so byte counts fire at a third of the intended length.
struct SentenceBuffer {
    buffer: String,
    chars: usize,
    pending: Option<DelimiterStrength>,
}

impl SentenceBuffer {
    fn new() -> Self {
        SentenceBuffer {
            buffer: String::new(),
            chars: 0,
            pending: None,
        }
    }

    /// Feed one character. Returns a chunk once a sentence boundary is reached.
    fn push(&mut self, ch: char) -> Option<String> {
        self.buffer.push(ch);
        self.chars += 1;

        let should_flush = match cjk_delimiter(ch) {
            Some(DelimiterStrength::Hard) => self.chars >= HARD_MIN_CHARS,
            Some(DelimiterStrength::Soft) => self.chars >= SOFT_MIN_CHARS,
            None if ch == ' ' => match self.pending {
                Some(DelimiterStrength::Hard) => self.chars >= HARD_MIN_CHARS,
                Some(DelimiterStrength::Soft) => self.chars >= SOFT_MIN_CHARS,
                None => false,
            },
            None => false,
        };

        if should_flush {
            return self.take();
        }

        self.pending = match ch {
            '.' | '!' | '?' | ':' => Some(DelimiterStrength::Hard),
            ',' | ';' => Some(DelimiterStrength::Soft),
            _ => None,
        };

        // Force-split earlier so the first spoken chunk starts faster.
        if self.chars >= FORCE_SPLIT_CHARS {
            return self.force_split();
        }

        None
    }

    /// Take everything buffered so far, if it is not blank.
    fn take(&mut self) -> Option<String> {
        let sentence = self.buffer.trim().to_string();
        self.buffer.clear();
        self.chars = 0;
        self.pending = None;
        if sentence.is_empty() {
            None
        } else {
            Some(sentence)
        }
    }

    fn force_split(&mut self) -> Option<String> {
        let sentence = if let Some(last_space) = self.buffer.rfind(' ') {
            let (s, remainder) = self.buffer.split_at(last_space);
            let sentence = s.trim().to_string();
            self.buffer = remainder.trim().to_string();
            sentence
        } else {
            // Spaceless run (CJK): the whole buffer is one chunk.
            let sentence = self.buffer.trim().to_string();
            self.buffer.clear();
            sentence
        };
        self.chars = self.buffer.chars().count();
        self.pending = None;
        if sentence.is_empty() {
            None
        } else {
            Some(sentence)
        }
    }
}

/// Full-width and ideographic punctuation. Written Chinese puts no space after
/// these, so they end a chunk on their own.
fn cjk_delimiter(ch: char) -> Option<DelimiterStrength> {
    match ch {
        '\u{3002}' | '\u{FF01}' | '\u{FF1F}' => Some(DelimiterStrength::Hard),
        '\u{FF0C}' | '\u{3001}' | '\u{FF1B}' => Some(DelimiterStrength::Soft),
        _ => None,
    }
}

/// Streaming TTS consumer that speaks sentences in real-time as tokens arrive.
pub struct StreamingTts {
    tx: mpsc::Sender<TtsCommand>,
    handle: tokio::task::JoinHandle<()>,
}

impl StreamingTts {
    pub fn new(tts: TextToSpeech, app_handle: tauri::AppHandle) -> Self {
        let (tx, rx) = mpsc::channel::<TtsCommand>(200);
        let context = crate::chat_session::CONTEXT.try_with(Clone::clone).ok();
        let handle = tokio::spawn(async move {
            let consumer = streaming_tts_consumer(tts, app_handle, rx);
            if let Some(context) = context { crate::chat_session::CONTEXT.scope(context, consumer).await }
            else { consumer.await }
        });
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
        // Drain the channel without speaking, and only then say the turn is
        // over. The consumer is spawned before the provider call, so emitting
        // on entry told the HUD "idle" while the request was still in flight:
        // App.tsx arms a 900 ms timer on any non-thinking state, so the
        // "Understanding your request..." HUD blanked a second in and stayed
        // blank until the first text token. The channel closes when the caller
        // drops the sender in `finish()`, which is the turn actually ending.
        while rx.recv().await.is_some() {}
        let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
        return;
    }

    tts.cancel_flag.store(false, Ordering::SeqCst);

    let mut buffer = SentenceBuffer::new();
    let mut sentence_count: usize = 0;

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
                    if let Some(sentence) = buffer.push(ch) {
                        if sentence_count == 0 {
                            let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
                        }
                        let _ = tts.speak_sentence(&sentence, 1, &app_handle).await;
                        sentence_count += 1;
                    }
                }
            }
            TtsCommand::Narrate(text) => {
                // Flush any buffered text FIRST so it's spoken before the narration
                if let Some(pending) = buffer.take() {
                    if sentence_count == 0 {
                        let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
                    }
                    let _ = tts.speak_sentence(&pending, 1, &app_handle).await;
                    sentence_count += 1;
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
                if let Some(remaining) = buffer.take() {
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
    if let Some(remaining) = buffer.take() {
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
    voice_yue: String,
    rate: u32,
    enabled: bool,
    cancel_flag: Arc<AtomicBool>,
}

impl TextToSpeech {
    pub fn new() -> Self {
        TextToSpeech {
            voice: "Samantha".to_string(),
            voice_yue: "Sinji".to_string(),
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
            voice_yue: get("tts_voice_yue", "Sinji"),
            rate: get("tts_rate", "200").parse().unwrap_or(200),
            enabled: get("tts_enabled", "true") == "true",
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_voice(&mut self, voice: String) {
        self.voice = voice;
    }
    pub fn set_voice_yue(&mut self, voice: String) {
        self.voice_yue = voice;
    }
    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate;
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::SeqCst)
    }

    /// Speak a whole utterance, one `say` per sentence.
    ///
    /// Split rather than handed to `say` in one piece because the daily
    /// briefing is a single 30-60 s utterance: spoken as one child, a cancel
    /// had nothing to take effect between and the briefing ran to the end
    /// regardless. Per sentence, `cancel()` lands between sentences as well as
    /// during them (`wait_with_cancel` kills the running child), so another
    /// speaker queued behind this utterance waits at most one sentence past the
    /// moment this one is cancelled.
    pub async fn speak(&self, text: &str) -> Result<(), String> {
        if !self.enabled || text.is_empty() {
            return Ok(());
        }
        // A cancel belongs to the utterance that was playing when it was
        // raised, not to this new one. Same convention as speak_queued.
        self.cancel_flag.store(false, Ordering::SeqCst);
        // Held for the whole utterance: the sentences of one briefing must not
        // have another speaker's sentences threaded between them.
        let _utterance = utterance_lock().lock().await;
        for sentence in split_sentences(text) {
            if self.is_cancelled() {
                return Ok(());
            }
            let _playback = playback_lock().lock().await;
            // Waiting for the lock can take as long as somebody else's
            // sentence; the cancel may have arrived while we were queued.
            if self.is_cancelled() {
                return Ok(());
            }
            let status = spawn_say(
                &self.voice,
                &self.voice_yue,
                self.rate,
                &sentence,
                &self.cancel_flag,
            )
            .await?;
            if self.is_cancelled() {
                return Ok(());
            }
            if !status.success() {
                return Err("TTS command failed".to_string());
            }
        }
        Ok(())
    }

    pub async fn speak_queued(
        &self,
        text: &str,
        app_handle: &tauri::AppHandle,
    ) -> Result<(), String> {
        if !self.enabled || text.is_empty() {
            return Ok(());
        }
        self.cancel_flag.store(false, Ordering::SeqCst);
        let sentences = split_sentences(text);
        let total = sentences.len();
        // One utterance, so it is not interleaved with somebody else's.
        let _utterance = utterance_lock().lock().await;
        for (i, sentence) in sentences.iter().enumerate() {
            if self.cancel_flag.load(Ordering::SeqCst) {
                let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                return Ok(());
            }
            let remaining = total - i - 1;
            if i == 0 {
                let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
            }
            self.speak_one(sentence, remaining, app_handle).await?;
            if remaining == 0 {
                let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
            }
        }
        Ok(())
    }

    /// Speak one sentence as a complete utterance.
    ///
    /// The streaming consumer only ever holds one sentence at a time -- the
    /// rest of the reply has not been generated yet -- so a sentence is the
    /// whole utterance it can claim. Checking the cancel flag before queueing
    /// matters: once a reply is cancelled its remaining sentences must not sit
    /// waiting behind somebody else's utterance just to be discarded.
    pub async fn speak_sentence(
        &self,
        sentence: &str,
        remaining: usize,
        app_handle: &tauri::AppHandle,
    ) -> Result<(), String> {
        if !self.enabled || sentence.is_empty() || self.cancel_flag.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _utterance = utterance_lock().lock().await;
        self.speak_one(sentence, remaining, app_handle).await
    }

    /// Body of `speak_sentence`, for callers that already hold the utterance
    /// lock for a run of sentences.
    async fn speak_one(
        &self,
        sentence: &str,
        remaining: usize,
        app_handle: &tauri::AppHandle,
    ) -> Result<(), String> {
        if !self.enabled || sentence.is_empty() || self.cancel_flag.load(Ordering::SeqCst) {
            return Ok(());
        }
        // Sanitize text for natural speech -- strip markdown and formatting
        let clean = sanitize_for_speech(sentence);
        if clean.is_empty() {
            return Ok(());
        }
        let _ = app_handle.emit(
            "tts-speaking",
            json!({
                "sentence": &clean, "remaining": remaining
            }),
        );
        // Serialise playback so two `say` children never speak at once.
        let _playback = playback_lock().lock().await;
        // Acquiring that lock can take as long as another utterance, so the
        // pre-lock check above is stale by the time we get here.
        if self.cancel_flag.load(Ordering::SeqCst) {
            return Ok(());
        }
        // Amplitude simulation in background
        let cancel = self.cancel_flag.clone();
        let ah = app_handle.clone();
        let est_duration_ms = estimate_speech_ms(&clean);
        let amp_handle = tokio::spawn(async move {
            let start = std::time::Instant::now();
            let interval = std::time::Duration::from_millis(66);
            loop {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let elapsed = start.elapsed().as_millis() as f64;
                if elapsed > est_duration_ms as f64 * 1.5 {
                    break;
                }
                let t = elapsed / 1000.0;
                let amp =
                    (0.4 + 0.3 * (t * 5.0).sin() + 0.2 * (t * 8.3).sin() + 0.1 * (t * 12.7).sin())
                        .clamp(0.0, 1.0);
                let _ = ah.emit("tts-amplitude", json!({"amplitude": amp}));
                tokio::time::sleep(interval).await;
            }
        });
        let status = spawn_say(
            &self.voice,
            &self.voice_yue,
            self.rate,
            &clean,
            &self.cancel_flag,
        )
        .await?;
        amp_handle.abort();
        let _ = app_handle.emit("tts-amplitude", json!({"amplitude": 0.0}));
        if !status.success() && !self.cancel_flag.load(Ordering::SeqCst) {
            log::warn!("TTS sentence failed: {}", crate::util::truncate_bytes(&clean, 50));
        }
        let _ = app_handle.emit("tts-sentence-done", json!({"remaining": remaining}));
        Ok(())
    }

    pub async fn list_voices() -> Result<Vec<String>, String> {
        let output = Command::new("say")
            .arg("-v")
            .arg("?")
            .output()
            .await
            .map_err(|e| format!("Failed to list voices: {}", e))?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text
            .lines()
            .filter_map(|line| line.split_whitespace().next().map(|s| s.to_string()))
            .collect())
    }
}

/// How often an in-flight `say` child is re-checked against the cancel flag.
const CANCEL_POLL_MS: u64 = 60;

/// Pick the macOS voice for one sentence: `yue_voice` when `text` contains any
/// CJK character, `en_voice` otherwise. An English voice given CJK text does
/// not mispronounce it -- it drops it, producing a fraction of a second of
/// silence instead of the sentence.
pub fn voice_for<'a>(text: &str, en_voice: &'a str, yue_voice: &'a str) -> &'a str {
    if text.chars().any(is_cjk) {
        yue_voice
    } else {
        en_voice
    }
}

/// Spawn macOS `say` with the utterance on stdin.
///
/// `say` parses its arguments with getopt, so text passed positionally is
/// consumed as an option the moment it starts with '-' -- and model output
/// summarising email routinely starts with a markdown bullet. With no
/// positional argument `say` reads the utterance from stdin, which keeps
/// untrusted text out of argv entirely.
async fn spawn_say(
    en_voice: &str,
    yue_voice: &str,
    rate: u32,
    text: &str,
    cancel: &AtomicBool,
) -> Result<std::process::ExitStatus, String> {
    let voice = voice_for(text, en_voice, yue_voice);
    let mut child = Command::new("say")
        .arg("-v")
        .arg(voice)
        .arg("-r")
        .arg(rate.to_string())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("TTS error: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .await
            .map_err(|e| format!("TTS error: {}", e))?;
        // Dropped here so `say` sees EOF and starts speaking.
    }

    wait_with_cancel(child, cancel).await
}

/// Wait for a `say` child, killing it as soon as `cancel` is raised.
///
/// A plain `child.wait()` makes cancellation advisory at best: the briefing is
/// a multi-second utterance, and until the child exits it also holds the
/// process-wide playback lock, so a cancel that cannot reach the child stalls
/// every other TTS consumer for the rest of the sentence.
async fn wait_with_cancel(
    mut child: tokio::process::Child,
    cancel: &AtomicBool,
) -> Result<std::process::ExitStatus, String> {
    loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("TTS error: {}", e))? {
            return Ok(status);
        }
        if cancel.load(Ordering::SeqCst) {
            let _ = child.start_kill();
            return child.wait().await.map_err(|e| format!("TTS error: {}", e));
        }
        tokio::time::sleep(std::time::Duration::from_millis(CANCEL_POLL_MS)).await;
    }
}

/// Rough spoken duration, used only to drive the amplitude animation.
///
/// Latin text is counted in whitespace-separated words. CJK has no
/// inter-word spaces, so a whole Chinese sentence would otherwise count as one
/// word and the animation would stop seconds before `say` does.
fn estimate_speech_ms(text: &str) -> u64 {
    let cjk_chars = text.chars().filter(|c| is_cjk(*c)).count();
    let latin_words = text
        .split_whitespace()
        .filter(|word| word.chars().any(|c| !is_cjk(c)))
        .count();
    (latin_words as u64) * 300 + (cjk_chars as u64) * 220
}

fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{3000}'..='\u{303F}'   // CJK punctuation
        | '\u{3040}'..='\u{30FF}' // kana
        | '\u{3400}'..='\u{4DBF}' // CJK unified ideographs extension A
        | '\u{4E00}'..='\u{9FFF}' // CJK unified ideographs
        | '\u{FF01}'..='\u{FF5E}' // fullwidth forms
    )
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
                while chars.peek() == Some(&'#') {
                    chars.next();
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
                continue;
            }
            // Convert markdown links [text](url) → "text"
            '[' => {
                let mut link_text = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    link_text.push(c);
                }
                // Skip the (url) part if present
                if chars.peek() == Some(&'(') {
                    chars.next();
                    let mut depth = 1;
                    for c in chars.by_ref() {
                        if c == '(' {
                            depth += 1;
                        }
                        if c == ')' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }
                }
                out.push_str(&link_text);
                continue;
            }
            // Strip bullet-point dashes at start of line: "- item" → "item"
            '-' if out.is_empty() || out.ends_with('\n') => {
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
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
            if !prev_space {
                result.push(' ');
            }
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
    let mut chars = 0usize;
    let mut prev_was_delimiter = false;
    for ch in text.chars() {
        buffer.push(ch);
        chars += 1;
        // CJK punctuation carries no trailing space, so it ends a sentence on
        // its own. Thresholds count characters, not bytes.
        let boundary = matches!(cjk_delimiter(ch), Some(DelimiterStrength::Hard))
            || (prev_was_delimiter && ch == ' ');
        if boundary && chars >= 20 {
            let trimmed = buffer.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(trimmed);
            }
            buffer.clear();
            chars = 0;
            prev_was_delimiter = false;
            continue;
        }
        prev_was_delimiter = matches!(ch, '.' | '!' | '?');
        if chars >= 200 {
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
            chars = buffer.chars().count();
        }
    }
    let trimmed = buffer.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }
    sentences
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    #[test]
    fn voice_for_picks_the_cantonese_voice_for_cjk() {
        // Chinese sentence -- English voice would drop this text entirely.
        assert_eq!(voice_for("今日天氣好好", "Samantha", "Sinji"), "Sinji");
        // English sentence -- stays on the configured English voice.
        assert_eq!(voice_for("Good morning", "Samantha", "Sinji"), "Samantha");
        // Mixed sentence -- any CJK content routes the whole sentence to the
        // Cantonese voice rather than losing the CJK half.
        assert_eq!(voice_for("Good morning 早晨", "Samantha", "Sinji"), "Sinji");
    }

    #[test]
    fn from_db_defaults_the_yue_voice_to_sinji() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE user_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .expect("create table");
        let db = crate::db::Database {
            conn: Mutex::new(conn),
        };
        let tts = TextToSpeech::from_db(&db);
        assert_eq!(tts.voice_yue, "Sinji");
    }

    fn segment(text: &str) -> Vec<String> {
        let mut buffer = SentenceBuffer::new();
        let mut out = Vec::new();
        for ch in text.chars() {
            if let Some(sentence) = buffer.push(ch) {
                out.push(sentence);
            }
        }
        if let Some(sentence) = buffer.take() {
            out.push(sentence);
        }
        out
    }

    #[test]
    fn segments_cantonese_on_full_width_punctuation() {
        // Before the fix this cut every 30 characters (90 bytes) and ignored
        // every sentence boundary.
        assert_eq!(
            segment("今日有三封重要郵件需要你處理，其中兩封來自客戶。下午三點有個會議，記得預先準備資料。"),
            vec![
                "今日有三封重要郵件需要你處理，其中兩封來自客戶。".to_string(),
                "下午三點有個會議，記得預先準備資料。".to_string(),
            ]
        );
    }

    #[test]
    fn segments_english_on_a_space_after_the_delimiter() {
        assert_eq!(
            segment("Hello there. This is a test."),
            vec!["Hello there.".to_string(), "This is a test.".to_string()]
        );
    }

    #[test]
    fn split_sentences_breaks_cantonese_at_full_stops() {
        let sentences = split_sentences(
            "今日有三封重要郵件需要你處理，其中兩封來自客戶。下午三點有個會議，記得預先準備資料。",
        );
        assert_eq!(
            sentences,
            vec![
                "今日有三封重要郵件需要你處理，其中兩封來自客戶。".to_string(),
                "下午三點有個會議，記得預先準備資料。".to_string(),
            ]
        );
    }

    #[test]
    fn split_sentences_still_splits_english() {
        assert_eq!(
            split_sentences("Here is the first sentence. And here is the second one."),
            vec![
                "Here is the first sentence.".to_string(),
                "And here is the second one.".to_string()
            ]
        );
    }

    #[test]
    fn clones_share_one_cancel_flag() {
        // lib.rs takes the briefing's TextToSpeech from VoiceEngine and chat.rs
        // cancels VoiceEngine's copy. Both only work because a clone shares the
        // flag -- with a privately constructed instance the briefing was
        // uncancellable and blocked the playback lock until it finished.
        let engine_tts = TextToSpeech::new();
        let briefing_tts = engine_tts.clone();
        assert!(!briefing_tts.is_cancelled());
        engine_tts.cancel();
        assert!(
            briefing_tts.is_cancelled(),
            "a cancel on one handle must reach the utterance actually playing"
        );
    }

    #[tokio::test]
    async fn wait_with_cancel_kills_a_running_child() {
        // The regression this guards: a briefing spoken as one `say` child ran
        // to completion no matter what, because nothing ever killed it.
        let cancel = Arc::new(AtomicBool::new(false));
        let child = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn a stand-in child");

        let flag = Arc::clone(&cancel);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            flag.store(true, Ordering::SeqCst);
        });

        let started = std::time::Instant::now();
        let status = wait_with_cancel(child, &cancel).await.expect("wait");
        assert!(!status.success(), "a killed child does not exit successfully");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "cancel must not have to wait for the utterance to finish"
        );
    }

    #[tokio::test]
    async fn an_utterance_waits_for_the_one_in_progress_instead_of_interleaving() {
        // R2-05: with only the per-sentence playback lock, a second speaker got
        // the lock between the first speaker's sentences -- tokio's mutex is
        // FIFO-fair -- and the user heard briefing / reply / briefing / reply.
        let tts = TextToSpeech::new();
        let in_progress = utterance_lock().lock().await;

        // Whitespace only: split_sentences yields nothing, so no `say` child is
        // spawned and what this measures is purely whether the second utterance
        // waits for the first.
        let second = tokio::spawn(async move { tts.speak("   ").await });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(
            !second.is_finished(),
            "a second utterance must wait for the one already speaking"
        );

        drop(in_progress);
        tokio::time::timeout(std::time::Duration::from_secs(5), second)
            .await
            .expect("the waiting utterance must run once the first one releases")
            .expect("join")
            .expect("speak");
    }

    #[tokio::test]
    async fn speak_rearms_the_cancel_flag_for_a_new_utterance() {
        // Otherwise one cancelled chat reply silences every later wake-word
        // acknowledgement, since nothing else clears the shared flag.
        let tts = TextToSpeech::new();
        tts.cancel();
        assert!(tts.is_cancelled());
        // Whitespace only: split_sentences yields nothing, so no `say` runs,
        // but the utterance is still armed.
        tts.speak("   ").await.expect("speak");
        assert!(!tts.is_cancelled());
    }

    /// Task 7 asked for the idle emit "before returning", and with TTS off
    /// that distinction is the whole fix: the consumer is spawned before the
    /// model call (commands/chat.rs, voice/commands.rs, voice/wake_word.rs all
    /// build StreamingTts first), so an emit at the top of the branch reports
    /// the turn finished before it starts. Ordering inside a spawned task that
    /// needs a real tauri::AppHandle cannot be observed from a unit test, so it
    /// is checked as a property of the source, bounded to the branch itself.
    #[test]
    fn the_disabled_branch_reports_idle_only_after_the_channel_closes() {
        let source = include_str!("tts.rs");
        let consumer = source
            .find("async fn streaming_tts_consumer(")
            .expect("streaming_tts_consumer must exist");
        let body = &source[consumer..];
        let branch_start = body
            .find("if !tts.enabled {")
            .expect("the disabled branch must exist");
        let branch_len = body[branch_start..]
            .find("\n    }\n")
            .expect("the disabled branch must be closed");
        let branch = &body[branch_start..branch_start + branch_len];

        let drain = branch
            .find("while rx.recv().await.is_some() {}")
            .expect("the disabled branch must drain the channel");
        let emit = branch
            .find("chat-state")
            .expect("the disabled branch must still return the HUD to idle");
        assert!(
            drain < emit,
            "idle must be emitted after the drain, not while the model call is still running"
        );
    }

    #[test]
    fn estimates_duration_from_characters_for_cjk() {
        // Three Latin words plus nine Han characters.
        assert_eq!(
            estimate_speech_ms("Checking your inbox 我幫你睇一睇封郵件"),
            3 * 300 + 9 * 220
        );
        // A spaceless Chinese sentence used to be counted as a single word.
        assert!(estimate_speech_ms("你好世界我係賈維斯") > 300);
    }
}
