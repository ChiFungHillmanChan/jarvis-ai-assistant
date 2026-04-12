use tokio::process::Command;

/// Open an application by name
pub async fn open_app(app_name: &str) -> Result<String, String> {
    let status = Command::new("open")
        .arg("-a")
        .arg(app_name)
        .status()
        .await
        .map_err(|e| format!("Failed to open {}: {}", app_name, e))?;

    if status.success() {
        Ok(format!("Opened {}", app_name))
    } else {
        Err(format!("Could not find application: {}", app_name))
    }
}

/// Open a URL in the default browser
pub async fn open_url(url: &str) -> Result<String, String> {
    let status = Command::new("open")
        .arg(url)
        .status()
        .await
        .map_err(|e| format!("Failed to open URL: {}", e))?;

    if status.success() {
        Ok(format!("Opened {}", url))
    } else {
        Err("Failed to open URL".to_string())
    }
}

/// Run a shell command and return output
pub async fn run_command(cmd: &str) -> Result<String, String> {
    // Safety: only allow read-only / informational commands
    let blocked = ["rm ", "sudo ", "mkfs", "dd ", "> /dev", "chmod 777", "curl | sh", "wget | sh"];
    let cmd_lower = cmd.to_lowercase();
    if blocked.iter().any(|b| cmd_lower.contains(b)) {
        return Err("Command blocked for safety. JARVIS won't run destructive commands.".to_string());
    }

    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .await
        .map_err(|e| format!("Command failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(if stdout.is_empty() { "Command completed.".to_string() } else { stdout.trim().to_string() })
    } else {
        Err(if stderr.is_empty() { "Command failed".to_string() } else { stderr.trim().to_string() })
    }
}

/// Search for files by name
pub async fn find_files(query: &str, search_path: Option<&str>) -> Result<Vec<String>, String> {
    let path = search_path.unwrap_or("~");
    let output = Command::new("mdfind")
        .arg("-name")
        .arg(query)
        .arg("-onlyin")
        .arg(shellexpand::tilde(path).to_string())
        .output()
        .await
        .map_err(|e| format!("Search failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = stdout.lines()
        .take(20)
        .map(|l| l.to_string())
        .collect();

    Ok(files)
}

/// Open a file with its default application
pub async fn open_file(path: &str) -> Result<String, String> {
    let expanded = shellexpand::tilde(path).to_string();
    let status = Command::new("open")
        .arg(&expanded)
        .status()
        .await
        .map_err(|e| format!("Failed to open file: {}", e))?;

    if status.success() {
        Ok(format!("Opened {}", path))
    } else {
        Err(format!("Could not open: {}", path))
    }
}

/// Get system info
pub async fn system_info() -> Result<String, String> {
    let hostname = Command::new("hostname").output().await.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let uptime = Command::new("uptime").output().await.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let disk = Command::new("df").arg("-h").arg("/").output().await.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let memory = Command::new("sh").arg("-c").arg("vm_stat | head -5").output().await.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();

    Ok(format!("Host: {}\n{}\nDisk:\n{}\nMemory:\n{}", hostname, uptime, disk, memory))
}

/// Write/append to a local text file (for quick notes)
pub async fn write_note(path: &str, content: &str, append: bool) -> Result<String, String> {
    let expanded = shellexpand::tilde(path).to_string();
    if append {
        let existing = tokio::fs::read_to_string(&expanded).await.unwrap_or_default();
        let new_content = format!("{}\n\n---\n{}: {}", existing, chrono::Local::now().format("%Y-%m-%d %H:%M"), content);
        tokio::fs::write(&expanded, new_content).await.map_err(|e| e.to_string())?;
    } else {
        tokio::fs::write(&expanded, content).await.map_err(|e| e.to_string())?;
    }
    Ok(format!("Note saved to {}", path))
}

/// Read clipboard contents via pbpaste
pub async fn clipboard_read() -> Result<String, String> {
    let output = Command::new("pbpaste")
        .output()
        .await
        .map_err(|e| format!("Failed to read clipboard: {}", e))?;

    if output.status.success() {
        let content = String::from_utf8_lossy(&output.stdout).to_string();
        if content.is_empty() {
            Ok("Clipboard is empty.".to_string())
        } else {
            Ok(content)
        }
    } else {
        Err("Failed to read clipboard".to_string())
    }
}

/// Write content to clipboard via pbcopy
pub async fn clipboard_write(content: &str) -> Result<String, String> {
    use tokio::io::AsyncWriteExt;

    let mut child = Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start pbcopy: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(content.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to clipboard: {}", e))?;
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("pbcopy failed: {}", e))?;

    if status.success() {
        Ok(format!("Copied {} characters to clipboard.", content.len()))
    } else {
        Err("Failed to write to clipboard".to_string())
    }
}

/// Remove jarvis screenshot temp files older than 1 hour.
fn cleanup_stale_screenshots() {
    let Ok(entries) = std::fs::read_dir("/tmp") else { return };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
        if !name_str.starts_with("jarvis_screenshot_") || !name_str.ends_with(".png") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                if modified < cutoff {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// Take a screenshot using screencapture
pub async fn screenshot(region: &str) -> Result<String, String> {
    cleanup_stale_screenshots();

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let path = format!("/tmp/jarvis_screenshot_{}.png", timestamp);

    let mut cmd = Command::new("screencapture");
    match region {
        "selection" => { cmd.arg("-i"); }
        "window" => { cmd.arg("-w"); }
        _ => {} // fullscreen, no extra flag
    }
    cmd.arg(&path);

    let status = cmd
        .status()
        .await
        .map_err(|e| format!("Screenshot failed: {}", e))?;

    if status.success() {
        Ok(format!("Screenshot saved to {}", path))
    } else {
        Err("Screenshot cancelled or failed".to_string())
    }
}

/// Manage windows via AppleScript
pub async fn manage_window(
    action: &str,
    app_name: Option<&str>,
    width: Option<i64>,
    height: Option<i64>,
    x: Option<i64>,
    y: Option<i64>,
) -> Result<String, String> {
    let script = match action {
        "list" => {
            r#"tell application "System Events"
                set windowList to ""
                repeat with proc in (every process whose visible is true)
                    set procName to name of proc
                    try
                        repeat with w in (every window of proc)
                            set windowList to windowList & procName & ": " & (name of w) & linefeed
                        end repeat
                    end try
                end repeat
                return windowList
            end tell"#.to_string()
        }
        "focus" => {
            let app = app_name.ok_or("app_name required for focus")?;
            format!(
                r#"tell application "{}" to activate"#,
                app
            )
        }
        "resize" => {
            let app = app_name.ok_or("app_name required for resize")?;
            let w = width.unwrap_or(1200);
            let h = height.unwrap_or(800);
            format!(
                r#"tell application "System Events"
                    tell process "{}"
                        set size of window 1 to {{{}, {}}}
                    end tell
                end tell"#,
                app, w, h
            )
        }
        "move" => {
            let app = app_name.ok_or("app_name required for move")?;
            let px = x.unwrap_or(0);
            let py = y.unwrap_or(0);
            format!(
                r#"tell application "System Events"
                    tell process "{}"
                        set position of window 1 to {{{}, {}}}
                    end tell
                end tell"#,
                app, px, py
            )
        }
        _ => return Err(format!("Unknown window action: {}. Use: list, focus, resize, move", action)),
    };

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await
        .map_err(|e| format!("AppleScript failed: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if stdout.is_empty() { format!("Window {} done.", action) } else { stdout })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("Window {} failed: {}", action, stderr))
    }
}

/// System controls: volume, brightness, dark mode
pub async fn system_controls(action: &str, value: Option<i64>) -> Result<String, String> {
    match action {
        "volume_set" => {
            let vol = value.unwrap_or(50);
            let vol_clamped = vol.clamp(0, 100);
            let script = format!("set volume output volume {}", vol_clamped);
            let output = Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
                .await
                .map_err(|e| format!("Volume set failed: {}", e))?;
            if output.status.success() {
                Ok(format!("Volume set to {}%", vol_clamped))
            } else {
                Err("Failed to set volume".to_string())
            }
        }
        "volume_get" => {
            let output = Command::new("osascript")
                .arg("-e")
                .arg("output volume of (get volume settings)")
                .output()
                .await
                .map_err(|e| format!("Volume get failed: {}", e))?;
            let vol = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(format!("Current volume: {}%", vol))
        }
        "volume_mute" => {
            let output = Command::new("osascript")
                .arg("-e")
                .arg("set volume with output muted")
                .output()
                .await
                .map_err(|e| format!("Mute failed: {}", e))?;
            if output.status.success() {
                Ok("Volume muted.".to_string())
            } else {
                Err("Failed to mute".to_string())
            }
        }
        "volume_unmute" => {
            let output = Command::new("osascript")
                .arg("-e")
                .arg("set volume without output muted")
                .output()
                .await
                .map_err(|e| format!("Unmute failed: {}", e))?;
            if output.status.success() {
                Ok("Volume unmuted.".to_string())
            } else {
                Err("Failed to unmute".to_string())
            }
        }
        "brightness_set" => {
            let val = value.unwrap_or(50);
            let brightness = (val.clamp(0, 100) as f64) / 100.0;
            let output = Command::new("brightness")
                .arg(format!("{:.2}", brightness))
                .output()
                .await
                .map_err(|e| format!("Brightness failed (is `brightness` CLI installed?): {}", e))?;
            if output.status.success() {
                Ok(format!("Brightness set to {}%", val))
            } else {
                Err("Failed to set brightness. Install with: brew install brightness".to_string())
            }
        }
        "dark_mode_on" => {
            let output = Command::new("osascript")
                .arg("-e")
                .arg(r#"tell application "System Events" to tell appearance preferences to set dark mode to true"#)
                .output()
                .await
                .map_err(|e| format!("Dark mode failed: {}", e))?;
            if output.status.success() {
                Ok("Dark mode enabled.".to_string())
            } else {
                Err("Failed to enable dark mode".to_string())
            }
        }
        "dark_mode_off" => {
            let output = Command::new("osascript")
                .arg("-e")
                .arg(r#"tell application "System Events" to tell appearance preferences to set dark mode to false"#)
                .output()
                .await
                .map_err(|e| format!("Dark mode failed: {}", e))?;
            if output.status.success() {
                Ok("Dark mode disabled.".to_string())
            } else {
                Err("Failed to disable dark mode".to_string())
            }
        }
        "dark_mode_toggle" => {
            let output = Command::new("osascript")
                .arg("-e")
                .arg(r#"tell application "System Events" to tell appearance preferences to set dark mode to not dark mode"#)
                .output()
                .await
                .map_err(|e| format!("Dark mode toggle failed: {}", e))?;
            if output.status.success() {
                Ok("Dark mode toggled.".to_string())
            } else {
                Err("Failed to toggle dark mode".to_string())
            }
        }
        _ => Err(format!("Unknown system control: {}. Use: volume_set, volume_get, volume_mute, volume_unmute, brightness_set, dark_mode_on, dark_mode_off, dark_mode_toggle", action)),
    }
}

/// Send a macOS notification
pub async fn send_notification(title: &str, message: &str, sound: bool) -> Result<String, String> {
    let sound_part = if sound {
        r#" sound name "Glass""#
    } else {
        ""
    };
    let script = format!(
        r#"display notification "{}" with title "{}"{}"#,
        message.replace('"', r#"\""#),
        title.replace('"', r#"\""#),
        sound_part
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await
        .map_err(|e| format!("Notification failed: {}", e))?;

    if output.status.success() {
        Ok(format!("Notification sent: {}", title))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("Notification failed: {}", stderr))
    }
}

/// List running processes with optional filter
pub async fn list_processes(filter: Option<&str>) -> Result<String, String> {
    let output = Command::new("ps")
        .arg("aux")
        .output()
        .await
        .map_err(|e| format!("ps failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    if let Some(f) = filter {
        let f_lower = f.to_lowercase();
        let header = lines.first().copied().unwrap_or("");
        let filtered: Vec<&str> = lines
            .iter()
            .skip(1)
            .filter(|l| l.to_lowercase().contains(&f_lower))
            .take(30)
            .copied()
            .collect();
        if filtered.is_empty() {
            Ok(format!("No processes matching '{}'", f))
        } else {
            Ok(format!("{}\n{}", header, filtered.join("\n")))
        }
    } else {
        let result: Vec<&str> = lines.into_iter().take(31).collect(); // header + 30 lines
        Ok(result.join("\n"))
    }
}

/// Kill a process by PID (with safety guards)
pub async fn kill_process(pid: i64) -> Result<String, String> {
    if pid < 100 {
        return Err(format!("Refusing to kill PID {} -- low PIDs are system-critical.", pid));
    }

    // Check process name for protected processes
    let check = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("comm=")
        .output()
        .await
        .map_err(|e| format!("Failed to check process: {}", e))?;

    let proc_name = String::from_utf8_lossy(&check.stdout).trim().to_string();
    let protected = ["kernel_task", "WindowServer", "loginwindow", "launchd", "syslogd"];
    for p in &protected {
        if proc_name.contains(p) {
            return Err(format!("Refusing to kill protected process: {} ({})", proc_name, pid));
        }
    }

    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .await
        .map_err(|e| format!("Kill failed: {}", e))?;

    if status.success() {
        Ok(format!("Process {} killed.", pid))
    } else {
        Err(format!("Failed to kill process {}. It may not exist or require elevated permissions.", pid))
    }
}

/// Read a file with size and line limits
pub async fn read_file(path: &str, max_lines: Option<usize>) -> Result<String, String> {
    let expanded = shellexpand::tilde(path).to_string();

    // Check file size first (100KB limit)
    let metadata = tokio::fs::metadata(&expanded)
        .await
        .map_err(|e| format!("Cannot access {}: {}", path, e))?;

    if metadata.len() > 100 * 1024 {
        return Err(format!(
            "File too large ({} bytes). Max 100KB for safety.",
            metadata.len()
        ));
    }

    let bytes = tokio::fs::read(&expanded)
        .await
        .map_err(|e| format!("Failed to read {}: {}", path, e))?;

    // Binary check: look for null bytes in first 512 bytes
    let check_len = bytes.len().min(512);
    if bytes[..check_len].contains(&0) {
        return Err("File appears to be binary. Use open_file instead.".to_string());
    }

    let content = String::from_utf8_lossy(&bytes).to_string();

    let lines_limit = max_lines.unwrap_or(200);
    let lines: Vec<&str> = content.lines().take(lines_limit).collect();
    let total_lines = content.lines().count();

    let mut result = lines.join("\n");
    if total_lines > lines_limit {
        result.push_str(&format!("\n... [{} more lines]", total_lines - lines_limit));
    }

    Ok(result)
}
