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

fn payment_api_base() -> Result<String, PaymentError> {
    env::var("JARVIS_PAYMENT_API").map_err(|_| PaymentError::NotConfigured)
}

pub async fn create_checkout_session(req: CheckoutRequest<'_>) -> Result<CheckoutResponse, PaymentError> {
    let base = payment_api_base()?;
    let url = format!("{}/checkout", base.trim_end_matches('/'));

    let client = reqwest::Client::new();
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

    let client = reqwest::Client::new();
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
