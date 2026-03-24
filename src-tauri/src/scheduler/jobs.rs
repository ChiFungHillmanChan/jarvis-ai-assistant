use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::{calendar, gmail, notion, github};
use std::sync::Arc;

pub async fn run_job(db: &Arc<Database>, auth: &Arc<GoogleAuth>, action_type: &str, job_id: i64) -> Result<String, String> {
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO cron_runs (job_id, status) VALUES (?1, 'running')", rusqlite::params![job_id]).map_err(|e| e.to_string())?;
    }

    let result = match action_type {
        "email_sync" => run_email_sync(db, auth).await,
        "calendar_sync" => run_calendar_sync(db, auth).await,
        "deadline_monitor" => run_deadline_monitor(db).await,
        "notion_sync" => run_notion_sync(db).await,
        "github_digest" => run_github_digest(db).await,
        "auto_archive_emails" => run_auto_archive(db, auth).await,
        "proactive_check" => run_proactive_check(db).await,
        other => Err(format!("Unknown job type: {}", other)),
    };

    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let (status, result_text, error_text) = match &result {
            Ok(msg) => ("completed", Some(msg.as_str()), None),
            Err(e) => ("failed", None, Some(e.as_str())),
        };
        conn.execute(
            "UPDATE cron_runs SET finished_at = datetime('now'), status = ?1, result = ?2, error = ?3
             WHERE id = (SELECT id FROM cron_runs WHERE job_id = ?4 AND status = 'running' ORDER BY id DESC LIMIT 1)",
            rusqlite::params![status, result_text, error_text, job_id],
        ).map_err(|e| e.to_string())?;
        conn.execute("UPDATE cron_jobs SET last_run = datetime('now') WHERE id = ?1", rusqlite::params![job_id]).map_err(|e| e.to_string())?;
    }
    result
}

async fn run_email_sync(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let token = match auth.get_access_token() { Some(t) => t, None => return Ok("Skipped: not authenticated".to_string()) };
    match gmail::fetch_inbox(&token, 20).await {
        Ok(msgs) => { let c = msgs.len(); gmail::save_to_db(db, &msgs)?; Ok(format!("Synced {} emails", c)) }
        Err(ref e) if e == "UNAUTHORIZED" => {
            auth.refresh_access_token().await?;
            let t = auth.get_access_token().ok_or("No token after refresh")?;
            let msgs = gmail::fetch_inbox(&t, 20).await?;
            let c = msgs.len(); gmail::save_to_db(db, &msgs)?;
            Ok(format!("Synced {} emails (refreshed)", c))
        }
        Err(e) => Err(e),
    }
}

async fn run_calendar_sync(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let token = match auth.get_access_token() { Some(t) => t, None => return Ok("Skipped: not authenticated".to_string()) };
    let now = chrono::Utc::now();
    let time_min = now.to_rfc3339();
    let time_max = (now + chrono::TimeDelta::days(7)).to_rfc3339();
    match calendar::fetch_events(&token, &time_min, &time_max).await {
        Ok(evts) => { let c = evts.len(); calendar::save_to_db(db, &evts)?; Ok(format!("Synced {} events", c)) }
        Err(ref e) if e == "UNAUTHORIZED" => {
            auth.refresh_access_token().await?;
            let t = auth.get_access_token().ok_or("No token after refresh")?;
            let evts = calendar::fetch_events(&t, &time_min, &time_max).await?;
            let c = evts.len(); calendar::save_to_db(db, &evts)?;
            Ok(format!("Synced {} events (refreshed)", c))
        }
        Err(e) => Err(e),
    }
}

async fn run_notion_sync(db: &Arc<Database>) -> Result<String, String> {
    let token = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM user_preferences WHERE key = 'notion_api_key'", [], |row| row.get::<_, String>(0)).ok()
    };
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => return Ok("Skipped: Notion API key not configured".to_string()),
    };
    let pages = notion::search_pages(&token, None).await?;
    let count = pages.len();
    notion::save_to_db(db, &pages)?;
    Ok(format!("Synced {} Notion pages", count))
}

