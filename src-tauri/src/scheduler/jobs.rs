use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::{calendar, gmail, notion};
use std::sync::Arc;

/// cron_runs is append-only otherwise: the seeded five-minute jobs add a row on
/// every tick, including ticks that skip immediately, and nothing else prunes
/// them. Keep a bounded history per job. Shared with the test below.
const CRON_RUNS_RETENTION_QUERY: &str = "DELETE FROM cron_runs WHERE job_id = ?1 AND id NOT IN \
     (SELECT id FROM cron_runs WHERE job_id = ?1 ORDER BY id DESC LIMIT 200)";

/// A cron_runs row still marked 'running' when the process starts cannot be
/// running -- the process that owned it is gone. Nothing else ever resolved
/// those rows, so quitting JARVIS mid-run left a permanently stuck entry in the
/// dashboard; the retention sweep above only evicted it after 200 later runs.
/// The reason goes in `error`, which CronTimeline already renders.
///
/// The status is 'interrupted' rather than 'failed' because the two are not the
/// same event and one of them is user-facing: FAILED_RUNS_ALERT_QUERY below
/// counts recent 'failed' runs and pushes an alert, so resolving these to
/// 'failed' turned an ordinary Cmd+Q during a five-minute sync into
/// "1 cron job(s) failed in the last hour" on the next launch. Nothing the job
/// did failed -- the process went away. `started_at` is deliberately left as it
/// was, so the dashboard still shows when the run began. Shared with the tests
/// below.
const RECONCILE_INTERRUPTED_RUNS_QUERY: &str = "UPDATE cron_runs SET status = 'interrupted', \
     finished_at = datetime('now'), \
     error = 'Interrupted: JARVIS exited while this run was in flight' \
     WHERE status = 'running'";

/// Resolve runs orphaned by an unclean exit. Idempotent, and safe only before
/// the first run of this process has inserted its own 'running' row.
pub fn reconcile_interrupted_runs(db: &Arc<Database>) -> Result<usize, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(RECONCILE_INTERRUPTED_RUNS_QUERY, []).map_err(|e| e.to_string())
}

pub async fn run_job(db: &Arc<Database>, auth: &Arc<GoogleAuth>, action_type: &str, job_id: i64) -> Result<String, String> {
    // Once per process, before this run inserts its own row, so a live run is
    // never mistaken for an orphan.
    static RECONCILE_ONCE: std::sync::Once = std::sync::Once::new();
    RECONCILE_ONCE.call_once(|| match reconcile_interrupted_runs(db) {
        Ok(0) => {}
        Ok(n) => log::info!("Resolved {} cron run(s) interrupted by an unclean exit", n),
        Err(e) => log::warn!("Could not reconcile interrupted cron runs: {}", e),
    });

    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        // The scheduler registers jobs once at startup. Re-check the job's
        // current status here so a pause or delete the user made in the
        // dashboard takes effect immediately instead of only after a restart.
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM cron_jobs WHERE id = ?1",
                rusqlite::params![job_id],
                |row| row.get(0),
            )
            .ok();
        match status.as_deref() {
            Some("active") => {}
            _ => return Ok("Skipped: job paused or removed".to_string()),
        }
        conn.execute("INSERT INTO cron_runs (job_id, status) VALUES (?1, 'running')", rusqlite::params![job_id]).map_err(|e| e.to_string())?;
    }

    let result = match action_type {
        "email_sync" => run_email_sync(db, auth).await,
        "mail_workspace_sync" => run_mail_workspace_sync(db).await,
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
        conn.execute(CRON_RUNS_RETENTION_QUERY, rusqlite::params![job_id]).map_err(|e| e.to_string())?;
    }
    result
}

/// The main account's token, or why the job does nothing. A grant Google
/// refused is a failed run the Cron page shows, not a quiet "skipped".
async fn main_token(auth: &Arc<GoogleAuth>) -> Result<Option<String>, String> {
    match auth.ensure_access_token().await {
        Ok(token) => Ok(Some(token)),
        Err(_) if auth.needs_reconnect() => Err(crate::auth::google::REVOKED.to_string()),
        Err(_) => Ok(None),
    }
}

