# JARVIS UI/UX Animations & Conversation Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add conversation persistence, streaming sentence-by-sentence TTS, radial waveform visualization, and tool call energy arc animations to the JARVIS desktop assistant.

**Architecture:** Backend changes in Rust: sentence buffer in chat.rs that intercepts the streaming token flow and dispatches completed sentences to a TTS queue in tts.rs for immediate playback (true streaming -- first sentence speaks while AI still generates). Tool-call events emitted from AI providers. Voice pipeline unified through send_message. Frontend: event listeners in App.tsx, new props in JarvisScene.tsx for waveform and energy arcs. All rendering stays in the existing Canvas2D pipeline.

**Tech Stack:** Tauri v2 (Rust), React 18, TypeScript, Canvas2D, tokio (async), macOS `say` TTS

**Spec:** `docs/superpowers/specs/2026-03-24-jarvis-uiux-animations-design.md`

---

### Task 1: Conversation Persistence Fix

**Files:**
- Modify: `src/hooks/useChat.ts:20`
- Modify: `src/components/ChatPanel.tsx:13`

- [ ] **Step 1: Fix useChat to load and display history on mount**

The conversations already load on line 20 but the messages may not scroll into view. Verify the load works by checking the `getConversations()` call populates `messages` state. The current code at line 20 already does this:

```typescript
useEffect(() => { getConversations().then(setMessages).catch((e) => setError(String(e))); }, []);
```

This is correct. The issue is that ChatPanel only auto-scrolls when `messages` or `streamingText` changes during a session. On mount, the scroll effect fires before the async load completes. Fix by adding a dependency on `messages.length` for the scroll effect in ChatPanel.

In `src/components/ChatPanel.tsx`, the auto-scroll effect at line 13 needs to trigger after history loads. Read the current scroll effect and ensure it covers the initial load case. The scroll ref (`messagesEndRef`) should scroll into view whenever `messages` changes, including the initial load.

Check that `ChatPanel.tsx` line 13 has:
```typescript
useEffect(() => {
  messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
}, [messages, streamingText]);
```

If `messages` is already in the dependency array, the scroll should work on initial load. If not, add it.

- [ ] **Step 2: Test conversation persistence**

Run: `cd jarvis && npm run tauri dev`
1. Send a chat message
2. Close the app (Cmd+Q)
3. Reopen with `npm run tauri dev`
4. Open chat panel (Cmd+K)
Expected: Previous messages visible, scrolled to bottom

- [ ] **Step 3: Commit**

```bash
cd jarvis && git add src/hooks/useChat.ts src/components/ChatPanel.tsx
git commit -m "fix: ensure conversation history loads and scrolls on app restart"
```

---

### Task 2: TTS Sentence Queue in Rust Backend

**Files:**
- Modify: `src-tauri/src/voice/tts.rs`

- [ ] **Step 1: Add sentence queue infrastructure to TextToSpeech**

Add `speak_queued`, `cancel`, and amplitude simulation to `tts.rs`. The existing `speak()` method stays for backward compatibility. Add these imports and new methods:

```rust
use tokio::process::Command;
use tokio::sync::mpsc;
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
```

Update `new()` and `from_db()` to initialize `cancel_flag: Arc::new(AtomicBool::new(false))`.

Add these methods to the `impl TextToSpeech` block:

```rust
/// Cancel any in-progress TTS and clear the queue.
pub fn cancel(&self) {
    self.cancel_flag.store(true, Ordering::SeqCst);
}

/// Speak text sentence-by-sentence with events for each sentence.
/// Emits: tts-speaking, tts-sentence-done, tts-amplitude, chat-state
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

        // Emit speaking state on first sentence
        if i == 0 {
            let _ = app_handle.emit("chat-state", json!({"state": "speaking"}));
        }

        let _ = app_handle.emit("tts-speaking", json!({
            "sentence": sentence,
            "remaining": remaining
        }));

        // Start amplitude simulation in background
        let cancel = self.cancel_flag.clone();
        let ah = app_handle.clone();
        let word_count = sentence.split_whitespace().count();
        let est_duration_ms = (word_count as u64) * 300; // ~200wpm at rate 200
        let amp_handle = tokio::spawn(async move {
            let start = std::time::Instant::now();
            let interval = std::time::Duration::from_millis(66); // ~15Hz
            loop {
                if cancel.load(Ordering::SeqCst) { break; }
                let elapsed = start.elapsed().as_millis() as f64;
                if elapsed > est_duration_ms as f64 * 1.5 { break; }
                // Simulate amplitude with sine waves
                let t = elapsed / 1000.0;
                let amp = (0.4 + 0.3 * (t * 5.0).sin() + 0.2 * (t * 8.3).sin() + 0.1 * (t * 12.7).sin()).clamp(0.0, 1.0);
                let _ = ah.emit("tts-amplitude", json!({"amplitude": amp}));
                tokio::time::sleep(interval).await;
            }
        });

        // Speak the sentence
        let status = Command::new("say")
            .arg("-v").arg(&self.voice)
            .arg("-r").arg(self.rate.to_string())
            .arg(sentence)
            .status().await.map_err(|e| format!("TTS error: {}", e))?;

        // Stop amplitude simulation
        amp_handle.abort();
        let _ = app_handle.emit("tts-amplitude", json!({"amplitude": 0.0}));

        if !status.success() {
            log::warn!("TTS command failed for sentence: {}", &sentence[..sentence.len().min(50)]);
        }

        let _ = app_handle.emit("tts-sentence-done", json!({"remaining": remaining}));

        // Emit idle when last sentence done
        if remaining == 0 {
            let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Add the sentence splitter function**

Add this function outside the impl block in `tts.rs`:

```rust
/// Split text into sentences for TTS queuing.
/// Rules: split on .!? followed by space, minimum 20 chars, max 200 chars at word boundary.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut buffer = String::new();
    let mut prev_was_delimiter = false;

    for ch in text.chars() {
        buffer.push(ch);

        // Check for delimiter-then-space pattern (". " or "! " or "? ")
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

        // Max buffer: flush at nearest word boundary
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

    // Flush remaining
    let trimmed = buffer.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }

    sentences
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd jarvis/src-tauri && cargo check 2>&1 | head -20`
Expected: No errors (warnings OK)

- [ ] **Step 4: Commit**

```bash
cd jarvis && git add src-tauri/src/voice/tts.rs
git commit -m "feat: add sentence-queued TTS with amplitude simulation and cancel support"
```

---

### Task 3: Streaming Sentence TTS in Chat Command + Voice Pipeline Unification

**Files:**
- Modify: `src-tauri/src/commands/chat.rs`
- Modify: `src-tauri/src/voice/commands.rs:82-139`

The key insight: the AI providers already emit `chat-token` events as tokens stream. We intercept these tokens in `chat.rs` to buffer sentences and dispatch them to TTS immediately -- achieving true streaming TTS where the first sentence speaks while the AI still generates.

- [ ] **Step 1: Add a SentenceBuffer and streaming TTS dispatch to chat.rs**

In `src-tauri/src/commands/chat.rs`, add a `SentenceBuffer` struct and a background TTS consumer task. Add after the imports (line 7):

```rust
use tokio::sync::mpsc;

/// Buffers streaming tokens and dispatches complete sentences to TTS.
struct SentenceBuffer {
    buffer: String,
    prev_was_delimiter: bool,
    tx: mpsc::Sender<String>,
}

impl SentenceBuffer {
    fn new(tx: mpsc::Sender<String>) -> Self {
        Self { buffer: String::new(), prev_was_delimiter: false, tx }
    }

    /// Feed tokens from the AI stream. Complete sentences are sent to TTS queue.
    fn push(&mut self, token: &str) {
        for ch in token.chars() {
            self.buffer.push(ch);

            if self.prev_was_delimiter && ch == ' ' && self.buffer.len() >= 20 {
                let sentence = self.buffer.trim().to_string();
                if !sentence.is_empty() {
                    let _ = self.tx.try_send(sentence);
                }
                self.buffer.clear();
                self.prev_was_delimiter = false;
                continue;
            }

            self.prev_was_delimiter = matches!(ch, '.' | '!' | '?');

            // Max buffer: flush at word boundary
            if self.buffer.len() >= 200 {
                if let Some(last_space) = self.buffer.rfind(' ') {
                    let sentence = self.buffer[..last_space].trim().to_string();
                    let leftover = self.buffer[last_space..].trim().to_string();
                    if !sentence.is_empty() {
                        let _ = self.tx.try_send(sentence);
                    }
                    self.buffer = leftover;
                } else {
                    let sentence = self.buffer.trim().to_string();
                    if !sentence.is_empty() {
                        let _ = self.tx.try_send(sentence);
                    }
                    self.buffer.clear();
                }
            }
        }
    }

    /// Flush remaining buffer as the final sentence.
    fn flush(&mut self) {
        let sentence = self.buffer.trim().to_string();
        if !sentence.is_empty() {
            let _ = self.tx.try_send(sentence);
        }
        self.buffer.clear();
    }
}
```

- [ ] **Step 2: Add TTS cancellation and streaming sentence dispatch to send_message**

Replace the entire TTS section in `send_message` (lines 230-240) and add the streaming infrastructure. The approach: before calling `router.send()`, set up a `chat-token` listener that feeds a `SentenceBuffer`, which dispatches sentences to a TTS consumer task via mpsc channel.

After the user message insert (line 30), add TTS cancellation:
```rust
    // Cancel any in-progress TTS from previous response
    {
        let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
        tts.cancel();
        let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
        let _ = app_handle.emit("tts-amplitude", json!({"amplitude": 0.0}));
    }
