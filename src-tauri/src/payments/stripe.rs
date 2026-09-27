use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Free,
    Pro,
    Team,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub enum PaymentLocale {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-HK")]
    TraditionalChinese,
}

/// Identity is taken from the verified token; locale only selects return pages.
#[derive(Debug, Serialize)]
pub struct CheckoutRequest {
    pub tier: Tier,
    pub locale: PaymentLocale,
    /// A promotion code typed in the app; omitted for a normal checkout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CheckoutResponse {
    pub url: String,
    pub session_id: String,
    #[serde(default)]
    pub already_subscribed: bool,
}

#[derive(Debug, Deserialize)]
pub struct PortalResponse {
    pub url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("checkout backend not configured (set JARVIS_PAYMENT_API)")]
    NotConfigured,
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("billing request failed: {0}")]
    Checkout(String),
    #[error("not_signed_in")]
    NotSignedIn,
    #[error("{0}")]
    Account(String),
    #[error("billing_busy")]
    Busy,
    #[error("invalid_code")]
    InvalidCode,
    #[error("The payment service returned an unsupported URL.")]
    InvalidUrl,
}

/// The checkout backend endpoint, embedded at compile time. A packaged .app has
/// no .env and no project root to read, so without this a release build cannot
/// reach the payment API at all and the upgrade button is dead. Same mechanism
/// the Google credentials use in auth/google.rs.
const DEFAULT_PAYMENT_API: Option<&str> = option_env!("JARVIS_PAYMENT_API");

/// Resolve the endpoint from the launch-time env var (when this build allows
/// it, see `auth::cognito::runtime_env`), then the value baked in at compile
/// time. Empty strings count as missing.
fn resolve_payment_api(
    runtime: Option<String>,
    compile_time: Option<&'static str>,
) -> Option<String> {
    runtime
        .filter(|v| !v.is_empty())
        .or_else(|| compile_time.map(str::to_string).filter(|v| !v.is_empty()))
}

fn payment_api_base() -> Result<String, PaymentError> {
    resolve_payment_api(crate::auth::cognito::runtime_env("JARVIS_PAYMENT_API"), DEFAULT_PAYMENT_API)
        .ok_or(PaymentError::NotConfigured)
}

/// Only Stripe-hosted payment pages may be opened, including retries from IPC.
pub fn validate_payment_url(value: &str) -> Result<(), PaymentError> {
    let url = reqwest::Url::parse(value).map_err(|_| PaymentError::InvalidUrl)?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("checkout.stripe.com" | "billing.stripe.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(PaymentError::InvalidUrl);
    }
    Ok(())
}

/// Maps a non-success reply to the error codes the frontend matches on.
fn rejection(status: reqwest::StatusCode, body: &str) -> PaymentError {
    if status == reqwest::StatusCode::CONFLICT {
        return PaymentError::Busy;
    }
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
    if status == reqwest::StatusCode::BAD_REQUEST && code.as_deref() == Some("invalid_code") {
        return PaymentError::InvalidCode;
    }
    PaymentError::Checkout(status.to_string())
}

