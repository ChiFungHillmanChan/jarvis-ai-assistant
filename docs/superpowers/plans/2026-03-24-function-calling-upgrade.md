# JARVIS Function Calling Upgrade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade JARVIS to use GPT-5 as primary AI with Claude Sonnet 4.6 fallback, expand from 8 to 32 function tools covering integrations and system control.

**Architecture:** Make `execute_tool()` stateful by passing `Database` and `Arc<GoogleAuth>` through the entire AI call chain (`chat.rs` -> `AiRouter::send()` -> `claude/openai::send()` -> `tools::execute_tool()`). New tools call existing integration clients directly instead of shelling out.

**Tech Stack:** Rust (Tauri v2), reqwest, serde_json, tokio, base64, urlencoding, macOS system commands (pbcopy/pbpaste/screencapture/osascript)

**Spec:** `docs/superpowers/specs/2026-03-24-jarvis-function-calling-upgrade.md`

---

## Task 1: Make Tool Execution Stateful (Core Plumbing)

**Files:**
- Modify: `src-tauri/src/ai/tools.rs` (lines 114-171 -- `execute_tool` signature)
- Modify: `src-tauri/src/ai/claude.rs` (lines 53-56, 68-69, 124 -- `send` signature + model + execute_tool calls)
- Modify: `src-tauri/src/ai/openai.rs` (lines 50-53, 68-69, 97 -- `send` signature + model + execute_tool calls)
- Modify: `src-tauri/src/ai/mod.rs` (lines 20-25, 34 -- default provider + `send` signature)
- Modify: `src-tauri/src/commands/chat.rs` (lines 71, 192 -- pass state to router)
- Modify: `src-tauri/src/commands/assistant.rs` (lines 8-13, 16-20, 33-37 -- add google_auth state)
- Modify: `src-tauri/src/assistant/briefing.rs` (lines 14-16, 22 -- add google_auth param)
- Modify: `src-tauri/src/lib.rs` (line 92-97 -- auto-briefing router)

- [ ] **Step 1: Update `execute_tool` signature in `tools.rs`**

Change the function signature to accept DB and auth state. Update `create_task` to use direct DB instead of sqlite3 shell. Add result truncation helper.

```rust
// In src-tauri/src/ai/tools.rs, replace the execute_tool function signature and create_task branch:

/// Truncate tool result to prevent context window overflow
pub fn truncate_result(result: String) -> String {
    if result.len() > 4000 {
        format!("{}... [truncated, showing first 4000 chars]", &result[..4000])
    } else {
        result
    }
}

/// Execute a tool by name with JSON arguments. Shared by both APIs.
pub async fn execute_tool(
    name: &str,
    args_str: &str,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::GoogleAuth>,
) -> String {
    let args: serde_json::Value = serde_json::from_str(args_str).unwrap_or_default();

    let result = match name {
        "open_app" => {
            let app = args["name"].as_str().unwrap_or("");
            crate::system::control::open_app(app).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "open_url" => {
            let url = args["url"].as_str().unwrap_or("");
            crate::system::control::open_url(url).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "run_command" => {
            let cmd = args["command"].as_str().unwrap_or("");
            crate::system::control::run_command(cmd).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "find_files" => {
            let query = args["query"].as_str().unwrap_or("");
            match crate::system::control::find_files(query, None).await {
                Ok(files) => if files.is_empty() { "No files found.".into() } else { files.join("\n") },
                Err(e) => format!("Error: {}", e),
            }
        }
        "open_file" => {
            let path = args["path"].as_str().unwrap_or("");
            crate::system::control::open_file(path).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "create_task" => {
            let title = args["title"].as_str().unwrap_or("Untitled");
            let desc = args["description"].as_str().unwrap_or("");
            let deadline = args["deadline"].as_str();
            let priority = args["priority"].as_i64().unwrap_or(1) as i32;
            let conn = match db.conn.lock() {
                Ok(c) => c,
                Err(e) => return format!("DB error: {}", e),
            };
            match conn.execute(
                "INSERT INTO tasks (title, description, deadline, priority) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![title, desc, deadline, priority],
            ) {
                Ok(_) => format!("Task created: {}", title),
                Err(e) => format!("Failed to create task: {}", e),
            }
        }
        "write_note" => {
            let path = args["path"].as_str().unwrap_or("~/jarvis-notes.md");
            let content = args["content"].as_str().unwrap_or("");
            let append = args["append"].as_bool().unwrap_or(true);
            crate::system::control::write_note(path, content, append).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "system_info" => {
            crate::system::control::system_info().await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        _ => format!("Unknown tool: {}", name),
    };

    truncate_result(result)
}
```

- [ ] **Step 2: Update `claude.rs` -- model, signature, and execute_tool calls**

```rust
// In src-tauri/src/ai/claude.rs, change the send function signature:

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::GoogleAuth>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {

// Change model on line 68:
    model: "claude-sonnet-4-6-20250610".into(),

// Change max_tokens on line 69:
    max_tokens: 4096,

// Change execute_tool call on line 124:
    let result = tools::execute_tool(name, &args_str, db, google_auth).await;
```

- [ ] **Step 3: Update `openai.rs` -- model, signature, and execute_tool calls**

```rust
// In src-tauri/src/ai/openai.rs, change the send function signature:

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::GoogleAuth>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {

// Change model on line 69:
    model: "gpt-5".into(),

// Change max_tokens on line 72:
    max_tokens: 4096,

// Change execute_tool call on line 97:
    let result = tools::execute_tool(&tc.function.name, &tc.function.arguments, db, google_auth).await;
```

- [ ] **Step 4: Update `AiRouter::send()` in `mod.rs`**

```rust
// In src-tauri/src/ai/mod.rs

// Change default provider on line 25:
    _ => AiProvider::OpenAIPrimary,

// Change send signature and all internal calls:
pub async fn send(
    &self,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::GoogleAuth>,
) -> Result<String, String> {
    match self.provider {
        AiProvider::ClaudePrimary => {
            if let Some(ref key) = self.claude_key {
                match claude::send(key, messages.clone(), db, google_auth).await {
                    Ok(response) => return Ok(response),
                    Err(e) => log::warn!("Claude failed, trying OpenAI fallback: {}", e),
                }
            }
            if let Some(ref key) = self.openai_key {
                openai::send(key, messages, db, google_auth).await.map_err(|e| format!("Both AI providers failed. OpenAI: {}", e))
            } else {
                Err("Claude failed and no OpenAI key configured".to_string())
            }
        }
        AiProvider::OpenAIPrimary => {
            if let Some(ref key) = self.openai_key {
                match openai::send(key, messages.clone(), db, google_auth).await {
                    Ok(response) => return Ok(response),
                    Err(e) => log::warn!("OpenAI failed, trying Claude fallback: {}", e),
                }
            }
            if let Some(ref key) = self.claude_key {
                claude::send(key, messages, db, google_auth).await.map_err(|e| format!("Both AI providers failed. Claude: {}", e))
            } else {
                Err("OpenAI failed and no Claude key configured".to_string())
            }
        }
        AiProvider::ClaudeOnly => {
            let key = self.claude_key.as_ref().ok_or("No Claude API key configured")?;
            claude::send(key, messages, db, google_auth).await.map_err(|e| format!("Claude error: {}", e))
        }
        AiProvider::OpenAIOnly => {
            let key = self.openai_key.as_ref().ok_or("No OpenAI API key configured")?;
            openai::send(key, messages, db, google_auth).await.map_err(|e| format!("OpenAI error: {}", e))
        }
    }
}
```

