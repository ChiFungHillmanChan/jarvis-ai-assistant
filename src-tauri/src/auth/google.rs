use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken,
    PkceCodeChallenge, RedirectUrl, Scope, TokenUrl,
    AuthorizationCode, TokenResponse, RefreshToken, reqwest::async_http_client,
};
use std::sync::Mutex;
use tokio::sync::oneshot;

use crate::security::keychain::KeychainStore;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";

/// Google access tokens live about an hour. Treat one as spent slightly early
/// so a call that starts just before the boundary does not land just after it.
const TOKEN_EXPIRY_MARGIN: std::time::Duration = std::time::Duration::from_secs(60);

/// Assumed lifetime when a token response omits `expires_in`. Matches Google's
/// documented default; the margin above absorbs the difference either way.
const DEFAULT_TOKEN_LIFETIME: std::time::Duration = std::time::Duration::from_secs(3600);

/// How long the callback listener waits for a single connection to send its
/// request line before dropping it and going back to accepting. A peer that
/// connects and then says nothing must not consume the whole consent window.
const CALLBACK_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Total time the user has to finish consent in the browser.
const CALLBACK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(180);

// The Google refresh token is a long-lived credential and lives in the macOS
// Keychain, never in plaintext SQLite. The DB keeps only this non-secret marker
// so existence checks keep working.
const REFRESH_KEYCHAIN_SERVICE: &str = "com.jarvis.google";
const REFRESH_KEYCHAIN_ACCOUNT: &str = "refresh_token";
const REFRESH_KEYCHAIN_MARKER: &str = "__keychain__";

pub struct GoogleAuth {
    client_id: String,
    client_secret: String,
    pub access_token: Mutex<Option<String>>,
    /// When the cached access token stops being usable. Written together with
    /// the token itself -- without it every caller kept using an hour-old token
    /// and depended on a 401 to find out.
    access_token_expiry: Mutex<Option<std::time::Instant>>,
    pub refresh_token: Mutex<Option<String>>,
    /// Serialises refreshes so a burst of calls that all find the token expired
    /// produces one round trip to Google, not one per caller.
    refresh_lock: tokio::sync::Mutex<()>,
}

/// Resolve a credential by checking the runtime env var first, then falling back to
/// a value baked in at compile time via `option_env!`. Empty strings count as missing.
fn resolve_credential(env_key: &str, compile_time: Option<&'static str>) -> Option<String> {
    std::env::var(env_key)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| compile_time.map(str::to_string).filter(|v| !v.is_empty()))
}

