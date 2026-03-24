use crate::db::Database;
use std::sync::Arc;

#[derive(serde::Serialize, Clone, Debug)]
pub struct ActionResult {
    pub action_taken: String,
    pub details: String,
    pub success: bool,
}

/// Parse and execute actions from AI response text.
/// Supported patterns:
/// [TASK:title|description|deadline|priority]  -- create task
/// [REMIND:title|datetime]                     -- create task with deadline
/// [NOTE:content]                              -- save to conversations as a note
/// [OPEN_APP:AppName]                          -- open an application
/// [OPEN_URL:https://...]                      -- open a URL
/// [RUN_CMD:command]                           -- run a shell command
/// [FIND_FILE:filename]                        -- search for files
/// [OPEN_FILE:/path/to/file]                   -- open a file
/// [NOTE:/path/to/file|content]                -- write a note to file
/// [SYSTEM_INFO]                               -- get system info
pub async fn execute_actions(response: &str, db: &Arc<Database>) -> (String, Vec<ActionResult>) {
    let mut clean_lines: Vec<String> = Vec::new();
    let mut actions = Vec::new();

    for raw_line in response.lines() {
        let line = raw_line.trim();
        if line.starts_with("[TASK:") && line.ends_with(']') {
            let inner = &line[6..line.len() - 1];
            let parts: Vec<&str> = inner.splitn(4, '|').collect();
            if let Some(title) = parts.first() {
                let description = parts
                    .get(1)
                    .and_then(|d| if d.is_empty() { None } else { Some(d.to_string()) });
                let deadline = parts
                    .get(2)
                    .and_then(|d| if d.is_empty() { None } else { Some(d.to_string()) });
                let priority: i32 = parts.get(3).and_then(|p| p.parse().ok()).unwrap_or(1);

                match create_task(db, title, description.as_deref(), deadline.as_deref(), priority)
                {
                    Ok(_) => actions.push(ActionResult {
                        action_taken: "task_created".into(),
                        details: title.to_string(),
                        success: true,
                    }),
                    Err(e) => actions.push(ActionResult {
                        action_taken: "task_created".into(),
                        details: e,
                        success: false,
                    }),
                }
            }
        } else if line.starts_with("[REMIND:") && line.ends_with(']') {
            let inner = &line[8..line.len() - 1];
            let parts: Vec<&str> = inner.splitn(2, '|').collect();
            if let Some(title) = parts.first() {
                let deadline = parts.get(1).map(|d| d.to_string());
                match create_task(db, title, None, deadline.as_deref(), 2) {
                    Ok(_) => actions.push(ActionResult {
                        action_taken: "reminder_created".into(),
                        details: title.to_string(),
                        success: true,
                    }),
                    Err(e) => actions.push(ActionResult {
                        action_taken: "reminder_created".into(),
                        details: e,
                        success: false,
                    }),
                }
            }
        } else if line.starts_with("[OPEN_APP:") && line.ends_with("]") {
            let app = &line[10..line.len()-1];
            match crate::system::control::open_app(app).await {
                Ok(msg) => { clean_lines.push(format!("Done: {}", msg)); actions.push(ActionResult { action_taken: "open_app".into(), details: msg, success: true }); },
                Err(ref e) => { clean_lines.push(format!("Failed: {}", e)); actions.push(ActionResult { action_taken: "open_app".into(), details: e.clone(), success: false }); },
            }
        } else if line.starts_with("[OPEN_URL:") && line.ends_with("]") {
            let url = &line[10..line.len()-1];
            match crate::system::control::open_url(url).await {
                Ok(msg) => { clean_lines.push(format!("Done: {}", msg)); actions.push(ActionResult { action_taken: "open_url".into(), details: msg, success: true }); },
                Err(ref e) => { clean_lines.push(format!("Failed: {}", e)); actions.push(ActionResult { action_taken: "open_url".into(), details: e.clone(), success: false }); },
            }
        } else if line.starts_with("[RUN_CMD:") && line.ends_with("]") {
            let cmd = &line[9..line.len()-1];
            match crate::system::control::run_command(cmd).await {
                Ok(msg) => { clean_lines.push(format!("Result: {}", msg)); actions.push(ActionResult { action_taken: "run_command".into(), details: msg, success: true }); },
                Err(ref e) => { clean_lines.push(format!("Failed: {}", e)); actions.push(ActionResult { action_taken: "run_command".into(), details: e.clone(), success: false }); },
            }
        } else if line.starts_with("[FIND_FILE:") && line.ends_with("]") {
            let query = &line[11..line.len()-1];
            match crate::system::control::find_files(query, None).await {
                Ok(files) => { let detail = files.join("\n"); clean_lines.push(format!("Found:\n{}", detail)); actions.push(ActionResult { action_taken: "find_files".into(), details: detail, success: true }); },
                Err(ref e) => { clean_lines.push(format!("Failed: {}", e)); actions.push(ActionResult { action_taken: "find_files".into(), details: e.clone(), success: false }); },
            }
        } else if line.starts_with("[OPEN_FILE:") && line.ends_with("]") {
            let path = &line[11..line.len()-1];
            match crate::system::control::open_file(path).await {
                Ok(msg) => { clean_lines.push(format!("Done: {}", msg)); actions.push(ActionResult { action_taken: "open_file".into(), details: msg, success: true }); },
                Err(ref e) => { clean_lines.push(format!("Failed: {}", e)); actions.push(ActionResult { action_taken: "open_file".into(), details: e.clone(), success: false }); },
            }
        } else if line.starts_with("[NOTE:") && line.ends_with("]") {
            let inner = &line[6..line.len()-1];
            if let Some(idx) = inner.find('|') {
                let path = &inner[..idx];
                let content = &inner[idx+1..];
                match crate::system::control::write_note(path, content, true).await {
                    Ok(msg) => { clean_lines.push(format!("Done: {}", msg)); actions.push(ActionResult { action_taken: "write_note".into(), details: msg, success: true }); },
                    Err(ref e) => { clean_lines.push(format!("Failed: {}", e)); actions.push(ActionResult { action_taken: "write_note".into(), details: e.clone(), success: false }); },
                }
            }
        } else if line.trim() == "[SYSTEM_INFO]" {
            match crate::system::control::system_info().await {
                Ok(info) => { clean_lines.push(format!("System:\n{}", info)); actions.push(ActionResult { action_taken: "system_info".into(), details: info, success: true }); },
                Err(ref e) => { clean_lines.push(format!("Failed: {}", e)); actions.push(ActionResult { action_taken: "system_info".into(), details: e.clone(), success: false }); },
            }
        } else {
            clean_lines.push(line.to_string());
        }
    }

    (clean_lines.join("\n"), actions)
}

fn create_task(
    db: &Arc<Database>,
    title: &str,
    description: Option<&str>,
    deadline: Option<&str>,
    priority: i32,
) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO tasks (title, description, deadline, priority) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![title, description, deadline, priority],
    )
    .map_err(|e| e.to_string())?;
    log::info!("Action: created task '{}'", title);
    Ok(())
}
