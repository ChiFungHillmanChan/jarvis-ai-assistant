use crate::ai::{classify_workload, cost::Feature, AiRouter, Workload};
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
///
/// English terms match whole words only (a plural "s" allowed): as substrings,
/// "pr" matched "prefer" and "project", so ordinary sentences pulled in the
/// [CONTEXT] block and marked the turn as having read outside text.
fn needs_personal_context(lower: &str, raw: &str) -> bool {
    const EN: &[&str] = &[
        "task", "meeting", "email", "deadline", "schedule", "today", "calendar",
        "work on", "priority", "github", "pr", "issue", "notion", "what should",
        "remind", "reminder", "status", "busy", "free", "obsidian", "notes", "vault", "job",
        "resume", "interview", "application", "inbox", "mail",
    ];
    const ZH: &[&str] = &[
        "今日", "今天", "會議", "会议", "開會", "开会", "郵件", "邮件", "電郵",
        "任務", "任务", "日程", "行事曆", "日曆", "日历", "死線", "截止",
        "提醒", "待辦", "待办", "要做", "安排", "未讀", "未读", "跟進", "跟进", "覆",
    ];
    EN.iter().any(|k| contains_word(lower, k)) || ZH.iter().any(|k| raw.contains(k))
}

/// Only ASCII letters and digits join a word: the terms are English, and in
/// code-mixed Cantonese ("有冇email") a Chinese character next to one is a
/// boundary, not part of the word.
fn contains_word(text: &str, word: &str) -> bool {
    let is_word_char = |c: char| c.is_ascii_alphanumeric();
    text.match_indices(word).any(|(start, _)| {
        let rest = &text[start + word.len()..];
        let rest = rest.strip_prefix('s').unwrap_or(rest);
        !text[..start].chars().next_back().is_some_and(is_word_char)
            && !rest.chars().next().is_some_and(is_word_char)
    })
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: Option<i64>,
    pub role: String,
    pub content: String,
    pub created_at: Option<String>,
    /// A voice request whose recording was kept and can be played back.
    #[serde(default)]
    pub has_recording: bool,
    /// Shown under the answer: for one answered by a model picked in the
    /// error card, that the chosen AI will not see it (mail_policy::PickedTurn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
    pick: Option<crate::ai::capability::Pick>,
    retry_of: Option<String>,
) -> Result<ChatMessage, String> {
    let (session_id, run_id) = begin_send(&db, session_id, run_id, retry_of.as_deref())?;
    let _guard = chat_session::FOREGROUND.try_lock().map_err(|_| "另一個助理任務進行中，請等完成後再試。".to_string())?;
    let context = chat_session::Context { session_id: session_id.clone(), run_id, app: app_handle.clone() };
    let retrying = retry_of.is_some();
    chat_session::CONTEXT.scope(context, on_pick(&db, pick, async {
    let result = send_scoped(&app_handle, &db, &router, &google_auth, &engine, message, workload, session_id, retrying).await;
    let _ = app_handle.emit("chat-state", json!({"state":"idle"}));
    result
    })).await
}

/// The session and run ids of a send, once a "send again" for a run that
/// carried out an action has been refused ([`refuse_acted_retry`]). The
/// command gets its ids only from here, so it cannot skip the refusal
/// (review A, N-2).
fn begin_send(db: &Database, session_id: Option<String>, run_id: Option<String>, retry_of: Option<&str>) -> Result<(String, String), String> {
    let session_id = chat_session::identity(session_id, "legacy")?;
    let run_id = chat_session::identity(run_id, &uuid::Uuid::new_v4().to_string())?;
    refuse_acted_retry(retry_of, crate::ai::bedrock::UiLanguage::read(db))?;
    Ok((session_id, run_id))
}

/// A request sent again from the error card (P06): never one that had
/// already carried out an action, whatever the frontend sends.
fn refuse_acted_retry(retry_of: Option<&str>, language: crate::ai::bedrock::UiLanguage) -> Result<(), String> {
    match retry_of {
        Some(retry_of) if !crate::ai::failure::may_retry(retry_of) => Err(language.pick(
            "This request already carried out an action, such as creating an event or a draft. JARVIS does not send it again, so the action is not done twice.",
            "這個請求已執行了操作，例如建立日曆事件或草稿。為免重複執行，JARVIS 不會再次傳送。",
        )),
        _ => Ok(()),
    }
}

/// A chat turn, on the model picked in the error card if the user picked
/// one (P06). What a picked turn saves is withheld from a chosen AI that
/// may not receive Gmail and Calendar content, and the answer says so
/// (mail_policy::PickedTurn).
async fn on_pick(
    db: &Database,
    pick: Option<crate::ai::capability::Pick>,
    turn: impl std::future::Future<Output = Result<ChatMessage, String>>,
) -> Result<ChatMessage, String> {
    let picked = pick.is_some();
    crate::ai::capability::with_pick(pick, async {
        let recorded = if picked { crate::ai::mail_policy::PickedTurn::begin(db)? } else { None };
        let mut answer = turn.await?;
        answer.note = joined([answer.note.take(), recorded.as_ref().and_then(|turn| turn.note())]);
        Ok(answer)
    })
    .await
}

