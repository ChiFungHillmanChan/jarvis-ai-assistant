# JARVIS Phase 2a: Voice Activation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add voice interaction to JARVIS -- press a hotkey or say a wake word to start listening, Whisper transcribes speech locally, AI responds, and macOS speaks the response aloud.

**Architecture:** Audio pipeline runs entirely in the Rust backend. `cpal` captures microphone audio as PCM samples. Two activation modes: (1) push-to-talk hotkey (Cmd+Shift+J, works immediately) and (2) optional Porcupine wake word detection (requires free Picovoice access key). Once activated, audio is buffered until silence is detected, then sent to `whisper-rs` for local transcription. The transcribed text is routed through the existing AI router (Claude/OpenAI), and the response is spoken via macOS `say` command. Frontend shows a voice indicator overlay during listening/processing.

**Tech Stack:** cpal (audio capture), whisper-rs (local STT), pv_porcupine (optional wake word), macOS `say` command (TTS), Tauri global shortcuts, Tauri events (backend-to-frontend state updates).

**Spec:** `docs/superpowers/specs/2026-03-23-jarvis-assistant-design.md`

**Depends on:** Phase 1 complete.

**Prerequisites:** Download a Whisper model file (ggml-base.bin, ~141MB). Optional: Picovoice access key for wake word.

---

## File Structure (new/modified files only)

```
jarvis/
├── src/
│   ├── components/
│   │   └── VoiceIndicator.tsx              # NEW: listening/processing/speaking overlay
│   ├── hooks/
│   │   └── useVoiceState.ts                # NEW: listen to voice state events from backend
│   ├── pages/
│   │   └── Settings.tsx                    # Update: voice settings panel
│   └── App.tsx                             # Update: render VoiceIndicator
├── .env.example                            # + PICOVOICE_ACCESS_KEY (optional)
│
└── src-tauri/
    ├── Cargo.toml                          # + cpal, whisper-rs, pv_porcupine
    ├── tauri.conf.json                     # + Info.plist microphone permission
    ├── Info.plist                           # NEW: NSMicrophoneUsageDescription
    └── src/
        ├── lib.rs                          # + voice module, start voice engine
        └── voice/
            ├── mod.rs                      # Voice engine coordinator
            ├── capture.rs                  # Microphone audio capture via cpal
            ├── wake_word.rs                # Optional Porcupine wake word detection
            ├── transcribe.rs               # Whisper STT transcription
            ├── tts.rs                      # macOS TTS via `say` command
            └── commands.rs                 # Tauri commands: toggle voice, set settings
```

---

## Task 1: Add Dependencies & Microphone Permission

**Files:**
- Modify: `jarvis/src-tauri/Cargo.toml`
- Create: `jarvis/src-tauri/Info.plist`
- Modify: `jarvis/src-tauri/tauri.conf.json`

- [ ] **Step 1: Add Rust dependencies**

Add to `[dependencies]` in `jarvis/src-tauri/Cargo.toml`:

```toml
cpal = "0.15"
whisper-rs = "0.16"
hound = "3.5"
```

Note: `pv_porcupine` is optional and added later. We start with push-to-talk mode.

- [ ] **Step 2: Create Info.plist for microphone permission**

Create `jarvis/src-tauri/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSMicrophoneUsageDescription</key>
    <string>JARVIS needs microphone access for voice commands</string>
</dict>
</plist>
```

- [ ] **Step 3: Update tauri.conf.json bundle settings**

Read `jarvis/src-tauri/tauri.conf.json` and add to the `bundle` section:

```json
"macOS": {
  "infoPlist": {
    "NSMicrophoneUsageDescription": "JARVIS needs microphone access for voice commands"
  }
}
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check --manifest-path jarvis/src-tauri/Cargo.toml
```

Note: whisper-rs compiles whisper.cpp from source -- first build will take a while.

- [ ] **Step 5: Commit**

```bash
git add jarvis/src-tauri/Cargo.toml jarvis/src-tauri/Info.plist jarvis/src-tauri/tauri.conf.json
git commit -m "feat: add cpal, whisper-rs deps and microphone permission for voice"
```

---

## Task 2: Audio Capture Module

