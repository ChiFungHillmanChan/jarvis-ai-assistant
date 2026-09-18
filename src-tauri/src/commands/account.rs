use crate::auth::cognito::{AccountStatus, CognitoSession};
use std::sync::Arc;
use tauri::State;

/// The frontend names providers in lowercase; Cognito expects the identity
/// provider's configured name.
fn identity_provider_for(provider: Option<&str>) -> Result<Option<&'static str>, String> {
    match provider {
        None => Ok(None),
        Some("google") => Ok(Some("Google")),
        Some(other) => Err(format!("unknown sign-in provider: {other}")),
    }
}

#[tauri::command]
pub async fn account_status(
    refresh: Option<bool>,
    session: State<'_, Arc<CognitoSession>>,
) -> Result<AccountStatus, String> {
    Ok(session.status(refresh.unwrap_or(false)).await)
}

#[tauri::command]
pub async fn account_sign_in(
    provider: Option<String>,
    session: State<'_, Arc<CognitoSession>>,
) -> Result<AccountStatus, String> {
    let provider = identity_provider_for(provider.as_deref())?;
    session.sign_in(provider).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn account_sign_out(session: State<'_, Arc<CognitoSession>>) -> Result<(), String> {
    session.sign_out().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::identity_provider_for;

    #[test]
    fn only_google_and_the_hosted_page_are_accepted() {
        assert_eq!(identity_provider_for(None), Ok(None));
        assert_eq!(identity_provider_for(Some("google")), Ok(Some("Google")));
        assert!(identity_provider_for(Some("facebook")).is_err());
    }
}