/// The notes under one answer, one per line.
fn joined(notes: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let notes: Vec<String> = notes.into_iter().flatten().collect();
    (!notes.is_empty()).then(|| notes.join("\n"))
}

/// A chat turn's answer: the reply, saved to the session as it is, and the
/// note under it when a local model ran the turn without tools (P07). The
/// note is not part of the reply: it is not saved, so a reloaded history
/// shows the reply alone, and it is not spoken.
fn answer(conn: &rusqlite::Connection, reply: String, session_id: &str, without_tools: bool, language: crate::ai::bedrock::UiLanguage) -> Result<ChatMessage, String> {
    persist_scoped_reply(conn, &reply, session_id).map_err(|e| e.to_string())?;
    Ok(ChatMessage { id: None, role: "assistant".to_string(), content: reply, created_at: None, has_recording: false, note: without_tools.then(|| crate::ai::local::no_tools_answer_note(language)) })
}

/// Runs a chat turn's request and returns its answer: the reply saved, with
/// the note when a local model ran the turn without tools. A request that
/// failed is returned as it is, with nothing saved. The UI language is read
/// before the database lock is taken, because `UiLanguage::read` takes the
/// same lock and a std mutex locked twice on one thread never returns.
async fn answered(db: &Database, session_id: &str, turn: impl std::future::Future<Output = Result<String, String>>) -> Result<ChatMessage, String> {
    let (sent, without_tools) = crate::ai::local::noting_no_tools(turn).await;
    let reply = sent?;
    let language = crate::ai::bedrock::UiLanguage::read(db);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    answer(&conn, reply, session_id, without_tools, language)
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
    retrying: bool,
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
    // The thread keeps the request's pick: what it may be shown is decided
    // for the model it is sent to.
    let db_arc = db.clone();
    let msg_clone = message.clone();
    let history_session = session_id.clone();
    let (history, context_messages, message_id, held_back) =
        crate::ai::capability::spawn_blocking(move || begin_turn(&db_arc, &history_session, &msg_clone, retrying)).await??;
    if let Some(blocked) = held_back {
        blocked.raise("chat");
    }

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
            let results: Vec<String> = crate::ai::capability::spawn_blocking(move || -> Result<Vec<String>, String> {
                Ok(search_history(&db_arc2, &search_session, &topic, Some(message_id))?
                    .into_iter()
                    .map(|(role, content, date)| format!("[{} - {}]: {}", date.unwrap_or_default(), role, crate::util::truncate_bytes(&content, 100)))
                    .collect())
            }).await??;

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
                let done = answered(db, &session_id, async {
                    let sent = router.send_with_untrusted_context(search_messages, &db, &google_auth, &app_handle, None, Workload::Volume, Feature::Chat).await;
                    if sent.is_err() {
                        let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    }
                    sent
                })
                .await?;

                {
                    let tts = engine.tts.lock().map_err(|e| e.to_string())?.clone();
                    if let Err(e) = tts.speak_queued(&done.content, &app_handle).await {
                        log::warn!("Chat TTS failed: {}", e);
                    }
                }
                // Unconditional: speak_queued only emits "idle" itself when TTS
                // is enabled and produced at least one sentence, so with TTS
                // off the "Composing response..." HUD never dismissed.
                let _ = app_handle.emit("chat-state", json!({"state": "idle"}));

                return Ok(done);
            }
        }
    }

    let mut all_messages = context_messages;
    // The [CONTEXT] block quotes today's event titles, and the [OBSIDIAN NOTES]
    // block below quotes note bodies. Neither arrives through a tool call, so
    // the per-turn ledger only knows this turn read text a stranger may have
    // written if the call site says so -- and that is what makes the tools in
    // tool_approval::NEEDS_APPROVAL_AFTER_UNTRUSTED_READ wait for a click.
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

    let done = answered(db, &session_id, async {
        let sent = if untrusted_context {
            router.send_with_untrusted_context(messages, &db, &google_auth, &app_handle, tts_tx, selected_workload, Feature::Chat).await
        } else {
            router.send(messages, &db, &google_auth, &app_handle, tts_tx, selected_workload, Feature::Chat).await
        };
        // Join the task-owned consumer before releasing the foreground gate on
        // every path, including provider and persistence errors.
        if sent.is_err() {
            if let Ok(tts) = engine.tts.lock() { tts.cancel(); }
        }
        streaming_tts.finish().await;
        if sent.is_err() {
            let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
        }
        sent
    })
    .await?;

    // Unconditional: the HUD must return to idle here regardless of whether
    // the streaming consumer's own "idle" emit ran.
    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));

    Ok(done)
}

/// (history, context messages, the saved message's id, email held back)
type Turn = (Vec<(String, String)>, Vec<(String, String)>, i64, Option<crate::ai::mail_policy::Blocked>);