- [ ] **Step 5: Update all callers of `router.send()`**

**`commands/chat.rs`** -- two call sites:

```rust
// Line 71 (search branch): change
let search_response = router.send(search_messages).await?;
// to
let search_response = router.send(search_messages, &db, &google_auth).await?;

// Line 192 (main send): change
let response_text = router.send(messages).await?;
// to
let response_text = router.send(messages, &db, &google_auth).await?;

// Add google_auth to send_message params:
pub async fn send_message(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    google_auth: State<'_, Arc<crate::auth::GoogleAuth>>,
    message: String,
) -> Result<ChatMessage, String> {
```

**`commands/assistant.rs`** -- add google_auth state to all commands:

```rust
use crate::auth::google::GoogleAuth;

#[tauri::command]
pub async fn get_briefing(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    google_auth: State<'_, Arc<GoogleAuth>>,
) -> Result<briefing::BriefingResult, String> {
    briefing::generate_briefing(&db, &router, &google_auth).await
}

#[tauri::command]
pub async fn speak_briefing(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    google_auth: State<'_, Arc<GoogleAuth>>,
) -> Result<briefing::BriefingResult, String> {
    let result = briefing::generate_briefing(&db, &router, &google_auth).await?;
    // ... rest unchanged
}

#[tauri::command]
pub async fn ask_jarvis(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    google_auth: State<'_, Arc<GoogleAuth>>,
    question: String,
) -> Result<String, String> {
    // ... context gathering unchanged ...
    let messages = vec![("user".to_string(), prompt)];
    router.send(messages, &db, &google_auth).await
}

// search_conversations also needs google_auth:
pub async fn search_conversations(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    google_auth: State<'_, Arc<GoogleAuth>>,
    query: String,
) -> Result<String, String> {
    // ... at the end:
    router.send(messages, &db, &google_auth).await
}
```

**`assistant/briefing.rs`** -- add google_auth param:

```rust
pub async fn generate_briefing(
    db: &Arc<Database>,
    router: &AiRouter,
    google_auth: &Arc<crate::auth::GoogleAuth>,
) -> Result<BriefingResult, String> {
    let context = DayContext::gather(db)?;
    let prompt = context.to_prompt();
    let messages = vec![("user".to_string(), prompt)];
    let briefing_text = router.send(messages, db, google_auth).await?;
    // ... rest unchanged
```

**`lib.rs`** -- auto-briefing spawn (lines 64-124): add `auth_brief` clone alongside existing `db_brief` clone (before the spawn), then pass it through:

```rust
// Line 64: existing clone stays
let db_brief = std::sync::Arc::clone(&db_arc);
// Line 65: ADD this new clone (auth_arc is defined on line 46, still in scope)
let auth_brief = std::sync::Arc::clone(&auth_arc);
// Line 66: the spawn block already moves db_brief; auth_brief is now also moved
tauri::async_runtime::spawn(async move {
    // ... existing auto-briefing checks unchanged (lines 67-90) ...
    let router = crate::ai::AiRouter::new(
        std::env::var("ANTHROPIC_API_KEY").ok(),
        std::env::var("OPENAI_API_KEY").ok(),
        "claude_primary",
    );
    // Pass auth_brief as the new third argument:
    match crate::assistant::briefing::generate_briefing(&db_brief, &router, &auth_brief).await {
        // ... rest unchanged (lines 98-124)
```

- [ ] **Step 6: Verify compilation**

Run: `cd jarvis && cargo check 2>&1 | head -30`
Expected: No errors. All callers pass the new params correctly.

- [ ] **Step 7: Commit**

```bash
cd jarvis && git add src-tauri/src/ai/ src-tauri/src/commands/chat.rs src-tauri/src/commands/assistant.rs src-tauri/src/assistant/briefing.rs src-tauri/src/lib.rs
git commit -m "feat: stateful tool execution with GPT-5 primary, Claude Sonnet 4.6 fallback"
```

---

## Task 2: New System Tools in `control.rs`

**Files:**
- Modify: `src-tauri/src/system/control.rs` (append new functions)
- Modify: `src-tauri/src/ai/tools.rs` (add tool definitions + execute_tool branches)

- [ ] **Step 1: Add 9 new system functions to `control.rs`**

Append to the end of `src-tauri/src/system/control.rs`:

