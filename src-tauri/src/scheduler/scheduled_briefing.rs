//! The scheduled daily briefing: the `daily_briefing` job.
//!
//! At the time the user chose (08:30 unless changed) the briefing is shown at
//! once from what is saved on this Mac: a notification and a brief on the
//! home page. The AI summary is a second step, because it needs the network.
//! It replaces the local text in place when it arrives; if the Mac is offline
//! that happens when the network returns, with no second notification and no
//! voice.
//!
//! It is spoken only on the live, on-time run, with the Mac awake and the
//! screen unlocked. A briefing that runs late for any reason (sleep, JARVIS
//! closed, a crash) is shown once and never spoken. At most one briefing a
//! day comes from the schedule.

use super::engine::{format_baseline, write_status};
use super::network::{Reachability, SystemReachability};
use super::slots::{parse_schedule, Trigger};
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use chrono::{DateTime, Datelike, FixedOffset, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ACTION_TYPE: &str = "daily_briefing";
pub const DEFAULT_TIME: &str = "08:30";

/// The event the window listens for to redraw the brief in place.
pub const CHANGED_EVENT: &str = "daily-briefing-updated";

const VOICE_KEY: &str = "daily_briefing_voice";
const LAST_DATE_KEY: &str = "daily_briefing_last_date";
const TODAY_KEY: &str = "daily_briefing_today";

/// How long the on-time run waits for the AI summary before speaking the
/// local text instead. The summary still replaces the text when it arrives.
const SUMMARY_WAIT_BEFORE_SPEAKING: Duration = Duration::from_secs(20);

/// After a summary attempt that ended offline, the next one waits this long,
/// however quickly reachability turns positive again.
const SUMMARY_RETRY_PAUSE: Duration = Duration::from_secs(60);

/// Whether anyone can be at the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub locked: bool,
    pub display_asleep: bool,
}

/// Whether this run of the briefing is spoken aloud.
///
/// Only the live, on-time run (`scheduled`) speaks, and only with voice turned
/// on, the screen unlocked and the display awake. A `catch_up` or
/// `retry_interrupted` run never speaks, however little it is late by: a Mac
/// that slept from 08:25 to 08:35 shows the 08:30 briefing at 08:35 in
/// silence. A run the user asked for by hand does not speak either. The
/// display is checked as well as the lock because a Mac can run code with its
/// lid closed (a dark wake), and nobody is there to hear it.
pub fn should_speak(trigger: Trigger, voice_on: bool, screen: Screen) -> bool {
    trigger == Trigger::Scheduled && voice_on && !screen.locked && !screen.display_asleep
}

/// The cron expression for a daily briefing at `time` ("08:30").
pub fn schedule_for_time(time: &str) -> Result<String, String> {
    let invalid = || format!("Briefing time must be HH:MM between 00:00 and 23:59, not '{}'", time);
    let (hour, minute) = time.split_once(':').ok_or_else(invalid)?;
    if hour.len() != 2 || minute.len() != 2 {
        return Err(invalid());
    }
    let hour: u32 = hour.parse().map_err(|_| invalid())?;
    let minute: u32 = minute.parse().map_err(|_| invalid())?;
    if hour > 23 || minute > 59 {
        return Err(invalid());
    }
    Ok(format!("0 {} {} * * *", minute, hour))
}

/// The time of day a briefing schedule stands for, if it is one this module
/// wrote.
pub fn time_from_schedule(schedule: &str) -> Option<String> {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    match fields.as_slice() {
        ["0", minute, hour, "*", "*", "*"] => {
            let (hour, minute): (u32, u32) = (hour.parse().ok()?, minute.parse().ok()?);
            (hour <= 23 && minute <= 59).then(|| format!("{:02}:{:02}", hour, minute))
        }
        _ => None,
    }
}

/// The local part of the briefing: what the database on this Mac already
/// knows. `None` means "never synced", which is not the same as zero.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalBrief {
    date: chrono::NaiveDate,
    /// Today's events as (time, title); the time is empty for an all-day one.
    meetings: Option<Vec<(String, String)>>,
    overdue: Vec<String>,
    due_today: Vec<String>,
    unread: Option<i64>,
}

/// How many meetings and task names the text spells out.
const LISTED: usize = 5;

