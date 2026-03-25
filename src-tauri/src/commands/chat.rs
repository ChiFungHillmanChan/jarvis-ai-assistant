use crate::ai::AiRouter;
use crate::db::Database;
use crate::voice::VoiceEngine;
use crate::voice::tts::StreamingTts;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tauri::{Emitter, State};

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: Option<i64>,
    pub role: String,
    pub content: String,
    pub created_at: Option<String>,
}

#[tauri::command]
pub async fn send_message(
    app_handle: tauri::AppHandle,
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    google_auth: State<'_, Arc<crate::auth::google::GoogleAuth>>,
    engine: State<'_, Arc<VoiceEngine>>,
    message: String,
) -> Result<ChatMessage, String> {
    // Cancel any in-progress TTS and emit thinking state FIRST for immediate UI feedback
    {
        let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
        tts.cancel();
        let _ = app_handle.emit("tts-amplitude", json!({"amplitude": 0.0}));
    }
    let _ = app_handle.emit("chat-state", json!({"state": "thinking"}));
    let _ = app_handle.emit("chat-status", json!({"status": "Processing..."}));

    // Run DB insert + conversation fetch + context gathering on a blocking thread
    // to avoid starving tokio's async worker pool when the mutex is contended.
    let db_arc = db.inner().clone();
    let msg_clone = message.clone();
    let msg_lower = message.to_lowercase();

    let (history, context_messages) = tokio::task::spawn_blocking(move || -> Result<(Vec<(String, String)>, Vec<(String, String)>), String> {
        // Insert user message
        {
            let conn = db_arc.conn.lock().map_err(|e| e.to_string())?;
            conn.execute("INSERT INTO conversations (role, content) VALUES ('user', ?1)", rusqlite::params![msg_clone])
                .map_err(|e| e.to_string())?;
        }

        // Fetch conversation history
        let history = {
            let conn = db_arc.conn.lock().map_err(|e| e.to_string())?;
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

        // Only add context for questions about personal data (saves tokens)
        let needs_context = {
            msg_lower.contains("task") || msg_lower.contains("meeting") || msg_lower.contains("email")
            || msg_lower.contains("deadline") || msg_lower.contains("schedule") || msg_lower.contains("today")
            || msg_lower.contains("calendar") || msg_lower.contains("work on") || msg_lower.contains("priority")
            || msg_lower.contains("github") || msg_lower.contains("pr") || msg_lower.contains("issue")
            || msg_lower.contains("notion") || msg_lower.contains("what should") || msg_lower.contains("remind")
            || msg_lower.contains("status") || msg_lower.contains("busy") || msg_lower.contains("free")
            || msg_lower.contains("obsidian") || msg_lower.contains("notes") || msg_lower.contains("vault")
            || msg_lower.contains("job") || msg_lower.contains("resume") || msg_lower.contains("interview")
            || msg_lower.contains("application")
        };

        let mut ctx_msgs = Vec::new();
        if needs_context {
            if let Ok(ctx) = crate::assistant::context::DayContext::gather(&db_arc) {
                let context_prompt = format!(
                    "[CONTEXT] TASKS: {} | CALENDAR: {} | EMAIL: {} | GITHUB: {}",
                    ctx.tasks_summary.lines().next().unwrap_or(""),
                    ctx.calendar_summary.lines().next().unwrap_or(""),
                    ctx.email_summary.lines().next().unwrap_or(""),
                    ctx.github_summary,
                );
                ctx_msgs.push(("user".to_string(), context_prompt));
                ctx_msgs.push(("assistant".to_string(), "Understood.".to_string()));
            }
        }

        Ok((history, ctx_msgs))
    }).await.map_err(|e| e.to_string())??;

    // Check if user is searching conversation history
    let search_keywords = ["what did i tell you", "what did i say", "remember when", "search for", "find our conversation about", "what did we discuss"];
    let msg_lower = message.to_lowercase();
    if search_keywords.iter().any(|kw| msg_lower.contains(kw)) {
        let topic = search_keywords.iter()
            .filter_map(|kw| msg_lower.find(kw).map(|pos| &message[pos + kw.len()..]))
            .next()
            .unwrap_or(&message)
            .trim()
            .trim_matches(|c: char| c == '?' || c == '.' || c == '"')
            .to_string();

        if !topic.is_empty() {
            let db_arc2 = db.inner().clone();
            let results: Vec<String> = tokio::task::spawn_blocking(move || -> Result<Vec<String>, String> {
                let conn = db_arc2.conn.lock().map_err(|e| e.to_string())?;
                let mut stmt = conn.prepare(
                    "SELECT role, content, created_at FROM conversations WHERE content LIKE ?1 ORDER BY created_at DESC LIMIT 10"
                ).map_err(|e| e.to_string())?;
                let pattern = format!("%{}%", topic);
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
                Ok(collected)
            }).await.map_err(|e| e.to_string())??;

            if !results.is_empty() {
                let search_context = results.join("\n");
                let search_prompt = format!(
                    "The user is asking about past conversations. Here are relevant matches:\n{}\n\nUser's question: {}\n\nSummarize what was discussed. Be specific.",
                    search_context, message
                );
                let search_messages = vec![("user".to_string(), search_prompt)];
                let search_response = match router.send(search_messages, &db, &google_auth, &app_handle, None).await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                        return Err(e);
                    }
                };

                {
                    let conn3 = db.conn.lock().map_err(|e| e.to_string())?;
                    conn3.execute("INSERT INTO conversations (role, content) VALUES ('assistant', ?1)", rusqlite::params![search_response])
                        .map_err(|e| e.to_string())?;
                }

                {
                    let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
                    if let Err(e) = tts.speak_queued(&search_response, &app_handle).await {
                        log::warn!("Chat TTS failed: {}", e);
                        let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    }
                }

                return Ok(crate::commands::chat::ChatMessage {
                    id: None, role: "assistant".to_string(), content: search_response, created_at: None,
                });
            }
        }
    }

    let mut all_messages = context_messages;

    // Search Obsidian for relevant notes (with 3s timeout to avoid blocking)
    let obsidian_keywords = [
        "obsidian", "notes", "vault", "job", "resume", "interview",
        "application", "career", "cover letter",
    ];
    if obsidian_keywords.iter().any(|kw| msg_lower.contains(kw)) {
        let obs_key = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = 'obsidian_api_key'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
        };
        if let Some(key) = obs_key {
            let search_term = message
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .take(3)
                .collect::<Vec<_>>()
                .join(" ");
            // 3-second timeout so Obsidian search doesn't block the response
            if let Ok(Ok(notes)) = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                crate::integrations::obsidian::search_vault(&key, &search_term),
            ).await {
                let note_context: String = notes
                    .iter()
                    .take(3)
                    .map(|n| {
                        format!(
                            "- {} {}",
                            n.path,
                            n.content
                                .as_deref()
                                .unwrap_or("")
                                .chars()
                                .take(200)
                                .collect::<String>()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !note_context.is_empty() {
                    let len = all_messages.len();
                    all_messages.insert(
                        if len > 0 { len - 1 } else { 0 },
                        ("user".to_string(), format!("[OBSIDIAN NOTES]\n{}", note_context)),
                    );
                    all_messages.insert(
                        if len > 0 { len } else { 1 },
                        ("assistant".to_string(), "I see the relevant notes from your Obsidian vault.".to_string()),
                    );
                }
            }
        }
    }

    all_messages.extend(history);
    let messages = all_messages;

    // Create streaming TTS -- speaks sentences as AI tokens arrive
    let streaming_tts = {
        let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
        StreamingTts::new(tts, app_handle.clone())
    };
    let tts_tx = Some(streaming_tts.sender());

    let response_text = match router.send(messages, &db, &google_auth, &app_handle, tts_tx).await {
        Ok(r) => r,
        Err(e) => {
            let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
            return Err(e);
        }
    };
    let final_response = response_text;
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO conversations (role, content) VALUES ('assistant', ?1)", rusqlite::params![final_response])
            .map_err(|e| e.to_string())?;
    }

    // Finish streaming TTS -- speaks any remaining buffered text
    log::debug!("[chat] TTS finish, len={}", final_response.len());
    streaming_tts.finish().await;

    Ok(ChatMessage { id: None, role: "assistant".to_string(), content: final_response, created_at: None })
}

#[tauri::command]
pub fn clear_conversations(db: State<Arc<Database>>) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM conversations", []).map_err(|e| e.to_string())?;
    log::info!("Conversations cleared");
    Ok(())
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
