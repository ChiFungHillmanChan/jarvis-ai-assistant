pub mod daily_briefing;
pub mod engine;
pub mod jobs;
pub mod network;
pub mod scheduled_briefing;
pub mod slots;

use crate::auth::google::GoogleAuth;
use crate::db::Database;
use engine::{Engine, JobRunner, Run, SystemClock};
use scheduled_briefing::BriefingService;
use std::sync::{Arc, Mutex};

/// The app's own jobs, as the engine runs them.
struct AppJobs {
    db: Arc<Database>,
    google_auth: Arc<GoogleAuth>,
    briefing: Arc<BriefingService>,
}

#[async_trait::async_trait]
impl JobRunner for AppJobs {
    async fn run(&self, run: &Run) -> Result<String, String> {
        // The briefing is the one job that behaves differently by run: only
        // an on-time run may speak, and only while no sleep separates it from
        // the moment the engine started it; and a run for a day that is over
        // shows nothing.
        if run.action_type == scheduled_briefing::ACTION_TYPE {
            return self.briefing.run(scheduled_briefing::Occasion::of(run)).await;
        }
        jobs::run_action(&self.db, &self.google_auth, &run.action_type).await
    }
}

/// How often the engine looks at the jobs. A slot is noticed within this long
/// of coming due, and a wake within this long of the Mac waking.
const TICK: std::time::Duration = std::time::Duration::from_secs(1);

pub struct Scheduler {
    engine: Arc<Engine<SystemClock>>,
    db: Arc<Database>,
    briefing: Arc<BriefingService>,
}

impl Scheduler {
    pub async fn new(db: Arc<Database>, google_auth: Arc<GoogleAuth>, briefing: Arc<BriefingService>) -> Result<Self, String> {
        // Reduce proactive check from every 1 min to every 5 min to lower DB contention
        {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let _ = conn.execute(
                "UPDATE cron_jobs SET schedule = '0 */5 * * * *' WHERE action_type = 'proactive_check' AND schedule = '0 */1 * * * *'",
                [],
            );
        }

        // Built before any job is registered: this is where runs orphaned by
        // the last exit are resolved, and where "launch" is timed from.
        let engine = Engine::new(
            Arc::clone(&db),
            Arc::new(SystemClock::new()),
            Arc::new(network::SystemReachability),
            Arc::new(AppJobs { db: Arc::clone(&db), google_auth, briefing: Arc::clone(&briefing) }),
        );

        let job_rows: Vec<(i64, String, String, String)> = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare("SELECT id, name, schedule, action_type FROM cron_jobs WHERE status = 'active'").map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            rows
        };

        let this = Scheduler { engine: Arc::new(engine), db, briefing };

