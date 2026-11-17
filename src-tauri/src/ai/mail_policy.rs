//! Which AI may receive email and calendar content.
//!
//! Google's Limited Use rules for Gmail data forbid transferring it to be used
//! to train AI models. OpenAI, Anthropic and Amazon Bedrock do not train on API
//! traffic by default, and a model on this Mac keeps email local. The
//! registry classifies every provider's data terms (`providers[].data_terms`):
//! a `safe` provider receives email; Gemini (`needs_billing_confirmation`:
//! its unpaid tier may use submitted content to improve Google's models,
//! outside the EEA, Switzerland and the UK) and every provider JARVIS cannot
//! vouch for (`unknown`: Kimi, Qwen and other compatible services, Ollama
//! Cloud, an Ollama on another machine) receive it once the user agrees on
//! the consent card (ai/consent.rs, P06). Every path that sends email text to
//! the chosen AI (sorting, analysis, briefings, chat context and tools) asks
//! `check` first. Google Calendar data falls under the same Google rules, so
//! event titles and places in the chat context and `list_events` ask it too.
//!
//! TypeSafe, which classifies mail when the user adds its key, is not the
//! chosen AI and is not checked here: its privacy policy says it does not
//! train or fine-tune models on prompts or other input.

use super::compat;
use super::consent::{self, Subject};
use super::registry::DataTerms;
use super::ProviderChainEntry;
use crate::db::Database;

#[derive(Debug, PartialEq)]
pub enum Blocked {
    /// The provider's terms need the user's consent, not given yet (or
    /// given under milder terms than those now in force).
    NeedsConsent(consent::Request),
    /// A row type this build does not know: there is nothing to agree to.
    NotAllowed(String),
}

impl Blocked {
    pub fn code(&self) -> &'static str {
        match self {
            Blocked::NeedsConsent(_) => "consent_needed",
            Blocked::NotAllowed(_) => "mail_ai_not_allowed",
        }
    }

    /// For the model, which explains it to the user in their language. The
    /// consent card is shown in the chat at the same time ([`Blocked::raise`]).
    pub fn tool_message(&self) -> String {
        match self {
            Blocked::NeedsConsent(request) if request.terms == DataTerms::NeedsBillingConfirmation => format!(
                "Email and calendar content is not shared with {} yet: its free tier may use submitted content to \
                 improve its models. JARVIS has shown the user a consent card in this chat; once they confirm \
                 there that their API key has billing enabled, this works. Do not ask them to change settings.",
                request.label
            ),
            Blocked::NeedsConsent(request) => format!(
                "Email and calendar content is not shared with {} yet: JARVIS cannot vouch for how this service \
                 uses the data it receives. JARVIS has shown the user a consent card in this chat; once they agree \
                 there, this works. Do not ask them to change settings.",
                request.label
            ),
            Blocked::NotAllowed(label) => format!("Email and calendar content is not shared with {label}, an AI service this version of JARVIS does not know."),
        }
    }

    /// The error a page the user is on fails with: the consent card's data,
    /// or a sentence when there is nothing to agree to.
    pub fn user_message(&self) -> String {
        match self {
            Blocked::NeedsConsent(request) => request.to_error(),
            Blocked::NotAllowed(label) => format!("電郵內容不會傳送至 {label}：這個版本的 JARVIS 不認識這個 AI 服務。"),
        }
    }

    /// Show the consent card for `feature` (see [`consent::Request::raise`]).
    pub fn raise(&self, feature: &str) {
        if let Blocked::NeedsConsent(request) = self {
            request.raise(feature);
        }
    }
}

/// Whether email content may go to the AI this request uses: a model
/// picked in the error card for this one request, else the first enabled
/// row of the saved list (the only one a request is sent to).
pub fn check(db: &Database) -> Result<(), Blocked> {
    if let Some(pick) = super::capability::picked() {
        return allowed(db, &pick.entry());
    }
    let chosen = crate::commands::ai_choice::saved_chain(db).unwrap_or_default().into_iter().find(|entry| entry.enabled);
    // No saved choice: an install from before setup asked, which chats with
    // Gemini by default.
    let chosen = chosen.unwrap_or(ProviderChainEntry { position: 0, provider_type: "gemini".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None });
    allowed(db, &chosen)
}

