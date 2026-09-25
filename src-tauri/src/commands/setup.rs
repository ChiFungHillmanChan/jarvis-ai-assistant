//! Read-only mail readiness and an explicit synthetic analysis probe.
use crate::{
    aws::credentials::{AwsCredentials, AwsCredsRepo},
    db::Database,
    mail::{analysis, models::*, store},
    security::keychain::KeychainStore,
};
use serde::{Deserialize, Serialize};
use std::{
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, OnceLock},
};
use tauri::State;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityStatus {
    pub scope: &'static str,
    pub state: &'static str,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub checked_at: Option<String>,
    pub error_code: Option<&'static str>,
    pub credential_available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailSetupStatus {
    pub connected_accounts: usize,
    pub total_accounts: usize,
    pub message_count: i64,
}

#[derive(Serialize)]
pub struct SetupStatus {
    pub mail: CapabilityStatus,
    pub gmail: GmailSetupStatus,
}

struct Verification {
    fingerprint: u64,
    checked_at: String,
}

fn verification() -> &'static Mutex<Option<Verification>> {
    static VERIFIED: OnceLock<Mutex<Option<Verification>>> = OnceLock::new();
    VERIFIED.get_or_init(|| Mutex::new(None))
}

fn selected_model(settings: &MailSettings) -> &str {
    match settings.processing_mode.as_str() {
        "local" => &settings.local_model,
        "bedrock" => &settings.bedrock_model,
        "openai" => &settings.openai_model,
        _ => "",
    }
}

fn openai_key() -> Result<Option<String>, &'static str> {
    if let Some(key) = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
    {
        return Ok(Some(key.trim().to_string()));
    }
    match KeychainStore::get(crate::ai::KEY_SERVICE, "openai_api_key") {
        Ok(key) => Ok(Some(key.trim().to_string()).filter(|key| !key.is_empty())),
        Err(error) => match credential_error(&error) {
            "credential_missing" => Ok(None),
            code => Err(code),
        },
    }
}

fn credential_error(error: &str) -> &'static str {
    if KeychainStore::is_not_found(error) {
        "credential_missing"
    } else {
        "credential_unavailable"
    }
}

struct CredentialCheck {
    aws: Option<AwsCredentials>,
    openai: Option<String>,
    available: bool,
    error_code: Option<&'static str>,
}

async fn credentials(db: Arc<Database>, mode: &str) -> CredentialCheck {
    match mode {
        "openai" => match openai_key() {
            Ok(key) => CredentialCheck {
                available: key.is_some(),
                aws: None,
                openai: key,
                error_code: None,
            },
            Err(code) => CredentialCheck {
                available: false,
                aws: None,
                openai: None,
                error_code: Some(code),
            },
        },
        "bedrock" => match AwsCredsRepo::new(db).load().await {
            Ok(creds) => {
                let creds = creds.filter(|creds| {
                    !creds.access_key_id.trim().is_empty()
                        && !creds.secret_access_key.trim().is_empty()
                });
                CredentialCheck {
                    available: creds.is_some(),
                    aws: creds,
                    openai: None,
                    error_code: None,
                }
            }
            Err(error) => CredentialCheck {
                available: false,
                aws: None,
                openai: None,
                error_code: Some(credential_error(&error)),
            },
        },
        "local" => CredentialCheck {
            available: true,
            aws: None,
            openai: None,
            error_code: None,
        },
        _ => CredentialCheck {
            available: false,
            aws: None,
            openai: None,
            error_code: None,
        },
    }
}

fn fingerprint(settings: &MailSettings, creds: Option<&AwsCredentials>, key: Option<&str>) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    settings.processing_mode.hash(&mut hash);
    settings.local_model.hash(&mut hash);
    settings.bedrock_model.hash(&mut hash);
    settings.openai_model.hash(&mut hash);
    key.hash(&mut hash);
    if let Some(creds) = creds {
        creds.access_key_id.hash(&mut hash);
        creds.secret_access_key.hash(&mut hash);
        creds.region.hash(&mut hash);
        creds.inference_profile.hash(&mut hash);
    }
    hash.finish()
}

