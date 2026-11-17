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

/// The TypeSafe key, while TypeSafe may receive mail (ai::mail_policy,
/// review B I-4); otherwise None, and the reason is raised once per launch.
fn usable_typesafe(db: &Database, key: Option<String>) -> Option<String> {
    let key = key?;
    match crate::ai::mail_policy::may_send_to(db, "typesafe") {
        Ok(()) => Some(key),
        Err(blocked) => {
            blocked.raise("mail_sort");
            None
        }
    }
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
    if !store::auto_triage_enabled(db).unwrap_or(false) {
        return false;
    }
    if usable_typesafe(db, typesafe_key()).is_some() {
        return true;
    }
    match chosen_ai_for_mail(db) {
        Ok(chosen) => chosen.is_some(),
        // Not skipped silently (P06): the consent card, or for content that
        // is not sent to this AI at all a notice, once per launch.
        Err(blocked) => {
            blocked.raise("mail_sort");
            false
        }
    }
}

pub(crate) async fn triage_untriaged(db: &Database, language: Option<&str>) -> Result<MailTriageRun, String> {
    if !store::auto_triage_enabled(db)? {
        return Err("請先在「分析設定」開啟自動整理。".into());
    }
    // The gate is asked once the run's turn has come (review B, I-3): a
    // consent withdrawn while an earlier run held the lock counts. triage_one
    // asks again before each message.
    let _run = TRIAGE_RUN.lock().await;
    let typesafe = usable_typesafe(db, typesafe_key());
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
        Some(analysis::AssistantRoute {
            router: router.lock().map_err(|e| e.to_string())?.clone(),
            db: db.inner().clone(),
        })
    } else {
        None
    };
    analyze_on(pick, &db, context, creds, openai_key, assistant).await
}

/// An analysis, in the scope of the model picked in the error card (P06),
/// if any. On the chosen AI ("assistant" mode) the gate asks about the model
/// the request runs on, so it is asked inside that scope: a pick is gated
/// like the saved choice.
async fn analyze_on(
    pick: Option<crate::ai::capability::Pick>,
    db: &Arc<Database>,
    context: MailContext,
    creds: Option<crate::aws::credentials::AwsCredentials>,
    openai_key: Option<String>,
    assistant: Option<analysis::AssistantRoute>,
) -> Result<MailAnalysis, String> {
    crate::ai::capability::with_pick(pick, async {
        if let Some(route) = &assistant {
            assistant_may_analyze(db, &route.router).await?;
        }
        // The modes on the user's own key follow the registry's terms too
        // (review B, I-4). Not the chosen AI, so no capability check here.
        let own_key = match context.settings.processing_mode.as_str() {
            "openai" => Some("openai"),
            "bedrock" => Some("bedrock"),
            _ => None,
        };
        if let Some(provider) = own_key {
            crate::ai::mail_policy::may_send_to(db, provider).map_err(|blocked| blocked.user_message())?;
        }
        analysis::analyze(context, creds, openai_key, assistant, Some(db)).await
    })
    .await
}