/// The start of a chat turn, before anything is sent: save the message and
/// gather what goes with it. Blocking; run it with
/// `capability::spawn_blocking`, so a pick reaches the gates here.
fn begin_turn(db: &Arc<Database>, session_id: &str, message: &str, retrying: bool) -> Result<Turn, String> {
    let (message_id, history) = if retrying { save_retried_turn(db, session_id, message)? } else { save_turn(db, session_id, message)? };

    // Only add context for questions about personal data (saves tokens)
    let needs_context = needs_personal_context(&message.to_lowercase(), message);

    let mut ctx_msgs = Vec::new();
    if needs_context {
        if let Ok(ctx) = crate::assistant::context::DayContext::gather(db) {
            ctx_msgs.push(("user".to_string(), ctx.inline_context()));
            ctx_msgs.push(("assistant".to_string(), "Understood.".to_string()));
        }
    }

    // Email and calendar content this turn leaves out (the senders and
    // titles of [CONTEXT], earlier chat that quoted email): not silently
    // (P06), the consent card goes up in send_scoped.
    let leaves_out = needs_context || crate::ai::mail_policy::withheld(db) != crate::ai::mail_policy::Withheld::default();
    let held_back = if leaves_out { crate::ai::mail_policy::check(db).err() } else { None };

    Ok((history, ctx_msgs, message_id, held_back))
}

/// Save the user's message and return the session's recent messages to send
/// with it, oldest first.
///
/// Messages the chosen AI must not be shown are left out (see
/// `ai::mail_policy::withheld`). The message being saved is always sent.
pub(crate) fn save_turn_and_load_history(db: &Database, session_id: &str, message: &str) -> Result<Vec<(String, String)>, String> {
    save_turn(db, session_id, message).map(|(_, history)| history)
}

/// `save_turn_and_load_history`, with the id of the message it saved.
pub(crate) fn save_turn(db: &Database, session_id: &str, message: &str) -> Result<(i64, Vec<(String, String)>), String> {
    save_turn_with(db, session_id, message, |_, _| {})
}

/// `save_turn`, running `also_save` on the saved message's id while the
/// database is still held, so what it writes is never seen missing.
pub(crate) fn save_turn_with(
    db: &Database,
    session_id: &str,
    message: &str,
    also_save: impl FnOnce(&rusqlite::Connection, i64),
) -> Result<(i64, Vec<(String, String)>), String> {
    // Before the lock below: this takes it too.
    let withheld = crate::ai::mail_policy::withheld_as_turn_begins(db)?;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO conversations (role, content, session_id) VALUES ('user', ?1, ?2)",
        rusqlite::params![message, session_id],
    )
    .map_err(|e| e.to_string())?;
    let saved = conn.last_insert_rowid();
    also_save(&conn, saved);
    Ok((saved, history_through(&conn, session_id, withheld.conversations, saved)?))
}

/// The last 20 messages of the session the AI may be shown, with `saved`
/// (the message being answered) always among them.
fn history_through(conn: &rusqlite::Connection, session_id: &str, withheld_through: i64, saved: i64) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT role, content FROM conversations WHERE session_id = ?1 AND (id > ?2 OR id = ?3) ORDER BY id DESC LIMIT 20")
        .map_err(|e| e.to_string())?;
    let mut messages: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![session_id, withheld_through, saved], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    messages.reverse();
    Ok(messages)
}

/// `save_turn` for a request sent again from the error card (P06): the
/// message the failed request saved is the session's last row, so it is
/// answered again rather than saved twice. Saved as usual otherwise.
pub(crate) fn save_retried_turn(db: &Database, session_id: &str, message: &str) -> Result<(i64, Vec<(String, String)>), String> {
    let last: Option<(i64, String, String)> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT id, role, content FROM conversations WHERE session_id = ?1 ORDER BY id DESC LIMIT 1", [session_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).ok()
    };
    match last {
        Some((id, role, content)) if role == "user" && content == message => {
            // Before the lock below: this takes it too.
            let withheld = crate::ai::mail_policy::withheld_as_turn_begins(db)?;
            let conn = db.conn.lock().map_err(|e| e.to_string())?;
            Ok((id, history_through(&conn, session_id, withheld.conversations, id)?))
        }
        _ => save_turn(db, session_id, message),
    }
}

