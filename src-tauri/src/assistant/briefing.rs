use crate::ai::AiRouter;
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
) -> Result<BriefingResult, String> {
    let context = DayContext::gather(db)?;
    let prompt = context.to_prompt();

    let messages = vec![("user".to_string(), prompt)];
    let briefing_text = router.send(messages).await?;

    Ok(BriefingResult {
        greeting: context.greeting,
        briefing: briefing_text,
        has_overdue: context.tasks_summary.contains("overdue"),
        task_count: extract_number(&context.tasks_summary),
    })
}

fn extract_number(s: &str) -> i64 {
    s.split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}
