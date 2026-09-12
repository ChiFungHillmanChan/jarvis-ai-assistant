use crate::ai::{classify_workload, AiRouter, Workload};
use crate::db::Database;
use crate::voice::tts::StreamingTts;
use crate::voice::VoiceEngine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tauri::State;
use crate::chat_session::{self, ScopedEmitter};

/// Whether a message is asking about the user's own data, in which case it is
/// worth the tokens to inject day context.
///
/// `lower` is the lowercased message (for the English terms) and `raw` is the
/// original (CJK has no case). The Chinese terms mirror the trigger list in
/// `ai/routing.rs` -- without them a Cantonese user asking "今日有咩會議" got
/// an answer with no calendar data in it, which quietly broke the bilingual
/// positioning on the default provider.
fn needs_personal_context(lower: &str, raw: &str) -> bool {
    const EN: &[&str] = &[
        "task", "meeting", "email", "deadline", "schedule", "today", "calendar",
        "work on", "priority", "github", "pr", "issue", "notion", "what should",
        "remind", "status", "busy", "free", "obsidian", "notes", "vault", "job",
        "resume", "interview", "application",
    ];
    const ZH: &[&str] = &[
        "今日", "今天", "會議", "会议", "開會", "开会", "郵件", "邮件", "電郵",
        "任務", "任务", "日程", "行事曆", "日曆", "日历", "死線", "截止",
        "提醒", "待辦", "待办", "要做", "安排", "未讀", "未读",
    ];
    EN.iter().any(|k| lower.contains(k)) || ZH.iter().any(|k| raw.contains(k))
}

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
    router: State<'_, Mutex<AiRouter>>,
    google_auth: State<'_, Arc<crate::auth::google::GoogleAuth>>,
    engine: State<'_, Arc<VoiceEngine>>,
    message: String,
    workload: Option<String>,
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<ChatMessage, String> {
    let session_id = chat_session::identity(session_id, "legacy")?;
    let run_id = chat_session::identity(run_id, &uuid::Uuid::new_v4().to_string())?;
    let _guard = chat_session::FOREGROUND.try_lock().map_err(|_| "另一個助理任務進行中，請等完成後再試。".to_string())?;
    let context = chat_session::Context { session_id: session_id.clone(), run_id, app: app_handle.clone() };
    chat_session::CONTEXT.scope(context, async {
    let result = send_scoped(&app_handle, &db, &router, &google_auth, &engine, message, workload, session_id).await;
    let _ = app_handle.emit("chat-state", json!({"state":"idle"}));
    result
    }).await
}