        for (job_id, name, schedule, action_type) in job_rows {
            // One unparseable row used to abort Scheduler::new with `?`, which
            // left every other job unregistered and the app running with no
            // scheduler at all. Park the offending row and keep going.
            if let Err(err) = this.register(job_id, &name, &schedule, &action_type).await {
                log::error!("Cron job {} ('{}') could not be scheduled: {}", job_id, name, err);
                this.park_unschedulable(job_id, &err);
            }
        }
        Ok(this)
    }

    /// Put a `cron_jobs` row under the running scheduler, replacing any earlier
    /// registration for the same row. Registration used to happen only at
    /// startup, so a job the user created, paused or deleted in the dashboard
    /// did not take effect until the next launch.
    pub async fn register(
        &self,
        job_id: i64,
        name: &str,
        schedule: &str,
        action_type: &str,
    ) -> Result<(), String> {
        self.engine.register(job_id, name, schedule, action_type)
    }

    /// Move a row the scheduler refused out of the active set and record why,
    /// so the dashboard shows the reason instead of reporting it as active and
    /// the next launch does not trip over the same row. Shared with the test
    /// below.
    const PARK_UNSCHEDULABLE_QUERY: &'static str =
        "UPDATE cron_jobs SET status = 'paused', description = TRIM(COALESCE(description, '') || ' ' || ?1) WHERE id = ?2";

    fn park_unschedulable(&self, job_id: i64, err: &str) {
        let note = format!("Schedule error: {}", err);
        match self.db.conn.lock() {
            Ok(conn) => {
                if let Err(e) = conn.execute(
                    Self::PARK_UNSCHEDULABLE_QUERY,
                    rusqlite::params![note, job_id],
                ) {
                    log::error!("Could not pause unschedulable cron job {}: {}", job_id, e);
                }
            }
            Err(e) => log::error!("Database lock poisoned pausing cron job {}: {}", job_id, e),
        }
    }

    /// Drop a job's live registration. A job that was never registered (paused
    /// at startup, or created while the scheduler was still booting) is not an
    /// error -- the caller only wants it gone.
    pub async fn unregister(&self, job_id: i64) {
        self.engine.unregister(job_id);
    }

    /// Start the tick. The first one runs at once, and that is the launch
    /// catch-up: every registered job is asked for its missed slot.
    pub async fn start(&self) -> Result<(), String> {
        let engine = Arc::clone(&self.engine);
        let briefing = Arc::clone(&self.briefing);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                for run in engine.tick() {
                    let engine = Arc::clone(&engine);
                    tauri::async_runtime::spawn(async move { engine.execute(run).await });
                }
                // A briefing shown while offline is owed its AI summary; the
                // same settled network the jobs wait for is what it waits for.
                briefing.on_tick(engine.online());
            }
        });
        log::info!("Cron scheduler started");
        Ok(())
    }

    /// Run a job now, outside its schedule. Returns once the run has started.
    pub fn run_now(&self, job_id: i64) -> Result<(), String> {
        let run = self.engine.run_now(job_id)?;
        let engine = Arc::clone(&self.engine);
        tauri::async_runtime::spawn(async move { engine.execute(run).await });
        Ok(())
    }
}

/// Handle to the live scheduler, published into Tauri state at startup. The
/// `JobScheduler` can only be built inside the async runtime, so commands reach
/// it through this holder, which is filled in once the scheduler is up.
#[derive(Default)]
pub struct SchedulerHandle {
    inner: Mutex<Option<Arc<Scheduler>>>,
}

impl SchedulerHandle {
    pub fn new() -> Self {
        SchedulerHandle { inner: Mutex::new(None) }
    }

    pub fn set(&self, scheduler: Arc<Scheduler>) {
        match self.inner.lock() {
            Ok(mut slot) => *slot = Some(scheduler),
            Err(e) => log::error!("Scheduler handle lock poisoned: {}", e),
        }
    }

    /// The live scheduler, or `None` while it is still starting up.
    pub fn get(&self) -> Option<Arc<Scheduler>> {
        self.inner.lock().ok().and_then(|slot| slot.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// A row the scheduler refuses must stop calling itself active and must say
    /// why. Before this, the first unparseable schedule aborted Scheduler::new
    /// and no job ran at all.
    #[test]
    fn unschedulable_row_is_paused_with_the_reason() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE cron_jobs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL,
                 schedule TEXT NOT NULL,
                 action_type TEXT NOT NULL,
                 status TEXT DEFAULT 'active',
                 description TEXT);
             INSERT INTO cron_jobs (name, schedule, action_type, status, description) VALUES
                 ('Friday digest', 'every friday', 'email_sync', 'active', 'Every Friday'),
                 ('Email Sync', '0 */5 * * * *', 'email_sync', 'active', 'Every 5 minutes');",
        )
        .unwrap();

        let note = format!("Schedule error: {}", "Failed to create job 'Friday digest': ParseSchedule");
        assert_eq!(
            conn.execute(Scheduler::PARK_UNSCHEDULABLE_QUERY, rusqlite::params![note, 1i64]).unwrap(),
            1
        );