/// The registry's data terms decide (`providers[].data_terms`): safe
/// receives email, anything else once the user agreed under those terms.
/// The bundled registry is the floor: a fetched copy can tighten a
/// provider, never loosen one (registry::load::apply_floor).
fn allowed(db: &Database, entry: &ProviderChainEntry) -> Result<(), Blocked> {
    let Some(provider) = registry_provider(entry) else {
        return Err(Blocked::NotAllowed(entry.provider_type.clone()));
    };
    let base_url = match (provider, entry.base_url.as_deref()) {
        // A "local" endpoint pointing at another machine is a remote
        // service like any other, named by its address.
        ("ollama", _) => match endpoint_off_this_mac(db, entry.endpoint_id.as_deref()) {
            Ok(None) => return Ok(()),
            Ok(Some(url)) => url,
            Err(label) => return Err(Blocked::NotAllowed(label)),
        },
        ("openai", _) | (_, None) => String::new(),
        (_, Some(base)) => base.to_string(),
    };
    let subject = Subject { provider: provider.to_string(), base_url };
    let (label, terms) = describe(&subject);
    if consent::holds(db, &subject, terms) {
        return Ok(());
    }
    Err(Blocked::NeedsConsent(consent::Request::new(subject, label, terms)))
}

/// The name the card gives a subject, and the terms in force for it. An
/// Ollama on another machine is not vouched for; a provider this registry
/// does not list is unknown.
pub(crate) fn describe(subject: &Subject) -> (String, DataTerms) {
    let registry = super::registry::current();
    let host = || compat::host(&subject.base_url);
    match (subject.provider.as_str(), registry.provider(&subject.provider)) {
        ("ollama", Some(p)) if !subject.base_url.is_empty() => (host(), p.data_terms.max(DataTerms::Unknown)),
        ("openai_compat", Some(p)) if !subject.base_url.is_empty() => (compat::label(&subject.base_url), p.data_terms),
        (_, Some(p)) => (p.display_name.en.clone(), p.data_terms),
        (id, None) => (id.to_string(), DataTerms::Unknown),
    }
}

/// A subject sent back by the consent card: a provider this registry lists,
/// and an address or none.
pub(crate) fn subject_for(provider: &str, base_url: &str) -> Result<Subject, String> {
    let invalid = || "Unknown AI service.".to_string();
    if super::registry::current().provider(provider).is_none() {
        return Err(invalid());
    }
    let base_url = base_url.trim();
    if base_url.len() > 2_048 || (!base_url.is_empty() && reqwest::Url::parse(base_url).is_err()) {
        return Err(invalid());
    }
    Ok(Subject { provider: provider.to_string(), base_url: base_url.to_string() })
}

/// The registry provider a chain row sends to, or None for a row type this
/// build does not know.
pub(crate) fn registry_provider(entry: &ProviderChainEntry) -> Option<&'static str> {
    match (entry.provider_type.as_str(), entry.base_url.as_deref()) {
        ("claude", _) => Some("claude"),
        ("bedrock", _) => Some("bedrock"),
        ("gemini", _) => Some("gemini"),
        ("openai", None) => Some("openai"),
        ("openai", Some(base)) if compat::is_openai(base) => Some("openai"),
        ("openai", Some(base)) => Some(compat::provider_for(base).unwrap_or("openai_compat")),
        ("local", _) if entry.model_id.as_deref().is_some_and(is_ollama_cloud_model) => Some("ollama_cloud"),
        ("local", _) => Some("ollama"),
        _ => None,
    }
}

/// Chat messages, memories and tasks the chosen AI must not be shown: the
/// highest withheld row id in each table, 0 when nothing is withheld.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Withheld {
    pub conversations: i64,
    pub memories: i64,
    pub tasks: i64,
}

