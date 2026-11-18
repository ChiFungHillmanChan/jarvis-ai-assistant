//! Which AI may receive email and calendar content.
//!
//! Google's Limited Use rules for Gmail data forbid transferring it to be used
//! to train AI models. OpenAI, Anthropic and Amazon Bedrock do not train on API
//! traffic by default, and a model on this Mac keeps email local. The
//! registry classifies every provider's data terms (`providers[].data_terms`):
//! a `safe` provider receives email; Gemini (`needs_billing_confirmation`:
//! its unpaid tier may use submitted content to improve Google's models,
//! outside the EEA, Switzerland and the UK) receives it once the user
//! confirms billing on the consent card (ai/consent.rs, P06). A provider
//! JARVIS cannot vouch for (`unknown`: Kimi, Qwen and other compatible
//! services, Ollama Cloud, an Ollama on another machine, a provider new in a
//! fetched copy) never receives it, whatever the user agreed to (DECISIONS,
//! P06 "Google Limited Use", 2026-11-17). Every path that sends email text to
//! the chosen AI (sorting, analysis, briefings, chat context and tools) asks
//! `check` first. Google Calendar data falls under the same Google rules, so
//! event titles and places in the chat context and `list_events` ask it too.
//!
//! TypeSafe, which classifies mail when the user adds its key, is not the
//! chosen AI and is not checked here: its privacy policy says it does not
//! train or fine-tune models on prompts or other input.

use super::bedrock::UiLanguage;
use super::compat;
use super::consent::{self, Subject};
use super::registry::DataTerms;
use super::ProviderChainEntry;
use crate::db::Database;

#[derive(Debug, PartialEq)]
pub enum Blocked {
    /// The provider's terms need the user's consent, not given yet (or
    /// given under milder terms than those now in force).
    NeedsConsent(Box<consent::Request>),
    /// Terms JARVIS cannot confirm (`unknown`): Gmail and Calendar content is
    /// not sent, consent or not. `label` names the service; `message` says
    /// why, in the UI language.
    NotForGoogleContent { label: String, message: String },
    /// A row type this build does not know: there is nothing to agree to.
    NotAllowed(String),
    /// JARVIS cannot tell which AI would receive it: the saved choice or
    /// the keys could not be read, or no AI is set up. Fail closed; the
    /// sentence says why, in the UI language.
    Undecided(String),
}

impl Blocked {
    pub fn code(&self) -> &'static str {
        match self {
            Blocked::NeedsConsent(_) => "consent_needed",
            Blocked::NotForGoogleContent { .. } => "google_content_not_sent",
            Blocked::NotAllowed(_) => "mail_ai_not_allowed",
            Blocked::Undecided(_) => "mail_ai_undecided",
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
            Blocked::NotForGoogleContent { label, .. } => format!(
                "Gmail and Calendar content is not sent to {label}: JARVIS cannot confirm how this service uses the \
                 data it receives, so this content stays with the AI services whose terms JARVIS can confirm. Tell \
                 the user in one sentence that reading their email or calendar needs another AI."
            ),
            Blocked::NotAllowed(label) => format!("Email and calendar content is not shared with {label}, an AI service this version of JARVIS does not know."),
            Blocked::Undecided(_) => "Gmail and Calendar content was not sent: JARVIS could not tell which AI service would receive it. Tell the user in one sentence.".to_string(),
        }
    }

    /// The error a page the user is on fails with: the consent card's data,
    /// or a sentence when there is nothing to agree to.
    pub fn user_message(&self) -> String {
        match self {
            Blocked::NeedsConsent(request) => request.to_error(),
            Blocked::NotForGoogleContent { message, .. } => message.clone(),
            Blocked::NotAllowed(label) => format!("電郵內容不會傳送至 {label}：這個版本的 JARVIS 不認識這個 AI 服務。"),
            Blocked::Undecided(message) => message.clone(),
        }
    }

    /// Show the consent card for `feature` (see [`consent::Request::raise`]).
    /// Content that is not sent at all raises one notice per launch for
    /// each service and feature: in the chat a refused tool call ends the
    /// turn on the error card (`ai::run_chosen`), and the notice says once
    /// that the content stays back from the AI in use.
    pub fn raise(&self, feature: &str) {
        match self {
            Blocked::NeedsConsent(request) => request.raise(feature),
            Blocked::NotForGoogleContent { label, message } => {
                if super::notify::first_time(&format!("google_content|{label}|{feature}")) {
                    log::info!("[mail_policy] {feature}: Gmail and Calendar content is not sent to the AI in use");
                    super::notify::emit("ai-needs-model", serde_json::json!({ "feature": feature, "message": message, "model": label, "code": "google_content" }));
                }
            }
            Blocked::NotAllowed(_) | Blocked::Undecided(_) => {}
        }
    }
}

/// Why Gmail and Calendar content is not sent to an `unknown`-terms service.
pub(crate) fn not_for_google_content(label: &str, language: UiLanguage) -> String {
    language.pick(
        format!("Gmail and Calendar content is not sent to {label}: JARVIS cannot confirm how this service uses data. Choose an AI whose terms JARVIS can confirm for this."),
        format!("Gmail 及日曆內容不會傳送至 {label}：JARVIS 無法確認這個服務如何使用資料。請改用 JARVIS 能確認其條款的 AI 處理。"),
    )
}

