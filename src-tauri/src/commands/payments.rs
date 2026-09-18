use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::auth::cognito::CognitoSession;
use crate::payments::{create_checkout_session, Tier};

#[derive(Debug, Serialize)]
pub struct CheckoutPayload {
    pub url: String,
    pub session_id: String,
}

/// Requires a signed-in JARVIS account: the checkout endpoint binds the Stripe
/// customer to the Cognito user in the access token. Returns `not_signed_in`
/// when there is no session, which the UI answers by starting sign-in.
#[tauri::command]
pub async fn payments_start_checkout(
    tier: String,
    session: State<'_, Arc<CognitoSession>>,
) -> Result<CheckoutPayload, String> {
    let parsed_tier = match tier.as_str() {
        "pro" => Tier::Pro,
        "team" => Tier::Team,
        other => return Err(format!("invalid tier: {}", other)),
    };

    let resp = create_checkout_session(parsed_tier, session.inner())
        .await
        .map_err(|e| e.to_string())?;

    if let Err(e) = open::that(&resp.url) {
        log::warn!("failed to open browser for checkout: {}", e);
    }

    Ok(CheckoutPayload {
        url: resp.url,
        session_id: resp.session_id,
    })
}
