use crate::ai::AiRouter;
use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: Option<i64>,
    pub role: String,
    pub content: String,
    pub created_at: Option<String>,
}

#[tauri::command]
pub async fn send_message(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    message: String,
) -> Result<ChatMessage, String> {
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('user', ?1)", rusqlite::params![message])
            .map_err(|e| e.to_string())?;
    }

    // Check if user is searching conversation history
    let search_keywords = ["what did i tell you", "what did i say", "remember when", "search for", "find our conversation about", "what did we discuss"];
    let msg_lower = message.to_lowercase();
    if search_keywords.iter().any(|kw| msg_lower.contains(kw)) {
        // Extract the search topic (rough: everything after the keyword)
        let topic = search_keywords.iter()
            .filter_map(|kw| msg_lower.find(kw).map(|pos| &message[pos + kw.len()..]))
            .next()
            .unwrap_or(&message)
            .trim()
            .trim_matches(|c: char| c == '?' || c == '.' || c == '"')
            .to_string();

        if !topic.is_empty() {
            // Search conversations — keep all DB work in a scoped block so
            // MutexGuard and Statement are dropped before the async `await`.
            let results: Vec<String> = {
                let conn2 = db.conn.lock().map_err(|e| e.to_string())?;
                let mut stmt = conn2.prepare(
                    "SELECT role, content, created_at FROM conversations WHERE content LIKE ?1 ORDER BY created_at DESC LIMIT 10"
                ).map_err(|e| e.to_string())?;
                let pattern = format!("%{}%", topic);
                // Bind to `collected` so MappedRows is fully consumed (and
                // dropped) before `stmt` / `conn2` leave scope.
                let collected = stmt
                    .query_map(rusqlite::params![pattern], |row| {
                        let role: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        let date: Option<String> = row.get(2)?;
                        Ok(format!("[{} - {}]: {}", date.unwrap_or_default(), role, &content[..content.len().min(100)]))
                    })
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                collected
            };

            if !results.is_empty() {
                let search_context = results.join("\n");
                let search_prompt = format!(
                    "The user is asking about past conversations. Here are relevant matches:\n{}\n\nUser's question: {}\n\nSummarize what was discussed. Be specific.",
                    search_context, message
                );
                let search_messages = vec![("user".to_string(), search_prompt)];
                let search_response = router.send(search_messages).await?;

                {
                    let conn3 = db.conn.lock().map_err(|e| e.to_string())?;
                    conn3.execute("INSERT INTO conversations (role, content) VALUES ('assistant', ?1)", rusqlite::params![search_response])
                        .map_err(|e| e.to_string())?;
                }

                return Ok(crate::commands::chat::ChatMessage {
                    id: None, role: "assistant".to_string(), content: search_response, created_at: None,
                });
            }
        }
    }

    let messages = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT role, content FROM conversations ORDER BY id DESC LIMIT 20")
            .map_err(|e| e.to_string())?;
        let mut msgs: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        msgs.reverse();
        msgs
    };
    // Prepend context about user's current status
    let context_prompt = match crate::assistant::context::DayContext::gather(&db) {
        Ok(ctx) => format!(
            "Current user status -- TASKS: {} | CALENDAR: {} | EMAIL: {} | GITHUB: {}",
            ctx.tasks_summary.lines().next().unwrap_or(""),
            ctx.calendar_summary.lines().next().unwrap_or(""),
            ctx.email_summary.lines().next().unwrap_or(""),
            ctx.github_summary,
        ),
        Err(_) => String::new(),
    };

    let mut context_messages = Vec::new();
    if !context_prompt.is_empty() {
        context_messages.push(("user".to_string(), format!("[CONTEXT] {}", context_prompt)));
        context_messages.push(("assistant".to_string(), "Understood. I have your current status.".to_string()));
    }
    context_messages.extend(messages);
    let messages = context_messages;

    let response_text = router.send(messages).await?;
    let (final_response, _actions) = crate::assistant::actions::execute_actions(&response_text, &db);
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('assistant', ?1)", rusqlite::params![final_response])
            .map_err(|e| e.to_string())?;
    }
    Ok(ChatMessage { id: None, role: "assistant".to_string(), content: final_response, created_at: None })
}

#[tauri::command]
pub fn get_conversations(db: State<Arc<Database>>) -> Result<Vec<ChatMessage>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id, role, content, created_at FROM conversations ORDER BY id ASC")
        .map_err(|e| e.to_string())?;
    let messages = stmt
        .query_map([], |row| {
            Ok(ChatMessage { id: row.get(0)?, role: row.get(1)?, content: row.get(2)?, created_at: row.get(3)? })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(messages)
}