/// Whether email content may go to the AI this request uses: a model
/// picked in the error card for this one request, else [`receiver`].
pub fn check(db: &Database) -> Result<(), Blocked> {
    if let Some(pick) = super::capability::picked() {
        return allowed(db, &pick.entry());
    }
    allowed(db, &receiver(db)?)
}

/// The row a request with no pick is sent to, as the router decides it
/// (`AiRouter::chosen`): the first enabled row of a saved chain, else, for a
/// chain nobody saved, [`super::unsaved_choice`] from the preset and the keys.
/// Refused when any of that cannot be read, or no AI is set up (review B,
/// I-1): never a guess.
pub(crate) fn receiver(db: &Database) -> Result<ProviderChainEntry, Blocked> {
    let language = UiLanguage::read(db);
    let unreadable = || {
        Blocked::Undecided(language.pick(
            "JARVIS could not tell which AI would answer, so Gmail and Calendar content was not sent. Try again.",
            "JARVIS 未能確定由哪個 AI 回覆，因此沒有傳送 Gmail 及日曆內容。請再試一次。",
        ))
    };
    let none_set_up = || {
        Blocked::Undecided(language.pick(
            "No AI is set up to answer, so Gmail and Calendar content was not sent. Add a key or choose an AI in Settings.",
            "尚未設定回覆的 AI，因此沒有傳送 Gmail 及日曆內容。請在設定中加入密鑰或選擇 AI。",
        ))
    };
    let saved = crate::commands::ai_choice::saved_chain(db).map_err(|_| unreadable())?;
    // The router follows a saved chain whenever it has rows (load_from_db).
    if !saved.is_empty() {
        return saved.into_iter().find(|entry| entry.enabled).ok_or_else(none_set_up);
    }
    let preset = {
        let conn = db.conn.lock().map_err(|_| unreadable())?;
        match conn.query_row("SELECT value FROM user_preferences WHERE key = 'ai_provider'", [], |row| row.get::<_, String>(0)) {
            Ok(preset) => preset,
            Err(rusqlite::Error::QueryReturnedNoRows) => String::new(),
            Err(_) => return Err(unreadable()),
        }
    };
    let keys = super::keys_found().ok_or_else(unreadable)?;
    super::unsaved_choice(&preset, keys, super::bedrock_saved(db)).ok_or_else(none_set_up)
}

/// Whether Gmail and Calendar content may go out on `router` now: the row
/// its next request is sent to (a model picked in the error card, or the
/// router's own choice, as `ai::complete` and `run_chosen` take it), judged
/// as [`check`] judges the saved one. A job that built its router when it
/// began (a sort run, the briefing) asks this before each send, so a choice
/// saved in Settings since then does not change what is judged (review B,
/// r4-m1).
pub(crate) async fn may_send_on(db: &Database, router: &super::AiRouter) -> Result<(), Blocked> {
    let entry = match super::capability::picked() {
        Some(pick) => Some(pick.entry()),
        None => router.chosen_now(db).await,
    };
    match entry {
        Some(entry) => allowed(db, &entry),
        None => Err(Blocked::Undecided(UiLanguage::read(db).pick(
            "No AI is set up to answer, so Gmail and Calendar content was not sent. Add a key or choose an AI in Settings.",
            "尚未設定回覆的 AI，因此沒有傳送 Gmail 及日曆內容。請在設定中加入密鑰或選擇 AI。",
        ))),
    }
}

/// Whether Gmail content may go to `provider` used on the user's own key
/// rather than as the chosen AI: the Email page's OpenAI-key and Bedrock
/// modes, TypeSafe sorting and the OpenAI summary in sorting. The registry's
/// terms decide as they do for the chosen AI, so a fetched copy that tightens
/// one of these takes effect there too (review B, I-4).
pub fn may_send_to(db: &Database, provider: &str) -> Result<(), Blocked> {
    judge(db, Subject { provider: provider.to_string(), base_url: String::new() })
}

/// Whether Gmail and Calendar content may go to `entry` now: the models
/// the error card offers when a request needed it.
pub fn may_receive(db: &Database, entry: &ProviderChainEntry) -> bool {
    allowed(db, entry).is_ok()
}

/// The registry's data terms decide (`providers[].data_terms`): safe
/// receives email, Gemini's billing terms once the user agreed under them,
/// and `unknown` never, consent or not: Google's Limited Use rules cover
/// Gmail and Calendar content, and JARVIS cannot confirm how such a service
/// uses it. The bundled registry is the floor: a fetched copy can tighten a
/// provider, never loosen one (registry::load::apply_floor).
fn allowed(db: &Database, entry: &ProviderChainEntry) -> Result<(), Blocked> {
    let Some(provider) = registry_provider(entry) else {
        return Err(Blocked::NotAllowed(entry.provider_type.clone()));
    };
    // A local row with no model sends the registry's default for the
    // feature (`ai::run_chosen` puts the resolved model on the row), and
    // this gate does not know the feature: judged by every default the
    // registry gives Ollama (review B, m5).
    let provider = if provider == "ollama" && entry.model_id.is_none() && ollama_defaults_include_a_cloud_model() { "ollama_cloud" } else { provider };
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
    judge(db, Subject { provider: provider.to_string(), base_url })
}