```rust
/// Read clipboard contents
pub async fn clipboard_read() -> Result<String, String> {
    let output = Command::new("pbpaste")
        .output()
        .await
        .map_err(|e| format!("Clipboard read failed: {}", e))?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Write to clipboard
pub async fn clipboard_write(content: &str) -> Result<String, String> {
    let mut child = Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Clipboard write failed: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(content.as_bytes()).await.map_err(|e| format!("Write failed: {}", e))?;
    }
    child.wait().await.map_err(|e| format!("Clipboard write failed: {}", e))?;
    Ok("Copied to clipboard.".to_string())
}

/// Take a screenshot
pub async fn screenshot(region: &str) -> Result<String, String> {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let path = format!("/tmp/jarvis_screenshot_{}.png", timestamp);

    let mut cmd = Command::new("screencapture");
    match region {
        "window" => { cmd.arg("-w"); }
        "selection" => { cmd.arg("-s"); }
        _ => {} // full screen is default
    }
    cmd.arg(&path);

    let status = cmd.status().await.map_err(|e| format!("Screenshot failed: {}", e))?;
    if status.success() {
        Ok(format!("Screenshot saved to {}", path))
    } else {
        Err("Screenshot cancelled or failed.".to_string())
    }
}

/// Manage windows via AppleScript
pub async fn manage_window(action: &str, app_name: Option<&str>, width: Option<i64>, height: Option<i64>, x: Option<i64>, y: Option<i64>) -> Result<String, String> {
    let script = match action {
        "list" => {
            r#"tell application "System Events" to get name of every process whose background only is false"#.to_string()
        }
        "focus" => {
            let app = app_name.ok_or("app_name required for focus")?;
            format!(r#"tell application "{}" to activate"#, app)
        }
        "resize" => {
            let app = app_name.ok_or("app_name required for resize")?;
            let w = width.unwrap_or(800);
            let h = height.unwrap_or(600);
            format!(
                r#"tell application "System Events" to tell process "{}" to set size of front window to {{{}, {}}}"#,
                app, w, h
            )
        }
        "move" => {
            let app = app_name.ok_or("app_name required for move")?;
            let px = x.unwrap_or(0);
            let py = y.unwrap_or(0);
            format!(
                r#"tell application "System Events" to tell process "{}" to set position of front window to {{{}, {}}}"#,
                app, px, py
            )
        }
        _ => return Err(format!("Unknown window action: {}", action)),
    };

    let output = Command::new("osascript").arg("-e").arg(&script)
        .output().await.map_err(|e| format!("AppleScript error: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// System controls: volume, brightness, dark mode
pub async fn system_controls(action: &str, value: Option<i64>) -> Result<String, String> {
    let script = match action {
        "get_volume" => r#"output volume of (get volume settings)"#.to_string(),
        "set_volume" => {
            let v = value.ok_or("value required for set_volume")?;
            format!("set volume output volume {}", v.clamp(0, 100))
        }
        "get_brightness" => {
            // Use brightness command if available, fallback to AppleScript
            let output = Command::new("brightness").arg("-l").output().await;
            match output {
                Ok(o) if o.status.success() => return Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
                _ => return Ok("Brightness control requires 'brightness' CLI (brew install brightness)".to_string()),
            }
        }
        "set_brightness" => {
            let v = value.ok_or("value required for set_brightness")?;
            let brightness = v as f64 / 100.0;
            let output = Command::new("brightness").arg(format!("{:.2}", brightness)).output().await;
            match output {
                Ok(o) if o.status.success() => return Ok(format!("Brightness set to {}%", v)),
                _ => return Ok("Brightness control requires 'brightness' CLI (brew install brightness)".to_string()),
            }
        }
        "toggle_dark_mode" => {
            r#"tell application "System Events" to tell appearance preferences to set dark mode to not dark mode"#.to_string()
        }
        _ => return Err(format!("Unknown system control action: {}", action)),
    };

    let output = Command::new("osascript").arg("-e").arg(&script)
        .output().await.map_err(|e| format!("System control error: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Send a macOS notification
pub async fn send_notification(title: &str, message: &str, sound: bool) -> Result<String, String> {
    let sound_part = if sound { " sound name \"default\"" } else { "" };
    let script = format!(
        r#"display notification "{}" with title "{}"{}"#,
        message.replace('"', r#"\""#),
        title.replace('"', r#"\""#),
        sound_part
    );
    let output = Command::new("osascript").arg("-e").arg(&script)
        .output().await.map_err(|e| format!("Notification error: {}", e))?;
    if output.status.success() {
        Ok("Notification sent.".to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// List running processes
pub async fn list_processes(filter: Option<&str>) -> Result<String, String> {
    let output = Command::new("ps").arg("aux").output().await
        .map_err(|e| format!("Process list failed: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if let Some(f) = filter {
        let f_lower = f.to_lowercase();
        let filtered: Vec<&str> = lines.iter()
            .filter(|l| l.to_lowercase().contains(&f_lower))
            .copied()
            .take(30)
            .collect();
        Ok(filtered.join("\n"))
    } else {
        // Return header + top 20 by CPU
        Ok(lines.iter().take(21).copied().collect::<Vec<_>>().join("\n"))
    }
}

/// Kill a process by PID
pub async fn kill_process(pid: i64) -> Result<String, String> {
    // Safety: refuse system-critical processes
    if pid < 100 {
        return Err("Refusing to kill system process (PID < 100).".to_string());
    }
    let protected = ["kernel_task", "WindowServer", "loginwindow", "launchd", "syslogd"];
    // Check process name
    let check = Command::new("ps").arg("-p").arg(pid.to_string()).arg("-o").arg("comm=")
        .output().await.map_err(|e| format!("Cannot check process: {}", e))?;
    let name = String::from_utf8_lossy(&check.stdout).trim().to_string();
    if protected.iter().any(|p| name.contains(p)) {
        return Err(format!("Refusing to kill protected system process: {}", name));
    }

    let status = Command::new("kill").arg(pid.to_string())
        .status().await.map_err(|e| format!("Kill failed: {}", e))?;
    if status.success() {
        Ok(format!("Process {} killed.", pid))
    } else {
        Err(format!("Failed to kill process {}. May require elevated privileges.", pid))
    }
}

/// Read file contents as text
pub async fn read_file(path: &str, max_lines: Option<i64>) -> Result<String, String> {
    let expanded = shellexpand::tilde(path).to_string();
    let metadata = tokio::fs::metadata(&expanded).await.map_err(|e| format!("File not found: {}", e))?;

    if metadata.len() > 100_000 {
        return Err("File too large (>100KB). Use max_lines to read a portion.".to_string());
    }

    let content = tokio::fs::read(&expanded).await.map_err(|e| format!("Read error: {}", e))?;

    // Check for binary content (more than 10% non-text bytes in first 1024 bytes)
    let check_len = content.len().min(1024);
    let non_text = content[..check_len].iter().filter(|&&b| b < 9 || (b > 13 && b < 32)).count();
    if non_text > check_len / 10 {
        return Err("File appears to be binary. Cannot display.".to_string());
    }

    let text = String::from_utf8_lossy(&content).to_string();
    let limit = max_lines.unwrap_or(100) as usize;
    let lines: Vec<&str> = text.lines().take(limit).collect();
    Ok(lines.join("\n"))
}
```

- [ ] **Step 2: Add system tool definitions and execute branches to `tools.rs`**

Add to `get_tool_definitions()` vec (after `system_info`):

```rust
        Tool {
            name: "clipboard_read".into(),
            description: "Read the current clipboard text content".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "clipboard_write".into(),
            description: "Write text to the clipboard".into(),
            parameters: json!({
                "type": "object",
                "properties": { "content": { "type": "string", "description": "Text to copy to clipboard" } },
                "required": ["content"]
            }),
        },
        Tool {
            name: "screenshot".into(),
            description: "Take a screenshot and save it. Returns the file path.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "region": { "type": "string", "description": "'full' (default), 'window', or 'selection'" } }
            }),
        },
        Tool {
            name: "manage_window".into(),
            description: "Manage application windows: focus, resize, move, or list open apps".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "'focus', 'resize', 'move', or 'list'" },
                    "app_name": { "type": "string", "description": "Application name (required for focus/resize/move)" },
                    "width": { "type": "integer" },
                    "height": { "type": "integer" },
                    "x": { "type": "integer" },
                    "y": { "type": "integer" }
                },
                "required": ["action"]
            }),
        },
        Tool {
            name: "system_controls".into(),
            description: "Control system settings: volume, brightness, dark mode".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "'get_volume', 'set_volume', 'get_brightness', 'set_brightness', 'toggle_dark_mode'" },
                    "value": { "type": "integer", "description": "0-100 for set_volume/set_brightness" }
                },
                "required": ["action"]
            }),
        },
        Tool {
            name: "send_notification".into(),
            description: "Send a macOS notification to the user".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "message": { "type": "string" },
                    "sound": { "type": "boolean", "description": "Play sound (default true)" }
                },
                "required": ["title", "message"]
            }),
        },
        Tool {
            name: "list_processes".into(),
            description: "List running processes on the system".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filter": { "type": "string", "description": "Filter by process name" }
                }
            }),
        },
        Tool {
            name: "kill_process".into(),
            description: "Kill a running process by PID. Refuses system-critical processes.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "description": "Process ID to kill" }
                },
                "required": ["pid"]
            }),
        },
        Tool {
            name: "read_file".into(),
            description: "Read the text contents of a file. Max 100KB, refuses binary files.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path (use ~ for home)" },
                    "max_lines": { "type": "integer", "description": "Max lines to read (default 100)" }
                },
                "required": ["path"]
            }),
        },
```

Add to `execute_tool` match arms (before the `_ =>` catch-all):

