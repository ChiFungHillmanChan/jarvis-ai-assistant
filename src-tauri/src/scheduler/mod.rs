pub mod daily_briefing;
pub mod engine;
pub mod jobs;
pub mod network;
pub mod slots;

use crate::auth::google::GoogleAuth;
use crate::db::Database;
use engine::{Engine, JobRunner, Run, SystemClock};
use std::sync::{Arc, Mutex};

/// The app's own jobs, as the engine runs them.
struct AppJobs {
    db: Arc<Database>,
    google_auth: Arc<GoogleAuth>,
}

#[async_trait::async_trait]
impl JobRunner for AppJobs {
    async fn run(&self, run: &Run) -> Result<String, String> {
        jobs::run_action(&self.db, &self.google_auth, &run.action_type).await
    }
}

/// How often the engine looks at the jobs. A slot is noticed within this long
/// of coming due, and a wake within this long of the Mac waking.
const TICK: std::time::Duration = std::time::Duration::from_secs(1);

pub struct Scheduler {
    engine: Arc<Engine<SystemClock>>,
    db: Arc<Database>,
}

impl Scheduler {
    pub async fn new(db: Arc<Database>, google_auth: Arc<GoogleAuth>) -> Result<Self, String> {
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
            Arc::new(AppJobs { db: Arc::clone(&db), google_auth }),
        );

        let job_rows: Vec<(i64, String, String, String)> = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare("SELECT id, name, schedule, action_type FROM cron_jobs WHERE status = 'active'").map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            rows
        };

        let this = Scheduler { engine: Arc::new(engine), db };

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
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                for run in engine.tick() {
                    let engine = Arc::clone(&engine);
                    tauri::async_runtime::spawn(async move { engine.execute(run).await });
                }
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
}
