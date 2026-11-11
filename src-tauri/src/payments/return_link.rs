//! `jarvis://billing/return` brings JARVIS forward when the Stripe page in the
//! browser is finished. The link carries no payment result and changes
//! nothing: the account screens recheck the plan with the server when the
//! window regains focus. Anything else that arrives on the scheme is ignored.

use tauri::{AppHandle, Manager, Url};

/// True only for the exact link the website's payment pages open.
pub fn is_billing_return(url: &Url) -> bool {
    url.scheme() == "jarvis"
        && url.host_str() == Some("billing")
        && url.path() == "/return"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
}

/// Handles URLs macOS delivers to the running app.
pub fn open_urls(app: &AppHandle, urls: &[Url]) {
    if !urls.iter().any(is_billing_return) {
        return;
    }
    log::info!("Back from the Stripe page; bringing JARVIS forward");
    if crate::wallpaper::is_active() {
        let _ = crate::wallpaper::raise_for_interaction(app);
    } else if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::is_billing_return;
    use tauri::Url;

    fn check(link: &str) -> bool {
        is_billing_return(&Url::parse(link).expect("test links parse"))
    }

    #[test]
    fn the_payment_pages_link_brings_jarvis_back() {
        assert!(check("jarvis://billing/return"));
        assert!(check("jarvis://billing/return?result=success"));
        assert!(check("jarvis://billing/return?result=cancel"));
        // The scheme is case-insensitive once parsed; the host of a custom
        // scheme is kept as written, and the website always sends lowercase.
        assert!(check("JARVIS://billing/return"));
    }

    #[test]
    fn the_bundle_registers_the_jarvis_scheme() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).expect("tauri.conf.json parses");
        let schemes = &config["plugins"]["deep-link"]["desktop"]["schemes"];
        assert!(
            schemes.as_array().is_some_and(|s| s.iter().any(|v| v == "jarvis")),
            "macOS only routes jarvis:// to the app when the bundle declares it: {schemes}"
        );
    }

    #[test]
    fn any_other_link_is_ignored() {
        for link in [
            "jarvis://billing/return/extra",
            "jarvis://billing/other",
            "jarvis://billing",
            "jarvis://evil/return",
            "jarvis://billing.example.com/return",
            "jarvis://user@billing/return",
            "jarvis://billing:8080/return",
            "https://billing/return",
            "https://jarvis-automation.com/billing/return",
        ] {
            assert!(!check(link), "{link} must not be treated as a billing return");
        }
    }
}
