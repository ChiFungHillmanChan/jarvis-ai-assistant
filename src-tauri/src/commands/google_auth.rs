use crate::auth::google::{GoogleAuth, GMAIL_SCOPE};
use crate::db::Database;
use crate::integrations::gmail;
use crate::mail::{additional, models::MailAccount, store};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

/// Returned when a live Google session already exists and the caller did not
/// ask to replace it. Stable so the UI can offer SWITCH ACCOUNT on this exact
/// result instead of treating it as success.
pub const ALREADY_CONNECTED: &str = "Already connected to Google";
/// Signed in, but Calendar was left unticked on Google's consent screen.
pub const CALENDAR_NOT_ALLOWED: &str = "Connected without Calendar";

/// Returned by `google_connect_gmail` when the account chosen in the browser
/// is not the main account. Stable so the UI can explain it in the user's
/// language; mirrored in src/lib/google-gmail.ts.
pub const GMAIL_WRONG_ACCOUNT: &str = "Gmail must be connected on the main Google account";

/// Returned by `google_connect_gmail` when the consent screen came back
/// without Gmail. Mirrored in src/lib/google-gmail.ts.
pub const GMAIL_NOT_GRANTED: &str = "Google did not grant Gmail access";

/// Scopes requested at the first connect. No Gmail scope: Gmail is
/// restricted and needs a CASA assessment, while these let the app be
/// verified for the sensitive Calendar scope alone. Gmail is asked for later,
/// only when the user turns on the Assistant's mail tools (`gmail_scopes`).
///
/// userinfo.profile is not decoration: `store_profile` reads the userinfo
/// endpoint, and a token without this scope is rejected there. Without it
/// user_name is never written and `util::display_name` greets every user as
/// "there" for the life of the install.
///
/// userinfo.email (non-sensitive) gives that same call the account's address.
/// Before Gmail became optional the address came from Gmail's profile; it
/// decides whether disconnecting revokes a grant a Mail mailbox shares.
const REQUESTED_SCOPES: &[&str] = &[
    // Events on the primary calendar are all JARVIS reads or writes.
    "https://www.googleapis.com/auth/calendar.events",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/userinfo.email",
];

/// Scopes for turning Gmail on: the first connect's, plus Gmail. Requested
/// together with include_granted_scopes, so the new token keeps Calendar.
fn gmail_scopes() -> Vec<String> {
    REQUESTED_SCOPES
        .iter()
        .chain([&GMAIL_SCOPE])
        .map(|s| s.to_string())
        .collect()
}

/// What `google_connect` asks for. A first connect, or a switch to another
/// account, never asks for Gmail. Repairing a grant Google stopped honouring
/// (weekly while the app is in Testing) asks again for what that grant had,
/// so a reconnect does not turn the user's mail tools off.
fn connect_scopes(had_gmail: bool) -> Vec<String> {
    if had_gmail {
        gmail_scopes()
    } else {
        REQUESTED_SCOPES.iter().map(|s| s.to_string()).collect()
    }
}

/// What `google_connect` should do for a given (linked, force) pair.
#[derive(Debug, PartialEq, Eq)]
enum ConnectPlan {
    /// A live session exists and the caller did not ask to replace it.
    Reuse,
    /// An account is linked and the caller asked to switch: unlink first, then
    /// run the OAuth flow. Without this branch the command dead-ended -- a user
    /// who linked the wrong Google account got "Already connected" forever and
    /// had no reachable way to change it.
    Relink,
    /// Nothing usable is linked: run the OAuth flow.
    Fresh,
}

fn connect_plan(linked: bool, force: bool) -> ConnectPlan {
    match (linked, force) {
        (true, true) => ConnectPlan::Relink,
        (true, false) => ConnectPlan::Reuse,
        (false, _) => ConnectPlan::Fresh,
    }
}

