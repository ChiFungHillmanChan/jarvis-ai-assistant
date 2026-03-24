use crate::ai::AiRouter;
use crate::assistant::{briefing, context::DayContext};
use crate::db::Database;
use crate::voice::tts::TextToSpeech;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn get_briefing(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
) -> Result<briefing::BriefingResult, String> {
    briefing::generate_briefing(&db, &router).await
}

#[tauri::command]
pub async fn speak_briefing(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
) -> Result<briefing::BriefingResult, String> {
    let result = briefing::generate_briefing(&db, &router).await?;

    // Speak the briefing
    let tts = TextToSpeech::new();
    let speech = format!("{}. {}", result.greeting, result.briefing);
    if let Err(e) = tts.speak(&speech).await {
        log::warn!("Briefing TTS failed: {}", e);
    }

    Ok(result)
}

#[tauri::command]
pub async fn ask_jarvis(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    question: String,
) -> Result<String, String> {
    // Build context + question
    let context = DayContext::gather(&db)?;
    let prompt = format!(
        "Here is the user's current status:\n\n\
         TASKS:\n{}\n\nCALENDAR:\n{}\n\nEMAIL:\n{}\n\nGITHUB:\n{}\n\n\
         The user asks: \"{}\"\n\n\
         Answer based on the data above. Be specific and actionable. \
         If the user asks what to work on, prioritize by urgency and deadlines. \
         Keep response concise.",
        context.tasks_summary,
        context.calendar_summary,
        context.email_summary,
        context.github_summary,
        question
    );

    let messages = vec![("user".to_string(), prompt)];
    router.send(messages).await
}
