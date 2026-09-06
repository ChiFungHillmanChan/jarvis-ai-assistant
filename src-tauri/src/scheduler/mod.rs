pub mod daily_briefing;
pub mod jobs;

use crate::auth::google::GoogleAuth;
use crate::db::Database;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_cron_scheduler::{Job, JobScheduler};
use uuid::Uuid;

pub struct Scheduler {
    scheduler: JobScheduler,
    db: Arc<Database>,
    google_auth: Arc<GoogleAuth>,
    /// cron_jobs.id -> its live JobScheduler registration. Registration used to
    /// happen only here at startup, so a job the user created, paused or
    /// deleted in the dashboard did not take effect until the next launch.
    registered: Mutex<HashMap<i64, Uuid>>,
}

impl Scheduler {
    pub async fn new(db: Arc<Database>, google_auth: Arc<GoogleAuth>) -> Result<Self, String> {
        let scheduler = JobScheduler::new().await.map_err(|e| format!("Failed to create scheduler: {}", e))?;

        // Reduce proactive check from every 1 min to every 5 min to lower DB contention
        {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let _ = conn.execute(
                "UPDATE cron_jobs SET schedule = '0 */5 * * * *' WHERE action_type = 'proactive_check' AND schedule = '0 */1 * * * *'",
                [],
            );
        }

        let job_rows: Vec<(i64, String, String, String, Option<String>)> = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn.prepare("SELECT id, name, schedule, action_type, parameters FROM cron_jobs WHERE status = 'active'").map_err(|e| e.to_string())?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            rows
        };

        let this = Scheduler {
            scheduler,
            db,
            google_auth,
            registered: Mutex::new(HashMap::new()),
        };

        for (job_id, name, schedule, action_type, _params) in job_rows {
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

    /// Wire a `cron_jobs` row into the running scheduler, replacing any earlier
    /// registration for the same row.
    pub async fn register(
        &self,
        job_id: i64,
        name: &str,
        schedule: &str,
        action_type: &str,
    ) -> Result<(), String> {
        self.unregister(job_id).await;

        let db_clone = Arc::clone(&self.db);
        let auth_clone = Arc::clone(&self.google_auth);
        let action_type = action_type.to_string();
        // Local, not UTC: the schedules are written by the user ("9am") and the
        // dashboard already renders upcoming runs in local time, so a UTC
        // registration fired the job at the wrong hour everywhere but London.
        let job = Job::new_async_tz(schedule, chrono::Local, move |_uuid, _lock| {
            let db = Arc::clone(&db_clone);
            let auth = Arc::clone(&auth_clone);
            let action = action_type.clone();
            let jid = job_id;
            Box::pin(async move {
                log::info!("Running cron job {}: {}", jid, action);
                let result = jobs::run_job(&db, &auth, &action, jid).await;
                match &result {
                    Ok(msg) => log::info!("Job {} completed: {}", jid, msg),
                    Err(e) => log::error!("Job {} failed: {}", jid, e),
                }
            })
        }).map_err(|e| format!("Failed to create job '{}': {}", name, e))?;
        let uuid = self.scheduler.add(job).await.map_err(|e| e.to_string())?;
        match self.registered.lock() {
            Ok(mut map) => { map.insert(job_id, uuid); }
            Err(e) => log::error!("Scheduler registry lock poisoned: {}", e),
        }
        log::info!("Registered cron job: {} ({})", name, schedule);
        Ok(())
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
        let uuid = match self.registered.lock() {
            Ok(mut map) => map.remove(&job_id),
            Err(e) => {
                log::error!("Scheduler registry lock poisoned: {}", e);
                None
            }
        };
        if let Some(uuid) = uuid {
            match self.scheduler.remove(&uuid).await {
                Ok(()) => log::info!("Unregistered cron job {}", job_id),
                Err(e) => log::warn!("Failed to unregister cron job {}: {}", job_id, e),
            }
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        self.scheduler.start().await.map_err(|e| format!("Failed to start scheduler: {}", e))?;
        log::info!("Cron scheduler started");
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