impl GoogleAuth {
    /// Construct from credentials. Reads `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET`
    /// from the runtime env, falling back to values embedded at compile time. Returns
    /// `None` only if no client_id is available — packaged release builds inject these
    /// via the build environment so end users never need an .env file.
    pub fn new() -> Option<Self> {
        let client_id = resolve_credential(
            "GOOGLE_CLIENT_ID",
            option_env!("GOOGLE_CLIENT_ID"),
        )?;
        let client_secret = resolve_credential(
            "GOOGLE_CLIENT_SECRET",
            option_env!("GOOGLE_CLIENT_SECRET"),
        )
        .unwrap_or_default();
        Some(GoogleAuth {
            client_id,
            client_secret,
            access_token: Mutex::new(None),
            access_token_expiry: Mutex::new(None),
            refresh_token: Mutex::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn new_empty() -> Self {
        GoogleAuth {
            client_id: String::new(),
            client_secret: String::new(),
            access_token: Mutex::new(None),
            access_token_expiry: Mutex::new(None),
            refresh_token: Mutex::new(None),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn has_credentials(&self) -> bool {
        !self.client_id.is_empty()
    }

    fn build_client(&self, redirect_port: u16) -> Result<BasicClient, String> {
        if self.client_id.is_empty() {
            return Err(
                "Google sign-in is not configured for this build. Please contact support."
                    .to_string(),
            );
        }
        let secret = if self.client_secret.is_empty() {
            None
        } else {
            Some(ClientSecret::new(self.client_secret.clone()))
        };
        let client = BasicClient::new(
            ClientId::new(self.client_id.clone()),
            secret,
            AuthUrl::new(AUTH_URL.to_string()).map_err(|e| e.to_string())?,
            Some(TokenUrl::new(TOKEN_URL.to_string()).map_err(|e| e.to_string())?),
        )
        .set_redirect_uri(
            RedirectUrl::new(format!("http://127.0.0.1:{}", redirect_port))
                .map_err(|e| e.to_string())?,
        );
        Ok(client)
    }

    pub async fn start_auth_flow(&self, scopes: Vec<String>) -> Result<(), String> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await.map_err(|e| format!("Failed to bind listener: {}", e))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();

        let client = self.build_client(port)?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let mut auth_request = client.authorize_url(CsrfToken::new_random);
        for scope in &scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }
        auth_request = auth_request.add_extra_param("access_type", "offline");
        auth_request = auth_request.add_extra_param("prompt", "consent");

        let (auth_url, csrf_token) = auth_request.set_pkce_challenge(pkce_challenge).url();
        let expected_state = csrf_token.secret().clone();

        open::that(auth_url.to_string()).map_err(|e| format!("Failed to open browser: {}", e))?;
        log::info!("Opened browser for Google OAuth at port {}", port);

        let (tx, rx) = oneshot::channel::<Result<String, String>>();

        tokio::spawn(async move {
            // Keep serving connections until one actually carries the redirect.
            // Accepting exactly once meant any stray loopback connection -- a
            // port scanner, a browser probe -- consumed the flow's only chance
            // and freed the port before the real redirect arrived.
            let deadline = tokio::time::Instant::now() + CALLBACK_DEADLINE;
            loop {
                let (mut stream, _) = match tokio::time::timeout_at(deadline, listener.accept()).await {
                    Ok(Ok(accepted)) => accepted,
                    Ok(Err(e)) => {
                        // The listener itself is broken; retrying would spin.
                        // Dropping tx resolves the caller immediately.
                        log::warn!("OAuth callback accept failed: {}", e);
                        return;
                    }
                    Err(_) => {
                        log::warn!("OAuth callback listener timed out");
                        return;
                    }
                };

                let mut buf = [0u8; 4096];
                let read = tokio::time::timeout(
                    CALLBACK_READ_TIMEOUT,
                    tokio::io::AsyncReadExt::read(&mut stream, &mut buf),
                )
                .await;
                let n = match read {
                    // A zero-byte read is a connect-and-close probe, and a read
                    // timeout is a peer holding the socket open. Neither is the
                    // redirect: drop it and keep listening.
                    Ok(Ok(0)) | Err(_) => continue,
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) => {
                        log::warn!("OAuth callback read failed: {}", e);
                        continue;
                    }
                };
                let request = String::from_utf8_lossy(&buf[..n]).to_string();

                let error = extract_error(&request);
                let code = extract_code(&request);
                if error.is_none() && code.is_none() {
                    continue;
                }

                // PKCE already stops a stolen code being redeemed, but the
                // state parameter is the control that says this callback is
                // ours. A mismatch is rejected without resolving the flow.
                let state_matches = extract_param(&request, "state")
                    .map(|state| constant_time_eq(&state, &expected_state))
                    .unwrap_or(false);
                if !state_matches {
                    log::warn!("Discarding OAuth callback with a mismatched state parameter");
                    let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<html><body><h1>JARVIS</h1><p>This sign-in response did not come from the request JARVIS started. Please try again.</p></body></html>";
                    let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                    continue;
                }

                if let Some(err) = error {
                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>JARVIS</h1><p>Authentication failed. You can close this tab.</p></body></html>";
                    let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                    let _ = tx.send(Err(err));
                } else if let Some(code) = code {
                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>JARVIS</h1><p>Authentication successful. You can close this tab.</p></body></html>";
                    let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                    let _ = tx.send(Ok(code));
                }
                return;
            }
        });

        let code = tokio::time::timeout(CALLBACK_DEADLINE, rx)
            .await.map_err(|_| "OAuth timeout: no response within 3 minutes. Please try again.".to_string())?
            .map_err(|_| "OAuth cancelled".to_string())?
            .map_err(|e| format!("Google OAuth denied: {}", e))?;

        let exchange_future = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client);
        let token_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            exchange_future,
        )
        .await
        .map_err(|_| {
            "Google did not respond within 30 seconds while exchanging the authorization code. \
             Check your network and try again."
                .to_string()
        })?
        .map_err(|e| format!("Token exchange failed: {}", e))?;

        let access = token_result.access_token().secret().clone();
        let refresh = token_result.refresh_token().map(|t| t.secret().clone());

        self.store_access_token(access, token_result.expires_in());
        *self.refresh_token.lock().unwrap() = refresh;

        log::info!("Google OAuth completed successfully");
        Ok(())
    }

