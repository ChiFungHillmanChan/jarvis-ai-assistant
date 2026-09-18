//! JARVIS account sign-in against the Cognito user pool (control plane part 1).
//!
//! The browser does the sign-in on Cognito's managed login page; this module
//! runs the authorization-code + PKCE handshake around it, keeps the refresh
//! token in the Keychain and the access token in memory, and calls the
//! control-plane API with it. Design:
//! docs/superpowers/specs/2026-09-18-aws-identity-entitlement-design.md

use base64::Engine;
use oauth2::{CsrfToken, PkceCodeChallenge, PkceCodeVerifier};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tokio::net::TcpListener;

use crate::auth::google::{constant_time_eq, extract_param};
use crate::security::keychain::KeychainStore;

/// Must match the callback URLs registered on the app client in
/// `infra/lib/auth-stack.ts`. Cognito compares them exactly, port included,
/// so the listener cannot use an ephemeral port the way the Google flow does.
pub const LOOPBACK_PORTS: [u16; 3] = [47821, 47822, 47823];
/// Registered as the app client's logout URL. A loopback logout URL would land
/// on a port nothing is listening on by the time the browser gets there.
pub const LOGOUT_REDIRECT: &str = "https://jarvis-automation.com/";
/// Error strings the frontend matches on.
pub const NOT_SIGNED_IN: &str = "not_signed_in";
pub const SIGN_IN_CANCELLED: &str = "sign_in_cancelled";

const SCOPES: &str = "openid email profile";
const REFRESH_MARGIN_SECS: i64 = 60;
/// Cognito's error text when a Google sign-in lands on a user the pre-sign-up
/// trigger linked a moment ago. Repeating the same sign-in succeeds.
const LINKED_RETRY_MARKER: &str = "Already found an entry for username";

#[derive(Clone, Debug, PartialEq)]
pub struct CognitoConfig {
    /// `https://<prefix>.auth.<region>.amazoncognito.com`, no trailing slash.
    pub domain: String,
    pub client_id: String,
    /// Control-plane API base; the same value as `JARVIS_PAYMENT_API`.
    pub api_base: String,
}

/// Runtime env var first, then the value baked in at compile time. Empty counts
/// as missing. Same rule as `resolve_credential` in `google.rs`.
fn resolve(runtime: Option<String>, compile_time: Option<&'static str>) -> Option<String> {
    runtime
        .filter(|v| !v.trim().is_empty())
        .or_else(|| compile_time.map(str::to_string).filter(|v| !v.trim().is_empty()))
}

impl CognitoConfig {
    pub fn from_parts(
        domain: Option<String>,
        client_id: Option<String>,
        api_base: Option<String>,
    ) -> Option<Self> {
        let domain = domain?.trim().trim_end_matches('/').to_string();
        let domain = if domain.starts_with("https://") {
            domain
        } else {
            format!("https://{domain}")
        };
        Some(Self {
            domain,
            client_id: client_id?.trim().to_string(),
            api_base: api_base?.trim().trim_end_matches('/').to_string(),
        })
    }

    /// `None` when the build carries no account configuration; the UI then
    /// hides sign-in instead of offering a button that cannot work.
    pub fn from_env() -> Option<Self> {
        Self::from_parts(
            resolve(std::env::var("JARVIS_COGNITO_DOMAIN").ok(), option_env!("JARVIS_COGNITO_DOMAIN")),
            resolve(std::env::var("JARVIS_COGNITO_CLIENT_ID").ok(), option_env!("JARVIS_COGNITO_CLIENT_ID")),
            resolve(std::env::var("JARVIS_PAYMENT_API").ok(), option_env!("JARVIS_PAYMENT_API")),
        )
    }
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/callback")
}

pub fn authorize_url(
    cfg: &CognitoConfig,
    port: u16,
    state: &str,
    challenge: &str,
    provider: Option<&str>,
) -> String {
    use urlencoding::encode;
    let mut url = format!(
        "{}/oauth2/authorize?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        cfg.domain,
        encode(&cfg.client_id),
        encode(&redirect_uri(port)),
        encode(SCOPES),
        encode(state),
        encode(challenge),
    );
    if let Some(provider) = provider {
        url.push_str("&identity_provider=");
        url.push_str(&encode(provider));
    }
    url
}

