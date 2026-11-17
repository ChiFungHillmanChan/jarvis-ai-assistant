//! A saved model the registry has disabled is never swapped for another on
//! its own (P05). The request that would use it stops before anything is
//! sent: the chain does not fall back to its next entry and no replacement
//! is tried. The user sees a notice naming the replacement and confirms it
//! (`confirm_model_replacement`); until then:
//!
//! - a foreground request (chat, mail analysis, setup) fails with
//!   [`Disabled::message`], and the frontend opens the notice again on
//!   every `model-disabled` event. TODO(P06): show P06's error card instead
//!   of the minimal blocking notice (src/components/ModelDisabledNotice.tsx).
//! - a background job (mail sorting, briefings) does not run: its run
//!   records the same message (`cron_runs.error`, the job's result) and the
//!   notice is raised once per model per launch.
//!
//! Only a model the user saved is checked. An empty choice follows the
//! registry's default, which registry/validate.ts keeps active.

use super::bedrock::UiLanguage;
use super::ProviderChainEntry;
use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// A saved model the registry has disabled.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Disabled {
    /// The registry provider it is used with.
    pub provider: String,
    pub model: String,
    /// What the notice offers; None when the registry names none for this
    /// provider (the user then chooses in Settings).
    pub replacement: Option<String>,
}

impl Disabled {
    /// What a stopped request reports and a background run records, in the
    /// app's language (Settings, as the Bedrock errors are).
    pub(crate) fn message(&self, language: UiLanguage) -> String {
        let model = &self.model;
        match &self.replacement {
            Some(replacement) => language.pick(
                format!("The model {model} has been turned off. Choose a replacement (suggested: {replacement}); JARVIS will not switch to another model until you confirm."),
                format!("模型 {model} 已停用，請選擇替代模型（建議：{replacement}）。未確認前不會改用其他模型。"),
            ),
            None => language.pick(
                format!("The model {model} has been turned off. Choose a replacement in Settings; JARVIS will not switch to another model until you choose one."),
                format!("模型 {model} 已停用，請在設定選擇替代模型。未選擇前不會改用其他模型。"),
            ),
        }
    }
}

/// Whether `model`, saved for the registry provider `provider`, may be sent.
pub fn check(provider: &str, model: &str) -> Result<(), Disabled> {
    let model = model.trim();
    if model.is_empty() {
        return Ok(());
    }
    match super::registry::current().model_state(provider, model) {
        super::registry::ModelState::Disabled { replacement } => Err(Disabled { provider: provider.into(), model: model.into(), replacement }),
        _ => Ok(()),
    }
}

/// [`check`] for a chain row: its saved model, if it has one.
pub fn check_entry(entry: &ProviderChainEntry) -> Result<(), Disabled> {
    match (super::mail_policy::registry_provider(entry), entry.model_id.as_deref()) {
        (Some(provider), Some(model)) => check(provider, model),
        _ => Ok(()),
    }
}

/// Whether a request for `feature` is one the user is waiting on.
pub fn is_foreground(feature: super::cost::Feature) -> bool {
    use super::cost::Feature;
    matches!(feature, Feature::Chat | Feature::Tools | Feature::MailAnalysis | Feature::Other)
}

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Called once at setup, so a request deep in a job can raise the notice.
pub fn init(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

/// Background notices already raised this launch, by provider and model.
fn raised() -> &'static Mutex<HashSet<(String, String)>> {
    static RAISED: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
    RAISED.get_or_init(Default::default)
}

/// Whether to raise the notice for this stop: every foreground request,
/// and a background one only the first time for its model this launch.
fn should_raise(disabled: &Disabled, foreground: bool) -> bool {
    let first = raised().lock().map(|mut seen| seen.insert((disabled.provider.clone(), disabled.model.clone()))).unwrap_or(true);
    foreground || first
}