```rust
        "clipboard_read" => {
            crate::system::control::clipboard_read().await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "clipboard_write" => {
            let content = args["content"].as_str().unwrap_or("");
            crate::system::control::clipboard_write(content).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "screenshot" => {
            let region = args["region"].as_str().unwrap_or("full");
            crate::system::control::screenshot(region).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "manage_window" => {
            let action = args["action"].as_str().unwrap_or("list");
            let app_name = args["app_name"].as_str();
            let width = args["width"].as_i64();
            let height = args["height"].as_i64();
            let x = args["x"].as_i64();
            let y = args["y"].as_i64();
            crate::system::control::manage_window(action, app_name, width, height, x, y).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "system_controls" => {
            let action = args["action"].as_str().unwrap_or("");
            let value = args["value"].as_i64();
            crate::system::control::system_controls(action, value).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "send_notification" => {
            let title = args["title"].as_str().unwrap_or("JARVIS");
            let message = args["message"].as_str().unwrap_or("");
            let sound = args["sound"].as_bool().unwrap_or(true);
            crate::system::control::send_notification(title, message, sound).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "list_processes" => {
            let filter = args["filter"].as_str();
            crate::system::control::list_processes(filter).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "kill_process" => {
            let pid = args["pid"].as_i64().unwrap_or(0);
            crate::system::control::kill_process(pid).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "read_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let max_lines = args["max_lines"].as_i64();
            crate::system::control::read_file(path, max_lines).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
```

- [ ] **Step 3: Verify compilation**

Run: `cd jarvis && cargo check 2>&1 | head -30`
Expected: No errors.

- [ ] **Step 4: Commit**

```bash
cd jarvis && git add src-tauri/src/system/control.rs src-tauri/src/ai/tools.rs
git commit -m "feat: add 9 new system tools (clipboard, screenshot, window, volume, notification, process, file read)"
```

---

## Task 3: Gmail Integration Tools

**Files:**
- Modify: `src-tauri/src/integrations/gmail.rs` (add 3 new public functions)
- Modify: `src-tauri/src/ai/tools.rs` (add 4 Gmail tool definitions + execute branches)

- [ ] **Step 1: Add `base64` and `urlencoding` crate dependencies**

These are needed by the new Gmail functions (base64url decoding for email bodies, URL encoding for search queries).

```bash
cd jarvis/src-tauri && cargo add base64 urlencoding
```

- [ ] **Step 2: Add 3 new Gmail functions**

Append to `src-tauri/src/integrations/gmail.rs`:

```rust
/// Search Gmail messages with query syntax
pub async fn search_messages(access_token: &str, query: &str) -> Result<Vec<GmailMessage>, String> {
    let client = Client::new();
    let url = format!("{}/messages?q={}&maxResults=10", GMAIL_API, urlencoding::encode(query));
    let resp = client.get(&url).bearer_auth(access_token).send().await.map_err(|e| format!("Gmail search error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { return Err(format!("Gmail API error: {}", resp.status())); }
    let list: ListResponse = resp.json().await.map_err(|e| e.to_string())?;
    let refs = list.messages.unwrap_or_default();
    let mut messages = Vec::new();
    for msg_ref in refs.iter().take(10) {
        match fetch_message_detail(access_token, &msg_ref.id).await {
            Ok(msg) => messages.push(msg),
            Err(e) => log::warn!("Failed to fetch message {}: {}", msg_ref.id, e),
        }
    }
    Ok(messages)
}

/// Get full email content including body
pub async fn get_message_full(access_token: &str, message_id: &str) -> Result<(GmailMessage, String), String> {
    let client = Client::new();
    let url = format!("{}/messages/{}?format=full", GMAIL_API, message_id);
    let resp = client.get(&url).bearer_auth(access_token).send().await.map_err(|e| e.to_string())?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { return Err(format!("Gmail API error: {}", resp.status())); }

    let detail: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    // Extract headers
    let headers = detail["payload"]["headers"].as_array();
    let get_header = |name: &str| -> Option<String> {
        headers.and_then(|h| h.iter().find(|hdr| hdr["name"].as_str() == Some(name)).and_then(|hdr| hdr["value"].as_str().map(String::from)))
    };
    let subject = get_header("Subject");
    let sender = get_header("From");
    let to = get_header("To").unwrap_or_default();
    let date = get_header("Date");

    let labels = detail["labelIds"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    let snippet = detail["snippet"].as_str().map(String::from);

    // Extract body -- try plain text first, then html
    let body = extract_body(&detail["payload"]).unwrap_or_else(|| snippet.clone().unwrap_or_default());

    let msg = GmailMessage {
        id: message_id.to_string(),
        thread_id: detail["threadId"].as_str().map(String::from),
        subject, sender, snippet,
        label_ids: labels.clone(),
        is_read: !labels.contains(&"UNREAD".to_string()),
        received_at: date.or(detail["internalDate"].as_str().map(String::from)),
    };

    Ok((msg, format!("From: {}\nTo: {}\nSubject: {}\nDate: {}\n\n{}", msg.sender.as_deref().unwrap_or(""), to, msg.subject.as_deref().unwrap_or(""), msg.received_at.as_deref().unwrap_or(""), body)))
}

fn extract_body(payload: &serde_json::Value) -> Option<String> {
    // Direct body data
    if let Some(data) = payload["body"]["data"].as_str() {
        if let Ok(decoded) = base64_url_decode(data) {
            return Some(decoded);
        }
    }
    // Check parts for text/plain first, then text/html
    if let Some(parts) = payload["parts"].as_array() {
        // Try text/plain
        for part in parts {
            if part["mimeType"].as_str() == Some("text/plain") {
                if let Some(data) = part["body"]["data"].as_str() {
                    if let Ok(decoded) = base64_url_decode(data) {
                        return Some(decoded);
                    }
                }
            }
        }
        // Try text/html (strip tags roughly)
        for part in parts {
            if part["mimeType"].as_str() == Some("text/html") {
                if let Some(data) = part["body"]["data"].as_str() {
                    if let Ok(decoded) = base64_url_decode(data) {
                        // Basic HTML tag stripping
                        let stripped = decoded
                            .replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n")
                            .replace("</p>", "\n").replace("</div>", "\n");
                        let re_stripped: String = strip_html_tags(&stripped);
                        return Some(re_stripped);
                    }
                }
            }
        }
        // Recurse into nested parts
        for part in parts {
            if let Some(body) = extract_body(part) {
                return Some(body);
            }
        }
    }
    None
}

fn base64_url_decode(data: &str) -> Result<String, String> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let bytes = engine.decode(data).map_err(|e| format!("Base64 decode error: {}", e))?;
    String::from_utf8(bytes).map_err(|e| format!("UTF8 error: {}", e))
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Send an email via Gmail API
pub async fn send_message(access_token: &str, to: &str, subject: &str, body: &str, cc: Option<&str>) -> Result<String, String> {
    let client = Client::new();

    // Build RFC 2822 message
    let mut raw = format!("To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n", to, subject);
    if let Some(cc_addr) = cc {
        raw.push_str(&format!("Cc: {}\r\n", cc_addr));
    }
    raw.push_str(&format!("\r\n{}", body));

    // Base64url encode
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let encoded = engine.encode(raw.as_bytes());

    let payload = serde_json::json!({ "raw": encoded });

    let resp = client
        .post(&format!("{}/messages/send", GMAIL_API))
        .bearer_auth(access_token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Gmail send error: {}", e))?;

    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail send failed {}: {}", s, t));
    }

    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(result["id"].as_str().unwrap_or("sent").to_string())
}
```

- [ ] **Step 3: Add Gmail tool definitions and execute branches to `tools.rs`**

Add to `get_tool_definitions()`:

