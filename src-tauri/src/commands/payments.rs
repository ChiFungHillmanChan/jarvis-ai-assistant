use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::auth::cognito::CognitoSession;
use crate::payments::{create_checkout_session, Tier};
use crate::payments::stripe::{create_portal_session, validate_payment_url, PaymentLocale};

#[derive(Debug, Serialize)]
pub struct CheckoutPayload {
    pub url: String,
    pub session_id: String,
    pub already_subscribed: bool,
    pub browser_opened: bool,
    pub session_generation: u64,
}

#[derive(Debug, Serialize)]
pub struct PaymentPage {
    pub url: String,
    pub browser_opened: bool,
    pub session_generation: u64,
}

fn open_payment_page(url: String, session: &CognitoSession, generation: u64, opener: impl FnOnce(&str) -> Result<(), String>) -> Result<PaymentPage, String> {
    validate_payment_url(&url).map_err(|e| e.to_string())?;
    session.with_current_generation(generation, || {
        let browser_opened = opener(&url).is_ok();
        PaymentPage { url, browser_opened, session_generation: generation }
    }).map_err(|e| e.to_string())
}

fn open_browser(url: &str) -> Result<(), String> {
    open::that(url).map_err(|e| e.to_string())
}

/// Requires a signed-in JARVIS account: the checkout endpoint binds the Stripe
/// customer to the Cognito user in the access token. Returns `not_signed_in`
/// when there is no session, which the UI answers by starting sign-in.
/// `code` is an optional promotion code; a code Stripe does not recognise returns `invalid_code`.
#[tauri::command]
pub async fn payments_start_checkout(
    tier: String,
    locale: Option<PaymentLocale>,
    code: Option<String>,
    session: State<'_, Arc<CognitoSession>>,
) -> Result<CheckoutPayload, String> {
    let parsed_tier = match tier.as_str() {
        "pro" => Tier::Pro,
        "team" => Tier::Team,
        other => return Err(format!("invalid tier: {}", other)),
    };

    // A blank code is a normal checkout; the server trims and limits it again.
    let code = code.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    let generation = session.generation();
    let resp = create_checkout_session(parsed_tier, locale.unwrap_or_default(), code, session.inner(), generation)
        .await
        .map_err(|e| e.to_string())?;

    let page = open_payment_page(resp.url, session.inner(), generation, open_browser)?;
    Ok(CheckoutPayload {
        url: page.url,
        session_id: resp.session_id,
        already_subscribed: resp.already_subscribed,
        browser_opened: page.browser_opened,
        session_generation: generation,
    })
}

#[tauri::command]
pub async fn payments_open_portal(locale: Option<PaymentLocale>, session: State<'_, Arc<CognitoSession>>) -> Result<PaymentPage, String> {
    let generation = session.generation();
    let response = create_portal_session(locale.unwrap_or_default(), session.inner(), generation).await.map_err(|e| e.to_string())?;
    open_payment_page(response.url, session.inner(), generation, open_browser)
}

#[tauri::command]
pub async fn payments_reopen_url(url: String, session_generation: u64, session: State<'_, Arc<CognitoSession>>) -> Result<PaymentPage, String> {
    session.with_current_generation(session_generation, || ()).map_err(|e| e.to_string())?;
    session.access_token().await.map_err(|e| e.to_string())?;
    open_payment_page(url, session.inner(), session_generation, open_browser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_failure_preserves_a_validated_url_and_reports_not_opened() {
        let session = CognitoSession::new(None);
        let result = open_payment_page("https://checkout.stripe.com/c/pay/test".into(), &session, session.generation(), |_| Err("no browser".into())).unwrap();
        assert!(!result.browser_opened);
        assert_eq!(result.url, "https://checkout.stripe.com/c/pay/test");
    }

    #[test]
    fn unsafe_url_never_reaches_the_browser_opener() {
        let session = CognitoSession::new(None);
        let result = open_payment_page("https://evil.test/".into(), &session, session.generation(), |_| panic!("must not open"));
        assert!(result.is_err());
    }

    #[test]
    fn a_page_from_another_session_never_reaches_the_browser_opener() {
        let session = CognitoSession::new(None);
        let result = open_payment_page("https://billing.stripe.com/p/session/old".into(), &session, session.generation().wrapping_add(1), |_| panic!("stale page must not open"));
        assert_eq!(result.unwrap_err(), "not_signed_in");
    }
}
