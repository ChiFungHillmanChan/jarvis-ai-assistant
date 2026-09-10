use crate::ai::{AiRouter, Workload};
use crate::assistant::{briefing, context::DayContext};
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::voice::VoiceEngine;
use std::sync::{Arc, Mutex};
use tauri::State;

/// Today's briefing, if one was already generated today.
///
/// `last_briefing_date` and `cached_briefing` are written together, so a
/// `cached_briefing` carrying an earlier date is stale and must not be served.
/// Only the generated text is cached: the greeting and the counts are rebuilt
/// from live context on every read.
fn cached_briefing_today(db: &Arc<Database>) -> Result<Option<briefing::BriefingResult>, String> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    let cached = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let last_date = conn.query_row(
            "SELECT value FROM user_preferences WHERE key = 'last_briefing_date'",
            [], |row| row.get::<_, String>(0),
        ).unwrap_or_default();

        if last_date == today {
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = 'cached_briefing'",
                [], |row| row.get::<_, String>(0),
            ).ok()
        } else {
            None
        }
    };

    let Some(text) = cached else {
        return Ok(None);
    };
    let context = DayContext::gather(db)?;
    Ok(Some(briefing::BriefingResult {
        greeting: context.greeting,
        briefing: text,
        has_overdue: context.tasks_summary.contains("overdue"),
        task_count: briefing::extract_task_count(&context.tasks_summary),
    }))
}

#[tauri::command]
pub async fn get_briefing(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    google_auth: State<'_, Arc<GoogleAuth>>,
) -> Result<briefing::BriefingResult, String> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    if let Some(cached) = cached_briefing_today(&db)? {
        return Ok(cached);
    }

    let router = router.lock().map_err(|e| e.to_string())?.clone();
    let result = briefing::generate_briefing(&db, &router, &google_auth, &app_handle).await?;

    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let _ = conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES ('last_briefing_date', ?1, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
            rusqlite::params![today],
        );
        let _ = conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES ('cached_briefing', ?1, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
            rusqlite::params![result.briefing],
        );
    }

    Ok(result)
}

#[tauri::command]
pub async fn speak_briefing(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    google_auth: State<'_, Arc<GoogleAuth>>,
    engine: State<'_, Arc<VoiceEngine>>,
) -> Result<briefing::BriefingResult, String> {
    // Pressing play regenerated the whole briefing -- a second full model call,
    // and a different text from the one already on screen. Speak what was
    // generated today if there is one.
    let result = match cached_briefing_today(&db)? {
        Some(cached) => cached,
        None => {
            let router = router.lock().map_err(|e| e.to_string())?.clone();
            briefing::generate_briefing(&db, &router, &google_auth, &app_handle).await?
        }
    };

    // Share VoiceEngine's TextToSpeech, and with it the cancel flag the rest of
    // the app reaches -- send_message and the stop control both cancel that one
    // instance. A privately built TextToSpeech::new() was uncancellable from
    // anywhere in the UI, so pressing play on the briefing card committed the
    // user to the whole briefing, and it also ignored their saved voice, rate
    // and tts_enabled preferences.
    let tts = match engine.tts.lock() {
        Ok(guard) => guard.clone(),
        Err(e) => {
            log::warn!("Briefing TTS unavailable: {}", e);
            return Ok(result);
        }
    };
    let speech = format!("{}. {}", result.greeting, result.briefing);
    if let Err(e) = tts.speak(&speech).await {
        log::warn!("Briefing TTS failed: {}", e);
    }

    Ok(result)
}

#[tauri::command]
pub async fn ask_jarvis(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    google_auth: State<'_, Arc<GoogleAuth>>,
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
    let router = router.lock().map_err(|e| e.to_string())?.clone();
    // Same as the briefing: the prompt already quotes unread senders and
    // subjects and today's event titles, none of it through a tool call, so
    // the turn is tainted before the model sees it.
    router
        .send_with_untrusted_context(messages, &db, &google_auth, &app_handle, None, Workload::Tools)
        .await
}

#[tauri::command]
pub async fn search_conversations(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    router: State<'_, Mutex<AiRouter>>,
    google_auth: State<'_, Arc<GoogleAuth>>,
    query: String,
    session_id: Option<String>,
) -> Result<String, String> {
    let session_id = crate::chat_session::identity(session_id, "legacy")?;
    let _guard = crate::chat_session::FOREGROUND.try_lock().map_err(|_| "另一個助理任務進行中，請等完成後再試。".to_string())?;
    // Search conversations for relevant messages
    let results = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT role, content, created_at FROM conversations
             WHERE content LIKE ?1 AND session_id = ?2
             ORDER BY created_at DESC LIMIT 10"
        ).map_err(|e| e.to_string())?;
        let pattern = format!("%{}%", query);
        let msgs: Vec<(String, String, Option<String>)> = stmt
            .query_map(rusqlite::params![pattern, session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        msgs
    };

    if results.is_empty() {
        return Ok(format!("No conversations found matching '{}'.", query));
    }

    // Summarize findings with AI
    let context = results.iter().map(|(role, content, date)| {
        format!("[{} - {}]: {}", date.as_deref().unwrap_or("unknown"), role, content)
    }).collect::<Vec<_>>().join("\n");

    let prompt = format!(
        "The user is searching their conversation history for: \"{}\"\n\nHere are the matching conversations:\n{}\n\nSummarize what was discussed about this topic. Be specific and reference dates if available.",
        query, context
    );

    let messages = vec![("user".to_string(), prompt)];
    let router = router.lock().map_err(|e| e.to_string())?.clone();
    router.send_with_untrusted_context(messages, &db, &google_auth, &app_handle, None, Workload::Tools).await
}