    /// Cache an access token together with the moment it stops being usable.
    /// Always set both -- a token with no recorded expiry is treated as spent.
    fn store_access_token(&self, token: String, expires_in: Option<std::time::Duration>) {
        let lifetime = expires_in.unwrap_or(DEFAULT_TOKEN_LIFETIME);
        *self.access_token.lock().unwrap() = Some(token);
        *self.access_token_expiry.lock().unwrap() =
            Some(std::time::Instant::now() + lifetime);
    }

    /// The cached access token, but only while it is still comfortably valid.
    fn live_access_token(&self) -> Option<String> {
        let token = self.access_token.lock().unwrap().clone()?;
        let expiry = (*self.access_token_expiry.lock().unwrap())?;
        if std::time::Instant::now() + TOKEN_EXPIRY_MARGIN < expiry {
            Some(token)
        } else {
            None
        }
    }

    pub async fn refresh_access_token(&self) -> Result<(), String> {
        let refresh = self.refresh_token.lock().unwrap().clone()
            .ok_or("No refresh token available")?;
        let client = self.build_client(0)?;
        let refresh_token = RefreshToken::new(refresh);
        let refresh_future = client
            .exchange_refresh_token(&refresh_token)
            .request_async(async_http_client);
        let token_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            refresh_future,
        )
        .await
        .map_err(|_| "Google did not respond within 30 seconds while refreshing the access token.".to_string())?
        .map_err(|e| format!("Token refresh failed: {}", e))?;

