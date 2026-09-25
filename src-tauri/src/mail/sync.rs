use futures_util::{stream, StreamExt};
use serde::{de::DeserializeOwned, Deserialize};

use super::{
    models::{MailAccount, MailSyncResult},
    store,
};
use crate::{
    auth::google::GoogleAuth, db::Database, integrations::gmail, security::keychain::KeychainStore,
};

const KEYCHAIN_SERVICE: &str = "com.jarvis.mail";
const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

pub struct MailSyncState {
    // One lock covers OAuth, sync and deletion: a late response must never
    // recreate an account or messages after the user removes it.
    operation: tokio::sync::Mutex<()>,
}

impl MailSyncState {
    pub fn new() -> Self {
        Self {
            operation: tokio::sync::Mutex::new(()),
        }
    }
}

fn auth_instance() -> Result<GoogleAuth, String> {
    #[cfg(feature = "native-qa")]
    return Err("原生 QA 停用 Google 登入；請用合成郵件測試。".into());
    #[cfg(not(feature = "native-qa"))]
    GoogleAuth::new().ok_or_else(|| "此版本尚未設定 Google 登入，請聯絡支援。".to_string())
}

fn persist_refresh(auth: &GoogleAuth, account_id: &str) -> Result<(), String> {
    let token = auth
        .refresh_token
        .lock()
        .map_err(|_| "暫時未能讀取登入狀態，請重試。".to_string())?
        .clone()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "Google 未提供持續登入授權，請重新連接信箱。".to_string())?;
    KeychainStore::put(KEYCHAIN_SERVICE, account_id, &token)
        .map_err(|_| "未能儲存信箱憑證，請解鎖 macOS 鑰匙圈後重試。".to_string())
}

/// Pages listed per click when catching up on new mail, and the most new
/// messages downloaded per click. A larger backlog continues on the next
/// click, because last_synced only moves once its whole window is stored.
const CATCH_UP_PAGES: usize = 200;
const CATCH_UP_LIMIT: usize = 200;
/// A window that has been retried this long (one message that always fails
/// to download) is accepted as done, with a warning, instead of growing and
/// being re-listed forever.
const GIVE_UP_WINDOW_SECS: i64 = 8 * 86_400;
/// Newest stored messages whose read and inbox state each sync refreshes.
const LABEL_REFRESH: usize = 50;

async fn refresh(db: &Database, auth: &GoogleAuth, account_id: &str) -> Result<String, String> {
    let before = auth.refresh_token.lock().ok().and_then(|t| t.clone());
    auth.refresh_access_token().await.map_err(|error| {
        if error.contains("invalid_grant") {
            // Only a new consent fixes this. Say so, and show the mailbox as
            // disconnected instead of "connected" with a dead credential.
            let _ = store::set_connected(db, account_id, false);
            "Google 授權已過期或已被撤銷。請按「連接 Gmail 帳戶」重新連接此信箱；本機郵件會保留。".to_string()
        } else {
            "信箱登入更新失敗，請檢查網絡後重試。".to_string()
        }
    })?;
    // Google can rotate a refresh token; persist a new one before using its
    // access token. An unchanged token is not rewritten: that was a Keychain
    // write on every sync, each one a chance for a password prompt.
    let after = auth.refresh_token.lock().ok().and_then(|t| t.clone());
    if after != before {
        persist_refresh(auth, account_id)?;
    }
    auth.get_access_token()
        .ok_or_else(|| "信箱登入未完成，請重新連接。".to_string())
}