/// Connect a Google account. Pass `force: true` to switch accounts: the current
/// grant is revoked and the local mirror dropped before the OAuth flow re-runs.
#[tauri::command]
pub async fn google_connect(
    force: Option<bool>,
    auth: State<'_, Arc<GoogleAuth>>,
    db: State<'_, Arc<Database>>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let mut plan = connect_plan(auth.is_linked(), force.unwrap_or(false));
    if plan == ConnectPlan::Reuse
        && (!auth.calendar_granted() || auth.ensure_access_token().await.is_err())
    {
        // Linked, but the stored grant no longer yields a token (revoked at
        // Google, refresh token expired), or Calendar was left unticked on
        // Google's consent screen. Re-running the flow is the repair.
        plan = ConnectPlan::Fresh;
    }
    match plan {
        ConnectPlan::Reuse => return Ok(ALREADY_CONNECTED.to_string()),
        ConnectPlan::Relink => unlink(&auth, &db, &app).await?,
        ConnectPlan::Fresh => {}
    }

    // After Relink the old grant's scopes are gone; after Fresh from a dead
    // grant they are still what the user had.
    auth.start_auth_flow(connect_scopes(auth.gmail_granted())).await?;
    // Google's last page says JARVIS is back in front; make it so.
    crate::payments::return_link::bring_forward(&app);
    // A Keychain write that fails silently leaves the user looking connected
    // now and signed out on the next launch. Say so instead.
    auth.save_to_db(&db).map_err(|e| {
        format!(
            "Signed in to Google, but the credential could not be saved to your Keychain ({}). \
             You will have to sign in again next launch.",
            e
        )
    })?;
    // Learn who the user is, so greetings and the system prompt address them
    // rather than whoever built the app. Best-effort: a failure here must not
    // fail an otherwise successful connection.
    if let Ok(token) = auth.ensure_access_token().await {
        // This may be a different account from the last one; learn it afresh.
        if let Ok(conn) = db.conn.lock() {
            let _ = conn.execute("DELETE FROM user_preferences WHERE key = ?1", [additional::MAIN_EMAIL_KEY]);
        }
        if let Err(e) = store_profile(&token, &db).await {
            log::warn!("Could not read Google profile name: {}", e);
        }
        // Gmail's profile is the fallback for the address, and only with
        // Gmail granted: asking without it is a refused call on every connect.
        if auth.gmail_granted() {
            additional::remember_main_email(&db, &token).await;
        }
    }
    // Nothing used to sync on connect: the first mail and events arrived with
    // the scheduler's five-minute tick, and until then the injected [CONTEXT]
    // block reported the empty mirror. Pull one of each now. Best-effort and
    // off the connect path -- the account is linked either way, and the two run
    // in sequence so they cannot race each other refreshing the same token.
    let sync_auth = auth.inner().clone();
    let sync_db = db.inner().clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::scheduler::jobs::run_email_sync(&sync_db, &sync_auth).await {
            log::warn!("First email sync after Google connect failed: {}", e);
        }
        if let Err(e) = crate::scheduler::jobs::run_calendar_sync(&sync_db, &sync_auth).await {
            log::warn!("First calendar sync after Google connect failed: {}", e);
        }
    });
    if !auth.calendar_granted() {
        return Ok(CALENDAR_NOT_ALLOWED.to_string());
    }
    Ok("Connected to Google".to_string())
}

/// Stop waiting for a browser sign-in the user abandoned, such as a closed
/// tab. `google_connect` then returns `SIGN_IN_CANCELLED`.
#[tauri::command]
pub fn google_cancel_connect() {
    crate::auth::google::cancel_sign_in(crate::auth::google::SignIn::Assistant);
}

/// Turn on the Assistant's mail tools for the main account: ask Google for
/// Gmail on top of the grant the user already gave. The sign-in runs on a
/// separate session and is adopted only when it is the main account and Gmail
/// was granted, so a wrong click in the browser changes nothing.
#[tauri::command]
pub async fn google_connect_gmail(
    auth: State<'_, Arc<GoogleAuth>>,
    db: State<'_, Arc<Database>>,
) -> Result<String, String> {
    if !auth.is_linked() {
        return Err("Connect Google first, then connect Gmail.".to_string());
    }
    // The main account's address, recorded at connect or read again now. It
    // is required: without it a different account chosen in the browser
    // could not be told apart, and would silently become the main account.
    let main = match additional::main_email(&db) {
        Some(email) => Some(email),
        None => match auth.ensure_access_token().await {
            Ok(token) => userinfo(&token).await.ok().as_ref().and_then(profile_email),
            Err(_) => None,
        },
    }
    .ok_or("Could not confirm the main Google account. Check your connection and try again.")?;
    let upgrade = auth.fresh_session();
    upgrade.start_incremental_auth_flow(gmail_scopes(), &main).await?;
    let token = upgrade.ensure_access_token().await?;
    let email = match userinfo(&token).await.ok().as_ref().and_then(profile_email) {
        Some(email) => Some(email),
        // userinfo did not answer: Gmail's profile, if Gmail was granted.
        None if upgrade.gmail_granted() => gmail::profile_email(&token).await.ok(),
        None => None,
    }
    .ok_or("Could not confirm which Google account was chosen. Please try again.")?;
    if !main.eq_ignore_ascii_case(&email) {
        // JARVIS keeps no token for that account. Revoke what it just granted,
        // unless a Mail mailbox is on that account: Google revokes per app and
        // account, and that would sign the mailbox out.
        if !shares_grant(&connected_mailboxes(&db), Some(&email)) {
            let grant = upgrade.refresh_token.lock().unwrap().clone().unwrap_or(token);
            if let Err(e) = crate::auth::google::revoke_grant(&grant).await {
                log::warn!("{}", e);
            }
        }
        return Err(GMAIL_WRONG_ACCOUNT.to_string());
    }
    if !upgrade.gmail_granted() {
        return Err(GMAIL_NOT_GRANTED.to_string());
    }
    auth.adopt(&upgrade);
    auth.save_to_db(&db).map_err(|e| {
        format!(
            "Gmail is connected, but the credential could not be saved to your Keychain ({}). \
             You will have to connect again next launch.",
            e
        )
    })?;
    additional::remember_main_email(&db, &token).await;
    let sync_auth = auth.inner().clone();
    let sync_db = db.inner().clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::scheduler::jobs::run_email_sync(&sync_db, &sync_auth).await {
            log::warn!("First email sync after connecting Gmail failed: {}", e);
        }
    });
    Ok("Gmail connected".to_string())
}

