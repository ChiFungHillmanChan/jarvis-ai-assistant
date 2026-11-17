//! The user's consent to send email and calendar content to a provider
//! whose data terms need it (P06, DECISIONS.md section 5 "Data-policy gate").
//!
//! The registry classifies each provider (`providers[].data_terms`). `safe`
//! providers just work. `needs_billing_confirmation` (Gemini: its free tier
//! may use what it is sent to improve Google's models) waits for one click
//! on the consent card. `unknown` (JARVIS cannot confirm how the provider
//! uses data: compatible services, Ollama Cloud, an Ollama on another
//! machine, a provider new in a fetched registry) never receives Gmail and
//! Calendar content, consent or not (DECISIONS, P06 "Google Limited Use",
//! 2026-11-17; `ai::mail_policy` refuses it before asking here). A voice
//! recording is not Google content: the speech gate (`voice`) still asks
//! for consent under any terms. A consent is stored per provider and
//! address, with the terms and the registry version it was given under
//! (V36), and holds while the terms in force are not stricter than those.
//! It is revoked in Settings. `ai::mail_policy` and the speech gate ask
//! [`holds`].

use super::registry::{self, DataTerms};
use crate::db::Database;
use serde::{Deserialize, Serialize};

/// The copy keys of the card for a provider the registry gives none (an
/// Ollama on another machine is `safe` as a provider).
const UNKNOWN_TERMS_KEYS: (&str, &str) = ("consentUnknownTermsBlocked", "consentUnknownTermsConfirm");

/// Who a consent is for: the registry provider id and, for a service at an
/// address the user entered, that address ("" for a provider at its own).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subject {
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
}

/// What the consent card shows, and what its button sends back.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    pub provider: String,
    pub base_url: String,
    /// "Google Gemini", "Kimi", "llm.example.com".
    pub label: String,
    pub terms: DataTerms,
    /// src/i18n/catalog.ts keys (registry `consent_copy_keys`).
    pub blocked_key: String,
    pub confirm_key: String,
}

impl Request {
    pub(crate) fn new(subject: Subject, label: String, terms: DataTerms) -> Request {
        // Billing has a card of its own per provider (Gemini's names its
        // billing page); "JARVIS cannot vouch" is one card for every
        // provider, whatever keys a provider tightened to unknown carries.
        let keys = match terms {
            DataTerms::NeedsBillingConfirmation => registry::current().provider(&subject.provider).and_then(|p| p.consent_copy_keys.clone()),
            _ => None,
        };
        let (blocked_key, confirm_key) = match keys {
            Some(keys) => (keys.blocked, keys.confirm),
            None => (UNKNOWN_TERMS_KEYS.0.to_string(), UNKNOWN_TERMS_KEYS.1.to_string()),
        };
        Request { provider: subject.provider, base_url: subject.base_url, label, terms, blocked_key, confirm_key }
    }

    pub fn subject(&self) -> Subject {
        Subject { provider: self.provider.clone(), base_url: self.base_url.clone() }
    }

    /// The error a page the user is on fails with: the consent card's data
    /// (src/lib/ai-error.ts).
    pub fn to_error(&self) -> String {
        serde_json::json!({ "jarvis_ai": { "kind": "consent", "consent": self } }).to_string()
    }

    /// Show the card: inline in the chat run in progress, or, for a job in
    /// the background (the briefing, mail sorting), once per launch as a
    /// notice. `feature` is the registry feature that was held back.
    pub fn raise(&self, feature: &str) {
        let payload = serde_json::json!({ "consent": self, "feature": feature });
        if crate::chat_session::CONTEXT.try_with(|_| ()).is_ok() {
            super::notify::emit("chat-consent", payload);
        } else if super::notify::first_time(&format!("consent|{}|{}|{feature}", self.provider, self.base_url)) {
            log::info!("[consent] {feature} held back until the user agrees to {}'s terms", self.provider);
            super::notify::emit("ai-consent-needed", payload);
        }
    }
}

fn terms_name(terms: DataTerms) -> &'static str {
    match terms {
        DataTerms::Safe => "safe",
        DataTerms::NeedsBillingConfirmation => "needs_billing_confirmation",
        DataTerms::Unknown => "unknown",
    }
}

fn parse_terms(name: &str) -> Option<DataTerms> {
    match name {
        "safe" => Some(DataTerms::Safe),
        "needs_billing_confirmation" => Some(DataTerms::NeedsBillingConfirmation),
        "unknown" => Some(DataTerms::Unknown),
        _ => None,
    }
}