fn capability(
    settings: &MailSettings,
    credential_available: bool,
    fingerprint: u64,
    credential_error: Option<&'static str>,
) -> CapabilityStatus {
    let mode = settings.processing_mode.as_str();
    let model = selected_model(settings).trim();
    let configured = matches!(mode, "local" | "openai" | "bedrock") && !model.is_empty();
    let verified = verification().lock().ok().and_then(|record| {
        record
            .as_ref()
            .filter(|record| record.fingerprint == fingerprint)
            .map(|record| record.checked_at.clone())
    });
    CapabilityStatus {
        scope: "mail",
        state: if !configured {
            "not_configured"
        } else if !credential_available {
            "action_required"
        } else if verified.is_some() {
            "ready"
        } else {
            "saved_unverified"
        },
        provider: configured.then(|| mode.to_string()),
        model: configured.then(|| model.to_string()),
        checked_at: verified,
        error_code: if configured && !credential_available {
            Some(credential_error.unwrap_or("credential_missing"))
        } else {
            None
        },
        credential_available,
    }
}

#[tauri::command]
pub async fn get_setup_status(db: State<'_, Arc<Database>>) -> Result<SetupStatus, String> {
    let settings = store::settings(&db)?;
    let accounts = store::list_accounts(&db)?;
    let credential = credentials(db.inner().clone(), &settings.processing_mode).await;
    let fp = fingerprint(
        &settings,
        credential.aws.as_ref(),
        credential.openai.as_deref(),
    );
    Ok(SetupStatus {
        mail: capability(&settings, credential.available, fp, credential.error_code),
        gmail: GmailSetupStatus {
            connected_accounts: accounts.iter().filter(|account| account.connected).count(),
            total_accounts: accounts.len(),
            message_count: accounts.iter().map(|account| account.message_count).sum(),
        },
    })
}

