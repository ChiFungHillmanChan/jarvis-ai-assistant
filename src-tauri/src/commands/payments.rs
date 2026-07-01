use crate::payments::{create_checkout_session, open_billing_portal, CheckoutRequest, Tier};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CheckoutPayload {
    pub url: String,
    pub session_id: String,
}

#[tauri::command]
pub async fn payments_start_checkout(
    tier: String,
    user_id: String,
    email: String,
) -> Result<CheckoutPayload, String> {
    let parsed_tier = match tier.as_str() {
        "pro" => Tier::Pro,
        "team" => Tier::Team,
        other => return Err(format!("invalid tier: {}", other)),
    };

    let resp = create_checkout_session(CheckoutRequest {
        tier: parsed_tier,
        user_id: &user_id,
        email: &email,
    })
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

#[tauri::command]
pub async fn payments_open_portal(user_id: String) -> Result<String, String> {
    let url = open_billing_portal(&user_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Err(e) = open::that(&url) {
        log::warn!("failed to open browser for portal: {}", e);
    }
    Ok(url)
}