/// The terms in force for `subject` decide: `unknown` never receives Gmail
/// or Calendar content; otherwise [`gate`].
fn judge(db: &Database, subject: Subject) -> Result<(), Blocked> {
    let (label, terms) = describe(&subject);
    if terms == DataTerms::Unknown {
        let message = not_for_google_content(&label, UiLanguage::read(db));
        return Err(Blocked::NotForGoogleContent { label, message });
    }
    gate(db, subject).map_err(Blocked::NeedsConsent)
}

/// Whether content may go to `subject`: its terms are safe, or the user
/// agreed to them under terms at least as strict as those in force now.
/// Otherwise the consent card to show.
pub(crate) fn gate(db: &Database, subject: Subject) -> Result<(), Box<consent::Request>> {
    let (label, terms) = describe(&subject);
    if consent::holds(db, &subject, terms) {
        return Ok(());
    }
    Err(Box::new(consent::Request::new(subject, label, terms)))
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
    // Only a turn beginning refuses (see `withheld_as_turn_begins`).
    line(db, false).unwrap_or(EVERYTHING)
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
///
/// On an AI that may receive email the turn is refused, before it saves
/// anything, when the record that such an AI answered cannot be written:
/// what the turn saves could quote an email, and without the record the next
/// AI that may not receive email would be shown it (review B, I-2).
pub fn withheld_as_turn_begins(db: &Database) -> Result<Withheld, String> {
    line(db, true).map_err(|NotRecorded| {
        UiLanguage::read(db).pick(
            "JARVIS could not record which AI answers this chat, so this message was not sent. Try again.".to_string(),
            "JARVIS 未能記錄由哪個 AI 回覆這段對話，因此沒有傳送這則訊息。請再試一次。".to_string(),
        )
    })
}

/// Fail closed: what a database that cannot be read, or a line that cannot
/// be saved, withholds.
const EVERYTHING: Withheld = Withheld { conversations: i64::MAX, memories: i64::MAX, tasks: i64::MAX };

/// An AI that may receive email is answering, and that could not be
/// recorded.
#[derive(Clone, Copy)]
struct NotRecorded;

fn line(db: &Database, turn_begins: bool) -> Result<Withheld, NotRecorded> {
    let allowed = check(db).is_ok();
    // A model picked once in the error card (P06) answers one request: what
    // it may be shown is worked out here, but nothing is recorded. What the
    // saved choice may be shown afterwards is [`PickedTurn`]'s to record.
    let records = super::capability::picked().is_none();
    // Gemini is what an install chats with until the user chooses otherwise.
    let gemini_only = crate::commands::ai_choice::saved_chain(db)
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.enabled)
        .all(|entry| entry.provider_type == "gemini");
    // Fail closed: a database that cannot be read withholds everything.
    let everything = Ok(EVERYTHING);
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
        if records && recorded.as_deref() != Some("true") {
            // The record first, then the line is lifted: the other way round,
            // a failed write would leave "never allowed, no line", and the
            // next AI that may not receive email would be shown everything.
            if !set(MAIL_AI_WAS_ALLOWED, "true") {
                return if turn_begins { Err(NotRecorded) } else { Ok(Withheld::default()) };
            }
            let _ = conn.execute("DELETE FROM user_preferences WHERE key = ?1", [WITHHELD_THROUGH]);
        }
        return Ok(Withheld::default());
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
        if turn_begins && records && (!set(WITHHELD_THROUGH, &format!("{conversations},{memories},{tasks}")) || !set(MAIL_AI_WAS_ALLOWED, "false")) {
            return everything;
        }
        return Ok(Withheld { conversations, memories, tasks });
    }
    if records && recorded.is_none() {
        set(MAIL_AI_WAS_ALLOWED, "false");
    }
    match pref(WITHHELD_THROUGH) {
        None => Ok(Withheld::default()),
        Some(saved) => {
            let ids: Vec<i64> = saved.split(',').filter_map(|id| id.parse().ok()).collect();
            match ids[..] {
                [conversations, memories, tasks] => Ok(Withheld { conversations, memories, tasks }),
                _ => everything,
            }
        }
    }
}

/// A request answered by a model picked once in the error card (P06,
/// Google Limited Use), while it runs.
///
/// When the picked model may receive Gmail and Calendar content, whatever
/// the request saves (the user's message, the reply, a memory or a task)
/// can quote it. The request is recorded as answered by an AI that may
/// receive email, as if the user had moved to it and back: a chosen AI that
/// may not is then shown nothing saved up to the end of the request
/// ([`withheld`]), until the user chooses one that may. It is recorded
/// before the request saves anything, and again when it ends, however it
/// ends: a line drawn while it ran (by another turn beginning) cannot leave
/// out what it saved after that line.
pub struct PickedTurn<'a> {
    db: &'a Database,
    /// Why the saved choice may not receive the content, if it may not.
    chosen: Option<Blocked>,
}