/// user_preferences: "true" or "false", whether the chosen AI could receive
/// email the last time `withheld` looked.
const MAIL_AI_WAS_ALLOWED: &str = "mail_ai_was_allowed";
/// user_preferences: the line drawn at the last move to an AI that may not
/// receive email, as "conversations,memories,tasks" row ids.
const WITHHELD_THROUGH: &str = "mail_withheld_through";

/// What the chosen AI must not be shown.
///
/// A reply that quoted an email, a memory saved from it and a task made from
/// it are Gmail content too, and all three are replayed into later prompts.
/// When the user moves from an AI that may receive email to one that may not,
/// everything saved up to that moment is withheld from the new AI; what is
/// saved afterwards cannot quote an email, because that AI is never given
/// one. Moving back to an allowed AI lifts the line. Every path that puts
/// stored chat, memory or task titles into a prompt asks this first.
///
/// The line is drawn by [`withheld_as_turn_begins`]. Until then everything
/// saved so far is withheld, which covers what a turn still running at the
/// move saves when it ends.
pub fn withheld(db: &Database) -> Withheld {
    line(db, false)
}

/// [`withheld`] for a tool or prompt block that leaves the withheld rows
/// out. When it hides anything, the consent card is shown for `feature`
/// (P06): nothing is held back silently.
pub fn withheld_shown(db: &Database, feature: &str) -> Withheld {
    let line = withheld(db);
    if line != Withheld::default() {
        if let Err(blocked) = check(db) {
            blocked.raise(feature);
        }
    }
    line
}

/// [`withheld`], for a chat turn that is about to save the user's message:
/// after a move this draws the line under everything saved so far.
pub fn withheld_as_turn_begins(db: &Database) -> Withheld {
    line(db, true)
}

fn line(db: &Database, turn_begins: bool) -> Withheld {
    let allowed = check(db).is_ok();
    // Gemini is what an install chats with until the user chooses otherwise.
    let gemini_only = crate::commands::ai_choice::saved_chain(db)
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.enabled)
        .all(|entry| entry.provider_type == "gemini");
    // Fail closed: a database that cannot be read withholds everything.
    let everything = Withheld { conversations: i64::MAX, memories: i64::MAX, tasks: i64::MAX };
    let Ok(conn) = db.conn.lock() else { return everything };
    let pref = |key: &str| {
        conn.query_row("SELECT value FROM user_preferences WHERE key = ?1", [key], |row| row.get::<_, String>(0)).ok()
    };
    let set = |key: &str, value: &str| {
        conn.execute(
            "INSERT INTO user_preferences (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
            [key, value],
        )
        .is_ok()
    };
    let recorded = pref(MAIL_AI_WAS_ALLOWED);
    if allowed {
        if recorded.as_deref() != Some("true") {
            let _ = conn.execute("DELETE FROM user_preferences WHERE key = ?1", [WITHHELD_THROUGH]);
            set(MAIL_AI_WAS_ALLOWED, "true");
        }
        return Withheld::default();
    }
    // No record: a build from before this existed. If Google data is on this
    // Mac, an allowed AI may have read it, so treat this as a move -- unless
    // the AI is Gemini without billing, refused email all along, and no other
    // AI has answered here: then nothing saved came from an AI that read email.
    let was_allowed = match recorded.as_deref() {
        Some(value) => value == "true",
        None => {
            // Both levels of the usage: a request already moved into the
            // daily totals still says another AI answered here.
            let other_ai = crate::ai::usage::any_usage_except(&conn, "gemini").unwrap_or(true);
            conn.query_row(
                "SELECT (EXISTS(SELECT 1 FROM emails) OR EXISTS(SELECT 1 FROM mail_messages) OR EXISTS(SELECT 1 FROM calendar_events)) \
                 AND (?1 = 0 OR ?2)",
                [gemini_only, other_ai],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(true)
        }
    };
    if was_allowed {
        let newest = |table: &str| conn.query_row(&format!("SELECT COALESCE(MAX(id), 0) FROM {table}"), [], |row| row.get::<_, i64>(0));
        let (Ok(conversations), Ok(memories), Ok(tasks)) = (newest("conversations"), newest("user_memory"), newest("tasks")) else {
            return everything;
        };
        // The line is saved before the record changes, so a failed write is
        // tried again on the next turn instead of losing the line.
        if turn_begins && (!set(WITHHELD_THROUGH, &format!("{conversations},{memories},{tasks}")) || !set(MAIL_AI_WAS_ALLOWED, "false")) {
            return everything;
        }
        return Withheld { conversations, memories, tasks };
    }
    if recorded.is_none() {
        set(MAIL_AI_WAS_ALLOWED, "false");
    }
    match pref(WITHHELD_THROUGH) {
        None => Withheld::default(),
        Some(saved) => {
            let ids: Vec<i64> = saved.split(',').filter_map(|id| id.parse().ok()).collect();
            match ids[..] {
                [conversations, memories, tasks] => Withheld { conversations, memories, tasks },
                _ => everything,
            }
        }
    }
}