/// Up to ten of the session's messages that mention `topic`, newest first, as
/// (role, content, created_at). The matches go to the chosen AI to summarise,
/// so messages it must not be shown are left out, as in the history above.
///
/// With `before`, only messages saved before that id. Chat saves the message
/// it is answering first, and that message always contains its own topic, so
/// without this every "search for ..." matched itself.
pub(crate) fn search_history(db: &Database, session_id: &str, topic: &str, before: Option<i64>) -> Result<Vec<(String, String, Option<String>)>, String> {
    let withheld = crate::ai::mail_policy::withheld(db);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT role, content, created_at FROM conversations WHERE content LIKE ?1 AND session_id = ?2 AND id > ?3 AND id < ?4 ORDER BY created_at DESC LIMIT 10")
        .map_err(|e| e.to_string())?;
    let found = stmt
        .query_map(rusqlite::params![format!("%{topic}%"), session_id, withheld.conversations, before.unwrap_or(i64::MAX)], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(found)
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
    "SELECT id, role, content, created_at, EXISTS(SELECT 1 FROM voice_recordings WHERE conversation_id = conversations.id) \
     FROM conversations WHERE session_id = ?2 ORDER BY id DESC LIMIT ?1";
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
                has_recording: row.get(4)?,
                note: None,
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
    fn a_saved_turn_reports_the_message_it_saved() {
        let db = crate::db::test_database();
        save_turn(&db, "legacy", "first").unwrap();
        let (id, history) = save_turn(&db, "legacy", "what is on my calendar").unwrap();
        let saved: String = db.conn.lock().unwrap().query_row("SELECT content FROM conversations WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(saved, "what is on my calendar");
        assert_eq!(history.len(), 2);
    }

    /// A request sent again from the error card answers the message the
    /// failed request saved; it is not saved a second time.
    #[test]
    fn a_retried_turn_is_not_saved_twice() {
        let db = crate::db::test_database();
        let (failed, _) = save_turn(&db, "s1", "move my 3pm").unwrap();
        let (retried, history) = save_retried_turn(&db, "s1", "move my 3pm").unwrap();
        assert_eq!(retried, failed);
        assert_eq!(history, [("user".to_string(), "move my 3pm".to_string())]);
        // Anything after it (a reply, another message) means a new turn.
        persist_scoped_reply(&db.conn.lock().unwrap(), "done", "s1").unwrap();
        let (again, history) = save_retried_turn(&db, "s1", "move my 3pm").unwrap();
        assert_ne!(again, failed);
        assert_eq!(history.len(), 3);
    }

    /// A recording is saved with its message: a history read landing between
    /// the two would show the message as one that cannot be played back.
    #[test]
    fn what_is_saved_with_a_turn_is_saved_before_anyone_else_can_read_the_database() {
        let db = crate::db::test_database();
        let mut seen = None;
        let (id, _) = save_turn_with(&db, "legacy", "what is on my calendar", |conn, id| {
            assert!(db.conn.try_lock().is_err(), "the database must still be held");
            seen = conn.query_row("SELECT content FROM conversations WHERE id = ?1", [id], |r| r.get::<_, String>(0)).ok();
        })
        .unwrap();
        assert!(id > 0);
        assert_eq!(seen.as_deref(), Some("what is on my calendar"));
    }

    #[test]
    fn history_marks_the_voice_requests_that_can_be_played_back() {
        let db = crate::db::test_database();
        crate::voice::recordings::save_spoken_turn(&db, "what is on my calendar", &[0.1; 1600]).unwrap();
        save_turn(&db, "legacy", "typed this one").unwrap();
        let conn = db.conn.lock().unwrap();
        let marks: Vec<(String, bool)> = history(&conn, "legacy", None).unwrap().into_iter().map(|m| (m.content, m.has_recording)).collect();
        assert_eq!(marks, [("what is on my calendar".to_string(), true), ("typed this one".to_string(), false)]);
    }

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

    /// The user's message is saved before the search runs, so it always
    /// matched its own topic: "search for flights" lost the session history.
    #[test]
    fn a_conversation_search_skips_the_message_being_answered() {
        let db = crate::db::test_database();
        save_turn(&db, "s1", "book flights to Tokyo").unwrap();
        let (current, _) = save_turn(&db, "s1", "search for the DHL invoice").unwrap();
        assert!(search_history(&db, "s1", "the DHL invoice", Some(current)).unwrap().is_empty());
        let (next, _) = save_turn(&db, "s1", "search for flights").unwrap();
        let found = search_history(&db, "s1", "flights", Some(next)).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].1, "book flights to Tokyo");
    }

    #[test]
    fn session_migration_retains_existing_legacy_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE conversations(id INTEGER PRIMARY KEY, role TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT DEFAULT CURRENT_TIMESTAMP); INSERT INTO conversations(role,content) VALUES('user','Before migration');").unwrap();
        conn.execute_batch(include_str!("../../migrations/V24__chat_sessions.sql")).unwrap();
        conn.execute_batch(include_str!("../../migrations/V32__voice_recordings.sql")).unwrap();
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

    /// Hong Kong users mix English into Cantonese with no space between.
    #[test]
    fn code_mixed_cantonese_pulls_in_personal_context() {
        assert!(asks("有冇email"));
        assert!(asks("睇下inbox"));
        assert!(asks("有冇新mail"));
        assert!(asks("呢單要跟進"));
        assert!(asks("我要覆邊個"));
        assert!(!asks("幫我寫首詩"));
    }

    /// "pr" used to match inside "prefer" and "project", so ordinary sentences
    /// pulled in the [CONTEXT] block and tainted the turn.
    #[test]
    fn a_trigger_inside_another_word_does_not_count() {
        assert!(!asks("remember I prefer tea"));
        assert!(!asks("my project is called Atlas"));
        assert!(!asks("add footnotes to the essay"));
        assert!(!asks("explain multitasking"));
    }

    #[test]
    fn whole_word_triggers_and_plurals_still_count() {
        assert!(asks("any open PRs"));
        assert!(asks("review this PR"));
        assert!(asks("what tasks are due"));
        assert!(asks("show my reminders"));
        assert!(asks("what should I work on next"));
        assert!(asks("am I free at 3?"));
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
        conn.execute_batch(include_str!("../../migrations/V32__voice_recordings.sql")).unwrap();
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

    /// A reply that quoted an email is replayed with every later message of
    /// the session. After a move to an AI that may not read email, the turn
    /// sends the new message and nothing saved before the move.
    #[test]
    fn a_turn_after_moving_to_an_ai_that_may_not_read_email_replays_nothing_saved_before() {
        let contents = |history: Vec<(String, String)>| history.into_iter().map(|(_, content)| content).collect::<Vec<_>>();
        let db = crate::ai::mail_policy::test_support::moved_to_kimi_after(|db| {
            let sent = save_turn_and_load_history(db, "s1", "What did Kenny's email say?").unwrap();
            assert_eq!(sent.len(), 1);
            persist_scoped_reply(&db.conn.lock().unwrap(), "Kenny asks for HK$12,000", "s1").unwrap();
        });

        assert_eq!(contents(save_turn_and_load_history(&db, "s1", "hello").unwrap()), ["hello"]);
        persist_scoped_reply(&db.conn.lock().unwrap(), "Hi", "s1").unwrap();
        assert_eq!(contents(save_turn_and_load_history(&db, "s1", "and again").unwrap()), ["hello", "Hi", "and again"]);

        assert!(search_history(&db, "s1", "Kenny", None).unwrap().is_empty());
        assert_eq!(search_history(&db, "s1", "hello", None).unwrap().len(), 1);
    }

    /// The AI can be changed while a turn that read an email is still
    /// running. Its reply is saved after the change and must not be replayed.
    #[test]
    fn the_reply_of_a_turn_still_running_when_the_ai_changed_is_not_replayed() {
        let db = crate::ai::mail_policy::test_support::moved_to_kimi_after(|db| {
            save_turn_and_load_history(db, "s1", "What did Kenny's email say?").unwrap();
        });
        // After the change the running turn lists tasks, then saves its reply.
        crate::ai::mail_policy::withheld(&db);
        persist_scoped_reply(&db.conn.lock().unwrap(), "Kenny asks for HK$12,000", "s1").unwrap();

        let sent = save_turn_and_load_history(&db, "s1", "hello").unwrap();
        assert_eq!(sent, vec![("user".to_string(), "hello".to_string())]);
    }

    // P06, Google Limited Use (DECISIONS 2026-11-17): a model picked once in
    // the error card can answer a mail question while the chosen AI may not
    // receive Gmail and Calendar content.

    const KENNY: &str = "Kenny asks for HK$12,000";

    fn openai_pick(base_url: Option<&str>) -> crate::ai::capability::Pick {
        crate::ai::capability::Pick { provider_type: "openai".into(), base_url: base_url.map(str::to_string), endpoint_id: None, model_id: None }
    }

    fn contents(history: Vec<(String, String)>) -> Vec<String> {
        history.into_iter().map(|(_, content)| content).collect()
    }

    /// What a chat turn saves, without a model: the message, begun as
    /// send_scoped begins it (off the async threads), a memory and a task as
    /// the tools save them, and the reply.
    async fn a_mail_turn(db: Arc<Database>) -> Result<ChatMessage, String> {
        let begun = db.clone();
        crate::ai::capability::spawn_blocking(move || begin_turn(&begun, "s1", "What did Kenny's email say?", false)).await??;
        let conn = db.conn.lock().unwrap();
        conn.execute("INSERT INTO user_memory (content) VALUES ('Kenny is owed HK$12,000')", []).unwrap();
        conn.execute("INSERT INTO tasks (title) VALUES ('Pay Kenny HK$12,000')", []).unwrap();
        persist_scoped_reply(&conn, KENNY, "s1").unwrap();
        Ok(ChatMessage { id: None, role: "assistant".into(), content: KENNY.into(), created_at: None, has_recording: false, note: None })
    }

    /// Kimi chosen after OpenAI, one turn on Kimi since (the line is drawn
    /// under what OpenAI saw), then the user picks OpenAI in the card for a
    /// mail question. The answer and its note.
    async fn picked_after_moving_to(chosen: impl FnOnce(&Database)) -> (Arc<Database>, ChatMessage) {
        let db = moved_and_answered(chosen);
        let answer = on_pick(&db, Some(openai_pick(None)), a_mail_turn(db.clone())).await.unwrap();
        (db, answer)
    }

    fn moved_and_answered(chosen: impl FnOnce(&Database)) -> Arc<Database> {
        let db = Arc::new(crate::db::test_database());
        crate::ai::mail_policy::test_support::use_ai(&db, "openai", None);
        crate::ai::mail_policy::withheld(&db);
        chosen(&db);
        save_turn(&db, "s1", "hello").unwrap();
        persist_scoped_reply(&db.conn.lock().unwrap(), "Hi", "s1").unwrap();
        db
    }

    fn kimi(db: &Database) {
        crate::ai::mail_policy::test_support::use_ai(db, "openai", crate::ai::mail_policy::test_support::KIMI);
    }

    fn newest(db: &Database, table: &str) -> i64 {
        db.conn.lock().unwrap().query_row(&format!("SELECT MAX(id) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    /// What the chosen AI is shown on its next turn: none of the picked
    /// turn (the question, the answer, the memory and the task it saved).
    /// "Send again" for a run that carried out an action is refused in
    /// both languages; a run that did not, and a first send, go ahead
    /// (review A, M-1).
    #[test]
    fn sending_again_a_run_that_carried_out_an_action_is_refused() {
        use crate::ai::bedrock::UiLanguage;
        let acted = uuid::Uuid::new_v4().to_string();
        crate::ai::failure::note_acted(&acted);
        assert_eq!(refuse_acted_retry(Some(&acted), UiLanguage::En).unwrap_err(), "This request already carried out an action, such as creating an event or a draft. JARVIS does not send it again, so the action is not done twice.");
        assert_eq!(refuse_acted_retry(Some(&acted), UiLanguage::ZhHk).unwrap_err(), "這個請求已執行了操作，例如建立日曆事件或草稿。為免重複執行，JARVIS 不會再次傳送。");
        assert_eq!(refuse_acted_retry(Some(&uuid::Uuid::new_v4().to_string()), UiLanguage::En), Ok(()));
        assert_eq!(refuse_acted_retry(None, UiLanguage::En), Ok(()));
    }

    /// The command takes its session and run ids from the step that refuses
    /// such a "send again", so it cannot skip the refusal (review A, N-2).
    #[test]
    fn the_send_command_refuses_a_run_that_acted_before_anything_runs() {
        let db = crate::db::test_database();
        let acted = uuid::Uuid::new_v4().to_string();
        crate::ai::failure::note_acted(&acted);
        let refused = refuse_acted_retry(Some(&acted), crate::ai::bedrock::UiLanguage::read(&db)).unwrap_err();
        assert_eq!(begin_send(&db, Some("s1".into()), Some("r2".into()), Some(&acted)), Err(refused));
        assert_eq!(begin_send(&db, Some("s1".into()), Some("r2".into()), None), Ok(("s1".to_string(), "r2".to_string())));
    }

    fn assert_next_turn_sees_none_of_it(db: &Database) {
        let (_, history) = save_turn(db, "s1", "And the deadline?").unwrap();
        assert_eq!(contents(history), ["And the deadline?"]);
        assert!(search_history(db, "s1", "Kenny", None).unwrap().is_empty());
        let line = crate::ai::mail_policy::withheld(db);
        assert!(line.memories >= newest(db, "user_memory"), "{line:?}");
        assert!(line.tasks >= newest(db, "tasks"), "{line:?}");
    }

    /// (1) Kimi, whose terms JARVIS cannot confirm, even after the user
    /// agreed to them.
    #[tokio::test]
    async fn a_picked_answer_to_a_mail_question_is_not_replayed_to_an_ai_that_may_not_receive_it() {
        let (db, answer) = picked_after_moving_to(|db| {
            kimi(db);
            crate::ai::mail_policy::test_support::agree_to_kimi(db);
        })
        .await;
        assert_next_turn_sees_none_of_it(&db);
        assert_eq!(answer.note.as_deref(), Some("Kimi will not see this answer or the chat before it: Gmail and Calendar content is not sent to Kimi."));
    }

    /// While a picked turn runs, a job on the saved choice (a background
    /// read on Kimi) is shown none of what the turn has saved so far.
    #[tokio::test]
    async fn while_a_picked_turn_runs_the_chosen_ai_is_shown_none_of_it() {
        let db = moved_and_answered(kimi);
        let turn = db.clone();
        on_pick(&db, Some(openai_pick(None)), async move {
            let answer = a_mail_turn(turn.clone()).await?;
            let line = crate::ai::capability::unpicked(|| crate::ai::mail_policy::withheld(&turn));
            assert!(line.conversations >= newest(&turn, "conversations"), "{line:?}");
            assert!(line.memories >= newest(&turn, "user_memory") && line.tasks >= newest(&turn, "tasks"), "{line:?}");
            Ok(answer)
        })
        .await
        .unwrap();
    }

    /// Another turn on the saved choice can begin while a picked turn runs
    /// (a spoken request) and draw the line there. What the picked turn
    /// saves after that line is still withheld once it ends.
    #[tokio::test]
    async fn a_line_drawn_while_a_picked_turn_runs_still_covers_what_it_saves_later() {
        let db = moved_and_answered(kimi);
        let turn = db.clone();
        on_pick(&db, Some(openai_pick(None)), async move {
            crate::ai::capability::unpicked(|| save_turn(&turn, "legacy", "Set a timer")).unwrap();
            a_mail_turn(turn.clone()).await
        })
        .await
        .unwrap();
        assert_next_turn_sees_none_of_it(&db);
    }

    /// (3) Gemini as the chosen AI, its billing not confirmed.
    #[tokio::test]
    async fn a_picked_answer_is_not_replayed_to_gemini_before_consent() {
        let (db, answer) = picked_after_moving_to(|db| crate::ai::mail_policy::test_support::use_ai(db, "gemini", None)).await;
        assert_next_turn_sees_none_of_it(&db);
        assert_eq!(
            answer.note.as_deref(),
            Some("Google Gemini will not see this answer or the chat before it until you agree to share Gmail and Calendar content with it.")
        );
    }

    /// (4) Choosing an AI that may receive it lifts the line, as a move does.
    #[tokio::test]
    async fn moving_to_an_ai_that_may_receive_google_content_lifts_it() {
        let (db, _) = picked_after_moving_to(kimi).await;
        assert_next_turn_sees_none_of_it(&db);
        crate::ai::mail_policy::test_support::use_ai(&db, "openai", None);
        let (_, history) = save_turn(&db, "s1", "Thanks").unwrap();
        assert!(contents(history).contains(&KENNY.to_string()));
        assert_eq!(crate::ai::mail_policy::withheld(&db), crate::ai::mail_policy::Withheld::default());
    }

    /// An answer from a model picked once changes nothing when the chosen AI
    /// may receive the content too, and the answer carries no note.
    #[tokio::test]
    async fn a_pick_while_the_chosen_ai_may_receive_it_withholds_nothing() {
        let db = Arc::new(crate::db::test_database());
        crate::ai::mail_policy::test_support::use_ai(&db, "claude", None);
        let answer = on_pick(&db, Some(openai_pick(None)), a_mail_turn(db.clone())).await.unwrap();
        assert_eq!(answer.note, None);
        let (_, history) = save_turn(&db, "s1", "Thanks").unwrap();
        assert!(contents(history).contains(&KENNY.to_string()));
    }

    /// The other way round: OpenAI chosen, Kimi picked once in a card. The
    /// turn begins in a blocking thread, which must still see the pick, or
    /// Kimi would be sent the calendar titles and the chat that quoted mail.
    #[tokio::test]
    async fn a_picked_ai_that_may_not_receive_google_content_is_sent_none() {
        let db = Arc::new(crate::db::test_database());
        crate::ai::mail_policy::test_support::use_ai(&db, "openai", None);
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO calendar_events (google_id, summary, start_time, end_time, location) VALUES ('e1', 'Term sheet call', ?1, ?1, 'Cyberport')",
                [chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)],
            )
            .unwrap();
        save_turn(&db, "s1", "What did Kenny's email say?").unwrap();
        persist_scoped_reply(&db.conn.lock().unwrap(), KENNY, "s1").unwrap();

        let begun = db.clone();
        let (history, context, _, _) = crate::ai::capability::with_pick(
            Some(openai_pick(crate::ai::mail_policy::test_support::KIMI)),
            crate::ai::capability::spawn_blocking(move || begin_turn(&begun, "s1", "What is on my calendar today?", false)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(contents(history), ["What is on my calendar today?"]);
        let sent = format!("{context:?}");
        assert!(!sent.contains("Term sheet") && !sent.contains("Cyberport"), "{sent}");

        // OpenAI, still the chosen AI, is shown everything as before.
        let (_, history) = save_turn(&db, "s1", "Thanks").unwrap();
        assert!(contents(history).contains(&KENNY.to_string()));
    }

    /// When the line cannot be read everything saved is withheld, but the
    /// message the user has just typed is still sent.
    #[test]
    fn a_line_that_cannot_be_read_still_sends_the_new_message() {
        let db = crate::ai::mail_policy::test_support::moved_to_kimi_after(|db| {
            save_turn_and_load_history(db, "s1", "What did Kenny's email say?").unwrap();
        });
        save_turn_and_load_history(&db, "s1", "hello").unwrap();
        db.conn.lock().unwrap().execute("UPDATE user_preferences SET value = 'not a line' WHERE key = 'mail_withheld_through'", []).unwrap();

        let sent = save_turn_and_load_history(&db, "s1", "and again").unwrap();
        assert_eq!(sent, vec![("user".to_string(), "and again".to_string())]);
    }

    /// The note under an answer from a local model without tools, in the
    /// UI language. It is not in the saved reply, so a reloaded history
    /// shows the reply alone; an answer with tools has none.
    #[test]
    fn an_answer_without_tools_carries_the_note_and_the_history_does_not() {
        use crate::ai::bedrock::UiLanguage;
        let db = crate::db::test_database();
        save_turn(&db, "s1", "What is on my calendar?").unwrap();
        let conn = db.conn.lock().unwrap();
        let noted = answer(&conn, "Hi.".into(), "s1", true, UiLanguage::En).unwrap();
        assert_eq!(noted.content, "Hi.");
        assert_eq!(noted.note.as_deref(), Some("This model cannot use tools, so this answer did not check your mail, calendar, tasks or apps. You can choose a model that supports tools in Settings."));
        let chinese = answer(&conn, "你好。".into(), "s1", true, UiLanguage::ZhHk).unwrap();
        assert_eq!(chinese.note.as_deref(), Some("此模型不能使用工具，這個回覆沒有查看你的郵件、日曆、工作或 App。可在設定中選擇支援工具的模型。"));
        let with_tools = answer(&conn, "Hello.".into(), "s1", false, UiLanguage::En).unwrap();
        assert_eq!(with_tools.note, None);
        drop(conn);
        let (_, history) = save_turn(&db, "s1", "And tomorrow?").unwrap();
        assert_eq!(contents(history), ["What is on my calendar?", "Hi.", "你好。", "Hello.", "And tomorrow?"]);
    }

    /// Both reply paths of `send_scoped` (web search and the normal turn)
    /// answer through `answered`, the one place that reads the UI language
    /// before it takes the database lock. `send_scoped` itself takes no
    /// lock, saves no answer, and reads no language, so it cannot lock the
    /// database twice (C-1). The test above calls `answered` directly; this
    /// reads the source, because `send_scoped` needs the running app.
    #[test]
    fn send_scoped_answers_only_through_answered() {
        let shipped = include_str!("chat.rs").split("\n#[cfg(test)]\nmod tests {").next().unwrap();
        let start = shipped.find("\nasync fn send_scoped(").expect("send_scoped");
        let body = &shipped[start..];
        let body = &body[..body.find("\n}\n").expect("the end of send_scoped")];
        assert_eq!(body.matches("answered(db, &session_id, async").count(), 2, "both reply paths answer through answered");
        let outside = body.replace("answered(", "");
        for taken in ["conn.lock()", "answer(", "UiLanguage::read", "read_language_pref", "persist_scoped_reply"] {
            assert!(!outside.contains(taken), "send_scoped calls {taken} itself");
        }
    }

    /// What both chat paths run once the request returns (`answered`): the
    /// answer comes back, the reply is saved, the note shows in the saved UI
    /// language when the local model ran without tools, and a failed request
    /// saves nothing. Run on its own thread with a time limit: round 2 read
    /// the language while holding the database lock, which it takes again,
    /// and the turn never returned.
    #[test]
    fn a_turn_is_answered_without_taking_the_database_lock_twice() {
        use crate::ai::local::backend::{BackendType, LocalEndpoint};
        let db = Arc::new(crate::db::test_database());
        save_turn(&db, "s1", "What is on my calendar?").unwrap();
        db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('ui_locale', 'zh-HK')", []).unwrap();
        let chat_only = crate::ai::local::LocalChat::plain(LocalEndpoint { id: "lm".into(), name: "Local".into(), url: "http://127.0.0.1:9".into(), backend_type: BackendType::Ollama, api_key: None, use_tls: false, connection_timeout_ms: 5000, keep_alive_minutes: 30, is_active: true, last_health_check: None, last_health_status: None });
        let (sender, received) = std::sync::mpsc::channel();
        let on_thread = db.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let answers = runtime.block_on(async {
                let with_tools = answered(&on_thread, "s1", async { Ok("Hi.".to_string()) }).await;
                let without_tools = answered(&on_thread, "s1", async {
                    let _ = chat_only.prepare(String::new(), Vec::new(), &[]);
                    Ok("你好。".to_string())
                })
                .await;
                let failed = answered(&on_thread, "s1", async { Err("The provider refused the request.".to_string()) }).await;
                (with_tools, without_tools, failed)
            });
            let _ = sender.send(answers);
        });
        let (with_tools, without_tools, failed) = received.recv_timeout(std::time::Duration::from_secs(5)).expect("no answer within 5 s: the database lock was taken twice");
        let with_tools = with_tools.unwrap();
        assert_eq!((with_tools.content.as_str(), with_tools.note), ("Hi.", None));
        let without_tools = without_tools.unwrap();
        assert_eq!(without_tools.content, "你好。");
        assert_eq!(without_tools.note.as_deref(), Some("此模型不能使用工具，這個回覆沒有查看你的郵件、日曆、工作或 App。可在設定中選擇支援工具的模型。"));
        assert_eq!(failed.err().as_deref(), Some("The provider refused the request."));
        let (_, history) = save_turn(&db, "s1", "And tomorrow?").unwrap();
        assert_eq!(contents(history), ["What is on my calendar?", "Hi.", "你好。", "And tomorrow?"]);
    }

    /// When P06's note under a picked answer applies too, both show, one per
    /// line; a turn that was not picked keeps its own note.
    #[tokio::test]
    async fn both_notes_show_one_per_line() {
        let db = moved_and_answered(|db| {
            kimi(db);
            crate::ai::mail_policy::test_support::agree_to_kimi(db);
        });
        let note = crate::ai::local::no_tools_answer_note(crate::ai::bedrock::UiLanguage::En);
        let reply = |note: &str| ChatMessage { id: None, role: "assistant".into(), content: "Hi.".into(), created_at: None, has_recording: false, note: Some(note.to_string()) };
        let picked = on_pick(&db, Some(openai_pick(None)), std::future::ready(Ok(reply(&note)))).await.unwrap();
        assert_eq!(picked.note, Some(format!("{note}\nKimi will not see this answer or the chat before it: Gmail and Calendar content is not sent to Kimi.")));
        let not_picked = on_pick(&db, None, std::future::ready(Ok(reply(&note)))).await.unwrap();
        assert_eq!(not_picked.note, Some(note));
    }
}