pub fn logout_url(cfg: &CognitoConfig) -> String {
    use urlencoding::encode;
    format!(
        "{}/logout?client_id={}&logout_uri={}",
        cfg.domain,
        encode(&cfg.client_id),
        encode(LOGOUT_REDIRECT)
    )
}

pub fn pkce_challenge_for(verifier: &str) -> String {
    PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(verifier.to_string()))
        .as_str()
        .to_string()
}

#[derive(Debug, PartialEq)]
pub enum Callback {
    Code(String),
    Denied(String),
    StateMismatch,
    Ignore,
}

/// Classify one HTTP request that reached the loopback listener. Anything that
/// is not a `/callback` carrying a code or an error is a probe and is ignored;
/// the state check comes before the error check so a forged error cannot end a
/// sign-in the user is still completing.
pub fn parse_callback(request: &str, expected_state: &str) -> Callback {
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    if !(path == "/callback" || path.starts_with("/callback?")) {
        return Callback::Ignore;
    }
    let code = extract_param(request, "code");
    let error = extract_param(request, "error");
    if code.is_none() && error.is_none() {
        return Callback::Ignore;
    }
    let state_matches = extract_param(request, "state")
        .map(|state| constant_time_eq(&state, expected_state))
        .unwrap_or(false);
    if !state_matches {
        return Callback::StateMismatch;
    }
    if let Some(error) = error {
        let detail = extract_param(request, "error_description").unwrap_or(error);
        return Callback::Denied(decode_query_value(&detail));
    }
    Callback::Code(code.unwrap_or_default())
}

fn decode_query_value(raw: &str) -> String {
    let spaced = raw.replace('+', " ");
    urlencoding::decode(&spaced)
        .map(|s| s.into_owned())
        .unwrap_or(spaced)
}

pub fn is_linked_account_retry(description: &str) -> bool {
    description.contains(LINKED_RETRY_MARKER)
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub id_token: Option<String>,
    /// Present on the authorization-code exchange, absent on a refresh.
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_in: i64,
}

#[derive(Debug, PartialEq)]
pub enum TokenError {
    /// The refresh token is expired or revoked: the only error that signs the
    /// user out.
    InvalidGrant,
    Rejected(String),
    Network(String),
    Malformed(String),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenError::InvalidGrant => write!(f, "invalid_grant"),
            TokenError::Rejected(m) => write!(f, "rejected: {m}"),
            TokenError::Network(m) => write!(f, "network: {m}"),
            TokenError::Malformed(m) => write!(f, "malformed response: {m}"),
        }
    }
}

pub fn parse_token_response(status: u16, body: &str) -> Result<TokenSet, TokenError> {
    if (200..300).contains(&status) {
        return serde_json::from_str(body).map_err(|e| TokenError::Malformed(e.to_string()));
    }
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
    match code.as_deref() {
        Some("invalid_grant") => Err(TokenError::InvalidGrant),
        Some(other) => Err(TokenError::Rejected(other.to_string())),
        None => Err(TokenError::Rejected(format!("HTTP {status}"))),
    }
}

pub fn needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at - now < REFRESH_MARGIN_SECS
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct IdClaims {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
}

/// Display-only: the token came straight from the token endpoint over TLS, and
/// every authorisation decision is made by API Gateway, which verifies the
/// signature. Nothing here trusts these claims for access.
pub fn id_token_claims(id_token: &str) -> Option<IdClaims> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

const KEYCHAIN_SERVICE: &str = "com.jarvis.account";
const KEYCHAIN_ACCOUNT: &str = "refresh_token";
/// Total time the user has to finish signing in in the browser.
const CALLBACK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(300);
/// A peer that connects and says nothing must not hold up the listener.
const CALLBACK_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Me {
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    pub tier: String,
    #[serde(rename = "subscriptionStatus", default)]
    pub subscription_status: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct AccountStatus {
    pub available: bool,
    pub signed_in: bool,
    pub email: Option<String>,
    /// `None` while signed in means the API could not be reached. The UI shows
    /// it as unknown; it is never guessed as a paid tier.
    pub tier: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum AccountError {
    NotConfigured,
    NotSignedIn,
    Failed(String),
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountError::NotConfigured => write!(f, "Accounts are not configured for this build."),
            AccountError::NotSignedIn => write!(f, "{NOT_SIGNED_IN}"),
            AccountError::Failed(m) => write!(f, "{m}"),
        }
    }
}

enum FlowError {
    Denied(String),
    TimedOut,
    Other(String),
}