async fn userinfo(access_token: &str) -> Result<serde_json::Value, String> {
    let resp = crate::util::http_client()
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("userinfo HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// The address in a userinfo response; present only with userinfo.email.
fn profile_email(body: &serde_json::Value) -> Option<String> {
    body.get("email")
        .and_then(|v| v.as_str())
        .map(|email| email.trim().to_lowercase())
        .filter(|email| !email.is_empty())
}

/// Fetch the signed-in user's profile and persist the given name as
/// `user_name` and, when granted, the address as the main account's.
async fn store_profile(access_token: &str, db: &Arc<Database>) -> Result<(), String> {
    save_profile(db, &userinfo(access_token).await?)
}

fn save_profile(db: &Database, body: &serde_json::Value) -> Result<(), String> {
    if let Some(email) = profile_email(body) {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
            rusqlite::params![additional::MAIN_EMAIL_KEY, email],
        )
        .map_err(|e| e.to_string())?;
    }
    let name = body
        .get("given_name")
        .or_else(|| body.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if name.is_empty() {
        return Err("userinfo returned no name".to_string());
    }
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES ('user_name', ?1, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
        rusqlite::params![name],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Revoke the grant at Google, remove the refresh token from the Keychain,
/// clear the in-memory session and drop the local mirror of the account's mail
/// and calendar. Shared by `google_disconnect` and by the account switch in
/// `google_connect`, so switching cannot leave the previous account's mail on
/// disk.
async fn unlink(auth: &GoogleAuth, db: &Database, app: &tauri::AppHandle) -> Result<(), String> {
    let revoke = !mailbox_shares_grant(auth, db).await;
    auth.disconnect(db, revoke).await?;
    // `disconnect` removed today's stored brief; an Automation page that is
    // open is still showing it.
    crate::scheduler::scheduled_briefing::announce_removed(app);

    // The mail, events and briefings on disk are all derived from the account
    // we just unlinked. Leaving them would show a disconnected user their old
    // inbox, which is the opposite of what disconnect promises. Additional
    // accounts' events are theirs and stay.
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM emails", []).map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM calendar_events WHERE account_id = ?1",
            [crate::integrations::calendar::MAIN_ACCOUNT],
        )
        .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM daily_briefings", []).map_err(|e| e.to_string())?;
    }
    crate::commands::storage::reclaim_db(db);
    Ok(())
}

/// Whether a connected Mail mailbox is on the main Google account. Google
/// revokes per app and account, so revoking the main grant would sign that
/// mailbox out too (mail::sync::revoke_unless_shared is the other direction).
async fn mailbox_shares_grant(auth: &GoogleAuth, db: &Database) -> bool {
    let mailboxes = connected_mailboxes(db);
    if mailboxes.is_empty() {
        return false;
    }
    let main = match additional::main_email(db) {
        Some(email) => Some(email),
        // Gmail's profile only answers a grant that includes Gmail.
        None if auth.gmail_granted() => match auth.ensure_access_token().await {
            Ok(token) => gmail::profile_email(&token).await.ok(),
            Err(_) => None,
        },
        None => None,
    };
    shares_grant(&mailboxes, main.as_deref())
}

fn connected_mailboxes(db: &Database) -> Vec<String> {
    store::list_accounts(db)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.connected)
        .map(|a| a.email)
        .collect()
}

/// Unconfirmed (offline, say) counts as shared: keeping a grant JARVIS no
/// longer holds is harmless; revoking a mailbox's by mistake is not.
fn shares_grant(mailboxes: &[String], main: Option<&str>) -> bool {
    main.is_none_or(|main| mailboxes.iter().any(|m| m.eq_ignore_ascii_case(main)))
}

/// Unlink the Google account. Until this existed there was no way to
/// disconnect, or to switch accounts, from inside the app at all.
#[tauri::command]
pub async fn google_disconnect(
    auth: State<'_, Arc<GoogleAuth>>,
    db: State<'_, Arc<Database>>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    unlink(&auth, &db, &app).await?;
    log::info!("Disconnected from Google");
    Ok("Disconnected from Google".to_string())
}

/// "connected", "reconnect" (Google refused the grant; only signing in again
/// fixes it) or "not_connected". A cached access token used to count as
/// connected even after it expired, so a revoked grant still showed as
/// connected until the app restarted.
async fn main_state(auth: &GoogleAuth) -> &'static str {
    if !auth.is_linked() {
        return "not_connected";
    }
    match auth.ensure_access_token().await {
        Ok(_) => "connected",
        Err(_) if auth.needs_reconnect() => "reconnect",
        // Offline or Google unavailable: still linked.
        Err(e) => {
            log::warn!("Google token refresh failed during status check: {}", e);
            "connected"
        }
    }
}

