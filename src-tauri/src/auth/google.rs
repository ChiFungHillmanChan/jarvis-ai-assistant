use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, PkceCodeChallenge, RedirectUrl, Scope, TokenUrl,
    AuthorizationCode, TokenResponse, RefreshToken,
};

/// Authorization and token endpoints set; device, introspection and
/// revocation endpoints unused (revocation is a plain POST in `disconnect`).
type GoogleClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[cfg(test)]
pub(crate) static REVOKED_IN_TESTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Revoke a Google grant. The token goes in the POST body, never the URL.
/// Google revokes per OAuth client and account, so this also ends every other
/// token this app holds for the same Google account.
pub(crate) async fn revoke_grant(token: &str) -> Result<(), String> {
    // Tests record the call instead of reaching Google, like KeychainStore's
    // memory backend.
    #[cfg(test)]
    {
        REVOKED_IN_TESTS.lock().unwrap().push(token.to_string());
        return Ok(());
    }
    #[cfg(not(test))]
    match crate::util::http_client()
        .post(REVOKE_URL)
        .form(&[("token", token)])
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            log::info!("Google grant revoked");
            Ok(())
        }
        Ok(resp) => Err(format!("Google revocation returned HTTP {}", resp.status())),
        Err(e) => Err(format!("Google revocation call failed: {}", e)),
    }
}

/// HTTP client for the token endpoint. Redirects are refused so an
/// authorization code or refresh token is never re-sent to another host.
fn token_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("Could not create the Google sign-in client: {}", e))
}
use std::sync::Mutex;
use tokio::sync::oneshot;

use crate::security::keychain::KeychainStore;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
#[cfg_attr(test, allow(dead_code))] // tests record revocations instead
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";

/// Google access tokens live about an hour. Treat one as spent slightly early
/// so a call that starts just before the boundary does not land just after it.
const TOKEN_EXPIRY_MARGIN_SECS: i64 = 60;

/// Whether an access token expiring at `expires_at` can still be used at
/// `now`. Both are wall-clock Unix seconds, as for Cognito
/// (`cognito::needs_refresh`). The expiry used to be a monotonic `Instant`,
/// which stands still while the Mac sleeps: after an hour asleep the token was
/// an hour older than the app believed, and the first calls after waking went
/// out with a spent token.
fn token_is_live(expires_at: i64, now: i64) -> bool {
    now + TOKEN_EXPIRY_MARGIN_SECS < expires_at
}

fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Assumed lifetime when a token response omits `expires_in`. Matches Google's
/// documented default; the margin above absorbs the difference either way.
const DEFAULT_TOKEN_LIFETIME: std::time::Duration = std::time::Duration::from_secs(3600);

/// How long the callback listener waits for a single connection to send its
/// request line before dropping it and going back to accepting. A peer that
/// connects and then says nothing must not consume the whole consent window.
const CALLBACK_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Total time the user has to finish consent in the browser.
const CALLBACK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(180);

/// Returned when the user cancels a sign-in that is waiting for the browser.
pub const SIGN_IN_CANCELLED: &str = "Google sign-in cancelled";

/// Returned once Google has refused the stored grant (`invalid_grant`).
pub const REVOKED: &str = "Google authorization expired or was revoked. Reconnect Google in Settings.";

/// Which browser sign-in a cancel is for. The Assistant connection and a Mail
/// mailbox run separate flows, and cancelling one must not end the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignIn {
    Assistant,
    Mailbox,
}

/// Sign-ins waiting for Google's redirect. Closing the browser tab tells
/// JARVIS nothing, so without a cancel the button stayed busy until
/// CALLBACK_DEADLINE, and a mailbox sign-in held the Mail lock that whole time.
static WAITING: Mutex<Vec<(SignIn, oneshot::Sender<()>)>> = Mutex::new(Vec::new());

fn wait_for_cancel(kind: SignIn) -> oneshot::Receiver<()> {
    let (tx, rx) = oneshot::channel();
    let mut waiting = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    // A finished sign-in dropped its receiver; forget it.
    waiting.retain(|(_, tx)| !tx.is_closed());
    waiting.push((kind, tx));
    rx
}