        self.store_access_token(
            token_result.access_token().secret().clone(),
            token_result.expires_in(),
        );
        if let Some(new_refresh) = token_result.refresh_token() {
            *self.refresh_token.lock().unwrap() = Some(new_refresh.secret().clone());
        }
        log::info!("Google access token refreshed");
        Ok(())
    }

    pub fn get_access_token(&self) -> Option<String> {
        self.access_token.lock().unwrap().clone()
    }

    pub fn has_refresh_token(&self) -> bool {
        self.refresh_token.lock().unwrap().is_some()
    }

    pub fn is_authenticated(&self) -> bool {
        self.access_token.lock().unwrap().is_some()
    }

    /// Returns true if we have either a live access token or a stored refresh token.
    pub fn is_linked(&self) -> bool {
        self.is_authenticated() || self.has_refresh_token()
    }

    /// Ensures a usable access token is available, refreshing from the stored
    /// refresh token when the cached one is expired or about to be. Callers
    /// still keep their 401 retry as a backstop; this stops them needing it
    /// once an hour, every hour.
    pub async fn ensure_access_token(&self) -> Result<String, String> {
        if let Some(token) = self.live_access_token() {
            return Ok(token);
        }
        if !self.has_refresh_token() {
            return Err("Not authenticated with Google".to_string());
        }
        // One refresh at a time. Without this, every caller that finds the
        // token expired at the same moment starts its own round trip to Google.
        let _refreshing = self.refresh_lock.lock().await;
        if let Some(token) = self.live_access_token() {
            return Ok(token);
        }
        self.refresh_access_token().await?;
        self.get_access_token()
            .ok_or_else(|| "Token refresh succeeded but no token available".to_string())
    }

    pub fn load_from_db(&self, db: &crate::db::Database) {
        // Preferred source: the Keychain.
        if let Ok(token) = KeychainStore::get(REFRESH_KEYCHAIN_SERVICE, REFRESH_KEYCHAIN_ACCOUNT) {
            if !token.is_empty() {
                *self.refresh_token.lock().unwrap() = Some(token);
                return;
            }
        }

        // Backward compatibility: older builds stored the refresh token in
        // plaintext in user_preferences. Read the DB row (tight lock scope, no
        // Keychain FFI while holding it); if it's a real token, adopt it and
        // migrate it into the Keychain, replacing the plaintext with a marker so
        // the credential no longer sits on disk. Existing users stay logged in.
        let db_value: Option<String> = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = 'google_refresh_token'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
        };
        if let Some(token) = db_value {
            if token != REFRESH_KEYCHAIN_MARKER && !token.is_empty() {
                *self.refresh_token.lock().unwrap() = Some(token.clone());
                if KeychainStore::put(
                    REFRESH_KEYCHAIN_SERVICE,
                    REFRESH_KEYCHAIN_ACCOUNT,
                    &token,
                )
                .is_ok()
                {
                    let conn = db.conn.lock().unwrap();
                    let _ = conn.execute(
                        "UPDATE user_preferences SET value = ?1, updated_at = datetime('now') WHERE key = 'google_refresh_token'",
                        rusqlite::params![REFRESH_KEYCHAIN_MARKER],
                    );
                }
            }
        }
    }

    /// Persist the refresh token to the Keychain. A failure here used to be a
    /// silent no-op, so the user looked signed in and was signed out again on
    /// the next launch with nothing in the log to explain it.
    pub fn save_to_db(&self, db: &crate::db::Database) -> Result<(), String> {
        // Clone the secret out and release the lock before any Keychain FFI.
        let token = self.refresh_token.lock().unwrap().clone();
        let Some(token) = token else {
            log::warn!("Google sign-in returned no refresh token; this session will not survive a restart");
            return Ok(());
        };
        if let Err(e) = KeychainStore::put(REFRESH_KEYCHAIN_SERVICE, REFRESH_KEYCHAIN_ACCOUNT, &token) {
            log::error!("Could not save the Google refresh token to the Keychain: {}", e);
            return Err(e);
        }
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let _ = conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES ('google_refresh_token', ?1, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
            rusqlite::params![REFRESH_KEYCHAIN_MARKER],
        );
        Ok(())
    }

    /// Unlink the Google account: revoke the grant at Google, delete the
    /// Keychain item and forget the in-memory tokens. Revocation is best
    /// effort -- someone disconnecting on a plane must still end up
    /// disconnected locally.
    pub async fn disconnect(&self, db: &crate::db::Database) -> Result<(), String> {
        let token = {
            let refresh = self.refresh_token.lock().unwrap().clone();
            refresh.or_else(|| self.access_token.lock().unwrap().clone())
        };
        if let Some(token) = token {
            match crate::util::http_client()
                .post(REVOKE_URL)
                .form(&[("token", token.as_str())])
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => log::info!("Google grant revoked"),
                Ok(resp) => log::warn!("Google revocation returned HTTP {}", resp.status()),
                Err(e) => log::warn!("Google revocation call failed: {}", e),
            }
        }

        *self.access_token.lock().unwrap() = None;
        *self.access_token_expiry.lock().unwrap() = None;
        *self.refresh_token.lock().unwrap() = None;

        if let Err(e) = KeychainStore::delete(REFRESH_KEYCHAIN_SERVICE, REFRESH_KEYCHAIN_ACCOUNT) {
            // Absent is the desired end state, so a "not found" is fine here.
            log::warn!("Keychain delete during disconnect: {}", e);
        }

        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM user_preferences WHERE key IN \
             ('google_refresh_token', 'user_name', 'cached_briefing', 'last_briefing_date')",
            [],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Read one query parameter out of an HTTP request line.
