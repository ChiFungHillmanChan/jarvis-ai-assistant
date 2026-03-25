pub mod jobs;

use crate::auth::google::GoogleAuth;
use crate::db::Database;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

pub struct Scheduler {
    scheduler: JobScheduler,
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

        for (job_id, name, schedule, action_type, _params) in job_rows {
            let db_clone = Arc::clone(&db);
            let auth_clone = Arc::clone(&google_auth);
            let job = Job::new_async(schedule.as_str(), move |_uuid, _lock| {
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
            scheduler.add(job).await.map_err(|e| e.to_string())?;
            log::info!("Registered cron job: {} ({})", name, schedule);
        }
        Ok(Scheduler { scheduler })
    }

    pub async fn start(&self) -> Result<(), String> {
        self.scheduler.start().await.map_err(|e| format!("Failed to start scheduler: {}", e))?;
        log::info!("Cron scheduler started");
        Ok(())
    }
}