pub(crate) async fn run_email_sync(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let Some(token) = main_token(auth).await? else {
        return Ok("Skipped: not authenticated".to_string());
    };
    crate::mail::additional::remember_main_email(db, &token).await;
    // The detailed form: `save_to_db` reconciles the local mirror against the
    // batch, and a batch missing messages to failed detail fetches must not
    // archive mail that is still in the inbox.
    let (token, summary) = match gmail::fetch_inbox_detailed(&token, 20).await {
        Ok(f) => { let c = f.messages.len(); gmail::save_to_db(db, &f)?; (token, format!("Synced {} emails", c)) }
        Err(ref e) if e == "UNAUTHORIZED" => {
            auth.refresh_access_token().await?;
            let t = auth.get_access_token().ok_or("No token after refresh")?;
            let f = gmail::fetch_inbox_detailed(&t, 20).await?;
            let c = f.messages.len(); gmail::save_to_db(db, &f)?;
            (t, format!("Synced {} emails (refreshed)", c))
        }
        Err(e) => return Err(e),
    };
    // Older mail read elsewhere is outside the newest 20; settle its read
    // state too. Best effort: the sync above already succeeded.
    match gmail::unread_inbox_ids(&token).await {
        Ok(Some(unread)) => { gmail::mark_read_except(db, &unread)?; }
        Ok(None) => {}
        Err(e) => log::warn!("[gmail] unread reconciliation skipped: {}", e),
    }
    Ok(summary)
}

/// Triage runs per background tick, 100 messages each: the same bound as a
/// manual sync in the mail page.
const BACKGROUND_TRIAGE_RUNS: usize = 3;

/// Download new mail for every connected workspace mailbox, then sort it if
/// the user turned AI sorting on. Separate from `email_sync`, which fills the
/// assistant's inbox cache through the main Google connection.
pub(crate) async fn run_mail_workspace_sync(db: &Arc<Database>) -> Result<String, String> {
    let Some((saved, problems)) = crate::mail::sync::sync_connected(db).await? else {
        return Ok("Skipped: a mail operation is in progress".to_string());
    };
    let mut summary = format!("Saved {} new emails", saved);
    if saved > 0 && crate::commands::mail::triage_ready(db) {
        let language = if crate::ai::tools::read_language_pref(db) == "en" { "en" } else { "zh-HK" };
        let mut sorted = 0;
        for _ in 0..BACKGROUND_TRIAGE_RUNS {
            match crate::commands::mail::triage_untriaged(db, Some(language)).await {
                Ok(run) => {
                    sorted += run.triaged;
                    // A failing message stays untriaged and would be sent
                    // and billed again by the next run; stop here instead.
                    if run.remaining == 0 || run.triaged == 0 || run.failed > 0 {
                        break;
                    }
                }
                Err(error) => {
                    summary.push_str(&format!("; sorting failed: {}", error));
                    break;
                }
            }
        }
        summary.push_str(&format!("; sorted {}", sorted));
    }
    if problems.is_empty() {
        Ok(summary)
    } else if saved == 0 {
        Err(problems.join(" "))
    } else {
        Ok(format!("{}; {}", summary, problems.join(" ")))
    }
}

/// The main account's calendar, then each additional account's (read-only).
/// One failing does not stop the others.
pub(crate) async fn run_calendar_sync(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let now = chrono::Utc::now();
    let time_min = now.to_rfc3339();
    let time_max = (now + chrono::TimeDelta::days(7)).to_rfc3339();
    let main = sync_main_calendar(db, auth, &time_min, &time_max).await;
    let (others, problems) = crate::mail::additional::sync_calendars(db, &time_min, &time_max).await?;
    let mut summary = match main {
        Ok(summary) => summary,
        Err(e) if others == 0 && problems.is_empty() => return Err(e),
        Err(e) => format!("Main calendar failed: {}", e),
    };
    if others > 0 {
        summary.push_str(&format!("; {} from other accounts", others));
    }
    if !problems.is_empty() {
        summary.push_str(&format!("; {}", problems.join(" ")));
    }
    Ok(summary)
}

async fn sync_main_calendar(db: &Arc<Database>, auth: &Arc<GoogleAuth>, time_min: &str, time_max: &str) -> Result<String, String> {
    use calendar::MAIN_ACCOUNT;
    let Some(token) = main_token(auth).await? else {
        return Ok("Skipped: not authenticated".to_string());
    };
    // Before the additional calendars sync, so a mailbox on the main account
    // is not read a second time.
    crate::mail::additional::remember_main_email(db, &token).await;
    // The detailed form: `save_to_db` reconciles the local mirror against the
    // window, so it has to know whether the fetch actually reached the end.
    match calendar::fetch_events_detailed(&token, time_min, time_max).await {
        Ok(f) => { let c = f.events.len(); calendar::save_to_db(db, &f, time_min, time_max, MAIN_ACCOUNT)?; Ok(format!("Synced {} events", c)) }
        Err(ref e) if e == "UNAUTHORIZED" => {
            auth.refresh_access_token().await?;
            let t = auth.get_access_token().ok_or("No token after refresh")?;
            let f = calendar::fetch_events_detailed(&t, time_min, time_max).await?;
            let c = f.events.len(); calendar::save_to_db(db, &f, time_min, time_max, MAIN_ACCOUNT)?;
            Ok(format!("Synced {} events (refreshed)", c))
        }
        Err(e) => Err(e),
    }
}

