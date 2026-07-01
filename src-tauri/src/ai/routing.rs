use crate::db::Database;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    Volume,
    Tools,
    Premium,
}

/// Conservative classification: if uncertain, return Tools (= status quo, no regression).
/// Only clearly read-only / summarize intent returns cheapest Volume tier.
/// Action requests (send / reply / schedule ...) return Tools (needs tool-calling capability).
pub fn classify_workload(message: &str, explicit_premium: bool) -> Workload {
    if explicit_premium {
        return Workload::Premium;
    }
    let m = message.trim().to_lowercase();

    const TOOL_TRIGGERS: &[&str] = &[
        "send", "reply", "draft", "schedule", "create", "delete", "archive",
        "label", "move ", "book ", "cancel", "forward", "mark ", "unsubscribe",
    ];
    if TOOL_TRIGGERS.iter().any(|t| m.contains(t)) {
        return Workload::Tools;
    }

    const VOLUME_INTENTS: &[&str] = &[
        "summar", "what emails", "what's in my inbox", "whats in my inbox",
        "unread", "list ", "show me", "any new", "catch me up", "tl;dr",
        "digest", "what do i have", "what needs",
    ];
    if VOLUME_INTENTS.iter().any(|t| m.contains(t)) {
        return Workload::Volume;
    }

    Workload::Tools
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

    #[test]
    fn explicit_premium_always_wins() {
        assert_eq!(classify_workload("summarize my inbox", true), Workload::Premium);
    }

    #[test]
    fn readonly_triage_goes_volume() {
        assert_eq!(classify_workload("summarize my unread emails", false), Workload::Volume);
        assert_eq!(classify_workload("what's in my inbox today", false), Workload::Volume);
        assert_eq!(classify_workload("catch me up", false), Workload::Volume);
    }

    #[test]
    fn action_requests_stay_tools() {
        assert_eq!(classify_workload("reply to the recruiter", false), Workload::Tools);
        assert_eq!(classify_workload("draft an email to Sam", false), Workload::Tools);
        assert_eq!(classify_workload("schedule a call tomorrow", false), Workload::Tools);
    }

    #[test]
    fn ambiguous_defaults_to_tools_no_regression() {
        assert_eq!(classify_workload("hmm ok interesting", false), Workload::Tools);
    }
}