/// Ollama names its cloud models "<name>:cloud" or "<name>:<size>-cloud" and
/// forwards their prompts to ollama.com.
pub fn is_ollama_cloud_model(model: &str) -> bool {
    let name = model.trim().to_ascii_lowercase();
    let registry = super::registry::current();
    registry
        .provider("ollama_cloud")
        .is_some_and(|cloud| cloud.model_id_suffixes.iter().any(|suffix| name.ends_with(&suffix.to_ascii_lowercase())))
}

/// A local model keeps email on this Mac. The address of an endpoint on
/// another machine, None for one on this Mac (or no endpoint named), or
/// the label to refuse an endpoint that is not saved.
fn endpoint_off_this_mac(db: &Database, endpoint_id: Option<&str>) -> Result<Option<String>, String> {
    let Some(id) = endpoint_id else { return Ok(None) };
    let url: Option<String> = db.conn.lock().ok().and_then(|conn| {
        conn.query_row("SELECT url FROM local_endpoints WHERE id = ?1", [id], |row| row.get(0)).ok()
    });
    let Some(url) = url else { return Err("this AI endpoint".to_string()) };
    if matches!(compat::host(&url).as_str(), "localhost" | "127.0.0.1" | "[::1]" | "::1") {
        Ok(None)
    } else {
        Ok(Some(url))
    }
}