impl From<FlowError> for AccountError {
    fn from(e: FlowError) -> Self {
        match e {
            FlowError::Denied(m) => AccountError::Failed(format!("Sign-in did not complete: {m}")),
            FlowError::TimedOut => AccountError::Failed(SIGN_IN_CANCELLED.to_string()),
            FlowError::Other(m) => AccountError::Failed(m),
        }
    }
}

#[derive(Default)]
struct Tokens {
    access_token: Option<String>,
    expires_at: i64,
    email: Option<String>,
    me: Option<Me>,
    /// Bumped by `forget_local`. A write that was started before a sign-out
    /// (a refresh, a sign-in, or a `/me` fetch that is still in flight)
    /// captures this before its `.await` and only applies if it is still
    /// current, so a slow response landing after sign-out cannot resurrect a
    /// session the user just ended.
    generation: u64,
}

pub struct CognitoSession {
    config: Option<CognitoConfig>,
    /// Never held across an `.await`.
    tokens: Mutex<Tokens>,
    /// Serialises refreshes so concurrent callers produce one token request.
    refresh_lock: tokio::sync::Mutex<()>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub async fn bind_first_free(ports: &[u16]) -> Option<(TcpListener, u16)> {
    for &port in ports {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Some((listener, port));
        }
    }
    None
}

fn page(status: u16, reason: &str, message: &str) -> String {
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<html><body style=\"font-family:-apple-system,sans-serif;padding:40px\"><h1>JARVIS</h1><p>{message}</p></body></html>"
    )
}

/// Serve loopback connections until one carries the redirect for this sign-in.
/// Same shape as the Google listener: probes and stray requests are answered
/// and skipped rather than ending the flow.
async fn wait_for_code(listener: TcpListener, expected_state: &str) -> Result<String, FlowError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let deadline = tokio::time::Instant::now() + CALLBACK_DEADLINE;
    loop {
        let (mut stream, _) = match tokio::time::timeout_at(deadline, listener.accept()).await {
            Ok(Ok(accepted)) => accepted,
            Ok(Err(e)) => return Err(FlowError::Other(format!("Sign-in listener failed: {e}"))),
            Err(_) => return Err(FlowError::TimedOut),
        };
        let mut buf = [0u8; 8192];
        let n = match tokio::time::timeout(CALLBACK_READ_TIMEOUT, stream.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => continue,
            Ok(Ok(n)) => n,
        };
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        match parse_callback(&request, expected_state) {
            Callback::Ignore => {
                let _ = stream.write_all(page(404, "Not Found", "Nothing here.").as_bytes()).await;
            }
            Callback::StateMismatch => {
                log::warn!("Discarding account callback with a mismatched state parameter");
                let _ = stream
                    .write_all(page(400, "Bad Request", "This sign-in response did not come from the request JARVIS started. Please start again from JARVIS.").as_bytes())
                    .await;
            }
            Callback::Denied(description) => {
                let _ = stream
                    .write_all(page(200, "OK", "Sign-in did not complete. You can close this tab and return to JARVIS.").as_bytes())
                    .await;
                return Err(FlowError::Denied(description));
            }
            Callback::Code(code) => {
                let _ = stream
                    .write_all(page(200, "OK", "Signed in. You can close this tab and return to JARVIS.").as_bytes())
                    .await;
                return Ok(code);
            }
        }
    }
}