/// The terms a consent was given under, if one was.
fn given(db: &Database, subject: &Subject) -> Option<DataTerms> {
    let conn = db.conn.lock().ok()?;
    let name: String = conn
        .query_row("SELECT data_terms FROM ai_consent WHERE provider = ?1 AND base_url = ?2", [&subject.provider, &subject.base_url], |row| row.get(0))
        .ok()?;
    parse_terms(&name)
}

/// Whether email may go to `subject` under the terms now in force: always
/// for `safe`; otherwise only with a consent given under terms at least as
/// strict. A registry that tightens a provider (billing to unknown) asks
/// again; a database that cannot be read fails closed.
pub fn holds(db: &Database, subject: &Subject, terms_now: DataTerms) -> bool {
    terms_now == DataTerms::Safe || given(db, subject).is_some_and(|given| given >= terms_now)
}

/// Record the user's consent under the terms now in force.
pub fn give(db: &Database, subject: &Subject, terms_now: DataTerms) -> Result<(), String> {
    if terms_now == DataTerms::Safe {
        return Ok(());
    }
    let version = registry::current().version as i64;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO ai_consent (provider, base_url, data_terms, registry_version, consented_at) VALUES (?1, ?2, ?3, ?4, datetime('now')) \
         ON CONFLICT(provider, base_url) DO UPDATE SET data_terms = ?3, registry_version = ?4, consented_at = datetime('now')",
        rusqlite::params![subject.provider, subject.base_url, terms_name(terms_now), version],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Withdraw a consent. True when there was one.
///
/// Withdrawing Gemini also clears the billing box that builds from before
/// V36 read (`gemini_billing_confirmed`, which V36 copied and left in
/// place), so going back to such a build does not agree again for the user.
pub fn revoke(db: &Database, subject: &Subject) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    if subject.provider == "gemini" {
        conn.execute("DELETE FROM user_preferences WHERE key = 'gemini_billing_confirmed'", []).map_err(|e| e.to_string())?;
    }
    conn.execute("DELETE FROM ai_consent WHERE provider = ?1 AND base_url = ?2", [&subject.provider, &subject.base_url]).map(|n| n > 0).map_err(|e| e.to_string())
}

/// A consent as Settings lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Given {
    pub provider: String,
    pub base_url: String,
    pub label: String,
    /// The terms it was given under, and those in force now.
    pub terms: DataTerms,
    pub terms_now: DataTerms,
    pub registry_version: i64,
    pub consented_at: String,
    /// False once the registry has tightened the terms: asked again.
    pub holds: bool,
}

/// The consents that let something through now, for Settings. One that
/// has no effect is not listed: a consent for terms that are safe now, and
/// one under `unknown` terms for a service no voice recording goes to (an
/// `unknown` service never receives Gmail and Calendar content).
pub fn list(db: &Database) -> Result<Vec<Given>, String> {
    let rows: Vec<(String, String, String, i64, String)> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT provider, base_url, data_terms, registry_version, consented_at FROM ai_consent ORDER BY consented_at DESC, provider").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    Ok(rows
        .into_iter()
        .filter_map(|(provider, base_url, terms, registry_version, consented_at)| {
            let terms = parse_terms(&terms)?;
            let subject = Subject { provider, base_url };
            let (label, terms_now) = super::mail_policy::describe(&subject);
            let has_effect = match terms_now {
                DataTerms::Safe => false,
                DataTerms::NeedsBillingConfirmation => true,
                DataTerms::Unknown => crate::voice::is_speech_provider(&subject.provider),
            };
            has_effect.then(|| Given { holds: terms >= terms_now, provider: subject.provider, base_url: subject.base_url, label, terms, terms_now, registry_version, consented_at })
        })
        .collect())
}

/// The consent card the AI in use needs before email can go to it, if any.
#[tauri::command]
pub fn ai_consent_needed(db: tauri::State<'_, std::sync::Arc<Database>>) -> Option<Request> {
    match super::mail_policy::check(&db) {
        Err(super::mail_policy::Blocked::NeedsConsent(request)) => Some(*request),
        _ => None,
    }
}

/// The plain line setup and Settings show when Gmail and Calendar content
/// is not sent to the AI in use at all (P06, Google Limited Use): there is
/// nothing to agree to, so no button.
#[tauri::command]
pub fn ai_google_content_note(db: tauri::State<'_, std::sync::Arc<Database>>) -> Option<String> {
    google_content_note(&db)
}