/// Stop the sign-in of this kind that is waiting for the browser, if any.
pub fn cancel_sign_in(kind: SignIn) -> bool {
    let mut waiting = WAITING.lock().unwrap_or_else(|e| e.into_inner());
    let (cancel, keep): (Vec<_>, Vec<_>) = waiting.drain(..).partition(|(k, _)| *k == kind);
    *waiting = keep;
    cancel
        .into_iter()
        .fold(false, |any, (_, tx)| tx.send(()).is_ok() || any)
}

/// The authorization code from Google's redirect, unless the user cancels or
/// the consent deadline passes first.
async fn wait_for_code(
    redirect: oneshot::Receiver<Result<String, String>>,
    cancel: oneshot::Receiver<()>,
) -> Result<String, String> {
    tokio::select! {
        result = tokio::time::timeout(CALLBACK_DEADLINE, redirect) => result
            .map_err(|_| "OAuth timeout: no response within 3 minutes. Please try again.".to_string())?
            .map_err(|_| "OAuth cancelled".to_string())?
            .map_err(|e| format!("Google OAuth denied: {}", e)),
        Ok(()) = cancel => Err(SIGN_IN_CANCELLED.to_string()),
    }
}

// The Google refresh token is a long-lived credential and lives in the macOS
// Keychain, never in plaintext SQLite. The DB keeps only this non-secret marker
// so existence checks keep working.
const REFRESH_KEYCHAIN_SERVICE: &str = "com.jarvis.google";
const REFRESH_KEYCHAIN_ACCOUNT: &str = "refresh_token";
const REFRESH_KEYCHAIN_MARKER: &str = "__keychain__";

/// The restricted scope behind the Assistant's mail tools. It is requested
/// only when the user turns those tools on (`google_connect_gmail`), never at
/// the first connect, so the app can be verified for Calendar without it.
pub const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

/// user_preferences key for the scopes Google granted the main account,
/// space-separated as Google lists them. Not a secret.
const GRANTED_SCOPES_KEY: &str = "google_granted_scopes";

/// What a grant stored before scopes were recorded covers: every earlier
/// build asked for exactly these at first connect, so those users already
/// consented to Gmail and keep it without a new consent screen.
const LEGACY_GRANT_SCOPES: &[&str] = &[
    GMAIL_SCOPE,
    "https://www.googleapis.com/auth/calendar.events",
    "https://www.googleapis.com/auth/userinfo.profile",
];

/// The granted scopes for a grant loaded at startup: the recorded list, or the
/// old full grant when a linked account has none recorded.
fn stored_scopes(recorded: Option<&str>, linked: bool) -> Vec<String> {
    match recorded {
        Some(list) => list.split_whitespace().map(str::to_string).collect(),
        None if linked => LEGACY_GRANT_SCOPES.iter().map(|s| s.to_string()).collect(),
        None => Vec::new(),
    }
}

/// Authorization parameters for each sign-in. `add_to` is the main account
/// when the Assistant asks for more scopes (Gmail): incremental authorization
/// (`include_granted_scopes`) then returns a token that still covers Calendar,
/// and `login_hint` preselects that account. The first connect does not
/// include granted scopes, so its token holds exactly what it asked for. A
/// mailbox's never does: on the main account it would be handed Calendar
/// write and Gmail modify access instead of its read-only pair.
fn sign_in_params(kind: SignIn, add_to: Option<&str>) -> Vec<(&'static str, String)> {
    match kind {
        SignIn::Assistant => {
            let mut params = vec![("prompt", "consent".to_string())];
            if let Some(account) = add_to {
                params.push(("include_granted_scopes", "true".to_string()));
                params.push(("login_hint", account.to_string()));
            }
            params
        }
        SignIn::Mailbox => vec![("prompt", "consent select_account".to_string())],
    }
}