```rust
        Tool {
            name: "search_emails".into(),
            description: "Search Gmail messages. Use Gmail search syntax: 'from:name', 'subject:text', 'is:unread', 'after:2026/03/01', etc.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "query": { "type": "string", "description": "Gmail search query" } },
                "required": ["query"]
            }),
        },
        Tool {
            name: "read_email".into(),
            description: "Read the full content of an email by its ID".into(),
            parameters: json!({
                "type": "object",
                "properties": { "email_id": { "type": "string", "description": "Gmail message ID" } },
                "required": ["email_id"]
            }),
        },
        Tool {
            name: "send_email".into(),
            description: "Compose and send an email via Gmail".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "to": { "type": "string", "description": "Recipient email address" },
                    "subject": { "type": "string" },
                    "body": { "type": "string", "description": "Email body text" },
                    "cc": { "type": "string", "description": "CC email address" }
                },
                "required": ["to", "subject", "body"]
            }),
        },
        Tool {
            name: "archive_email".into(),
            description: "Archive a Gmail message (remove from inbox)".into(),
            parameters: json!({
                "type": "object",
                "properties": { "email_id": { "type": "string", "description": "Gmail message ID" } },
                "required": ["email_id"]
            }),
        },
```

Add to `execute_tool` match arms -- these need access token with refresh logic:

```rust
        "search_emails" | "read_email" | "send_email" | "archive_email" => {
            let token = match google_auth.get_access_token() {
                Some(t) => t,
                None => return "Google account not connected. Please connect in Settings.".to_string(),
            };
            // Try operation, refresh token on 401 and retry once
            let result = execute_gmail_tool(name, &args, &token, db).await;
            match result {
                Ok(r) => r,
                Err(e) if e.contains("UNAUTHORIZED") => {
                    log::info!("Gmail token expired, refreshing...");
                    if let Err(re) = google_auth.refresh_access_token().await {
                        return format!("Token refresh failed: {}", re);
                    }
                    let new_token = match google_auth.get_access_token() {
                        Some(t) => t,
                        None => return "Token refresh succeeded but no token available.".to_string(),
                    };
                    execute_gmail_tool(name, &args, &new_token, db).await.unwrap_or_else(|e| format!("Error: {}", e))
                }
                Err(e) => format!("Error: {}", e),
            }
        }
```

Add this helper function in `tools.rs`:

```rust
async fn execute_gmail_tool(name: &str, args: &serde_json::Value, token: &str, db: &crate::db::Database) -> Result<String, String> {
    match name {
        "search_emails" => {
            let query = args["query"].as_str().unwrap_or("");
            let messages = crate::integrations::gmail::search_messages(token, query).await?;
            if messages.is_empty() { return Ok("No emails found.".to_string()); }
            let formatted: Vec<String> = messages.iter().map(|m| {
                format!("ID: {} | From: {} | Subject: {} | Date: {}",
                    m.id, m.sender.as_deref().unwrap_or("?"), m.subject.as_deref().unwrap_or("(no subject)"), m.received_at.as_deref().unwrap_or("?"))
            }).collect();
            Ok(formatted.join("\n"))
        }
        "read_email" => {
            let id = args["email_id"].as_str().unwrap_or("");
            let (_, full_text) = crate::integrations::gmail::get_message_full(token, id).await?;
            Ok(full_text)
        }
        "send_email" => {
            let to = args["to"].as_str().unwrap_or("");
            let subject = args["subject"].as_str().unwrap_or("");
            let body = args["body"].as_str().unwrap_or("");
            let cc = args["cc"].as_str();
            // Safety: log outgoing email to DB before sending
            {
                let conn = db.conn.lock().map_err(|e| format!("DB error: {}", e))?;
                let now = chrono::Local::now().to_rfc3339();
                let _ = conn.execute(
                    "INSERT INTO emails (gmail_id, subject, sender, snippet, labels, is_read, received_at) VALUES (?1, ?2, 'me (outgoing)', ?3, 'SENT', 1, ?4)",
                    rusqlite::params![format!("outgoing_{}", now), subject, &body[..body.len().min(200)], now],
                );
            }
            let msg_id = crate::integrations::gmail::send_message(token, to, subject, body, cc).await?;
            Ok(format!("Email sent to {}. Message ID: {}", to, msg_id))
        }
        "archive_email" => {
            let id = args["email_id"].as_str().unwrap_or("");
            crate::integrations::gmail::archive_message(token, id).await?;
            Ok(format!("Email {} archived.", id))
        }
        _ => Err(format!("Unknown Gmail tool: {}", name)),
    }
}
```

- [ ] **Step 4: Verify compilation**

Run: `cd jarvis && cargo check 2>&1 | head -30`

- [ ] **Step 5: Commit**

```bash
cd jarvis && git add src-tauri/src/integrations/gmail.rs src-tauri/src/ai/tools.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat: add Gmail integration tools (search, read, send, archive with DB logging)"
```

---

## Task 4: Google Calendar Integration Tools

**Files:**
- Modify: `src-tauri/src/integrations/calendar.rs` (extend create_event, add update_event, delete_event)
- Modify: `src-tauri/src/ai/tools.rs` (add 4 Calendar tool definitions + execute branches)

- [ ] **Step 1: Extend `create_event` and add `update_event`, `delete_event`, fix existing callers**

In `src-tauri/src/integrations/calendar.rs`, replace existing `create_event` and append new functions. Also update `commands/calendar.rs` to pass `None, None` for the new location/attendees params in the existing Tauri command (search for `calendar::create_event` in `commands/calendar.rs` and add `, None, None` at the end of the call).

```rust
pub async fn create_event(
    access_token: &str,
    summary: &str,
    start: &str,
    end: &str,
    description: Option<&str>,
    location: Option<&str>,
    attendees: Option<&str>,
) -> Result<String, String> {
    let client = Client::new();
    let url = format!("{}/calendars/primary/events", CALENDAR_API);

    let mut body = serde_json::json!({
        "summary": summary,
        "start": { "dateTime": start },
        "end": { "dateTime": end },
    });
    if let Some(desc) = description {
        body["description"] = serde_json::Value::String(desc.to_string());
    }
    if let Some(loc) = location {
        body["location"] = serde_json::Value::String(loc.to_string());
    }
    if let Some(att) = attendees {
        let attendee_list: Vec<serde_json::Value> = att.split(',')
            .map(|e| serde_json::json!({"email": e.trim()}))
            .collect();
        body["attendees"] = serde_json::Value::Array(attendee_list);
    }

    let resp = client.post(&url).bearer_auth(access_token).json(&body).send().await
        .map_err(|e| format!("Create event error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("Create event failed {}: {}", s, t));
    }
    let created: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let id = created["id"].as_str().unwrap_or("").to_string();
    let link = created["htmlLink"].as_str().unwrap_or("").to_string();
    Ok(format!("{} | {}", id, link))
}

pub async fn update_event(
    access_token: &str,
    event_id: &str,
    title: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    location: Option<&str>,
    description: Option<&str>,
) -> Result<String, String> {
    let client = Client::new();
    let url = format!("{}/calendars/primary/events/{}", CALENDAR_API, event_id);

    let mut body = serde_json::json!({});
    if let Some(t) = title { body["summary"] = serde_json::Value::String(t.to_string()); }
    if let Some(s) = start { body["start"] = serde_json::json!({"dateTime": s}); }
    if let Some(e) = end { body["end"] = serde_json::json!({"dateTime": e}); }
    if let Some(l) = location { body["location"] = serde_json::Value::String(l.to_string()); }
    if let Some(d) = description { body["description"] = serde_json::Value::String(d.to_string()); }

    let resp = client.patch(&url).bearer_auth(access_token).json(&body).send().await
        .map_err(|e| format!("Update event error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("Update event failed {}: {}", s, t));
    }
    Ok(format!("Event {} updated.", event_id))
}

pub async fn delete_event(access_token: &str, event_id: &str) -> Result<String, String> {
    let client = Client::new();
    let url = format!("{}/calendars/primary/events/{}", CALENDAR_API, event_id);
    let resp = client.delete(&url).bearer_auth(access_token).send().await
        .map_err(|e| format!("Delete event error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() && resp.status().as_u16() != 204 {
        return Err(format!("Delete event failed: {}", resp.status()));
    }
    Ok(format!("Event {} deleted.", event_id))
}
```

