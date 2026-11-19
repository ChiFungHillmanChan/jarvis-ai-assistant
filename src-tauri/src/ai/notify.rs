//! Events a request deep in the AI layer raises for the window: the notice a
//! background job surfaces once (P05's disabled model, P06's "needs a model
//! that supports X"), and the consent card a blocked page shows (P06).

use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Called once at setup.
pub fn init(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

/// Emit to the window. Nothing happens before setup; the tests record what
/// was emitted (`emitted`).
/// An event whose name starts with "chat-" is scoped to the chat run in
/// progress, as every chat event is (chat_session::ScopedEmitter).
pub fn emit<S: Serialize + Clone>(event: &str, payload: S) {
    #[cfg(test)]
    EMITTED.with(|emitted| emitted.borrow_mut().push((event.to_string(), serde_json::to_value(&payload).unwrap_or_default())));
    if let Some(app) = APP.get() {
        use crate::chat_session::ScopedEmitter;
        let _ = app.emit(event, payload);
    }
}

#[cfg(test)]
thread_local! {
    /// What this test thread emitted, so a test can read a notice's text.
    static EMITTED: std::cell::RefCell<Vec<(String, serde_json::Value)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The events this test thread emitted, in order.
#[cfg(test)]
pub fn emitted() -> Vec<(String, serde_json::Value)> {
    EMITTED.with(|emitted| emitted.borrow().clone())
}

/// True the first time `key` is seen this launch, so a background job that
/// keeps failing for the same reason raises its notice once.
pub fn first_time(key: &str) -> bool {
    // Tests running side by side each see their own launch, so one test's
    // notice does not hide another's.
    #[cfg(test)]
    let key = &format!("{key}|{:?}", std::thread::current().id());
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(Default::default).lock().map(|mut seen| seen.insert(key.to_string())).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_notice_key_is_new_only_once_per_launch() {
        let key = format!("test-{}", uuid::Uuid::new_v4());
        assert!(super::first_time(&key));
        assert!(!super::first_time(&key));
    }
}