async fn run_notion_sync(db: &Arc<Database>) -> Result<String, String> {
    if !notion::is_configured(db)? { return Ok("已略過：尚未連接 Notion。".into()); }
    notion::sync_connected(db).await
}

async fn run_github_digest(db: &Arc<Database>) -> Result<String, String> {
    // The detailed form: `save_to_db` only reconciles the halves of the fetch
    // that actually succeeded, and the warning is what tells it which.
    let fetched = match crate::integrations::github_auth::sync_connected(db).await {
        Ok(fetched) => fetched,
        // Same as the Google syncs: an integration the user never connected is
        // skipped, not failed. As a failure it tripped the "jobs failed" alert
        // every hour for anyone who does not use GitHub.
        Err(error) if error.starts_with("GitHub 尚未連接") => return Ok("Skipped: GitHub not connected".to_string()),
        Err(error) => return Err(error),
    };
    let count = fetched.items.len();
    Ok(format!("Synced {} GitHub items", count))
}

/// tasks.deadline is a plain local calendar date ("YYYY-MM-DD", see the
/// create_task tool schema in ai/tools.rs), so only the 'now' side takes the
/// modifier. date('now') on its own is UTC: for a UTC+8 user, between 00:00 and
/// 08:00 local the scheduler called today's tasks neither due nor overdue while
/// assistant::context called them due today, and yesterday's the reverse. Same
/// three definitions as assistant::context and commands::dashboard. Shared with
/// the tests below.
const DEADLINE_WINDOW_QUERY: &str = "SELECT id, title, deadline FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline <= date('now', 'localtime', '+3 days') ORDER BY deadline ASC";
const OVERDUE_TASKS_QUERY: &str = "SELECT title FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline < date('now', 'localtime') ORDER BY deadline ASC, id ASC";
const DUE_TODAY_QUERY: &str = "SELECT title FROM tasks WHERE status != 'completed' AND deadline = date('now', 'localtime') ORDER BY id ASC";

async fn run_deadline_monitor(db: &Arc<Database>) -> Result<String, String> {
    let warnings: Vec<(i64, String, String)> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(DEADLINE_WINDOW_QUERY).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    if warnings.is_empty() { return Ok("No upcoming deadlines".to_string()); }
    for (id, title, deadline) in &warnings {
        log::warn!("Deadline approaching: '{}' (id={}) due {}", title, id, deadline);
    }
    Ok(format!("{} tasks with deadlines within 3 days", warnings.len()))
}

/// calendar_events.start_time holds Google's raw RFC3339 value
/// ('2026-07-27T14:30:00+08:00'), which is TEXT with a 'T' where
/// datetime('now') has a space. Compared raw, SQLite does a byte compare and
/// 'T' > ' ', so the upper bound can never be satisfied and the reminder never
/// fires. Both sides are wrapped in datetime() so they are parsed instead.
/// Shared with the test below so it cannot drift back to a string compare.
const UPCOMING_MEETING_QUERY: &str = "SELECT summary, description, attendees FROM calendar_events WHERE datetime(start_time) > datetime('now') AND datetime(start_time) <= datetime('now', '+15 minutes') ORDER BY datetime(start_time) ASC LIMIT 1";

/// What raises the "cron job(s) failed" alert the user actually sees. Only
/// genuine failures count: a run resolved as 'interrupted' by
/// RECONCILE_INTERRUPTED_RUNS_QUERY means JARVIS was quit mid-run, which is not
/// something to alert anybody about. Shared with the test below.
const FAILED_RUNS_ALERT_QUERY: &str = "SELECT COUNT(*) FROM cron_runs WHERE status = 'failed' AND started_at > datetime('now', '-1 hour')";

/// NotificationBanner renders whatever user_preferences.last_alerts holds, and
/// the check only ever wrote the key. A check that finds nothing has to remove
/// it, otherwise an alert from hours ago stays on screen until the user
/// dismisses it by hand. Shared with the test below.
const CLEAR_ALERTS_QUERY: &str = "DELETE FROM user_preferences WHERE key = 'last_alerts'";

/// Alerts are shown to the user as macOS notifications, so they follow the
/// interface language the frontend stores in user_preferences.ui_locale.
#[derive(Clone, Copy, Debug, PartialEq)]
enum AlertLocale { En, ZhHk }

impl AlertLocale {
    fn from_preference(value: Option<&str>) -> Self {
        if value == Some("zh-HK") { AlertLocale::ZhHk } else { AlertLocale::En }
    }
}

#[derive(Clone, Copy)]
enum TaskAlert { Overdue, DueToday }

/// A notification body is truncated by macOS well before this many names.
const ALERT_TASK_NAMES: usize = 3;