- [ ] **Step 2: Add Calendar tool definitions and execute branches to `tools.rs`**

Add to `get_tool_definitions()`:

```rust
        Tool {
            name: "list_events".into(),
            description: "List Google Calendar events for a date range. Defaults to today + 7 days.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "date_from": { "type": "string", "description": "Start date YYYY-MM-DD (default: today)" },
                    "date_to": { "type": "string", "description": "End date YYYY-MM-DD (default: 7 days from now)" }
                }
            }),
        },
        Tool {
            name: "create_event".into(),
            description: "Create a Google Calendar event".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "start": { "type": "string", "description": "ISO datetime, e.g. 2026-03-25T09:00:00+08:00" },
                    "end": { "type": "string", "description": "ISO datetime" },
                    "location": { "type": "string" },
                    "description": { "type": "string" },
                    "attendees": { "type": "string", "description": "Comma-separated email addresses" }
                },
                "required": ["title", "start", "end"]
            }),
        },
        Tool {
            name: "update_event".into(),
            description: "Update a Google Calendar event (reschedule, change location, etc)".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "event_id": { "type": "string" },
                    "title": { "type": "string" },
                    "start": { "type": "string" },
                    "end": { "type": "string" },
                    "location": { "type": "string" },
                    "description": { "type": "string" }
                },
                "required": ["event_id"]
            }),
        },
        Tool {
            name: "delete_event".into(),
            description: "Delete a Google Calendar event by ID".into(),
            parameters: json!({
                "type": "object",
                "properties": { "event_id": { "type": "string" } },
                "required": ["event_id"]
            }),
        },
```

Add to `execute_tool` match arms (add these alongside the Gmail tools pattern):

```rust
        "list_events" | "create_event" | "update_event" | "delete_event" => {
            let token = match google_auth.get_access_token() {
                Some(t) => t,
                None => return "Google account not connected. Please connect in Settings.".to_string(),
            };
            let result = execute_calendar_tool(name, &args, &token).await;
            match result {
                Ok(r) => r,
                Err(e) if e.contains("UNAUTHORIZED") => {
                    log::info!("Calendar token expired, refreshing...");
                    if let Err(re) = google_auth.refresh_access_token().await {
                        return format!("Token refresh failed: {}", re);
                    }
                    let new_token = match google_auth.get_access_token() {
                        Some(t) => t,
                        None => return "Token refresh succeeded but no token available.".to_string(),
                    };
                    execute_calendar_tool(name, &args, &new_token).await.unwrap_or_else(|e| format!("Error: {}", e))
                }
                Err(e) => format!("Error: {}", e),
            }
        }
```

Add the helper function:

```rust
async fn execute_calendar_tool(name: &str, args: &serde_json::Value, token: &str) -> Result<String, String> {
    match name {
        "list_events" => {
            let now = chrono::Local::now();
            let tz_offset = now.format("%:z").to_string();
            let default_from = now.format("%Y-%m-%dT00:00:00%:z").to_string();
            let default_to = (now + chrono::TimeDelta::days(7)).format("%Y-%m-%dT23:59:59%:z").to_string();
            let from = args["date_from"].as_str().map(|d| format!("{}T00:00:00{}", d, tz_offset)).unwrap_or(default_from);
            let to = args["date_to"].as_str().map(|d| format!("{}T23:59:59{}", d, tz_offset)).unwrap_or(default_to);
            let events = crate::integrations::calendar::fetch_events(token, &from, &to).await?;
            if events.is_empty() { return Ok("No events found.".to_string()); }
            let formatted: Vec<String> = events.iter().map(|e| {
                format!("ID: {} | {} | {} - {} | Location: {}",
                    e.id, e.summary, e.start_time, e.end_time, e.location.as_deref().unwrap_or("none"))
            }).collect();
            Ok(formatted.join("\n"))
        }
        "create_event" => {
            let title = args["title"].as_str().unwrap_or("");
            let start = args["start"].as_str().unwrap_or("");
            let end = args["end"].as_str().unwrap_or("");
            let location = args["location"].as_str();
            let description = args["description"].as_str();
            let attendees = args["attendees"].as_str();
            let result = crate::integrations::calendar::create_event(token, title, start, end, description, location, attendees).await?;
            Ok(format!("Event created: {}", result))
        }
        "update_event" => {
            let event_id = args["event_id"].as_str().unwrap_or("");
            let title = args["title"].as_str();
            let start = args["start"].as_str();
            let end = args["end"].as_str();
            let location = args["location"].as_str();
            let description = args["description"].as_str();
            crate::integrations::calendar::update_event(token, event_id, title, start, end, location, description).await
        }
        "delete_event" => {
            let event_id = args["event_id"].as_str().unwrap_or("");
            crate::integrations::calendar::delete_event(token, event_id).await
        }
        _ => Err(format!("Unknown calendar tool: {}", name)),
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd jarvis && cargo check 2>&1 | head -30`

- [ ] **Step 4: Commit**

```bash
cd jarvis && git add src-tauri/src/integrations/calendar.rs src-tauri/src/ai/tools.rs src-tauri/src/commands/calendar.rs
git commit -m "feat: add Calendar integration tools (list, create, update, delete events)"
```

---

## Task 5: Notion Integration Tools

**Files:**
- Modify: `src-tauri/src/integrations/notion.rs` (add `get_page_content`)
- Modify: `src-tauri/src/ai/tools.rs` (add 3 Notion tool definitions + execute branches)

- [ ] **Step 1: Add `get_page_content` to `notion.rs`**

Append to `src-tauri/src/integrations/notion.rs`:

```rust
/// Fetch page content as markdown by retrieving block children
pub async fn get_page_content(api_key: &str, page_id: &str) -> Result<String, String> {
    let client = Client::new();
    let url = format!("{}/blocks/{}/children?page_size=100", NOTION_API, page_id);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Notion-Version", NOTION_VERSION)
        .send()
        .await
        .map_err(|e| format!("Notion blocks error: {}", e))?;

    if resp.status() == 401 { return Err("UNAUTHORIZED: Invalid Notion API key".to_string()); }
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("Notion API error {}: {}", s, t));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let results = body["results"].as_array().ok_or("No blocks found")?;

    let mut content = String::new();
    for block in results {
        let block_type = block["type"].as_str().unwrap_or("");
        match block_type {
            "paragraph" => {
                let text = extract_rich_text(&block["paragraph"]["rich_text"]);
                content.push_str(&text);
                content.push('\n');
            }
            "heading_1" => {
                let text = extract_rich_text(&block["heading_1"]["rich_text"]);
                content.push_str(&format!("# {}\n", text));
            }
            "heading_2" => {
                let text = extract_rich_text(&block["heading_2"]["rich_text"]);
                content.push_str(&format!("## {}\n", text));
            }
            "heading_3" => {
                let text = extract_rich_text(&block["heading_3"]["rich_text"]);
                content.push_str(&format!("### {}\n", text));
            }
            "bulleted_list_item" => {
                let text = extract_rich_text(&block["bulleted_list_item"]["rich_text"]);
                content.push_str(&format!("- {}\n", text));
            }
            "numbered_list_item" => {
                let text = extract_rich_text(&block["numbered_list_item"]["rich_text"]);
                content.push_str(&format!("1. {}\n", text));
            }
            "to_do" => {
                let text = extract_rich_text(&block["to_do"]["rich_text"]);
                let checked = block["to_do"]["checked"].as_bool().unwrap_or(false);
                content.push_str(&format!("- [{}] {}\n", if checked { "x" } else { " " }, text));
            }
            "code" => {
                let text = extract_rich_text(&block["code"]["rich_text"]);
                let lang = block["code"]["language"].as_str().unwrap_or("");
                content.push_str(&format!("```{}\n{}\n```\n", lang, text));
            }
            "divider" => { content.push_str("---\n"); }
            _ => {} // Skip unsupported block types
        }
    }
    Ok(content)
}

fn extract_rich_text(rich_text: &serde_json::Value) -> String {
    rich_text.as_array()
        .map(|arr| arr.iter()
            .filter_map(|item| item["plain_text"].as_str().map(String::from))
            .collect::<Vec<_>>()
            .join(""))
        .unwrap_or_default()
}
```

- [ ] **Step 2: Add Notion tool definitions and execute branches to `tools.rs`**

Add to `get_tool_definitions()`:

```rust
        Tool {
            name: "search_notion".into(),
            description: "Search Notion pages by query".into(),
            parameters: json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        },
        Tool {
            name: "read_notion_page".into(),
            description: "Read the full content of a Notion page as markdown".into(),
            parameters: json!({
                "type": "object",
                "properties": { "page_id": { "type": "string", "description": "Notion page ID" } },
                "required": ["page_id"]
            }),
        },
        Tool {
            name: "create_notion_page".into(),
            description: "Create a new Notion page under a parent page".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "content": { "type": "string", "description": "Page content text" },
                    "parent_id": { "type": "string", "description": "Parent page ID" }
                },
                "required": ["title", "content", "parent_id"]
            }),
        },
```

Add to `execute_tool` match arms:

```rust
        "search_notion" | "read_notion_page" | "create_notion_page" => {
            let token = {
                let conn = match db.conn.lock() {
                    Ok(c) => c,
                    Err(e) => return format!("DB error: {}", e),
                };
                conn.query_row("SELECT value FROM user_preferences WHERE key = 'notion_token'", [], |row| row.get::<_, String>(0)).ok()
            };
            let token = match token {
                Some(t) => t,
                None => return "Notion not connected. Set your token in Settings.".to_string(),
            };
            execute_notion_tool(name, &args, &token).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
```

Add the helper:

```rust
async fn execute_notion_tool(name: &str, args: &serde_json::Value, token: &str) -> Result<String, String> {
    match name {
        "search_notion" => {
            let query = args["query"].as_str().unwrap_or("");
            let pages = crate::integrations::notion::search_pages(token, Some(query)).await?;
            if pages.is_empty() { return Ok("No Notion pages found.".to_string()); }
            let formatted: Vec<String> = pages.iter().take(20).map(|p| {
                format!("ID: {} | {} | Edited: {} | URL: {}",
                    p.notion_id, p.title, p.last_edited.as_deref().unwrap_or("?"), p.url.as_deref().unwrap_or(""))
            }).collect();
            Ok(formatted.join("\n"))
        }
        "read_notion_page" => {
            let page_id = args["page_id"].as_str().unwrap_or("");
            crate::integrations::notion::get_page_content(token, page_id).await
        }
        "create_notion_page" => {
            let title = args["title"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            let parent_id = args["parent_id"].as_str().unwrap_or("");
            let id = crate::integrations::notion::create_page(token, parent_id, title, content).await?;
            Ok(format!("Notion page created: {}", id))
        }
        _ => Err(format!("Unknown Notion tool: {}", name)),
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd jarvis && cargo check 2>&1 | head -30`

- [ ] **Step 4: Commit**

```bash
cd jarvis && git add src-tauri/src/integrations/notion.rs src-tauri/src/ai/tools.rs
git commit -m "feat: add Notion integration tools (search, read, create pages)"
```

---

## Task 6: GitHub Integration Tools

**Files:**
- Modify: `src-tauri/src/integrations/github.rs` (extend `fetch_assigned_items`, extend `create_issue`)
- Modify: `src-tauri/src/ai/tools.rs` (add 2 GitHub tool definitions + execute branches)

- [ ] **Step 1: Add filtered fetch function and extend `create_issue`**

Add to `src-tauri/src/integrations/github.rs`:

```rust
/// Fetch items filtered by type and optional repo
pub async fn fetch_items_filtered(token: &str, item_type: &str, repo: Option<&str>) -> Result<Vec<GitHubItem>, String> {
    let all_items = fetch_assigned_items(token).await?;

    let filtered: Vec<GitHubItem> = all_items.into_iter().filter(|item| {
        let type_match = match item_type {
            "prs" => item.item_type == "pr" || item.item_type == "pr_review",
            "issues" => item.item_type == "issue",
            _ => true,
        };
        let repo_match = repo.map(|r| item.repo == r).unwrap_or(true);
        type_match && repo_match
    }).collect();

    Ok(filtered)
}
```

Replace `create_issue` to support labels:

```rust
pub async fn create_issue(
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: Option<&str>,
    labels: Option<&str>,
) -> Result<String, String> {
    let client = Client::new();
    let mut payload = serde_json::json!({ "title": title, "body": body });
    if let Some(labels_str) = labels {
        let label_list: Vec<&str> = labels_str.split(',').map(|l| l.trim()).collect();
        payload["labels"] = serde_json::json!(label_list);
    }

    let resp = client
        .post(&format!("{}/repos/{}/{}/issues", GITHUB_API, owner, repo))
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "JARVIS-App")
        .header("Accept", "application/vnd.github+json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("GitHub create issue error: {}", e))?;

    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub create issue failed {}: {}", s, t));
    }

    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(result["html_url"].as_str().unwrap_or("").to_string())
}
```

- [ ] **Step 2: Add GitHub tool definitions and execute branches to `tools.rs`**

Add to `get_tool_definitions()`:

```rust
        Tool {
            name: "list_github_items".into(),
            description: "List GitHub PRs or issues assigned to you".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "item_type": { "type": "string", "description": "'prs', 'issues', or 'all'" },
                    "repo": { "type": "string", "description": "Optional: 'owner/repo' to filter" }
                },
                "required": ["item_type"]
            }),
        },
        Tool {
            name: "create_github_issue".into(),
            description: "Create a GitHub issue in a repository".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "'owner/repo'" },
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "labels": { "type": "string", "description": "Comma-separated labels" }
                },
                "required": ["repo", "title"]
            }),
        },
```

