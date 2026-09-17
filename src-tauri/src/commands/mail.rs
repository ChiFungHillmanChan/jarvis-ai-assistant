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
    },
};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn mail_workspace(db: State<Arc<Database>>) -> Result<MailWorkspace, String> {
    Ok(MailWorkspace {
        accounts: store::list_accounts(&db)?,
        groups: store::list_groups(&db)?,
        settings: store::settings(&db)?,
    })
}
#[tauri::command]
pub async fn mail_connect_account(
    db: State<'_, Arc<Database>>,
    state: State<'_, MailSyncState>,
) -> Result<MailAccount, String> {
    sync::connect(&db, &state).await
}
#[tauri::command]
pub async fn mail_disconnect_account(
    db: State<'_, Arc<Database>>,
    state: State<'_, MailSyncState>,
    account_id: String,
) -> Result<(), String> {
    sync::disconnect(&db, &state, &account_id).await
}
#[tauri::command]
pub async fn mail_remove_account(
    db: State<'_, Arc<Database>>,
    state: State<'_, MailSyncState>,
    account_id: String,
) -> Result<(), String> {
    sync::remove(&db, &state, &account_id).await
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
#[tauri::command]
pub fn mail_list_messages(
    db: State<Arc<Database>>,
    account_id: Option<String>,
    group_id: Option<i64>,
    query: Option<String>,
    offset: Option<u32>,
) -> Result<MailPage, String> {
    store::list_messages(
        &db,
        account_id.as_deref(),
        group_id,
        query.as_deref().unwrap_or(""),
        offset.unwrap_or(0),
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
    group_id: i64,
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
        Some(select_openai_key(
            std::env::var("OPENAI_API_KEY").ok(),
            || {
                crate::security::keychain::KeychainStore::get(
                    crate::ai::KEY_SERVICE,
                    "openai_api_key",
                )
            },
        )?)
    } else {
        None
    };
    analysis::analyze(context, creds, openai_key).await
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