        let (status, description): (String, String) = conn
            .query_row("SELECT status, description FROM cron_jobs WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(status, "paused", "a job nothing can run must not report active");
        assert_eq!(description, "Every Friday Schedule error: Failed to create job 'Friday digest': ParseSchedule");

        let (other_status, other_desc): (String, String) = conn
            .query_row("SELECT status, description FROM cron_jobs WHERE id = 2", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(other_status, "active", "the remaining jobs keep running");
        assert_eq!(other_desc, "Every 5 minutes");
    }

    /// A database from the last release (V32), upgraded in place.
    fn upgraded(prepare: impl FnOnce(&Connection)) -> Database {
        let directory = std::env::temp_dir().join(format!("jarvis-scheduler-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("jarvis.db");
        {
            let db = crate::db::test_database_at_version(&path, 32);
            prepare(&db.conn.lock().unwrap());
        }
        let db = crate::db::test_database_at(&path);
        std::fs::remove_dir_all(&directory).unwrap();
        db
    }

    /// The first launch after upgrading must not catch up history: every
    /// existing job starts counting from the moment of the upgrade.
    #[test]
    fn upgrading_starts_every_existing_job_from_now() {
        let before = chrono::Utc::now() - chrono::TimeDelta::seconds(2);
        let db = upgraded(|conn| {
            conn.execute("UPDATE cron_jobs SET last_run = '2026-01-01 09:00:00'", []).unwrap();
            conn.execute(
                "INSERT INTO cron_runs (job_id, status, started_at, finished_at) SELECT id, 'completed', '2026-01-01 09:00:00', '2026-01-01 09:00:05' FROM cron_jobs LIMIT 1",
                [],
            )
            .unwrap();
        });
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT baseline_at, handled_slot FROM cron_jobs").unwrap();
        let rows: Vec<(String, Option<String>)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect();
        assert!(rows.len() > 1, "the seeded jobs");
        for (baseline_at, handled_slot) in rows {
            let baseline = chrono::DateTime::parse_from_rfc3339(&baseline_at).unwrap();
            assert!(baseline >= before && baseline <= chrono::Utc::now() + chrono::TimeDelta::seconds(2), "{baseline_at}");
            assert_eq!(handled_slot, None);
        }
        let (trigger, slot): (Option<String>, Option<String>) =
            conn.query_row("SELECT \"trigger\", slot FROM cron_runs", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((trigger, slot), (None, None), "runs from before the upgrade keep no trigger or slot");
    }

    fn briefing_job(db: &Database) -> (String, String) {
        db.conn
            .lock()
            .unwrap()
            .query_row("SELECT schedule, status FROM cron_jobs WHERE action_type = 'daily_briefing'", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
    }

    fn preference(db: &Database, key: &str) -> Option<String> {
        use rusqlite::OptionalExtension;
        db.conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM user_preferences WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .unwrap()
    }

    /// The scheduled briefing is off after an upgrade, at 08:30, for everyone
    /// who never turned on the briefing that was spoken at startup.
    #[test]
    fn upgrading_leaves_the_briefing_off_by_default() {
        for old_setting in [None, Some("false")] {
            let db = upgraded(|conn| {
                conn.execute("DELETE FROM user_preferences WHERE key = 'auto_briefing'", []).unwrap();
                if let Some(value) = old_setting {
                    conn.execute("INSERT INTO user_preferences (key, value) VALUES ('auto_briefing', ?1)", [value]).unwrap();
                }
            });
            assert_eq!(briefing_job(&db), ("0 30 8 * * *".to_string(), "paused".to_string()), "{old_setting:?}");
            assert_eq!(preference(&db, "auto_briefing"), None, "the old setting is gone");
        }
    }

    /// Someone who had "speak a briefing at startup" on is moved to the
    /// scheduled briefing, on, and is not briefed a second time today.
    #[test]
    fn upgrading_moves_a_startup_briefing_user_to_the_scheduled_one() {
        let db = upgraded(|conn| {
            conn.execute_batch(
                "INSERT INTO user_preferences (key, value) VALUES ('auto_briefing', 'true')
                     ON CONFLICT(key) DO UPDATE SET value = 'true';
                 INSERT INTO user_preferences (key, value) VALUES ('last_briefing_date', '2026-11-12')
                     ON CONFLICT(key) DO UPDATE SET value = '2026-11-12';",
            )
            .unwrap();
        });
        assert_eq!(briefing_job(&db), ("0 30 8 * * *".to_string(), "active".to_string()));
        assert_eq!(preference(&db, "auto_briefing"), None);
        assert_eq!(preference(&db, "daily_briefing_last_date").as_deref(), Some("2026-11-12"));
        let settings = scheduled_briefing::read_settings(&db.conn.lock().unwrap()).unwrap();
        assert!(settings.enabled && settings.voice && settings.time == "08:30");
    }
}
