//! JARVIS account sign-in against the Cognito user pool (control plane part 1).
//!
//! The browser does the sign-in on Cognito's managed login page; this module
//! runs the authorization-code + PKCE handshake around it, keeps the refresh
//! token in the Keychain and the access token in memory, and calls the
//! control-plane API with it. Design:
//! docs/superpowers/specs/2026-09-18-aws-identity-entitlement-design.md

use base64::Engine;
use oauth2::{PkceCodeChallenge, PkceCodeVerifier};
use serde::Deserialize;

use crate::auth::google::{constant_time_eq, extract_param};

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
}