/// last_alerts holds one alert per line, so every alert has to be one line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn task_alert(locale: AlertLocale, kind: TaskAlert, titles: &[String]) -> Option<String> {
    if titles.is_empty() { return None; }
    let shown: Vec<String> = titles.iter().take(ALERT_TASK_NAMES).map(|t| one_line(t)).collect();
    let more = titles.len() - shown.len();
    Some(match locale {
        AlertLocale::En => {
            let label = match kind { TaskAlert::Overdue => "Overdue", TaskAlert::DueToday => "Due today" };
            let extra = if more > 0 { format!(" and {more} more") } else { String::new() };
            format!("{label}: {}{extra}", shown.join(", "))
        }
        AlertLocale::ZhHk => {
            let label = match kind { TaskAlert::Overdue => "已逾期", TaskAlert::DueToday => "今天到期" };
            let extra = if more > 0 { format!(" 及另外 {more} 項") } else { String::new() };
            format!("{label}：{}{extra}", shown.join("、"))
        }
    })
}

/// The alert lines the previous check had not already stored. The check runs
/// every five minutes, so notifying on every line would repeat a reminder
/// until the task is done.
fn alerts_to_notify<'a>(previous: Option<&str>, current: &'a [String]) -> Vec<&'a str> {
    let seen: std::collections::HashSet<&str> = previous.unwrap_or("").lines().collect();
    current.iter().map(String::as_str).filter(|line| !seen.contains(line)).collect()
}

async fn run_proactive_check(db: &Arc<Database>) -> Result<String, String> {
    // Each query acquires and releases the lock independently to avoid blocking
    // other jobs and UI commands during the check.
    let mut alerts = Vec::new();

    let (locale, previous): (AlertLocale, Option<String>) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let pref = |key: &str| conn
            .query_row("SELECT value FROM user_preferences WHERE key = ?1", [key], |r| r.get::<_, String>(0))
            .ok();
        (AlertLocale::from_preference(pref("ui_locale").as_deref()), pref("last_alerts"))
    };
    let zh = locale == AlertLocale::ZhHk;

    for (query, kind) in [(OVERDUE_TASKS_QUERY, TaskAlert::Overdue), (DUE_TODAY_QUERY, TaskAlert::DueToday)] {
        let titles: Vec<String> = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };
        alerts.extend(task_alert(locale, kind, &titles));
    }

    let upcoming_meeting: Option<(String, Option<String>, Option<String>)> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            UPCOMING_MEETING_QUERY,
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        ).ok()
    };
    if let Some((summary, description, attendees)) = upcoming_meeting {
        let mut prep = if zh { format!("15 分鐘後會議：{}", summary) } else { format!("Meeting in 15 min: {}", summary) };
        if let Some(desc) = description { if !desc.is_empty() { prep.push_str(&format!(" -- {}", desc)); } }
        if let Some(att) = attendees {
            if !att.is_empty() { prep.push_str(&if zh { format!(" [參與者：{}]", att) } else { format!(" [with: {}]", att) }); }
        }
        let related_tasks: Vec<String> = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare(
                "SELECT title FROM tasks WHERE status != 'completed' AND (title LIKE ?1 OR description LIKE ?1) LIMIT 3"
            ).map_err(|e| e.to_string())?;
            let pattern = format!("%{}%", summary.split_whitespace().next().unwrap_or(&summary));
            let rows = stmt.query_map(rusqlite::params![pattern], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        if !related_tasks.is_empty() {
            let label = if zh { "相關工作：" } else { "Related tasks: " };
            prep.push_str(&format!(" | {}{}", label, related_tasks.join(", ")));
        }
        alerts.push(one_line(&prep));
    }

    let failed: i64 = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(FAILED_RUNS_ALERT_QUERY, [], |r| r.get(0)).unwrap_or(0)
    };
    if failed > 0 {
        // No count in the line: the digest re-notifies whenever a line
        // changes, so "2 failed", "4 failed"... chimed every 5 minutes while
        // the Mac was offline.
        alerts.push(if zh { "過去一小時有排程執行失敗，請查看「自動排程」".to_string() } else { "Scheduled jobs failed in the last hour; see Automations".to_string() });
    }

    // Email follow-ups the user asked to be reminded about. Each fires once,
    // at its own time, independent of the digest lines above.
    let now_local = chrono::Local::now().format("%Y-%m-%dT%H:%M").to_string();
    for item in crate::mail::store::due_reminders(db, &now_local).unwrap_or_default() {
        let when = match (item.due_date.as_deref(), item.due_time.as_deref()) {
            (Some(date), Some(time)) => format!("{date} {time}"),
            (Some(date), None) => date.to_string(),
            _ => String::new(),
        };
        let body = match (zh, when.is_empty()) {
            (true, false) => format!("{}（{}）", item.title, when),
            (false, false) => format!("{} ({})", item.title, when),
            _ => item.title.clone(),
        };
        // Marked first, like last_alerts: a failed post is not retried forever.
        if crate::mail::store::mark_reminded(db, item.id).is_ok() {
            let title = if zh { "JARVIS 電郵提醒" } else { "JARVIS reminder" };
            if let Err(e) = crate::system::control::send_notification(title, &one_line(&body), true).await {
                log::warn!("Mail reminder could not post a notification: {}", e);
            }
        }
    }

    if alerts.is_empty() {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(CLEAR_ALERTS_QUERY, []).map_err(|e| e.to_string())?;
        Ok("No alerts".to_string())
    } else {
        let alert_text = alerts.join("\n");
        {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO user_preferences (key, value, updated_at) VALUES ('last_alerts', ?1, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
                rusqlite::params![alert_text],
            ).map_err(|e| e.to_string())?;
        }
        // Stored first, so a notification that fails is not retried every tick.
        for line in alerts_to_notify(previous.as_deref(), &alerts) {
            if let Err(e) = crate::system::control::send_notification("JARVIS", line, true).await {
                log::warn!("Proactive check could not post a notification: {}", e);
            }
        }
        Ok(format!("Alerts: {}", alerts.join("; ")))
    }
}

