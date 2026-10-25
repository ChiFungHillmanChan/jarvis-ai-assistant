//! Which AI may receive email and calendar content.
//!
//! Google's Limited Use rules for Gmail data forbid transferring it to be used
//! to train AI models. OpenAI, Anthropic and Amazon Bedrock do not train on API
//! traffic by default, and a model on this Mac keeps email local. Gemini's
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

#[cfg(test)]
mod tests {
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
}
