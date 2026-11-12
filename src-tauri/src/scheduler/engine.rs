//! The slot engine. On every tick it works out which jobs have a slot to run,
//! starts those runs, and records how each one ended. See `slots` for what a
//! slot, a handled slot, a baseline, a missed slot and a trigger are.
//!
//! The same code path serves the live tick, a wake from sleep and a launch:
//! in each case the question is "what is the missed slot now", and the answer
//! comes from `cron_jobs.handled_slot` and `cron_jobs.baseline_at`, never from
//! memory. The clock, the network and the jobs themselves are injected, so the
//! tests below drive this exact code through sleep, offline wakes and crashes.

use super::jobs::{reconcile_interrupted_runs, CRON_RUNS_RETENTION_QUERY};
use super::network::{
    classify_outcome, network_class, offline_retry_delay_secs, NetworkClass, Reachability, RunStatus, NETWORK_SETTLE_SECS,
    OFFLINE_WAIT_SECS,
};
use super::slots::{self, Slot, Trigger};
use crate::db::Database;
use chrono::{DateTime, FixedOffset, Offset, SecondsFormat, TimeDelta, TimeZone, Utc};
use croner::Cron;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Wall-clock time and awake time, read together.
///
/// Wake detection compares the two: the wall clock keeps counting while the
/// Mac sleeps and the awake clock stands still, so a tick where wall-clock
/// time moved further than awake time is the first tick after a sleep.
///
/// This was chosen over the macOS did-wake notification for three reasons.
/// The check runs inside the tick, before any slot is looked at, so a slot can
/// never be judged "on time" by a tick that ran ahead of a notification still
/// queued on the main thread. It also catches what the notification does not:
/// a clock set by hand, and a dark wake with the lid closed. And it is driven
/// by the injected clock, so the tests exercise the code that ships.
pub trait Clock: Send + Sync + 'static {
    type Zone: TimeZone;
    /// Wall-clock time. Keeps counting while the Mac sleeps.
    fn now(&self) -> DateTime<Utc>;
    /// Time spent awake since a fixed point. Stands still while the Mac sleeps.
    fn awake(&self) -> Duration;
    /// The Mac's time zone as it is now. Read on every tick, so a job is never
    /// tied to the offset that was in force when it was registered.
    fn zone(&self) -> Self::Zone;
}

/// The Mac's own clocks. `Instant` is the awake clock: on macOS it does not
/// advance during sleep.
pub struct SystemClock {
    started: std::time::Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        SystemClock { started: std::time::Instant::now() }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    type Zone = chrono::Local;

    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn awake(&self) -> Duration {
        self.started.elapsed()
    }

    fn zone(&self) -> chrono::Local {
        chrono::Local
    }
}

/// How far wall-clock time may run ahead of awake time between two readings
/// before it counts as a sleep. Clock corrections from the time server are
/// far smaller, and no sleep is this short.
const WAKE_GAP_MILLIS: i64 = 3_000;

/// Whether the wall clock moved differently from the awake clock: the Mac
/// slept, or the clock was changed. Either way, slots may have gone by.
pub fn wall_clock_jumped(wall: TimeDelta, awake: Duration) -> bool {
    let awake_millis = i64::try_from(awake.as_millis()).unwrap_or(i64::MAX);
    (wall.num_milliseconds() - awake_millis).abs() > WAKE_GAP_MILLIS
}

/// Does the work of one run. The engine does the bookkeeping around it.
#[async_trait::async_trait]
pub trait JobRunner: Send + Sync {
    async fn run(&self, run: &Run) -> Result<String, String>;
}

/// One run that has been started: its `cron_runs` row exists and says
/// `running`.
#[derive(Clone, Debug)]
pub struct Run {
    pub run_id: i64,
    pub job_id: i64,
    pub action_type: String,
    pub trigger: Trigger,
    /// `None` for a manual run, which belongs to no slot.
    pub slot: Option<Slot>,
    started_wall: DateTime<Utc>,
    started_awake: Duration,
}

struct Job {
    name: String,
    action_type: String,
    cron: Cron,
    /// When to look at the job again. `None`: at the next tick.
    next_check: Option<DateTime<Utc>>,
    wait: Option<OfflineWait>,
}

/// A network-required slot that is owed but has no network to run on.
struct OfflineWait {
    slot: Slot,
    /// Awake time when the wait began.
    since: Duration,
    /// A `skipped_offline` row has been written for the slot.
    recorded: bool,
    /// Runs of this slot that ended offline; spaces out the next one.
    attempts: u32,
    not_before: Duration,
}

#[derive(Clone, Copy)]
enum Network {
    Offline,
    /// Reachable again since this awake time, not yet for long enough.
    Settling(Duration),
    Online,
}

struct State {
    jobs: HashMap<i64, Job>,
    /// Jobs with a run in flight, scheduled or manual.
    running: HashSet<i64>,
    last_wall: DateTime<Utc>,
    last_awake: Duration,
    last_wake_or_launch: DateTime<Utc>,
    offset: FixedOffset,
    network: Network,
}

/// What one tick knows.
struct Moment<Z: TimeZone> {
    now: DateTime<Utc>,
    awake: Duration,
    zone: Z,
    /// The network is up and has been for long enough to use.
    online: bool,
    last_wake_or_launch: DateTime<Utc>,
}

struct JobRow {
    active: bool,
    schedule: String,
    handled: Option<Slot>,
    baseline: Option<DateTime<Utc>>,
}

const SLEPT_DURING_RUN: &str = "Interrupted: the Mac went to sleep during this run";
const INTERRUPTED_TWICE: &str = "Interrupted again on the retry; this occurrence is not retried a second time";

fn db_err(e: rusqlite::Error) -> String {
    e.to_string()
}