impl<'a> PickedTurn<'a> {
    /// In the pick's scope. None when the picked model may not receive the
    /// content either: nothing it saves can quote it. A record that cannot
    /// be written refuses the request.
    pub fn begin(db: &'a Database) -> Result<Option<PickedTurn<'a>>, String> {
        if check(db).is_err() {
            return Ok(None);
        }
        let chosen = super::capability::unpicked(|| check(db)).err();
        record_an_allowed_answer(db)?;
        Ok(Some(PickedTurn { db, chosen }))
    }

    /// The sentence under the answer when the saved choice will not see it.
    pub fn note(&self) -> Option<String> {
        let language = UiLanguage::read(self.db);
        match self.chosen.as_ref()? {
            Blocked::NeedsConsent(request) => Some(language.pick(
                format!("{} will not see this answer or the chat before it until you agree to share Gmail and Calendar content with it.", request.label),
                format!("在你同意與 {0} 分享 Gmail 及日曆內容前，{0} 不會看到這個回覆及之前的對話。", request.label),
            )),
            Blocked::NotForGoogleContent { label, .. } | Blocked::NotAllowed(label) => Some(language.pick(
                format!("{label} will not see this answer or the chat before it: Gmail and Calendar content is not sent to {label}."),
                format!("{label} 不會看到這個回覆及之前的對話：Gmail 及日曆內容不會傳送至 {label}。"),
            )),
            Blocked::Undecided(_) => Some(language.pick(
                "The AI you chose may not see this answer or the chat before it: JARVIS could not tell which AI answers next.",
                "你選用的 AI 未必會看到這個回覆及之前的對話：JARVIS 未能確定下一次由哪個 AI 回覆。",
            )),
        }
    }
}

impl Drop for PickedTurn<'_> {
    fn drop(&mut self) {
        if let Err(error) = record_an_allowed_answer(self.db) {
            log::warn!("[mail_policy] a picked answer could not be recorded at its end: {error}");
        }
    }
}

fn record_an_allowed_answer(db: &Database) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value) VALUES (?1, 'true') \
         ON CONFLICT(key) DO UPDATE SET value = 'true', updated_at = datetime('now')",
        [MAIL_AI_WAS_ALLOWED],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Whether a default the registry gives Ollama, for any feature, is a cloud