fn synthetic_context(settings: MailSettings) -> MailContext {
    MailContext {
        group: MailGroup {
            id: 1,
            name: "合成設定測試".into(),
            instructions: "這是合成設定測試。只回覆一個 JSON 物件，欄位必須為 deadline（YYYY-MM-DD）、currency（ISO 4217）、budget（整數）、paymentRequired（布林值）、citation（字串，必須使用方括號格式 [郵件 #原信編號]，不可使用陣列）；請從郵件提取事實，不要加入額外文字或 Markdown。".into(),
            message_count: 1,
        },
        messages: vec![MailMessage {
            id: 1,
            account_id: "synthetic".into(),
            account_email: "test@example.invalid".into(),
            provider_id: "synthetic".into(),
            thread_id: None,
            subject: "合成測試".into(),
            sender: "test@example.invalid".into(),
            snippet: "合成測試內容".into(),
            body: Some(
                "完全虛構：請於 2026 年 10 月 15 日前回覆報價。預算上限 HK$1000。今次毋須付款。"
                    .into(),
            ),
            is_read: true,
            is_inbox: true,
            received_at: "2026-09-12T12:00:00Z".into(),
            attachments: None,
            triage: None,
            next_due: None,
        }],
        memories: vec![],
        settings,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SyntheticFacts {
    deadline: String,
    currency: String,
    budget: u64,
    payment_required: bool,
    citation: String,
}

fn synthetic_facts_present(text: &str) -> bool {
    let text = text.trim();
    // Some models wrap an otherwise valid JSON answer in a Markdown fence.
    // Accept only one complete outer fence; extra prose/fields still fail.
    let json = if let Some(fenced) = text.strip_prefix("```json").or_else(|| text.strip_prefix("```")) {
        let Some(inner) = fenced.strip_suffix("```") else { return false; };
        inner.trim()
    } else {
        text
    };
    let Ok(facts) = serde_json::from_str::<SyntheticFacts>(json) else {
        return false;
    };
    facts.deadline == "2026-10-15"
        && facts.currency == "HKD"
        && facts.budget == 1000
        && !facts.payment_required
        && facts.citation == "[郵件 #1]"
}

#[tauri::command]
pub async fn test_mail_ai_connection(
    db: State<'_, Arc<Database>>,
    input: Option<MailSettings>,
) -> Result<CapabilityStatus, String> {
    let settings = match input {
        Some(settings) => settings,
        None => store::settings(&db)?,
    };
    let credential = credentials(db.inner().clone(), &settings.processing_mode).await;
    let fp = fingerprint(
        &settings,
        credential.aws.as_ref(),
        credential.openai.as_deref(),
    );
    let mut status = capability(&settings, credential.available, fp, credential.error_code);
    if status.state == "not_configured" || !credential.available {
        return Ok(status);
    }
    // Only this explicit command sends a synthetic message to the selected provider.
    match analysis::analyze(
        synthetic_context(settings),
        credential.aws,
        credential.openai,
    )
    .await
    {
        Ok(analysis) if synthetic_facts_present(&analysis.text) => {
            let checked_at = chrono::Utc::now().to_rfc3339();
            if let Ok(mut record) = verification().lock() {
                *record = Some(Verification {
                    fingerprint: fp,
                    checked_at: checked_at.clone(),
                });
            }
            status.state = "ready";
            status.checked_at = Some(checked_at);
        }
        result => {
            let error = match result {
                Ok(_) => "合成事實檢查失敗".to_string(),
                Err(error) => error,
            };
            if let Ok(mut record) = verification().lock() {
                if record
                    .as_ref()
                    .is_some_and(|record| record.fingerprint == fp)
                {
                    *record = None;
                }
            }
            status.state = "action_required";
            status.error_code = Some(if error.contains("合成事實") {
                "incorrect_facts"
            } else if error.contains("引用") {
                "invalid_citation"
            } else if error.contains("未能連接") || error.contains("網絡") {
                "connection_failed"
            } else if error.contains("模型") {
                "model_unavailable"
            } else {
                "probe_failed"
            });
            status.checked_at = None;
        }
    }
    Ok(status)
}

#[tauri::command]
pub async fn apply_tested_mail_settings(
    db: State<'_, Arc<Database>>,
    input: MailSettings,
    expected: MailSettings,
) -> Result<(), String> {
    let credential = credentials(db.inner().clone(), &input.processing_mode).await;
    if !credential.available {
        return Err("probe_required".into());
    }
    let fingerprint = fingerprint(
        &input,
        credential.aws.as_ref(),
        credential.openai.as_deref(),
    );
    apply_verified_settings(&db, &input, &expected, fingerprint)
}

fn apply_verified_settings(
    db: &Database,
    input: &MailSettings,
    expected: &MailSettings,
    fingerprint: u64,
) -> Result<(), String> {
    let verified = verification()
        .lock()
        .map_err(|_| "probe_required".to_string())?;
    if !verified
        .as_ref()
        .is_some_and(|record| record.fingerprint == fingerprint)
    {
        return Err("probe_required".into());
    }
    // The SQL compare and update run under one database lock. A stale wizard
    // cannot replace a provider changed in another window during its probe.
    store::save_settings_if_unchanged(db, input, expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verification_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn tested_settings_apply_only_if_expected_settings_still_match() {
        let _test_lock = verification_test_lock();
        let db = crate::db::test_database();
        let original = store::settings(&db).unwrap();
        let mut candidate = original.clone();
        candidate.processing_mode = "openai".into();
        candidate.openai_model = "gpt-probe".into();
        let fp = fingerprint(&candidate, None, Some("test-key"));
        *verification().lock().unwrap() = None;
        assert_eq!(
            apply_verified_settings(&db, &candidate, &original, fp).unwrap_err(),
            "probe_required"
        );
        assert_eq!(
            store::settings(&db).unwrap().processing_mode,
            original.processing_mode
        );

        *verification().lock().unwrap() = Some(Verification {
            fingerprint: fp,
            checked_at: "time".into(),
        });
        let mut concurrent = original.clone();
        concurrent.processing_mode = "bedrock".into();
        store::save_settings(&db, &concurrent).unwrap();
        assert_eq!(
            apply_verified_settings(&db, &candidate, &original, fp).unwrap_err(),
            "settings_changed"
        );
        assert_eq!(store::settings(&db).unwrap().processing_mode, "bedrock");

        let mut changed_model = concurrent.clone();
        changed_model.local_model = "another-local-model".into();
        store::save_settings(&db, &changed_model).unwrap();
        assert_eq!(
            apply_verified_settings(&db, &candidate, &concurrent, fp).unwrap_err(),
            "settings_changed"
        );

        apply_verified_settings(&db, &candidate, &changed_model, fp).unwrap();
        let applied = store::settings(&db).unwrap();
        assert_eq!(applied.processing_mode, "openai");
        assert_eq!(applied.openai_model, "gpt-probe");
    }

    #[test]
    fn missing_and_locked_credentials_have_distinct_sanitized_codes() {
        assert_eq!(
            credential_error(
                "keychain get: The specified item could not be found in the keychain."
            ),
            "credential_missing"
        );
        assert_eq!(
            credential_error("keychain get: access denied SECRET-DO-NOT-SHOW"),
            "credential_unavailable"
        );
    }

    #[test]
    fn synthetic_probe_accepts_a_complete_json_fence_without_relaxing_facts() {
        let valid = r#"{"deadline":"2026-10-15","currency":"HKD","budget":1000,"paymentRequired":false,"citation":"[郵件 #1]"}"#;
        assert!(synthetic_facts_present(&format!("```json\n{valid}\n```")));
        assert!(synthetic_facts_present(&format!("```\r\n{valid}\r\n```")));
        for invalid in [
            format!("答案：```json\n{valid}\n```"),
            format!("```json\n{valid}\n```另外請付款"),
            format!("```json\n{valid}"),
            format!("```json\n{valid} {valid}\n```"),
            format!("```json\n{}\n```", valid.replace("false", "true")),
            format!("```json\n{}\n```", valid.replace("\"budget\":1000", "\"budget\":1000,\"extra\":1")),
        ] {
            assert!(!synthetic_facts_present(&invalid), "{invalid}");
        }
    }

    #[tokio::test]
    #[ignore = "requires isolated local Ollama and explicit JARVIS_TEST_OLLAMA_MODEL; synthetic data only"]
    async fn real_local_setup_probe_accepts_synthetic_facts() {
        let model = std::env::var("JARVIS_TEST_OLLAMA_MODEL")
            .expect("set JARVIS_TEST_OLLAMA_MODEL explicitly");
        assert!(!model.trim().is_empty());
        let settings = MailSettings {
            processing_mode: "local".into(),
            local_model: model,
            bedrock_model: String::new(),
            openai_model: String::new(),
        };
        let result = analysis::analyze(synthetic_context(settings), None, None)
            .await
            .expect("the production local setup probe must return an answer");
        assert!(synthetic_facts_present(&result.text), "synthetic facts failed: {}", result.text);
    }

    #[test]
    fn synthetic_probe_rejects_missing_date_currency_or_payment_negation() {
        let valid = r#"{"deadline":"2026-10-15","currency":"HKD","budget":1000,"paymentRequired":false,"citation":"[郵件 #1]"}"#;
        assert!(synthetic_facts_present(valid));
        for invalid in [
            valid.replace("2026-10-15", "2026-11-15"),
            valid.replace("\"HKD\"", "\"USD\""),
            valid.replace("\"budget\":1000", "\"budget\":10000"),
            valid.replace("\"paymentRequired\":false", "\"paymentRequired\":true"),
            valid.replace("[郵件 #1]", "[郵件 #2]"),
            format!("並非毋須付款 {valid}"),
            format!("{valid} {valid}"),
        ] {
            assert!(!synthetic_facts_present(&invalid), "{invalid}");
        }
    }

    #[test]
    fn settings_or_credentials_change_invalidates_verification_without_exposing_secrets() {
        let _test_lock = verification_test_lock();
        let settings = MailSettings {
            processing_mode: "openai".into(),
            local_model: "qwen3:8b".into(),
            bedrock_model: "bedrock".into(),
            openai_model: "gpt-a".into(),
        };
        let fp = fingerprint(&settings, None, Some("secret-a"));
        *verification().lock().unwrap() = Some(Verification {
            fingerprint: fp,
            checked_at: "time".into(),
        });
        assert_eq!(capability(&settings, true, fp, None).state, "ready");
        assert_eq!(
            capability(
                &settings,
                true,
                fingerprint(&settings, None, Some("secret-b")),
                None
            )
            .state,
            "saved_unverified"
        );
        let mut edited = settings.clone();
        edited.openai_model = "gpt-b".into();
        assert_eq!(
            capability(
                &edited,
                true,
                fingerprint(&edited, None, Some("secret-a")),
                None
            )
            .state,
            "saved_unverified"
        );
        assert_eq!(
            capability(&settings, false, fp, None).state,
            "action_required"
        );
        assert_eq!(
            capability(&settings, false, fp, Some("credential_unavailable")).error_code,
            Some("credential_unavailable")
        );
        let wire = serde_json::to_string(&capability(&settings, true, fp, None)).unwrap();
        assert!(!wire.contains("secret-a"));
    }
}
