//! The user's click before JARVIS changes something on their behalf.
//!
//! Email text reaches the model in every conversation (subjects in the
//! background context, bodies through the email tools), so a crafted email can
//! ask it to archive mail, delete events or plant a memory. Tools that change
//! the user's data therefore wait for the user to allow that one action. No
//! answer within two minutes, no window, or no app at all (tests, scheduled
//! jobs) means the action does not happen.

use serde_json::json;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use tokio::sync::oneshot;

/// Tools that change the user's data and so need a click first.
pub const NEEDS_APPROVAL: &[&str] = &["remember", "archive_email", "delete_event", "update_follow_up"];

const WAIT: Duration = Duration::from_secs(120);

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Called once at startup so requests can reach the window from any caller,
/// including voice commands, which run outside a chat session.
pub fn init(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

#[cfg(test)]
tokio::task_local! {
    /// Tests answer for the user inside one task, without a window.
    pub static TEST_ANSWER: bool;
}

fn pending() -> MutexGuard<'static, HashMap<String, oneshot::Sender<bool>>> {
    static PENDING: OnceLock<Mutex<HashMap<String, oneshot::Sender<bool>>>> = OnceLock::new();
    PENDING
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Ask the user to allow one action. True only on an explicit Allow.
pub async fn confirm(tool: &str, detail: String, status: Option<&str>) -> bool {
    #[cfg(test)]
    if let Ok(answer) = TEST_ANSWER.try_with(|answer| *answer) {
        return answer;
    }
    let Some(app) = APP.get() else { return false };
    let id = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = oneshot::channel();
    pending().insert(id.clone(), sender);
    let request = json!({
        "approval_id": id,
        "tool_name": tool,
        "detail": detail,
        "status": status,
        "session_id": crate::chat_session::CONTEXT.try_with(|c| c.session_id.clone()).ok(),
    });
    if tauri::Emitter::emit(app, "tool-approval-request", request).is_err() {
        pending().remove(&id);
        return false;
    }
    let approved = matches!(tokio::time::timeout(WAIT, receiver).await, Ok(Ok(true)));
    if pending().remove(&id).is_some() {
        // Timed out: take the request off the screen.
        let _ = tauri::Emitter::emit(app, "tool-approval-closed", json!({ "approval_id": id }));
    }
    approved
}

/// The user's answer from the approval card.
#[tauri::command]
pub fn resolve_tool_approval(approval_id: String, approved: bool) -> Result<(), String> {
    let sender = pending()
        .remove(&approval_id)
        .ok_or("This request has expired. Ask JARVIS again.")?;
    let _ = sender.send(approved);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn without_a_window_nothing_is_approved() {
        // No app handle in unit tests, as for a scheduled job with no window.
        assert!(!confirm("archive_email", "a message".into(), None).await);
    }

    #[test]
    fn an_answer_reaches_the_waiting_request_once() {
        let (sender, mut receiver) = oneshot::channel();
        pending().insert("req-1".into(), sender);
        resolve_tool_approval("req-1".into(), true).unwrap();
        assert_eq!(receiver.try_recv(), Ok(true));
        assert!(resolve_tool_approval("req-1".into(), true).is_err(), "a second answer is refused");
    }
}