```

Before the `router.send()` call (around line 215), set up the streaming TTS pipeline:
```rust
    // Set up streaming sentence TTS: tokens flow -> SentenceBuffer -> mpsc -> TTS consumer
    let (sentence_tx, mut sentence_rx) = mpsc::channel::<String>(32);
    let tts_for_consumer = engine.tts.lock().map_err(|e| e.to_string())?.clone();
    let ah_for_consumer = app_handle.clone();
    let tts_consumer = tokio::spawn(async move {
        let mut first = true;
        let mut count = 0u32;
        while let Some(sentence) = sentence_rx.recv().await {
            count += 1;
            if first {
                let _ = ah_for_consumer.emit("chat-state", json!({"state": "speaking"}));
                first = false;
            }
            // speak_sentence handles tts-speaking, tts-amplitude, tts-sentence-done events
            // remaining count is approximate since we don't know total upfront
            let _ = tts_for_consumer.speak_sentence(&sentence, 0, &ah_for_consumer).await;
        }
        // All sentences done
        if !first {
            let _ = ah_for_consumer.emit("chat-state", json!({"state": "idle"}));
        }
    });

    // Hook into the chat-token stream to feed the sentence buffer
    let sentence_tx_clone = sentence_tx.clone();
    let token_listener = app_handle.listen("chat-token", move |event| {
        // This is a simplified listener -- in practice, the SentenceBuffer
        // needs to be called from inside the router.send flow.
        // See Step 3 for the actual integration approach.
    });
```

**Actually, the cleaner approach:** Since `router.send()` already emits `chat-token` events, and we can't easily intercept those inside `chat.rs` without modifying the AI router, we take a simpler but equally effective approach: after `router.send()` returns the full text, we split and queue sentences. BUT we also register a `chat-token` listener that feeds the sentence buffer in real-time.

**Revised approach (simpler, still streaming):** Instead of intercepting tokens, we add a `listen` on `chat-token` inside `send_message` that feeds the SentenceBuffer directly. This gives us true streaming because tokens are emitted as the AI generates them.

Replace lines 214-240 with:
```rust
    log::info!("[STREAM-DEBUG] Emitting chat-state: thinking");
    let _ = app_handle.emit("chat-state", json!({"state": "thinking"}));

    // Set up streaming sentence TTS consumer
    let (sentence_tx, mut sentence_rx) = mpsc::channel::<String>(32);
    let tts_for_consumer = engine.tts.lock().map_err(|e| e.to_string())?.clone();
    let ah_for_tts = app_handle.clone();
    let engine_for_tts = engine.inner().clone();
    let tts_consumer = tokio::spawn(async move {
        let mut first = true;
        while let Some(sentence) = sentence_rx.recv().await {
            if tts_for_consumer.is_cancelled() { break; }
            if first {
                let _ = ah_for_tts.emit("chat-state", json!({"state": "speaking"}));
                // Mute mic during TTS to prevent feedback
                let _ = engine_for_tts.audio_router.lock().map(|mut r| r.mute());
                first = false;
            }
            let _ = tts_for_consumer.speak_sentence(&sentence, 0, &ah_for_tts).await;
        }
        if !first {
            let _ = ah_for_tts.emit("chat-state", json!({"state": "idle"}));
            let _ = engine_for_tts.audio_router.lock().map(|mut r| r.unmute());
        }
    });

    // Feed streaming tokens into SentenceBuffer via chat-token listener
    let sentence_tx_for_listener = sentence_tx.clone();
    let buffer = std::sync::Arc::new(std::sync::Mutex::new(SentenceBuffer::new(sentence_tx_for_listener)));
    let buffer_clone = buffer.clone();
    let token_listener_id = app_handle.listen("chat-token", move |event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            if payload.get("done").and_then(|d| d.as_bool()) == Some(true) {
                // Flush remaining buffer
                if let Ok(mut buf) = buffer_clone.lock() {
                    buf.flush();
                }
            } else if let Some(token) = payload.get("token").and_then(|t| t.as_str()) {
                if let Ok(mut buf) = buffer_clone.lock() {
                    buf.push(token);
                }
            }
        }
    });

    let response_text = match router.send(messages, &db, &google_auth, &app_handle).await {
        Ok(r) => r,
        Err(e) => {
            app_handle.unlisten(token_listener_id);
            drop(sentence_tx);
            let _ = tts_consumer.await;
            let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
            return Err(e);
        }
    };

    // Clean up: stop listening, drop sender to signal consumer to finish
    app_handle.unlisten(token_listener_id);
    // Flush any remaining buffer content
    if let Ok(mut buf) = buffer.lock() { buf.flush(); }
    drop(sentence_tx);
    // Wait for TTS to finish speaking all queued sentences
    let _ = tts_consumer.await;

    let final_response = response_text;
