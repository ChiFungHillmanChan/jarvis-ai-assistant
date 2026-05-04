use crate::ai::{AiRouter, Workload};
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::gmail::{self, GmailMessage};
use std::sync::Arc;

/// Run the daily email briefing for today (HKT date = UTC+8).
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
    // Compute today's date in HKT (UTC+8).
    // chrono-tz is not in Cargo.toml; use a manual +8h offset instead.
    let date = {
        let now_utc = chrono::Utc::now();
        let hkt = now_utc + chrono::Duration::hours(8);
        hkt.format("%Y-%m-%d").to_string()
    };

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

    // Step 2: Haiku 4.5 — 5-bullet summary + 3 suggested actions.
    let (summary_md, actions_json) =
        summarize_with_haiku(&classified, &router_val, &db, &auth, &app_handle, "en").await?;

    // Persist.
    let now_ts = chrono::Utc::now().timestamp();
    {
        let conn = db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
        conn.execute(
            "INSERT INTO daily_briefings \
             (date_hkt, summary_md, actions_json, language, created_at) \
             VALUES (?1, ?2, ?3, 'en', ?4)",
            rusqlite::params![&date, &summary_md, &actions_json, now_ts],
        )
        .map_err(|e| format!("db insert: {e}"))?;
    }

    log::info!("[daily_briefing] persisted briefing for {}", date);
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