/// Rows already archived must not be selected again -- fetch_inbox only ever
/// returns INBOX messages, so the local `labels` value of an archived message is
/// frozen with INBOX in it forever and cannot narrow the set on its own.
/// Built here so the test below exercises the predicate the job actually runs.
fn auto_archive_select_sql(sender_count: usize) -> String {
    let placeholders: Vec<String> = (1..=sender_count).map(|i| format!("?{}", i)).collect();
    format!(
        "SELECT gmail_id FROM emails WHERE sender IN ({}) AND labels LIKE '%INBOX%' AND is_archived = 0",
        placeholders.join(",")
    )
}

/// Refresh the Google token and retry one archive call, mirroring the
/// refresh-and-retry arm run_email_sync and run_calendar_sync already have.
/// `gmail::archive_message` returns the "UNAUTHORIZED" sentinel on a 401 and
/// the archive loop consumed nothing, so a revoked grant was reported as a
/// completed run that archived zero mail. Returns the refreshed token.
async fn refresh_and_archive(auth: &Arc<GoogleAuth>, gmail_id: &str) -> Result<String, String> {
    auth.refresh_access_token().await?;
    let token = auth.get_access_token().ok_or("No token after refresh")?;
    gmail::archive_message(&token, gmail_id).await?;
    Ok(token)
}