pub struct GoogleAuth {
    client_id: String,
    client_secret: String,
    pub access_token: Mutex<Option<String>>,
    /// When the cached access token stops being usable, as wall-clock Unix
    /// seconds. Written together with the token itself -- without it every
    /// caller kept using an hour-old token and depended on a 401 to find out.
    access_token_expiry: Mutex<Option<i64>>,
    pub refresh_token: Mutex<Option<String>>,
    /// Serialises refreshes so a burst of calls that all find the token expired
    /// produces one round trip to Google, not one per caller.
    refresh_lock: tokio::sync::Mutex<()>,
    /// The scopes Google granted at the last sign-in. The consent screen lets
    /// the user untick some, so this can be less than what was requested.
    pub granted_scopes: Mutex<Vec<String>>,
    /// Set when Google answered a refresh with `invalid_grant`: the grant was
    /// revoked or expired, and only a new sign-in fixes it. Until then no call
    /// goes back to Google, and status reports "reconnect" instead of a stale
    /// "connected".
    revoked: std::sync::atomic::AtomicBool,
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
            granted_scopes: Mutex::new(Vec::new()),
            revoked: std::sync::atomic::AtomicBool::new(false),
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
            granted_scopes: Mutex::new(Vec::new()),
            revoked: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn has_credentials(&self) -> bool {
        !self.client_id.is_empty()
    }

    fn build_client(&self, redirect_port: u16) -> Result<GoogleClient, String> {
        if self.client_id.is_empty() {
            return Err(
                "Google sign-in is not configured for this build. Please contact support."
                    .to_string(),
            );
        }
        let mut client = BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_auth_uri(AuthUrl::new(AUTH_URL.to_string()).map_err(|e| e.to_string())?)
            .set_token_uri(TokenUrl::new(TOKEN_URL.to_string()).map_err(|e| e.to_string())?)
            .set_redirect_uri(
                RedirectUrl::new(format!("http://127.0.0.1:{}", redirect_port))
                    .map_err(|e| e.to_string())?,
            );
        if !self.client_secret.is_empty() {
            client = client.set_client_secret(ClientSecret::new(self.client_secret.clone()));
        }
        Ok(client)
    }

    /// The Assistant's sign-in; `cancel_sign_in(SignIn::Assistant)` stops it.
    pub async fn start_auth_flow(&self, scopes: Vec<String>) -> Result<(), String> {
        self.start_auth_flow_with_prompt(scopes, sign_in_params(SignIn::Assistant, None), SignIn::Assistant).await
    }

    /// The Assistant asking `account` for more scopes on top of its grant
    /// (Google incremental authorization). Cancelled like `start_auth_flow`.
    pub async fn start_incremental_auth_flow(&self, scopes: Vec<String>, account: &str) -> Result<(), String> {
        self.start_auth_flow_with_prompt(scopes, sign_in_params(SignIn::Assistant, Some(account)), SignIn::Assistant).await
    }

    /// Separate mailbox grants need an explicit chooser without changing the
    /// existing Calendar/chat sign-in behavior. `cancel_sign_in(SignIn::Mailbox)`
    /// stops it.
    pub async fn start_auth_flow_with_account_selection(&self, scopes: Vec<String>) -> Result<(), String> {
        self.start_auth_flow_with_prompt(scopes, sign_in_params(SignIn::Mailbox, None), SignIn::Mailbox).await
    }

