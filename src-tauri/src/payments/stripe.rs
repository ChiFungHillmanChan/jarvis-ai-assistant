use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Free,
    Pro,
    Team,
}

/// The server takes the user id from the verified token and the email from
/// Cognito, so the body carries only the tier.
#[derive(Debug, Serialize)]
pub struct CheckoutRequest {
    pub tier: Tier,
}

#[derive(Debug, Deserialize)]
pub struct CheckoutResponse {
    pub url: String,
    pub session_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("checkout backend not configured (set JARVIS_PAYMENT_API)")]
    NotConfigured,
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("checkout failed: {0}")]
    Checkout(String),
    #[error("not_signed_in")]
    NotSignedIn,
    #[error("{0}")]
    Account(String),
}

/// The checkout backend endpoint, embedded at compile time. A packaged .app has
/// no .env and no project root to read, so without this a release build cannot
/// reach the payment API at all and the upgrade button is dead. Same mechanism
/// the Google credentials use in auth/google.rs.
const DEFAULT_PAYMENT_API: Option<&str> = option_env!("JARVIS_PAYMENT_API");

/// Resolve the endpoint by checking the runtime env var first, then falling back
/// to the value baked in at compile time. Empty strings count as missing.
fn resolve_payment_api(
    runtime: Option<String>,
    compile_time: Option<&'static str>,
) -> Option<String> {
    runtime
        .filter(|v| !v.is_empty())
        .or_else(|| compile_time.map(str::to_string).filter(|v| !v.is_empty()))
}

fn payment_api_base() -> Result<String, PaymentError> {
    resolve_payment_api(env::var("JARVIS_PAYMENT_API").ok(), DEFAULT_PAYMENT_API)
        .ok_or(PaymentError::NotConfigured)
}

pub async fn create_checkout_session(
    tier: Tier,
    session: &crate::auth::cognito::CognitoSession,
) -> Result<CheckoutResponse, PaymentError> {
    use crate::auth::cognito::AccountError;
    let base = payment_api_base()?;
    let url = format!("{}/checkout", base.trim_end_matches('/'));
    let body = CheckoutRequest { tier };
    let client = crate::util::http_client();

    let resp = session
        .send_authed(|token| client.post(&url).bearer_auth(token).json(&body))
        .await
        .map_err(|e| match e {
            AccountError::NotSignedIn => PaymentError::NotSignedIn,
            other => PaymentError::Account(other.to_string()),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(PaymentError::Checkout(format!("{}: {}", status, body)));
    }

    let parsed: CheckoutResponse = resp.json().await?;
    Ok(parsed)
}

// The billing-portal client was removed 2026-08-07: it POSTed to a /portal
// route the payment stack has never registered, and no UI called it -- a
// guaranteed 404 masquerading as a feature. Rebuild it together with the
// entitlement layer, backend route first.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_env_wins_over_baked_in_endpoint() {
        let resolved = resolve_payment_api(
            Some("https://runtime.example.com".to_string()),
            Some("https://baked.example.com"),
        );
        assert_eq!(resolved.as_deref(), Some("https://runtime.example.com"));
    }

    #[test]
    fn falls_back_to_baked_in_endpoint() {
        assert_eq!(
            resolve_payment_api(None, Some("https://baked.example.com")).as_deref(),
            Some("https://baked.example.com"),
            "a packaged .app has no env var and must use the compile-time value"
        );
        assert_eq!(
            resolve_payment_api(Some(String::new()), Some("https://baked.example.com")).as_deref(),
            Some("https://baked.example.com"),
            "an empty env var must not shadow the baked-in endpoint"
        );
    }

    #[test]
    fn unconfigured_build_resolves_to_none() {
        assert_eq!(resolve_payment_api(None, None), None);
        assert_eq!(resolve_payment_api(Some(String::new()), Some("")), None);
    }

    #[test]
    fn checkout_body_carries_only_the_tier() {
        let body = serde_json::to_value(CheckoutRequest { tier: Tier::Pro }).unwrap();
        assert_eq!(body, serde_json::json!({ "tier": "pro" }));
    }
}