async fn run_github_digest(db: &Arc<Database>) -> Result<String, String> {
    let token = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM user_preferences WHERE key = 'github_token'", [], |row| row.get::<_, String>(0)).ok()
    };
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => return Ok("Skipped: GitHub token not configured".to_string()),
    };
    let items = github::fetch_assigned_items(&token).await?;
    let count = items.len();
    github::save_to_db(db, &items)?;
    Ok(format!("Synced {} GitHub items", count))
}

async fn run_deadline_monitor(db: &Arc<Database>) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, title, deadline FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline <= date('now', '+3 days') ORDER BY deadline ASC"
    ).map_err(|e| e.to_string())?;
    let warnings: Vec<(i64, String, String)> = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    if warnings.is_empty() { return Ok("No upcoming deadlines".to_string()); }
    for (id, title, deadline) in &warnings {
        log::warn!("Deadline approaching: '{}' (id={}) due {}", title, id, deadline);
    }
    Ok(format!("{} tasks with deadlines within 3 days", warnings.len()))
}

async fn run_proactive_check(db: &Arc<Database>) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut alerts = Vec::new();

    let overdue: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline < date('now')",
        [], |r| r.get(0)
    ).unwrap_or(0);
    if overdue > 0 { alerts.push(format!("{} overdue task(s)", overdue)); }

    let due_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline = date('now')",
        [], |r| r.get(0)
    ).unwrap_or(0);
    if due_today > 0 { alerts.push(format!("{} task(s) due today", due_today)); }

    let upcoming: Option<String> = conn.query_row(
        "SELECT summary FROM calendar_events WHERE start_time > datetime('now') AND start_time <= datetime('now', '+15 minutes') ORDER BY start_time ASC LIMIT 1",
        [], |row| row.get(0)
    ).ok();
    if let Some(meeting) = upcoming { alerts.push(format!("Meeting in 15 min: {}", meeting)); }

    let failed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cron_runs WHERE status = 'failed' AND started_at > datetime('now', '-1 hour')",
        [], |r| r.get(0)
    ).unwrap_or(0);
    if failed > 0 { alerts.push(format!("{} cron job(s) failed in the last hour", failed)); }

    if alerts.is_empty() {
        Ok("No alerts".to_string())
    } else {
        let alert_text = alerts.join("; ");
        conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES ('last_alerts', ?1, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
            rusqlite::params![alert_text],
        ).map_err(|e| e.to_string())?;
        Ok(format!("Alerts: {}", alert_text))
    }
}

async fn run_auto_archive(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let token = match auth.get_access_token() {
        Some(t) => t,
        None => return Ok("Skipped: not authenticated".to_string()),
    };

    // Get active rules (senders to auto-archive)
    let active_senders: Vec<String> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT sender FROM email_rules WHERE rule_status = 'active'")
            .map_err(|e| e.to_string())?;
        let result = stmt.query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        result
    };

    if active_senders.is_empty() {
        return Ok("No active archive rules".to_string());
    }

    // Find unarchived emails from those senders
    let to_archive: Vec<String> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let placeholders: Vec<String> = active_senders.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT gmail_id FROM emails WHERE sender IN ({}) AND labels LIKE '%INBOX%'",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let params: Vec<&dyn rusqlite::types::ToSql> = active_senders.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let result = stmt.query_map(params.as_slice(), |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        result
    };

    let mut archived = 0;
    for gmail_id in &to_archive {
        match gmail::archive_message(&token, gmail_id).await {
            Ok(_) => archived += 1,
            Err(e) => log::warn!("Failed to auto-archive {}: {}", gmail_id, e),
        }
    }

    Ok(format!("Auto-archived {} emails from {} rules", archived, active_senders.len()))
}