async fn send_scoped(
    app_handle: &tauri::AppHandle,
    db: &Arc<Database>,
    router: &Mutex<AiRouter>,
    google_auth: &Arc<crate::auth::google::GoogleAuth>,
    engine: &Arc<VoiceEngine>,
    message: String,
    workload: Option<String>,
    session_id: String,
) -> Result<ChatMessage, String> {
    let selected_workload = classify_workload(&message, workload.as_deref() == Some("premium"));
    // Cancel any in-progress TTS and emit thinking state FIRST for immediate UI feedback
    {
        let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
        tts.cancel();
        let _ = app_handle.emit("tts-amplitude", json!({"amplitude": 0.0}));
    }
    let _ = app_handle.emit("chat-state", json!({"state": "thinking"}));
    let _ = app_handle.emit(
        "chat-status",
        json!({
            "status": "Understanding your request...",
            "phase": "thinking"
        }),
    );

    // Run DB insert + conversation fetch + context gathering on a blocking thread
    // to avoid starving tokio's async worker pool when the mutex is contended.
    let db_arc = db.clone();
    let msg_clone = message.clone();
    let msg_lower = message.to_lowercase();
    let history_session = session_id.clone();

    let (history, context_messages) = tokio::task::spawn_blocking(
        move || -> Result<(Vec<(String, String)>, Vec<(String, String)>), String> {
            // Insert user message
            {
                let conn = db_arc.conn.lock().map_err(|e| e.to_string())?;
                conn.execute(
                    "INSERT INTO conversations (role, content, session_id) VALUES ('user', ?1, ?2)",
                    rusqlite::params![msg_clone, history_session],
                )
                .map_err(|e| e.to_string())?;
            }

            // Fetch conversation history
            let history = {
                let conn = db_arc.conn.lock().map_err(|e| e.to_string())?;
                let mut stmt = conn
                    .prepare("SELECT role, content FROM conversations WHERE session_id = ?1 ORDER BY id DESC LIMIT 20")
                    .map_err(|e| e.to_string())?;
                let mut msgs: Vec<(String, String)> = stmt
                    .query_map([&history_session], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                msgs.reverse();
                msgs
            };

            // Only add context for questions about personal data (saves tokens)
            let needs_context = needs_personal_context(&msg_lower, &msg_clone);

            let mut ctx_msgs = Vec::new();
            if needs_context {
                if let Ok(ctx) = crate::assistant::context::DayContext::gather(&db_arc) {
                    ctx_msgs.push(("user".to_string(), ctx.inline_context()));
                    ctx_msgs.push(("assistant".to_string(), "Understood.".to_string()));
                }
            }

            Ok((history, ctx_msgs))
        },
    )
    .await
    .map_err(|e| e.to_string())??;

    // Clone router for async operations (Mutex can't be held across await)
    let router = router.lock().map_err(|e| e.to_string())?.clone();

    // Check if user is searching conversation history
    let search_keywords = [
        "what did i tell you",
        "what did i say",
        "remember when",
        "search for",
        "find our conversation about",
        "what did we discuss",
    ];
    let msg_lower = message.to_lowercase();
    if search_keywords.iter().any(|kw| msg_lower.contains(kw)) {
        let topic = search_keywords
            .iter()
            .filter_map(|kw| msg_lower.find(kw).map(|pos| &message[pos + kw.len()..]))
            .next()
            .unwrap_or(&message)
            .trim()
            .trim_matches(|c: char| c == '?' || c == '.' || c == '"')
            .to_string();

        if !topic.is_empty() {
            let db_arc2 = db.clone();
            let search_session = session_id.clone();
            let results: Vec<String> = tokio::task::spawn_blocking(move || -> Result<Vec<String>, String> {
                let conn = db_arc2.conn.lock().map_err(|e| e.to_string())?;
                let mut stmt = conn.prepare(
                    "SELECT role, content, created_at FROM conversations WHERE content LIKE ?1 AND session_id = ?2 ORDER BY created_at DESC LIMIT 10"
                ).map_err(|e| e.to_string())?;
                let pattern = format!("%{}%", topic);
                let collected = stmt
                    .query_map(rusqlite::params![pattern, search_session], |row| {
                        let role: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        let date: Option<String> = row.get(2)?;
                        Ok(format!("[{} - {}]: {}", date.unwrap_or_default(), role, crate::util::truncate_bytes(&content, 100)))
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
                let _ = app_handle.emit(
                    "chat-status",
                    json!({
                        "status": "Reviewing earlier conversations...",
                        "phase": "thinking"
                    }),
                );
                let search_response = match router
                    .send_with_untrusted_context(search_messages, &db, &google_auth, &app_handle, None, Workload::Volume)
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                        return Err(e);
                    }
                };

                {
                    let conn3 = db.conn.lock().map_err(|e| e.to_string())?;
                    persist_scoped_reply(&conn3, &search_response, &session_id)
                        .map_err(|e| e.to_string())?;
                }

                {
                    let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
                    if let Err(e) = tts.speak_queued(&search_response, &app_handle).await {
                        log::warn!("Chat TTS failed: {}", e);
                    }
                }
                // Unconditional: speak_queued only emits "idle" itself when TTS
                // is enabled and produced at least one sentence, so with TTS
                // off the "Composing response..." HUD never dismissed.
                let _ = app_handle.emit("chat-state", json!({"state": "idle"}));

                return Ok(crate::commands::chat::ChatMessage {
                    id: None,
                    role: "assistant".to_string(),
                    content: search_response,
                    created_at: None,
                });
            }
        }
    }

    let mut all_messages = context_messages;
    // The [CONTEXT] block quotes today's event titles, and the [OBSIDIAN NOTES]
    // block below quotes note bodies. Neither arrives through a tool call, so
    // the per-turn ledger only knows this turn read text a stranger may have
    // written if the call site says so -- and `remember` writes a standing
    // instruction into every later system prompt.
    let mut untrusted_context = !all_messages.is_empty();

    // Search Obsidian for relevant notes (with 3s timeout to avoid blocking)
    let obsidian_keywords = [
        "obsidian",
        "notes",
        "vault",
        "job",
        "resume",
        "interview",
        "application",
        "career",
        "cover letter",
    ];
    if obsidian_keywords.iter().any(|kw| msg_lower.contains(kw)) {
        let search_term = message
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        // 3-second timeout so Obsidian search doesn't block the response
        if let Ok(Ok(notes)) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            crate::integrations::obsidian::search_connected(&db, &search_term),
        )
        .await
        {
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
                untrusted_context = true;
                let len = all_messages.len();
                all_messages.insert(
                    if len > 0 { len - 1 } else { 0 },
                    (
                        "user".to_string(),
                        format!("[OBSIDIAN NOTES]\n{}", note_context),
                    ),
                );
                all_messages.insert(
                    if len > 0 { len } else { 1 },
                    (
                        "assistant".to_string(),
                        "I see the relevant notes from your Obsidian vault.".to_string(),
                    ),
                );
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

    let sent = if untrusted_context {
        router
            .send_with_untrusted_context(
                messages,
                &db,
                &google_auth,
                &app_handle,
                tts_tx,
                selected_workload,
            )
            .await
    } else {
        router
            .send(messages, &db, &google_auth, &app_handle, tts_tx, selected_workload)
            .await
    };
    // Join the task-owned consumer before releasing the foreground gate on
    // every path, including provider and persistence errors.
    if sent.is_err() {
        if let Ok(tts) = engine.tts.lock() { tts.cancel(); }
    }
    streaming_tts.finish().await;
    let response_text = match sent {
        Ok(r) => r,
        Err(e) => {
            let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
            return Err(e);
        }
    };
    let final_response = response_text;
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        persist_scoped_reply(&conn, &final_response, &session_id).map_err(|e| e.to_string())?;
    }

    // Unconditional: the HUD must return to idle here regardless of whether
    // the streaming consumer's own "idle" emit ran.
    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));

    Ok(ChatMessage {
        id: None,
        role: "assistant".to_string(),
        content: final_response,
        created_at: None,
    })
}