/// Whether an analysis may send the group's mail to the AI the request
/// runs on (ai::mail_policy): the reason when not.
async fn assistant_may_analyze(db: &Database, router: &crate::ai::AiRouter) -> Result<(), String> {
    match crate::ai::mail_policy::check(db) {
        Ok(()) => Ok(()),
        // Not sent to this service: the error card, with the models that may
        // receive it (P06, Google Limited Use).
        Err(crate::ai::mail_policy::Blocked::NotForGoogleContent { message, .. }) => Err(router.google_content_card(db, crate::ai::cost::Feature::MailAnalysis, message).await),
        Err(blocked) => Err(blocked.user_message()),
    }
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

    /// Google's Limited Use rules (DECISIONS, P06 "Google Limited Use",
    /// 2026-11-17) for the two mail paths on the chosen AI. Analysis and AI
    /// sorting send no mail to a service JARVIS cannot confirm (Kimi), even
    /// after the user agreed to its terms; Gemini with billing confirmed and
    /// a safe provider (OpenAI) receive it.
    mod google_content {
        use super::super::{analyze_on, assistant_may_analyze, chosen_ai_for_mail};
        use crate::ai::mail_policy::test_support::{agree_to_gemini, agree_to_kimi, use_ai, KIMI};
        use crate::ai::mail_policy::Blocked;
        use std::sync::Arc;

        fn on_kimi_after_agreeing() -> crate::db::Database {
            let db = crate::db::test_database();
            use_ai(&db, "openai", KIMI);
            agree_to_kimi(&db);
            db
        }

        #[test]
        fn ai_sorting_sends_no_mail_to_an_unknown_terms_service() {
            let db = on_kimi_after_agreeing();
            assert!(matches!(chosen_ai_for_mail(&db), Err(Blocked::NotForGoogleContent { ref label, .. }) if label == "Kimi"));
            let gemini = crate::db::test_database();
            use_ai(&gemini, "gemini", None);
            agree_to_gemini(&gemini);
            assert!(matches!(chosen_ai_for_mail(&gemini), Ok(Some(_))));
            let openai = crate::db::test_database();
            use_ai(&openai, "openai", None);
            assert!(matches!(chosen_ai_for_mail(&openai), Ok(Some(_))));
        }

        const QWEN: Option<&str> = Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1");

        /// A router with keys for Kimi, Qwen and OpenAI given to it, so
        /// nothing is read from the Keychain; `chain` is the saved choice.
        fn router(chain: &[(&str, Option<&str>)]) -> crate::ai::AiRouter {
            let mut router = crate::ai::AiRouter::new("gemini_primary");
            router.set_provider_chain(
                chain
                    .iter()
                    .enumerate()
                    .map(|(position, (provider, base_url))| crate::ai::ProviderChainEntry { position: position as i32, provider_type: provider.to_string(), endpoint_id: None, model_id: base_url.map(|_| "kimi-k2".to_string()), enabled: true, base_url: base_url.map(str::to_string) })
                    .collect(),
            );
            router.set_openai_key("test-openai");
            router.set_compat_key(KIMI.unwrap(), "test-kimi");
            router.set_compat_key(QWEN.unwrap(), "test-qwen");
            router
        }

        fn card(error: &str) -> serde_json::Value {
            crate::ai::failure::parse(error).unwrap_or_else(|| panic!("not the error card: {error}"))
        }

        fn offered(card: &serde_json::Value) -> Vec<(String, Option<String>)> {
            card["alternatives"].as_array().unwrap().iter().map(|a| (a["pick"]["provider_type"].as_str().unwrap().to_string(), a["pick"]["base_url"].as_str().map(str::to_string))).collect()
        }

        /// Option A: the Email page shows the error card, offering the
        /// models that may receive the mail (OpenAI here, not Kimi).
        #[tokio::test]
        async fn mail_analysis_sends_no_mail_to_an_unknown_terms_service() {
            // Qwen can run mail analysis but may not receive the mail either.
            let both = [("openai", KIMI), ("openai", QWEN), ("openai", None)];
            let error = assistant_may_analyze(&on_kimi_after_agreeing(), &router(&both)).await.unwrap_err();
            let card = card(&error);
            assert_eq!(card["code"], "google_content");
            assert!(card["message"].as_str().unwrap().starts_with("Gmail and Calendar content is not sent to Kimi"), "{card}");
            assert_eq!(offered(&card), [("openai".to_string(), None)]);
            let gemini = crate::db::test_database();
            use_ai(&gemini, "gemini", None);
            agree_to_gemini(&gemini);
            assert_eq!(assistant_may_analyze(&gemini, &router(&[("gemini", None)])).await, Ok(()));
            let openai = crate::db::test_database();
            use_ai(&openai, "openai", None);
            assert_eq!(assistant_may_analyze(&openai, &router(&[("openai", None)])).await, Ok(()));
        }

        fn own_key_context(mode: &str) -> crate::mail::models::MailContext {
            crate::mail::models::MailContext {
                group: crate::mail::models::MailGroup { id: 1, name: "Board".into(), instructions: String::new(), message_count: 0 },
                messages: Vec::new(),
                memories: Vec::new(),
                settings: crate::mail::models::MailSettings { processing_mode: mode.into(), local_model: String::new(), bedrock_model: String::new(), openai_model: String::new() },
            }
        }

        /// The Email page's OpenAI-key and Bedrock modes follow the
        /// registry's terms too: a fetched copy that tightens OpenAI or
        /// Bedrock takes effect there (review B, I-4). No key or credentials
        /// are given, so nothing could be sent whatever the gate said.
        #[tokio::test]
        async fn the_email_pages_own_key_modes_follow_a_tightened_registry() {
            use crate::ai::registry::DataTerms;
            let db = Arc::new(crate::db::test_database());
            for (mode, provider) in [("openai", "openai"), ("bedrock", "bedrock")] {
                let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == provider).unwrap().data_terms = DataTerms::Unknown);
                let error = analyze_on(None, &db, own_key_context(mode), None, None, None).await.unwrap_err();
                assert!(error.starts_with("Gmail and Calendar content is not sent to "), "{mode}: {error}");
            }
            let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "openai").unwrap().data_terms = DataTerms::NeedsBillingConfirmation);
            let error = analyze_on(None, &db, own_key_context("openai"), None, None, None).await.unwrap_err();
            let Err(Blocked::NeedsConsent(request)) = crate::ai::mail_policy::may_send_to(&db, "openai") else { panic!("OpenAI needs consent now") };
            assert_eq!(error, request.to_error(), "the consent card");
        }

        /// A TypeSafe key sorts mail only while TypeSafe may receive it.
        #[test]
        fn a_typesafe_key_sorts_only_while_typesafe_may_receive_mail() {
            use crate::ai::registry::DataTerms;
            let db = crate::db::test_database();
            assert_eq!(super::super::usable_typesafe(&db, Some("ts-test".into())).as_deref(), Some("ts-test"));
            assert_eq!(super::super::usable_typesafe(&db, None), None);
            let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "typesafe").unwrap().data_terms = DataTerms::Unknown);
            assert_eq!(super::super::usable_typesafe(&db, Some("ts-test".into())), None);
        }

        /// A model picked in the error card is gated like the saved choice:
        /// with OpenAI saved, picking Kimi for one analysis sends it nothing
        /// (the gate is asked inside the pick's scope).
        #[tokio::test]
        async fn a_picked_unknown_terms_service_gets_no_mail_either() {
            let db = Arc::new(crate::db::test_database());
            use_ai(&db, "openai", None);
            agree_to_kimi(&db);
            let kimi = crate::ai::capability::Pick { provider_type: "openai".into(), base_url: KIMI.map(str::to_string), endpoint_id: None, model_id: Some("kimi-k2".into()) };
            let route = crate::mail::analysis::AssistantRoute { router: router(&[("openai", None), ("openai", KIMI)]), db: db.clone() };
            let context = crate::mail::models::MailContext {
                group: crate::mail::models::MailGroup { id: 1, name: "Board".into(), instructions: String::new(), message_count: 0 },
                messages: Vec::new(),
                memories: Vec::new(),
                settings: crate::mail::models::MailSettings { processing_mode: "assistant".into(), local_model: String::new(), bedrock_model: String::new(), openai_model: String::new() },
            };
            let card = card(&analyze_on(Some(kimi), &db, context, None, None, Some(route)).await.unwrap_err());
            assert_eq!(card["code"], "google_content");
            assert_eq!(card["model"]["pick"]["base_url"], KIMI.unwrap(), "the card names the picked model");
            assert_eq!(offered(&card), [("openai".to_string(), None)]);
        }

        /// A run that waits for an earlier one is judged when its turn
        /// comes (review B, I-3 and R17): Gemini's consent is withdrawn while
        /// the earlier run holds the lock, and the waiting run then refuses
        /// with the consent card. TypeSafe may not receive mail here, so
        /// whatever key the environment holds only the chosen AI could sort;
        /// the mailbox is empty, so nothing could be sent anyway.
        #[tokio::test]
        async fn a_run_waiting_for_an_earlier_one_is_judged_when_its_turn_comes() {
            use crate::ai::registry::DataTerms;
            let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "typesafe").unwrap().data_terms = DataTerms::Unknown);
            let db = crate::db::test_database();
            crate::mail::store::set_auto_triage(&db, true).unwrap();
            use_ai(&db, "gemini", None);
            agree_to_gemini(&db);
            let earlier_run = super::super::TRIAGE_RUN.lock().await;
            let db = &db;
            let (outcome, ()) = tokio::join!(super::super::triage_untriaged(db, Some("en")), async move {
                // The run reaches the lock and waits.
                for _ in 0..3 {
                    tokio::task::yield_now().await;
                }
                crate::ai::consent::revoke(db, &crate::ai::consent::Subject { provider: "gemini".into(), base_url: String::new() }).unwrap();
                drop(earlier_run);
            });
            let error = outcome.expect_err("the consent withdrawn while it waited counts");
            let Err(Blocked::NeedsConsent(request)) = crate::ai::mail_policy::check(db) else { panic!("Gemini needs consent again") };
            assert_eq!(error, request.to_error(), "the consent card");
        }
    }

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
