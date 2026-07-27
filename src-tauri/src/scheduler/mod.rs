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
            this.register(job_id, &name, &schedule, &action_type).await?;
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
        let job = Job::new_async(schedule, move |_uuid, _lock| {
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