/// For tests of the paths that ask `withheld`: choose the one AI in use.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::db::Database;

    /// An OpenAI-compatible service that may not receive email.
    pub const KIMI: Option<&str> = Some("https://api.moonshot.ai/v1");

    pub fn use_ai(db: &Database, provider: &str, base_url: Option<&str>) {
        let conn = db.conn.lock().unwrap();
        conn.execute("DELETE FROM provider_chain", []).unwrap();
        conn.execute(
            "INSERT INTO provider_chain(position, provider_type, endpoint_id, model_id, enabled, base_url) VALUES (0, ?1, NULL, NULL, 1, ?2)",
            rusqlite::params![provider, base_url],
        )
        .unwrap();
    }

    /// A database where OpenAI has been in use and the user has just moved to
    /// Kimi. Call `before` while OpenAI is still the chosen AI.
    pub fn moved_to_kimi_after(before: impl FnOnce(&Database)) -> Database {
        let db = crate::db::test_database();
        use_ai(&db, "openai", None);
        super::withheld(&db);
        before(&db);
        use_ai(&db, "openai", KIMI);
        db
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{use_ai, KIMI};
    use super::*;

    fn db_with(chain: &[(&str, Option<&str>, Option<&str>)], prefs: &[(&str, &str)]) -> Database {
        let path = std::env::temp_dir().join(format!("jarvis-mailpolicy-{}.db", uuid::Uuid::new_v4()));
        let db = crate::db::test_database_at(&path);
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO local_endpoints(id, name, url, backend_type) VALUES ('mac', 'Ollama', 'http://localhost:11434', 'ollama'), ('lan', 'Box', 'https://llm.example.com', 'generic')",
                [],
            )
            .unwrap();
            conn.execute("DELETE FROM provider_chain", []).unwrap();
            for (i, (provider, base_url, endpoint)) in chain.iter().enumerate() {
                conn.execute(
                    "INSERT INTO provider_chain(position, provider_type, endpoint_id, model_id, enabled, base_url) VALUES (?1, ?2, ?3, NULL, 1, ?4)",
                    rusqlite::params![i as i64, provider, endpoint, base_url],
                )
                .unwrap();
            }
            for (key, value) in prefs {
                conn.execute("INSERT INTO user_preferences(key, value) VALUES (?1, ?2)", [key, value]).unwrap();
            }
        }
        db
    }

    #[test]
    fn providers_that_do_not_train_on_api_data_receive_email() {
        for chain in [
            vec![("openai", None, None)],
            vec![("claude", None, None)],
            vec![("bedrock", None, None)],
            vec![("local", None, Some("mac"))],
            vec![("openai", Some("https://api.openai.com/v1"), None)],
        ] {
            assert_eq!(check(&db_with(&chain, &[])), Ok(()), "{chain:?}");
        }
    }

    /// The consent card `check` asks for.
    fn card(db: &Database) -> consent::Request {
        match check(db) {
            Err(Blocked::NeedsConsent(request)) => request,
            other => panic!("expected a consent card, got {other:?}"),
        }
    }

    fn summary(request: &consent::Request) -> (&str, &str, &str, DataTerms) {
        (request.provider.as_str(), request.base_url.as_str(), request.label.as_str(), request.terms)
    }

    /// Each data_terms level: safe just works (above); the others ask for
    /// consent, are allowed after it and ask again after it is revoked.
    #[test]
    fn each_data_terms_level_before_and_after_consent_and_after_revocation() {
        let cases: [(&[(&str, Option<&str>, Option<&str>)], (&str, &str, &str, DataTerms)); 3] = [
            (&[("gemini", None, None)], ("gemini", "", "Google Gemini", DataTerms::NeedsBillingConfirmation)),
            (&[("openai", KIMI, None)], ("kimi", "https://api.moonshot.ai/v1", "Kimi", DataTerms::Unknown)),
            (&[("local", None, Some("lan"))], ("ollama", "https://llm.example.com", "llm.example.com", DataTerms::Unknown)),
        ];
        for (chain, expected) in cases {
            let db = db_with(chain, &[]);
            let request = card(&db);
            assert_eq!(summary(&request), expected, "before consent");
            consent::give(&db, &request.subject(), request.terms).unwrap();
            assert_eq!(check(&db), Ok(()), "after consent: {expected:?}");
            consent::revoke(&db, &request.subject()).unwrap();
            assert_eq!(summary(&card(&db)), expected, "after revocation");
        }
        // No saved choice means the old Gemini default.
        assert_eq!(card(&db_with(&[], &[])).provider, "gemini");
    }

    /// A request goes only to the first enabled row, so only that row is
    /// asked about; a model picked in the error card is asked about for the
    /// request it runs.
    #[test]
    fn the_gate_asks_about_the_model_the_request_uses() {
        const QWEN: &str = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1";
        let db = db_with(&[("openai", None, None), ("openai", Some(QWEN), None)], &[]);
        assert_eq!(check(&db), Ok(()), "a later row is only offered in the error card");
        let qwen = crate::ai::capability::Pick { provider_type: "openai".into(), base_url: Some(QWEN.into()), endpoint_id: None, model_id: Some("qwen-plus".into()) };
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let picked = runtime.block_on(crate::ai::capability::with_pick(Some(qwen), async { check(&db) }));
        assert!(matches!(picked, Err(Blocked::NeedsConsent(ref r)) if r.provider == "qwen" && r.base_url == QWEN), "{picked:?}");
    }

    /// A consent holds while the terms in force are not stricter: a registry
    /// that moves Gemini from billing to unknown asks again, with the
    /// unknown-terms card.
    #[test]
    fn tightened_terms_ask_for_consent_again() {
        let db = db_with(&[("gemini", None, None)], &[]);
        let request = card(&db);
        assert_eq!((request.blocked_key.as_str(), request.confirm_key.as_str()), ("consentGeminiBillingBlocked", "consentGeminiBillingConfirm"));
        consent::give(&db, &request.subject(), request.terms).unwrap();
        assert_eq!(check(&db), Ok(()));
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "gemini").unwrap().data_terms = DataTerms::Unknown);
        let again = card(&db);
        assert_eq!(again.terms, DataTerms::Unknown);
        assert_eq!(again.blocked_key, "consentUnknownTermsBlocked");
    }

    /// P05's rule still holds at the gate: a fetched copy that calls every
    /// provider safe is floored by the bundled copy, so Gemini, Kimi and
    /// Ollama Cloud still wait for consent.
    #[test]
    fn a_fetched_registry_cannot_loosen_the_gate() {
        let bundled = crate::ai::registry::fixture();
        let mut loose = (*bundled).clone();
        for p in &mut loose.providers {
            p.data_terms = DataTerms::Safe;
            p.consent_copy_keys = None;
        }
        crate::ai::registry::load::apply_floor(&bundled, &mut loose);
        let _registry = crate::ai::registry::test_override::set(loose);
        assert_eq!(card(&db_with(&[("gemini", None, None)], &[])).terms, DataTerms::NeedsBillingConfirmation);
        assert_eq!(card(&db_with(&[("openai", KIMI, None)], &[])).terms, DataTerms::Unknown);
        let cloud = db_with(&[("local", None, Some("mac"))], &[]);
        cloud.conn.lock().unwrap().execute("UPDATE provider_chain SET model_id = 'gpt-oss:120b-cloud'", []).unwrap();
        assert_eq!(card(&cloud).terms, DataTerms::Unknown);
        assert_eq!(check(&db_with(&[("claude", None, None)], &[])), Ok(()));
    }

    /// The registry's data terms decide. A copy that tightens a provider asks
    /// for consent under the stricter terms, and a consent given for one
    /// provider is not another's; the Ollama endpoint must still be on this
    /// Mac.
    #[test]
    fn the_registry_data_terms_decide() {
        let _registry = crate::ai::registry::test_override::with(|r| {
            let mut terms = |id: &str, t: DataTerms| r.providers.iter_mut().find(|p| p.id == id).unwrap().data_terms = t;
            terms("claude", DataTerms::Unknown);
            terms("openai", DataTerms::NeedsBillingConfirmation);
            terms("kimi", DataTerms::Safe);
        });
        assert_eq!(summary(&card(&db_with(&[("claude", None, None)], &[]))), ("claude", "", "Anthropic Claude", DataTerms::Unknown));
        let openai = db_with(&[("openai", None, None)], &[]);
        consent::give(&openai, &Subject { provider: "gemini".into(), base_url: String::new() }, DataTerms::NeedsBillingConfirmation).unwrap();
        assert_eq!(summary(&card(&openai)), ("openai", "", "OpenAI (GPT)", DataTerms::NeedsBillingConfirmation), "Gemini's consent is not OpenAI's");
        assert_eq!(check(&db_with(&[("openai", Some("https://api.moonshot.ai/v1"), None)], &[])), Ok(()), "the terms the copy in use states");
        assert_eq!(check(&db_with(&[("bedrock", None, None)], &[])), Ok(()));
        assert_eq!(card(&db_with(&[("local", None, Some("lan"))], &[])).label, "llm.example.com");
        let mystery = ProviderChainEntry { position: 0, provider_type: "mystery".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None };
        assert_eq!(registry_provider(&mystery), None, "a row type this build does not know has no terms");
    }

    /// After consent, everything `withheld` hid from that AI is shown again;
    /// revoking it draws the line again.
    #[test]
    fn consent_lifts_what_was_withheld_and_revoking_it_withholds_again() {
        let db = db_with(&[("openai", None, None)], &[]);
        assert_eq!(withheld(&db), Withheld::default());
        save_a_turn(&db, "Kenny owes HK$12,000");
        use_ai(&db, "openai", KIMI);
        assert_eq!(withheld_as_turn_begins(&db), Withheld { conversations: 1, memories: 1, tasks: 1 });

        let kimi = card(&db).subject();
        consent::give(&db, &kimi, DataTerms::Unknown).unwrap();
        assert_eq!(withheld_as_turn_begins(&db), Withheld::default());
        save_a_turn(&db, "As Kenny wrote, HK$12,000 is due");

        consent::revoke(&db, &kimi).unwrap();
        assert_eq!(withheld_as_turn_begins(&db), Withheld { conversations: 2, memories: 2, tasks: 2 });
    }

    /// One chat message, one memory and one task, as a turn that read an
    /// email leaves behind.
    fn save_a_turn(db: &Database, text: &str) {
        db.conn
            .lock()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO conversations(role, content) VALUES ('assistant', '{text}');
                 INSERT INTO user_memory(content) VALUES ('{text}');
                 INSERT INTO tasks(title) VALUES ('{text}');"
            ))
            .unwrap();
    }

    /// A reply that quoted an email, a memory saved from it and a task made
    /// from it are replayed to whichever AI is chosen later. Moving to an AI
    /// that may not receive email draws a line under everything saved so far.
    #[test]
    fn switching_to_an_ai_that_may_not_read_email_withholds_what_was_saved_before() {
        let db = db_with(&[("openai", None, None)], &[]);
        save_a_turn(&db, "Kenny owes HK$12,000");
        assert_eq!(withheld(&db), Withheld::default(), "an AI that may read email sees everything");

        use_ai(&db, "openai", KIMI);
        let line = withheld_as_turn_begins(&db);
        assert_eq!(line, Withheld { conversations: 1, memories: 1, tasks: 1 });

        // What is saved while Kimi is in use cannot quote an email, and the
        // line stays where it was drawn.
        save_a_turn(&db, "Prefers short replies");
        assert_eq!(withheld(&db), line);
    }

    #[test]
    fn going_back_to_an_allowed_ai_shows_everything_and_the_next_switch_draws_a_new_line() {
        let db = db_with(&[("openai", None, None)], &[]);
        assert_eq!(withheld(&db), Withheld::default());
        save_a_turn(&db, "first");
        use_ai(&db, "openai", KIMI);
        assert_eq!(withheld_as_turn_begins(&db).conversations, 1);

        use_ai(&db, "openai", None);
        assert_eq!(withheld(&db), Withheld::default());
        save_a_turn(&db, "second");

        use_ai(&db, "openai", KIMI);
        assert_eq!(withheld_as_turn_begins(&db), Withheld { conversations: 2, memories: 2, tasks: 2 });
    }

    /// The AI can be changed while a turn that read an email is still
    /// running. What that turn saves when it ends is from before the move, so
    /// the line is drawn when the next turn begins, not by whichever tool or
    /// briefing happens to ask first.
    #[test]
    fn what_a_turn_still_running_at_the_move_saves_is_withheld_too() {
        let db = db_with(&[("openai", None, None)], &[]);
        assert_eq!(withheld(&db), Withheld::default());
        save_a_turn(&db, "Kenny owes HK$12,000");
        use_ai(&db, "openai", KIMI);

        // The running turn lists tasks: everything saved so far is withheld.
        assert_eq!(withheld(&db), Withheld { conversations: 1, memories: 1, tasks: 1 });
        save_a_turn(&db, "As Kenny wrote, HK$12,000 is due");

        let line = withheld_as_turn_begins(&db);
        assert_eq!(line, Withheld { conversations: 2, memories: 2, tasks: 2 });
        save_a_turn(&db, "Prefers short replies");
        assert_eq!(withheld(&db), line);
    }

    /// An install that has only ever used an AI that may not read email has
    /// nothing to withhold. One updating from a build without this record is
    /// treated as coming from an allowed AI when Google data is on the Mac.
    #[test]
    fn without_a_recorded_switch_only_an_install_holding_google_data_withholds() {
        let db = db_with(&[("openai", KIMI, None)], &[]);
        save_a_turn(&db, "no Google account was ever connected");
        assert_eq!(withheld_as_turn_begins(&db), Withheld::default());
        save_a_turn(&db, "still nothing to withhold");
        assert_eq!(withheld_as_turn_begins(&db), Withheld::default());

        let db = db_with(&[("openai", KIMI, None)], &[]);
        save_a_turn(&db, "saved while another AI could read email");
        add_a_calendar_event(&db);
        assert_eq!(withheld_as_turn_begins(&db), Withheld { conversations: 1, memories: 1, tasks: 1 });
    }

    fn add_a_calendar_event(db: &Database) {
        db.conn
            .lock()
            .unwrap()
            .execute("INSERT INTO calendar_events(google_id, summary, start_time, end_time) VALUES ('e1', 'Board', '2026-11-02T10:00:00Z', '2026-11-02T11:00:00Z')", [])
            .unwrap();
    }

    /// Gemini without billing confirmed is what an install chats with until
    /// the user chooses otherwise, and it has been refused email all along.
    /// Updating must not hide that user's memory and chat from the AI that
    /// wrote them. Only an answer from some other AI on record means an AI
    /// that could read email was in use before.
    #[test]
    fn updating_on_gemini_without_billing_withholds_only_if_another_ai_answered_before() {
        for chain in [&[][..], &[("gemini", None, None)][..]] {
            let db = db_with(chain, &[]);
            save_a_turn(&db, "Prefers short replies");
            add_a_calendar_event(&db);
            assert_eq!(withheld_as_turn_begins(&db), Withheld::default());
            assert_eq!(withheld_as_turn_begins(&db), Withheld::default());

            let db = db_with(chain, &[]);
            save_a_turn(&db, "Kenny owes HK$12,000");
            add_a_calendar_event(&db);
            db.conn
                .lock()
                .unwrap()
                .execute("INSERT INTO inference_metrics(ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider) VALUES (1, 'volume', 'gpt-5', 1, 1, 1, 'openai')", [])
                .unwrap();
            assert_eq!(withheld_as_turn_begins(&db), Withheld { conversations: 1, memories: 1, tasks: 1 });
        }
    }

    /// The same answer once it has been moved into the daily totals: the
    /// check reads both levels of the usage.
    #[test]
    fn an_answer_already_in_the_daily_totals_still_counts() {
        let db = db_with(&[("gemini", None, None)], &[]);
        save_a_turn(&db, "Kenny owes HK$12,000");
        add_a_calendar_event(&db);
        let now = chrono::Utc::now().timestamp();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO inference_metrics(ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider) VALUES (?1, 'volume', 'gpt-5', 1, 1, 1, 'openai')",
                [now - 40 * 86_400],
            )
            .unwrap();
            assert_eq!(crate::ai::usage::roll_up(&conn, now, 30).unwrap().requests_moved, 1);
            let left: i64 = conn.query_row("SELECT COUNT(*) FROM inference_metrics", [], |r| r.get(0)).unwrap();
            assert_eq!(left, 0, "the answer is only in the daily totals now");
        }
        assert_eq!(withheld_as_turn_begins(&db), Withheld { conversations: 1, memories: 1, tasks: 1 });
    }

    /// Ollama on this Mac forwards a "-cloud" model's prompt to ollama.com, so
    /// the loopback endpoint alone does not keep email on the Mac.
    #[test]
    fn an_ollama_cloud_model_is_not_a_model_on_this_mac() {
        let with_model = |model: &str| {
            let db = db_with(&[("local", None, Some("mac"))], &[]);
            db.conn.lock().unwrap().execute("UPDATE provider_chain SET model_id = ?1", [model]).unwrap();
            db
        };
        for model in ["gpt-oss:120b-cloud", "qwen3-coder:480b-cloud", "glm-4.6:cloud", "GPT-OSS:20B-CLOUD"] {
            assert_eq!(summary(&card(&with_model(model))), ("ollama_cloud", "", "Ollama Cloud", DataTerms::Unknown), "{model}");
        }
        for model in ["qwen3:8b", "cloudy-day:latest", "llama3.2"] {
            assert_eq!(check(&with_model(model)), Ok(()), "{model}");
        }
    }
}
