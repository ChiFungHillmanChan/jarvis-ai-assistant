use crate::ai::{AiRouter, Workload};
use crate::db::Database;
use croner::Cron;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

/// The one parser. `tokio-cron-scheduler` reads every registration with croner
/// in exactly this configuration, so validating or previewing with a different
/// one (the `cron` crate numbers weekdays from Sunday = 1, croner from
/// Sunday = 0) showed the user a schedule other than the one that runs.
fn parse_schedule(schedule: &str) -> Result<Cron, String> {
    Cron::new(schedule)
        .with_seconds_required()
        .with_dom_and_dow()
        .parse()
        .map_err(|e| format!("Invalid cron expression: {}", e))
}

/// Parse before touching the table. The insert used to happen first, so an
/// expression the scheduler cannot read reached `cron_jobs` and was only
/// undone by the registration failure path below -- and while the scheduler
/// was still booting there was no such path, leaving a permanently dead row
/// the dashboard called active.
fn insert_cron_job(
    conn: &rusqlite::Connection,
    name: &str,
    schedule: &str,
    action_type: &str,
    description: &str,
) -> Result<i64, String> {
    parse_schedule(schedule)?;
    conn.execute(
        "INSERT INTO cron_jobs (name, schedule, action_type, status, description) VALUES (?1, ?2, ?3, 'active', ?4)",
        rusqlite::params![name, schedule, action_type, description],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

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
    scheduler: State<'_, Arc<crate::scheduler::SchedulerHandle>>,
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
    let response = router.send(messages, &db, &google_auth, &app_handle, None, Workload::Tools).await?;

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

    // Validate the model-produced schedule, then insert
    let job_id = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        insert_cron_job(&conn, &name, &schedule, &action_type, &desc)?
    };

    // Register with the live scheduler now. The scheduler only reads the table
    // at startup, so without this the job sits there reporting "active" and
    // never fires until the app is relaunched.
    match scheduler.get() {
        Some(sched) => {
            if let Err(err) = sched.register(job_id, &name, &schedule, &action_type).await {
                // Never leave behind a row the dashboard calls active that
                // nothing can run -- and that would fail scheduler init on the
                // next launch.
                {
                    let conn = db.conn.lock().map_err(|e| e.to_string())?;
                    let _ = conn.execute(
                        "DELETE FROM cron_jobs WHERE id = ?1",
                        rusqlite::params![job_id],
                    );
                }
                return Err(format!("Could not schedule '{}': {}", name, err));
            }
        }
        None => log::warn!(
            "Scheduler not started yet; cron job {} will be registered on the next launch",
            job_id
        ),
    }

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
pub async fn delete_cron_job(
    db: State<'_, Arc<Database>>,
    scheduler: State<'_, Arc<crate::scheduler::SchedulerHandle>>,
    job_id: i64,
) -> Result<(), String> {
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        // Delete child run rows before the parent job: cron_runs.job_id has a
        // foreign key to cron_jobs(id) (with foreign_keys=ON), so removing the
        // parent first fails the constraint and the job can never be deleted. Wrap
        // both in one transaction so a failure can't leave orphaned run rows.
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM cron_runs WHERE job_id = ?1", rusqlite::params![job_id])
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM cron_jobs WHERE id = ?1", rusqlite::params![job_id])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }

    // The row is gone; tear down its live registration too, otherwise the tick
    // keeps firing for a job that no longer exists.
    if let Some(sched) = scheduler.get() {
        sched.unregister(job_id).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_cron_job(
    db: State<'_, Arc<Database>>,
    scheduler: State<'_, Arc<crate::scheduler::SchedulerHandle>>,
    job_id: i64,
) -> Result<String, String> {
    let (new_status, name, schedule, action_type) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let (current, name, schedule, action_type): (String, String, String, String) = conn.query_row(
            "SELECT status, name, schedule, action_type FROM cron_jobs WHERE id = ?1",
            rusqlite::params![job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(|e| e.to_string())?;

        let new_status = if current == "active" { "paused" } else { "active" };
        conn.execute(
            "UPDATE cron_jobs SET status = ?1 WHERE id = ?2",
            rusqlite::params![new_status, job_id],
        ).map_err(|e| e.to_string())?;

        (new_status.to_string(), name, schedule, action_type)
    };

    // Keep the live scheduler in step with the row. Pausing used to leave the
    // job registered (it still ticked, and only run_job's status re-check made
    // it a no-op) and resuming left it dead until the next launch.
    if let Some(sched) = scheduler.get() {
        if new_status == "active" {
            if let Err(e) = sched.register(job_id, &name, &schedule, &action_type).await {
                // Put the row back where it was rather than report active for a
                // job the scheduler refused.
                {
                    let conn = db.conn.lock().map_err(|le| le.to_string())?;
                    let _ = conn.execute(
                        "UPDATE cron_jobs SET status = 'paused' WHERE id = ?1",
                        rusqlite::params![job_id],
                    );
                }
                return Err(format!("Could not resume '{}': {}", name, e));
            }
        } else {
            sched.unregister(job_id).await;
        }
    }

    Ok(new_status)
}

#[tauri::command]
pub fn get_upcoming_runs(schedule: String, count: Option<usize>) -> Result<Vec<String>, String> {
    let n = count.unwrap_or(3);

    let cron = parse_schedule(&schedule)?;

    let runs: Vec<String> = cron
        .iter_from(chrono::Local::now())
        .take(n)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .collect();

    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, NaiveDateTime, Weekday};

    /// An expression the scheduler cannot read must never reach `cron_jobs`.
    #[test]
    fn bad_schedule_is_rejected_before_insert() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cron_jobs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL,
                 schedule TEXT NOT NULL,
                 action_type TEXT NOT NULL,
                 status TEXT DEFAULT 'active',
                 description TEXT);",
        )
        .unwrap();

        let err = insert_cron_job(&conn, "Friday digest", "every friday", "email_sync", "Every Friday")
            .unwrap_err();
        assert!(err.contains("Invalid cron expression"), "the parse error must reach the caller: {}", err);

        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM cron_jobs", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0, "a job the scheduler would refuse must not be inserted");

        let id = insert_cron_job(&conn, "Friday digest", "0 0 9 * * 5", "email_sync", "Fridays at 9am").unwrap();
        assert_eq!(id, 1, "a schedule the scheduler accepts is still inserted");
    }

    /// The preview and the scheduler must agree on which day fires. The `cron`
    /// crate numbered weekdays Sunday = 1, so '0 0 9 * * 1' previewed as Sunday
    /// while tokio-cron-scheduler (croner, Sunday = 0) ran it on Monday.
    #[test]
    fn upcoming_runs_use_croner_weekdays() {
        let runs = get_upcoming_runs("0 0 9 * * 1".to_string(), Some(3)).unwrap();
        assert_eq!(runs.len(), 3);
        for run in &runs {
            let dt = NaiveDateTime::parse_from_str(run, "%Y-%m-%d %H:%M")
                .unwrap_or_else(|e| panic!("unexpected format '{}': {}", run, e));
            assert_eq!(dt.weekday(), Weekday::Mon, "weekday 1 is Monday: {}", run);
            assert_eq!(dt.format("%H:%M").to_string(), "09:00", "hour field must be honoured: {}", run);
        }
    }
}