/// `baseline_at` as it is stored: UTC, to the second.
pub fn format_baseline(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// `next_run` is stored like `last_run` (UTC, 'YYYY-MM-DD HH:MM:SS'), which is
/// what the dashboard already reads.
fn format_next_run(slot: &Slot) -> String {
    slot.with_timezone(&Utc).format("%Y-%m-%d %H:%M:%S").to_string()
}

impl JobRow {
    fn read(conn: &Connection, job_id: i64) -> Result<Option<JobRow>, String> {
        conn.query_row(
            "SELECT status, schedule, handled_slot, baseline_at FROM cron_jobs WHERE id = ?1",
            params![job_id],
            |row| {
                let status: String = row.get(0)?;
                let handled: Option<String> = row.get(2)?;
                let baseline: Option<String> = row.get(3)?;
                Ok(JobRow {
                    active: status == "active",
                    schedule: row.get(1)?,
                    handled: handled.as_deref().and_then(slots::parse_slot),
                    baseline: baseline.as_deref().and_then(slots::parse_slot).map(|at| at.with_timezone(&Utc)),
                })
            },
        )
        .optional()
        .map_err(db_err)
    }

    /// The baseline, set to now for a row that has none. Every job gets one
    /// when it is created; this covers a row written by anything that did not.
    fn baseline_or_set(&self, conn: &Connection, job_id: i64, now: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        if let Some(baseline) = self.baseline {
            return Ok(baseline);
        }
        conn.execute("UPDATE cron_jobs SET baseline_at = ?1 WHERE id = ?2", params![format_baseline(now), job_id])
            .map_err(db_err)?;
        Ok(now)
    }
}

/// Pause or resume a job. Pausing keeps `handled_slot`. Resuming moves the
/// baseline to now, so nothing that came due while the job was paused is owed.
pub fn write_status(conn: &Connection, job_id: i64, active: bool, now: DateTime<Utc>) -> Result<(), String> {
    if active {
        conn.execute(
            "UPDATE cron_jobs SET status = 'active', baseline_at = ?1 WHERE id = ?2",
            params![format_baseline(now), job_id],
        )
    } else {
        conn.execute("UPDATE cron_jobs SET status = 'paused', next_run = NULL WHERE id = ?1", params![job_id])
    }
    .map_err(db_err)?;
    Ok(())
}

fn write_next_run(conn: &Connection, job_id: i64, next: Option<&Slot>) -> Result<(), String> {
    conn.execute("UPDATE cron_jobs SET next_run = ?1 WHERE id = ?2", params![next.map(format_next_run), job_id])
        .map_err(db_err)?;
    Ok(())
}

/// Record that `slot` reached a final state: note the older owed slots that
/// collapsed into it, then advance `handled_slot`. One place, so an owed slot
/// that never ran is counted as collapsed exactly once.
fn mark_handled<Z: TimeZone>(conn: &Connection, job_id: i64, slot: &Slot, now: DateTime<Utc>, zone: &Z) -> Result<(), String> {
    let Some(row) = JobRow::read(conn, job_id)? else { return Ok(()) };
    if row.handled.as_ref().is_some_and(|handled| handled >= slot) {
        return Ok(());
    }
    if let Ok(cron) = slots::parse_schedule(&row.schedule) {
        let baseline = row.baseline.unwrap_or(now);
        let older = slots::collapsed_before(&cron, zone, row.handled.as_ref(), baseline, slot, now);
        if let (Some(oldest), Some(newest)) = (&older.oldest, &older.newest) {
            let detail = serde_json::json!({
                "count": older.count,
                "capped": older.count >= slots::COLLAPSED_COUNT_CAP,
                "from": slots::format_slot(oldest),
                "to": slots::format_slot(newest),
            });
            // Dated at the newest occurrence it stands for, so it sits in the
            // history where those occurrences were due.
            conn.execute(
                "INSERT INTO cron_runs (job_id, started_at, finished_at, status, result, slot) \
                 VALUES (?1, ?2, datetime('now'), 'collapsed', ?3, ?4)",
                params![job_id, format_next_run(newest), detail.to_string(), slots::format_slot(newest)],
            )
            .map_err(db_err)?;
        }
    }
    conn.execute("UPDATE cron_jobs SET handled_slot = ?1 WHERE id = ?2", params![slots::format_slot(slot), job_id])
        .map_err(db_err)?;
    Ok(())
}

fn record_skipped_offline(conn: &Connection, job_id: i64, slot: &Slot) -> Result<(), String> {
    conn.execute(
        "INSERT INTO cron_runs (job_id, finished_at, status, slot) VALUES (?1, datetime('now'), 'skipped_offline', ?2)",
        params![job_id, slots::format_slot(slot)],
    )
    .map_err(db_err)?;
    Ok(())
}

fn has_skipped_offline(conn: &Connection, job_id: i64, slot: &Slot) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM cron_runs WHERE job_id = ?1 AND slot = ?2 AND status = 'skipped_offline')",
        params![job_id, slots::format_slot(slot)],
        |row| row.get(0),
    )
    .map_err(db_err)
}

/// A slot recorded as `skipped_offline` is still owed, until a newer slot
/// comes due. From then on the newer one is the missed slot, and the old one
/// is final: `skipped_offline_final`. Returns whether anything changed.
fn close_superseded_offline<Z: TimeZone>(
    conn: &Connection,
    job_id: i64,
    current: &Slot,
    now: DateTime<Utc>,
    zone: &Z,
) -> Result<bool, String> {
    let waiting: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT slot FROM cron_runs WHERE job_id = ?1 AND status = 'skipped_offline' AND slot IS NOT NULL")
            .map_err(db_err)?;
        let rows = stmt.query_map(params![job_id], |row| row.get::<_, String>(0)).map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
    };
    let mut newest: Option<Slot> = None;
    for text in &waiting {
        let Some(slot) = slots::parse_slot(text).filter(|slot| slot < current) else { continue };
        conn.execute(
            "UPDATE cron_runs SET status = 'skipped_offline_final' WHERE job_id = ?1 AND status = 'skipped_offline' AND slot = ?2",
            params![job_id, text],
        )
        .map_err(db_err)?;
        newest = Some(newest.map_or(slot, |seen| seen.max(slot)));
    }
    match newest {
        Some(slot) => mark_handled(conn, job_id, &slot, now, zone).map(|()| true),
        None => Ok(false),
    }
}

/// The interrupted runs of a slot: how many, and how many of those were
/// themselves the retry.
fn interrupted_runs(conn: &Connection, job_id: i64, slot: &Slot) -> Result<(i64, i64), String> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(\"trigger\" = 'retry_interrupted'), 0) FROM cron_runs \
         WHERE job_id = ?1 AND slot = ?2 AND status = 'interrupted'",
        params![job_id, slots::format_slot(slot)],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(db_err)
}

pub struct Engine<C: Clock> {
    db: Arc<Database>,
    clock: Arc<C>,
    reachability: Arc<dyn Reachability>,
    runner: Arc<dyn JobRunner>,
    state: Mutex<State>,
}

impl<C: Clock> Engine<C> {
    /// Call once per launch, before anything else touches `cron_runs`: a row
    /// still marked `running` belongs to a process that is gone, and is
    /// resolved to `interrupted` here.
    pub fn new(db: Arc<Database>, clock: Arc<C>, reachability: Arc<dyn Reachability>, runner: Arc<dyn JobRunner>) -> Self {
        match reconcile_interrupted_runs(&db) {
            Ok(0) => {}
            Ok(n) => log::info!("Resolved {} cron run(s) interrupted by an unclean exit", n),
            Err(e) => log::warn!("Could not reconcile interrupted cron runs: {}", e),
        }
        let now = clock.now();
        let state = State {
            jobs: HashMap::new(),
            running: HashSet::new(),
            last_wall: now,
            last_awake: clock.awake(),
            last_wake_or_launch: now,
            offset: clock.zone().offset_from_utc_datetime(&now.naive_utc()).fix(),
            network: if reachability.online() { Network::Online } else { Network::Offline },
        };
        Engine { db, clock, reachability, runner, state: Mutex::new(state) }
    }

    /// Put a `cron_jobs` row under the engine, replacing any earlier
    /// registration of the same row, and write its `next_run`.
    pub fn register(&self, job_id: i64, name: &str, schedule: &str, action_type: &str) -> Result<(), String> {
        let cron = slots::parse_schedule(schedule)?;
        let now = self.clock.now();
        let zone = self.clock.zone();
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        {
            let conn = self.db.conn.lock().map_err(|e| e.to_string())?;
            if let Some(row) = JobRow::read(&conn, job_id)? {
                let baseline = row.baseline_or_set(&conn, job_id, now)?;
                let next = slots::next_slot(&cron, &zone, row.handled.as_ref(), baseline, now);
                write_next_run(&conn, job_id, next.as_ref())?;
            }
        }
        state.jobs.insert(
            job_id,
            Job { name: name.to_string(), action_type: action_type.to_string(), cron, next_check: None, wait: None },
        );
        log::info!("Registered cron job: {} ({})", name, schedule);
        Ok(())
    }

