use crate::ai::{AiRouter, Workload};
use crate::assistant::context::DayContext;
use crate::db::Database;
use std::sync::Arc;

#[derive(serde::Serialize, Clone)]
pub struct BriefingResult {
    pub greeting: String,
    pub briefing: String,
    pub has_overdue: bool,
    pub task_count: i64,
}

pub async fn generate_briefing(
    db: &Arc<Database>,
    router: &AiRouter,
    google_auth: &Arc<crate::auth::google::GoogleAuth>,
    app_handle: &tauri::AppHandle,
) -> Result<BriefingResult, String> {
    let context = DayContext::gather(db)?;
    let prompt = context.to_prompt();

    let messages = vec![("user".to_string(), prompt)];
    // The prompt carries the senders and subjects of the three newest unread
    // mails verbatim. No tool call fetched them, so the turn has to be marked
    // tainted here, or a stranger's subject line could ask for the tools that
    // otherwise wait for a click after an untrusted read.
    let briefing_text = router
        .send_with_untrusted_context(messages, db, google_auth, app_handle, None, Workload::Tools)
        .await?;

    Ok(BriefingResult {
        greeting: context.greeting,
        briefing: briefing_text,
        has_overdue: context.tasks_summary.contains("overdue"),
        task_count: extract_task_count(&context.tasks_summary),
    })
}

pub fn extract_task_count(s: &str) -> i64 {
    s.split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}