#[tauri::command]
pub async fn google_status(auth: State<'_, Arc<GoogleAuth>>) -> Result<bool, String> {
    Ok(main_state(&auth).await == "connected")
}

#[derive(Serialize)]
pub struct MainGoogleAccount {
    state: &'static str,
    name: Option<String>,
    email: Option<String>,
    /// Whether the Assistant's mail tools are on; Settings offers Connect
    /// Gmail while they are off.
    gmail: bool,
    /// Whether Google granted Calendar; Settings asks to allow it when not.
    calendar: bool,
}

/// The main account and the additional ones, for Settings > Google.
#[derive(Serialize)]
pub struct GoogleAccounts {
    main: MainGoogleAccount,
    /// Every Mail mailbox except the main account's, disconnected ones too
    /// so they can be reconnected.
    others: Vec<MailAccount>,
}

#[tauri::command]
pub async fn google_accounts(
    auth: State<'_, Arc<GoogleAuth>>,
    db: State<'_, Arc<Database>>,
) -> Result<GoogleAccounts, String> {
    let state = main_state(&auth).await;
    if state == "connected" && auth.gmail_granted() {
        if let Ok(token) = auth.ensure_access_token().await {
            additional::remember_main_email(&db, &token).await;
        }
    }
    let email = if state == "not_connected" { None } else { additional::main_email(&db) };
    let name = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM user_preferences WHERE key = 'user_name'", [], |r| r.get::<_, String>(0))
            .ok()
            .filter(|name| !name.trim().is_empty())
    };
    let others = store::list_accounts(&db)?
        .into_iter()
        .filter(|a| email.as_deref().is_none_or(|main| !main.eq_ignore_ascii_case(&a.email)))
        .collect();
    Ok(GoogleAccounts {
        main: MainGoogleAccount {
            state,
            name: name.filter(|_| state != "not_connected"),
            email,
            gmail: state != "not_connected" && auth.gmail_granted(),
            calendar: state != "not_connected" && auth.calendar_granted(),
        },
        others,
    })
}

#[cfg(test)]
mod tests {
    use super::{connect_plan, connect_scopes, gmail_scopes, mailbox_shares_grant, save_profile, shares_grant, ConnectPlan, REQUESTED_SCOPES};
    use crate::auth::google::{GoogleAuth, GMAIL_SCOPE};

    /// Google OAuth verification, stage 1: the first connect is reviewed for
    /// the sensitive Calendar scope only. Any Gmail scope here is restricted
    /// and needs the CASA assessment, so none may appear.
    #[test]
    fn first_connect_asks_for_no_gmail_scope() {
        assert!(
            !REQUESTED_SCOPES.iter().any(|scope| scope.contains("gmail") || scope.contains("mail.google.com")),
            "first connect must not ask for Gmail: {:?}",
            REQUESTED_SCOPES
        );
        // Non-sensitive: the address that tells a Mail mailbox on this same
        // account apart from another account, now that Gmail's profile is not
        // available to every user.
        assert!(REQUESTED_SCOPES.contains(&"https://www.googleapis.com/auth/userinfo.email"));
    }

