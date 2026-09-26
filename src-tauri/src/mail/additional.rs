//! Additional Google accounts: the user's second and third Google accounts,
//! connected as Mail mailboxes. Their mail syncs to the Mail page
//! (mail::sync), their calendar to the Calendar page (here), and the assistant
//! can read both. Everything here is read-only: creating, changing and
//! deleting events stays on the main account (Settings > Google).

use super::{models::MailAccount, store, sync};
use crate::db::Database;
use crate::integrations::{calendar, gmail};

/// Read-only access to a mailbox's events; nothing here can change a calendar.
pub const CALENDAR_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.readonly";

/// user_preferences key for the main account's address, so a mailbox on the
/// same Google account is not read a second time as an additional one.
pub const MAIN_EMAIL_KEY: &str = "google_account_email";

pub fn main_email(db: &Database) -> Option<String> {
    let conn = db.conn.lock().ok()?;
    conn.query_row(
        "SELECT value FROM user_preferences WHERE key = ?1",
        [MAIN_EMAIL_KEY],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .filter(|email| !email.is_empty())
}

/// Learn the main account's address once, with its access token. Accounts
/// connected before this was recorded have no row until then.
pub async fn remember_main_email(db: &Database, main_token: &str) {
    if main_email(db).is_some() {
        return;
    }
    match gmail::profile_email(main_token).await {
        Ok(email) => {
            if let Ok(conn) = db.conn.lock() {
                let _ = conn.execute(
                    "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
                     ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
                    rusqlite::params![MAIN_EMAIL_KEY, email],
                );
            }
        }
        Err(e) => log::warn!("[google] main account address not read: {}", e),
    }
}

/// Connected mailboxes other than the main account.
pub fn accounts(db: &Database) -> Result<Vec<MailAccount>, String> {
    Ok(others(store::list_accounts(db)?, main_email(db).as_deref()))
}

fn others(accounts: Vec<MailAccount>, main: Option<&str>) -> Vec<MailAccount> {
    accounts
        .into_iter()
        .filter(|a| a.connected)
        .filter(|a| main.is_none_or(|m| !m.eq_ignore_ascii_case(&a.email)))
        .collect()
}

/// Sync each additional calendar for the window; returns the number of
/// events and one note per account that failed. Skipped while the Mail lock
/// is held (a sign-in waiting in the browser, say), like the background mail
/// sync, so a mailbox removed meanwhile cannot get its events back.
pub async fn sync_calendars(db: &Database, time_min: &str, time_max: &str) -> Result<(usize, Vec<String>), String> {
    let Some(_operation) = sync::try_operation() else {
        return Ok((0, vec!["其他帳戶的日曆會在郵件操作完成後同步。".to_string()]));
    };
    let mut count = 0;
    let mut problems = Vec::new();
    for account in accounts(db)?.into_iter().filter(|a| a.calendar) {
        let fetched = async {
            let (_, token) = sync::mailbox_session(db, &account.id).await?;
            calendar::fetch_events_detailed(&token, time_min, time_max).await
        }
        .await;
        match fetched {
            Ok(fetched) => {
                count += fetched.events.len();
                calendar::save_to_db(db, &fetched, time_min, time_max, &account.id)?;
            }
            Err(e) => problems.push(format!("{}：{}", account.email, e)),
        }
    }
    Ok((count, problems))
}

/// Events on every additional calendar in the window, one line each, for the
/// assistant's list_events. Read live, like the main account's.
pub async fn event_lines(db: &Database, time_min: &str, time_max: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for account in accounts(db).unwrap_or_default().into_iter().filter(|a| a.calendar) {
        let events = async {
            let (_, token) = sync::mailbox_session(db, &account.id).await?;
            calendar::fetch_events(&token, time_min, time_max).await
        }
        .await;
        match events {
            Ok(events) => lines.extend(events.iter().map(|e| {
                format!(
                    "Account: {} | ID: {} | {} | {} - {} | Location: {}",
                    account.email,
                    e.id,
                    e.summary,
                    e.start_time,
                    e.end_time,
                    e.location.as_deref().unwrap_or("none"),
                )
            })),
            Err(e) => lines.push(format!("Account: {} | Calendar unavailable: {}", account.email, e)),
        }
    }
    lines
}

/// Gmail search across every additional mailbox, one line per message.
pub async fn search_lines(db: &Database, query: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for account in accounts(db).unwrap_or_default() {
        let found = async {
            let (_, token) = sync::mailbox_session(db, &account.id).await?;
            gmail::search_messages(&token, query).await
        }
        .await;
        match found {
            Ok(messages) => lines.extend(messages.iter().map(|m| {
                format!(
                    "Account: {} | ID: {} | From: {} | Subject: {} | Date: {}",
                    account.email,
                    m.id,
                    m.sender.as_deref().unwrap_or("unknown"),
                    m.subject.as_deref().unwrap_or("(no subject)"),
                    m.received_at.as_deref().unwrap_or("unknown"),
                )
            })),
            Err(e) => lines.push(format!("Account: {} | Search unavailable: {}", account.email, e)),
        }
    }
    lines
}

/// One message's full text from the additional mailbox at `email`, or None
/// when `email` is not one (it is the main account's, or unknown).
pub async fn read_message(db: &Database, email: &str, message_id: &str) -> Option<Result<String, String>> {
    let account = accounts(db)
        .ok()?
        .into_iter()
        .find(|a| a.email.eq_ignore_ascii_case(email.trim()))?;
    Some(
        async {
            let (_, token) = sync::mailbox_session(db, &account.id).await?;
            gmail::get_message_full(&token, message_id).await.map(|(_, text)| text)
        }
        .await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(email: &str, connected: bool) -> MailAccount {
        MailAccount {
            id: email.into(),
            email: email.into(),
            connected,
            last_synced: None,
            next_page_token: None,
            message_count: 0,
            calendar: true,
        }
    }

    /// The main account's mail and calendar are already read through the
    /// main connection; its mailbox must not be read a second time.
    #[test]
    fn the_main_account_and_disconnected_mailboxes_are_not_additional() {
        let all = vec![
            account("Me@Example.com", true),
            account("work@example.com", true),
            account("old@example.com", false),
        ];
        let emails: Vec<String> = others(all.clone(), Some("me@example.com")).into_iter().map(|a| a.email).collect();
        assert_eq!(emails, vec!["work@example.com"]);
        assert_eq!(others(all, None).len(), 2, "unknown main address: every connected mailbox");
    }

    #[test]
    fn the_main_address_is_read_back_once_stored() {
        let db = crate::db::test_database();
        assert_eq!(main_email(&db), None);
        db.conn.lock().unwrap().execute(
            "INSERT INTO user_preferences (key, value) VALUES (?1, 'me@example.com')",
            [MAIN_EMAIL_KEY],
        ).unwrap();
        assert_eq!(main_email(&db).as_deref(), Some("me@example.com"));
    }
}