    /// Take a job out of the engine. A job that was never registered is not
    /// an error: the caller only wants it gone.
    pub fn unregister(&self, job_id: i64) {
        let removed = match self.state.lock() {
            Ok(mut state) => state.jobs.remove(&job_id).is_some(),
            Err(e) => {
                log::error!("Scheduler state lock poisoned: {}", e);
                false
            }
        };
        if removed {
            log::info!("Unregistered cron job {}", job_id);
        }
        match self.db.conn.lock() {
            Ok(conn) => {
                if let Err(e) = conn.execute("UPDATE cron_jobs SET next_run = NULL WHERE id = ?1", params![job_id]) {
                    log::warn!("Could not clear next_run of cron job {}: {}", job_id, e);
                }
            }
            Err(e) => log::error!("Database lock poisoned clearing next_run: {}", e),
        }
    }

    /// One pass. Notices a wake and a change of UTC offset, settles every job
    /// that needs attention, and returns the runs it started. Each returned
    /// run must be passed to [`Engine::execute`].
    pub fn tick(&self) -> Vec<Run> {
        let now = self.clock.now();
        let awake = self.clock.awake();
        let zone = self.clock.zone();
        let reachable = self.reachability.online();
        let Ok(mut guard) = self.state.lock() else { return Vec::new() };
        let state = &mut *guard;

        if wall_clock_jumped(now - state.last_wall, awake.saturating_sub(state.last_awake)) {
            log::info!("Scheduler: the Mac woke or its clock changed; checking every job for a missed run");
            state.last_wake_or_launch = now;
            // The network that was up before the sleep has to show it is up again.
            if matches!(state.network, Network::Online) {
                state.network = Network::Settling(awake);
            }
            state.jobs.values_mut().for_each(|job| job.next_check = None);
        }
        state.last_wall = now;
        state.last_awake = awake;

        // Travel and daylight saving. Slots are always computed in the zone as
        // it is now, so "re-registering" is no more than looking at every job
        // again and rewriting its next run.
        let offset = zone.offset_from_utc_datetime(&now.naive_utc()).fix();
        if offset != state.offset {
            log::info!("Scheduler: UTC offset changed from {} to {}; re-registering every job", state.offset, offset);
            state.offset = offset;
            state.jobs.values_mut().for_each(|job| job.next_check = None);
        }

        state.network = match (reachable, state.network) {
            (false, _) => Network::Offline,
            (true, Network::Offline) => Network::Settling(awake),
            (true, Network::Settling(since)) if awake.saturating_sub(since) >= Duration::from_secs(NETWORK_SETTLE_SECS) => {
                Network::Online
            }
            (true, unchanged) => unchanged,
        };
        let moment = Moment {
            now,
            awake,
            zone,
            online: matches!(state.network, Network::Online),
            last_wake_or_launch: state.last_wake_or_launch,
        };

        let State { jobs, running, .. } = state;
        let mut due: Vec<i64> = jobs
            .iter()
            .filter(|(job_id, job)| !running.contains(job_id) && job.needs_attention(&moment))
            .map(|(job_id, _)| *job_id)
            .collect();
        if due.is_empty() {
            return Vec::new();
        }
        due.sort_unstable();
        let Ok(conn) = self.db.conn.lock() else { return Vec::new() };
        let mut runs = Vec::new();
        for job_id in due {
            let Some(job) = jobs.get_mut(&job_id) else { continue };
            match settle(&conn, job_id, job, &moment) {
                Ok(Some(run)) => {
                    running.insert(job_id);
                    runs.push(run);
                }
                Ok(None) => {}
                Err(e) => {
                    log::error!("Cron job {} ('{}') could not be settled: {}", job_id, job.name, e);
                    job.next_check = Some(now + TimeDelta::seconds(60));
                }
            }
        }
        runs
    }

    /// Run a started run to its end and record the outcome.
    pub async fn execute(&self, run: Run) {
        log::info!("Running cron job {}: {} ({})", run.job_id, run.action_type, run.trigger.as_str());
        let outcome = self.runner.run(&run).await;

        let now = self.clock.now();
        let awake = self.clock.awake();
        let slept = wall_clock_jumped(now - run.started_wall, awake.saturating_sub(run.started_awake));
        let status = classify_outcome(outcome.is_ok(), slept, network_class(&run.action_type), self.reachability.online());
        match &outcome {
            Ok(message) => log::info!("Job {} completed: {}", run.job_id, message),
            Err(error) => log::warn!("Job {} ended {}: {}", run.job_id, status.as_str(), error),
        }
        let (result, error) = match (&outcome, status) {
            (Ok(message), _) => (Some(message.as_str()), None),
            (Err(_), RunStatus::Interrupted) => (None, Some(SLEPT_DURING_RUN)),
            (Err(error), _) => (None, Some(error.as_str())),
        };

        let Ok(mut state) = self.state.lock() else { return };
        let recorded = self.db.conn.lock().map_err(|e| e.to_string()).and_then(|conn| {
            conn.execute(
                "UPDATE cron_runs SET finished_at = datetime('now'), status = ?1, result = ?2, error = ?3 WHERE id = ?4",
                params![status.as_str(), result, error, run.run_id],
            )
            .map_err(db_err)?;
            conn.execute("UPDATE cron_jobs SET last_run = datetime('now') WHERE id = ?1", params![run.job_id])
                .map_err(db_err)?;
            if let Some(slot) = run.slot.as_ref().filter(|_| status.is_final()) {
                mark_handled(&conn, run.job_id, slot, now, &self.clock.zone())?;
            }
            conn.execute(CRON_RUNS_RETENTION_QUERY, params![run.job_id]).map_err(db_err)?;
            Ok(())
        });
        if let Err(e) = recorded {
            log::error!("Could not record the outcome of cron job {}: {}", run.job_id, e);
        }

        state.running.remove(&run.job_id);
        // A manual run leaves the schedule alone: whatever slot is owed is
        // still owed, and whatever was waiting still waits.
        let (Some(slot), Some(job)) = (run.slot, state.jobs.get_mut(&run.job_id)) else { return };
        job.wait = match status {
            RunStatus::SkippedOffline => {
                let earlier = job.wait.take();
                let attempts = earlier.as_ref().map_or(0, |wait| wait.attempts);
                Some(OfflineWait {
                    slot,
                    since: earlier.map_or(awake, |wait| wait.since),
                    recorded: true,
                    attempts: attempts + 1,
                    not_before: awake + Duration::from_secs(offline_retry_delay_secs(attempts)),
                })
            }
            _ => None,
        };
        job.next_check = None;
    }

    /// Start a manual run of a job, whatever its schedule says. It belongs to
    /// no slot and never changes `handled_slot`.
    pub fn run_now(&self, job_id: i64) -> Result<Run, String> {
        let mut state = self.state.lock().map_err(|e| e.to_string())?;
        if state.running.contains(&job_id) {
            return Err("This automation is already running".to_string());
        }
        let conn = self.db.conn.lock().map_err(|e| e.to_string())?;
        let action_type: String = conn
            .query_row("SELECT action_type FROM cron_jobs WHERE id = ?1", params![job_id], |row| row.get(0))
            .optional()
            .map_err(db_err)?
            .ok_or("Automation not found")?;
        let run = start(&conn, job_id, &action_type, None, Trigger::Manual, self.clock.now(), self.clock.awake())?;
        state.running.insert(job_id);
        Ok(run)
    }

    /// Whether the network is up and has been for long enough to use.
    pub fn online(&self) -> bool {
        self.state.lock().is_ok_and(|state| matches!(state.network, Network::Online))
    }
}

impl Job {
    fn needs_attention<Z: TimeZone>(&self, moment: &Moment<Z>) -> bool {
        if self.next_check.is_none_or(|at| moment.now >= at) {
            return true;
        }
        self.wait.as_ref().is_some_and(|wait| {
            let can_run = moment.online && moment.awake >= wait.not_before;
            let overdue = !wait.recorded && moment.awake.saturating_sub(wait.since) >= Duration::from_secs(OFFLINE_WAIT_SECS);
            can_run || overdue
        })
    }
}