fn persist_scoped_reply(conn: &rusqlite::Connection, content: &str, session: &str) -> rusqlite::Result<()> {
    if !content.trim().is_empty() {
        conn.execute("INSERT INTO conversations(role,content,session_id) VALUES ('assistant',?1,?2)", rusqlite::params![content,session])?;
    }
    Ok(())
}

#[derive(Serialize)]
pub struct ChatSession { pub id: String, pub title: String, pub updated_at: String }
#[tauri::command]
pub fn list_chat_sessions(db: State<Arc<Database>>) -> Result<Vec<ChatSession>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT c.session_id, COALESCE((SELECT substr(content,1,60) FROM conversations WHERE session_id=c.session_id AND role='user' ORDER BY id LIMIT 1),'對話'), COALESCE(MAX(c.created_at),'') FROM conversations c GROUP BY c.session_id ORDER BY MAX(c.id) DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(ChatSession {id:r.get(0)?,title:r.get(1)?,updated_at:r.get(2)?})).map_err(|e| e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string());
    rows
}

/// Persist the assistant's turn. A blank reply is not stored: the next turn
/// would replay it as an empty message part, which Gemini rejects with 400,
/// and every message after that fails until "New chat".
pub(crate) fn persist_assistant_reply(
    conn: &rusqlite::Connection,
    content: &str,
) -> rusqlite::Result<()> {
    if content.trim().is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO conversations (role, content) VALUES ('assistant', ?1)",
        rusqlite::params![content],
    )?;
    Ok(())
}