#[derive(Deserialize)]
struct Profile {
    #[serde(rename = "emailAddress")]
    email_address: String,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

#[derive(Deserialize)]
struct MessagePage {
    #[serde(default)]
    messages: Vec<MessageRef>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

async fn gmail_json<T: DeserializeOwned>(
    path: &str,
    token: &str,
    page: Option<&str>,
    query: Option<&str>,
) -> Result<T, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "未能建立網絡連線，請重試。".to_string())?;
    let mut request = client.get(format!("{GMAIL_API}/{path}")).bearer_auth(token);
    if path == "messages" {
        // No INBOX restriction: archived and sent mail belong in this vault too.
        request = request.query(&[("maxResults", "50")]);
        if let Some(page) = page {
            request = request.query(&[("pageToken", page)]);
        }
        if let Some(query) = query {
            request = request.query(&[("q", query)]);
        }
    }
    let response = request
        .send()
        .await
        .map_err(|_| "未能連接 Gmail，請檢查網絡後重試。".to_string())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("UNAUTHORIZED".to_string());
    }
    if !response.status().is_success() {
        return Err(format!(
            "暫時未能讀取 Gmail（{}），請稍後重試；若持續失敗，請重新連接信箱。",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "未能解析 Gmail 回覆格式，請稍後重試。".to_string())
}

pub async fn connect(db: &Database, state: &MailSyncState) -> Result<MailAccount, String> {
    let _operation = state.operation.lock().await;
    let auth = auth_instance()?;
    auth.start_auth_flow_with_account_selection(vec![
        "https://www.googleapis.com/auth/gmail.readonly".to_string(),
    ])
    .await
    .map_err(|_| "Google 登入未完成或已取消，請重試。".to_string())?;
    let token = auth
        .ensure_access_token()
        .await
        .map_err(|_| "Google 登入失敗，請重試。".to_string())?;
    let profile: Profile = gmail_json("profile", &token, None, None)
        .await
        .map_err(|_| "未能讀取 Gmail 帳戶資料，請重新連接信箱。".to_string())?;
    let email = profile.email_address.trim().to_lowercase();
    if email.is_empty() {
        return Err("Google 未提供信箱地址，請重新連接。".to_string());
    }
    let previous = store::find_account_by_email(db, &email)?;
    let id = previous
        .as_ref()
        .map(|account| account.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    persist_refresh(&auth, &id)?;
    match store::upsert_account(db, &id, &email, true) {
        Ok(account) => Ok(account),
        Err(error) => {
            // A failed new DB insert must not strand a credential. Existing
            // account credentials are preserved if reconnect storage fails.
            if previous.is_none() {
                let _ = KeychainStore::delete(KEYCHAIN_SERVICE, &id);
            }
            Err(error)
        }
    }
}

fn disconnect_locked(db: &Database, account_id: &str) -> Result<(), String> {
    store::get_account(db, account_id)?;
    // Local disconnect never revokes Google's shared client grant: revocation
    // could also break the separate Calendar/chat integration.
    if let Err(error) = KeychainStore::delete(KEYCHAIN_SERVICE, account_id) {
        if !KeychainStore::is_not_found(&error) {
            return Err("未能刪除信箱憑證，請解鎖 macOS 鑰匙圈後重試。".to_string());
        }
    }
    store::set_connected(db, account_id, false)
}

pub async fn disconnect(
    db: &Database,
    state: &MailSyncState,
    account_id: &str,
) -> Result<(), String> {
    let _operation = state.operation.lock().await;
    disconnect_locked(db, account_id)
}

pub async fn remove(db: &Database, state: &MailSyncState, account_id: &str) -> Result<(), String> {
    let _operation = state.operation.lock().await;
    disconnect_locked(db, account_id)?;
    store::delete_account(db, account_id)
}

pub async fn sync_account(
    db: &Database,
    state: &MailSyncState,
    account_id: &str,
    older: bool,
) -> Result<MailSyncResult, String> {
    sync_account_inner(db, state, account_id, older, false).await
}

/// Restart traversal from the newest page without deleting any local data.
/// This can recover the gap when more than one page arrived since last sync.
pub async fn restart_sync(
    db: &Database,
    state: &MailSyncState,
    account_id: &str,
) -> Result<MailSyncResult, String> {
    sync_account_inner(db, state, account_id, false, true).await
}

async fn sync_account_inner(
    db: &Database,
    state: &MailSyncState,
    account_id: &str,
    older: bool,
    restart: bool,
) -> Result<MailSyncResult, String> {
    let _operation = state.operation.lock().await;
    let account = store::get_account(db, account_id)?;
    if !account.connected {
        return Err("此信箱已斷開，請先重新連接。".to_string());
    }
    if older && account.last_synced.is_some() && account.next_page_token.is_none() {
        return Ok(MailSyncResult {
            saved: 0,
            has_more: false,
            warning: None,
        });
    }
    let auth = auth_instance()?;
    let secret = KeychainStore::get(KEYCHAIN_SERVICE, account_id)
        .map_err(|_| "未能讀取信箱憑證，請解鎖 macOS 鑰匙圈或重新連接。".to_string())?;
    *auth
        .refresh_token
        .lock()
        .map_err(|_| "暫時未能讀取登入狀態，請重試。".to_string())? = Some(secret);
    let mut token = refresh(db, &auth, account_id).await?;
    if !older && !restart {
        if let Some(after) = account.last_synced.as_deref().and_then(window_start) {
            return catch_up(db, &auth, account_id, token, after).await;
        }
    }
    let page_token = if older {
        account.next_page_token.as_deref()
    } else {
        None
    };
    let page = list_page(db, &auth, account_id, &mut token, page_token, None).await?;
    let mut ids: Vec<String> = page.messages.into_iter().map(|m| m.id).collect();
    if older {
        // Older pages are rarely stored already; skipping any that are saves
        // a full download each. A rescan still re-reads everything.
        let known = store::known_provider_ids(db, account_id, &ids)?;
        ids.retain(|id| !known.contains(id));
    }
    let (rows, failures) = download(db, &auth, account_id, &token, ids).await;
    let saved = store::save_messages(db, account_id, &rows)?;
    let advance = advance_cursor(older, account.next_page_token.is_some(), restart, failures);
    // Partial pages must not initialize pagination or move past missing rows.
    if failures == 0 {
        // Only the very first sync sets last_synced here; "load older" and
        // "rescan" must not move it, or the next normal sync skips new mail.
        store::finish_sync(db, account_id, page.next_page_token.as_deref(), advance, account.last_synced.is_none())?;
    }
    let current = store::get_account(db, account_id)?;
    Ok(MailSyncResult {
        saved,
        has_more: current.next_page_token.is_some(),
        warning: (failures > 0).then(|| format!("有 {failures} 封郵件未能下載，成功下載的 {saved} 封已儲存。請重試相同的同步操作；若持續失敗，請重新連接信箱。")),
    })
}

/// The start of the next catch-up window: one day before the last complete
/// sync (SQLite UTC "YYYY-MM-DD HH:MM:SS"), as Unix seconds for Gmail's
/// `after:`. The overlap absorbs clock skew and late-delivered mail.
fn window_start(last_synced: &str) -> Option<i64> {
    chrono::NaiveDateTime::parse_from_str(last_synced, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|at| at.and_utc().timestamp() - 86_400)
}

async fn list_page(
    db: &Database,
    auth: &GoogleAuth,
    account_id: &str,
    token: &mut String,
    page: Option<&str>,
    query: Option<&str>,
) -> Result<MessagePage, String> {
    match gmail_json::<MessagePage>("messages", token, page, query).await {
        Err(error) if error == "UNAUTHORIZED" => {
            *token = refresh(db, auth, account_id).await?;
            gmail_json::<MessagePage>("messages", token, page, query)
                .await
                .map_err(|error| {
                    if error == "UNAUTHORIZED" {
                        "信箱授權已失效，請重新連接。".to_string()
                    } else {
                        error
                    }
                })
        }
        result => result,
    }
}

/// Download full messages, retrying only the ones that failed with 401 after
/// one refresh, so a mid-page token expiry keeps every body already read.
async fn download(
    db: &Database,
    auth: &GoogleAuth,
    account_id: &str,
    token: &str,
    ids: Vec<String>,
) -> (Vec<(gmail::GmailMessage, String)>, usize) {
    let results = stream::iter(ids.into_iter().map(|id| {
        let token = token.to_string();
        async move {
            let result = gmail::get_message_full(&token, &id).await;
            (id, result)
        }
    }))
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await;
    let mut rows = Vec::new();
    let mut unauthorized = Vec::new();
    let mut failures = 0;
    for (id, result) in results {
        match result {
            Ok(row) => rows.push(row),
            Err(error) if error == "UNAUTHORIZED" => unauthorized.push(id),
            Err(_) => failures += 1,
        }
    }
    if !unauthorized.is_empty() {
        match refresh(db, auth, account_id).await {
            Ok(refreshed) => {
                let retries = stream::iter(unauthorized.into_iter().map(|id| {
                    let token = refreshed.clone();
                    async move { gmail::get_message_full(&token, &id).await }
                }))
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;
                for result in retries {
                    match result {
                        Ok(row) => rows.push(row),
                        Err(_) => failures += 1,
                    }
                }
            }
            Err(_) => failures += unauthorized.len(),
        }
    }
    (rows, failures)
}

#[derive(Deserialize)]
struct MinimalMessage {
    #[serde(rename = "labelIds", default)]
    label_ids: Vec<String>,
}

/// Mail read or archived in Gmail should look the same here. A catch-up only
/// downloads new messages, so re-read the labels of the newest stored ones
/// (format=minimal, a few hundred bytes each). Best effort: failures are
/// ignored and retried on the next sync.
async fn refresh_labels(db: &Database, account_id: &str, token: &str, ids: Vec<String>) {
    let results = stream::iter(ids.into_iter().map(|id| async move {
        let found = gmail_json::<MinimalMessage>(&format!("messages/{id}?format=minimal"), token, None, None).await;
        (id, found)
    }))
    .buffer_unordered(8)
    .collect::<Vec<_>>()
    .await;
    for (id, found) in results {
        if let Ok(message) = found {
            let is_read = !message.label_ids.iter().any(|l| l == "UNREAD");
            let is_inbox = message.label_ids.iter().any(|l| l == "INBOX");
            let _ = store::update_labels(db, account_id, &id, is_read, is_inbox);
        }
    }
}

/// Normal sync once an account has history: list every message since the
/// last complete sync (not just the newest page of 50, which silently skipped
/// anything beyond it) and download only the ones not stored yet.
async fn catch_up(
    db: &Database,
    auth: &GoogleAuth,
    account_id: &str,
    mut token: String,
    after: i64,
) -> Result<MailSyncResult, String> {
    let query = format!("after:{after}");
    let mut page_token: Option<String> = None;
    let mut new_ids = Vec::new();
    let mut stored_recent = Vec::new();
    let mut complete = false;
    for _ in 0..CATCH_UP_PAGES {
        let page = list_page(db, auth, account_id, &mut token, page_token.as_deref(), Some(&query)).await?;
        let ids: Vec<String> = page.messages.into_iter().map(|m| m.id).collect();
        let known = store::known_provider_ids(db, account_id, &ids)?;
        for id in ids {
            if !known.contains(&id) {
                new_ids.push(id);
            } else if stored_recent.len() < LABEL_REFRESH {
                stored_recent.push(id);
            }
        }
        page_token = page.next_page_token;
        if page_token.is_none() {
            complete = true;
            break;
        }
        if new_ids.len() >= CATCH_UP_LIMIT {
            break;
        }
    }
    if new_ids.len() > CATCH_UP_LIMIT {
        new_ids.truncate(CATCH_UP_LIMIT);
        complete = false;
    }
    let (rows, failures) = download(db, auth, account_id, &token, new_ids).await;
    let saved = store::save_messages(db, account_id, &rows)?;
    refresh_labels(db, account_id, &token, stored_recent).await;
    let give_up = chrono::Utc::now().timestamp() - after > GIVE_UP_WINDOW_SECS;
    let finished = complete && (failures == 0 || give_up);
    if finished {
        store::finish_sync(db, account_id, None, false, true)?;
    }
    let current = store::get_account(db, account_id)?;
    Ok(MailSyncResult {
        saved,
        has_more: current.next_page_token.is_some(),
        warning: if failures > 0 && finished {
            Some(format!("有 {failures} 封郵件多日未能下載，已略過；如需補回，請在信箱「管理」選擇「由最新郵件重新掃描」。"))
        } else if failures > 0 {
            Some(format!("有 {failures} 封郵件未能下載，成功下載的 {saved} 封已儲存。請再按「同步」重試。"))
        } else if !finished {
            Some("仍有較新的郵件未下載，請再按「同步」繼續。".to_string())
        } else {
            None
        },
    })
}

// Ordinary refreshes preserve active older-mail traversal; an exhausted
// traversal can start again, and an explicit restart replaces an active cursor.
fn advance_cursor(older: bool, has_cursor: bool, restart: bool, failures: usize) -> bool {
    failures == 0 && (older || restart || !has_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_accounts() -> (Database, String, String) {
        let db = crate::db::test_database();
        let a = uuid::Uuid::new_v4().to_string();
        let b = uuid::Uuid::new_v4().to_string();
        store::upsert_account(&db, &a, "first@example.com", true).unwrap();
        store::upsert_account(&db, &b, "second@example.com", true).unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO mail_messages(id,account_id,provider_id,body,received_at) VALUES
                (1,?1,'same-id','First account private body','2026-09-10'),
                (2,?2,'same-id','Second account private body','2026-09-10')",
                rusqlite::params![a, b],
            )
            .unwrap();
            conn.execute_batch(
                "INSERT INTO mail_groups(id,name) VALUES (1,'Shared group');
                INSERT INTO mail_group_members(group_id,message_id) VALUES (1,1),(1,2);
                INSERT INTO mail_memories(id,group_id,content,kind,source_message_id) VALUES
                (1,1,'First account sourced note','fact',1),
                (2,1,'Second account sourced note','fact',2),
                (3,1,'Independent user preference','preference',NULL);",
            )
            .unwrap();
        }
        // KeychainStore uses its memory backend in tests, never macOS secrets.
        KeychainStore::put(KEYCHAIN_SERVICE, &a, "first-test-refresh").unwrap();
        KeychainStore::put(KEYCHAIN_SERVICE, &b, "second-test-refresh").unwrap();
        (db, a, b)
    }

    #[tokio::test]
    async fn disconnect_keeps_local_mail_and_other_account_credentials() {
        let (db, a, b) = two_accounts();
        let state = MailSyncState::new();
        disconnect(&db, &state, &a).await.unwrap();
        assert!(!store::get_account(&db, &a).unwrap().connected);
        assert_eq!(store::get_account(&db, &a).unwrap().message_count, 1);
        assert!(KeychainStore::is_not_found(
            &KeychainStore::get(KEYCHAIN_SERVICE, &a).unwrap_err()
        ));
        assert!(store::get_account(&db, &b).unwrap().connected);
        assert_eq!(
            KeychainStore::get(KEYCHAIN_SERVICE, &b).unwrap(),
            "second-test-refresh"
        );
        let notes: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM mail_memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(notes, 3);
        // Repeated disconnect is valid even when its credential is already gone.
        disconnect(&db, &state, &a).await.unwrap();
        KeychainStore::delete(KEYCHAIN_SERVICE, &b).unwrap();
    }

    #[tokio::test]
    async fn remove_deletes_only_selected_mail_and_its_sourced_notes() {
        let (db, a, b) = two_accounts();
        remove(&db, &MailSyncState::new(), &a).await.unwrap();
        assert!(store::get_account(&db, &a).is_err());
        assert!(KeychainStore::is_not_found(
            &KeychainStore::get(KEYCHAIN_SERVICE, &a).unwrap_err()
        ));
        assert_eq!(store::get_account(&db, &b).unwrap().message_count, 1);
        assert_eq!(
            KeychainStore::get(KEYCHAIN_SERVICE, &b).unwrap(),
            "second-test-refresh"
        );
        let conn = db.conn.lock().unwrap();
        let body: String = conn
            .query_row("SELECT body FROM mail_messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(body, "Second account private body");
        let mut statement = conn
            .prepare("SELECT id FROM mail_memories ORDER BY id")
            .unwrap();
        let notes = statement
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(notes, vec![2, 3]);
        let memberships: i64 = conn
            .query_row(
                "SELECT count(*) FROM mail_group_members WHERE message_id=2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(memberships, 1);
        let groups: i64 = conn
            .query_row("SELECT count(*) FROM mail_groups", [], |row| row.get(0))
            .unwrap();
        assert_eq!(groups, 1);
        KeychainStore::delete(KEYCHAIN_SERVICE, &b).unwrap();
    }

    #[test]
    fn catch_up_window_starts_a_day_before_the_last_complete_sync() {
        assert_eq!(window_start("2026-09-25 18:01:23"), Some(1_790_359_283 - 86_400));
        assert_eq!(window_start("not a date"), None);
    }

    #[test]
    fn fresh_page_reopens_exhausted_older_traversal() {
        assert!(advance_cursor(false, false, false, 0));
    }

    #[test]
    fn explicit_restart_replaces_existing_cursor_only_after_complete_page() {
        assert!(advance_cursor(false, true, true, 0));
        assert!(!advance_cursor(false, true, true, 1));
    }

    #[test]
    fn partial_download_keeps_the_retry_page() {
        assert!(!advance_cursor(true, true, false, 1));
        assert!(!advance_cursor(false, false, false, 1));
    }

    #[test]
    fn recent_refresh_preserves_older_progress() {
        assert!(!advance_cursor(false, true, false, 0));
        assert!(advance_cursor(false, false, false, 0));
        assert!(advance_cursor(true, true, false, 0));
    }
}
