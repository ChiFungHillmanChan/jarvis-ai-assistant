//! Foreground identities are task-local; voice and scheduler calls remain unscoped.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Context {
    pub session_id: String,
    pub run_id: String,
    pub app: tauri::AppHandle,
}
tokio::task_local! { pub static CONTEXT: Context; }
pub static FOREGROUND: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn identity(value: Option<String>, default: &str) -> Result<String, String> {
    match value {
        None => Ok(default.to_owned()),
        Some(s)
            if !s.is_empty()
                && s.len() <= 128
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) =>
        {
            Ok(s)
        }
        _ => Err("Invalid assistant task or run ID".into()),
    }
}
fn scoped_payload(mut payload: Value, identity: Option<(&str, &str)>) -> Value {
    if let (Some(object), Some((session, run))) = (payload.as_object_mut(), identity) {
        object.insert("session_id".into(), json!(session));
        object.insert("run_id".into(), json!(run));
    }
    payload
}
/// Same call shape as Tauri Emitter, with scope only for chat events.
pub trait ScopedEmitter {
    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> tauri::Result<()>;
}
impl ScopedEmitter for tauri::AppHandle {
    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> tauri::Result<()> {
        let value = serde_json::to_value(payload)?;
        let value = if event.starts_with("chat-") {
            CONTEXT
                .try_with(|c| scoped_payload(value.clone(), Some((&c.session_id, &c.run_id))))
                .unwrap_or(value)
        } else {
            value
        };
        tauri::Emitter::emit(self, event, value)
    }
}
/// Fixed route registry. Legacy Gmail identifiers can never become native Mail IDs.
#[derive(Serialize, Deserialize)]
#[serde(tag = "view", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ToolTarget {
    #[serde(rename = "email")]
    LegacyMail {
        store: LegacyStore,
        account_id: LegacyAccount,
        message_id: Option<String>,
        draft_id: Option<String>,
    },
    #[serde(rename = "calendar")]
    Calendar {
        account_id: LegacyAccount,
        event_id: Option<String>,
        date_range: Option<DateRange>,
    },
    #[serde(rename = "github")]
    GitHub { repo: Option<String> },
    #[serde(rename = "notion")]
    Notion {
        provider: NotionProvider,
        page_id: String,
        parent_id: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
pub enum LegacyStore {
    #[serde(rename = "legacy")]
    Legacy,
}
#[derive(Serialize, Deserialize)]
pub enum LegacyAccount {
    #[serde(rename = "legacy-google")]
    Google,
}
#[derive(Serialize, Deserialize)]
pub enum NotionProvider {
    #[serde(rename = "notion")]
    Notion,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DateRange {
    from: String,
    to: String,
}

/// Called only after the integration's Result returned Ok. No model prose is parsed.
pub fn tool_result(name: &str, target: Value, result: Value) {
    let Ok(target) = serde_json::from_value::<ToolTarget>(target) else {
        return;
    };
    let _ = TOOL_RESULT_AVAILABLE.try_with(|available| available.set(true));
    let _ = CONTEXT.try_with(|c| {
        c.app.emit(
            "chat-tool-result",
            json!({
                "tool_name": name, "target": target, "result": result,
                "tool_call_id": TOOL_CALL_ID.try_with(Clone::clone).ok()
            }),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chat_event_scope_preserves_tokens_and_fallback_reset() {
        for payload in [
            json!({"token":"hello","done":false}),
            json!({"reset":true}),
            json!({"state":"idle"}),
        ] {
            let scoped = scoped_payload(payload.clone(), Some(("task-a", "run-a")));
            assert_eq!(scoped["session_id"], "task-a");
            assert_eq!(scoped["run_id"], "run-a");
            for (k, v) in payload.as_object().unwrap() {
                assert_eq!(&scoped[k], v);
            }
            assert_eq!(scoped_payload(payload.clone(), None), payload);
        }
    }
    #[test]
    fn result_routes_reject_native_mail_and_arbitrary_urls() {
        assert!(serde_json::from_value::<ToolTarget>(
            json!({"view":"email","store":"legacy","accountId":"legacy-google","messageId":"abc"})
        )
        .is_ok());
        assert!(serde_json::from_value::<ToolTarget>(
            json!({"view":"email","store":"native","accountId":1,"messageId":1})
        )
        .is_err());
        assert!(
            serde_json::from_value::<ToolTarget>(json!({"view":"https://example.com"})).is_err()
        );
        assert!(serde_json::from_value::<ToolTarget>(
            json!({"view":"github","url":"https://example.com"})
        )
        .is_err());
    }
    #[tokio::test]
    async fn foreground_requests_cannot_overlap() {
        let gate = &FOREGROUND;
        let first = gate.try_lock().unwrap();
        assert!(gate.try_lock().is_err());
        drop(first);
        assert!(gate.try_lock().is_ok());
    }

    #[test]
    fn ids_reject_empty_and_oversized_values() {
        assert_eq!(identity(None, "legacy").unwrap(), "legacy");
        assert!(identity(Some("".into()), "legacy").is_err());
        assert!(identity(Some("a".repeat(129)), "legacy").is_err());
        assert!(identity(Some("task-123".into()), "legacy").is_ok());
    }
}

tokio::task_local! { pub static TOOL_CALL_ID: String; }

tokio::task_local! { pub static TOOL_RESULT_AVAILABLE: std::cell::Cell<bool>; }
pub fn tool_lifecycle(name: &str, state: &str) {
    let _ = CONTEXT.try_with(|c| {
        c.app.emit("chat-tool-lifecycle", json!({
        "tool_name":name,"tool_call_id":TOOL_CALL_ID.try_with(Clone::clone).ok(),
        "state":state,"result_available":TOOL_RESULT_AVAILABLE.try_with(|v|v.get()).unwrap_or(false)
    }))
    });
}

// Preview requests may use providers but must never execute model tools.
tokio::task_local! { pub static TOOL_EXECUTION_DISABLED: bool; }
