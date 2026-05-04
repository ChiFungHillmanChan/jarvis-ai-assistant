use crate::db::Database;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    Volume,
    Tools,
    Premium,
}

pub struct WorkloadRouter {
    db: Arc<Database>,
}

impl WorkloadRouter {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn model_for(&self, w: Workload) -> Result<String, String> {
        let key = match w {
            Workload::Volume => "volume",
            Workload::Tools => "tools",
            Workload::Premium => "premium",
        };
        let conn = self.db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
        conn.query_row(
            "SELECT model_id FROM workload_routing WHERE workload = ?1 AND enabled = 1",
            rusqlite::params![key],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| format!("workload lookup: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn fresh_db_with_routing() -> Arc<Database> {
        let path = std::env::temp_dir()
            .join(format!("jarvis-test-{}.db", uuid::Uuid::new_v4()));
        let conn = Connection::open(&path).expect("open temp db");
        // Mirrors V13 workload_routing DDL -- update if that table's schema changes.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workload_routing (
                workload TEXT PRIMARY KEY CHECK (workload IN ('volume', 'tools', 'premium')),
                model_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1
            );
             INSERT OR IGNORE INTO workload_routing (workload, model_id) VALUES
               ('volume',  'apac.amazon.nova-micro-v1:0'),
               ('tools',   'global.anthropic.claude-haiku-4-5-20251001-v1:0'),
               ('premium', 'global.anthropic.claude-sonnet-4-6');",
        )
        .expect("seed routing");
        Arc::new(Database {
            conn: Mutex::new(conn),
        })
    }

    #[tokio::test]
    async fn defaults_match_v13_seed() {
        let db = fresh_db_with_routing();
        let r = WorkloadRouter::new(db);
        assert_eq!(
            r.model_for(Workload::Volume).await.unwrap(),
            "apac.amazon.nova-micro-v1:0"
        );
        assert_eq!(
            r.model_for(Workload::Tools).await.unwrap(),
            "global.anthropic.claude-haiku-4-5-20251001-v1:0"
        );
        assert_eq!(
            r.model_for(Workload::Premium).await.unwrap(),
            "global.anthropic.claude-sonnet-4-6"
        );
    }
}
