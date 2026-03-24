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
pub fn execute_actions(response: &str, db: &Arc<Database>) -> (String, Vec<ActionResult>) {
    let mut clean_lines = Vec::new();
    let mut actions = Vec::new();

    for line in response.lines() {
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
        } else {
            clean_lines.push(line);
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
