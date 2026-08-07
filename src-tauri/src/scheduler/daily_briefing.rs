use crate::ai::{AiRouter, Workload};
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::gmail::{self, GmailMessage};
use std::sync::Arc;

/// The dedupe key for `daily_briefings.date_hkt`: the machine's local calendar
/// day, formatted the way tasks.deadline and Google's all-day events are
/// ("YYYY-MM-DD"). Kept as a function so the test below exercises the same
/// formatting the insert uses.
fn today_local() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Run the daily email briefing for today, where "today" is the user's local
/// calendar day -- the same definition assistant::context, the calendar queries
/// and the scheduler's deadline checks use.
///
/// 1. Skips if a row for today's date already exists in `daily_briefings`.
/// 2. Checks that `briefing_enabled = 1` in `user_preferences`.
/// 3. Fetches recent inbox messages (up to 50) via Gmail.
/// 4. Uses `Workload::Volume` (Nova Micro) to classify each email as
///    urgent | important | fyi | noise.
/// 5. Uses `Workload::Tools` (Haiku 4.5) to write a 5-bullet markdown
///    summary and up to 3 suggested actions in JSON.
/// 6. Persists the result to `daily_briefings`.
///
/// The `app_handle` is required because `AiRouter::send` emits Tauri events
/// (chat-provider, chat-token, chat-status) to the frontend.
///
/// # Cron auto-trigger: DEFERRED
///
/// The existing `Scheduler` struct (scheduler/mod.rs) only holds `db` and
/// `google_auth`. It does not hold `AiRouter` or `AppHandle`, both of which
/// are required by `AiRouter::send`. Restructuring `Scheduler::new` to accept
/// these is left for a follow-up task. For now, use the
/// `trigger_daily_briefing` Tauri command for on-demand execution.
///
/// The router is passed as `Arc<std::sync::Mutex<AiRouter>>`. Internally this
/// function clones the router out of the Mutex before any `.await` to satisfy
/// the `Send` bound on Tauri async commands (std::sync::MutexGuard is not
/// Send and cannot be held across await points).
pub async fn run_briefing(
    db: Arc<Database>,
    router: Arc<std::sync::Mutex<AiRouter>>,
    auth: Arc<GoogleAuth>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // The dedupe key is the machine's local calendar day. A hardcoded +8h
    // offset was wrong for every user outside HKT and for a HK user abroad --
    // the key rolled over at 16:00 or 17:00 local, so the "already briefed
    // today" check disagreed with every other "today" in the app. Rows written
    // under the old scheme are re-keyed by
    // migrations/V19__daily_briefing_local_date.sql: without that, a stale row
    // dated a day ahead makes the skip check below swallow that day's briefing.
    // The column is still named date_hkt (V15); renaming it needs a migration.
    let date = today_local();

    // Skip if already generated today.
    {
        let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM daily_briefings WHERE date_hkt = ?1",
                rusqlite::params![&date],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if exists {
            log::info!("[daily_briefing] already generated for {}, skipping", date);
            return Ok(());
        }
    }

    // Check briefing_enabled preference.
    {
        let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
        let enabled: String = conn
            .query_row(
                "SELECT value FROM user_preferences WHERE key = 'briefing_enabled'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| "0".to_string());
        if enabled != "1" {
            log::info!("[daily_briefing] briefing_enabled != 1, skipping");
            return Ok(());
        }
    }

    // Clone router out of the Mutex so we can call .send() (which is async)
    // without holding a non-Send MutexGuard across .await points.
    let router_val: AiRouter = router
        .lock()
        .map_err(|e| format!("router lock: {e}"))?
        .clone();

    // Fetch Gmail access token.
    let access_token = match auth.ensure_access_token().await {
        Ok(t) => t,
        Err(_) => {
            log::warn!("[daily_briefing] not authenticated with Google, skipping");
            return Ok(());
        }
    };

    // Fetch up to 50 inbox messages.
    let messages = match gmail::fetch_inbox(&access_token, 50).await {
        Ok(m) => m,
        Err(ref e) if e == "UNAUTHORIZED" => {
            auth.refresh_access_token().await?;
            let refreshed = auth
                .get_access_token()
                .ok_or_else(|| "No token after refresh".to_string())?;
            gmail::fetch_inbox(&refreshed, 50)
                .await
                .map_err(|e| format!("gmail fetch (retry): {e}"))?
        }
        Err(e) => return Err(format!("gmail fetch: {e}")),
    };

    if messages.is_empty() {
        log::info!("[daily_briefing] inbox empty, nothing to brief");
        return Ok(());
    }

    log::info!(
        "[daily_briefing] {} messages fetched, classifying ...",
        messages.len()
    );

    // Step 1: Nova Micro — classify importance.
    let classified =
        classify_with_nova(&messages, &router_val, &db, &auth, &app_handle).await?;

    // Read language preference before summarising so the prompt and persisted
    // record both reflect the user's actual setting.
    let language = crate::ai::tools::read_language_pref(&db);

    // Step 2: Haiku 4.5 — 5-bullet summary + 3 suggested actions.
    let (summary_md, actions_json) =
        summarize_with_haiku(&classified, &router_val, &db, &auth, &app_handle, &language).await?;

    // Persist.
    let now_ts = chrono::Utc::now().timestamp();
    {
        let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
        conn.execute(
            "INSERT INTO daily_briefings \
             (date_hkt, summary_md, actions_json, language, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![&date, &summary_md, &actions_json, &language, now_ts],
        )
        .map_err(|e| format!("db insert: {e}"))?;
    }

    log::info!("[daily_briefing] persisted briefing for {}", date);

    // Speak the briefing if voice is enabled in user preferences.
    let speak_briefing = {
        let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
        conn.query_row(
            "SELECT value FROM user_preferences WHERE key = 'tts_enabled'",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "0".into())
            == "1"
    };
    if speak_briefing {
        speak_briefing_via_router(&db, &summary_md, &language).await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers (take &AiRouter by value — no Mutex involved)
// ---------------------------------------------------------------------------

async fn classify_with_nova(
    messages: &[GmailMessage],
    router: &AiRouter,
    db: &Arc<Database>,
    auth: &Arc<GoogleAuth>,
    app_handle: &tauri::AppHandle,
) -> Result<Vec<(GmailMessage, String)>, String> {
    let email_list: String = messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            format!(
                "[{}] from={} subject={} snippet={}",
                i,
                m.sender.as_deref().unwrap_or("unknown"),
                m.subject.as_deref().unwrap_or("(no subject)"),
                m.snippet
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Classify each email by importance. Return ONLY a JSON array \
         (no prose, no code fence) of {{\"id\": \"<index>\", \"class\": \
         \"urgent|important|fyi|noise\"}} in the same order as the input. \
         Emails:\n{email_list}"
    );

    let chat_messages = vec![("user".to_string(), prompt)];

    let response = router
        .send(chat_messages, db, auth, app_handle, None, Workload::Volume)
        .await
        .map_err(|e| format!("nova classify: {e}"))?;

    // Parse JSON — LLM may wrap in code fence; extract the array if needed.
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&response)
        .or_else(|_| {
            if let (Some(start), Some(end)) = (response.find('['), response.rfind(']')) {
                serde_json::from_str(&response[start..=end])
            } else {
                Ok(vec![])
            }
        })
        .unwrap_or_default();

    let result = messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let class = parsed
                .get(i)
                .and_then(|v| v.get("class"))
                .and_then(|c| c.as_str())
                .unwrap_or("fyi")
                .to_string();
            (m.clone(), class)
        })
        .collect();

    Ok(result)
}

async fn summarize_with_haiku(
    classified: &[(GmailMessage, String)],
    router: &AiRouter,
    db: &Arc<Database>,
    auth: &Arc<GoogleAuth>,
    app_handle: &tauri::AppHandle,
    language: &str,
) -> Result<(String, String), String> {
    // Only summarize urgent/important emails.
    let important: Vec<&(GmailMessage, String)> = classified
        .iter()
        .filter(|(_, c)| c == "urgent" || c == "important")
        .collect();

    if important.is_empty() {
        log::info!("[daily_briefing] no urgent/important emails — empty summary");
        return Ok(("(No urgent emails today)".into(), "[]".into()));
    }

    let email_list: String = important
        .iter()
        .map(|(m, _)| {
            format!(
                "- from={} subject={} snippet={}",
                m.sender.as_deref().unwrap_or("unknown"),
                m.subject.as_deref().unwrap_or("(no subject)"),
                m.snippet
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(300)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are summarizing today's important emails for the user. Output exactly:\n\
         (1) A 5-bullet markdown summary (one bullet per email, ~15 words each).\n\
         (2) On a NEW LINE after the summary, the literal marker `===ACTIONS===`.\n\
         (3) Then a JSON array of up to 3 suggested actions: \
         [{{\"label\": \"...\", \"tool\": \"send_reply|schedule_meeting|create_task\", \
         \"args\": {{}}}}].\n\
         Respond in {language}.\n\nEmails:\n{email_list}"
    );

    let chat_messages = vec![("user".to_string(), prompt)];

    let response = router
        .send(chat_messages, db, auth, app_handle, None, Workload::Tools)
        .await
        .map_err(|e| format!("haiku summarize: {e}"))?;

    // Split on the marker.
    let parts: Vec<&str> = response.splitn(2, "===ACTIONS===").collect();
    let summary = parts.first().unwrap_or(&"").trim().to_string();
    let actions_raw = parts.get(1).unwrap_or(&"[]").trim();

    // Extract JSON array, falling back gracefully.
    let actions = if let (Some(s), Some(e)) = (actions_raw.find('['), actions_raw.rfind(']')) {
        actions_raw[s..=e].to_string()
    } else {
        "[]".to_string()
    };

    // Validate JSON; fall back to empty array on parse error.
    let validated_actions = serde_json::from_str::<serde_json::Value>(&actions)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "[]".to_string());

    Ok((summary, validated_actions))
}

// ---------------------------------------------------------------------------
// TTS dispatch for the briefing
// ---------------------------------------------------------------------------

/// Dispatch the briefing summary through TtsRouter.
///
/// MacSay: invokes the macOS `say` binary directly (works immediately).
/// MiniMax / CosyVoice: the audio byte stream is consumed but not routed to
/// an output device in this implementation. Properly wiring cloud/offline
/// audio output to the system audio sink is the same problem that
/// `tts.rs::speak` solves and will be addressed in the post-B9 tts.rs
/// refactor. The dispatch path exists so TtsRouter selection is exercised and
/// Sin-ji (MacSay) plays for most users at CHECKPOINT 2.
async fn speak_briefing_via_router(
    db: &std::sync::Arc<crate::db::Database>,
    text: &str,
    language: &str,
) -> Result<(), String> {
    use crate::voice::tts_router::{TtsMode, TtsProvider, TtsRequest, TtsRouter};

    let router = TtsRouter::new(db.clone());
    let req = TtsRequest {
        text: text.to_string(),
        mode: TtsMode::Content,
        language: language.to_string(),
    };
    let provider = router.select_provider(&req)?;

    match provider {
        TtsProvider::MacSay { voice } => {
            use tokio::io::AsyncWriteExt;

            // `text` is the model's markdown summary of email written by
            // strangers. `say` parses argv with getopt, so anything starting
            // with '-' (every markdown bullet) is read as an option instead of
            // as speech. With no positional argument `say` reads stdin.
            let _playback = crate::voice::tts::playback_lock().lock().await;
            let mut child = tokio::process::Command::new("say")
                .args(["-v", &voice])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("say spawn: {e}"))?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(text.as_bytes())
                    .await
                    .map_err(|e| format!("say stdin: {e}"))?;
                // Dropped here so `say` sees EOF and starts speaking.
            }
            let status = child.wait().await.map_err(|e| format!("say wait: {e}"))?;
            if !status.success() {
                log::warn!("[daily_briefing] say exited with {}", status);
            }
        }
        TtsProvider::Minimax => {
            let Some(api_key) = crate::integration_keys::resolve("minimax_api_key") else {
                log::warn!(
                    "[daily_briefing] TTS: MiniMax selected but no API key — skipping audio"
                );
                return Ok(());
            };
            let voice_id = match language {
                "yue" => "Cantonese_PrettyGirl",
                _ => "English_FriendlyMan",
            };
            let client = crate::voice::minimax::MinimaxClient::new(api_key);
            let mut rx = client.stream_speak(text, voice_id).await?;
            // Drain stream — audio playback wiring is post-B9.
            while let Some(_chunk) = rx.recv().await {}
            log::info!(
                "[daily_briefing] TTS: MiniMax stream consumed (audio playback wiring is post-B9)"
            );
        }
        TtsProvider::CosyVoice => {
            let model_path = {
                let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
                conn.query_row(
                    "SELECT value FROM user_preferences WHERE key = 'cosyvoice_model_path'",
                    [],
                    |r| r.get::<_, String>(0),
                )
                .unwrap_or_default()
            };
            if model_path.is_empty() {
                log::warn!(
                    "[daily_briefing] TTS: CosyVoice selected but model not downloaded — skipping audio"
                );
                return Ok(());
            }
            let mut rx = crate::voice::cosyvoice::speak(
                text,
                language,
                std::path::Path::new(&model_path),
            )
            .await?;
            // Drain stream — audio playback wiring is post-B9.
            while let Some(_chunk) = rx.recv().await {}
            log::info!(
                "[daily_briefing] TTS: CosyVoice stream consumed (audio playback wiring is post-B9)"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::today_local;

    /// The shipped repair for rows written under the old UTC+8 key. Run here
    /// rather than copied, so the test cannot drift from the migration.
    const V19: &str = include_str!("../../migrations/V19__daily_briefing_local_date.sql");

    /// The briefing's dedupe key must follow the machine's local calendar day,
    /// like every other "today" in the app. It was a hardcoded UTC+8, so for a
    /// UK-based user it rolled over at 16:00/17:00 local and the briefing's
    /// "today" disagreed with the calendar and task queries for the rest of the
    /// evening.
    #[test]
    fn briefing_date_follows_the_machine_local_day() {
        // Sampled either side of the call so a midnight rollover mid-test
        // cannot make this flaky.
        let before = chrono::Local::now().date_naive().to_string();
        let today = today_local();
        let after = chrono::Local::now().date_naive().to_string();
        assert!(
            today == before || today == after,
            "expected the local calendar day ({} or {}), got {}",
            before,
            after,
            today
        );
    }

    /// R2-03: rows written before that fix carry the UTC+8 key, and date_hkt is
    /// UNIQUE, so a stale row dated a day ahead makes the skip check swallow
    /// that day's briefing. V19 re-keys them off created_at. Run against the
    /// V15 schema with 'localtime' pinned to +01:00 -- the UK install the
    /// regression was recorded against -- so the result does not depend on the
    /// zone of the machine running the test.
    #[test]
    fn v19_rekeys_pre_upgrade_briefings_onto_the_local_day() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE daily_briefings (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 date_hkt TEXT NOT NULL UNIQUE,
                 summary_md TEXT NOT NULL,
                 actions_json TEXT NOT NULL,
                 language TEXT NOT NULL DEFAULT 'en',
                 created_at INTEGER NOT NULL
             );",
        )
        .unwrap();

        let at = |rfc3339: &str| {
            chrono::DateTime::parse_from_rfc3339(rfc3339)
                .unwrap()
                .timestamp()
        };
        let insert = |old_key: &str, summary: &str, created: i64| {
            conn.execute(
                "INSERT INTO daily_briefings (date_hkt, summary_md, actions_json, created_at) \
                 VALUES (?1, ?2, '[]', ?3)",
                rusqlite::params![old_key, summary, created],
            )
            .unwrap();
        };

        // 09:00 BST: UTC+8 and the local day agree, so this row is correct.
        insert("2026-07-24", "morning", at("2026-07-24T08:00:00Z"));
        // 18:00 BST: the old key had already rolled over to tomorrow. This is
        // the row that suppressed the next day's briefing.
        insert("2026-07-27", "evening", at("2026-07-26T17:00:00Z"));
        // Both halves of one local day, which now resolve to the same key.
        insert("2026-07-28", "same-day-morning", at("2026-07-28T08:00:00Z"));
        insert("2026-07-29", "same-day-evening", at("2026-07-28T17:00:00Z"));

        let migration = V19.replace("'localtime'", "'+1 hours'");
        conn.execute_batch(&migration).unwrap();

        let mut stmt = conn
            .prepare("SELECT date_hkt, summary_md FROM daily_briefings ORDER BY date_hkt")
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        drop(stmt);

        assert_eq!(
            rows,
            vec![
                ("2026-07-24".to_string(), "morning".to_string()),
                ("2026-07-26".to_string(), "evening".to_string()),
                ("2026-07-28".to_string(), "same-day-evening".to_string()),
            ],
            "every row must sit on the local day it was written on, with the \
             later briefing kept when two land on the same day"
        );

        // today_local() can never collide with a leftover key now, and a second
        // pass changes nothing.
        conn.execute_batch(&migration).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_briefings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }
}