/// Stop the request: log it (the model id only), raise the notice, and
/// return the message the request fails with, in `language`.
pub(crate) fn stop(disabled: Disabled, foreground: bool, language: UiLanguage) -> String {
    log::warn!(
        "registry: {} model {} is disabled; request not sent ({})",
        disabled.provider,
        disabled.model,
        if foreground { "foreground" } else { "background" }
    );
    if should_raise(&disabled, foreground) {
        if let Some(app) = APP.get() {
            use tauri::Emitter;
            let _ = app.emit("model-disabled", &disabled);
        }
    }
    disabled.message(language)
}

// ---------------------------------------------------------------------------
// The saved choices, for the notice.

/// Where a disabled model is saved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "place", rename_all = "snake_case")]
pub enum Place {
    /// A provider_chain row, by position.
    Chain { position: i32 },
    /// The mail_settings column the current processing mode reads.
    Mail { field: MailField },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MailField {
    LocalModel,
    OpenaiModel,
    BedrockModel,
}

impl MailField {
    fn column(self) -> &'static str {
        match self {
            MailField::LocalModel => "local_model",
            MailField::OpenaiModel => "openai_model",
            MailField::BedrockModel => "bedrock_model",
        }
    }

    fn provider(self) -> &'static str {
        match self {
            MailField::LocalModel => "ollama",
            MailField::OpenaiModel => "openai",
            MailField::BedrockModel => "bedrock",
        }
    }

    /// The column mail analysis reads in this processing mode.
    fn for_mode(mode: &str) -> Option<MailField> {
        match mode.trim().to_ascii_lowercase().as_str() {
            "local" => Some(MailField::LocalModel),
            "openai" => Some(MailField::OpenaiModel),
            "bedrock" => Some(MailField::BedrockModel),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DisabledChoice {
    #[serde(flatten)]
    pub place: Place,
    #[serde(flatten)]
    pub disabled: Disabled,
    /// The registry's name for the replacement, when it has one.
    pub replacement_label: Option<String>,
}

fn choice(place: Place, disabled: Disabled) -> DisabledChoice {
    let replacement_label = disabled.replacement.as_deref().and_then(super::models::label_for);
    DisabledChoice { place, disabled, replacement_label }
}

/// Every saved model the registry in use has disabled.
pub fn saved(db: &Database) -> Result<Vec<DisabledChoice>, String> {
    let mut found = Vec::new();
    for entry in crate::commands::ai_choice::saved_chain(db)?.iter().filter(|e| e.enabled) {
        if let Err(disabled) = check_entry(entry) {
            found.push(choice(Place::Chain { position: entry.position }, disabled));
        }
    }
    let settings = crate::mail::store::settings(db)?;
    if let Some(field) = MailField::for_mode(&settings.processing_mode) {
        let model = match field {
            MailField::LocalModel => &settings.local_model,
            MailField::OpenaiModel => &settings.openai_model,
            MailField::BedrockModel => &settings.bedrock_model,
        };
        if let Err(disabled) = check(field.provider(), model) {
            found.push(choice(Place::Mail { field }, disabled));
        }
    }
    Ok(found)
}

/// Save the replacement the notice offered for `model` at `place`. Refused
/// when the saved model is no longer `model`, or the registry in use no
/// longer disables it or names no replacement: the notice is then stale.
pub fn confirm(db: &Database, place: &Place, model: &str) -> Result<String, String> {
    let stale = || "這項設定已更改，請重新開啟提示。".to_string();
    let current = saved(db)?.into_iter().find(|c| c.place == *place && c.disabled.model == model.trim()).ok_or_else(stale)?;
    let replacement = current.disabled.replacement.ok_or("沒有建議的替代模型，請在設定選擇。")?;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let changed = match place {
        Place::Chain { position } => conn.execute(
            "UPDATE provider_chain SET model_id = ?1 WHERE position = ?2 AND model_id = ?3",
            rusqlite::params![replacement, position, current.disabled.model],
        ),
        Place::Mail { field } => conn.execute(
            &format!("UPDATE mail_settings SET {0} = ?1 WHERE id = 1 AND {0} = ?2", field.column()),
            rusqlite::params![replacement, current.disabled.model],
        ),
    }
    .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(stale());
    }
    log::info!("registry: disabled model {} replaced by {} after the user confirmed", current.disabled.model, replacement);
    Ok(replacement)
}

#[tauri::command]
pub fn disabled_models(db: tauri::State<'_, std::sync::Arc<Database>>) -> Result<Vec<DisabledChoice>, String> {
    saved(&db)
}

#[tauri::command]
pub fn confirm_model_replacement(
    db: tauri::State<'_, std::sync::Arc<Database>>,
    router: tauri::State<'_, std::sync::Mutex<super::AiRouter>>,
    place: Place,
    model: String,
) -> Result<String, String> {
    let replacement = confirm(&db, &place, &model)?;
    if matches!(place, Place::Chain { .. }) {
        super::install_live(&router, crate::commands::ai_choice::rebuilt_router(&db))?;
    }
    Ok(replacement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::registry::test_override;

    /// The test registry with `id` disabled and `replacement` named.
    fn disabling(id: &'static str, replacement: Option<&'static str>) -> test_override::Guard {
        test_override::with(move |r| {
            let entry = r.models.iter_mut().find(|m| m.id == id).unwrap();
            entry.status = crate::ai::registry::Status::Disabled;
            entry.replacement = replacement.map(str::to_string);
        })
    }

    fn row(provider_type: &str, model: Option<&str>, base_url: Option<&str>) -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id: None, model_id: model.map(str::to_string), enabled: true, base_url: base_url.map(str::to_string) }
    }

    #[test]
    fn only_a_saved_model_the_registry_disables_is_stopped() {
        assert_eq!(check_entry(&row("openai", Some("gpt-5.6-luna"), None)), Ok(()));
        let _registry = disabling("gpt-5.6-luna", Some("gpt-6-luna"));
        assert_eq!(
            check_entry(&row("openai", Some("gpt-5.6-luna"), None)),
            Err(Disabled { provider: "openai".into(), model: "gpt-5.6-luna".into(), replacement: Some("gpt-6-luna".into()) })
        );
        assert_eq!(check_entry(&row("openai", None, None)), Ok(()), "no saved model follows the default");
        assert_eq!(check_entry(&row("openai", Some("gpt-7-unlisted"), None)), Ok(()), "a model the registry does not list is the user's own pick");
        // A compatible service's id matches only that service's own entries.
        assert_eq!(check_entry(&row("openai", Some("gpt-5.6-luna"), Some("https://api.deepseek.com/v1"))), Ok(()));
    }

    #[test]
    fn the_message_names_the_replacement_and_never_switches_in_the_apps_language() {
        let with = Disabled { provider: "openai".into(), model: "gpt-5.6-luna".into(), replacement: Some("gpt-6-luna".into()) };
        let zh = with.message(UiLanguage::ZhHk);
        assert!(zh.contains("gpt-5.6-luna") && zh.contains("gpt-6-luna") && zh.contains("請選擇替代模型"), "{zh}");
        let en = with.message(UiLanguage::En);
        assert!(en.contains("gpt-5.6-luna") && en.contains("gpt-6-luna") && en.contains("Choose a replacement") && en.contains("until you confirm"), "{en}");
        assert!(!en.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)), "the English message has no Chinese: {en}");
        let without = Disabled { replacement: None, ..with };
        assert!(without.message(UiLanguage::ZhHk).contains("請在設定選擇替代模型"));
        assert!(without.message(UiLanguage::En).contains("Choose a replacement in Settings"));
        // The language is the one Settings saved.
        let db = crate::db::test_database();
        let stopped = || stop(without.clone(), true, UiLanguage::read(&db));
        assert!(stopped().starts_with("The model "));
        db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('ui_locale', 'zh-HK')", []).unwrap();
        assert!(stopped().starts_with("模型 "));
    }

    #[test]
    fn a_background_notice_is_raised_once_per_model_and_a_foreground_one_every_time() {
        let disabled = Disabled { provider: "test".into(), model: format!("m-{}", uuid::Uuid::new_v4()), replacement: None };
        assert!(should_raise(&disabled, false));
        assert!(!should_raise(&disabled, false), "a second background stop raises nothing");
        assert!(should_raise(&disabled, true), "a foreground request always opens the notice");
        assert!(is_foreground(crate::ai::cost::Feature::Chat) && !is_foreground(crate::ai::cost::Feature::MailSort) && !is_foreground(crate::ai::cost::Feature::Briefing));
    }

    #[test]
    fn the_notice_lists_saved_choices_and_confirm_saves_only_the_offered_replacement() {
        let db = crate::db::test_database();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(
                "DELETE FROM provider_chain;
                 INSERT INTO provider_chain (position, provider_type, model_id, enabled) VALUES (0, 'openai', 'gpt-5.6-luna', 1), (1, 'claude', NULL, 1);
                 UPDATE mail_settings SET processing_mode = 'openai', openai_model = 'gpt-5.6-luna' WHERE id = 1;",
            )
            .unwrap();
        assert_eq!(saved(&db).unwrap(), vec![], "nothing is disabled in the test registry");
        let _registry = disabling("gpt-5.6-luna", Some("gpt-6-luna"));
        let found = saved(&db).unwrap();
        assert_eq!(found.iter().map(|c| c.place.clone()).collect::<Vec<_>>(), vec![Place::Chain { position: 0 }, Place::Mail { field: MailField::OpenaiModel }]);
        assert_eq!(found[0].replacement_label.as_deref(), Some("GPT-6 Luna"));
        let wire = serde_json::to_value(&found[1]).unwrap();
        assert_eq!((wire["place"].as_str(), wire["field"].as_str(), wire["model"].as_str()), (Some("mail"), Some("openai_model"), Some("gpt-5.6-luna")));

        // Nothing changed until confirmed; a stale confirm changes nothing.
        assert_eq!(crate::commands::ai_choice::saved_chain(&db).unwrap()[0].model_id.as_deref(), Some("gpt-5.6-luna"));
        assert!(confirm(&db, &Place::Chain { position: 0 }, "gpt-other").is_err());
        assert!(confirm(&db, &Place::Chain { position: 1 }, "gpt-5.6-luna").is_err());
        assert_eq!(confirm(&db, &Place::Chain { position: 0 }, "gpt-5.6-luna").unwrap(), "gpt-6-luna");
        assert_eq!(confirm(&db, &Place::Mail { field: MailField::OpenaiModel }, "gpt-5.6-luna").unwrap(), "gpt-6-luna");
        assert_eq!(crate::commands::ai_choice::saved_chain(&db).unwrap()[0].model_id.as_deref(), Some("gpt-6-luna"));
        assert_eq!(crate::mail::store::settings(&db).unwrap().openai_model, "gpt-6-luna");
        assert_eq!(saved(&db).unwrap(), vec![]);
    }

    #[test]
    fn a_disabled_model_without_a_replacement_cannot_be_confirmed() {
        let db = crate::db::test_database();
        db.conn.lock().unwrap().execute_batch("UPDATE mail_settings SET processing_mode = 'local', local_model = 'qwen3:8b' WHERE id = 1;").unwrap();
        let _registry = disabling("qwen3:8b", None);
        let found = saved(&db).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].disabled.replacement, None, "Ollama has no default to offer");
        assert!(confirm(&db, &Place::Mail { field: MailField::LocalModel }, "qwen3:8b").is_err());
        assert_eq!(crate::mail::store::settings(&db).unwrap().local_model, "qwen3:8b");
    }
}