fn start(
    conn: &Connection,
    job_id: i64,
    action_type: &str,
    slot: Option<Slot>,
    trigger: Trigger,
    now: DateTime<Utc>,
    awake: Duration,
) -> Result<Run, String> {
    conn.execute(
        "INSERT INTO cron_runs (job_id, status, \"trigger\", slot) VALUES (?1, 'running', ?2, ?3)",
        params![job_id, trigger.as_str(), slot.as_ref().map(slots::format_slot)],
    )
    .map_err(db_err)?;
    Ok(Run {
        run_id: conn.last_insert_rowid(),
        job_id,
        action_type: action_type.to_string(),
        trigger,
        slot,
        started_wall: now,
        started_awake: awake,
    })
}

/// Decide what one job does now: nothing, wait for the network, or run.
fn settle<Z: TimeZone>(conn: &Connection, job_id: i64, job: &mut Job, moment: &Moment<Z>) -> Result<Option<Run>, String> {
    let now = moment.now;
    // Each pass either returns or moves `handled_slot` forward, so a handful
    // is more than enough.
    for _ in 0..4 {
        let row = match JobRow::read(conn, job_id)? {
            Some(row) if row.active => row,
            // Paused or deleted by something that did not unregister it.
            _ => {
                job.wait = None;
                job.next_check = Some(now + TimeDelta::seconds(60));
                return Ok(None);
            }
        };
        let baseline = row.baseline_or_set(conn, job_id, now)?;
        let next = slots::next_slot(&job.cron, &moment.zone, row.handled.as_ref(), baseline, now);
        let next_check = Some(next.as_ref().map_or(now + TimeDelta::days(1), |slot| slot.with_timezone(&Utc)));

        let Some(missed) = slots::missed_slot(&job.cron, &moment.zone, row.handled.as_ref(), baseline, now) else {
            job.wait = None;
            job.next_check = next_check;
            write_next_run(conn, job_id, next.as_ref())?;
            return Ok(None);
        };
        let slot = missed.slot;

        // A slot that was waiting for the network and has been overtaken by a
        // newer one: the newer one supersedes it.
        if let Some(overtaken) = job.wait.take_if(|wait| wait.slot != slot) {
            if !overtaken.recorded {
                record_skipped_offline(conn, job_id, &overtaken.slot)?;
            }
        }
        if close_superseded_offline(conn, job_id, &slot, now, &moment.zone)? {
            continue;
        }

        let trigger = match interrupted_runs(conn, job_id, &slot)? {
            // Anything that had to wait for the network is a catch-up, even
            // inside the two minutes.
            (0, _) if job.wait.is_some() => Trigger::CatchUp,
            (0, _) => slots::trigger_for(&slot, now, moment.last_wake_or_launch),
            (_, 0) => Trigger::RetryInterrupted,
            // The retry was cut short as well. Never more than one retry per
            // slot: the slot ends here, as a failure the user is told about.
            _ => {
                conn.execute(
                    "UPDATE cron_runs SET status = 'failed', error = ?1 \
                     WHERE job_id = ?2 AND slot = ?3 AND status = 'interrupted' AND \"trigger\" = 'retry_interrupted'",
                    params![INTERRUPTED_TWICE, job_id, slots::format_slot(&slot)],
                )
                .map_err(db_err)?;
                mark_handled(conn, job_id, &slot, now, &moment.zone)?;
                continue;
            }
        };

        job.next_check = next_check;
        write_next_run(conn, job_id, next.as_ref())?;

        if network_class(&job.action_type) == NetworkClass::Required && !moment.online {
            if job.wait.is_none() {
                job.wait = Some(OfflineWait {
                    slot,
                    since: moment.awake,
                    recorded: has_skipped_offline(conn, job_id, &slot)?,
                    attempts: 0,
                    not_before: moment.awake,
                });
            }
            if let Some(wait) = job.wait.as_mut() {
                let waited = moment.awake.saturating_sub(wait.since);
                if !wait.recorded && waited >= Duration::from_secs(OFFLINE_WAIT_SECS) {
                    record_skipped_offline(conn, job_id, &slot)?;
                    wait.recorded = true;
                }
            }
            return Ok(None);
        }
        // Backing off after a run that ended offline.
        if job.wait.as_ref().is_some_and(|wait| moment.awake < wait.not_before) {
            return Ok(None);
        }
        return start(conn, job_id, &job.action_type, Some(slot), trigger, now, moment.awake).map(Some);
    }
    Err("the job did not settle".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::slots::test_zone::TestZone;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc)
    }

    /// The injected clock. `pass` is time with the Mac awake; `sleep_until`
    /// moves the wall clock only, which is what a sleeping Mac does.
    struct TestClock {
        wall: Mutex<DateTime<Utc>>,
        awake: Mutex<Duration>,
        zone: Mutex<TestZone>,
    }

    impl TestClock {
        fn pass(&self, secs: u64) {
            *self.wall.lock().unwrap() += TimeDelta::seconds(secs as i64);
            *self.awake.lock().unwrap() += Duration::from_secs(secs);
        }

        fn sleep_until(&self, text: &str) {
            let mut wall = self.wall.lock().unwrap();
            assert!(at(text) > *wall, "a sleep ends later than it began");
            *wall = at(text);
        }

        fn set_zone(&self, zone: TestZone) {
            *self.zone.lock().unwrap() = zone;
        }
    }

    impl Clock for TestClock {
        type Zone = TestZone;

        fn now(&self) -> DateTime<Utc> {
            *self.wall.lock().unwrap()
        }

        fn awake(&self) -> Duration {
            *self.awake.lock().unwrap()
        }

        fn zone(&self) -> TestZone {
            *self.zone.lock().unwrap()
        }
    }

    /// The injected reachability source.
    struct TestNetwork(AtomicBool);

    impl TestNetwork {
        fn set(&self, online: bool) {
            self.0.store(online, Ordering::SeqCst);
        }
    }

    impl Reachability for TestNetwork {
        fn online(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    type Step = Box<dyn FnOnce() -> Result<String, String> + Send>;

    /// The injected jobs. Every run is written down; a scripted step decides
    /// how the next run ends, and a run with no step succeeds.
    #[derive(Default)]
    struct TestJobs {
        ran: Mutex<Vec<String>>,
        script: Mutex<VecDeque<Step>>,
    }

    impl TestJobs {
        fn next_run_will(&self, step: impl FnOnce() -> Result<String, String> + Send + 'static) {
            self.script.lock().unwrap().push_back(Box::new(step));
        }
    }

    #[async_trait::async_trait]
    impl JobRunner for TestJobs {
        async fn run(&self, run: &Run) -> Result<String, String> {
            let slot = run.slot.as_ref().map_or("-".to_string(), slots::format_slot);
            self.ran.lock().unwrap().push(format!("{} {}", run.trigger.as_str(), slot));
            let step = self.script.lock().unwrap().pop_front();
            match step {
                Some(step) => step(),
                None => Ok("done".to_string()),
            }
        }
    }

    /// One Mac: a database, a clock, a network and the engine over them.
    struct Sim {
        db: Arc<Database>,
        clock: Arc<TestClock>,
        network: Arc<TestNetwork>,
        jobs: Arc<TestJobs>,
        engine: Engine<TestClock>,
    }

    impl Sim {
        /// A Mac in Hong Kong with the network up and no jobs yet.
        fn start(now: &str) -> Sim {
            let db = Arc::new(crate::db::test_database());
            db.conn.lock().unwrap().execute_batch("DELETE FROM cron_runs; DELETE FROM cron_jobs;").unwrap();
            let clock = Arc::new(TestClock {
                wall: Mutex::new(at(now)),
                awake: Mutex::new(Duration::ZERO),
                zone: Mutex::new(TestZone::fixed(8)),
            });
            Sim::launch(db, clock, Arc::new(TestNetwork(AtomicBool::new(true))), Arc::new(TestJobs::default()))
        }

        fn launch(db: Arc<Database>, clock: Arc<TestClock>, network: Arc<TestNetwork>, jobs: Arc<TestJobs>) -> Sim {
            let engine = Engine::new(Arc::clone(&db), Arc::clone(&clock), network.clone(), jobs.clone());
            let rows: Vec<(i64, String, String)> = {
                let conn = db.conn.lock().unwrap();
                let mut stmt = conn.prepare("SELECT id, schedule, action_type FROM cron_jobs WHERE status = 'active'").unwrap();
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
                rows.collect::<Result<_, _>>().unwrap()
            };
            for (id, schedule, action) in rows {
                engine.register(id, &action, &schedule, &action).unwrap();
            }
            Sim { db, clock, network, jobs, engine }
        }

        /// JARVIS exits, cleanly or not, and is opened again: a new engine
        /// over the same database, as on any launch.
        fn relaunch(self) -> Sim {
            let Sim { db, clock, network, jobs, engine } = self;
            drop(engine);
            Sim::launch(db, clock, network, jobs)
        }

        /// A job created now, the way the app creates one.
        fn add_job(&self, action: &str, schedule: &str) -> i64 {
            let id = {
                let conn = self.db.conn.lock().unwrap();
                conn.execute(
                    "INSERT INTO cron_jobs (name, schedule, action_type, status, baseline_at) VALUES (?1, ?2, ?1, 'active', ?3)",
                    params![action, schedule, format_baseline(self.clock.now())],
                )
                .unwrap();
                conn.last_insert_rowid()
            };
            self.engine.register(id, action, schedule, action).unwrap();
            id
        }

        fn pause(&self, job_id: i64) {
            write_status(&self.db.conn.lock().unwrap(), job_id, false, self.clock.now()).unwrap();
            self.engine.unregister(job_id);
        }

        fn resume(&self, job_id: i64) {
            let (schedule, action): (String, String) = {
                let conn = self.db.conn.lock().unwrap();
                write_status(&conn, job_id, true, self.clock.now()).unwrap();
                conn.query_row("SELECT schedule, action_type FROM cron_jobs WHERE id = ?1", [job_id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .unwrap()
            };
            self.engine.register(job_id, &action, &schedule, &action).unwrap();
        }

        /// One tick, with every run it starts carried through to its end.
        async fn tick(&self) -> usize {
            let runs = self.engine.tick();
            let started = runs.len();
            for run in runs {
                self.engine.execute(run).await;
            }
            started
        }

        /// The Mac stays awake, ticking once a second, until the given time.
        async fn awake_until(&self, text: &str) {
            let end = at(text);
            assert!(end >= self.clock.now(), "awake_until goes forwards");
            while self.clock.now() < end {
                self.clock.pass(1);
                self.tick().await;
            }
        }

        async fn awake_for(&self, secs: u64) {
            for _ in 0..secs {
                self.clock.pass(1);
                self.tick().await;
            }
        }

        /// Every run the jobs were asked to do: "trigger slot".
        fn ran(&self) -> Vec<String> {
            self.jobs.ran.lock().unwrap().clone()
        }

        /// The job's `cron_runs` rows, oldest first: "status trigger slot".
        fn history(&self, job_id: i64) -> Vec<String> {
            let conn = self.db.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT status, COALESCE(\"trigger\", '-'), COALESCE(slot, '-') FROM cron_runs WHERE job_id = ?1 ORDER BY id")
                .unwrap();
            let rows = stmt
                .query_map([job_id], |r| Ok(format!("{} {} {}", r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
                .unwrap();
            rows.collect::<Result<_, _>>().unwrap()
        }

        fn column(&self, job_id: i64, column: &str) -> Option<String> {
            let conn = self.db.conn.lock().unwrap();
            conn.query_row(&format!("SELECT {column} FROM cron_jobs WHERE id = ?1"), [job_id], |r| r.get(0)).unwrap()
        }

        fn handled(&self, job_id: i64) -> Option<String> {
            self.column(job_id, "handled_slot")
        }

        /// What the collapsed rows of a job say, oldest first.
        fn collapsed(&self, job_id: i64) -> Vec<serde_json::Value> {
            let conn = self.db.conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT result FROM cron_runs WHERE job_id = ?1 AND status = 'collapsed' ORDER BY id").unwrap();
            let rows = stmt.query_map([job_id], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|text| serde_json::from_str(&text.unwrap()).unwrap()).collect()
        }

        /// What the "scheduled jobs failed" alert counts.
        fn failed_runs(&self) -> i64 {
            let conn = self.db.conn.lock().unwrap();
            conn.query_row("SELECT COUNT(*) FROM cron_runs WHERE status = 'failed'", [], |r| r.get(0)).unwrap()
        }
    }

    const DAILY_0900: &str = "0 0 9 * * *";
    const EVERY_5_MIN: &str = "0 */5 * * * *";
    const HOURLY: &str = "0 0 * * * *";

    // Case: new job, never run.
    #[tokio::test]
    async fn a_new_job_first_runs_at_its_next_slot() {
        // Created at 10:00, an hour after today's 09:00.
        let sim = Sim::start("2026-11-12T10:00:00+08:00");
        let job = sim.add_job("deadline_monitor", DAILY_0900);
        assert_eq!(sim.column(job, "next_run").as_deref(), Some("2026-11-13 01:00:00"), "next_run is written at registration");

        sim.awake_for(3600).await;
        assert!(sim.ran().is_empty(), "the 09:00 before the baseline is not owed");
        assert!(sim.history(job).is_empty());

        // The Mac sleeps overnight and is opened after the first real slot.
        sim.clock.sleep_until("2026-11-13T09:30:00+08:00");
        sim.tick().await;
        assert_eq!(sim.ran(), ["catch_up 2026-11-13T09:00:00+08:00"]);
        assert!(sim.collapsed(job).is_empty(), "nothing before the baseline collapses either");
    }

    // Case: Mac awake, on time.
    #[tokio::test]
    async fn an_on_time_slot_runs_once_as_scheduled() {
        let sim = Sim::start("2026-11-12T08:59:50+08:00");
        let job = sim.add_job("deadline_monitor", DAILY_0900);
        assert_eq!(sim.column(job, "next_run").as_deref(), Some("2026-11-12 01:00:00"));

        sim.awake_until("2026-11-12T08:59:59+08:00").await;
        assert!(sim.ran().is_empty());
        sim.awake_until("2026-11-12T09:10:00+08:00").await;

        assert_eq!(sim.ran(), ["scheduled 2026-11-12T09:00:00+08:00"]);
        assert_eq!(sim.history(job), ["completed scheduled 2026-11-12T09:00:00+08:00"]);
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T09:00:00+08:00"));
        assert_eq!(sim.column(job, "next_run").as_deref(), Some("2026-11-13 01:00:00"), "next_run is rewritten after the run");
        assert!(sim.column(job, "last_run").is_some());
    }

    // Case: slept through one or more slots.
    #[tokio::test]
    async fn sleeping_through_slots_runs_the_latest_once_and_collapses_the_rest() {
        let sim = Sim::start("2026-11-12T09:58:00+08:00");
        let job = sim.add_job("proactive_check", EVERY_5_MIN);
        sim.awake_until("2026-11-12T10:00:30+08:00").await;
        assert_eq!(sim.ran(), ["scheduled 2026-11-12T10:00:00+08:00"]);

        // Lid closed for three hours.
        sim.clock.sleep_until("2026-11-12T13:01:00+08:00");
        assert_eq!(sim.tick().await, 1, "one run on wake, not thirty-six");
        sim.awake_for(120).await;

        assert_eq!(sim.ran(), ["scheduled 2026-11-12T10:00:00+08:00", "catch_up 2026-11-12T13:00:00+08:00"]);
        assert_eq!(
            sim.collapsed(job),
            [serde_json::json!({
                "count": 35, "capped": false,
                "from": "2026-11-12T10:05:00+08:00", "to": "2026-11-12T12:55:00+08:00",
            })]
        );
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T13:00:00+08:00"));
        assert_eq!(sim.failed_runs(), 0);

        // The schedule carries on from there as normal.
        sim.awake_until("2026-11-12T13:05:30+08:00").await;
        assert_eq!(sim.ran().last().unwrap(), "scheduled 2026-11-12T13:05:00+08:00");
        assert_eq!(sim.ran().len(), 3);
    }

    /// A wake a few seconds after the slot is still a catch-up: this is the
    /// briefing that must be shown without voice.
    #[tokio::test]
    async fn a_slot_that_passed_during_a_short_sleep_is_a_catch_up() {
        let sim = Sim::start("2026-11-12T08:20:00+08:00");
        sim.add_job("daily_briefing", "0 30 8 * * *");
        sim.awake_until("2026-11-12T08:25:00+08:00").await;
        sim.clock.sleep_until("2026-11-12T08:30:20+08:00");
        sim.awake_for(5).await;
        assert_eq!(sim.ran(), ["catch_up 2026-11-12T08:30:00+08:00"]);
    }

    // Case: JARVIS quit through slots.
    #[tokio::test]
    async fn slots_missed_while_jarvis_was_quit_collapse_on_the_next_launch() {
        let sim = Sim::start("2026-11-09T08:59:00+08:00");
        let job = sim.add_job("deadline_monitor", DAILY_0900);
        sim.awake_until("2026-11-09T09:01:00+08:00").await;
        assert_eq!(sim.ran(), ["scheduled 2026-11-09T09:00:00+08:00"]);

        // Quit on the 9th, opened again on the afternoon of the 12th. The Mac
        // was awake in between, so only the wall clock tells.
        sim.clock.pass(3 * 86_400 + 6 * 3_600);
        let sim = sim.relaunch();
        assert_eq!(sim.tick().await, 1);
        sim.awake_for(600).await;

        assert_eq!(sim.ran(), ["scheduled 2026-11-09T09:00:00+08:00", "catch_up 2026-11-12T09:00:00+08:00"]);
        assert_eq!(
            sim.collapsed(job),
            [serde_json::json!({
                "count": 2, "capped": false,
                "from": "2026-11-10T09:00:00+08:00", "to": "2026-11-11T09:00:00+08:00",
            })]
        );
        assert_eq!(sim.failed_runs(), 0);
    }

    // Case: run failed (real error).
    #[tokio::test]
    async fn a_real_failure_is_final_and_is_not_run_again() {
        let sim = Sim::start("2026-11-12T09:59:00+08:00");
        let job = sim.add_job("email_sync", EVERY_5_MIN);
        // The network is up: a timeout is a real failure.
        sim.jobs.next_run_will(|| Err("Gmail request timed out".to_string()));
        sim.awake_until("2026-11-12T10:04:59+08:00").await;

        assert_eq!(sim.ran(), ["scheduled 2026-11-12T10:00:00+08:00"], "no immediate re-run");
        assert_eq!(sim.history(job), ["failed scheduled 2026-11-12T10:00:00+08:00"]);
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T10:00:00+08:00"), "a failure is final");
        assert_eq!(sim.failed_runs(), 1, "the existing failure alert applies");

        sim.awake_until("2026-11-12T10:05:30+08:00").await;
        assert_eq!(sim.ran().last().unwrap(), "scheduled 2026-11-12T10:05:00+08:00", "the next slot runs as usual");
    }

    // Case: network-required job, no network at wake; the network returns
    // inside the wait.
    #[tokio::test]
    async fn an_offline_wake_waits_and_runs_once_when_the_network_returns() {
        let sim = Sim::start("2026-11-12T09:58:00+08:00");
        let job = sim.add_job("email_sync", EVERY_5_MIN);
        sim.awake_until("2026-11-12T10:00:30+08:00").await;

        // Asleep for half an hour; Wi-Fi is not back yet when the lid opens.
        sim.network.set(false);
        sim.clock.sleep_until("2026-11-12T10:31:00+08:00");
        assert_eq!(sim.tick().await, 0, "nothing runs into a dead network");
        sim.awake_for(60).await;
        assert_eq!(sim.ran().len(), 1);
        assert_eq!(sim.history(job).len(), 1, "a wait shorter than five minutes writes nothing");

        // Wi-Fi joins. The job waits for it to settle, then runs once.
        sim.network.set(true);
        sim.awake_for(NETWORK_SETTLE_SECS - 1).await;
        assert_eq!(sim.ran().len(), 1, "not in the first seconds of a new connection");
        sim.awake_for(60).await;

        assert_eq!(sim.ran(), ["scheduled 2026-11-12T10:00:00+08:00", "catch_up 2026-11-12T10:30:00+08:00"]);
        assert_eq!(
            sim.history(job),
            [
                "completed scheduled 2026-11-12T10:00:00+08:00",
                "completed catch_up 2026-11-12T10:30:00+08:00",
                "collapsed - 2026-11-12T10:25:00+08:00",
            ]
        );
        assert_eq!(sim.collapsed(job)[0]["count"], 5);
        assert_eq!(sim.failed_runs(), 0, "no false failure after the wake");
    }

    // Case: network-required job, no network; it stays away past the wait.
    #[tokio::test]
    async fn a_wait_past_five_minutes_is_recorded_once_and_the_slot_still_runs() {
        let sim = Sim::start("2026-11-12T09:59:00+08:00");
        let job = sim.add_job("github_digest", HOURLY);
        sim.network.set(false);

        sim.awake_until("2026-11-12T10:04:59+08:00").await;
        assert!(sim.history(job).is_empty(), "still inside the five-minute wait");
        sim.awake_until("2026-11-12T10:05:01+08:00").await;
        assert_eq!(sim.history(job), ["skipped_offline - 2026-11-12T10:00:00+08:00"]);
        assert_eq!(sim.handled(job), None, "skipped_offline does not make the slot handled");

        // Still offline twenty minutes later, across a relaunch: one record, not one per tick.
        sim.awake_for(600).await;
        let sim = sim.relaunch();
        sim.awake_for(600).await;
        assert_eq!(sim.history(job), ["skipped_offline - 2026-11-12T10:00:00+08:00"]);
        assert!(sim.ran().is_empty());

        // Reachability turns online: the slot runs once.
        sim.network.set(true);
        sim.awake_for(60).await;
        assert_eq!(sim.ran(), ["catch_up 2026-11-12T10:00:00+08:00"]);
        assert_eq!(
            sim.history(job),
            ["skipped_offline - 2026-11-12T10:00:00+08:00", "completed catch_up 2026-11-12T10:00:00+08:00"]
        );
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T10:00:00+08:00"));
        assert_eq!(sim.failed_runs(), 0);
    }

    // Case: offline lasts past the next slot.
    #[tokio::test]
    async fn a_newer_slot_supersedes_one_that_stayed_offline() {
        let sim = Sim::start("2026-11-12T09:59:00+08:00");
        let job = sim.add_job("github_digest", HOURLY);
        sim.network.set(false);
        sim.awake_until("2026-11-12T10:59:59+08:00").await;
        assert_eq!(sim.history(job), ["skipped_offline - 2026-11-12T10:00:00+08:00"]);

        // 11:00 comes due with the network still down.
        sim.awake_until("2026-11-12T11:00:01+08:00").await;
        assert_eq!(sim.history(job), ["skipped_offline_final - 2026-11-12T10:00:00+08:00"]);
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T10:00:00+08:00"), "the old slot is final");

        sim.network.set(true);
        sim.awake_for(120).await;
        assert_eq!(sim.ran(), ["catch_up 2026-11-12T11:00:00+08:00"], "the new slot runs once; the old one never does");
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T11:00:00+08:00"));
        assert!(sim.collapsed(job).is_empty(), "a slot recorded as skipped is not counted as collapsed too");
        assert_eq!(sim.failed_runs(), 0);
    }

    /// The same, for a job whose next slot arrives just as the wait ends.
    #[tokio::test]
    async fn every_slot_missed_offline_while_awake_is_closed_as_the_next_arrives() {
        let sim = Sim::start("2026-11-12T09:59:00+08:00");
        let job = sim.add_job("email_sync", EVERY_5_MIN);
        sim.network.set(false);
        sim.awake_until("2026-11-12T10:11:00+08:00").await;
        assert_eq!(
            sim.history(job),
            ["skipped_offline_final - 2026-11-12T10:00:00+08:00", "skipped_offline_final - 2026-11-12T10:05:00+08:00"]
        );

        sim.network.set(true);
        sim.awake_for(60).await;
        assert_eq!(sim.ran(), ["catch_up 2026-11-12T10:10:00+08:00"]);
        assert_eq!(sim.failed_runs(), 0);
    }

    /// The other half of the classification, end to end: the same error is a
    /// failure with the network up (above) and not one with it down.
    #[tokio::test]
    async fn a_run_that_errors_with_the_network_down_is_retried_not_failed() {
        let sim = Sim::start("2026-11-12T09:59:00+08:00");
        let job = sim.add_job("email_sync", HOURLY);
        let network = Arc::clone(&sim.network);
        sim.jobs.next_run_will(move || {
            network.set(false);
            Err("Gmail request timed out".to_string())
        });
        sim.awake_until("2026-11-12T10:00:30+08:00").await;
        assert_eq!(sim.history(job), ["skipped_offline scheduled 2026-11-12T10:00:00+08:00"]);
        assert_eq!(sim.handled(job), None);

        sim.network.set(true);
        sim.awake_for(120).await;
        assert_eq!(sim.ran(), ["scheduled 2026-11-12T10:00:00+08:00", "catch_up 2026-11-12T10:00:00+08:00"]);
        assert_eq!(
            sim.history(job),
            ["skipped_offline scheduled 2026-11-12T10:00:00+08:00", "completed catch_up 2026-11-12T10:00:00+08:00"]
        );
        assert_eq!(sim.failed_runs(), 0);
    }

    /// A network that keeps dropping does not get a run every few seconds:
    /// the pause doubles after each run that ends offline.
    #[tokio::test]
    async fn runs_that_keep_ending_offline_are_spaced_out() {
        let sim = Sim::start("2026-11-12T09:59:55+08:00");
        let job = sim.add_job("email_sync", HOURLY);
        for _ in 0..4 {
            let network = Arc::clone(&sim.network);
            sim.jobs.next_run_will(move || {
                network.set(false);
                Err("connection lost".to_string())
            });
        }
        // The link is back by the next tick every time, so reachability alone
        // would allow a run each second.
        let mut runs_by = Vec::new();
        for (until, _) in [("10:00:04", 1), ("10:00:05", 2), ("10:00:14", 2), ("10:00:15", 3), ("10:00:35", 4), ("10:01:14", 4), ("10:01:15", 5)] {
            let end = at(&format!("2026-11-12T{until}+08:00"));
            while sim.clock.now() < end {
                sim.clock.pass(1);
                sim.tick().await;
                sim.network.set(true);
            }
            runs_by.push(sim.ran().len());
        }
        assert_eq!(runs_by, [1, 2, 2, 3, 4, 4, 5], "5 s, 10 s, 20 s, then 40 s apart");
        assert_eq!(sim.history(job).iter().filter(|row| row.starts_with("skipped_offline ")).count(), 4);
        assert_eq!(sim.history(job).last().unwrap(), "completed catch_up 2026-11-12T10:00:00+08:00");
        assert_eq!(sim.failed_runs(), 0);
    }

    // Case: crash or force-quit mid-run.
    #[tokio::test]
    async fn a_crash_mid_run_is_retried_once() {
        let sim = Sim::start("2026-11-12T08:59:58+08:00");
        let job = sim.add_job("deadline_monitor", DAILY_0900);
        sim.awake_until("2026-11-12T08:59:59+08:00").await;

        // The run starts and JARVIS is force-quit before it ends.
        sim.clock.pass(1);
        let started = sim.engine.tick();
        assert_eq!(started.len(), 1);
        drop(started);
        assert_eq!(sim.history(job), ["running scheduled 2026-11-12T09:00:00+08:00"]);

        sim.clock.pass(30);
        let sim = sim.relaunch();
        assert_eq!(sim.history(job), ["interrupted scheduled 2026-11-12T09:00:00+08:00"]);
        assert_eq!(sim.tick().await, 1);
        sim.awake_for(600).await;

        assert_eq!(
            sim.history(job),
            ["interrupted scheduled 2026-11-12T09:00:00+08:00", "completed retry_interrupted 2026-11-12T09:00:00+08:00"]
        );
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T09:00:00+08:00"));
        assert_eq!(sim.failed_runs(), 0, "a force-quit is not a failed job");
    }

    /// Never more than one retry per slot.
    #[tokio::test]
    async fn a_retry_that_is_interrupted_too_is_not_retried_again() {
        let sim = Sim::start("2026-11-12T08:59:59+08:00");
        let job = sim.add_job("deadline_monitor", DAILY_0900);
        sim.clock.pass(1);
        drop(sim.engine.tick());

        sim.clock.pass(30);
        let sim = sim.relaunch();
        let retry = sim.engine.tick();
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].trigger, Trigger::RetryInterrupted);
        drop(retry);

        sim.clock.pass(30);
        let sim = sim.relaunch();
        sim.awake_for(600).await;
        assert!(sim.ran().is_empty(), "the slot is not run a third time");
        assert_eq!(
            sim.history(job),
            ["interrupted scheduled 2026-11-12T09:00:00+08:00", "failed retry_interrupted 2026-11-12T09:00:00+08:00"]
        );
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T09:00:00+08:00"));

        // The next day's slot is a new slot and runs as usual.
        sim.clock.sleep_until("2026-11-13T08:59:00+08:00");
        sim.awake_until("2026-11-13T09:00:30+08:00").await;
        assert_eq!(sim.ran(), ["scheduled 2026-11-13T09:00:00+08:00"]);
    }

    /// A run that was in flight when the lid closed did not fail: its
    /// connection died with the sleep. It is retried like any interrupted run.
    #[tokio::test]
    async fn a_run_the_mac_slept_under_is_retried_not_failed() {
        let sim = Sim::start("2026-11-12T09:59:00+08:00");
        let job = sim.add_job("email_sync", HOURLY);
        let clock = Arc::clone(&sim.clock);
        sim.jobs.next_run_will(move || {
            clock.sleep_until("2026-11-12T10:20:00+08:00");
            Err("connection reset".to_string())
        });
        sim.awake_until("2026-11-12T10:21:00+08:00").await;

        assert_eq!(
            sim.history(job),
            ["interrupted scheduled 2026-11-12T10:00:00+08:00", "completed retry_interrupted 2026-11-12T10:00:00+08:00"]
        );
        assert_eq!(sim.failed_runs(), 0, "no false failure after the wake");
    }

    // Case: paused, then re-enabled.
    #[tokio::test]
    async fn re_enabling_a_paused_job_does_not_catch_up() {
        let sim = Sim::start("2026-11-12T09:58:00+08:00");
        let job = sim.add_job("proactive_check", EVERY_5_MIN);
        sim.awake_until("2026-11-12T10:01:00+08:00").await;
        sim.pause(job);
        assert_eq!(sim.column(job, "next_run"), None);

        // Paused for the rest of the morning, part of it asleep.
        sim.awake_until("2026-11-12T10:30:00+08:00").await;
        sim.clock.sleep_until("2026-11-12T12:02:00+08:00");
        sim.awake_for(10).await;
        assert_eq!(sim.ran().len(), 1, "a paused job does not run");

        sim.resume(job);
        assert_eq!(sim.column(job, "baseline_at").as_deref(), Some("2026-11-12T04:02:10Z"), "the baseline is the re-enable time");
        sim.awake_for(60).await;
        assert_eq!(sim.ran().len(), 1, "nothing from the paused period is owed");
        assert!(sim.collapsed(job).is_empty(), "and nothing from it is recorded as collapsed");

        sim.awake_until("2026-11-12T12:05:30+08:00").await;
        assert_eq!(sim.ran().last().unwrap(), "scheduled 2026-11-12T12:05:00+08:00");
        assert_eq!(sim.ran().len(), 2);
    }

    // Case: manual "run now".
    #[tokio::test]
    async fn a_manual_run_does_not_change_the_handled_slot() {
        let sim = Sim::start("2026-11-12T08:00:00+08:00");
        let job = sim.add_job("deadline_monitor", DAILY_0900);
        sim.awake_until("2026-11-12T08:59:00+08:00").await;

        let run = sim.engine.run_now(job).unwrap();
        assert!(sim.engine.run_now(job).is_err(), "not twice at once");
        sim.engine.execute(run).await;
        assert_eq!(sim.history(job), ["completed manual -"]);
        assert_eq!(sim.handled(job), None);

        // The 09:00 slot is still owed and runs at its time.
        sim.awake_until("2026-11-12T09:01:00+08:00").await;
        assert_eq!(sim.ran(), ["manual -", "scheduled 2026-11-12T09:00:00+08:00"]);
        assert_eq!(sim.handled(job).as_deref(), Some("2026-11-12T09:00:00+08:00"));
    }

    /// Local jobs never wait for the network: task alerts and reminders fire
    /// on an offline wake.
    #[tokio::test]
    async fn a_local_job_runs_on_an_offline_wake() {
        let sim = Sim::start("2026-11-12T09:58:00+08:00");
        let local = sim.add_job("proactive_check", EVERY_5_MIN);
        let remote = sim.add_job("email_sync", EVERY_5_MIN);
        sim.awake_until("2026-11-12T10:00:30+08:00").await;

        sim.network.set(false);
        sim.clock.sleep_until("2026-11-12T10:31:00+08:00");
        assert_eq!(sim.tick().await, 1);
        assert_eq!(sim.ran().last().unwrap(), "catch_up 2026-11-12T10:30:00+08:00");
        assert!(sim.history(local).contains(&"completed catch_up 2026-11-12T10:30:00+08:00".to_string()));
        assert_eq!(sim.history(remote), ["completed scheduled 2026-11-12T10:00:00+08:00"], "the sync waits");
    }

    // Case: UTC offset change (travel). The daily job is re-registered on the
    // new offset and the local slot it already ran is not run again.
    #[tokio::test]
    async fn a_flight_west_does_not_run_the_same_local_slot_twice() {
        let sim = Sim::start("2026-11-12T08:59:00+08:00");
        let job = sim.add_job("deadline_monitor", DAILY_0900);
        sim.awake_until("2026-11-12T09:01:00+08:00").await;
        assert_eq!(sim.column(job, "next_run").as_deref(), Some("2026-11-13 01:00:00"), "tomorrow 09:00, Hong Kong");

        // The Mac's zone becomes Los Angeles while it is awake. There it is
        // still the 11th.
        sim.clock.set_zone(TestZone::fixed(-8));
        sim.awake_for(2).await;
        assert_eq!(
            sim.column(job, "next_run").as_deref(),
            Some("2026-11-13 17:00:00"),
            "re-registered: 09:00 on the 13th in Los Angeles, because the 12th's 09:00 already ran"
        );

        // 09:00 on the 12th, Los Angeles time, comes and goes.
        sim.clock.sleep_until("2026-11-12T08:59:00-08:00");
        sim.awake_until("2026-11-12T09:05:00-08:00").await;
        assert_eq!(sim.ran(), ["scheduled 2026-11-12T09:00:00+08:00"], "the same local slot does not run twice");

        // The 13th's 09:00 is a new slot.
        sim.clock.sleep_until("2026-11-13T08:59:00-08:00");
        sim.awake_until("2026-11-13T09:01:00-08:00").await;
        assert_eq!(sim.ran(), ["scheduled 2026-11-12T09:00:00+08:00", "scheduled 2026-11-13T09:00:00-08:00"]);
    }

    /// Jobs whose slots are an hour or less apart follow real time across the
    /// same flight: the sync and the alerts keep going.
    #[tokio::test]
    async fn a_frequent_job_keeps_running_after_a_flight_west() {
        let sim = Sim::start("2026-11-12T09:58:00+08:00");
        sim.add_job("proactive_check", EVERY_5_MIN);
        sim.awake_until("2026-11-12T10:00:30+08:00").await;

        sim.clock.set_zone(TestZone::fixed(-8));
        sim.awake_until("2026-11-12T10:05:30+08:00").await;
        assert_eq!(sim.ran(), ["scheduled 2026-11-12T10:00:00+08:00", "scheduled 2026-11-11T18:05:00-08:00"]);
    }

    // Case: UTC offset change (daylight saving). London, 25 October 2026:
    // 01:30 happens twice and the 01:30 job runs once.
    #[tokio::test]
    async fn the_hour_repeated_by_daylight_saving_runs_a_set_time_job_once() {
        let sim = Sim::start("2026-10-25T01:29:00+01:00");
        sim.clock.set_zone(TestZone::changing("2026-10-25T01:00:00", 1, 0));
        let sim = sim.relaunch();
        sim.add_job("deadline_monitor", "0 30 1 * * *");
        sim.awake_until("2026-10-25T03:00:00+00:00").await;
        assert_eq!(sim.ran(), ["scheduled 2026-10-25T01:30:00+01:00"]);
    }

    #[test]
    fn a_wake_is_wall_clock_time_that_awake_time_did_not_cover() {
        let secs = TimeDelta::seconds;
        assert!(!wall_clock_jumped(secs(1), Duration::from_secs(1)), "an ordinary tick");
        assert!(!wall_clock_jumped(secs(3), Duration::from_secs(1)), "a small clock correction");
        assert!(wall_clock_jumped(secs(600), Duration::from_secs(1)), "ten minutes asleep");
        assert!(wall_clock_jumped(secs(-3600), Duration::from_secs(1)), "the clock set back an hour");
    }
}
