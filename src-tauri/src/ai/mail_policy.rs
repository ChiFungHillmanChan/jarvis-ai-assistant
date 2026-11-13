//! Which AI may receive email and calendar content.
//!
//! Google's Limited Use rules for Gmail data forbid transferring it to be used
//! to train AI models. OpenAI, Anthropic and Amazon Bedrock do not train on API
//! traffic by default, and a model on this Mac keeps email local (an Ollama
//! cloud model does not run on this Mac and is refused). Gemini's
//! unpaid tier may use submitted content to improve Google's models (outside
//! the EEA, Switzerland and the UK), so Gemini needs the user's confirmation
//! that billing is on. Kimi, Qwen and other services have not been checked and
//! do not receive email. Every path that sends email text to the chosen AI
//! (sorting, analysis, briefings, chat context and tools) asks `check` first.
//! Google Calendar data falls under the same Google rules, so event titles and
//! places in the chat context and `list_events` ask it too.
//!
//! TypeSafe, which classifies mail when the user adds its key, is not the
//! chosen AI and is not checked here: its privacy policy says it does not
//! train or fine-tune models on prompts or other input.

use super::compat;
use super::ProviderChainEntry;
use crate::db::Database;

/// user_preferences key set when the user confirms their Gemini key is billed.
pub const GEMINI_BILLING_CONFIRMED: &str = "gemini_billing_confirmed";

#[derive(Debug, PartialEq)]
pub enum Blocked {
    /// Gemini, before the user confirmed that billing is enabled.
    GeminiBillingUnconfirmed,
    /// A service that may use email to train models, or has not been checked.
    NotAllowed(String),
}

impl Blocked {
    pub fn code(&self) -> &'static str {
        match self {
            Blocked::GeminiBillingUnconfirmed => "gemini_billing_unconfirmed",
            Blocked::NotAllowed(_) => "mail_ai_not_allowed",
        }
    }

    /// For the model, which explains it to the user in their language.
    pub fn tool_message(&self) -> String {
        match self {
            Blocked::GeminiBillingUnconfirmed => "Email and calendar content is not shared with Gemini yet. \
                Gemini's free tier may use submitted content to train Google's models, so the user \
                must first confirm in Settings > Models that their Gemini API key has billing enabled."
                .to_string(),
            Blocked::NotAllowed(label) => format!(
                "Email and calendar content is not shared with {label}. Google's rules for Gmail and Calendar \
                 data do not allow sending it to an AI service that may use it for training. Email and calendar \
                 features work with \
                 OpenAI, Claude, AWS Bedrock, a local Ollama model, or Gemini with billing enabled; \
                 the user can choose one in Settings > Models."
            ),
        }
    }

    /// For errors shown in the app, next to the other mail messages.
    pub fn user_message(&self) -> String {
        match self {
            Blocked::GeminiBillingUnconfirmed => "Gemini 免費額度可能以提交的內容訓練 Google 的模型。\
                請先在「設定 → 模型」確認你的 Gemini API 金鑰已啟用付費，JARVIS 才會把電郵傳送給 Gemini。"
                .to_string(),
            Blocked::NotAllowed(label) => format!(
                "為符合 Google 對 Gmail 資料的規定，電郵內容不會傳送至 {label}。\
                 請在「設定 → 模型」改用 OpenAI、Claude、AWS Bedrock、本機 Ollama 模型，或已啟用付費的 Gemini。"
            ),
        }
    }
}

/// Whether email content may go to the AI the user chose.
pub fn check(db: &Database) -> Result<(), Blocked> {
    let chain: Vec<ProviderChainEntry> = crate::commands::ai_choice::saved_chain(db)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.enabled)
        .collect();
    // No saved choice: an install from before setup asked, which chats with
    // Gemini by default.
    if chain.is_empty() {
        return gemini(db);
    }
    chain.iter().try_for_each(|entry| allowed(db, entry))
}

fn allowed(db: &Database, entry: &ProviderChainEntry) -> Result<(), Blocked> {
    match (entry.provider_type.as_str(), entry.base_url.as_deref()) {
        ("claude" | "bedrock", _) | ("openai", None) => Ok(()),
        ("openai", Some(base)) if compat::host(base) == "api.openai.com" => Ok(()),
        ("openai", Some(base)) => Err(Blocked::NotAllowed(compat::label(base))),
        ("gemini", _) => gemini(db),
        ("local", _) if entry.model_id.as_deref().is_some_and(is_ollama_cloud_model) => Err(Blocked::NotAllowed("Ollama Cloud".into())),
        ("local", _) => local_on_this_mac(db, entry.endpoint_id.as_deref()),
        (other, _) => Err(Blocked::NotAllowed(other.to_string())),
    }
}

fn gemini(db: &Database) -> Result<(), Blocked> {
    let confirmed = db
        .conn
        .lock()
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = ?1",
                [GEMINI_BILLING_CONFIRMED],
                |row| row.get::<_, String>(0),
            )
            .ok()
        })
        .is_some_and(|value| value == "true");
    if confirmed {
        Ok(())
    } else {
        Err(Blocked::GeminiBillingUnconfirmed)
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
    name.ends_with(":cloud") || name.ends_with("-cloud")
}

/// A local model keeps email on this Mac; a "local" endpoint pointing at
/// another machine is a remote service like any other.
fn local_on_this_mac(db: &Database, endpoint_id: Option<&str>) -> Result<(), Blocked> {
    let Some(id) = endpoint_id else { return Ok(()) };
    let url: Option<String> = db.conn.lock().ok().and_then(|conn| {
        conn.query_row("SELECT url FROM local_endpoints WHERE id = ?1", [id], |row| row.get(0)).ok()
    });
    let host = url.as_deref().map(compat::host).unwrap_or_default();
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]" | "::1") {
        Ok(())
    } else {
        Err(Blocked::NotAllowed(if host.is_empty() { "this AI endpoint".to_string() } else { host }))
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

    #[test]
    fn gemini_needs_billing_confirmed_and_unchecked_services_are_refused() {
        assert_eq!(check(&db_with(&[("gemini", None, None)], &[])), Err(Blocked::GeminiBillingUnconfirmed));
        assert_eq!(check(&db_with(&[("gemini", None, None)], &[(GEMINI_BILLING_CONFIRMED, "true")])), Ok(()));
        assert_eq!(
            check(&db_with(&[("openai", Some("https://api.moonshot.ai/v1"), None)], &[])),
            Err(Blocked::NotAllowed("Kimi".into()))
        );
        assert_eq!(
            check(&db_with(&[("local", None, Some("lan"))], &[])),
            Err(Blocked::NotAllowed("llm.example.com".into()))
        );
        // A refused fallback blocks the chain: a request can land on any entry.
        assert!(check(&db_with(&[("openai", None, None), ("openai", Some("https://dashscope.aliyuncs.com/compatible-mode/v1"), None)], &[])).is_err());
        // No saved choice means the old Gemini default.
        assert_eq!(check(&db_with(&[], &[])), Err(Blocked::GeminiBillingUnconfirmed));
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
            assert_eq!(check(&with_model(model)), Err(Blocked::NotAllowed("Ollama Cloud".into())), "{model}");
        }
        for model in ["qwen3:8b", "cloudy-day:latest", "llama3.2"] {
            assert_eq!(check(&with_model(model)), Ok(()), "{model}");
        }
    }
}
