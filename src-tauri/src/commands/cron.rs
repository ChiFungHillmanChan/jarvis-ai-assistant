use crate::ai::{AiRouter, Workload};
use crate::db::Database;
use croner::Cron;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::State;

static CRON_MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const VALID_ACTIONS: &[&str] = &["email_sync", "calendar_sync", "deadline_monitor", "notion_sync", "github_digest", "auto_archive_emails"];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronInterpretation { pub name: String, pub schedule: String, pub action_type: String, pub description: String }
fn validate_interpretation(value: &CronInterpretation) -> Result<(), String> {
    if value.name.trim().is_empty() || value.name.len() > 200 || value.description.len() > 4000 || value.schedule.len() > 200 { return Err("Invalid automation name, description or schedule length".into()); }
    if !VALID_ACTIONS.contains(&value.action_type.as_str()) { return Err("Unsupported automation action".into()); }
    parse_schedule(&value.schedule)?;
    Ok(())
}

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
    validate_interpretation(&CronInterpretation { name:name.into(),schedule:schedule.into(),action_type:action_type.into(),description:description.into() })?;
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
    let interpretation = preview_custom_cron(app_handle, db.clone(), router, google_auth, description).await?;
    save_interpretation(&db, &scheduler, interpretation).await
}

#[tauri::command]
pub async fn preview_custom_cron(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    google_auth: State<'_, Arc<crate::auth::google::GoogleAuth>>,
    description: String,
) -> Result<CronInterpretation, String> {
    if description.trim().is_empty() || description.len() > 4000 { return Err("Describe an automation in at most 4000 bytes".into()); }
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
    let _guard = crate::chat_session::FOREGROUND.try_lock().map_err(|_| "另一個助理任務進行中，請等完成後再試。".to_string())?;
    let response = crate::chat_session::TOOL_EXECUTION_DISABLED.scope(true,
        router.send(messages, &db, &google_auth, &app_handle, None, Workload::Volume)).await?;

    parse_interpretation(&response)
}

fn parse_interpretation(response: &str) -> Result<CronInterpretation, String> {
    let value: CronInterpretation = serde_json::from_str(response.trim().trim_start_matches("```json").trim_end_matches("```").trim())
        .map_err(|_| "Could not interpret this schedule. Please clarify the timing and action.".to_string())?;
    validate_interpretation(&value)?;
    Ok(value)
}

#[tauri::command]
pub async fn create_reviewed_cron(
    db: State<'_, Arc<Database>>,
    scheduler: State<'_, Arc<crate::scheduler::SchedulerHandle>>,
    interpretation: CronInterpretation,
) -> Result<CronJobView, String> {
    validate_interpretation(&interpretation)?;
    save_interpretation(&db, &scheduler, interpretation).await
}

async fn save_interpretation(
    db: &Database,
    scheduler: &crate::scheduler::SchedulerHandle,
    interpretation: CronInterpretation,
) -> Result<CronJobView, String> {
    let _guard = CRON_MUTATION.lock().await;
    let CronInterpretation {name,schedule,action_type,description:desc} = interpretation;
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
    let _guard = CRON_MUTATION.lock().await;
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
    change_cron_status(&db, &scheduler, job_id, None).await
}

#[tauri::command]
pub async fn set_cron_job_status(
    db: State<'_, Arc<Database>>,
    scheduler: State<'_, Arc<crate::scheduler::SchedulerHandle>>,
    job_id: i64,
    status: String,
) -> Result<String, String> {
    if status != "active" && status != "paused" { return Err("Status must be active or paused".into()); }
    change_cron_status(&db, &scheduler, job_id, Some(status)).await
}

async fn change_cron_status(db: &Database, scheduler: &crate::scheduler::SchedulerHandle, job_id: i64, desired: Option<String>) -> Result<String, String> {
    let _guard = CRON_MUTATION.lock().await;
    let (new_status, name, schedule, action_type) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let (current, name, schedule, action_type): (String, String, String, String) = conn.query_row(
            "SELECT status, name, schedule, action_type FROM cron_jobs WHERE id = ?1",
            rusqlite::params![job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(|e| e.to_string())?;

        let new_status = desired.as_deref().unwrap_or(if current == "active" { "paused" } else { "active" });
        if new_status == current { return Ok(current); }
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

    #[test]
    fn unsupported_reviewed_action_is_rejected_before_insert() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        assert!(insert_cron_job(&conn,"Bad","0 0 9 * * *","send_email","Bad").is_err());
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM cron_jobs WHERE name='Bad'",[],|r|r.get(0)).unwrap();
        assert_eq!(count,0);
    }

    #[tokio::test]
    async fn reviewed_schedule_is_not_saved_until_create_and_status_is_idempotent() {
        let db = crate::db::test_database();
        let scheduler = crate::scheduler::SchedulerHandle::new();
        let before: i64 = db.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM cron_jobs",[],|r|r.get(0)).unwrap();
        let interpretation = parse_interpretation(r#"{"name":"Reviewed","schedule":"0 0 9 * * 1","action_type":"calendar_sync","description":"Monday at 9"}"#).unwrap();
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM cron_jobs",[],|r|r.get::<_,i64>(0)).unwrap(),before);
        let job = save_interpretation(&db,&scheduler,interpretation).await.unwrap();
        assert_eq!(job.schedule,"0 0 9 * * 1");
        assert_eq!(change_cron_status(&db,&scheduler,job.id,Some("paused".into())).await.unwrap(),"paused");
        assert_eq!(change_cron_status(&db,&scheduler,job.id,Some("paused".into())).await.unwrap(),"paused");
        assert_eq!(change_cron_status(&db,&scheduler,job.id,Some("active".into())).await.unwrap(),"active");
    }
    #[tokio::test]
    async fn preview_context_blocks_tools_before_database_side_effects() {
        let db = crate::db::test_database();
        let auth = Arc::new(crate::auth::google::GoogleAuth::new_empty());
        let ledger = crate::ai::SideEffectLedger::new();
        let output = crate::chat_session::TOOL_EXECUTION_DISABLED.scope(true,
            crate::ai::tools::execute_tool("create_task",r#"{"title":"Must not exist"}"#,&db,&auth,&ledger)).await;
        assert!(output.contains("disabled"));
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM tasks WHERE title='Must not exist'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
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