async fn post_session<T: serde::de::DeserializeOwned>(
    path: &str,
    body: Option<serde_json::Value>,
    session: &crate::auth::cognito::CognitoSession,
    generation: u64,
) -> Result<T, PaymentError> {
    use crate::auth::cognito::AccountError;
    let base = payment_api_base()?;
    let url = format!("{}/{}", base.trim_end_matches('/'), path);
    let client = crate::util::http_client();
    let resp = session
        .send_authed_in_generation(generation, |token| {
            let request = client.post(&url).bearer_auth(token);
            match &body {
                Some(body) => request.json(body),
                None => request,
            }
        })
        .await
        .map_err(|e| match e {
            AccountError::NotSignedIn => PaymentError::NotSignedIn,
            other => PaymentError::Account(other.to_string()),
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(rejection(status, &body));
    }
    let parsed = resp.json().await?;
    session.with_current_generation(generation, || ()).map_err(|_| PaymentError::NotSignedIn)?;
    Ok(parsed)
}

pub async fn create_checkout_session(
    tier: Tier,
    locale: PaymentLocale,
    code: Option<String>,
    session: &crate::auth::cognito::CognitoSession,
    generation: u64,
) -> Result<CheckoutResponse, PaymentError> {
    let parsed: CheckoutResponse = post_session("checkout", Some(serde_json::json!(CheckoutRequest { tier, locale, code })), session, generation).await?;
    validate_payment_url(&parsed.url)?;
    Ok(parsed)
}

pub async fn create_portal_session(
    locale: PaymentLocale,
    session: &crate::auth::cognito::CognitoSession,
    generation: u64,
) -> Result<PortalResponse, PaymentError> {
    let parsed: PortalResponse = post_session("portal", Some(serde_json::json!({ "locale": locale })), session, generation).await?;
    validate_payment_url(&parsed.url)?;
    Ok(parsed)
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

    #[test]
    fn checkout_body_carries_only_tier_and_the_selected_locale() {
        let body = serde_json::to_value(CheckoutRequest { tier: Tier::Pro, locale: PaymentLocale::TraditionalChinese, code: None }).unwrap();
        assert_eq!(body, serde_json::json!({ "tier": "pro", "locale": "zh-HK" }));
    }

    #[test]
    fn a_code_is_sent_only_when_given() {
        let body = serde_json::to_value(CheckoutRequest { tier: Tier::Pro, locale: PaymentLocale::English, code: Some("PILOT100".into()) }).unwrap();
        assert_eq!(body, serde_json::json!({ "tier": "pro", "locale": "en", "code": "PILOT100" }));
    }

    #[test]
    fn server_rejections_map_to_the_codes_the_app_matches() {
        use reqwest::StatusCode;
        assert!(matches!(rejection(StatusCode::CONFLICT, ""), PaymentError::Busy));
        assert!(matches!(rejection(StatusCode::BAD_REQUEST, r#"{"error":"invalid_code"}"#), PaymentError::InvalidCode));
        assert_eq!(PaymentError::InvalidCode.to_string(), "invalid_code");
        assert!(matches!(rejection(StatusCode::BAD_REQUEST, r#"{"error":"invalid_tier"}"#), PaymentError::Checkout(_)));
        assert!(matches!(rejection(StatusCode::SERVICE_UNAVAILABLE, "not json"), PaymentError::Checkout(_)));
    }

    #[test]
    fn payment_locale_accepts_only_the_two_ui_languages_and_defaults_to_english() {
        assert_eq!(serde_json::to_value(PaymentLocale::default()).unwrap(), "en");
        assert!(serde_json::from_str::<PaymentLocale>("\"en\"").is_ok());
        assert!(serde_json::from_str::<PaymentLocale>("\"zh-HK\"").is_ok());
        assert!(serde_json::from_str::<PaymentLocale>("\"../../other\"").is_err());
    }

    #[test]
    fn only_stripe_https_checkout_and_billing_urls_are_accepted() {
        for url in ["https://checkout.stripe.com/c/pay/cs_test_123", "https://billing.stripe.com/p/session/test_123"] {
            assert!(validate_payment_url(url).is_ok(), "{url}");
        }
        for url in ["http://checkout.stripe.com/pay", "https://checkout.stripe.com.evil.test/pay", "https://evil.test/?checkout.stripe.com", "https://user@billing.stripe.com/session", "https://billing.stripe.com:8443/session", "file:///tmp/billing.stripe.com", "javascript:alert(1)", "https://stripe.com/pay"] {
            assert!(validate_payment_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn checkout_accepts_existing_subscription_portal_and_defaults_new_checkout_flag() {
        let new: CheckoutResponse = serde_json::from_str(r#"{"url":"https://checkout.stripe.com/pay","session_id":"cs_test"}"#).unwrap();
        assert!(!new.already_subscribed);
        let existing: CheckoutResponse = serde_json::from_str(r#"{"url":"https://billing.stripe.com/session","session_id":"bps_test","already_subscribed":true}"#).unwrap();
        assert!(existing.already_subscribed);
    }
}