**Files:**
- Create: `jarvis/src-tauri/src/voice/capture.rs`

- [ ] **Step 1: Create capture.rs**

This module captures microphone audio as f32 PCM samples at 16kHz (Whisper's required sample rate).

```rust
// jarvis/src-tauri/src/voice/capture.rs
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<Mutex<bool>>,
}

impl AudioCapture {
    pub fn new() -> Result<Self, String> {
        Ok(AudioCapture {
            stream: None,
            buffer: Arc::new(Mutex::new(Vec::new())),
            is_recording: Arc::new(Mutex::new(false)),
        })
    }

    pub fn start_recording(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host.default_input_device()
            .ok_or("No input device available")?;

        log::info!("Using input device: {}", device.name().unwrap_or_default());

        // Whisper needs 16kHz mono f32
        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(16000),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer = Arc::clone(&self.buffer);
        let is_recording = Arc::clone(&self.is_recording);

        // Clear buffer
        buffer.lock().unwrap().clear();
        *is_recording.lock().unwrap() = true;

        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if *is_recording.lock().unwrap() {
                    buffer.lock().unwrap().extend_from_slice(data);
                }
            },
            |err| log::error!("Audio capture error: {}", err),
            None,
        ).map_err(|e| format!("Failed to build input stream: {}", e))?;

        stream.play().map_err(|e| format!("Failed to start stream: {}", e))?;
        self.stream = Some(stream);

        log::info!("Audio capture started");
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Vec<f32> {
        *self.is_recording.lock().unwrap() = false;
        self.stream = None; // drops the stream, stops capture

        let samples = self.buffer.lock().unwrap().clone();
        self.buffer.lock().unwrap().clear();

        log::info!("Audio capture stopped: {} samples ({:.1}s)", samples.len(), samples.len() as f32 / 16000.0);
        samples
    }

    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock().unwrap()
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add jarvis/src-tauri/src/voice/capture.rs
git commit -m "feat: add microphone audio capture module via cpal"
```

---

## Task 3: Whisper Transcription Module

**Files:**
- Create: `jarvis/src-tauri/src/voice/transcribe.rs`

- [ ] **Step 1: Create transcribe.rs**

```rust
// jarvis/src-tauri/src/voice/transcribe.rs
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
use std::path::PathBuf;

pub struct Transcriber {
    ctx: WhisperContext,
}

impl Transcriber {
    pub fn new(model_path: &str) -> Result<Self, String> {
        let params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(model_path, params)
            .map_err(|e| format!("Failed to load Whisper model: {}", e))?;
        log::info!("Whisper model loaded from {}", model_path);
        Ok(Transcriber { ctx })
    }

    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        if audio_data.len() < 1600 {
            // Less than 0.1s of audio -- skip
            return Ok(String::new());
        }

        let mut state = self.ctx.create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_single_segment(true);

        state.full(params, audio_data)
            .map_err(|e| format!("Whisper transcription failed: {}", e))?;

        let num_segments = state.full_n_segments()
            .map_err(|e| format!("Failed to get segments: {}", e))?;

        let mut text = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = state.full_get_segment_text(i) {
                text.push_str(&segment);
            }
        }

        let text = text.trim().to_string();
        log::info!("Transcribed: \"{}\"", text);
        Ok(text)
    }

    /// Get the default model path in the app data directory
    pub fn default_model_path() -> PathBuf {
        let data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        data_dir.join("jarvis").join("models").join("ggml-base.bin")
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add jarvis/src-tauri/src/voice/transcribe.rs
git commit -m "feat: add Whisper speech-to-text transcription module"
```

---

## Task 4: Text-to-Speech Module

**Files:**
- Create: `jarvis/src-tauri/src/voice/tts.rs`

- [ ] **Step 1: Create tts.rs**

Uses macOS `say` command for simplicity and reliability. Runs async to avoid blocking.

```rust
// jarvis/src-tauri/src/voice/tts.rs
use tokio::process::Command;

pub struct TextToSpeech {
    voice: String,
    rate: u32,
    enabled: bool,
}

impl TextToSpeech {
    pub fn new() -> Self {
        TextToSpeech {
            voice: "Samantha".to_string(), // Default macOS voice
            rate: 200,
            enabled: true,
        }
    }

    pub fn set_voice(&mut self, voice: String) {
        self.voice = voice;
    }

    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub async fn speak(&self, text: &str) -> Result<(), String> {
        if !self.enabled || text.is_empty() {
            return Ok(());
        }

        let status = Command::new("say")
            .arg("-v")
            .arg(&self.voice)
            .arg("-r")
            .arg(self.rate.to_string())
            .arg(text)
            .status()
            .await
            .map_err(|e| format!("TTS error: {}", e))?;

        if !status.success() {
            return Err("TTS command failed".to_string());
        }

        Ok(())
    }

    /// List available macOS voices
    pub async fn list_voices() -> Result<Vec<String>, String> {
        let output = Command::new("say")
            .arg("-v")
            .arg("?")
            .output()
            .await
            .map_err(|e| format!("Failed to list voices: {}", e))?;

        let text = String::from_utf8_lossy(&output.stdout);
        let voices: Vec<String> = text
            .lines()
            .filter_map(|line| {
                line.split_whitespace().next().map(|s| s.to_string())
            })
            .collect();

        Ok(voices)
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add jarvis/src-tauri/src/voice/tts.rs
git commit -m "feat: add macOS text-to-speech module via say command"
```

---

## Task 5: Voice Engine Coordinator

**Files:**
- Create: `jarvis/src-tauri/src/voice/mod.rs`
- Modify: `jarvis/src-tauri/src/lib.rs`

- [ ] **Step 1: Create voice/mod.rs**

This is the main coordinator that ties capture, transcription, and TTS together. It manages the voice state machine: Idle -> Listening -> Processing -> Speaking -> Idle.

```rust
// jarvis/src-tauri/src/voice/mod.rs
pub mod capture;
pub mod commands;
pub mod transcribe;
pub mod tts;

use capture::AudioCapture;
use transcribe::Transcriber;
use tts::TextToSpeech;

use std::sync::Mutex;

#[derive(Clone, serde::Serialize, Debug, PartialEq)]
pub enum VoiceState {
    Idle,
    Listening,
    Processing,
    Speaking,
    Error(String),
    Disabled,
}

pub struct VoiceEngine {
    pub capture: Mutex<AudioCapture>,
    pub transcriber: Mutex<Option<Transcriber>>,
    pub tts: Mutex<TextToSpeech>,
    pub state: Mutex<VoiceState>,
    pub enabled: Mutex<bool>,
    pub muted: Mutex<bool>,
}

impl VoiceEngine {
    pub fn new() -> Self {
        let capture = AudioCapture::new().unwrap_or_else(|e| {
            log::warn!("Audio capture init failed: {}. Voice disabled.", e);
            AudioCapture::new().unwrap() // Will fail gracefully at runtime
        });

        // Try to load Whisper model
        let model_path = Transcriber::default_model_path();
        let transcriber = if model_path.exists() {
            match Transcriber::new(model_path.to_str().unwrap_or("")) {
                Ok(t) => {
                    log::info!("Whisper model loaded successfully");
                    Some(t)
                }
                Err(e) => {
                    log::warn!("Whisper model load failed: {}. STT disabled.", e);
                    None
                }
            }
        } else {
            log::info!("No Whisper model found at {:?}. Download ggml-base.bin to enable voice.", model_path);
            None
        };

        VoiceEngine {
            capture: Mutex::new(capture),
            transcriber: Mutex::new(transcriber),
            tts: Mutex::new(TextToSpeech::new()),
            state: Mutex::new(VoiceState::Idle),
            enabled: Mutex::new(true),
            muted: Mutex::new(false),
        }
    }

    pub fn set_state(&self, state: VoiceState) {
        *self.state.lock().unwrap() = state;
    }

    pub fn get_state(&self) -> VoiceState {
        self.state.lock().unwrap().clone()
    }

    pub fn is_available(&self) -> bool {
        self.transcriber.lock().unwrap().is_some()
    }

    pub fn toggle_mute(&self) -> bool {
        let mut muted = self.muted.lock().unwrap();
        *muted = !*muted;
        if *muted {
            self.set_state(VoiceState::Disabled);
        } else {
            self.set_state(VoiceState::Idle);
        }
        *muted
    }
}
```

- [ ] **Step 2: Add `pub mod voice;` to lib.rs and manage VoiceEngine**

Read `jarvis/src-tauri/src/lib.rs` and:
1. Add `pub mod voice;` to module declarations
2. In the setup closure, after existing state management, add:

```rust
let voice_engine = std::sync::Arc::new(voice::VoiceEngine::new());
app.manage(voice_engine);
```

3. Add voice commands to the `invoke_handler` (they'll be created in the next task):

```rust
commands::voice::start_listening,
commands::voice::stop_listening,
commands::voice::get_voice_state,
commands::voice::toggle_mute,
commands::voice::get_voice_settings,
commands::voice::set_voice_setting,
commands::voice::list_tts_voices,
```

Wait -- the commands module is `voice::commands`, not `commands::voice`. Register as:

```rust
voice::commands::start_listening,
voice::commands::stop_listening,
voice::commands::get_voice_state,
voice::commands::toggle_mute,
voice::commands::get_voice_settings,
voice::commands::set_voice_setting,
voice::commands::list_tts_voices,
```

- [ ] **Step 3: Verify compilation** (will have errors until commands are created -- that's OK, just check module structure)

- [ ] **Step 4: Commit**

```bash
git add jarvis/src-tauri/src/voice/mod.rs jarvis/src-tauri/src/lib.rs
git commit -m "feat: add VoiceEngine coordinator with state machine"
```

---

## Task 6: Voice Tauri Commands

**Files:**
- Create: `jarvis/src-tauri/src/voice/commands.rs`

- [ ] **Step 1: Create voice/commands.rs**

```rust
// jarvis/src-tauri/src/voice/commands.rs
use crate::ai::AiRouter;
use crate::db::Database;
use crate::voice::{VoiceEngine, VoiceState};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn start_listening(
    engine: State<'_, Arc<VoiceEngine>>,
) -> Result<String, String> {
    if !engine.is_available() {
        return Err("Voice not available. Download Whisper model to ~/Library/Application Support/jarvis/models/ggml-base.bin".to_string());
    }

    if *engine.muted.lock().unwrap() {
        return Err("Voice is muted".to_string());
    }

    engine.set_state(VoiceState::Listening);
    engine.capture.lock().map_err(|e| e.to_string())?
        .start_recording()?;

    Ok("Listening...".to_string())
}

#[tauri::command]
pub async fn stop_listening(
    engine: State<'_, Arc<VoiceEngine>>,
    router: State<'_, AiRouter>,
    db: State<'_, Arc<Database>>,
) -> Result<String, String> {
    // Stop recording and get audio
    let samples = engine.capture.lock().map_err(|e| e.to_string())?
        .stop_recording();

    if samples.is_empty() {
        engine.set_state(VoiceState::Idle);
        return Ok(String::new());
    }

    // Transcribe
    engine.set_state(VoiceState::Processing);
    let text = {
        let transcriber = engine.transcriber.lock().map_err(|e| e.to_string())?;
        match transcriber.as_ref() {
            Some(t) => t.transcribe(&samples)?,
            None => return Err("Whisper not loaded".to_string()),
        }
    };

    if text.is_empty() {
        engine.set_state(VoiceState::Idle);
        return Ok(String::new());
    }

    // Save user message to conversations
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO conversations (role, content) VALUES ('user', ?1)",
            rusqlite::params![text],
        ).map_err(|e| e.to_string())?;
    }

    // Get AI response
    let messages = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT role, content FROM conversations ORDER BY id DESC LIMIT 20")
            .map_err(|e| e.to_string())?;
        let mut msgs: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        msgs.reverse();
        msgs
    };

    let response = router.send(messages).await?;

    // Save assistant response
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO conversations (role, content) VALUES ('assistant', ?1)",
            rusqlite::params![response],
        ).map_err(|e| e.to_string())?;
    }

    // Speak the response
    engine.set_state(VoiceState::Speaking);
    let tts = engine.tts.lock().map_err(|e| e.to_string())?;
    if let Err(e) = tts.speak(&response).await {
        log::warn!("TTS failed: {}", e);
    }

    engine.set_state(VoiceState::Idle);
    Ok(response)
}

#[tauri::command]
pub fn get_voice_state(engine: State<Arc<VoiceEngine>>) -> VoiceState {
    engine.get_state()
}

#[tauri::command]
pub fn toggle_mute(engine: State<Arc<VoiceEngine>>) -> bool {
    engine.toggle_mute()
}

#[tauri::command]
pub fn get_voice_settings(db: State<Arc<Database>>) -> Result<VoiceSettings, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let get = |key: &str, default: &str| -> String {
        conn.query_row(
            "SELECT value FROM user_preferences WHERE key = ?1",
            rusqlite::params![key], |row| row.get(0),
        ).unwrap_or_else(|_| default.to_string())
    };

    Ok(VoiceSettings {
        enabled: get("voice_enabled", "true") == "true",
        tts_voice: get("tts_voice", "Samantha"),
        tts_rate: get("tts_rate", "200").parse().unwrap_or(200),
        tts_enabled: get("tts_enabled", "true") == "true",
    })
}

#[tauri::command]
pub fn set_voice_setting(db: State<Arc<Database>>, key: String, value: String) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
        rusqlite::params![key, value],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_tts_voices() -> Result<Vec<String>, String> {
    crate::voice::tts::TextToSpeech::list_voices().await
}

#[derive(serde::Serialize)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub tts_voice: String,
    pub tts_rate: u32,
    pub tts_enabled: bool,
}
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check --manifest-path jarvis/src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/src-tauri/src/voice/commands.rs
git commit -m "feat: add voice Tauri commands for listening, transcription, TTS"
```

---

## Task 7: Frontend Voice Indicator & Hook

**Files:**
- Create: `jarvis/src/hooks/useVoiceState.ts`
- Create: `jarvis/src/components/VoiceIndicator.tsx`
- Modify: `jarvis/src/App.tsx`
- Modify: `jarvis/src/lib/types.ts`
- Modify: `jarvis/src/lib/commands.ts`

- [ ] **Step 1: Add voice types**

Append to `jarvis/src/lib/types.ts`:
```ts
export type VoiceState = "Idle" | "Listening" | "Processing" | "Speaking" | "Disabled" | { Error: string };

export interface VoiceSettings {
  enabled: boolean;
  tts_voice: string;
  tts_rate: number;
  tts_enabled: boolean;
}
```

- [ ] **Step 2: Add voice commands**

Append to `jarvis/src/lib/commands.ts`:
```ts
// Voice
export async function startListening(): Promise<string> { return invoke("start_listening"); }
export async function stopListening(): Promise<string> { return invoke("stop_listening"); }
export async function getVoiceState(): Promise<VoiceState> { return invoke("get_voice_state"); }
export async function toggleMute(): Promise<boolean> { return invoke("toggle_mute"); }
export async function getVoiceSettings(): Promise<VoiceSettings> { return invoke("get_voice_settings"); }
export async function setVoiceSetting(key: string, value: string): Promise<void> { return invoke("set_voice_setting", { key, value }); }
export async function listTtsVoices(): Promise<string[]> { return invoke("list_tts_voices"); }
```

Add `VoiceState` and `VoiceSettings` to the type imports.

- [ ] **Step 3: Create useVoiceState.ts**

```ts
// jarvis/src/hooks/useVoiceState.ts
import { useState, useCallback } from "react";
import type { VoiceState } from "../lib/types";
import { startListening, stopListening, getVoiceState } from "../lib/commands";

export function useVoiceState() {
  const [state, setState] = useState<VoiceState>("Idle");
  const [lastResponse, setLastResponse] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const poll = useCallback(async () => {
    try {
      const s = await getVoiceState();
      setState(s);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const startVoice = useCallback(async () => {
    setError(null);
    try {
      await startListening();
      setState("Listening");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const stopVoice = useCallback(async () => {
    try {
      setState("Processing");
      const response = await stopListening();
      setLastResponse(response);
      setState("Idle");
    } catch (e) {
      setError(String(e));
      setState("Idle");
    }
  }, []);

  return { state, lastResponse, error, startVoice, stopVoice, poll };
}
```

- [ ] **Step 4: Create VoiceIndicator.tsx**

```tsx
// jarvis/src/components/VoiceIndicator.tsx
import type { VoiceState } from "../lib/types";

interface VoiceIndicatorProps {
  state: VoiceState;
  onStop: () => void;
}

export default function VoiceIndicator({ state, onStop }: VoiceIndicatorProps) {
  if (state === "Idle" || state === "Disabled") return null;

  const label = state === "Listening" ? "LISTENING..."
    : state === "Processing" ? "PROCESSING..."
    : state === "Speaking" ? "SPEAKING..."
    : typeof state === "object" && "Error" in state ? `ERROR: ${state.Error}`
    : "";

  const color = state === "Listening" ? "rgba(0, 180, 255, 0.9)"
    : state === "Processing" ? "rgba(255, 180, 0, 0.8)"
    : state === "Speaking" ? "rgba(16, 185, 129, 0.8)"
    : "rgba(255, 100, 100, 0.8)";

  return (
    <div style={styles.overlay} onClick={state === "Listening" ? onStop : undefined}>
      <div style={{ ...styles.indicator, borderColor: color }}>
        <div style={{ ...styles.dot, background: color }} className="animate-glow" />
        <span style={{ ...styles.label, color }}>{label}</span>
        {state === "Listening" && (
          <span style={styles.hint}>Click or release to stop</span>
        )}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  overlay: {
    position: "fixed", bottom: 24, left: "50%", transform: "translateX(-50%)",
    zIndex: 200, cursor: "pointer",
  },
  indicator: {
    display: "flex", alignItems: "center", gap: 10,
    padding: "10px 20px", borderRadius: 24,
    border: "1px solid", background: "rgba(10, 14, 26, 0.95)",
  },
  dot: { width: 10, height: 10, borderRadius: "50%" },
  label: { fontFamily: "var(--font-mono)", fontSize: 11, letterSpacing: 1.5 },
  hint: { color: "rgba(0, 180, 255, 0.3)", fontSize: 9, fontFamily: "var(--font-mono)" },
};
```

- [ ] **Step 5: Update App.tsx**

Read `jarvis/src/App.tsx` and:
1. Import `VoiceIndicator` and `useVoiceState`
2. Add `useVoiceState` hook in the App component
3. Add a keyboard shortcut for voice: `Cmd+Shift+J` triggers start/stop
4. Render `<VoiceIndicator>` at the bottom of the root div

Add to the `useKeyboard` shortcuts:
```ts
"cmd+shift+j": () => {
  if (voiceState === "Listening") {
    stopVoice();
  } else if (voiceState === "Idle") {
    startVoice();
  }
},
```

Add before the closing `</div>`:
```tsx
<VoiceIndicator state={voiceState} onStop={stopVoice} />
```

- [ ] **Step 6: Commit**

```bash
git add jarvis/src/lib/types.ts jarvis/src/lib/commands.ts jarvis/src/hooks/useVoiceState.ts jarvis/src/components/VoiceIndicator.tsx jarvis/src/App.tsx
git commit -m "feat: add VoiceIndicator overlay and voice keyboard shortcut (Cmd+Shift+J)"
```

---

## Task 8: Voice Settings in Settings Page

**Files:**
- Modify: `jarvis/src/pages/Settings.tsx`

- [ ] **Step 1: Add voice settings panel**

Read `jarvis/src/pages/Settings.tsx` and add a VOICE panel. Import `getVoiceSettings`, `setVoiceSetting`, `listTtsVoices` from commands.

Add state:
```tsx
const [voiceSettings, setVoiceSettingsState] = useState<VoiceSettings | null>(null);
const [ttsVoices, setTtsVoices] = useState<string[]>([]);
```

Add useEffect:
```tsx
useEffect(() => {
  getVoiceSettings().then(setVoiceSettingsState);
  listTtsVoices().then(setTtsVoices);
}, []);
```

Add a panel after the existing panels:
```tsx
<div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
  <div className="label" style={{ marginBottom: 12 }}>VOICE</div>
  <div style={styles.hint}>Cmd+Shift+J to start/stop voice input</div>

  <label style={styles.option}>
    <input type="checkbox" checked={voiceSettings?.tts_enabled ?? true}
      onChange={(e) => { setVoiceSetting("tts_enabled", String(e.target.checked)); setVoiceSettingsState(prev => prev ? {...prev, tts_enabled: e.target.checked} : prev); }}
      style={styles.radio} />
    <span style={styles.optionLabel}>Enable text-to-speech</span>
  </label>

  {ttsVoices.length > 0 && (
    <div style={{ marginTop: 8 }}>
      <div style={{ color: "rgba(0, 180, 255, 0.5)", fontSize: 10, marginBottom: 4 }}>TTS Voice</div>
      <select value={voiceSettings?.tts_voice ?? "Samantha"}
        onChange={(e) => { setVoiceSetting("tts_voice", e.target.value); setVoiceSettingsState(prev => prev ? {...prev, tts_voice: e.target.value} : prev); }}
        style={{ background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "4px 8px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)" }}>
        {ttsVoices.map(v => <option key={v} value={v}>{v}</option>)}
      </select>
    </div>
  )}

  <div style={{ marginTop: 12, color: "rgba(0, 180, 255, 0.4)", fontSize: 10 }}>
    Whisper model: {voiceSettings ? "loaded" : "not found"}
  </div>
  <code style={styles.code}>~/Library/Application Support/jarvis/models/ggml-base.bin</code>
</div>
```

Import `VoiceSettings` type and the voice command functions.

- [ ] **Step 2: Commit**

```bash
git add jarvis/src/pages/Settings.tsx
git commit -m "feat: add voice settings panel with TTS voice selection"
```

---

## Task 9: Model Download Helper & .env.example

**Files:**
- Modify: `jarvis/.env.example`
- Create: `jarvis/scripts/download-whisper-model.sh`

- [ ] **Step 1: Update .env.example**

Append to `jarvis/.env.example`:
```
# Optional: Picovoice access key for wake word detection (free tier at picovoice.ai)
PICOVOICE_ACCESS_KEY=
```

- [ ] **Step 2: Create download script**

Create `jarvis/scripts/download-whisper-model.sh`:
```bash
#!/bin/bash
# Download Whisper base model for JARVIS voice
MODEL_DIR="$HOME/Library/Application Support/jarvis/models"
MODEL_PATH="$MODEL_DIR/ggml-base.bin"

if [ -f "$MODEL_PATH" ]; then
    echo "Model already exists at $MODEL_PATH"
    exit 0
fi

mkdir -p "$MODEL_DIR"
echo "Downloading Whisper base model (~141MB)..."
curl -L "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin" -o "$MODEL_PATH"

if [ -f "$MODEL_PATH" ]; then
    echo "Model downloaded successfully to $MODEL_PATH"
else
    echo "Download failed"
    exit 1
fi
```

```bash
chmod +x jarvis/scripts/download-whisper-model.sh
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/.env.example jarvis/scripts/download-whisper-model.sh
git commit -m "feat: add Whisper model download script and Picovoice env var"
```

---

## Summary

After completing all 9 tasks, Phase 2a delivers:

- **Audio capture** via cpal at 16kHz mono (Whisper's required format)
- **Speech-to-text** via whisper-rs with local ggml-base model (~141MB)
- **Text-to-speech** via macOS `say` command with configurable voice and rate
- **Voice state machine**: Idle -> Listening -> Processing -> Speaking -> Idle
- **Push-to-talk**: Cmd+Shift+J starts/stops voice input
- **Voice indicator** overlay showing current state (listening/processing/speaking)
- **7 new Tauri commands** for voice control
- **Voice settings** in Settings page: TTS toggle, voice selection
- **Model download script** for easy setup
- **Microphone permission** configured in Info.plist

**To use voice after implementation:**
1. Run `scripts/download-whisper-model.sh` to get the Whisper model
2. Start the app, press Cmd+Shift+J to start listening
3. Speak your command, press Cmd+Shift+J again to stop
4. JARVIS transcribes, gets AI response, speaks it back

**Next:** Phase 2b -- Rule-based email learning, Phase 2c -- Custom cron jobs via natural language.
