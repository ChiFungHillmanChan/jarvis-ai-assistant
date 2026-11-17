//! Local mail workspace commands. Deliberately separate from the legacy AI
//! tool registry: reading a mail group never implicitly activates cloud AI.
use crate::{
    aws::credentials::AwsCredsRepo,
    db::Database,
    mail::{
        analysis,
        models::*,
        store,
        sync::{self, MailSyncState},
        triage,
    },
};
use std::sync::Arc;
use tauri::State;

// `async` runs these on a worker thread. A plain command runs on the main
// thread, and the queries take long enough on a large mailbox to stall the window.
#[tauri::command(async)]
pub fn mail_workspace(db: State<Arc<Database>>, account_id: Option<String>) -> Result<MailWorkspace, String> {
    Ok(MailWorkspace {
        accounts: store::list_accounts(&db)?,
        groups: store::list_groups(&db)?,
        settings: store::settings(&db)?,
        triage: store::triage_status(&db, account_id.as_deref().filter(|id| !id.is_empty()), typesafe_key().is_some(), openai_key().is_ok(), chosen_ai(&db).map(|ai| ai.label))?,
    })
}

fn typesafe_key() -> Option<String> {
    crate::integration_keys::resolve("typesafe_api_key")
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn openai_key() -> Result<String, String> {
    select_openai_key(std::env::var("OPENAI_API_KEY").ok(), || {
        crate::security::keychain::KeychainStore::get(crate::ai::KEY_SERVICE, "openai_api_key")
    })
}

/// Classify untriaged local mail. Sends message text to TypeSafe, and for
/// some messages to OpenAI, so it refuses to run until the user has turned
/// triage on in the workspace settings.
#[tauri::command]
pub async fn mail_triage_run(
    db: State<'_, Arc<Database>>,
    language: Option<String>,
) -> Result<MailTriageRun, String> {
    triage_untriaged(&db, language.as_deref()).await
}

/// Manual and background triage run one at a time: two runs would pick the
/// same untriaged messages and pay for them twice.
static TRIAGE_RUN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The AI the user chose in setup, if any; a default chain nobody chose does
/// not count.
fn chosen_ai(db: &Database) -> Option<crate::commands::ai_choice::CurrentAi> {
    crate::commands::ai_choice::describe(&crate::commands::ai_choice::saved_chain(db).ok()?)
}

/// The chosen AI, if it may read email (ai::mail_policy); otherwise why not.
fn chosen_ai_for_mail(db: &Database) -> Result<Option<crate::commands::ai_choice::CurrentAi>, crate::ai::mail_policy::Blocked> {
    match chosen_ai(db) {
        Some(ai) => crate::ai::mail_policy::check(db).map(|_| Some(ai)),
        None => Ok(None),
    }
}

/// Whether the background sync should sort new mail: the user opted in, and
/// either a TypeSafe key or a chosen AI that may read email can classify it.
pub(crate) fn triage_ready(db: &Database) -> bool {
    store::auto_triage_enabled(db).unwrap_or(false)
        && (typesafe_key().is_some() || matches!(chosen_ai_for_mail(db), Ok(Some(_))))
}

pub(crate) async fn triage_untriaged(db: &Database, language: Option<&str>) -> Result<MailTriageRun, String> {
    if !store::auto_triage_enabled(db)? {
        return Err("請先在「分析設定」開啟自動整理。".into());
    }
    let typesafe = typesafe_key();
    // An AI that may not read email is left out; TypeSafe alone can still sort.
    let chosen = match chosen_ai_for_mail(db) {
        Ok(chosen) => chosen,
        Err(blocked) if typesafe.is_none() => return Err(blocked.user_message()),
        Err(_) => None,
    };
    if typesafe.is_none() && chosen.is_none() {
        return Err(crate::ai::NO_AI_CHOSEN.into());
    }
    // Only the AI the user chose reads the mail, with the model chosen with
    // it (none: the latest JARVIS ships). OpenAI keeps its strict-schema path
    // when TypeSafe classifies; that path only summarises, so without TypeSafe
    // OpenAI goes through the router like every other choice.
    let openai_model = chosen
        .as_ref()
        .and_then(|ai| ai.model.clone())
        .unwrap_or_else(|| crate::ai::models::default_for("mail_sort", "openai"));
    let (openai, assistant) = match chosen {
        Some(ai) if ai.provider == "openai" && typesafe.is_some() => (openai_key().ok(), None),
        Some(_) => (None, Some(crate::commands::ai_choice::rebuilt_router(db))),
        None => (None, None),
    };
    let keys = triage::TriageKeys { typesafe, openai, openai_model, assistant };
    let language = if language == Some("en") { "en" } else { "zh-HK" };
    let _run = TRIAGE_RUN.lock().await;
    triage::run(db, keys, language).await
}

#[tauri::command]
pub fn mail_set_auto_triage(db: State<Arc<Database>>, enabled: bool) -> Result<(), String> {
    store::set_auto_triage(&db, enabled)
}

#[tauri::command]
pub fn mail_set_attention(db: State<Arc<Database>>, message_id: i64, attention: bool) -> Result<(), String> {
    store::set_attention(&db, message_id, attention)
}

#[tauri::command]
pub fn mail_list_items(
    db: State<Arc<Database>>,
    message_id: Option<i64>,
    include_closed: Option<bool>,
) -> Result<Vec<MailItem>, String> {
    store::list_items(&db, message_id, include_closed.unwrap_or(false))
}

#[tauri::command]
pub fn mail_update_item(
    db: State<Arc<Database>>,
    item_id: i64,
    status: String,
    remind_at: Option<String>,
) -> Result<MailItem, String> {
    store::update_item(&db, item_id, &status, remind_at.as_deref().filter(|s| !s.is_empty()))
}

/// Add one extracted item to the user's primary Google Calendar. Runs only
/// on an explicit click; no attendees, so nothing leaves the user's calendar.
#[tauri::command]
pub async fn mail_item_to_calendar(
    db: State<'_, Arc<Database>>,
    auth: State<'_, Arc<crate::auth::google::GoogleAuth>>,
    item_id: i64,
) -> Result<MailItem, String> {
    let item = store::get_item(&db, item_id)?;
    if item.calendar_event_id.is_some() {
        return Ok(item);
    }
    let date = item
        .due_date
        .as_deref()
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .ok_or("此項目沒有日期，未能加入日曆。")?;
    let token = auth
        .ensure_access_token()
        .await
        .map_err(|_| "請先在「連接」頁面連接 Google 日曆，再加入日曆。".to_string())?;
    let description = format!(
        "由 JARVIS 電郵工作區建立\n郵件：{}\n寄件者：{}\n依據：{}",
        item.subject, item.sender, item.evidence
    );
    let event_id = match item.due_time.as_deref().and_then(|t| chrono::NaiveTime::parse_from_str(t, "%H:%M").ok()) {
        None => crate::integrations::calendar::create_all_day_event(&token, &item.title, date, Some(&description)).await,
        Some(start_time) => {
            use chrono::TimeZone;
            let start = chrono::Local
                .from_local_datetime(&date.and_time(start_time))
                .earliest()
                .ok_or("時間無效。")?;
            let end = item
                .end_time
                .as_deref()
                .and_then(|t| chrono::NaiveTime::parse_from_str(t, "%H:%M").ok())
                .and_then(|t| chrono::Local.from_local_datetime(&date.and_time(t)).earliest())
                .filter(|end| *end > start)
                .unwrap_or(start + chrono::Duration::hours(1));
            crate::integrations::calendar::create_event(&token, &item.title, &start.to_rfc3339(), &end.to_rfc3339(), Some(&description), item.location.as_deref(), None)
                .await
                .map(|created| created.split(" | ").next().unwrap_or("").to_string())
        }
    }
    .map_err(|e| if e == "UNAUTHORIZED" { "Google 日曆授權已失效，請重新連接。".to_string() } else { "未能加入 Google 日曆，請稍後重試。".to_string() })?;
    store::set_item_calendar_event(&db, item_id, if event_id.is_empty() { "created" } else { &event_id })?;
    store::get_item(&db, item_id)
}
#[tauri::command]
pub async fn mail_connect_account(
    db: State<'_, Arc<Database>>,
    state: State<'_, MailSyncState>,
) -> Result<MailAccount, String> {
    sync::connect(&db, &state).await
}
/// Stop waiting for a mailbox sign-in the user abandoned, such as a closed
/// browser tab; this also releases the Mail lock the sign-in holds.
#[tauri::command]
pub fn mail_cancel_connect() {
    crate::auth::google::cancel_sign_in(crate::auth::google::SignIn::Mailbox);
}
#[tauri::command]
pub async fn mail_disconnect_account(
    db: State<'_, Arc<Database>>,
    state: State<'_, MailSyncState>,
    google: State<'_, Arc<crate::auth::google::GoogleAuth>>,
    account_id: String,
) -> Result<(), String> {
    sync::disconnect(&db, &state, &google, &account_id).await
}
#[tauri::command]
pub async fn mail_remove_account(
    db: State<'_, Arc<Database>>,
    state: State<'_, MailSyncState>,
    google: State<'_, Arc<crate::auth::google::GoogleAuth>>,
    account_id: String,
) -> Result<(), String> {
    sync::remove(&db, &state, &google, &account_id).await
}
#[tauri::command]
pub async fn mail_sync_account(
    db: State<'_, Arc<Database>>,
    state: State<'_, MailSyncState>,
    account_id: String,
    older: bool,
) -> Result<MailSyncResult, String> {
    sync::sync_account(&db, &state, &account_id, older).await
}
#[tauri::command(async)]
pub fn mail_list_messages(
    db: State<Arc<Database>>,
    account_id: Option<String>,
    group_id: Option<i64>,
    query: Option<String>,
    offset: Option<u32>,
    view: Option<String>,
) -> Result<MailPage, String> {
    store::list_messages(
        &db,
        account_id.as_deref(),
        group_id,
        query.as_deref().unwrap_or(""),
        offset.unwrap_or(0),
        view.as_deref().filter(|v| !v.is_empty()),
    )
}

#[tauri::command]
pub async fn mail_restart_sync(
    db: State<'_, Arc<Database>>,
    state: State<'_, MailSyncState>,
    account_id: String,
) -> Result<MailSyncResult, String> {
    sync::restart_sync(&db, &state, &account_id).await
}
#[tauri::command]
pub fn mail_get_message(db: State<Arc<Database>>, message_id: i64) -> Result<MailMessage, String> {
    store::get_message(&db, message_id)
}
#[tauri::command]
pub fn mail_save_group(
    db: State<Arc<Database>>,
    group_id: Option<i64>,
    name: String,
    instructions: String,
) -> Result<MailGroup, String> {
    store::save_group(&db, group_id, &name, &instructions)
}
#[tauri::command]
pub fn mail_delete_group(db: State<Arc<Database>>, group_id: i64) -> Result<(), String> {
    store::delete_group(&db, group_id)
}
#[tauri::command]
pub fn mail_set_group_members(
    db: State<Arc<Database>>,
    group_id: i64,
    message_ids: Vec<i64>,
    add: bool,
) -> Result<(), String> {
    store::set_group_members(&db, group_id, &message_ids, add)
}
#[tauri::command]
pub fn mail_list_memories(
    db: State<Arc<Database>>,
    group_id: i64,
) -> Result<Vec<MailMemory>, String> {
    store::list_memories(&db, group_id)
}
#[tauri::command]
pub fn mail_save_memory(
    db: State<Arc<Database>>,
    input: MailMemoryInput,
) -> Result<MailMemory, String> {
    store::save_memory(&db, &input)
}
#[tauri::command]
pub fn mail_delete_memory(db: State<Arc<Database>>, memory_id: i64) -> Result<(), String> {
    store::delete_memory(&db, memory_id)
}
#[tauri::command]
pub fn mail_save_settings(db: State<Arc<Database>>, input: MailSettings) -> Result<(), String> {
    store::save_settings(&db, &input)
}
#[tauri::command]
pub async fn mail_analyze_group(
    db: State<'_, Arc<Database>>,
    router: State<'_, std::sync::Mutex<crate::ai::AiRouter>>,
    group_id: i64,
    pick: Option<crate::ai::capability::Pick>,
) -> Result<MailAnalysis, String> {
    let context = store::context(&db, group_id)?;
    if context.messages.is_empty() {
        return Err("請先將郵件加入群組，再進行分析。".into());
    }
    let creds = if context.settings.processing_mode == "bedrock" {
        Some(
            AwsCredsRepo::new(db.inner().clone())
                .load()
                .await
                .map_err(|_| "未能讀取 AWS 憑證，請先在設定中連接你的 AWS 帳戶。".to_string())?
                .ok_or("請先在設定中儲存你的 AWS 憑證。")?,
        )
    } else {
        None
    };
    let openai_key = if context.settings.processing_mode == "openai" {
        Some(openai_key()?)
    } else {
        None
    };
    let assistant = if context.settings.processing_mode == "assistant" {
        crate::ai::mail_policy::check(&db).map_err(|blocked| blocked.user_message())?;
        Some(analysis::AssistantRoute {
            router: router.lock().map_err(|e| e.to_string())?.clone(),
            db: db.inner().clone(),
        })
    } else {
        None
    };
    // A model picked in the error card runs this analysis once (P06).
    crate::ai::capability::with_pick(pick, analysis::analyze(context, creds, openai_key, assistant, Some(&db))).await
}

fn select_openai_key(
    environment: Option<String>,
    stored: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    if let Some(key) = environment.filter(|value| !value.trim().is_empty()) {
        return Ok(key.trim().to_string());
    }
    stored()
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "未能讀取 OpenAI API 密鑰，請在設定頁檢查或重新儲存。".to_string())
}

#[cfg(test)]
mod tests {
    use super::select_openai_key;

    #[test]
    fn explicit_environment_key_does_not_access_keychain() {
        assert_eq!(
            select_openai_key(Some("  test-env-key  ".into()), || panic!(
                "unnecessary Keychain access"
            ))
            .unwrap(),
            "test-env-key"
        );
    }

    #[test]
    fn blank_environment_uses_saved_key() {
        for env in [None, Some("   ".into())] {
            assert_eq!(
                select_openai_key(env, || Ok("  test-saved-key  ".into())).unwrap(),
                "test-saved-key"
            );
        }
    }

    #[test]
    fn unusable_saved_keys_fail_without_exposing_errors_or_secrets() {
        for saved in [
            Ok("   ".to_string()),
            Err("sensitive backend detail".to_string()),
        ] {
            let error = select_openai_key(None, || saved).unwrap_err();
            assert!(error.contains("OpenAI"));
            assert!(!error.contains("sensitive backend detail"));
        }
    }
}