pub(crate) fn google_content_note(db: &Database) -> Option<String> {
    use super::mail_policy::Blocked;
    let label = match super::mail_policy::check(db) {
        Err(Blocked::NotForGoogleContent { label, .. } | Blocked::NotAllowed(label)) => label,
        _ => return None,
    };
    Some(super::bedrock::UiLanguage::read(db).pick(
        format!("Gmail and Calendar content is not sent to {label}: JARVIS cannot confirm how this service uses data. Chat and voice requests work with {label}; mail features, reading your calendar and the daily briefing need another AI."),
        format!("Gmail 及日曆內容不會傳送至 {label}：JARVIS 無法確認這個服務如何使用資料。你仍可用 {label} 對話及發出語音指令；郵件功能、讀取日曆及每日簡報則須改用其他 AI。"),
    ))
}

/// The consent card's button. The terms are the ones in force now, read
/// here rather than taken from the page.
#[tauri::command]
pub fn give_ai_consent(db: tauri::State<'_, std::sync::Arc<Database>>, provider: String, base_url: Option<String>) -> Result<(), String> {
    let subject = super::mail_policy::subject_for(&provider, base_url.as_deref().unwrap_or_default())?;
    let (_, terms_now) = super::mail_policy::describe(&subject);
    give(&db, &subject, terms_now)?;
    super::notify::emit("ai-consent-changed", ());
    Ok(())
}

#[tauri::command]
pub fn revoke_ai_consent(db: tauri::State<'_, std::sync::Arc<Database>>, provider: String, base_url: Option<String>) -> Result<(), String> {
    revoke(&db, &Subject { provider, base_url: base_url.unwrap_or_default() })?;
    super::notify::emit("ai-consent-changed", ());
    Ok(())
}