/// model.
fn ollama_defaults_include_a_cloud_model() -> bool {
    let registry = super::registry::current();
    registry.features.values().filter_map(|defaults| defaults.get("ollama")).any(|model| is_ollama_cloud_model(model))
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

    /// The user agrees to Kimi's terms, which JARVIS cannot confirm.
    pub fn agree_to_kimi(db: &Database) {
        let kimi = crate::ai::consent::Subject { provider: "kimi".into(), base_url: KIMI.unwrap().into() };
        crate::ai::consent::give(db, &kimi, crate::ai::registry::DataTerms::Unknown).unwrap();
    }

    /// The user confirms that their Gemini key has billing set up.
    pub fn agree_to_gemini(db: &Database) {
        let gemini = crate::ai::consent::Subject { provider: "gemini".into(), base_url: String::new() };
        crate::ai::consent::give(db, &gemini, crate::ai::registry::DataTerms::NeedsBillingConfirmation).unwrap();
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
    use super::test_support::{agree_to_gemini, agree_to_kimi, use_ai, KIMI};
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

    /// The gate and the router agree with no saved chain, for every set of
    /// the three keys, with and without saved AWS credentials, under each
    /// preset (one the registry does not have, and one with Bedrock first,
    /// as a fetched copy could add): `receiver` is the row the router's run
    /// sends to (`chosen` over what `configured` finds, as `run_chosen` takes
    /// it), or both have none; `check` judges that row; and a consent card
    /// names only that row (review B, r4-m2, from its matrix). Both are
    /// called; the rule is not restated.
    #[tokio::test]
    async fn the_gate_and_the_router_agree_for_every_key_combination() {
        use crate::ai::registry::doc::PresetEntry;
        use crate::ai::{keys_found, AiRouter, KeysFound};
        let _registry = crate::ai::registry::test_override::with(|r| {
            let mut bedrock_first = r.presets.chains[0].1.clone();
            bedrock_first.entries = vec![PresetEntry { provider: "bedrock".into(), model: None }, PresetEntry { provider: "gemini".into(), model: None }];
            r.presets.chains.push(("bedrock_first".into(), bedrock_first));
        });
        let row = |e: &ProviderChainEntry| (e.provider_type.clone(), e.model_id.clone(), e.base_url.clone(), e.endpoint_id.clone());
        let mut cases = 0;
        for preset in ["", "gemini_primary", "gemini_pro_only", "no_such_preset", "bedrock_first"] {
            for bits in 0..16u8 {
                let keys = KeysFound { gemini: bits & 1 != 0, claude: bits & 2 != 0, openai: bits & 4 != 0 };
                let bedrock = bits & 8 != 0;
                let db = crate::db::test_database();
                {
                    let conn = db.conn.lock().unwrap();
                    conn.execute("DELETE FROM provider_chain", []).unwrap();
                    conn.execute("DELETE FROM user_preferences WHERE key = 'ai_provider'", []).unwrap();
                    if !preset.is_empty() {
                        conn.execute("INSERT INTO user_preferences(key, value) VALUES ('ai_provider', ?1)", [preset]).unwrap();
                    }
                    if bedrock {
                        conn.execute("INSERT INTO aws_credentials(id, access_key_id, secret_access_key) VALUES (1, 'AKIA-TEST', 'not-real')", []).unwrap();
                    }
                }
                let _gate_sees = keys_found::with(keys);
                // Keys set on the router itself, so none in the environment counts.
                let mut router = AiRouter::new(preset).without_keys();
                if keys.gemini { router.set_gemini_key("test-g"); }
                if keys.claude { router.set_claude_key("test-c"); }
                if keys.openai { router.set_openai_key("test-o"); }
                router.append_default_fallbacks();
                assert_eq!(router.keys_found(), keys);
                let case = format!("{preset:?} {keys:?} bedrock={bedrock}");
                let sent = router.chosen(&router.configured(&db).await);
                match (&sent, receiver(&db)) {
                    (Some(s), Ok(j)) => assert_eq!(row(s), row(&j), "{case}"),
                    (None, Err(Blocked::Undecided(_))) => {}
                    (s, j) => panic!("{case}: the router sends to {s:?}, the gate judges {j:?}"),
                }
                match &sent {
                    Some(s) => {
                        assert_eq!(check(&db), allowed(&db, s), "{case}");
                        if let Err(Blocked::NeedsConsent(request)) = check(&db) {
                            assert_eq!(request.subject().provider, s.provider_type, "{case}");
                        }
                    }
                    None => assert!(matches!(check(&db), Err(Blocked::Undecided(_))), "{case}"),
                }
                cases += 1;
            }
        }
        assert_eq!(cases, 80);
    }

    /// With no saved chain the gate judges the row the request runs on, by
    /// the router's own rule (`ai::unsaved_choice`), not Gemini (review B,
    /// I-1, with review A, M-2).
    #[tokio::test]
    async fn with_no_saved_chain_the_gate_judges_the_row_the_request_runs_on() {
        use crate::ai::{keys_found, KeysFound};
        let claude_only = KeysFound { claude: true, ..KeysFound::default() };
        // A beta install: no saved chain, a Claude key only, and the Gemini
        // confirmation V36 carried over.
        let beta = db_with(&[], &[]);
        agree_to_gemini(&beta);
        let _keys = keys_found::with(claude_only);
        let mut router = crate::ai::AiRouter::new("");
        router.set_claude_key("test-claude");
        router.append_default_fallbacks();
        let runs_on = router.chosen(&router.configured(&beta).await).unwrap();
        assert_eq!(runs_on.provider_type, "claude");
        let row = |e: ProviderChainEntry| (e.provider_type, e.model_id, e.base_url, e.endpoint_id);
        assert_eq!(row(receiver(&beta).unwrap()), row(runs_on));
        assert_eq!(check(&beta), Ok(()));
        {
            // A fetched copy tightens Claude: refused, whatever was agreed to Gemini.
            let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "claude").unwrap().data_terms = DataTerms::Unknown);
            assert!(matches!(check(&beta), Err(Blocked::NotForGoogleContent { .. })), "{:?}", check(&beta));
        }
        // The wrong card: only a Claude key and nothing agreed. Claude
        // answers, so no Gemini consent card.
        let claude_install = db_with(&[], &[]);
        assert_eq!(check(&claude_install), Ok(()));
        {
            let _keys = keys_found::with(KeysFound { openai: true, ..KeysFound::default() });
            assert_eq!(receiver(&claude_install).unwrap().provider_type, "openai");
            assert_eq!(check(&claude_install), Ok(()));
        }
        {
            // A Gemini key: the preset's first row answers, and its card is asked for.
            let _keys = keys_found::with(KeysFound { gemini: true, claude: true, openai: true });
            assert_eq!(card(&claude_install).subject().provider, "gemini");
        }
        // Fail closed: no key at all, keys not read yet, a chain that cannot be read.
        let refused = |db: &Database| matches!(check(db), Err(Blocked::Undecided(_)));
        {
            let _keys = keys_found::with(KeysFound::default());
            assert!(refused(&claude_install));
        }
        {
            // Keys not read yet: JARVIS cannot tell, which is not "no AI set up".
            let _keys = keys_found::without();
            let not_read = check(&claude_install);
            assert!(matches!(&not_read, Err(Blocked::Undecided(message)) if message.starts_with("JARVIS could not tell which AI would answer")), "{not_read:?}");
        }
        claude_install.conn.lock().unwrap().execute_batch("DROP TABLE provider_chain").unwrap();
        assert!(refused(&claude_install));
        // A saved chain with every row turned off sends nothing: refused too.
        let all_off = db_with(&[("claude", None, None)], &[]);
        all_off.conn.lock().unwrap().execute("UPDATE provider_chain SET enabled = 0", []).unwrap();
        assert!(refused(&all_off));
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
            Err(Blocked::NeedsConsent(request)) => *request,
            other => panic!("expected a consent card, got {other:?}"),
        }
    }

    /// The service `check` refuses Gmail and Calendar content to, whatever
    /// was agreed (unknown terms).
    fn not_sent(db: &Database) -> String {
        match check(db) {
            Err(Blocked::NotForGoogleContent { label, message }) => {
                assert!(message.contains(&label), "{message}");
                label
            }
            other => panic!("expected the content to stay back, got {other:?}"),
        }
    }

    fn summary(request: &consent::Request) -> (&str, &str, &str, DataTerms) {
        (request.provider.as_str(), request.base_url.as_str(), request.label.as_str(), request.terms)
    }

    /// Each data_terms level: safe just works (above); Gemini's billing
    /// terms ask for consent, are allowed after it and ask again after it is
    /// revoked; unknown terms never receive the content, consent or not
    /// (DECISIONS, P06 "Google Limited Use", 2026-11-17).
    #[test]
    fn each_data_terms_level_before_and_after_consent_and_after_revocation() {
        let db = db_with(&[("gemini", None, None)], &[]);
        let request = card(&db);
        assert_eq!(summary(&request), ("gemini", "", "Google Gemini", DataTerms::NeedsBillingConfirmation), "before consent");
        consent::give(&db, &request.subject(), request.terms).unwrap();
        assert_eq!(check(&db), Ok(()), "after consent");
        consent::revoke(&db, &request.subject()).unwrap();
        assert_eq!(card(&db).provider, "gemini", "after revocation");

        // (chain row, the service's subject, its label)
        let unknown = [(("openai", KIMI, None), ("kimi", "https://api.moonshot.ai/v1"), "Kimi"), (("local", None, Some("lan")), ("ollama", "https://llm.example.com"), "llm.example.com")];
        for (row, (provider, base_url), label) in unknown {
            let db = db_with(&[row], &[]);
            assert_eq!(not_sent(&db), label, "before consent");
            let subject = Subject { provider: provider.into(), base_url: base_url.into() };
            assert_eq!(describe(&subject).1, DataTerms::Unknown);
            consent::give(&db, &subject, DataTerms::Unknown).unwrap();
            assert_eq!(not_sent(&db), label, "after consent");
            consent::revoke(&db, &subject).unwrap();
            assert_eq!(not_sent(&db), label, "after revocation");
        }
        // No saved choice: the preset's Gemini row answers when its key is
        // there, so its card is asked for.
        let _keys = crate::ai::keys_found::with(crate::ai::KeysFound { gemini: true, ..Default::default() });
        assert_eq!(card(&db_with(&[], &[])).provider, "gemini");
    }

    /// The rule is about the terms, not the name: Gemini classified as
    /// unknown in a registry copy keeps the content back although the user
    /// had confirmed billing, and a provider new in a copy (unknown to this
    /// build's list of terms) is refused too.
    #[test]
    fn unknown_terms_keep_google_content_back_whatever_was_agreed() {
        let db = db_with(&[("gemini", None, None)], &[]);
        agree_to_gemini(&db);
        assert_eq!(check(&db), Ok(()));
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "gemini").unwrap().data_terms = DataTerms::Unknown);
        assert_eq!(not_sent(&db), "Google Gemini");
        assert_eq!(describe(&Subject { provider: "brand_new".into(), base_url: String::new() }).1, DataTerms::Unknown);
    }

    /// Content that is not sent at all raises one notice per launch for the
    /// feature that kept it back (mail sorting, the briefing, the chat's
    /// held-back context); a refused tool call ends on the error card.
    #[test]
    fn content_not_sent_raises_one_notice_per_launch() {
        let db = db_with(&[("openai", KIMI, None)], &[]);
        let feature = format!("test-{}", uuid::Uuid::new_v4());
        check(&db).unwrap_err().raise(&feature);
        assert!(!crate::ai::notify::first_time(&format!("google_content|Kimi|{feature}")), "the notice was raised");
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
        assert!(matches!(picked, Err(Blocked::NotForGoogleContent { .. })), "{picked:?}");
        let gemini = crate::ai::capability::Pick { provider_type: "gemini".into(), base_url: None, endpoint_id: None, model_id: None };
        let picked = runtime.block_on(crate::ai::capability::with_pick(Some(gemini), async { check(&db) }));
        assert!(matches!(picked, Err(Blocked::NeedsConsent(ref r)) if r.provider == "gemini"), "{picked:?}");
    }

    /// A consent holds while the terms in force are not stricter: a registry
    /// that moves OpenAI from safe to billing terms asks for consent, and one
    /// that moves Gemini from billing to unknown keeps the content back
    /// although billing was confirmed.
    #[test]
    fn tightened_terms_ask_for_consent_again() {
        let db = db_with(&[("gemini", None, None)], &[]);
        let request = card(&db);
        assert_eq!((request.blocked_key.as_str(), request.confirm_key.as_str()), ("consentGeminiBillingBlocked", "consentGeminiBillingConfirm"));
        consent::give(&db, &request.subject(), request.terms).unwrap();
        assert_eq!(check(&db), Ok(()));
        let openai = db_with(&[("openai", None, None)], &[]);
        assert_eq!(check(&openai), Ok(()));
        let _registry = crate::ai::registry::test_override::with(|r| {
            r.providers.iter_mut().find(|p| p.id == "gemini").unwrap().data_terms = DataTerms::Unknown;
            r.providers.iter_mut().find(|p| p.id == "openai").unwrap().data_terms = DataTerms::NeedsBillingConfirmation;
        });
        assert_eq!(not_sent(&db), "Google Gemini");
        assert_eq!(card(&openai).terms, DataTerms::NeedsBillingConfirmation);
    }

    /// A consent is for one provider at one address: pointing the same
    /// service type at another address asks again, and coming back to the
    /// first address finds the consent still there until it is revoked.
    /// Only Gemini has consent terms in today's registry, and it has no
    /// address, so the compatible services stand in here with billing terms.
    #[test]
    fn a_consent_stops_holding_when_the_address_changes() {
        const OTHER: &str = "https://llm.example.org/v1";
        let _registry = crate::ai::registry::test_override::with(|r| {
            for p in r.providers.iter_mut().filter(|p| p.id == "kimi" || p.id == "openai_compat") {
                p.data_terms = DataTerms::NeedsBillingConfirmation;
            }
        });
        let db = db_with(&[("openai", KIMI, None)], &[]);
        let kimi = card(&db);
        consent::give(&db, &kimi.subject(), kimi.terms).unwrap();
        assert_eq!(check(&db), Ok(()));

        test_support::use_ai(&db, "openai", Some(OTHER));
        let other = card(&db);
        assert_eq!((other.provider.as_str(), other.base_url.as_str()), ("openai_compat", OTHER), "another address is asked about");

        test_support::use_ai(&db, "openai", KIMI);
        assert_eq!(check(&db), Ok(()), "Kimi's consent is untouched");
        consent::revoke(&db, &kimi.subject()).unwrap();
        assert_eq!(card(&db).base_url, KIMI.unwrap());
    }

    /// P05's rule still holds at the gate: a fetched copy that calls every
    /// provider safe is floored by the bundled copy, so Gemini still waits
    /// for consent and Kimi and Ollama Cloud still get no Google content.
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
        assert_eq!(not_sent(&db_with(&[("openai", KIMI, None)], &[])), "Kimi");
        let cloud = db_with(&[("local", None, Some("mac"))], &[]);
        cloud.conn.lock().unwrap().execute("UPDATE provider_chain SET model_id = 'gpt-oss:120b-cloud'", []).unwrap();
        assert_eq!(not_sent(&cloud), "Ollama Cloud");
        assert_eq!(check(&db_with(&[("claude", None, None)], &[])), Ok(()));
    }

    /// The registry's data terms decide. A copy that tightens a provider asks
    /// for consent under the stricter terms or keeps the content back, and a consent given for one
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
        assert_eq!(not_sent(&db_with(&[("claude", None, None)], &[])), "Anthropic Claude");
        let openai = db_with(&[("openai", None, None)], &[]);
        consent::give(&openai, &Subject { provider: "gemini".into(), base_url: String::new() }, DataTerms::NeedsBillingConfirmation).unwrap();
        assert_eq!(summary(&card(&openai)), ("openai", "", "OpenAI (GPT)", DataTerms::NeedsBillingConfirmation), "Gemini's consent is not OpenAI's");
        assert_eq!(check(&db_with(&[("openai", Some("https://api.moonshot.ai/v1"), None)], &[])), Ok(()), "the terms the copy in use states");
        assert_eq!(check(&db_with(&[("bedrock", None, None)], &[])), Ok(()));
        assert_eq!(not_sent(&db_with(&[("local", None, Some("lan"))], &[])), "llm.example.com");
        let mystery = ProviderChainEntry { position: 0, provider_type: "mystery".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None };
        assert_eq!(registry_provider(&mystery), None, "a row type this build does not know has no terms");
    }

    /// After Gemini's billing is confirmed, everything `withheld` hid from it
    /// is shown again; revoking it draws the line again. Agreeing to Kimi's
    /// terms lifts nothing: a chat turn, memory or task saved before the move
    /// may hold Gmail or Calendar content, and its origin cannot be told.
    #[test]
    fn consent_lifts_what_was_withheld_and_revoking_it_withholds_again() {
        let db = db_with(&[("openai", None, None)], &[]);
        assert_eq!(withheld(&db), Withheld::default());
        save_a_turn(&db, "Kenny owes HK$12,000");
        use_ai(&db, "gemini", None);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld { conversations: 1, memories: 1, tasks: 1 });

        agree_to_gemini(&db);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld::default());
        save_a_turn(&db, "As Kenny wrote, HK$12,000 is due");

        let gemini = Subject { provider: "gemini".into(), base_url: String::new() };
        consent::revoke(&db, &gemini).unwrap();
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld { conversations: 2, memories: 2, tasks: 2 });

        // Unknown terms: withheld before and after agreeing.
        use_ai(&db, "openai", None);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld::default());
        use_ai(&db, "openai", KIMI);
        let line = withheld_as_turn_begins(&db).unwrap();
        assert_eq!(line, Withheld { conversations: 2, memories: 2, tasks: 2 });
        agree_to_kimi(&db);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), line);
        assert_eq!(withheld(&db), line);
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
        let line = withheld_as_turn_begins(&db).unwrap();
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
        assert_eq!(withheld_as_turn_begins(&db).unwrap().conversations, 1);

        use_ai(&db, "openai", None);
        assert_eq!(withheld(&db), Withheld::default());
        save_a_turn(&db, "second");

        use_ai(&db, "openai", KIMI);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld { conversations: 2, memories: 2, tasks: 2 });
    }

    /// Every write of `key` (with `value`, if given) to user_preferences
    /// fails from here on, as on a full disk.
    fn fail_writes(db: &Database, key: &str, value: Option<&str>) {
        let when = match value {
            Some(value) => format!("NEW.key = '{key}' AND NEW.value = '{value}'"),
            None => format!("NEW.key = '{key}'"),
        };
        let name = uuid::Uuid::new_v4().simple().to_string();
        db.conn
            .lock()
            .unwrap()
            .execute_batch(&format!(
                "CREATE TRIGGER insert_{name} BEFORE INSERT ON user_preferences WHEN {when} BEGIN SELECT RAISE(ABORT, 'disk full'); END;
                 CREATE TRIGGER update_{name} BEFORE UPDATE ON user_preferences WHEN {when} BEGIN SELECT RAISE(ABORT, 'disk full'); END;"
            ))
            .unwrap();
    }

    /// Back on an AI that may receive email, the record that it answered
    /// cannot be written. The line drawn for Kimi stays where it was, and a
    /// chat turn is refused before it saves anything: what it saved could
    /// quote an email, with nothing to say so. The next Kimi turn is shown
    /// neither the turn from before the move nor anything after it.
    #[test]
    fn a_record_that_cannot_be_written_keeps_the_line_and_refuses_the_turn() {
        let db = db_with(&[("openai", None, None)], &[]);
        assert_eq!(withheld(&db), Withheld::default());
        save_a_turn(&db, "MAIL-CANARY-1");
        use_ai(&db, "openai", KIMI);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld { conversations: 1, memories: 1, tasks: 1 });

        fail_writes(&db, MAIL_AI_WAS_ALLOWED, Some("true"));
        use_ai(&db, "openai", None);
        assert_eq!(withheld(&db), Withheld::default(), "OpenAI may still be shown everything");
        let refused = withheld_as_turn_begins(&db).unwrap_err();
        assert_eq!(refused, "JARVIS could not record which AI answers this chat, so this message was not sent. Try again.");
        let saved = |db: &Database| db.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get::<_, i64>(0)).unwrap();
        let before = saved(&db);
        assert_eq!(crate::commands::chat::save_turn(&db, "s1", "What did Kenny write?").unwrap_err(), refused);
        assert_eq!(saved(&db), before, "nothing of the refused turn is saved");
        // "Send again" from the error card, for a message saved when it was
        // first sent (review B, R10).
        db.conn.lock().unwrap().execute("INSERT INTO conversations (role, content, session_id) VALUES ('user', 'What did Kenny write?', 's2')", []).unwrap();
        let before = saved(&db);
        assert_eq!(crate::commands::chat::save_retried_turn(&db, "s2", "What did Kenny write?").unwrap_err(), refused);
        assert_eq!(crate::commands::chat::save_retried_turn(&db, "s2", "Something else").unwrap_err(), refused);
        assert_eq!(saved(&db), before, "nothing of the refused retries is saved");

        use_ai(&db, "openai", KIMI);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld { conversations: 1, memories: 1, tasks: 1 });
    }

    /// A line that cannot be written when a Kimi turn begins withholds
    /// everything from that turn (review B, m3).
    #[test]
    fn a_line_that_cannot_be_written_withholds_everything() {
        let db = db_with(&[("openai", None, None)], &[]);
        assert_eq!(withheld(&db), Withheld::default());
        save_a_turn(&db, "MAIL-CANARY-1");
        use_ai(&db, "openai", KIMI);
        fail_writes(&db, WITHHELD_THROUGH, None);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), EVERYTHING);
    }

    /// A database lock that cannot be taken withholds everything (review B,
    /// m3).
    #[test]
    fn a_database_that_cannot_be_read_withholds_everything() {
        let db = db_with(&[("openai", KIMI, None)], &[]);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = db.conn.lock().unwrap();
            panic!("poisons the lock on purpose");
        }));
        assert_eq!(withheld(&db), EVERYTHING);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), EVERYTHING);
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

        let line = withheld_as_turn_begins(&db).unwrap();
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
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld::default());
        save_a_turn(&db, "still nothing to withhold");
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld::default());

        let db = db_with(&[("openai", KIMI, None)], &[]);
        save_a_turn(&db, "saved while another AI could read email");
        add_a_calendar_event(&db);
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld { conversations: 1, memories: 1, tasks: 1 });
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
            assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld::default());
            assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld::default());

            let db = db_with(chain, &[]);
            save_a_turn(&db, "Kenny owes HK$12,000");
            add_a_calendar_event(&db);
            db.conn
                .lock()
                .unwrap()
                .execute("INSERT INTO inference_metrics(ts, workload, model_id, input_tokens, output_tokens, latency_ms, provider) VALUES (1, 'volume', 'gpt-5', 1, 1, 1, 'openai')", [])
                .unwrap();
            assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld { conversations: 1, memories: 1, tasks: 1 });
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
        assert_eq!(withheld_as_turn_begins(&db).unwrap(), Withheld { conversations: 1, memories: 1, tasks: 1 });
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
            assert_eq!(not_sent(&with_model(model)), "Ollama Cloud", "{model}");
        }
        for model in ["qwen3:8b", "cloudy-day:latest", "llama3.2"] {
            assert_eq!(check(&with_model(model)), Ok(()), "{model}");
        }
    }

    /// A local row with no model sends the registry's default for the
    /// feature (`ai::run_chosen` puts the resolved model on the row). A
    /// copy that makes a "-cloud" model a default sends the prompt on to
    /// ollama.com, so the row is judged as Ollama Cloud (review B, m5).
    #[test]
    fn a_local_row_without_a_model_is_judged_by_the_model_it_sends() {
        let db = db_with(&[("local", None, Some("mac"))], &[]);
        assert_eq!(check(&db), Ok(()));
        for feature in ["chat", "mail_analysis"] {
            let _registry = crate::ai::registry::test_override::with(|r| {
                r.features.entry(feature.into()).or_default().insert("ollama".into(), "gpt-oss:120b-cloud".into());
            });
            assert_eq!(not_sent(&db), "Ollama Cloud", "{feature}");
            // A model saved on the row is the one sent.
            db.conn.lock().unwrap().execute("UPDATE provider_chain SET model_id = 'qwen3:8b'", []).unwrap();
            assert_eq!(check(&db), Ok(()), "{feature}");
            db.conn.lock().unwrap().execute("UPDATE provider_chain SET model_id = NULL", []).unwrap();
        }
    }
}