fn listed(items: &[String], zh: bool) -> String {
    let shown: Vec<&str> = items.iter().take(LISTED).map(String::as_str).collect();
    let more = items.len() - shown.len();
    match (zh, more) {
        (true, 0) => shown.join("、"),
        (true, more) => format!("{} 及另外 {} 項", shown.join("、"), more),
        (false, 0) => shown.join(", "),
        (false, more) => format!("{} and {} more", shown.join(", "), more),
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{} {}", count, if count == 1 { one } else { many })
}

impl LocalBrief {
    pub fn gather(db: &Database, now: DateTime<FixedOffset>) -> Result<LocalBrief, String> {
        use crate::assistant::context::{todays_events_query, HAS_CALENDAR_ROWS_QUERY, HAS_EMAIL_ROWS_QUERY, UNREAD_COUNT_QUERY};
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let titles = |query: &str| -> Result<Vec<String>, String> {
            let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        };
        let synced = |query: &str| conn.query_row(query, [], |row| row.get::<_, bool>(0)).unwrap_or(false);

        let meetings = if synced(HAS_CALENDAR_ROWS_QUERY) {
            let mut stmt = conn.prepare(&todays_events_query()).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?;
            let events = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            Some(
                events
                    .into_iter()
                    .map(|(summary, start)| {
                        // A timed event is an instant; an all-day one is a bare date.
                        let time = DateTime::parse_from_rfc3339(&start)
                            .map(|at| at.with_timezone(now.offset()).format("%H:%M").to_string())
                            .unwrap_or_default();
                        (time, summary)
                    })
                    .collect(),
            )
        } else {
            None
        };
        let unread = if synced(HAS_EMAIL_ROWS_QUERY) {
            Some(conn.query_row(UNREAD_COUNT_QUERY, [], |row| row.get(0)).map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(LocalBrief {
            date: now.date_naive(),
            meetings,
            overdue: titles(super::jobs::OVERDUE_TASKS_QUERY)?,
            due_today: titles(super::jobs::DUE_TODAY_QUERY)?,
            unread,
        })
    }

    fn date_line(&self, zh: bool) -> String {
        if zh {
            let weekday = ["一", "二", "三", "四", "五", "六", "日"][self.date.weekday().num_days_from_monday() as usize];
            format!("{} 月 {} 日（星期{}）", self.date.month(), self.date.day(), weekday)
        } else {
            self.date.format("%A %-d %B").to_string()
        }
    }

    /// The brief as shown on the home page, and as spoken when there is no AI
    /// summary to speak.
    pub fn text(&self, zh: bool) -> String {
        let mut lines = vec![self.date_line(zh)];
        match &self.meetings {
            None => lines.push(if zh { "會議：尚未同步日曆" } else { "Meetings: the calendar has not been synced yet" }.to_string()),
            Some(meetings) if meetings.is_empty() => {
                lines.push(if zh { "會議：今天沒有會議" } else { "Meetings: none today" }.to_string())
            }
            Some(meetings) => {
                lines.push(if zh {
                    format!("會議：今天有 {} 個", meetings.len())
                } else {
                    format!("Meetings: {} today", meetings.len())
                });
                for (time, title) in meetings.iter().take(LISTED) {
                    let time = match (time.is_empty(), zh) {
                        (false, _) => time.as_str(),
                        (true, true) => "全日",
                        (true, false) => "All day",
                    };
                    lines.push(format!("  {}　{}", time, title));
                }
                if meetings.len() > LISTED {
                    let more = meetings.len() - LISTED;
                    lines.push(if zh { format!("  及另外 {} 個", more) } else { format!("  and {} more", more) });
                }
            }
        }
        if self.overdue.is_empty() && self.due_today.is_empty() {
            lines.push(if zh { "工作：沒有已逾期或今天到期的工作" } else { "Tasks: nothing overdue or due today" }.to_string());
        } else {
            lines.push(if zh {
                format!("工作：{} 項已逾期，{} 項今天到期", self.overdue.len(), self.due_today.len())
            } else {
                format!("Tasks: {} overdue, {} due today", self.overdue.len(), self.due_today.len())
            });
            if !self.overdue.is_empty() {
                lines.push(format!("  {}{}", if zh { "已逾期：" } else { "Overdue: " }, listed(&self.overdue, zh)));
            }
            if !self.due_today.is_empty() {
                lines.push(format!("  {}{}", if zh { "今天到期：" } else { "Due today: " }, listed(&self.due_today, zh)));
            }
        }
        lines.push(match (self.unread, zh) {
            (None, true) => "電郵：尚未同步".to_string(),
            (None, false) => "Mail: not synced yet".to_string(),
            (Some(0), true) => "電郵：沒有未讀郵件".to_string(),
            (Some(0), false) => "Mail: nothing unread".to_string(),
            (Some(count), true) => format!("電郵：{} 封未讀", count),
            (Some(count), false) => format!("Mail: {} unread", count),
        });
        lines.join("\n")
    }

    /// One line for the notification.
    pub fn headline(&self, zh: bool) -> String {
        let mut parts = Vec::new();
        let meetings = self.meetings.as_ref().map_or(0, Vec::len);
        if meetings > 0 {
            parts.push(if zh { format!("{} 個會議", meetings) } else { plural(meetings, "meeting", "meetings") });
        }
        if !self.overdue.is_empty() {
            let count = self.overdue.len();
            parts.push(if zh { format!("{} 項工作已逾期", count) } else { plural(count, "overdue task", "overdue tasks") });
        }
        if !self.due_today.is_empty() {
            let count = self.due_today.len();
            parts.push(if zh { format!("{} 項工作今天到期", count) } else { plural(count, "task due today", "tasks due today") });
        }
        if let Some(unread) = self.unread.filter(|count| *count > 0) {
            parts.push(if zh { format!("{} 封未讀電郵", unread) } else { plural(unread as usize, "unread email", "unread emails") });
        }
        match (parts.is_empty(), zh) {
            (true, true) => "今天沒有會議，也沒有已逾期或今天到期的工作。".to_string(),
            (true, false) => "No meetings today, and nothing overdue or due.".to_string(),
            (false, true) => format!("今天：{}。", parts.join("、")),
            (false, false) => format!("Today: {}.", parts.join(", ")),
        }
    }
}

/// The brief as the home page shows it. Kept in `user_preferences`, so it is
/// still there after a relaunch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Brief {
    /// The local day it is for, 'YYYY-MM-DD'.
    pub date: String,
    pub shown_at: String,
    /// The trigger of the run that produced it.
    pub trigger: String,
    /// From what is saved on this Mac.
    pub local: String,
    /// The AI summary, once there is one.
    pub summary: Option<String>,
    /// The AI summary is still owed: the Mac was offline when it was due.
    pub summary_pending: bool,
    /// Why there is no AI summary, when that is not for lack of a network.
    pub summary_error: Option<String>,
}

fn preference(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT value FROM user_preferences WHERE key = ?1", [key], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

fn set_preference(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now')) \
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn save(conn: &Connection, brief: &Brief) -> Result<(), String> {
    set_preference(conn, TODAY_KEY, &serde_json::to_string(brief).map_err(|e| e.to_string())?)
}

/// The stored brief, whatever day it is for.
fn load(conn: &Connection) -> Result<Option<Brief>, String> {
    Ok(preference(conn, TODAY_KEY)?.and_then(|text| serde_json::from_str(&text).ok()))
}

/// Today's brief, if one has been shown today.
pub fn todays_brief(db: &Database, today: &str) -> Result<Option<Brief>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    Ok(load(&conn)?.filter(|brief| brief.date == today))
}

/// Everything the briefing does outside the database, so the rules above can
/// be tested without a network, a screen or a voice.
#[async_trait::async_trait]
pub trait BriefingHost: Send + Sync {
    /// Whether macOS reports a usable network right now.
    fn online(&self) -> bool;
    fn screen(&self) -> Screen;
    /// Ask the AI for the day's summary. Needs the network.
    async fn summarise(&self) -> Result<String, String>;
    async fn notify(&self, title: &str, body: &str);
    async fn speak(&self, text: &str);
    /// The brief changed: redraw it in the window.
    fn changed(&self, brief: &Brief);
}

/// Record how the AI step ended. Returns what the run reports.
fn apply_summary(db: &Database, host: &dyn BriefingHost, brief: &mut Brief, outcome: Result<String, String>) -> Result<String, String> {
    let report = match outcome {
        Ok(summary) => {
            brief.summary = Some(summary);
            brief.summary_pending = false;
            brief.summary_error = None;
            "Briefing shown; AI summary added".to_string()
        }
        // Offline when it failed: still owed, and added when the network is back.
        Err(_) if !host.online() => {
            brief.summary_pending = true;
            "Briefing shown from what is saved on this Mac; the AI summary follows when the network returns".to_string()
        }
        Err(error) => {
            brief.summary_pending = false;
            brief.summary_error = Some(error.clone());
            format!("Briefing shown from what is saved on this Mac; the AI summary failed: {}", error)
        }
    };
    save(&*db.conn.lock().map_err(|e| e.to_string())?, brief)?;
    host.changed(brief);
    Ok(report)
}

/// One run of the briefing job.
pub async fn run_with(db: &Database, host: &dyn BriefingHost, trigger: Trigger, now: DateTime<FixedOffset>) -> Result<String, String> {
    let today = now.format("%Y-%m-%d").to_string();
    let (zh, voice_on, last_date) = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let zh = preference(&conn, "ui_locale")?.as_deref() == Some("zh-HK");
        // The briefing's own switch, and the app-wide one for spoken replies:
        // someone who turned speech off everywhere is not spoken to at 08:30.
        let voice_on = preference(&conn, VOICE_KEY)?.as_deref() != Some("false")
            && preference(&conn, "tts_enabled")?.as_deref() != Some("false");
        (zh, voice_on, preference(&conn, LAST_DATE_KEY)?)
    };
    // At most one briefing a day from the schedule. A briefing the user asks
    // for by hand is always shown, and does not use up the day's scheduled one.
    if trigger != Trigger::Manual && last_date.as_deref() == Some(today.as_str()) {
        return Ok("Already shown today".to_string());
    }

    // Shown at once, from local data, before anything that can wait.
    let local = LocalBrief::gather(db, now)?;
    let mut brief = Brief {
        date: today.clone(),
        shown_at: now.to_rfc3339(),
        trigger: trigger.as_str().to_string(),
        local: local.text(zh),
        summary: None,
        summary_pending: true,
        summary_error: None,
    };
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        save(&conn, &brief)?;
        if trigger != Trigger::Manual {
            set_preference(&conn, LAST_DATE_KEY, &today)?;
        }
    }
    host.changed(&brief);
    if trigger != Trigger::Manual {
        host.notify(if zh { "JARVIS 每日簡報" } else { "JARVIS daily briefing" }, &local.headline(zh)).await;
    }

    let speak = should_speak(trigger, voice_on, host.screen());
    if !host.online() {
        if speak {
            host.speak(&brief.local).await;
        }
        return Ok("Briefing shown from what is saved on this Mac; the AI summary follows when the network returns".to_string());
    }

    let mut summarise = host.summarise();
    match tokio::time::timeout(SUMMARY_WAIT_BEFORE_SPEAKING, &mut summarise).await {
        Ok(outcome) => {
            let report = apply_summary(db, host, &mut brief, outcome)?;
            if speak {
                host.speak(brief.summary.as_deref().unwrap_or(&brief.local)).await;
            }
            Ok(report)
        }
        // The summary is slow. Speak the local text now and let the summary
        // replace the brief in place when it comes.
        Err(_) => {
            let spoken = async {
                if speak {
                    host.speak(&brief.local).await;
                }
            };
            let (_, outcome) = tokio::join!(spoken, &mut summarise);
            apply_summary(db, host, &mut brief, outcome)
        }
    }
}