#[tauri::command]
pub fn list_ai_consents(db: tauri::State<'_, std::sync::Arc<Database>>) -> Result<Vec<Given>, String> {
    list(&db)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(provider: &str, base_url: &str) -> Subject {
        Subject { provider: provider.into(), base_url: base_url.into() }
    }

    /// Each level of terms, before consent, after it and after revocation;
    /// a consent covers its own provider and address only.
    #[test]
    fn a_consent_holds_for_its_terms_until_revoked() {
        let db = crate::db::test_database();
        let gemini = subject("gemini", "");
        let kimi = subject("kimi", "https://api.moonshot.ai/v1");
        assert!(holds(&db, &gemini, DataTerms::Safe), "safe needs no consent");
        assert!(!holds(&db, &gemini, DataTerms::NeedsBillingConfirmation));
        assert!(!holds(&db, &kimi, DataTerms::Unknown));

        give(&db, &gemini, DataTerms::NeedsBillingConfirmation).unwrap();
        give(&db, &kimi, DataTerms::Unknown).unwrap();
        assert!(holds(&db, &gemini, DataTerms::NeedsBillingConfirmation));
        assert!(holds(&db, &kimi, DataTerms::Unknown));
        assert!(!holds(&db, &subject("kimi", "https://api.moonshot.cn/v1"), DataTerms::Unknown), "another address is asked about on its own");

        // Tightened from billing to unknown: asked again.
        assert!(!holds(&db, &gemini, DataTerms::Unknown));

        assert!(revoke(&db, &kimi).unwrap());
        assert!(!holds(&db, &kimi, DataTerms::Unknown));
        assert!(!revoke(&db, &kimi).unwrap());

        let listed = list(&db).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].provider.as_str(), listed[0].terms, listed[0].holds), ("gemini", DataTerms::NeedsBillingConfirmation, true));
        assert_eq!(listed[0].registry_version, registry::current().version as i64);
    }

    /// A consent row for a service JARVIS cannot confirm (one an earlier
    /// build stored for Kimi) unlocks nothing: no Gmail or Calendar content
    /// goes to Kimi, and Settings does not list it. One for Gemini, which
    /// voice recordings go to, still lets a recording through when a
    /// registry copy has tightened Gemini to unknown, and stays listed so it
    /// can be withdrawn; Gmail and Calendar content stays back.
    #[test]
    fn an_unknown_terms_consent_unlocks_no_google_content_and_is_listed_only_when_voice_uses_it() {
        let db = crate::db::test_database();
        crate::ai::mail_policy::test_support::use_ai(&db, "openai", crate::ai::mail_policy::test_support::KIMI);
        crate::ai::mail_policy::test_support::agree_to_kimi(&db);
        assert!(matches!(crate::ai::mail_policy::check(&db), Err(crate::ai::mail_policy::Blocked::NotForGoogleContent { .. })));
        assert!(list(&db).unwrap().is_empty(), "a consent with no effect is not listed");

        let _registry = registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "gemini").unwrap().data_terms = DataTerms::Unknown);
        crate::ai::mail_policy::test_support::use_ai(&db, "gemini", None);
        let gemini = subject("gemini", "");
        give(&db, &gemini, DataTerms::Unknown).unwrap();
        assert!(holds(&db, &gemini, DataTerms::Unknown), "the speech gate's consent");
        assert!(matches!(crate::ai::mail_policy::check(&db), Err(crate::ai::mail_policy::Blocked::NotForGoogleContent { .. })));
        let listed: Vec<String> = list(&db).unwrap().into_iter().map(|g| g.provider).collect();
        assert_eq!(listed, ["gemini"]);
    }

    /// Setup and Settings say plainly, with no button, when the AI in use
    /// receives no Gmail and Calendar content; nothing for one that does or
    /// one that waits for consent (its card is shown instead).
    #[test]
    fn setup_says_plainly_when_google_content_is_not_sent() {
        let db = crate::db::test_database();
        crate::ai::mail_policy::test_support::use_ai(&db, "openai", crate::ai::mail_policy::test_support::KIMI);
        let note = google_content_note(&db).unwrap();
        assert!(note.starts_with("Gmail and Calendar content is not sent to Kimi: JARVIS cannot confirm how this service uses data."), "{note}");
        assert!(note.contains("mail features, reading your calendar and the daily briefing need another AI"), "{note}");
        crate::ai::mail_policy::test_support::use_ai(&db, "gemini", None);
        assert_eq!(google_content_note(&db), None, "Gemini has its consent card");
        crate::ai::mail_policy::test_support::use_ai(&db, "openai", None);
        assert_eq!(google_content_note(&db), None);
    }

    /// The card's keys are the registry's for the provider; an Ollama on
    /// another machine (a safe provider) gets the unknown-terms card.
    #[test]
    fn the_card_uses_the_registrys_copy_keys() {
        let gemini = Request::new(subject("gemini", ""), "Google Gemini".into(), DataTerms::NeedsBillingConfirmation);
        assert_eq!((gemini.blocked_key.as_str(), gemini.confirm_key.as_str()), ("consentGeminiBillingBlocked", "consentGeminiBillingConfirm"));
        let remote = Request::new(subject("ollama", "https://llm.example.com"), "llm.example.com".into(), DataTerms::Unknown);
        assert_eq!((remote.blocked_key.as_str(), remote.confirm_key.as_str()), UNKNOWN_TERMS_KEYS);
        let value: serde_json::Value = serde_json::from_str(&gemini.to_error()).unwrap();
        assert_eq!(value["jarvis_ai"]["kind"], "consent");
        assert_eq!(value["jarvis_ai"]["consent"]["terms"], "needs_billing_confirmation");
    }

    /// The shipped V36: a Gemini billing confirmation given before it is
    /// carried over as that consent; an unchecked one is not.
    #[test]
    fn withdrawing_gemini_also_clears_the_old_billing_box() {
        let db = crate::db::test_database();
        let legacy = |db: &Database| db.conn.lock().unwrap().query_row("SELECT value FROM user_preferences WHERE key = 'gemini_billing_confirmed'", [], |r| r.get::<_, String>(0)).ok();
        db.conn.lock().unwrap().execute("INSERT INTO user_preferences (key, value) VALUES ('gemini_billing_confirmed', 'true')", []).unwrap();
        let gemini = Subject { provider: "gemini".into(), base_url: String::new() };
        give(&db, &gemini, DataTerms::NeedsBillingConfirmation).unwrap();
        // Withdrawing another service's consent leaves it.
        revoke(&db, &Subject { provider: "kimi".into(), base_url: "https://api.moonshot.ai/v1".into() }).unwrap();
        assert_eq!(legacy(&db).as_deref(), Some("true"));
        // A build from before V36 reads only this box: withdrawing Gemini in
        // this build must not leave it ticked for that build (review B, m6).
        assert!(revoke(&db, &gemini).unwrap());
        assert_eq!(legacy(&db), None);
        assert!(!holds(&db, &gemini, DataTerms::NeedsBillingConfirmation));
    }

    #[test]
    fn v36_carries_over_a_gemini_billing_confirmation() {
        const V36: &str = include_str!("../../migrations/V36__ai_consent.sql");
        for (value, carried) in [("true", true), ("false", false)] {
            let conn = rusqlite::Connection::open_in_memory().unwrap();
            conn.execute_batch("CREATE TABLE user_preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT (datetime('now')));").unwrap();
            conn.execute("INSERT INTO user_preferences (key, value) VALUES ('gemini_billing_confirmed', ?1)", [value]).unwrap();
            conn.execute_batch(V36).unwrap();
            let rows: Vec<(String, String, String, i64)> = conn
                .prepare("SELECT provider, base_url, data_terms, registry_version FROM ai_consent")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let expected = if carried { vec![("gemini".to_string(), String::new(), "needs_billing_confirmation".to_string(), 0)] } else { vec![] };
            assert_eq!(rows, expected, "{value}");
        }
    }
}