impl CognitoSession {
    pub fn new(config: Option<CognitoConfig>) -> Self {
        Self {
            config,
            tokens: Mutex::new(Tokens::default()),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn available(&self) -> bool {
        self.config.is_some()
    }

    fn config(&self) -> Result<&CognitoConfig, AccountError> {
        self.config.as_ref().ok_or(AccountError::NotConfigured)
    }

    fn tokens(&self) -> std::sync::MutexGuard<'_, Tokens> {
        self.tokens.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Snapshot to compare against after an `.await`; see `Tokens::generation`.
    fn generation(&self) -> u64 {
        self.tokens().generation
    }

    fn store(&self, set: &TokenSet) {
        let email = set.id_token.as_deref().and_then(id_token_claims).and_then(|c| c.email);
        let mut t = self.tokens();
        t.access_token = Some(set.access_token.clone());
        t.expires_at = now() + set.expires_in;
        if email.is_some() {
            t.email = email;
        }
    }

    fn live_access_token(&self) -> Option<String> {
        let t = self.tokens();
        t.access_token.clone().filter(|_| !needs_refresh(t.expires_at, now()))
    }

    fn drop_access_token(&self) {
        self.tokens().access_token = None;
    }

    fn forget_local(&self) {
        {
            let mut t = self.tokens();
            let next_generation = t.generation.wrapping_add(1);
            *t = Tokens::default();
            t.generation = next_generation;
        }
        let _ = KeychainStore::delete(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<TokenSet, TokenError> {
        let cfg = self.config().map_err(|e| TokenError::Rejected(e.to_string()))?;
        let resp = crate::util::http_client()
            .post(format!("{}/oauth2/token", cfg.domain))
            .form(form)
            .send()
            .await
            .map_err(|e| TokenError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        parse_token_response(status, &body)
    }

    /// A valid access token, refreshed from the Keychain when needed. Only an
    /// `invalid_grant` signs the user out; a network failure is reported as a
    /// failure so a flaky connection never logs anyone out.
    pub async fn access_token(&self) -> Result<String, AccountError> {
        let client_id = self.config()?.client_id.clone();
        if let Some(token) = self.live_access_token() {
            return Ok(token);
        }
        let _guard = self.refresh_lock.lock().await;
        if let Some(token) = self.live_access_token() {
            return Ok(token);
        }
        let refresh = KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
            .map_err(|_| AccountError::NotSignedIn)?;
        let generation = self.generation();
        match self
            .token_request(&[
                ("grant_type", "refresh_token"),
                ("client_id", client_id.as_str()),
                ("refresh_token", refresh.as_str()),
            ])
            .await
        {
            Ok(set) => {
                // A sign-out that ran while this refresh was in flight must
                // not be undone by writing the token it fetched.
                if self.generation() == generation {
                    self.store(&set);
                }
                Ok(set.access_token)
            }
            Err(TokenError::InvalidGrant) => {
                self.forget_local();
                Err(AccountError::NotSignedIn)
            }
            Err(e) => Err(AccountError::Failed(format!("Token refresh failed: {e}"))),
        }
    }

    /// Send a request with the bearer token. A 401 gets one forced refresh and
    /// one retry; a second 401 means the session is gone.
    pub async fn send_authed<F>(&self, build: F) -> Result<reqwest::Response, AccountError>
    where
        F: Fn(&str) -> reqwest::RequestBuilder,
    {
        let token = self.access_token().await?;
        let resp = build(&token)
            .send()
            .await
            .map_err(|e| AccountError::Failed(format!("network: {e}")))?;
        if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        self.drop_access_token();
        let token = self.access_token().await?;
        let resp = build(&token)
            .send()
            .await
            .map_err(|e| AccountError::Failed(format!("network: {e}")))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.forget_local();
            return Err(AccountError::NotSignedIn);
        }
        Ok(resp)
    }

    pub async fn fetch_me(&self) -> Result<Me, AccountError> {
        let url = format!("{}/me", self.config()?.api_base);
        let client = crate::util::http_client();
        let generation = self.generation();
        let resp = self.send_authed(|token| client.get(&url).bearer_auth(token)).await?;
        if !resp.status().is_success() {
            return Err(AccountError::Failed(format!("GET /me returned {}", resp.status())));
        }
        let me: Me = resp
            .json()
            .await
            .map_err(|e| AccountError::Failed(format!("GET /me: {e}")))?;
        // A sign-out that ran while this fetch was in flight must not leave a
        // cached `me` behind for the next `status(false)` to serve.
        if self.generation() == generation {
            self.tokens().me = Some(me.clone());
        }
        Ok(me)
    }

    pub async fn status(&self, force: bool) -> AccountStatus {
        if !self.available() {
            return AccountStatus::default();
        }
        if !force {
            let cached = {
                let t = self.tokens();
                t.me.clone().map(|me| (me, t.email.clone()))
            };
            if let Some((me, email)) = cached {
                return AccountStatus {
                    available: true,
                    signed_in: true,
                    email: me.email.or(email),
                    tier: Some(me.tier),
                };
            }
        }
        match self.fetch_me().await {
            Ok(me) => {
                let fallback = self.tokens().email.clone();
                AccountStatus {
                    available: true,
                    signed_in: true,
                    email: me.email.or(fallback),
                    tier: Some(me.tier),
                }
            }
            Err(AccountError::Failed(e)) => {
                log::warn!("account status: {e}");
                AccountStatus {
                    available: true,
                    signed_in: true,
                    email: self.tokens().email.clone(),
                    tier: None,
                }
            }
            Err(_) => AccountStatus {
                available: true,
                ..AccountStatus::default()
            },
        }
    }

    async fn run_browser_flow(
        &self,
        cfg: &CognitoConfig,
        provider: Option<&str>,
    ) -> Result<TokenSet, FlowError> {
        let (listener, port) = bind_first_free(&LOOPBACK_PORTS).await.ok_or_else(|| {
            FlowError::Other(
                "Ports 47821-47823 are all in use. Close the app using them and try again.".to_string(),
            )
        })?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let state = CsrfToken::new_random().secret().clone();
        let url = authorize_url(cfg, port, &state, challenge.as_str(), provider);
        open::that(&url).map_err(|e| FlowError::Other(format!("Could not open the browser: {e}")))?;
        let code = wait_for_code(listener, &state).await?;
        let redirect = redirect_uri(port);
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("client_id", cfg.client_id.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("code_verifier", verifier.secret().as_str()),
        ])
        .await
        .map_err(|e| FlowError::Other(format!("Token exchange failed: {e}")))
    }

    /// `provider` is `Some("Google")` to go straight to Google, `None` for the
    /// hosted page (which offers both email and Google).
    pub async fn sign_in(&self, provider: Option<&str>) -> Result<AccountStatus, AccountError> {
        let cfg = self.config()?.clone();
        let generation = self.generation();
        let mut result = self.run_browser_flow(&cfg, provider).await;
        if let Err(FlowError::Denied(description)) = &result {
            if is_linked_account_retry(description) {
                log::info!("Google identity was just linked to an existing account; retrying sign-in once");
                result = self.run_browser_flow(&cfg, provider).await;
            }
        }
        let set = result?;
        let refresh = set
            .refresh_token
            .clone()
            .ok_or_else(|| AccountError::Failed("Cognito returned no refresh token".to_string()))?;
        KeychainStore::put(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, &refresh).map_err(AccountError::Failed)?;
        // A sign-out that ran while the browser flow was open must not have
        // this now-stale flow write a token back into memory.
        if self.generation() == generation {
            self.store(&set);
            self.tokens().me = None;
        }
        Ok(self.status(true).await)
    }

    pub async fn sign_out(&self) -> Result<(), AccountError> {
        let cfg = self.config()?.clone();
        if let Ok(refresh) = KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            let revoke = crate::util::http_client()
                .post(format!("{}/oauth2/revoke", cfg.domain))
                .form(&[("token", refresh.as_str()), ("client_id", cfg.client_id.as_str())])
                .send()
                .await;
            if let Err(e) = revoke {
                log::warn!("refresh token revoke failed: {e}");
            }
        }
        self.forget_local();
        // Clears the managed-login session cookie, otherwise the next "Sign in"
        // silently signs the same account back in.
        if let Err(e) = open::that(logout_url(&cfg)) {
            log::warn!("could not open the logout page: {e}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> CognitoConfig {
        CognitoConfig {
            domain: "https://jarvis-automation-sandbox.auth.ap-southeast-1.amazoncognito.com".into(),
            client_id: "client123".into(),
            api_base: "https://api.example.com".into(),
        }
    }

    #[test]
    fn config_needs_all_three_values_and_normalises_them() {
        assert_eq!(CognitoConfig::from_parts(None, Some("c".into()), Some("a".into())), None);
        assert_eq!(CognitoConfig::from_parts(Some("d".into()), None, Some("a".into())), None);
        assert_eq!(CognitoConfig::from_parts(Some("d".into()), Some("c".into()), None), None);
        let c = CognitoConfig::from_parts(
            Some("jarvis.auth.ap-southeast-1.amazoncognito.com/".into()),
            Some(" client ".into()),
            Some("https://api.example.com/".into()),
        )
        .unwrap();
        assert_eq!(c.domain, "https://jarvis.auth.ap-southeast-1.amazoncognito.com");
        assert_eq!(c.client_id, "client");
        assert_eq!(c.api_base, "https://api.example.com");
    }

    #[test]
    fn pkce_matches_the_rfc_7636_appendix_b_vector() {
        assert_eq!(
            pkce_challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_carries_every_required_parameter() {
        let url = authorize_url(&cfg(), 47822, "st4te", "ch4llenge", None);
        assert!(url.starts_with(
            "https://jarvis-automation-sandbox.auth.ap-southeast-1.amazoncognito.com/oauth2/authorize?"
        ));
        for part in [
            "response_type=code",
            "client_id=client123",
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A47822%2Fcallback",
            "scope=openid%20email%20profile",
            "state=st4te",
            "code_challenge=ch4llenge",
            "code_challenge_method=S256",
        ] {
            assert!(url.contains(part), "missing {part} in {url}");
        }
        assert!(!url.contains("identity_provider"));
    }

    #[test]
    fn google_sign_in_skips_the_hosted_page() {
        let url = authorize_url(&cfg(), 47821, "s", "c", Some("Google"));
        assert!(url.ends_with("&identity_provider=Google"));
    }

    #[test]
    fn logout_url_returns_to_the_website() {
        assert_eq!(
            logout_url(&cfg()),
            "https://jarvis-automation-sandbox.auth.ap-southeast-1.amazoncognito.com/logout?client_id=client123&logout_uri=https%3A%2F%2Fjarvis-automation.com%2F"
        );
    }

    fn req(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
    }

    #[test]
    fn callback_with_matching_state_yields_the_code() {
        assert_eq!(
            parse_callback(&req("/callback?code=abc-123&state=s1"), "s1"),
            Callback::Code("abc-123".into())
        );
    }

    #[test]
    fn callback_with_a_different_state_is_rejected() {
        assert_eq!(
            parse_callback(&req("/callback?code=abc&state=other"), "s1"),
            Callback::StateMismatch
        );
        assert_eq!(parse_callback(&req("/callback?code=abc"), "s1"), Callback::StateMismatch);
    }

    #[test]
    fn cognito_errors_surface_their_decoded_description() {
        let got = parse_callback(
            &req("/callback?error_description=Already+found+an+entry+for+username+Google_1%2B2&state=s1&error=invalid_request"),
            "s1",
        );
        assert_eq!(got, Callback::Denied("Already found an entry for username Google_1+2".into()));
        assert_eq!(
            parse_callback(&req("/callback?error=access_denied&state=s1"), "s1"),
            Callback::Denied("access_denied".into())
        );
    }

    #[test]
    fn probes_and_other_paths_are_ignored() {
        assert_eq!(parse_callback(&req("/favicon.ico"), "s1"), Callback::Ignore);
        assert_eq!(parse_callback(&req("/callback"), "s1"), Callback::Ignore);
        assert_eq!(parse_callback(&req("/other?code=abc&state=s1"), "s1"), Callback::Ignore);
        assert_eq!(parse_callback("", "s1"), Callback::Ignore);
    }

    #[test]
    fn only_the_linking_message_triggers_a_retry() {
        assert!(is_linked_account_retry("Already found an entry for username Google_123"));
        assert!(!is_linked_account_retry("access_denied"));
    }

    #[test]
    fn token_responses_are_parsed_and_errors_classified() {
        let ok = parse_token_response(
            200,
            r#"{"access_token":"a","id_token":"i","refresh_token":"r","expires_in":3600,"token_type":"Bearer"}"#,
        )
        .unwrap();
        assert_eq!(ok.access_token, "a");
        assert_eq!(ok.id_token.as_deref(), Some("i"));
        assert_eq!(ok.refresh_token.as_deref(), Some("r"));
        assert_eq!(ok.expires_in, 3600);

        let refreshed = parse_token_response(200, r#"{"access_token":"a2","id_token":"i2","expires_in":3600}"#).unwrap();
        assert_eq!(refreshed.refresh_token, None);

        assert_eq!(parse_token_response(400, r#"{"error":"invalid_grant"}"#), Err(TokenError::InvalidGrant));
        assert_eq!(
            parse_token_response(400, r#"{"error":"invalid_client"}"#),
            Err(TokenError::Rejected("invalid_client".into()))
        );
        assert_eq!(parse_token_response(502, "<html>"), Err(TokenError::Rejected("HTTP 502".into())));
        assert!(matches!(parse_token_response(200, "{}"), Err(TokenError::Malformed(_))));
    }

    #[test]
    fn refresh_happens_inside_the_last_minute() {
        assert!(!needs_refresh(1_000, 1_000 - 61));
        assert!(!needs_refresh(1_000, 1_000 - 60));
        assert!(needs_refresh(1_000, 1_000 - 59));
        assert!(needs_refresh(1_000, 1_000));
        assert!(needs_refresh(1_000, 2_000));
    }

    #[test]
    fn id_token_claims_are_read_from_the_payload() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"sub":"abc-sub","email":"a@b.com","email_verified":true}"#);
        let token = format!("eyJhbGciOiJSUzI1NiJ9.{payload}.sig");
        let claims = id_token_claims(&token).unwrap();
        assert_eq!(claims.sub, "abc-sub");
        assert_eq!(claims.email.as_deref(), Some("a@b.com"));
        assert_eq!(id_token_claims("not-a-jwt"), None);
    }

    /// The cfg(test) keychain is one process-global map; every test that
    /// writes this module's (service, account) entry holds this guard.
    static KEYCHAIN_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn the_first_free_port_is_used() {
        let held = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = held.local_addr().unwrap().port();
        let free = {
            let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            probe.local_addr().unwrap().port()
        };
        let (_listener, port) = bind_first_free(&[busy, free]).await.expect("one port is free");
        assert_eq!(port, free);
        assert!(bind_first_free(&[busy]).await.is_none());
    }

    #[tokio::test]
    async fn an_unconfigured_build_reports_accounts_unavailable() {
        let session = CognitoSession::new(None);
        assert!(!session.available());
        assert_eq!(session.status(false).await, AccountStatus::default());
        assert_eq!(session.access_token().await, Err(AccountError::NotConfigured));
    }

    #[tokio::test]
    async fn no_refresh_token_means_signed_out_without_a_network_call() {
        let _guard = KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = KeychainStore::delete(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
        let session = CognitoSession::new(Some(cfg()));
        assert_eq!(session.access_token().await, Err(AccountError::NotSignedIn));
        let status = session.status(false).await;
        assert!(status.available);
        assert!(!status.signed_in);
    }

    #[tokio::test]
    async fn a_live_access_token_is_served_from_memory() {
        let session = CognitoSession::new(Some(cfg()));
        session.store(&TokenSet {
            access_token: "live".into(),
            id_token: None,
            refresh_token: None,
            expires_in: 3600,
        });
        assert_eq!(session.access_token().await.as_deref(), Ok("live"));
    }

    #[test]
    fn forgetting_clears_memory_and_the_keychain() {
        let _guard = KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        KeychainStore::put(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, "r").unwrap();
        let session = CognitoSession::new(Some(cfg()));
        session.store(&TokenSet { access_token: "a".into(), id_token: None, refresh_token: None, expires_in: 3600 });
        session.forget_local();
        assert!(session.live_access_token().is_none());
        assert!(KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).is_err());
    }

    #[test]
    fn not_signed_in_has_a_stable_message() {
        assert_eq!(AccountError::NotSignedIn.to_string(), NOT_SIGNED_IN);
    }

    /// Sends one raw HTTP/1.1 request line to a loopback listener under test
    /// and reads (and discards) whatever comes back, so the caller knows the
    /// other side has actually read the request before the next one is sent.
    async fn send_and_drain(port: u16, request: &str) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut buf = [0u8; 512];
        let _ = stream.read(&mut buf).await;
    }

    #[tokio::test]
    async fn wait_for_code_skips_a_probe_and_a_mismatched_state_then_returns_the_code() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(wait_for_code(listener, "s1"));

        send_and_drain(port, &req("/favicon.ico")).await;
        send_and_drain(port, &req("/callback?code=abc&state=other")).await;
        send_and_drain(port, &req("/callback?code=abc-123&state=s1")).await;

        match handle.await.unwrap() {
            Ok(code) => assert_eq!(code, "abc-123"),
            Err(FlowError::Denied(_)) => panic!("expected a code, got Denied"),
            Err(FlowError::TimedOut) => panic!("expected a code, got TimedOut"),
            Err(FlowError::Other(_)) => panic!("expected a code, got Other"),
        }
    }

    #[tokio::test]
    async fn wait_for_code_returns_denied_for_a_matching_state_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(wait_for_code(listener, "s1"));

        send_and_drain(port, &req("/callback?error=access_denied&state=s1")).await;

        match handle.await.unwrap() {
            Err(FlowError::Denied(description)) => assert_eq!(description, "access_denied"),
            Ok(_) => panic!("expected Denied, got a code"),
            Err(FlowError::TimedOut) => panic!("expected Denied, got TimedOut"),
            Err(FlowError::Other(_)) => panic!("expected Denied, got Other"),
        }
    }

    /// A network failure while refreshing must be reported, not treated as a
    /// sign-out. Constructed directly rather than through `from_parts` (which
    /// forces `https://`) so the domain can point at a closed loopback port.
    #[tokio::test]
    async fn a_refresh_network_failure_is_reported_and_never_signs_out() {
        let _guard = KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let closed_port = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
            // `listener` drops here, so the port is bound to nothing by the
            // time `access_token` connects to it.
        };
        let cfg = CognitoConfig {
            domain: format!("http://127.0.0.1:{closed_port}"),
            client_id: "client123".into(),
            api_base: "https://api.example.com".into(),
        };
        KeychainStore::put(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, "refresh-token-value").unwrap();
        let session = CognitoSession::new(Some(cfg));

        let result = session.access_token().await;

        assert!(matches!(result, Err(AccountError::Failed(_))), "got {result:?}");
        assert_eq!(
            KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).unwrap(),
            "refresh-token-value"
        );
    }

    /// Minimal fake HTTP server for these tests: accepts one connection per
    /// entry in `responses`, in order, ignores the request content beyond
    /// reading it off the socket, and writes back the paired canned
    /// HTTP/1.1 response. The tests that use it know the exact request
    /// sequence `CognitoSession` will make, so responses are matched
    /// positionally rather than by path.
    async fn fake_server(listener: tokio::net::TcpListener, responses: Vec<(u16, &'static str, String)>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, reason, body) in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    }

    fn token_json() -> String {
        r#"{"access_token":"fresh-token","expires_in":3600}"#.to_string()
    }

    #[tokio::test]
    async fn an_invalid_grant_signs_the_user_out() {
        let _guard = KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(fake_server(
            listener,
            vec![(400, "Bad Request", r#"{"error":"invalid_grant"}"#.to_string())],
        ));
        let cfg = CognitoConfig {
            domain: format!("http://127.0.0.1:{port}"),
            client_id: "client123".into(),
            api_base: "https://api.example.com".into(),
        };
        KeychainStore::put(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, "refresh-token-value").unwrap();
        let session = CognitoSession::new(Some(cfg));

        let result = session.access_token().await;

        assert_eq!(result, Err(AccountError::NotSignedIn));
        assert!(KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn two_consecutive_401s_from_me_sign_the_user_out() {
        let _guard = KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(fake_server(
            listener,
            vec![
                (200, "OK", token_json()),
                (401, "Unauthorized", "{}".to_string()),
                (200, "OK", token_json()),
                (401, "Unauthorized", "{}".to_string()),
            ],
        ));
        let cfg = CognitoConfig {
            domain: format!("http://127.0.0.1:{port}"),
            client_id: "client123".into(),
            api_base: format!("http://127.0.0.1:{port}"),
        };
        KeychainStore::put(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, "refresh-token-value").unwrap();
        let session = CognitoSession::new(Some(cfg));

        let result = session.fetch_me().await;

        assert_eq!(result, Err(AccountError::NotSignedIn));
        assert!(KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_500_from_me_keeps_signed_in_true_with_an_unknown_tier() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(fake_server(
            listener,
            vec![(500, "Internal Server Error", "{}".to_string())],
        ));
        let cfg = CognitoConfig {
            domain: "https://auth.example.com".into(),
            client_id: "client123".into(),
            api_base: format!("http://127.0.0.1:{port}"),
        };
        let session = CognitoSession::new(Some(cfg));
        session.store(&TokenSet {
            access_token: "live".into(),
            id_token: None,
            refresh_token: None,
            expires_in: 3600,
        });

        let status = session.status(true).await;

        assert!(status.signed_in);
        assert_eq!(status.tier, None);
        server.await.unwrap();
    }

    /// Exercises the generation guard directly (no network, no browser): the
    /// same check `access_token`, `sign_in` and `fetch_me` each do after
    /// their `.await` before writing a token or `me` back into `Tokens`.
    #[test]
    fn a_write_guarded_by_a_stale_generation_is_not_applied() {
        let _guard = KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let session = CognitoSession::new(Some(cfg()));
        let generation = session.generation();

        session.forget_local();
        assert_ne!(session.generation(), generation);

        // What the guarded call sites do: capture the generation before the
        // await, then only apply the write if it is still current.
        if session.generation() == generation {
            session.store(&TokenSet {
                access_token: "late".into(),
                id_token: None,
                refresh_token: None,
                expires_in: 3600,
            });
        }
        assert!(session.live_access_token().is_none());
    }
}
