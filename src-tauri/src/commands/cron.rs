use crate::ai::AiRouter;
use crate::db::Database;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

#[derive(Serialize)]
pub struct CronJobView { pub id: i64, pub name: String, pub schedule: String, pub action_type: String, pub status: String, pub last_run: Option<String>, pub next_run: Option<String>, pub description: Option<String> }

#[derive(Serialize)]
pub struct CronRunView { pub id: i64, pub job_id: i64, pub started_at: String, pub finished_at: Option<String>, pub status: String, pub result: Option<String>, pub error: Option<String> }

#[tauri::command]
pub fn get_cron_jobs(db: State<Arc<Database>>) -> Result<Vec<CronJobView>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id, name, schedule, action_type, status, last_run, next_run, description FROM cron_jobs ORDER BY id ASC").map_err(|e| e.to_string())?;
    let jobs = stmt.query_map([], |row| Ok(CronJobView { id: row.get(0)?, name: row.get(1)?, schedule: row.get(2)?, action_type: row.get(3)?, status: row.get(4)?, last_run: row.get(5)?, next_run: row.get(6)?, description: row.get(7)? }))
        .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(jobs)
}

#[tauri::command]
pub fn get_cron_runs(db: State<Arc<Database>>, job_id: i64, limit: Option<u32>) -> Result<Vec<CronRunView>, String> {
    let limit = limit.unwrap_or(10);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id, job_id, started_at, finished_at, status, result, error FROM cron_runs WHERE job_id = ?1 ORDER BY started_at DESC LIMIT ?2").map_err(|e| e.to_string())?;
    let runs = stmt.query_map(rusqlite::params![job_id, limit], |row| Ok(CronRunView { id: row.get(0)?, job_id: row.get(1)?, started_at: row.get(2)?, finished_at: row.get(3)?, status: row.get(4)?, result: row.get(5)?, error: row.get(6)? }))
        .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(runs)
}

#[tauri::command]
pub async fn create_custom_cron(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    google_auth: State<'_, Arc<crate::auth::google::GoogleAuth>>,
    description: String,
) -> Result<CronJobView, String> {
    // Ask AI to parse natural language into cron schedule + action
    let prompt = format!(
        "Parse this scheduling request into a JSON object. Supported schedule patterns: daily, weekly, monthly, every N hours/days. \
         Supported action_types: email_sync, calendar_sync, deadline_monitor, notion_sync, github_digest, auto_archive_emails. \
         Return ONLY valid JSON with these fields: \
         {{\"name\": \"short name\", \"schedule\": \"cron expression (6-field: sec min hour day month weekday)\", \"action_type\": \"one of the supported types\", \"description\": \"human-readable schedule like 'Every Friday at midnight'\"}} \
         \nRequest: \"{}\"", description
    );

    let messages = vec![("user".to_string(), prompt)];
    let router = router.lock().map_err(|e| e.to_string())?.clone();
    let response = router.send(messages, &db, &google_auth, &app_handle, None).await?;

    // Parse AI response as JSON
    let parsed: serde_json::Value = serde_json::from_str(response.trim().trim_start_matches("```json").trim_end_matches("```").trim())
        .map_err(|e| format!("Failed to parse AI response as JSON: {}. Response was: {}", e, response))?;

    let name = parsed["name"].as_str().ok_or("Missing 'name' in AI response")?.to_string();
    let schedule = parsed["schedule"].as_str().ok_or("Missing 'schedule' in AI response")?.to_string();
    let action_type = parsed["action_type"].as_str().ok_or("Missing 'action_type' in AI response")?.to_string();
    let desc = parsed["description"].as_str().unwrap_or("").to_string();

    // Validate action_type
    let valid_actions = ["email_sync", "calendar_sync", "deadline_monitor", "notion_sync", "github_digest", "auto_archive_emails"];
    if !valid_actions.contains(&action_type.as_str()) {
        return Err(format!("Invalid action_type '{}'. Must be one of: {}", action_type, valid_actions.join(", ")));
    }

    // Insert into database
    let job_id = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO cron_jobs (name, schedule, action_type, status, description) VALUES (?1, ?2, ?3, 'active', ?4)",
            rusqlite::params![name, schedule, action_type, desc],
        ).map_err(|e| e.to_string())?;
        conn.last_insert_rowid()
    };

    log::info!("Created custom cron job: {} ({}) -> {}", name, schedule, action_type);

    Ok(CronJobView {
        id: job_id,
        name,
        schedule,
        action_type,
        status: "active".to_string(),
        last_run: None,
        next_run: None,
        description: Some(desc),
    })
}

#[tauri::command]
pub fn delete_cron_job(db: State<Arc<Database>>, job_id: i64) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM cron_jobs WHERE id = ?1", rusqlite::params![job_id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM cron_runs WHERE job_id = ?1", rusqlite::params![job_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn toggle_cron_job(db: State<Arc<Database>>, job_id: i64) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let current: String = conn.query_row(
        "SELECT status FROM cron_jobs WHERE id = ?1", rusqlite::params![job_id], |row| row.get(0)
    ).map_err(|e| e.to_string())?;

    let new_status = if current == "active" { "paused" } else { "active" };
    conn.execute(
        "UPDATE cron_jobs SET status = ?1 WHERE id = ?2",
        rusqlite::params![new_status, job_id],
    ).map_err(|e| e.to_string())?;

    Ok(new_status.to_string())
}

#[tauri::command]
pub fn get_upcoming_runs(schedule: String, count: Option<usize>) -> Result<Vec<String>, String> {
    use cron::Schedule;
    use std::str::FromStr;
    use chrono::Local;

    let n = count.unwrap_or(3);

    // Try parsing as-is first (6-field), then try prepending seconds
    let sched = Schedule::from_str(&schedule)
        .or_else(|_| Schedule::from_str(&format!("0 {}", schedule)))
        .map_err(|e| format!("Invalid cron expression: {}", e))?;

    let runs: Vec<String> = sched
        .upcoming(Local)
        .take(n)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .collect();

    Ok(runs)
}