    async fn start_auth_flow_with_prompt(&self, scopes: Vec<String>, params: Vec<(&'static str, String)>, kind: SignIn) -> Result<(), String> {
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
        for (key, value) in params {
            auth_request = auth_request.add_extra_param(key, value);
        }

        let (auth_url, csrf_token) = auth_request.set_pkce_challenge(pkce_challenge).url();
        let expected_state = csrf_token.secret().clone();

        let cancel = wait_for_cancel(kind);
        open::that(auth_url.to_string()).map_err(|e| format!("Failed to open browser: {}", e))?;
        log::info!("Opened browser for Google OAuth at port {}", port);

        let (tx, rx) = oneshot::channel::<Result<String, String>>();

        tokio::spawn(async move {
            // Keep serving connections until one actually carries the redirect.
            // Accepting exactly once meant any stray loopback connection -- a
            // port scanner, a browser probe -- consumed the flow's only chance
            // and freed the port before the real redirect arrived.
            let deadline = tokio::time::Instant::now() + CALLBACK_DEADLINE;
            let mut tx = tx;
            loop {
                let accepted = tokio::select! {
                    // Cancelled: free the port instead of answering a late
                    // redirect with "successful" for a sign-in nobody finishes.
                    _ = tx.closed() => return,
                    accepted = tokio::time::timeout_at(deadline, listener.accept()) => accepted,
                };
                let (mut stream, _) = match accepted {
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

        let code = wait_for_code(rx, cancel).await?;

        let http = token_http_client()?;
        let exchange_future = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(&http);
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
        // Google lists what was granted; a response without the list granted
        // everything requested.
        let granted = token_result
            .scopes()
            .map(|granted| granted.iter().map(|scope| scope.to_string()).collect())
            .unwrap_or(scopes);

        self.store_access_token(access, token_result.expires_in());
        *self.refresh_token.lock().unwrap() = refresh;
        *self.granted_scopes.lock().unwrap() = granted;
        self.revoked.store(false, std::sync::atomic::Ordering::SeqCst);

        log::info!("Google OAuth completed successfully");
        Ok(())
    }

    /// Cache an access token together with the moment it stops being usable.
    /// Always set both -- a token with no recorded expiry is treated as spent.
    fn store_access_token(&self, token: String, expires_in: Option<std::time::Duration>) {
        let lifetime = expires_in.unwrap_or(DEFAULT_TOKEN_LIFETIME);
        *self.access_token.lock().unwrap() = Some(token);
        let lifetime_secs = i64::try_from(lifetime.as_secs()).unwrap_or(i64::MAX);
        *self.access_token_expiry.lock().unwrap() = Some(unix_now().saturating_add(lifetime_secs));
    }

    /// The cached access token, but only while it is still comfortably valid.
    fn live_access_token(&self) -> Option<String> {
        let token = self.access_token.lock().unwrap().clone()?;
        let expiry = (*self.access_token_expiry.lock().unwrap())?;
        if token_is_live(expiry, unix_now()) {
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
        let http = token_http_client()?;
        let refresh_future = client
            .exchange_refresh_token(&refresh_token)
            .request_async(&http);
        let token_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            refresh_future,
        )
        .await
        .map_err(|_| "Google did not respond within 30 seconds while refreshing the access token.".to_string())?
        .map_err(|e| match e {
            // Keep Google's error code: "invalid_grant" means the grant was
            // revoked or expired and only a new consent can fix it, which
            // callers must tell apart from a network failure.
            oauth2::RequestTokenError::ServerResponse(response) => {
                if response.error().to_string() == "invalid_grant" {
                    self.revoked.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                format!("Token refresh failed: {}", response.error())
            }
            other => format!("Token refresh failed: {}", other),
        })?;

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

    /// Whether the user turned on Gmail for the Assistant. Every caller of the
    /// Gmail API through this account checks it and skips quietly when it is
    /// off; nothing here ever asks Google for Gmail in the background.
    pub fn gmail_granted(&self) -> bool {
        self.granted_scopes.lock().unwrap().iter().any(|scope| scope == GMAIL_SCOPE)
    }

    /// An empty session on the same OAuth client. The Gmail sign-in runs on
    /// one so that choosing another account in the browser cannot replace
    /// this session before the account is checked.
    pub fn fresh_session(&self) -> GoogleAuth {
        GoogleAuth {
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            ..GoogleAuth::new_empty()
        }
    }

    /// Take over a confirmed sign-in from `fresh_session`. Google normally
    /// returns a refresh token on consent; if it did not, the current one is
    /// kept so the session still survives a restart.
    pub fn adopt(&self, other: &GoogleAuth) {
        *self.access_token.lock().unwrap() = other.access_token.lock().unwrap().clone();
        *self.access_token_expiry.lock().unwrap() = *other.access_token_expiry.lock().unwrap();
        if let Some(refresh) = other.refresh_token.lock().unwrap().clone() {
            *self.refresh_token.lock().unwrap() = Some(refresh);
        }
        *self.granted_scopes.lock().unwrap() = other.granted_scopes.lock().unwrap().clone();
        self.revoked.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Signed in with a live token and these scopes, without Google.
    #[cfg(test)]
    pub(crate) fn signed_in_for_tests(scopes: &[&str]) -> GoogleAuth {
        let auth = GoogleAuth::new_empty();
        *auth.refresh_token.lock().unwrap() = Some("1//test".to_string());
        auth.store_access_token("test-token".to_string(), Some(std::time::Duration::from_secs(3600)));
        *auth.granted_scopes.lock().unwrap() = scopes.iter().map(|s| s.to_string()).collect();
        auth
    }

    /// Linked, but Google no longer honours the grant: only a new sign-in
    /// fixes it. A refresh that failed for any other reason (offline, Google
    /// down) is not this.
    pub fn needs_reconnect(&self) -> bool {
        self.is_linked()
            && (self.revoked.load(std::sync::atomic::Ordering::SeqCst) || !self.has_refresh_token())
            && self.live_access_token().is_none()
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
        // A revoked grant stays revoked; asking again every tick only spends
        // a round trip to be told so.
        if self.revoked.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(REVOKED.to_string());
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
        self.load_refresh_token(db);
        self.load_granted_scopes(db);
    }

    /// Read the recorded scopes, or assume the old full grant for an account
    /// linked before they were recorded. Call after the refresh token loads.
    fn load_granted_scopes(&self, db: &crate::db::Database) {
        let recorded: Option<String> = {
            let conn = db.conn.lock().unwrap();
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = ?1",
                [GRANTED_SCOPES_KEY],
                |row| row.get::<_, String>(0),
            )
            .ok()
        };
        *self.granted_scopes.lock().unwrap() = stored_scopes(recorded.as_deref(), self.has_refresh_token());
    }

    /// Record the granted scopes. A failure is returned, not ignored: a grant
    /// saved without its scopes would read as the old full grant, Gmail
    /// included, on the next launch.
    fn save_granted_scopes(&self, db: &crate::db::Database) -> Result<(), String> {
        let scopes = self.granted_scopes.lock().unwrap().join(" ");
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
            rusqlite::params![GRANTED_SCOPES_KEY, scopes],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn load_refresh_token(&self, db: &crate::db::Database) {
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
        // Before the token: a token stored without its scopes would load as
        // the old full grant.
        self.save_granted_scopes(db)?;
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
    /// disconnected locally. `revoke` is false when a Mail mailbox still uses
    /// the same Google account: Google revokes per app and account, so
    /// revoking here would sign that mailbox out too.
    pub async fn disconnect(&self, db: &crate::db::Database, revoke: bool) -> Result<(), String> {
        let token = {
            let refresh = self.refresh_token.lock().unwrap().clone();
            refresh.or_else(|| self.access_token.lock().unwrap().clone())
        };
        if let Some(token) = token.filter(|_| revoke) {
            if let Err(e) = revoke_grant(&token).await {
                log::warn!("{}", e);
            }
        }

        *self.access_token.lock().unwrap() = None;
        *self.access_token_expiry.lock().unwrap() = None;
        *self.refresh_token.lock().unwrap() = None;
        self.granted_scopes.lock().unwrap().clear();
        self.revoked.store(false, std::sync::atomic::Ordering::SeqCst);

        if let Err(e) = KeychainStore::delete(REFRESH_KEYCHAIN_SERVICE, REFRESH_KEYCHAIN_ACCOUNT) {
            // Absent is the desired end state, so a "not found" is fine here.
            log::warn!("Keychain delete during disconnect: {}", e);
        }

        // The last two are the scheduled daily briefing's
        // (scheduler/scheduled_briefing.rs): the brief on the Automation page
        // is drawn from this account's meetings and mail, and the record of
        // the day it counted against goes with it, so an account connected
        // afterwards gets its own briefing today.
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM user_preferences WHERE key IN \
             ('google_refresh_token', 'user_name', 'google_account_email', 'cached_briefing', 'last_briefing_date', \
              'daily_briefing_today', 'daily_briefing_last_date', ?1)",
            [GRANTED_SCOPES_KEY],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Read one query parameter out of an HTTP request line.
pub(crate) fn extract_param(request: &str, key: &str) -> Option<String> {
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
pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
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

    /// The expiry is a wall-clock time, so sleep counts against it. A token
    /// issued at 09:00 for an hour is spent at 10:00 whether or not the Mac
    /// was awake in between; the old monotonic clock stopped with the lid.
    #[test]
    fn a_token_expires_by_the_wall_clock() {
        let issued_at = 1_794_450_000;
        let expires_at = issued_at + 3600;
        assert!(token_is_live(expires_at, issued_at));
        assert!(token_is_live(expires_at, issued_at + 3500));
        assert!(!token_is_live(expires_at, issued_at + 3540), "inside the one-minute margin");
        assert!(!token_is_live(expires_at, issued_at + 3 * 3600), "three hours later, asleep or not");

        let auth = GoogleAuth::new_empty();
        auth.store_access_token("fresh".to_string(), Some(std::time::Duration::from_secs(3600)));
        let stored = auth.access_token_expiry.lock().unwrap().unwrap();
        assert!((3598..=3602).contains(&(stored - unix_now())), "stored as Unix seconds an hour from now");
    }

    /// Closing the browser tab tells JARVIS nothing, so the button stayed busy
    /// (and a mailbox sign-in held the Mail lock) until the 3-minute deadline.
    #[tokio::test]
    async fn a_waiting_sign_in_can_be_cancelled_without_ending_the_other_kind() {
        let (_redirect_tx, redirect) = oneshot::channel::<Result<String, String>>();
        let waiting = tokio::spawn(wait_for_code(redirect, wait_for_cancel(SignIn::Mailbox)));
        tokio::task::yield_now().await;

        assert!(!cancel_sign_in(SignIn::Assistant), "no Assistant sign-in is waiting");
        assert!(!waiting.is_finished(), "cancelling the other kind must not end this one");

        assert!(cancel_sign_in(SignIn::Mailbox));
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .expect("a cancelled sign-in stops waiting at once")
            .unwrap();
        assert_eq!(result, Err(SIGN_IN_CANCELLED.to_string()));
        assert!(!cancel_sign_in(SignIn::Mailbox), "nothing is left waiting");
    }

    #[tokio::test]
    async fn the_redirect_still_completes_a_sign_in_that_could_be_cancelled() {
        let (redirect_tx, redirect) = oneshot::channel::<Result<String, String>>();
        redirect_tx.send(Ok("4/code".to_string())).unwrap();
        let (_cancel_tx, cancel) = oneshot::channel();
        assert_eq!(wait_for_code(redirect, cancel).await, Ok("4/code".to_string()));
    }

    /// A revoked grant used to read as connected until restart: any cached
    /// access token, even an expired one, counted.
    #[tokio::test]
    async fn a_revoked_grant_needs_reconnecting_and_stops_asking_google() {
        let auth = GoogleAuth::new_empty();
        *auth.refresh_token.lock().unwrap() = Some("1//stored".to_string());
        auth.store_access_token("expired".to_string(), Some(std::time::Duration::from_secs(0)));
        assert!(!auth.needs_reconnect(), "not known to be revoked: offline is not revoked");

        auth.revoked.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(auth.needs_reconnect());
        // Returns at once: build_client would fail first if this reached Google.
        assert_eq!(auth.ensure_access_token().await, Err(REVOKED.to_string()));

        auth.store_access_token("fresh".to_string(), Some(std::time::Duration::from_secs(3600)));
        assert!(!auth.needs_reconnect(), "a live token is still usable");
    }

    fn with_scopes(scopes: &[&str]) -> GoogleAuth {
        let auth = GoogleAuth::new_empty();
        *auth.granted_scopes.lock().unwrap() = scopes.iter().map(|s| s.to_string()).collect();
        auth
    }

    /// Gmail is the restricted scope the Assistant now asks for only when the
    /// user turns on its mail tools. gmail.readonly, which a Mail mailbox on
    /// the same account can add to Google's record, does not cover drafts or
    /// archiving, so it does not count.
    #[test]
    fn only_the_gmail_modify_grant_turns_gmail_on() {
        let calendar = "https://www.googleapis.com/auth/calendar.events";
        assert!(!with_scopes(&[]).gmail_granted());
        assert!(!with_scopes(&[calendar]).gmail_granted());
        assert!(!with_scopes(&[calendar, "https://www.googleapis.com/auth/gmail.readonly"]).gmail_granted());
        assert!(with_scopes(&[calendar, GMAIL_SCOPE]).gmail_granted());
    }

    /// Every build before this one asked for Gmail at first connect, so a
    /// stored grant with no recorded scopes already includes it: those users
    /// keep their mail tools without a new consent screen.
    #[test]
    fn a_grant_stored_before_scopes_were_recorded_is_the_old_full_grant() {
        assert!(stored_scopes(None, true).iter().any(|s| s == GMAIL_SCOPE));
        assert!(stored_scopes(None, false).is_empty(), "nothing is linked: nothing is granted");
        assert_eq!(
            stored_scopes(Some("https://www.googleapis.com/auth/calendar.events https://www.googleapis.com/auth/userinfo.profile"), true),
            vec!["https://www.googleapis.com/auth/calendar.events", "https://www.googleapis.com/auth/userinfo.profile"],
            "a recorded list is taken as it is, Gmail or not"
        );
    }

    #[test]
    fn granted_scopes_survive_a_restart_and_a_legacy_grant_keeps_gmail() {
        let db = crate::db::test_database();
        let legacy = GoogleAuth::new_empty();
        *legacy.refresh_token.lock().unwrap() = Some("1//legacy".to_string());
        legacy.load_granted_scopes(&db);
        assert!(legacy.gmail_granted(), "no recorded scopes: the old grant, Gmail included");

        let calendar_only = with_scopes(&[
            "https://www.googleapis.com/auth/calendar.events",
            "https://www.googleapis.com/auth/userinfo.profile",
        ]);
        calendar_only.save_granted_scopes(&db).unwrap();
        let restarted = GoogleAuth::new_empty();
        *restarted.refresh_token.lock().unwrap() = Some("1//new".to_string());
        restarted.load_granted_scopes(&db);
        assert!(!restarted.gmail_granted(), "a Calendar-only grant must not read as Gmail after a restart");
        assert_eq!(*restarted.granted_scopes.lock().unwrap(), *calendar_only.granted_scopes.lock().unwrap());
    }

    /// Disconnect and switch account must not leave the old grant's scopes
    /// behind for the next account to inherit.
    #[tokio::test]
    async fn disconnect_forgets_the_granted_scopes() {
        let db = crate::db::test_database();
        let auth = with_scopes(&[GMAIL_SCOPE]);
        auth.save_granted_scopes(&db).unwrap();
        // revoke = false: nothing is sent to Google. The memory Keychain has
        // no item under this service, so its delete finds nothing.
        auth.disconnect(&db, false).await.unwrap();
        assert!(!auth.gmail_granted());
        let rows: i64 = db.conn.lock().unwrap()
            .query_row("SELECT COUNT(*) FROM user_preferences WHERE key = ?1", [GRANTED_SCOPES_KEY], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    /// The brief on the Automation page lists the disconnected account's
    /// meetings and mail. It goes with the account, and so does the record
    /// that today's briefing has been shown.
    #[tokio::test]
    async fn disconnect_forgets_the_daily_brief_drawn_from_the_account() {
        use crate::scheduler::scheduled_briefing::{todays_brief, LAST_DATE_KEY, TODAY_KEY};
        let db = crate::db::test_database();
        let brief = serde_json::json!({
            "date": "2026-11-12", "shown_at": "2026-11-12T08:30:01+08:00", "trigger": "scheduled",
            "local": "Meetings: 1 today\n  09:30\u{3000}Board call", "summary": "One meeting, with the board.",
            "summary_pending": false, "summary_error": null,
        });
        {
            let conn = db.conn.lock().unwrap();
            for (key, value) in [(TODAY_KEY, brief.to_string()), (LAST_DATE_KEY, "2026-11-12".to_string())] {
                conn.execute("INSERT INTO user_preferences (key, value) VALUES (?1, ?2)", [key, value.as_str()]).unwrap();
            }
        }
        assert!(todays_brief(&db, "2026-11-12").unwrap().is_some(), "the brief is on the page before the disconnect");

        with_scopes(&[]).disconnect(&db, false).await.unwrap();

        assert_eq!(todays_brief(&db, "2026-11-12").unwrap(), None);
        let left: i64 = db.conn.lock().unwrap()
            .query_row("SELECT COUNT(*) FROM user_preferences WHERE key IN (?1, ?2)", [TODAY_KEY, LAST_DATE_KEY], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0, "a newly connected account gets a briefing today");
    }

    /// Incremental authorization: asking for Gmail later returns a token that
    /// still covers Calendar. The first connect does not include granted
    /// scopes, so its token holds only what it asked for. A mailbox sign-in
    /// must not either, or a mailbox on the main account would be handed
    /// Calendar write and Gmail modify access instead of its read-only pair.
    #[test]
    fn only_adding_gmail_includes_granted_scopes() {
        let adding = sign_in_params(SignIn::Assistant, Some("me@example.com"));
        assert!(adding.contains(&("include_granted_scopes", "true".to_string())));
        assert!(adding.contains(&("prompt", "consent".to_string())));
        assert!(adding.contains(&("login_hint", "me@example.com".to_string())));

        let first = sign_in_params(SignIn::Assistant, None);
        assert_eq!(first, vec![("prompt", "consent".to_string())]);

        let mailbox = sign_in_params(SignIn::Mailbox, None);
        assert!(mailbox.contains(&("prompt", "consent select_account".to_string())));
        assert!(!mailbox.iter().any(|(k, _)| *k == "include_granted_scopes"));
    }

    /// The Gmail sign-in runs on a separate session and replaces the main one
    /// only once its account is confirmed.
    #[test]
    fn adopting_a_confirmed_sign_in_keeps_the_old_refresh_token_if_none_came_back() {
        let main = with_scopes(&["https://www.googleapis.com/auth/calendar.events"]);
        *main.refresh_token.lock().unwrap() = Some("1//calendar".to_string());
        let upgrade = main.fresh_session();
        assert!(upgrade.granted_scopes.lock().unwrap().is_empty(), "a fresh session shares only the OAuth client");
        *upgrade.granted_scopes.lock().unwrap() = vec![GMAIL_SCOPE.to_string()];
        upgrade.store_access_token("gmail-token".to_string(), Some(std::time::Duration::from_secs(3600)));

        main.adopt(&upgrade);
        assert!(main.gmail_granted());
        assert_eq!(main.live_access_token(), Some("gmail-token".to_string()));
        assert_eq!(main.refresh_token.lock().unwrap().as_deref(), Some("1//calendar"));

        *upgrade.refresh_token.lock().unwrap() = Some("1//combined".to_string());
        main.adopt(&upgrade);
        assert_eq!(main.refresh_token.lock().unwrap().as_deref(), Some("1//combined"));
    }

    #[test]
    fn state_must_match_exactly() {
        let state = extract_param(CALLBACK, "state").unwrap();
        assert!(constant_time_eq(&state, "abc-123"));
        assert!(!constant_time_eq(&state, "abc-124"), "a forged state must be rejected");
        assert!(!constant_time_eq(&state, "abc-1234"), "length differences must be rejected");
        assert!(!constant_time_eq(&state, ""), "an absent state must be rejected");
    }

    /// Live: Google must answer a made-up refresh token with `invalid_grant`.
    /// That proves the token request works end to end (TLS, client
    /// authentication, form encoding, error parsing); a wrong client or a
    /// broken request would come back as `invalid_client` or a transport error.
    /// Needs GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET in the environment.
    #[tokio::test]
    #[ignore]
    async fn live_token_endpoint_rejects_a_made_up_refresh_token() {
        let auth = GoogleAuth::new().expect("GOOGLE_CLIENT_ID must be set");
        *auth.refresh_token.lock().unwrap() = Some("1//made-up-refresh-token".to_string());
        let err = auth.refresh_access_token().await.unwrap_err();
        assert_eq!(err, "Token refresh failed: invalid_grant");
    }
}