async fn run_auto_archive(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let Some(mut token) = main_token(auth).await? else {
        return Ok("Skipped: not authenticated".to_string());
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
        let sql = auto_archive_select_sql(active_senders.len());
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let params: Vec<&dyn rusqlite::types::ToSql> = active_senders.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let result = stmt.query_map(params.as_slice(), |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        result
    };

    let mut archived: Vec<&str> = Vec::new();
    let mut refreshed = false;
    // A 401 that survives one refresh means the grant is gone, not the token.
    // Recorded rather than returned immediately so the messages already
    // archived at Gmail are still mirrored locally below.
    let mut auth_error: Option<String> = None;
    for gmail_id in &to_archive {
        // Bound to a local so the borrow of `token` ends before an arm can
        // replace it with a refreshed one.
        let outcome = gmail::archive_message(&token, gmail_id).await;
        match outcome {
            Ok(_) => archived.push(gmail_id.as_str()),
            Err(ref e) if e == "UNAUTHORIZED" && !refreshed => {
                refreshed = true;
                match refresh_and_archive(auth, gmail_id).await {
                    Ok(t) => { token = t; archived.push(gmail_id.as_str()); }
                    Err(e) => { auth_error = Some(e); break; }
                }
            }
            Err(ref e) if e == "UNAUTHORIZED" => { auth_error = Some(e.clone()); break; }
            Err(e) => log::warn!("Failed to auto-archive {}: {}", gmail_id, e),
        }
    }

    // Mirror the archive locally, the way commands::email::archive_email does
    // for the manual path. Without this the rows stay visible in the app and
    // the next tick re-sends the same messages to Gmail. One transaction after
    // the loop rather than one statement per message.
    if !archived.is_empty() {
        let mut conn = db.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut stmt = tx.prepare("UPDATE emails SET is_archived = 1 WHERE gmail_id = ?1").map_err(|e| e.to_string())?;
            for gmail_id in &archived {
                stmt.execute(rusqlite::params![gmail_id]).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
    }

    if let Some(e) = auth_error {
        return Err(e);
    }

    Ok(format!("Auto-archived {} emails from {} rules", archived.len(), active_senders.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// The reminder window must compare instants, not raw text. Before the fix
    /// the upper bound was unsatisfiable for every timed event, so the
    /// "Meeting in 15 min" alert never fired.
    #[test]
    fn meeting_reminder_window_matches_rfc3339_events() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE calendar_events (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 google_id TEXT UNIQUE NOT NULL,
                 summary TEXT NOT NULL,
                 description TEXT,
                 location TEXT,
                 start_time TEXT NOT NULL,
                 end_time TEXT NOT NULL,
                 attendees TEXT,
                 status TEXT DEFAULT 'confirmed');
             INSERT INTO calendar_events (google_id, summary, start_time, end_time)
                 VALUES ('soon', 'Mentor sync', '2026-07-27T14:40:00+08:00', '2026-07-27T15:00:00+08:00'),
                        ('past', 'Standup', '2026-07-27T09:00:00+08:00', '2026-07-27T09:30:00+08:00');",
        )
        .unwrap();

        // Pin the clock to 2026-07-27 06:30:00 UTC (14:30 in HK) so the
        // assertions do not depend on when the test runs.
        let pinned = UPCOMING_MEETING_QUERY.replace("'now'", "'2026-07-27 06:30:00'");
        let found: Option<String> = conn.query_row(&pinned, [], |r| r.get(0)).ok();
        assert_eq!(found.as_deref(), Some("Mentor sync"), "a meeting 10 minutes out must be found");

        // The pre-fix predicate compared the stored TEXT directly, where
        // 'T' > ' ', so no timed event could ever satisfy the upper bound.
        let old = "SELECT summary FROM calendar_events \
                   WHERE start_time > datetime('2026-07-27 06:30:00') \
                   AND start_time <= datetime('2026-07-27 06:30:00', '+15 minutes') \
                   ORDER BY start_time ASC LIMIT 1";
        let old_found: Option<String> = conn.query_row(old, [], |r| r.get(0)).ok();
        assert_eq!(old_found, None, "the raw TEXT comparison never matched a timed event");
    }

    /// A message already archived must not be re-selected, otherwise every tick
    /// re-sends the same ids to Gmail and the rule looks inert.
    #[test]
    fn auto_archive_skips_rows_already_archived() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE emails (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 gmail_id TEXT UNIQUE NOT NULL,
                 sender TEXT NOT NULL,
                 labels TEXT,
                 received_at TEXT NOT NULL,
                 is_archived INTEGER NOT NULL DEFAULT 0);
             INSERT INTO emails (gmail_id, sender, labels, received_at, is_archived)
                 VALUES ('new', 'news@example.com', 'INBOX,UNREAD', '2026-07-27', 0),
                        ('done', 'news@example.com', 'INBOX,UNREAD', '2026-07-26', 1);",
        )
        .unwrap();

        let sql = auto_archive_select_sql(1);
        let mut stmt = conn.prepare(&sql).unwrap();
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params!["news@example.com"], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(ids, vec!["new".to_string()], "already-archived mail must not be sent to Gmail again");
    }

    fn cron_runs_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cron_runs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 job_id INTEGER NOT NULL,
                 started_at TEXT NOT NULL DEFAULT (datetime('now')),
                 finished_at TEXT,
                 status TEXT NOT NULL DEFAULT 'running',
                 result TEXT,
                 error TEXT);",
        )
        .unwrap();
        conn
    }

    fn tasks_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 title TEXT NOT NULL,
                 description TEXT,
                 deadline TEXT,
                 priority INTEGER DEFAULT 2,
                 status TEXT DEFAULT 'pending');
             INSERT INTO tasks (title, deadline, status) VALUES
                 ('File HKSTP milestone', '2026-07-27', 'pending'),
                 ('Call mentor', '2026-07-27', 'pending'),
                 ('Yesterday invoice', '2026-07-26', 'pending'),
                 ('Older chase-up', '2026-07-20', 'pending'),
                 ('Board deck', '2026-07-30', 'pending'),
                 ('Already sent', '2026-07-27', 'completed');",
        )
        .unwrap();
        conn
    }

    /// The scheduler's idea of "today" has to be the same local day
    /// assistant::context and commands::dashboard use, otherwise the proactive
    /// alert and the briefing report different overdue/due-today counts for the
    /// same tasks during the local morning.
    #[test]
    fn scheduler_task_deadlines_follow_the_local_day() {
        let conn = tasks_db();

        // 'localtime' resolves against whatever machine runs the test, so pin
        // it to +08:00. Clock: 07:00 HK on 2026-07-27, still 07-26 in UTC.
        let pin = |q: &str| {
            q.replace("'localtime'", "'+8 hours'")
                .replace("'now'", "'2026-07-26 23:00:00'")
        };

        let titles_for = |q: &str| -> Vec<String> {
            let mut stmt = conn.prepare(&pin(q)).unwrap();
            stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|r| r.unwrap()).collect()
        };
        assert_eq!(
            titles_for(OVERDUE_TASKS_QUERY),
            vec!["Older chase-up".to_string(), "Yesterday invoice".to_string()],
            "yesterday's and older open tasks are overdue, oldest first"
        );
        assert_eq!(
            titles_for(DUE_TODAY_QUERY),
            vec!["File HKSTP milestone".to_string(), "Call mentor".to_string()],
            "tasks dated today must count as due today"
        );

        let mut stmt = conn.prepare(&pin(DEADLINE_WINDOW_QUERY)).unwrap();
        let titles: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            titles.contains(&"Board deck".to_string()),
            "the 3-day window is measured from the local day"
        );

        // The pre-fix predicates used the UTC day, which was still 2026-07-26:
        // one overdue and one due today instead of two and two.
        let utc_overdue: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status != 'completed' \
                 AND deadline IS NOT NULL AND deadline < date('2026-07-26 23:00:00')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(utc_overdue, 1);
        let utc_due_today: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status != 'completed' \
                 AND deadline = date('2026-07-26 23:00:00')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(utc_due_today, 1);
    }

    /// A row left at 'running' by an unclean exit must be resolved at startup.
    /// Nothing else ever closes it, so it rendered as a permanently stuck run.
    #[test]
    fn interrupted_runs_are_resolved_not_left_running() {
        let conn = cron_runs_db();
        conn.execute_batch(
            "INSERT INTO cron_runs (job_id, status, finished_at, result) VALUES
                 (1, 'completed', datetime('now'), 'Synced 3 emails');
             INSERT INTO cron_runs (job_id, status) VALUES (1, 'running');
             INSERT INTO cron_runs (job_id, status) VALUES (2, 'running');
             INSERT INTO cron_runs (job_id, status, finished_at, error) VALUES
                 (2, 'failed', datetime('now'), 'Gmail API error: 500');",
        )
        .unwrap();

        let resolved = conn.execute(RECONCILE_INTERRUPTED_RUNS_QUERY, []).unwrap();
        assert_eq!(resolved, 2, "both orphans are resolved, across jobs");

        let still_running: i64 = conn
            .query_row("SELECT COUNT(*) FROM cron_runs WHERE status = 'running'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(still_running, 0);

        let unfinished: i64 = conn
            .query_row("SELECT COUNT(*) FROM cron_runs WHERE finished_at IS NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(unfinished, 0, "a resolved run has an end time");

        let reason: String = conn
            .query_row("SELECT error FROM cron_runs WHERE id = 2", [], |r| r.get(0))
            .unwrap();
        assert!(reason.starts_with("Interrupted:"), "the dashboard has to say why: {}", reason);

        // Already-resolved rows are untouched, and a second pass is a no-op.
        let kept: String = conn
            .query_row("SELECT result FROM cron_runs WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, "Synced 3 emails");
        assert_eq!(conn.execute(RECONCILE_INTERRUPTED_RUNS_QUERY, []).unwrap(), 0);
    }

    /// R2-02: quitting the app during a run must not look like a job failure.
    /// The orphan is still resolved so the dashboard is not stuck on 'running',
    /// but the proactive check alerts only on runs that actually failed --
    /// otherwise every Cmd+Q during the five-minute email sync greeted the user
    /// with "1 cron job(s) failed in the last hour" on the next launch.
    #[test]
    fn an_interrupted_run_does_not_raise_a_failure_alert() {
        let conn = cron_runs_db();
        conn.execute(
            "INSERT INTO cron_runs (job_id, status, started_at) VALUES \
             (1, 'running', datetime('now', '-20 minutes'))",
            [],
        )
        .unwrap();

        assert_eq!(conn.execute(RECONCILE_INTERRUPTED_RUNS_QUERY, []).unwrap(), 1);
        let status: String = conn
            .query_row("SELECT status FROM cron_runs WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "interrupted", "an unclean exit is not a failure");

        let alerted: i64 = conn.query_row(FAILED_RUNS_ALERT_QUERY, [], |r| r.get(0)).unwrap();
        assert_eq!(alerted, 0, "an ordinary quit must not alert the user");

        // A run that genuinely failed inside the same window still does.
        conn.execute(
            "INSERT INTO cron_runs (job_id, status, started_at, finished_at, error) VALUES \
             (1, 'failed', datetime('now', '-10 minutes'), datetime('now'), 'Gmail API error: 500')",
            [],
        )
        .unwrap();
        let alerted: i64 = conn.query_row(FAILED_RUNS_ALERT_QUERY, [], |r| r.get(0)).unwrap();
        assert_eq!(alerted, 1, "a real failure must still surface");
    }

    /// An alert has to say which task it is about: "1 overdue task(s)" gave the
    /// user nothing to act on, and the app has no task list to look it up in.
    #[test]
    fn task_alerts_name_the_tasks_in_the_interface_language() {
        let titles = vec!["Send invoice".to_string(), "Call Brian".to_string()];
        assert_eq!(
            task_alert(AlertLocale::En, TaskAlert::Overdue, &titles).as_deref(),
            Some("Overdue: Send invoice, Call Brian")
        );
        assert_eq!(
            task_alert(AlertLocale::ZhHk, TaskAlert::DueToday, &titles).as_deref(),
            Some("今天到期：Send invoice、Call Brian")
        );
        assert_eq!(task_alert(AlertLocale::En, TaskAlert::Overdue, &[]), None);
    }

    #[test]
    fn task_alerts_cap_the_names_and_stay_on_one_line() {
        let titles: Vec<String> = ["A", "B", "C", "D", "E"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            task_alert(AlertLocale::En, TaskAlert::DueToday, &titles).as_deref(),
            Some("Due today: A, B, C and 2 more")
        );
        assert_eq!(
            task_alert(AlertLocale::ZhHk, TaskAlert::Overdue, &titles).as_deref(),
            Some("已逾期：A、B、C 及另外 2 項")
        );
        // A title with a line break would split one alert into two lines of
        // last_alerts and defeat the comparison in alerts_to_notify.
        let multiline = vec!["Reply to\nlandlord".to_string()];
        assert_eq!(
            task_alert(AlertLocale::En, TaskAlert::Overdue, &multiline).as_deref(),
            Some("Overdue: Reply to landlord")
        );
    }

    /// The check runs every five minutes. Notifying on every alert line would
    /// repeat the same reminder until the task is done, so only lines the
    /// previous check did not already store are sent.
    #[test]
    fn only_alerts_new_since_the_last_check_are_notified() {
        let current = vec!["Overdue: A".to_string(), "Due today: B".to_string()];
        assert_eq!(alerts_to_notify(None, &current), vec!["Overdue: A", "Due today: B"]);
        assert_eq!(alerts_to_notify(Some("Overdue: A\nDue today: B"), &current), Vec::<&str>::new());
        assert_eq!(alerts_to_notify(Some("Overdue: A"), &current), vec!["Due today: B"]);
        // A changed line is a new alert: another task became overdue.
        let grown = vec!["Overdue: A, C".to_string()];
        assert_eq!(alerts_to_notify(Some("Overdue: A"), &grown), vec!["Overdue: A, C"]);
    }

    #[test]
    fn interface_locale_defaults_to_english() {
        assert_eq!(AlertLocale::from_preference(Some("zh-HK")), AlertLocale::ZhHk);
        assert_eq!(AlertLocale::from_preference(Some("en")), AlertLocale::En);
        assert_eq!(AlertLocale::from_preference(None), AlertLocale::En);
    }

    /// A resolved alert must disappear from the banner on the next check.
    #[test]
    fn no_alerts_clears_last_alerts() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE user_preferences (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT (datetime('now')));
             INSERT INTO user_preferences (key, value) VALUES
                 ('last_alerts', '2 overdue task(s)'),
                 ('ai_provider', 'gemini_primary');",
        )
        .unwrap();

        assert_eq!(conn.execute(CLEAR_ALERTS_QUERY, []).unwrap(), 1);

        let stale: i64 = conn
            .query_row("SELECT COUNT(*) FROM user_preferences WHERE key = 'last_alerts'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stale, 0, "a check with no alerts must clear the banner");

        let others: i64 = conn
            .query_row("SELECT COUNT(*) FROM user_preferences", [], |r| r.get(0))
            .unwrap();
        assert_eq!(others, 1, "no other preference may be touched");
    }

    /// Without retention the seeded five-minute jobs grow cron_runs forever.
    #[test]
    fn cron_runs_retention_bounds_the_history() {
        let conn = cron_runs_db();
        for _ in 0..250 {
            conn.execute("INSERT INTO cron_runs (job_id, status) VALUES (1, 'completed')", []).unwrap();
        }
        for _ in 0..5 {
            conn.execute("INSERT INTO cron_runs (job_id, status) VALUES (2, 'completed')", []).unwrap();
        }

        conn.execute(CRON_RUNS_RETENTION_QUERY, rusqlite::params![1i64]).unwrap();

        let kept: i64 = conn.query_row("SELECT COUNT(*) FROM cron_runs WHERE job_id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, 200, "history for the job must be bounded");
        let oldest_kept: i64 = conn.query_row("SELECT MIN(id) FROM cron_runs WHERE job_id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(oldest_kept, 51, "the runs kept must be the most recent ones");
        let other: i64 = conn.query_row("SELECT COUNT(*) FROM cron_runs WHERE job_id = 2", [], |r| r.get(0)).unwrap();
        assert_eq!(other, 5, "other jobs must not be pruned");
    }
}
