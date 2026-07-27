use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Free,
    Pro,
    Team,
}

#[derive(Debug, Serialize)]
pub struct CheckoutRequest<'a> {
    pub tier: Tier,
    pub user_id: &'a str,
    pub email: &'a str,
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

pub async fn create_checkout_session(req: CheckoutRequest<'_>) -> Result<CheckoutResponse, PaymentError> {
    let base = payment_api_base()?;
    let url = format!("{}/checkout", base.trim_end_matches('/'));

    let client = crate::util::http_client();
    let resp = client
        .post(&url)
        .json(&req)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(PaymentError::Checkout(format!("{}: {}", status, body)));
    }

    let parsed: CheckoutResponse = resp.json().await?;
    Ok(parsed)
}

pub async fn open_billing_portal(user_id: &str) -> Result<String, PaymentError> {
    let base = payment_api_base()?;
    let url = format!("{}/portal", base.trim_end_matches('/'));

    #[derive(Serialize)]
    struct PortalReq<'a> { user_id: &'a str }
    #[derive(Deserialize)]
    struct PortalResp { url: String }

    let client = crate::util::http_client();
    let resp = client
        .post(&url)
        .json(&PortalReq { user_id })
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(PaymentError::Checkout(format!("{}: {}", status, body)));
    }
    let parsed: PortalResp = resp.json().await?;
    Ok(parsed.url)
}

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
}