fn extract_param(request: &str, key: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for param in query.split('&') {
        let mut parts = param.splitn(2, '=');
        if parts.next()? == key {
            return parts.next().map(|s| s.to_string());
        }
    }
    None
}

fn extract_error(request: &str) -> Option<String> {
    extract_param(request, "error").map(|s| s.replace('+', " "))
}

fn extract_code(request: &str) -> Option<String> {
    extract_param(request, "code")
}

/// Compare the returned `state` against the one we generated without leaking
/// the position of the first differing byte through timing.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALLBACK: &str = "GET /?state=abc-123&code=4/auth-code HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";

    #[test]
    fn callback_params_are_parsed() {
        assert_eq!(extract_code(CALLBACK), Some("4/auth-code".to_string()));
        assert_eq!(extract_param(CALLBACK, "state"), Some("abc-123".to_string()));
        assert_eq!(extract_error(CALLBACK), None);
    }

    #[test]
    fn denied_consent_is_reported_as_an_error() {
        let denied = "GET /?state=abc-123&error=access+denied HTTP/1.1\r\n\r\n";
        assert_eq!(extract_error(denied), Some("access denied".to_string()));
        assert_eq!(extract_code(denied), None);
    }

    /// A probe that connects and sends nothing, or a request with no query,
    /// must not look like a callback -- the listener has to keep waiting for
    /// the real redirect instead of consuming its one chance on a port scan.
    #[test]
    fn probes_carry_neither_a_code_nor_an_error() {
        for probe in ["", "\r\n", "GET / HTTP/1.1\r\n\r\n", "GET /favicon.ico HTTP/1.1\r\n\r\n"] {
            assert_eq!(extract_code(probe), None, "probe {:?} must not yield a code", probe);
            assert_eq!(extract_error(probe), None, "probe {:?} must not yield an error", probe);
            assert_eq!(extract_param(probe, "state"), None);
        }
    }

    /// The cache used to have no expiry at all, so every Gmail and Calendar
    /// call kept presenting an hour-old token and relied on a 401 to notice.
    #[test]
    fn a_token_near_expiry_is_not_handed_out() {
        let auth = GoogleAuth::new_empty();
        assert_eq!(auth.live_access_token(), None, "nothing is cached yet");

        auth.store_access_token("fresh".to_string(), Some(std::time::Duration::from_secs(3600)));
        assert_eq!(auth.live_access_token(), Some("fresh".to_string()));

        auth.store_access_token("nearly-spent".to_string(), Some(std::time::Duration::from_secs(30)));
        assert_eq!(
            auth.get_access_token(),
            Some("nearly-spent".to_string()),
            "the token is still cached"
        );
        assert_eq!(
            auth.live_access_token(),
            None,
            "a token inside the refresh margin must trigger a refresh"
        );

        auth.store_access_token("spent".to_string(), Some(std::time::Duration::from_secs(0)));
        assert_eq!(auth.live_access_token(), None);
    }

    #[test]
    fn state_must_match_exactly() {
        let state = extract_param(CALLBACK, "state").unwrap();
        assert!(constant_time_eq(&state, "abc-123"));
        assert!(!constant_time_eq(&state, "abc-124"), "a forged state must be rejected");
        assert!(!constant_time_eq(&state, "abc-1234"), "length differences must be rejected");
        assert!(!constant_time_eq(&state, ""), "an absent state must be rejected");
    }
}