/// Add the AI summary a briefing is still owed, in place: no notification and
/// no voice. Returns whether the summary is still owed afterwards.
pub async fn complete_pending_with(db: &Database, host: &dyn BriefingHost, today: &str) -> Result<bool, String> {
    let brief = load(&*db.conn.lock().map_err(|e| e.to_string())?)?;
    // Yesterday's brief is no longer on the page; its summary is not owed.
    let Some(mut brief) = brief.filter(|brief| brief.date == today && brief.summary_pending) else {
        return Ok(false);
    };
    let outcome = host.summarise().await;
    apply_summary(db, host, &mut brief, outcome)?;
    Ok(brief.summary_pending)
}

/// The briefing settings as the Settings page shows them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BriefingSettings {
    pub enabled: bool,
    /// "HH:MM", local time.
    pub time: String,
    pub voice: bool,
    /// The app-wide switch for spoken replies. With it off the briefing is
    /// never spoken, whatever `voice` says.
    pub speech_enabled: bool,
}

struct BriefingJob {
    id: i64,
    schedule: String,
    active: bool,
}

fn briefing_job(conn: &Connection) -> Result<Option<BriefingJob>, String> {
    conn.query_row(
        "SELECT id, schedule, status FROM cron_jobs WHERE action_type = ?1 ORDER BY id LIMIT 1",
        [ACTION_TYPE],
        |row| Ok(BriefingJob { id: row.get(0)?, schedule: row.get(1)?, active: row.get::<_, String>(2)? == "active" }),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn read_settings(conn: &Connection) -> Result<BriefingSettings, String> {
    let job = briefing_job(conn)?;
    Ok(BriefingSettings {
        enabled: job.as_ref().is_some_and(|job| job.active),
        time: job.and_then(|job| time_from_schedule(&job.schedule)).unwrap_or_else(|| DEFAULT_TIME.to_string()),
        voice: preference(conn, VOICE_KEY)?.as_deref() != Some("false"),
        speech_enabled: preference(conn, "tts_enabled")?.as_deref() != Some("false"),
    })
}

/// The briefing job after its settings were written: what the scheduler has
/// to register, or unregister.
pub struct SavedBriefingJob {
    pub job_id: i64,
    pub schedule: String,
    pub active: bool,
}

pub const JOB_NAME: &str = "Daily Briefing";
const JOB_DESCRIPTION: &str = "Shows your day at the time you chose, from what is saved on this Mac";

/// Write the briefing settings. Turning the briefing on, or moving its time,
/// resets the job's baseline to now: the first briefing is the next one at the
/// new time, never one whose time has already passed today.
pub fn write_settings(conn: &Connection, enabled: bool, time: &str, voice: bool, now: DateTime<Utc>) -> Result<SavedBriefingJob, String> {
    let schedule = schedule_for_time(time)?;
    parse_schedule(&schedule)?;
    set_preference(conn, VOICE_KEY, if voice { "true" } else { "false" })?;
    let job_id = match briefing_job(conn)? {
        None => {
            conn.execute(
                "INSERT INTO cron_jobs (name, schedule, action_type, status, description, baseline_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![JOB_NAME, schedule, ACTION_TYPE, if enabled { "active" } else { "paused" }, JOB_DESCRIPTION, format_baseline(now)],
            )
            .map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        }
        Some(job) => {
            if job.schedule != schedule {
                conn.execute(
                    "UPDATE cron_jobs SET schedule = ?1, baseline_at = ?2 WHERE id = ?3",
                    params![schedule, format_baseline(now), job.id],
                )
                .map_err(|e| e.to_string())?;
            }
            if job.active != enabled {
                write_status(conn, job.id, enabled, now)?;
            }
            job.id
        }
    };
    Ok(SavedBriefingJob { job_id, schedule, active: enabled })
}

/// The briefing in the running app: the real network, screen, AI and voice.
pub struct BriefingService {
    db: Arc<Database>,
    google_auth: Arc<GoogleAuth>,
    app: tauri::AppHandle,
    /// A summary is owed and waiting for the network.
    pending: AtomicBool,
    /// A summary is being fetched for it now.
    busy: AtomicBool,
    retry_at: Mutex<Option<Instant>>,
}

impl BriefingService {
    pub fn new(db: Arc<Database>, google_auth: Arc<GoogleAuth>, app: tauri::AppHandle) -> Self {
        // A summary still owed from before the last quit.
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let pending = todays_brief(&db, &today).ok().flatten().is_some_and(|brief| brief.summary_pending);
        BriefingService { db, google_auth, app, pending: AtomicBool::new(pending), busy: AtomicBool::new(false), retry_at: Mutex::new(None) }
    }

    pub async fn run(&self, trigger: Trigger) -> Result<String, String> {
        let report = run_with(&self.db, self, trigger, chrono::Local::now().fixed_offset()).await;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let owed = todays_brief(&self.db, &today).ok().flatten().is_some_and(|brief| brief.summary_pending);
        self.pending.store(owed, Ordering::SeqCst);
        report
    }

    /// Called on every scheduler tick with whether the network is up and
    /// settled. Fetches an owed summary once it is.
    pub fn on_tick(self: &Arc<Self>, online: bool) {
        if !online || !self.pending.load(Ordering::SeqCst) {
            return;
        }
        if self.retry_at.lock().is_ok_and(|at| at.is_some_and(|at| Instant::now() < at)) {
            return;
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let service = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let still_owed = match complete_pending_with(&service.db, service.as_ref(), &today).await {
                Ok(still_owed) => still_owed,
                Err(e) => {
                    log::warn!("Daily briefing: could not add the AI summary: {}", e);
                    false
                }
            };
            if still_owed {
                if let Ok(mut retry_at) = service.retry_at.lock() {
                    *retry_at = Some(Instant::now() + SUMMARY_RETRY_PAUSE);
                }
            }
            service.pending.store(still_owed, Ordering::SeqCst);
            service.busy.store(false, Ordering::SeqCst);
        });
    }
}

/// Read something out of Tauri state, waiting for it to be published.
///
/// State is published at the end of app setup, and a briefing owed from before
/// the launch can start a moment earlier than that.
async fn when_managed<T, R>(app: &tauri::AppHandle, read: impl Fn(&T) -> R) -> Option<R>
where
    T: Send + Sync + 'static,
{
    use tauri::Manager;
    for _ in 0..100 {
        if let Some(state) = app.try_state::<T>() {
            return Some(read(state.inner()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

#[async_trait::async_trait]
impl BriefingHost for BriefingService {
    fn online(&self) -> bool {
        SystemReachability.online()
    }

    fn screen(&self) -> Screen {
        Screen { locked: crate::system::screen::screen_locked(), display_asleep: crate::system::screen::display_asleep() }
    }

    async fn summarise(&self) -> Result<String, String> {
        let router = when_managed(&self.app, |router: &Mutex<crate::ai::AiRouter>| router.lock().map(|router| router.clone()).map_err(|e| e.to_string()))
            .await
            .ok_or("JARVIS has not finished starting")??;
        crate::assistant::briefing::generate_briefing(&self.db, &router, &self.google_auth, &self.app)
            .await
            .map(|result| result.briefing)
    }

    async fn notify(&self, title: &str, body: &str) {
        if let Err(e) = crate::system::control::send_notification(title, body, true).await {
            log::warn!("Daily briefing could not post a notification: {}", e);
        }
    }

    async fn speak(&self, text: &str) {
        // The app's one TextToSpeech, so the stop control reaches it and the
        // saved voice and rate apply.
        let tts = when_managed(&self.app, |voice: &Arc<crate::voice::VoiceEngine>| voice.tts.lock().ok().map(|tts| tts.clone())).await.flatten();
        let Some(tts) = tts else {
            log::warn!("Daily briefing voice unavailable");
            return;
        };
        if let Err(e) = tts.speak(text).await {
            log::warn!("Daily briefing voice failed: {}", e);
        }
    }

    fn changed(&self, brief: &Brief) {
        use tauri::Emitter;
        if let Err(e) = self.app.emit(CHANGED_EVENT, brief) {
            log::warn!("Daily briefing could not update the window: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AWAKE_AND_UNLOCKED: Screen = Screen { locked: false, display_asleep: false };

    #[test]
    fn only_the_on_time_run_with_the_screen_unlocked_speaks() {
        assert!(should_speak(Trigger::Scheduled, true, AWAKE_AND_UNLOCKED));
        // Any late run is silent, however little it is late by.
        assert!(!should_speak(Trigger::CatchUp, true, AWAKE_AND_UNLOCKED));
        assert!(!should_speak(Trigger::RetryInterrupted, true, AWAKE_AND_UNLOCKED));
        assert!(!should_speak(Trigger::Manual, true, AWAKE_AND_UNLOCKED));
        // On time, but nobody can be there to hear it.
        assert!(!should_speak(Trigger::Scheduled, true, Screen { locked: true, display_asleep: false }));
        assert!(!should_speak(Trigger::Scheduled, true, Screen { locked: false, display_asleep: true }));
        // Voice turned off.
        assert!(!should_speak(Trigger::Scheduled, false, AWAKE_AND_UNLOCKED));
    }

    #[test]
    fn a_time_of_day_becomes_a_daily_schedule_and_back() {
        assert_eq!(schedule_for_time("08:30").unwrap(), "0 30 8 * * *");
        assert_eq!(schedule_for_time("00:00").unwrap(), "0 0 0 * * *");
        assert_eq!(schedule_for_time("23:59").unwrap(), "0 59 23 * * *");
        for bad in ["24:00", "08:60", "8:30", "0830", "", "ab:cd", "08:30:00"] {
            assert!(schedule_for_time(bad).is_err(), "{bad}");
        }
        assert_eq!(time_from_schedule("0 30 8 * * *").as_deref(), Some("08:30"));
        assert_eq!(time_from_schedule("0 5 17 * * *").as_deref(), Some("17:05"));
        assert_eq!(time_from_schedule("0 */5 * * * *"), None, "not a daily time");
        // Every schedule this module writes is one the scheduler can read.
        assert!(parse_schedule(&schedule_for_time(DEFAULT_TIME).unwrap()).is_ok());
    }

    fn hk(text: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(text).unwrap()
    }

    fn brief_of(date: &str) -> LocalBrief {
        LocalBrief {
            date: chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            meetings: Some(vec![("09:30".to_string(), "Standup".to_string()), (String::new(), "Company day".to_string())]),
            overdue: vec!["報稅表".to_string()],
            due_today: vec!["回覆客戶".to_string(), "訂機票".to_string()],
            unread: Some(5),
        }
    }

    #[test]
    fn the_local_brief_reads_in_both_languages() {
        let brief = brief_of("2026-11-12");
        assert_eq!(
            brief.text(true),
            "11 月 12 日（星期四）\n會議：今天有 2 個\n  09:30　Standup\n  全日　Company day\n\
             工作：1 項已逾期，2 項今天到期\n  已逾期：報稅表\n  今天到期：回覆客戶、訂機票\n電郵：5 封未讀"
        );
        assert_eq!(
            brief.text(false),
            "Thursday 12 November\nMeetings: 2 today\n  09:30　Standup\n  All day　Company day\n\
             Tasks: 1 overdue, 2 due today\n  Overdue: 報稅表\n  Due today: 回覆客戶, 訂機票\nMail: 5 unread"
        );
        assert_eq!(brief.headline(true), "今天：2 個會議、1 項工作已逾期、2 項工作今天到期、5 封未讀電郵。");
        assert_eq!(brief.headline(false), "Today: 2 meetings, 1 overdue task, 2 tasks due today, 5 unread emails.");
    }

    /// Never synced is not the same claim as nothing there.
    #[test]
    fn an_unsynced_calendar_or_mailbox_is_not_reported_as_empty() {
        let unsynced = LocalBrief { meetings: None, unread: None, overdue: vec![], due_today: vec![], ..brief_of("2026-11-12") };
        assert_eq!(unsynced.text(true), "11 月 12 日（星期四）\n會議：尚未同步日曆\n工作：沒有已逾期或今天到期的工作\n電郵：尚未同步");
        let empty = LocalBrief { meetings: Some(vec![]), unread: Some(0), ..unsynced.clone() };
        assert_eq!(empty.text(true), "11 月 12 日（星期四）\n會議：今天沒有會議\n工作：沒有已逾期或今天到期的工作\n電郵：沒有未讀郵件");
        assert_eq!(empty.headline(true), "今天沒有會議，也沒有已逾期或今天到期的工作。");
        assert_eq!(empty.headline(false), "No meetings today, and nothing overdue or due.");
    }

    #[test]
    fn the_local_brief_is_gathered_from_the_database() {
        let db = crate::db::test_database();
        let today = chrono::Local::now().fixed_offset();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("INSERT INTO tasks (title, status, deadline) VALUES ('Tax form', 'pending', date('now', 'localtime', '-1 day'))", []).unwrap();
            conn.execute("INSERT INTO tasks (title, status, deadline) VALUES ('Reply to client', 'pending', date('now', 'localtime'))", []).unwrap();
            conn.execute("INSERT INTO tasks (title, status, deadline) VALUES ('Done already', 'completed', date('now', 'localtime'))", []).unwrap();
            conn.execute(
                "INSERT INTO calendar_events (google_id, summary, start_time, end_time) VALUES ('e1', 'Standup', ?1, ?2)",
                params![today.to_rfc3339(), (today + chrono::TimeDelta::minutes(30)).to_rfc3339()],
            )
            .unwrap();
        }
        let brief = LocalBrief::gather(&db, today).unwrap();
        assert_eq!(brief.overdue, ["Tax form"]);
        assert_eq!(brief.due_today, ["Reply to client"]);
        assert_eq!(brief.meetings, Some(vec![(today.format("%H:%M").to_string(), "Standup".to_string())]));
        assert_eq!(brief.unread, None, "no mail has ever been synced");
    }

    /// A host that records what the briefing did to the outside world.
    struct FakeHost {
        online: AtomicBool,
        screen: Screen,
        summary: Mutex<Result<String, String>>,
        asked_for_summary: Mutex<u32>,
        notifications: Mutex<Vec<String>>,
        spoken: Mutex<Vec<String>>,
        shown: Mutex<Vec<Brief>>,
    }

    impl FakeHost {
        fn new(online: bool) -> FakeHost {
            FakeHost {
                online: AtomicBool::new(online),
                screen: AWAKE_AND_UNLOCKED,
                summary: Mutex::new(Ok("You have one meeting and two things due.".to_string())),
                asked_for_summary: Mutex::new(0),
                notifications: Mutex::new(Vec::new()),
                spoken: Mutex::new(Vec::new()),
                shown: Mutex::new(Vec::new()),
            }
        }

        fn last_shown(&self) -> Brief {
            self.shown.lock().unwrap().last().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl BriefingHost for FakeHost {
        fn online(&self) -> bool {
            self.online.load(Ordering::SeqCst)
        }

        fn screen(&self) -> Screen {
            self.screen
        }

        async fn summarise(&self) -> Result<String, String> {
            *self.asked_for_summary.lock().unwrap() += 1;
            self.summary.lock().unwrap().clone()
        }

        async fn notify(&self, _title: &str, body: &str) {
            self.notifications.lock().unwrap().push(body.to_string());
        }

        async fn speak(&self, text: &str) {
            self.spoken.lock().unwrap().push(text.to_string());
        }

        fn changed(&self, brief: &Brief) {
            self.shown.lock().unwrap().push(brief.clone());
        }
    }

    const MORNING: &str = "2026-11-12T08:30:01+08:00";

    #[tokio::test]
    async fn the_on_time_briefing_is_shown_summarised_and_spoken() {
        let db = crate::db::test_database();
        let host = FakeHost::new(true);
        let report = run_with(&db, &host, Trigger::Scheduled, hk(MORNING)).await.unwrap();
        assert_eq!(report, "Briefing shown; AI summary added");

        let shown = host.shown.lock().unwrap().clone();
        assert_eq!(shown.len(), 2, "first from local data, then updated in place");
        assert!(shown[0].summary.is_none() && shown[0].local.starts_with("Thursday 12 November"));
        assert_eq!(shown[1].summary.as_deref(), Some("You have one meeting and two things due."));
        assert!(!shown[1].summary_pending);
        assert_eq!(host.notifications.lock().unwrap().len(), 1);
        assert_eq!(*host.spoken.lock().unwrap(), ["You have one meeting and two things due."]);
        assert_eq!(todays_brief(&db, "2026-11-12").unwrap(), Some(host.last_shown()));
    }

    /// A Mac that slept from 08:25 to 08:35 shows the 08:30 briefing at 08:35
    /// without voice.
    #[tokio::test]
    async fn a_late_briefing_is_shown_once_and_never_spoken() {
        for trigger in [Trigger::CatchUp, Trigger::RetryInterrupted] {
            let db = crate::db::test_database();
            let host = FakeHost::new(true);
            run_with(&db, &host, trigger, hk("2026-11-12T08:35:00+08:00")).await.unwrap();
            assert_eq!(host.notifications.lock().unwrap().len(), 1, "{trigger:?}: shown once");
            assert!(host.spoken.lock().unwrap().is_empty(), "{trigger:?}: never spoken");
            assert_eq!(host.last_shown().trigger, trigger.as_str());
        }
    }

    #[tokio::test]
    async fn an_on_time_briefing_is_silent_with_the_screen_locked_or_voice_off() {
        let db = crate::db::test_database();
        let mut host = FakeHost::new(true);
        host.screen = Screen { locked: true, display_asleep: false };
        run_with(&db, &host, Trigger::Scheduled, hk(MORNING)).await.unwrap();
        assert!(host.spoken.lock().unwrap().is_empty(), "locked");
        assert_eq!(host.notifications.lock().unwrap().len(), 1, "it is still shown");

        for (key, label) in [(VOICE_KEY, "the briefing's voice switch"), ("tts_enabled", "spoken replies switched off app-wide")] {
            let db = crate::db::test_database();
            set_preference(&db.conn.lock().unwrap(), key, "false").unwrap();
            let host = FakeHost::new(true);
            run_with(&db, &host, Trigger::Scheduled, hk(MORNING)).await.unwrap();
            assert!(host.spoken.lock().unwrap().is_empty(), "{label}");
        }
    }

    /// Offline at briefing time: the local version is shown, and the AI
    /// summary replaces it in place when the network returns, with no second
    /// notification and no voice.
    #[tokio::test]
    async fn an_offline_briefing_is_completed_in_place_when_the_network_returns() {
        let db = crate::db::test_database();
        let host = FakeHost::new(false);
        let report = run_with(&db, &host, Trigger::Scheduled, hk(MORNING)).await.unwrap();
        assert!(report.contains("follows when the network returns"), "{report}");
        assert_eq!(*host.asked_for_summary.lock().unwrap(), 0, "no AI call into a dead network");
        assert!(host.last_shown().summary_pending);
        assert_eq!(host.notifications.lock().unwrap().len(), 1);
        assert_eq!(host.spoken.lock().unwrap().len(), 1, "the local version is spoken on time");
        assert!(host.spoken.lock().unwrap()[0].starts_with("Thursday 12 November"));

        // Still offline: nothing to add yet.
        *host.summary.lock().unwrap() = Err("connection failed".to_string());
        assert!(complete_pending_with(&db, &host, "2026-11-12").await.unwrap(), "still owed");

        host.online.store(true, Ordering::SeqCst);
        *host.summary.lock().unwrap() = Ok("One meeting today.".to_string());
        assert!(!complete_pending_with(&db, &host, "2026-11-12").await.unwrap());
        assert_eq!(host.last_shown().summary.as_deref(), Some("One meeting today."));
        assert_eq!(host.notifications.lock().unwrap().len(), 1, "no second notification");
        assert_eq!(host.spoken.lock().unwrap().len(), 1, "no voice for the update");

        // Nothing is owed any more.
        assert!(!complete_pending_with(&db, &host, "2026-11-12").await.unwrap());
        assert_eq!(*host.asked_for_summary.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn a_summary_owed_from_yesterday_is_dropped() {
        let db = crate::db::test_database();
        let host = FakeHost::new(false);
        run_with(&db, &host, Trigger::CatchUp, hk(MORNING)).await.unwrap();
        host.online.store(true, Ordering::SeqCst);
        assert!(!complete_pending_with(&db, &host, "2026-11-13").await.unwrap());
        assert_eq!(*host.asked_for_summary.lock().unwrap(), 0);
        assert_eq!(todays_brief(&db, "2026-11-13").unwrap(), None, "yesterday's brief is not today's");
    }

    /// An AI error with the network up is not a reason to wait: the local
    /// brief stays, with the reason.
    #[tokio::test]
    async fn an_ai_failure_with_the_network_up_leaves_the_local_brief() {
        let db = crate::db::test_database();
        let host = FakeHost::new(true);
        *host.summary.lock().unwrap() = Err("No AI provider is set up".to_string());
        let report = run_with(&db, &host, Trigger::Scheduled, hk(MORNING)).await.unwrap();
        assert!(report.ends_with("the AI summary failed: No AI provider is set up"), "{report}");
        let brief = host.last_shown();
        assert!(!brief.summary_pending && brief.summary.is_none());
        assert_eq!(brief.summary_error.as_deref(), Some("No AI provider is set up"));
        assert_eq!(host.spoken.lock().unwrap().len(), 1, "the local text is spoken instead");
    }

    #[tokio::test]
    async fn the_schedule_gives_at_most_one_briefing_a_day() {
        let db = crate::db::test_database();
        let host = FakeHost::new(true);
        run_with(&db, &host, Trigger::Scheduled, hk(MORNING)).await.unwrap();
        // The time is moved later the same morning and comes round again.
        let again = run_with(&db, &host, Trigger::Scheduled, hk("2026-11-12T10:00:00+08:00")).await.unwrap();
        assert_eq!(again, "Already shown today");
        assert_eq!(host.notifications.lock().unwrap().len(), 1);
        assert_eq!(host.spoken.lock().unwrap().len(), 1);

        // The next local day is a new briefing.
        run_with(&db, &host, Trigger::Scheduled, hk("2026-11-13T08:30:00+08:00")).await.unwrap();
        assert_eq!(host.notifications.lock().unwrap().len(), 2);
    }

    /// "Show now" is the user's own request: always shown, never announced or
    /// spoken, and the day's scheduled briefing still comes.
    #[tokio::test]
    async fn a_briefing_asked_for_by_hand_does_not_use_up_the_scheduled_one() {
        let db = crate::db::test_database();
        let host = FakeHost::new(true);
        run_with(&db, &host, Trigger::Manual, hk("2026-11-12T07:00:00+08:00")).await.unwrap();
        assert!(host.notifications.lock().unwrap().is_empty());
        assert!(host.spoken.lock().unwrap().is_empty());
        assert_eq!(host.last_shown().trigger, "manual");

        run_with(&db, &host, Trigger::Scheduled, hk(MORNING)).await.unwrap();
        assert_eq!(host.notifications.lock().unwrap().len(), 1);
        assert_eq!(host.spoken.lock().unwrap().len(), 1);

        // And by hand again afterwards.
        run_with(&db, &host, Trigger::Manual, hk("2026-11-12T12:00:00+08:00")).await.unwrap();
        assert_eq!(host.last_shown().trigger, "manual");
    }

    fn briefing_row(db: &Database) -> (String, String, String) {
        db.conn
            .lock()
            .unwrap()
            .query_row("SELECT schedule, status, baseline_at FROM cron_jobs WHERE action_type = 'daily_briefing'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap()
    }

    #[test]
    fn the_briefing_is_off_by_default_at_0830_with_voice_on() {
        let db = crate::db::test_database();
        let settings = read_settings(&db.conn.lock().unwrap()).unwrap();
        assert_eq!(settings, BriefingSettings { enabled: false, time: "08:30".to_string(), voice: true, speech_enabled: true });
        let (schedule, status, _) = briefing_row(&db);
        assert_eq!((schedule.as_str(), status.as_str()), ("0 30 8 * * *", "paused"), "seeded by the migration, paused");
    }

    #[test]
    fn turning_it_on_or_moving_its_time_starts_from_the_next_slot() {
        let db = crate::db::test_database();
        let at = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc);
        let conn = db.conn.lock().unwrap();
        conn.execute("UPDATE cron_jobs SET baseline_at = '2026-01-01T00:00:00Z' WHERE action_type = 'daily_briefing'", []).unwrap();

        let saved = write_settings(&conn, true, "08:30", true, at("2026-11-12T09:00:00+08:00")).unwrap();
        assert!(saved.active);
        assert_eq!(saved.schedule, "0 30 8 * * *");
        drop(conn);
        assert_eq!(briefing_row(&db), ("0 30 8 * * *".to_string(), "active".to_string(), "2026-11-12T01:00:00Z".to_string()));

        // Moved to 07:00 at 09:30: 07:00 today has passed and must not run now.
        let conn = db.conn.lock().unwrap();
        let saved = write_settings(&conn, true, "07:00", false, at("2026-11-12T09:30:00+08:00")).unwrap();
        assert_eq!(saved.schedule, "0 0 7 * * *");
        let settings = read_settings(&conn).unwrap();
        assert_eq!((settings.enabled, settings.time.as_str(), settings.voice), (true, "07:00", false));
        drop(conn);
        assert_eq!(briefing_row(&db).2, "2026-11-12T01:30:00Z");

        // Saving again with nothing changed leaves the baseline alone.
        let conn = db.conn.lock().unwrap();
        write_settings(&conn, true, "07:00", false, at("2026-11-12T10:00:00+08:00")).unwrap();
        assert!(write_settings(&conn, true, "25:00", false, at("2026-11-12T10:00:00+08:00")).is_err());
        drop(conn);
        assert_eq!(briefing_row(&db).2, "2026-11-12T01:30:00Z");
    }

    /// The job can be deleted from the Automations page like any other;
    /// Settings then puts it back.
    #[test]
    fn a_deleted_briefing_job_is_recreated_from_settings() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        conn.execute("DELETE FROM cron_jobs WHERE action_type = 'daily_briefing'", []).unwrap();
        assert!(!read_settings(&conn).unwrap().enabled);
        let saved = write_settings(&conn, true, "09:15", true, Utc::now()).unwrap();
        assert!(saved.job_id > 0);
        let settings = read_settings(&conn).unwrap();
        assert_eq!((settings.enabled, settings.time.as_str()), (true, "09:15"));
    }
}
