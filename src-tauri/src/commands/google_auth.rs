use crate::auth::google::GoogleAuth;
use crate::db::Database;
use std::sync::Arc;
use tauri::State;

/// Returned when a live Google session already exists and the caller did not
/// ask to replace it. Stable so the UI can offer SWITCH ACCOUNT on this exact
/// result instead of treating it as success.
pub const ALREADY_CONNECTED: &str = "Already connected to Google";

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
) -> Result<String, String> {
    let mut plan = connect_plan(auth.is_linked(), force.unwrap_or(false));
    if plan == ConnectPlan::Reuse && auth.ensure_access_token().await.is_err() {
        // Linked, but the stored grant no longer yields a token (revoked at
        // Google, refresh token expired). Re-running the flow is the repair.
        plan = ConnectPlan::Fresh;
    }
    match plan {
        ConnectPlan::Reuse => return Ok(ALREADY_CONNECTED.to_string()),
        ConnectPlan::Relink => unlink(&auth, &db).await?,
        ConnectPlan::Fresh => {}
    }

    let scopes = vec![
        "https://www.googleapis.com/auth/gmail.readonly".to_string(),
        "https://www.googleapis.com/auth/gmail.modify".to_string(),
        "https://www.googleapis.com/auth/calendar".to_string(),
    ];
    auth.start_auth_flow(scopes).await?;
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
        if let Err(e) = store_profile_name(&token, &db).await {
            log::warn!("Could not read Google profile name: {}", e);
        }
    }
    Ok("Connected to Google".to_string())
}

/// Fetch the signed-in user's given name and persist it as `user_name`.
async fn store_profile_name(access_token: &str, db: &Arc<Database>) -> Result<(), String> {
    let resp = crate::util::http_client()
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("userinfo HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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
async fn unlink(auth: &GoogleAuth, db: &Database) -> Result<(), String> {
    auth.disconnect(db).await?;

    // The mail, events and briefings on disk are all derived from the account
    // we just unlinked. Leaving them would show a disconnected user their old
    // inbox, which is the opposite of what disconnect promises.
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM emails", []).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM calendar_events", []).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM daily_briefings", []).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Unlink the Google account. Until this existed there was no way to
/// disconnect, or to switch accounts, from inside the app at all.
#[tauri::command]
pub async fn google_disconnect(
    auth: State<'_, Arc<GoogleAuth>>,
    db: State<'_, Arc<Database>>,
) -> Result<String, String> {
    unlink(&auth, &db).await?;
    log::info!("Disconnected from Google");
    Ok("Disconnected from Google".to_string())
}

#[tauri::command]
pub async fn google_status(auth: State<'_, Arc<GoogleAuth>>) -> Result<bool, String> {
    if auth.is_authenticated() {
        return Ok(true);
    }
    if auth.has_refresh_token() {
        match auth.refresh_access_token().await {
            Ok(()) => return Ok(true),
            Err(e) => {
                log::warn!("Google token refresh failed during status check: {}", e);
                return Ok(false);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{connect_plan, ConnectPlan};

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
