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