Add to `execute_tool` match arms:

```rust
        "list_github_items" | "create_github_issue" => {
            let token = {
                let conn = match db.conn.lock() {
                    Ok(c) => c,
                    Err(e) => return format!("DB error: {}", e),
                };
                conn.query_row("SELECT value FROM user_preferences WHERE key = 'github_token'", [], |row| row.get::<_, String>(0)).ok()
            };
            let token = match token {
                Some(t) => t,
                None => return "GitHub not connected. Set your token in Settings.".to_string(),
            };
            execute_github_tool(name, &args, &token).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
```

Add the helper:

```rust
async fn execute_github_tool(name: &str, args: &serde_json::Value, token: &str) -> Result<String, String> {
    match name {
        "list_github_items" => {
            let item_type = args["item_type"].as_str().unwrap_or("all");
            let repo = args["repo"].as_str();
            let items = crate::integrations::github::fetch_items_filtered(token, item_type, repo).await?;
            if items.is_empty() { return Ok("No GitHub items found.".to_string()); }
            let formatted: Vec<String> = items.iter().map(|i| {
                format!("[{}] {} - {} ({}) | {}", i.item_type, i.repo, i.title, i.state, i.url.as_deref().unwrap_or(""))
            }).collect();
            Ok(formatted.join("\n"))
        }
        "create_github_issue" => {
            let repo_full = args["repo"].as_str().unwrap_or("");
            let parts: Vec<&str> = repo_full.splitn(2, '/').collect();
            if parts.len() != 2 { return Err("repo must be 'owner/repo' format".to_string()); }
            let title = args["title"].as_str().unwrap_or("");
            let body = args["body"].as_str();
            let labels = args["labels"].as_str();
            let url = crate::integrations::github::create_issue(token, parts[0], parts[1], title, body, labels).await?;
            Ok(format!("Issue created: {}", url))
        }
        _ => Err(format!("Unknown GitHub tool: {}", name)),
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cd jarvis && cargo check 2>&1 | head -30`

- [ ] **Step 4: Commit**

```bash
cd jarvis && git add src-tauri/src/integrations/github.rs src-tauri/src/ai/tools.rs
git commit -m "feat: add GitHub integration tools (list items, create issues with labels)"
```

---

## Task 7: Obsidian Integration Tools

**Files:**
- Modify: `src-tauri/src/ai/tools.rs` (add 2 Obsidian tool definitions + execute branches)

No changes to `obsidian.rs` needed -- existing `search_vault` and `get_note` functions match the spec.

- [ ] **Step 1: Add Obsidian tool definitions and execute branches to `tools.rs`**

Add to `get_tool_definitions()`:

```rust
        Tool {
            name: "search_notes".into(),
            description: "Search notes in the Obsidian vault by keyword".into(),
            parameters: json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        },
        Tool {
            name: "read_note".into(),
            description: "Read the full content of an Obsidian note".into(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Note path relative to vault root" } },
                "required": ["path"]
            }),
        },
```

Add to `execute_tool` match arms:

```rust
        "search_notes" | "read_note" => {
            let api_key = {
                let conn = match db.conn.lock() {
                    Ok(c) => c,
                    Err(e) => return format!("DB error: {}", e),
                };
                conn.query_row("SELECT value FROM user_preferences WHERE key = 'obsidian_api_key'", [], |row| row.get::<_, String>(0)).ok()
            };
            let api_key = match api_key {
                Some(k) => k,
                None => return "Obsidian not connected. Set your API key in Settings.".to_string(),
            };
            execute_obsidian_tool(name, &args, &api_key).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
```

Add the helper:

```rust
async fn execute_obsidian_tool(name: &str, args: &serde_json::Value, api_key: &str) -> Result<String, String> {
    match name {
        "search_notes" => {
            let query = args["query"].as_str().unwrap_or("");
            let notes = crate::integrations::obsidian::search_vault(api_key, query).await?;
            if notes.is_empty() { return Ok("No notes found.".to_string()); }
            let formatted: Vec<String> = notes.iter().take(20).map(|n| {
                format!("{} | {}", n.path, n.content.as_deref().unwrap_or("").chars().take(100).collect::<String>())
            }).collect();
            Ok(formatted.join("\n"))
        }
        "read_note" => {
            let path = args["path"].as_str().unwrap_or("");
            crate::integrations::obsidian::get_note(api_key, path).await
        }
        _ => Err(format!("Unknown Obsidian tool: {}", name)),
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd jarvis && cargo check 2>&1 | head -30`

- [ ] **Step 3: Commit**

```bash
cd jarvis && git add src-tauri/src/ai/tools.rs
git commit -m "feat: add Obsidian integration tools (search notes, read note)"
```

---

## Task 8: Update System Prompt and Final Verification

**Files:**
- Modify: `src-tauri/src/ai/tools.rs` (update SYSTEM_PROMPT constant)

- [ ] **Step 1: Update SYSTEM_PROMPT**

In `tools.rs`, replace the SYSTEM_PROMPT constant:

```rust
pub const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant on macOS for Hillman Chan (GitHub: ChiFungHillmanChan). Be concise and direct like the JARVIS from Iron Man.

You have 32 tools to control the computer and manage integrations. Use them proactively when the user asks you to do something.

Capabilities:
- System control: open apps, URLs, files, run commands, clipboard, screenshots, window management, volume/brightness, notifications, process management
- Gmail: search, read, send, and archive emails
- Google Calendar: list, create, update, and delete events
- Notion: search, read, and create pages
- GitHub: list PRs/issues, create issues
- Obsidian: search and read notes
- Tasks: create tasks and reminders
- File I/O: read file contents, write notes

You can chain multiple tools in sequence. Think step by step -- gather information first, then act. Always confirm destructive actions in your response text before executing them.";
```

- [ ] **Step 2: Full build verification**

Run: `cd jarvis && cargo build 2>&1 | tail -5`
Expected: `Finished` with no errors.

- [ ] **Step 3: Run the app to smoke test**

Run: `cd jarvis && npm run tauri dev`
Expected: App launches, chat works, tools execute.

- [ ] **Step 4: Final commit**

```bash
cd jarvis && git add src-tauri/src/ai/tools.rs
git commit -m "feat: update JARVIS system prompt for 32 tools"
```

---

## Task Summary

| Task | Description | Files | Est. |
|------|-------------|-------|------|
| 1 | Stateful tool execution plumbing | mod.rs, claude.rs, openai.rs, tools.rs, chat.rs, assistant.rs, briefing.rs, lib.rs | Core |
| 2 | 9 new system tools | control.rs, tools.rs | System |
| 3 | 4 Gmail tools + 3 new functions | gmail.rs, tools.rs, Cargo.toml | Integration |
| 4 | 4 Calendar tools + 2 new functions | calendar.rs, tools.rs | Integration |
| 5 | 3 Notion tools + 1 new function | notion.rs, tools.rs | Integration |
| 6 | 2 GitHub tools + extended functions | github.rs, tools.rs | Integration |
| 7 | 2 Obsidian tools (existing functions) | tools.rs | Integration |
| 8 | System prompt + final build | tools.rs | Finalize |