```

This replaces the old lines 214-240 entirely. The TTS now starts speaking the first complete sentence while the AI is still streaming tokens.

- [ ] **Step 3: Add speak_sentence method to tts.rs**

Add a simpler per-sentence method to `tts.rs` (the `speak_queued` method is no longer needed for the main path, but keep it for the search-response fallback):

```rust
/// Speak a single sentence with amplitude simulation and events.
pub async fn speak_sentence(&self, sentence: &str, remaining: usize, app_handle: &tauri::AppHandle) -> Result<(), String> {
    if !self.enabled || sentence.is_empty() || self.cancel_flag.load(Ordering::SeqCst) {
        return Ok(());
    }

    let _ = app_handle.emit("tts-speaking", json!({
        "sentence": sentence, "remaining": remaining
    }));

    // Amplitude simulation
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

pub fn is_cancelled(&self) -> bool {
    self.cancel_flag.load(Ordering::SeqCst)
}
```

- [ ] **Step 4: Update search-response TTS path**

Replace the search-response TTS section (lines 92-99 in chat.rs) with speak_queued:
```rust
                {
                    let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
                    if let Err(e) = tts.speak_queued(&search_response, &app_handle).await {
                        log::warn!("Chat TTS failed: {}", e);
                        let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    }
                }
```

- [ ] **Step 5: Unify voice pipeline through send_message**

In `src-tauri/src/voice/commands.rs`, replace the entire AI call + save + TTS section (lines 91-139) with a call to `send_message` via the Tauri command system. After transcription completes and the user message is saved (line 87), instead of calling `router.send()` and `tts.speak()` directly, emit the transcribed text as a `chat-new-message` and invoke `send_message`:

Replace lines 91-139 with:
```rust
    let _ = app_handle.emit("chat-state", json!({"state": "thinking"}));

    // Route through unified send_message path for consistent sentence TTS,
    // tool call events, and cancellation behavior
    let response = match crate::commands::chat::send_message_inner(
        &app_handle, &db, &router, &google_auth, &engine, text.clone()
    ).await {
        Ok(msg) => msg.content,
        Err(e) => {
            engine.set_state_and_emit(VoiceState::Error(e.clone()));
            return Err(e);
        }
    };

    engine.set_state_and_emit(VoiceState::Idle);
    Ok(response)
```

This requires extracting the core logic of `send_message` into a `send_message_inner` function that takes references instead of Tauri State types. In `chat.rs`, refactor:

```rust
/// Inner function callable from both the Tauri command and voice pipeline.
pub async fn send_message_inner(
    app_handle: &tauri::AppHandle,
    db: &Database,
    router: &AiRouter,
    google_auth: &crate::auth::google::GoogleAuth,
    engine: &VoiceEngine,
    message: String,
) -> Result<ChatMessage, String> {
    // ... (move the existing send_message body here, using direct refs instead of State<>)
}

#[tauri::command]
pub async fn send_message(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    google_auth: State<'_, Arc<crate::auth::google::GoogleAuth>>,
    engine: State<'_, Arc<VoiceEngine>>,
    message: String,
) -> Result<ChatMessage, String> {
    send_message_inner(&app_handle, &db, &router, &google_auth, &engine, message).await
}
```

The voice `stop_listening` command then calls `send_message_inner` directly, skipping the user message insert (since it already inserted the message at line 85). Adjust accordingly -- the `send_message_inner` should accept a `skip_user_insert: bool` parameter or the voice command should just handle the insert separately.

- [ ] **Step 6: Verify it compiles**

Run: `cd jarvis/src-tauri && cargo check 2>&1 | head -20`
Expected: No errors

- [ ] **Step 7: Commit**

```bash
cd jarvis && git add src-tauri/src/commands/chat.rs src-tauri/src/voice/commands.rs src-tauri/src/voice/tts.rs
git commit -m "feat: streaming sentence TTS with real-time token buffering and unified voice pipeline"
```

---

### Task 4: Emit Tool Call Events from AI Providers

**Files:**
- Modify: `src-tauri/src/ai/claude.rs:243-246`
- Modify: `src-tauri/src/ai/openai.rs:223-226`

- [ ] **Step 1: Add chat-tool-call emission in claude.rs**

In `src-tauri/src/ai/claude.rs`, in the tool execution loop (around line 243), add the event emission BEFORE `execute_tool`:

Find this code:
```rust
            for (id, name, input) in &tool_uses {
                let args_str = serde_json::to_string(input).unwrap_or_default();
                log::info!("JARVIS tool call: {}({})", name, args_str);
                let result = tools::execute_tool(name, &args_str, db, google_auth).await;
```

Add the emit line before `execute_tool`:
```rust
            for (id, name, input) in &tool_uses {
                let args_str = serde_json::to_string(input).unwrap_or_default();
                log::info!("JARVIS tool call: {}({})", name, args_str);
                let _ = app_handle.emit("chat-tool-call", json!({"tool_name": name}));
                let result = tools::execute_tool(name, &args_str, db, google_auth).await;
```

- [ ] **Step 2: Add chat-tool-call emission in openai.rs**

In `src-tauri/src/ai/openai.rs`, in the tool execution loop (around line 223), add the event emission BEFORE `execute_tool`:

Find this code:
```rust
            for tc in &tool_calls_vec {
                log::info!("JARVIS tool call: {}({})", tc.function.name, tc.function.arguments);
                let result = tools::execute_tool(&tc.function.name, &tc.function.arguments, db, google_auth).await;
```

Add the emit line before `execute_tool`:
```rust
            for tc in &tool_calls_vec {
                log::info!("JARVIS tool call: {}({})", tc.function.name, tc.function.arguments);
                let _ = app_handle.emit("chat-tool-call", json!({"tool_name": tc.function.name}));
                let result = tools::execute_tool(&tc.function.name, &tc.function.arguments, db, google_auth).await;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd jarvis/src-tauri && cargo check 2>&1 | head -20`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
cd jarvis && git add src-tauri/src/ai/claude.rs src-tauri/src/ai/openai.rs
git commit -m "feat: emit chat-tool-call events from AI providers before tool execution"
```

---

### Task 5: Frontend Event Listeners and Types

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/hooks/useChat.ts`
- Modify: `src/App.tsx:122`

- [ ] **Step 1: Add new types**

In `src/lib/types.ts`, add after the `ChatStatePayload` type (after line 131):

```typescript
export interface TtsAmplitudePayload {
  amplitude: number;
}

export interface ToolCallPayload {
  tool_name: string;
}
```

- [ ] **Step 2: Add event listeners in App.tsx only (not useChat.ts)**

The `tts-amplitude` and `chat-tool-call` events are consumed by JarvisScene (via App.tsx), not by the chat UI. Add listeners only in App.tsx to avoid duplicate registrations.

No changes to `useChat.ts` for these events -- it already handles `chat-state` for `aiState`.

In `src/App.tsx`, after line 29, add:
```typescript
  const [ttsAmplitude, setTtsAmplitude] = useState(0);
  const [pendingToolCall, setPendingToolCall] = useState<string | null>(null);
```

Add a new useEffect after the AI state listener (after line 64):
```typescript
  // Listen for TTS amplitude and tool call events
  useEffect(() => {
    const unlistenAmp = listen<{ amplitude: number }>("tts-amplitude", (event) => {
      setTtsAmplitude(event.payload.amplitude);
    });
    const unlistenTool = listen<{ tool_name: string }>("chat-tool-call", (event) => {
      setPendingToolCall(event.payload.tool_name);
    });
    return () => {
      unlistenAmp.then((fn) => fn());
      unlistenTool.then((fn) => fn());
    };
  }, []);
```

Update the JarvisScene render (line 122) to pass new props:
```tsx
<JarvisScene activityLevel={getActivityLevel()} ttsAmplitude={ttsAmplitude} pendingToolCall={pendingToolCall} onToolCallConsumed={() => setPendingToolCall(null)} />
```

- [ ] **Step 4: Verify frontend compiles (will have type error for JarvisScene props -- that's expected, fixed in Task 6)**

Run: `cd jarvis && npx tsc --noEmit 2>&1 | head -20`
Expected: Error about JarvisScene props not matching (fixed in next task)

- [ ] **Step 5: Commit**

```bash
cd jarvis && git add src/lib/types.ts src/hooks/useChat.ts src/App.tsx
git commit -m "feat: add frontend event listeners for TTS amplitude and tool call events"
```

---

### Task 6: Radial Waveform in JarvisScene

**Files:**
- Modify: `src/components/3d/JarvisScene.tsx:78-80` (props)
- Modify: `src/components/3d/JarvisScene.tsx:456-491` (replace sound-bar with radial waveform)
- Modify: `src/components/3d/JarvisScene.tsx:421-454` (core crossfade)

- [ ] **Step 1: Update JarvisScene props interface**

In `src/components/3d/JarvisScene.tsx`, update the props interface at line 78:

```typescript
interface JarvisSceneProps {
  activityLevel?: "idle" | "listening" | "processing" | "active";
  ttsAmplitude?: number;
  pendingToolCall?: string | null;
  onToolCallConsumed?: () => void;
}
```

Update the component function signature at line 82:
```typescript
export default function JarvisScene({ activityLevel = "idle", ttsAmplitude = 0, pendingToolCall = null, onToolCallConsumed }: JarvisSceneProps) {
```

- [ ] **Step 2: Add speakingAlpha ref and update logic**

After the existing refs (around line 100), add:
```typescript
  const speakingAlpha = useRef(0);
  const ttsAmpRef = useRef(0);
  const arcsRef = useRef<EnergyArc[]>([]);
  const ringSpeedRef = useRef(1.0);
```

Add the EnergyArc interface near the top of the file (after DataNode interface, around line 28):
```typescript
interface EnergyArc {
  targetIdx: number;
  progress: number;
  speed: number;
  trail: { x: number; y: number }[];
  color: { r: number; g: number; b: number };
  active: boolean;
  side: number; // 1 or -1 for curve direction
}
```

- [ ] **Step 3: Update ttsAmpRef when prop changes**

Add a useEffect to sync the prop to the ref (after the existing useEffects, before the main animate useEffect):
```typescript
  useEffect(() => {
    ttsAmpRef.current = ttsAmplitude;
  }, [ttsAmplitude]);
```

- [ ] **Step 4: Handle pendingToolCall to create energy arcs**

Add a useEffect for tool calls:
```typescript
  useEffect(() => {
    if (pendingToolCall && nodes.current.length > 0) {
      // Map tool name to node type
      const name = pendingToolCall.toLowerCase();
      let nodeType = "task";
      if (name.includes("calendar") || name.includes("event")) nodeType = "meeting";
      else if (name.includes("email") || name.includes("gmail")) nodeType = "email";
      else if (name.includes("github") || name.includes("pr") || name.includes("issue")) nodeType = "github";
      else if (name.includes("notion")) nodeType = "notion";
      else if (name.includes("cron") || name.includes("schedule")) nodeType = "cron";
      else if (name.includes("note") || name.includes("obsidian")) nodeType = "notion";

      // Find a matching node
      const candidates = nodes.current.filter(n => n.type === nodeType);
      const target = candidates.length > 0
        ? candidates[Math.floor(Math.random() * candidates.length)]
        : nodes.current[Math.floor(Math.random() * nodes.current.length)];

      const targetIdx = nodes.current.indexOf(target);
      const color = TYPE_COLORS[target.type] || TYPE_COLORS.task;

      arcsRef.current.push({
        targetIdx,
        progress: 0,
        speed: 0.025 + Math.random() * 0.015,
        trail: [],
        color,
        active: true,
        side: arcsRef.current.length % 2 === 0 ? 1 : -1,
      });

      onToolCallConsumed?.();
    }
  }, [pendingToolCall, onToolCallConsumed]);
```

- [ ] **Step 5: Add speakingAlpha interpolation in the animate loop**

Inside the `animate()` function, after the activity interpolation (around line 311), add:
```typescript
      // Speaking crossfade
      const isSpeaking = ttsAmpRef.current > 0.01;
      const speakTarget = isSpeaking ? 1 : 0;
      const speakSpeed = isSpeaking ? 0.06 : 0.035; // 300ms in, 500ms out
      speakingAlpha.current += (speakTarget - speakingAlpha.current) * speakSpeed;
      const spkAlpha = speakingAlpha.current;
```

- [ ] **Step 6: Add orbital ring speed multiplier**

In the animate loop, after the speakingAlpha interpolation, add ring speed interpolation with distinct acceleration/deceleration rates:
```typescript
      // Ring speed multiplier (500ms accel, 800ms decel via different lerp rates)
      const ringTarget = activityLevel === "processing" ? 3.0 : activityLevel === "listening" ? 1.2 : 1.0;
      const ringLerp = ringTarget > ringSpeedRef.current ? 0.05 : 0.03; // 500ms accel, 800ms decel
      ringSpeedRef.current += (ringTarget - ringSpeedRef.current) * ringLerp;
      const ringMult = ringSpeedRef.current;
```

In the orbital rings section (around line 386-419), replace the energy dot speed line. Find:
```typescript
        const da = time.current * ring.spd * (1 + act * 2.0);
```

Replace with:
```typescript
        const da = time.current * ring.spd * ringMult;
```

After drawing the energy dot (line 418), add trailing afterimage when speed is high:
```typescript
        // Speed trail when processing
        if (ringMult > 1.5) {
          for (let ti = 1; ti <= 4; ti++) {
            const trailA = da - ti * 0.15 * ring.spd;
            let tp: Point3D = { x: Math.cos(trailA) * ring.r, y: Math.sin(trailA) * ring.r, z: 0 };
            tp = rotateX(tp, ring.tx); tp = rotateZ(tp, ring.tz); tp = transform(tp, ry, rx);
            const tpr = project(tp, cx, cy, fov);
            const trailAlpha = 0.9 - ti * 0.22;
            if (trailAlpha > 0) {
              ctx.beginPath(); ctx.arc(tpr.x, tpr.y, 1.5 * tpr.scale, 0, Math.PI * 2);
              ctx.fillStyle = `rgba(180, 240, 255, ${trailAlpha})`; ctx.fill();
            }
          }
        }
```

- [ ] **Step 7: Modify core wireframe to crossfade with waveform**

In the core wireframe section (lines 435-454), wrap the wireframe rendering with the crossfade alpha:

Find:
```typescript
      ctx.strokeStyle = "rgba(100, 220, 255, 0.2)";
```

Replace with:
```typescript
      const coreAlpha = 0.2 * (1 - spkAlpha * 0.7);
      ctx.strokeStyle = `rgba(100, 220, 255, ${coreAlpha})`;
```

Also in the core glow section (lines 426-434), increase glow radius during speech. Find:
```typescript
      const cR = CORE_RADIUS * breath;
```

Replace with:
```typescript
      const cR = CORE_RADIUS * breath * (1 + spkAlpha * 0.15); // grows ~15% during speech (50->57px)
```
```

- [ ] **Step 8: Replace the sound-bar visualizer with the radial waveform**

Replace the entire sound-bar section (lines 456-491) with the radial waveform:

```typescript
      // === RADIAL WAVEFORM (speaking state) ===
      if (spkAlpha > 0.01) {
        const waveAlpha = spkAlpha;
        const amp = ttsAmpRef.current;
        const barCount = 48;
        const innerR = 18;
        const maxOuterR = 55;

        // Inner ring
        ctx.strokeStyle = `rgba(0, 180, 255, ${0.3 * waveAlpha})`;
        ctx.lineWidth = 1;
        ctx.beginPath(); ctx.arc(cx, cy, innerR, 0, Math.PI * 2); ctx.stroke();

        // Center dot
        ctx.fillStyle = `rgba(0, 180, 255, ${0.9 * waveAlpha})`;
        ctx.beginPath(); ctx.arc(cx, cy, 3, 0, Math.PI * 2); ctx.fill();

        // Radial bars
        for (let bi = 0; bi < barCount; bi++) {
          const angle = (bi / barCount) * Math.PI * 2;
          // Offset each bar's amplitude by angle for rotating wave pattern
          const barAmp = amp * (0.3 + 0.7 * (
            0.5 * Math.sin(time.current * 0.07 + bi * 0.5) +
            0.3 * Math.sin(time.current * 0.11 + bi * 0.8) +
            0.2 * Math.sin(time.current * 0.15 + bi * 1.3)
          ));
          const norm = Math.max(0, Math.min(1, (barAmp + 1) / 2));
          const barLen = innerR + norm * (maxOuterR - innerR);

          const x1 = cx + Math.cos(angle) * innerR;
          const y1 = cy + Math.sin(angle) * innerR;
          const x2 = cx + Math.cos(angle) * barLen;
          const y2 = cy + Math.sin(angle) * barLen;

          ctx.strokeStyle = `rgba(0, 180, 255, ${(0.4 + norm * 0.5) * waveAlpha})`;
          ctx.lineWidth = 2.5;
          ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke();

          // Bright tip dot
          ctx.fillStyle = `rgba(0, 180, 255, ${norm * 0.9 * waveAlpha})`;
          ctx.beginPath(); ctx.arc(x2, y2, 2, 0, Math.PI * 2); ctx.fill();
        }
      }
```

- [ ] **Step 9: Verify frontend compiles**

Run: `cd jarvis && npx tsc --noEmit 2>&1 | head -20`
Expected: No errors

- [ ] **Step 10: Commit**

```bash
cd jarvis && git add src/components/3d/JarvisScene.tsx
git commit -m "feat: add radial waveform visualization with speakingAlpha crossfade in atom core"
```

---

### Task 7: Energy Arc Animation in JarvisScene

**Files:**
- Modify: `src/components/3d/JarvisScene.tsx` (inside animate loop, after data nodes section)

- [ ] **Step 1: Add energy arc rendering and update logic**

In the animate loop, after the data nodes rendering section (after the legend/scan-line sections, around line 684), add the energy arc rendering. Note: we need access to the `transformed` array from the data nodes section (line 498), so the arc rendering should go AFTER the node rendering but BEFORE the cursor hint.

Find a suitable location after the node rendering loop (around line 645) and add:

```typescript
      // === ENERGY ARCS ===
      const arcs = arcsRef.current;
      for (let ai = arcs.length - 1; ai >= 0; ai--) {
        const arc = arcs[ai];
        if (!arc.active) { arcs.splice(ai, 1); continue; }

        const targetNode = nodes.current[arc.targetIdx];
        if (!targetNode) { arcs.splice(ai, 1); continue; }

        // Get target screen position
        const tCart = sphereToCart(targetNode.theta, targetNode.phi, targetNode.r);
        const tRot = transform(tCart, ry, rx);
        const tProj = project(tRot, cx, cy, fov);

        arc.progress += arc.speed;
        const prog = Math.min(1, arc.progress);
        const eased = 1 - Math.pow(1 - prog, 3); // ease-out cubic

        // Quadratic bezier from center to target with perpendicular offset
        const midX = cx + (tProj.x - cx) * 0.5 + Math.sin(prog * Math.PI) * 40 * arc.side;
        const midY = cy + (tProj.y - cy) * 0.5 - Math.sin(prog * Math.PI) * 30;
        const headX = (1-eased)*(1-eased)*cx + 2*(1-eased)*eased*midX + eased*eased*tProj.x;
        const headY = (1-eased)*(1-eased)*cy + 2*(1-eased)*eased*midY + eased*eased*tProj.y;

        arc.trail.push({ x: headX, y: headY });
        if (arc.trail.length > 15) arc.trail.shift();

        // Draw trail
        for (let ti = 1; ti < arc.trail.length; ti++) {
          const trailAlpha = (ti / arc.trail.length) * 0.7;
          const trailWidth = (ti / arc.trail.length) * 2.5;
          ctx.strokeStyle = `rgba(${arc.color.r}, ${arc.color.g}, ${arc.color.b}, ${trailAlpha})`;
          ctx.lineWidth = trailWidth;
          ctx.beginPath();
          ctx.moveTo(arc.trail[ti-1].x, arc.trail[ti-1].y);
          ctx.lineTo(arc.trail[ti].x, arc.trail[ti].y);
          ctx.stroke();
        }

        // Glowing head
        if (prog < 1) {
          const hg = ctx.createRadialGradient(headX, headY, 0, headX, headY, 8);
          hg.addColorStop(0, `rgba(${arc.color.r}, ${arc.color.g}, ${arc.color.b}, 0.9)`);
          hg.addColorStop(1, `rgba(${arc.color.r}, ${arc.color.g}, ${arc.color.b}, 0)`);
          ctx.beginPath(); ctx.arc(headX, headY, 8, 0, Math.PI * 2);
          ctx.fillStyle = hg; ctx.fill();
        }

        // Arc arrived at target
        if (prog >= 1) {
          arc.active = false;
          // Flash the target node -- we store flash state directly on the node
          (targetNode as any)._flashAlpha = 1.0;
          (targetNode as any)._flashScale = 1.8;
        }
      }
```

- [ ] **Step 2: Add flash decay and rendering to the node rendering section**

In the data node rendering loop (around line 527-645), find where node dots are drawn. We need to add flash alpha and scale handling. Find the node dot drawing section (around line 543-560) and modify it to include flash effects.

After `const sz = node.size * sc * dpr.scale;` (or wherever the node size is computed), add:

```typescript
        // Flash effect from energy arc arrival
        const flashAlpha = (node as any)._flashAlpha || 0;
        const flashScale = (node as any)._flashScale || 1;
        if (flashAlpha > 0.01) {
          (node as any)._flashAlpha *= 0.96;
          (node as any)._flashScale += (1 - (node as any)._flashScale) * 0.08;
        }
        const finalSz = sz * flashScale;
```

Use `finalSz` instead of `sz` for the node dot radius. Add a flash glow ring after the node dot:

```typescript
        // Flash glow ring
        if (flashAlpha > 0.1) {
          ctx.strokeStyle = `rgba(${col.r}, ${col.g}, ${col.b}, ${flashAlpha * 0.5})`;
          ctx.lineWidth = 1.5;
          ctx.beginPath(); ctx.arc(dpr.x, dpr.y, finalSz + 6 * flashAlpha, 0, Math.PI * 2); ctx.stroke();
        }
```

- [ ] **Step 3: Verify it compiles and test visually**

Run: `cd jarvis && npx tsc --noEmit 2>&1 | head -20`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
cd jarvis && git add src/components/3d/JarvisScene.tsx
git commit -m "feat: add energy arc animations from atom core to data nodes on tool calls"
```

---

### Task 8: Integration Test

**Files:** None (manual testing)

- [ ] **Step 1: Run the full app and test the complete flow**

Run: `cd jarvis && npm run tauri dev`

Test each feature:

1. **Conversation persistence:** Send a message, close app (Cmd+Q), reopen, check messages persist
2. **Sentence TTS:** Send "Tell me about the weather in Hong Kong and what I should wear today" -- should start speaking the first sentence before the full response is generated
3. **Radial waveform:** While JARVIS speaks, observe the atom core -- radial bars should pulse
4. **Tool call arcs:** Ask "What meetings do I have today?" -- observe energy arcs shoot to meeting nodes when the calendar tool is called
5. **Cancellation:** While JARVIS is speaking, send a new message -- speech should stop immediately

- [ ] **Step 2: Commit any fixes needed**

If any issues found, fix and commit with descriptive messages.

- [ ] **Step 3: Final commit (if any fixes were made)**

```bash
cd jarvis && git add src/hooks/useChat.ts src/components/ChatPanel.tsx src/components/3d/JarvisScene.tsx src/App.tsx src/lib/types.ts src-tauri/src/commands/chat.rs src-tauri/src/voice/tts.rs src-tauri/src/voice/commands.rs src-tauri/src/ai/claude.rs src-tauri/src/ai/openai.rs
git commit -m "fix: integration fixes for JARVIS UIUX animations"
```