    /// Turning Gmail on asks for it on top of everything the first connect
    /// granted, so the new token still covers Calendar and the profile.
    #[test]
    fn connecting_gmail_keeps_the_first_connect_scopes() {
        let scopes = gmail_scopes();
        assert!(scopes.iter().any(|s| s == GMAIL_SCOPE));
        for base in REQUESTED_SCOPES {
            assert!(scopes.iter().any(|s| s == base), "{base} would be dropped");
        }
    }

    /// Without Gmail, the main address comes from the userinfo call. It must
    /// still decide revocation: a mailbox on another account does not hold
    /// the main grant back, a mailbox on the same account does.
    #[tokio::test]
    async fn the_userinfo_address_decides_whether_disconnect_revokes() {
        let db = crate::db::test_database();
        save_profile(&db, &serde_json::json!({"given_name": "Alex", "email": "Me@Example.com"})).unwrap();
        crate::mail::store::upsert_account(&db, "work", "work@example.com", true).unwrap();
        // Calendar only: nothing may fall back to Gmail's profile.
        let calendar_only = GoogleAuth::new_empty();
        assert!(!mailbox_shares_grant(&calendar_only, &db).await, "another account's mailbox: revoke");

        crate::mail::store::upsert_account(&db, "me", "me@example.com", true).unwrap();
        assert!(mailbox_shares_grant(&calendar_only, &db).await, "the main account's own mailbox: keep");
        assert_eq!(crate::util::display_name(&db), "Alex");
    }

    /// Disconnecting the Assistant's Google used to revoke the grant a Mail
    /// mailbox on the same account also depends on.
    #[test]
    fn the_main_grant_is_kept_while_a_mailbox_on_that_account_uses_it() {
        let mailboxes = vec!["Me@Example.com".to_string(), "work@example.com".to_string()];
        assert!(shares_grant(&mailboxes, Some("me@example.com")));
        assert!(!shares_grant(&mailboxes, Some("other@example.com")));
        assert!(shares_grant(&mailboxes, None), "unconfirmed: keep it");
    }

    #[test]
    fn profile_scope_is_requested() {
        // store_profile_name reads https://www.googleapis.com/oauth2/v2/userinfo.
        // A token carrying only gmail.modify and calendar is rejected there, so
        // user_name was never written and display_name greeted every user as
        // "there" for the life of the install.
        assert!(
            REQUESTED_SCOPES.contains(&"https://www.googleapis.com/auth/userinfo.profile"),
            "the userinfo call needs its own scope: {:?}",
            REQUESTED_SCOPES
        );
    }

    #[test]
    fn calendar_access_is_limited_to_events() {
        // Google's review asks for the narrowest scope; JARVIS only touches
        // events on the primary calendar.
        assert!(REQUESTED_SCOPES.contains(&"https://www.googleapis.com/auth/calendar.events"));
        assert!(!REQUESTED_SCOPES.contains(&"https://www.googleapis.com/auth/calendar"));
    }

    /// Only a reconnect of a grant that already had Gmail asks for it again.
    /// A first connect (nothing granted) and a switch (unlink clears the old
    /// grant's scopes first) both ask for Calendar only.
    #[tokio::test]
    async fn only_a_reconnect_of_a_gmail_grant_asks_for_gmail_again() {
        let has_gmail = |scopes: &[String]| scopes.iter().any(|s| s.contains("gmail"));
        assert!(!has_gmail(&connect_scopes(GoogleAuth::new_empty().gmail_granted())), "first connect");
        assert_eq!(connect_scopes(true), gmail_scopes(), "reconnect after Gmail was on");

        let db = crate::db::test_database();
        let switching = GoogleAuth::signed_in_for_tests(&[GMAIL_SCOPE]);
        switching.disconnect(&db, false).await.unwrap();
        assert!(!has_gmail(&connect_scopes(switching.gmail_granted())), "switch account");
    }

    #[test]
    fn force_relinks_rather_than_dead_ending() {
        // The original defect: someone who authorised the wrong Google account
        // hit "Already connected" on every retry and could not switch.
        assert_eq!(connect_plan(true, true), ConnectPlan::Relink);
    }

    #[test]
    fn plain_connect_reuses_a_live_session() {
        assert_eq!(connect_plan(true, false), ConnectPlan::Reuse);
    }

    #[test]
    fn nothing_linked_always_runs_the_flow() {
        assert_eq!(connect_plan(false, false), ConnectPlan::Fresh);
        assert_eq!(connect_plan(false, true), ConnectPlan::Fresh);
    }
}