#[tauri::command]
pub fn clear_conversations(db: State<Arc<Database>>, session_id: Option<String>) -> Result<(), String> {
    let session_id = chat_session::identity(session_id, "legacy")?;
    let _guard = chat_session::FOREGROUND.try_lock().map_err(|_| "助理任務進行中，暫時無法清除記錄。".to_string())?;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    clear_session(&conn, &session_id)?;
    log::info!("Conversations cleared");
    Ok(())
}

fn clear_session(conn: &rusqlite::Connection, session_id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM conversations WHERE session_id = ?1", [session_id]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Nothing prunes `conversations` -- the only delete is the user pressing
/// clear -- and the frontend reloads this on every mount, so an unbounded
/// SELECT makes cold start cost grow with lifetime history. Walk the primary
/// key backwards for the most recent window and reverse it back into the
/// oldest-first order the UI appends to.
const CONVERSATION_WINDOW_QUERY: &str =
    "SELECT id, role, content, created_at FROM conversations WHERE session_id = ?2 ORDER BY id DESC LIMIT ?1";
const DEFAULT_CONVERSATION_LIMIT: i64 = 200;
const MAX_CONVERSATION_LIMIT: i64 = 1000;

/// A missing limit means the default, and SQLite reads a negative LIMIT as "no
/// limit", which is the exact thing being fixed -- so clamp rather than trust.
fn conversation_limit(requested: Option<i64>) -> i64 {
    requested
        .unwrap_or(DEFAULT_CONVERSATION_LIMIT)
        .clamp(1, MAX_CONVERSATION_LIMIT)
}

#[tauri::command]
pub fn get_conversations(
    db: State<Arc<Database>>,
    limit: Option<i64>,
    session_id: Option<String>,
) -> Result<Vec<ChatMessage>, String> {
    let session_id = chat_session::identity(session_id, "legacy")?;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    history(&conn, &session_id, limit)
}

fn history(conn: &rusqlite::Connection, session_id: &str, limit: Option<i64>) -> Result<Vec<ChatMessage>, String> {
    let mut stmt = conn
        .prepare(CONVERSATION_WINDOW_QUERY)
        .map_err(|e| e.to_string())?;
    let mut messages = stmt
        .query_map(rusqlite::params![conversation_limit(limit),session_id], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    messages.reverse();
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn chat_session_history_and_clear_are_isolated() {
        let db = crate::db::test_database();
        let conn = db.conn.lock().unwrap();
        conn.execute("INSERT INTO conversations(role,content) VALUES ('user','legacy')", []).unwrap();
        conn.execute("INSERT INTO conversations(role,content,session_id) VALUES ('user','Alpha','task-a'),('user','Beta','task-b')", []).unwrap();
        assert_eq!(history(&conn, "task-a", None).unwrap()[0].content, "Alpha");
        assert_eq!(history(&conn, "task-b", None).unwrap().len(), 1);
        persist_scoped_reply(&conn, "Reply A", "task-a").unwrap();
        persist_scoped_reply(&conn, " ", "task-b").unwrap();
        assert_eq!(history(&conn, "task-a", None).unwrap().len(), 2);
        assert_eq!(history(&conn, "task-b", None).unwrap().len(), 1);
        clear_session(&conn, "task-a").unwrap();
        assert!(history(&conn, "task-a", None).unwrap().is_empty());
        assert_eq!(history(&conn, "legacy", None).unwrap()[0].content, "legacy");
        let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM conversations WHERE session_id IN ('task-b','legacy')", [], |r| r.get(0)).unwrap();
        assert_eq!(remaining, 2);
    }

    #[test]
    fn session_migration_retains_existing_legacy_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE conversations(id INTEGER PRIMARY KEY, role TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT DEFAULT CURRENT_TIMESTAMP); INSERT INTO conversations(role,content) VALUES('user','Before migration');").unwrap();
        conn.execute_batch(include_str!("../../migrations/V24__chat_sessions.sql")).unwrap();
        let messages = history(&conn, "legacy", None).unwrap();
        assert_eq!(messages.len(),1);
        assert_eq!(messages[0].content,"Before migration");
        assert!(history(&conn,"task-new",None).unwrap().is_empty());
    }

    fn asks(msg: &str) -> bool {
        needs_personal_context(&msg.to_lowercase(), msg)
    }

    #[test]
    fn cantonese_questions_pull_in_personal_context() {
        assert!(asks("今日有咩會議"));
        assert!(asks("幫我睇下啲未讀郵件"));
        assert!(asks("聽日嘅日程係點"));
        assert!(asks("有咩任務要做"));
    }

    #[test]
    fn english_triggers_still_work() {
        assert!(asks("what meetings do I have today"));
        assert!(asks("any urgent email"));
        assert!(!asks("write me a haiku about rust"));
    }

    #[test]
    fn conversation_limit_is_bounded_in_both_directions() {
        assert_eq!(conversation_limit(None), DEFAULT_CONVERSATION_LIMIT);
        assert_eq!(conversation_limit(Some(50)), 50);
        // A negative LIMIT is "no limit" in SQLite -- it must never reach the query.
        assert_eq!(conversation_limit(Some(-1)), 1);
        assert_eq!(conversation_limit(Some(0)), 1);
        assert_eq!(conversation_limit(Some(10_000)), MAX_CONVERSATION_LIMIT);
    }

    /// The window must return the newest rows, but hand them back oldest-first:
    /// the chat view appends new messages to the end of the list.
    #[test]
    fn conversation_window_returns_the_newest_rows_oldest_first() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE conversations (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 created_at TEXT DEFAULT CURRENT_TIMESTAMP, session_id TEXT NOT NULL DEFAULT 'legacy');",
        )
        .unwrap();
        for i in 1..=10 {
            conn.execute(
                "INSERT INTO conversations (role, content) VALUES ('user', ?1)",
                rusqlite::params![format!("msg{}", i)],
            )
            .unwrap();
        }

        let mut stmt = conn.prepare(CONVERSATION_WINDOW_QUERY).unwrap();
        let mut contents: Vec<String> = stmt
            .query_map(rusqlite::params![conversation_limit(Some(3)),"legacy"], |r| {
                r.get::<_, String>(2)
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        contents.reverse();
        assert_eq!(contents, vec!["msg8", "msg9", "msg10"]);
    }

    /// A blank reply must never be stored: the next turn's history builder
    /// would hand Gemini an empty text part and every later message would
    /// fail with 400 until "New chat".
    #[test]
    fn blank_assistant_reply_is_not_persisted() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE conversations (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 created_at TEXT DEFAULT CURRENT_TIMESTAMP, session_id TEXT NOT NULL DEFAULT 'legacy');",
        )
        .unwrap();
        persist_assistant_reply(&conn, "").unwrap();
        persist_assistant_reply(&conn, " \n\t").unwrap();
        persist_assistant_reply(&conn, "hello").unwrap();

        let rows: Vec<(String, String)> = conn
            .prepare("SELECT role, content FROM conversations ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(rows, vec![("assistant".to_string(), "hello".to_string())]);
    }
}
