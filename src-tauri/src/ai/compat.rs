//! OpenAI-compatible cloud services (Kimi, Qwen and others) used as the
//! user's one chosen AI. They are `openai` chain rows with a `base_url`:
//! same wire format, native tool calling, their own key in the Keychain.
use reqwest::Url;

// The services offered in setup, their regions and their hosts are the
// registry's (`providers[].regions`, `host_suffixes`), always as this build
// shipped them. The user can also enter any other base URL.

/// A base URL the app may send the user's key and messages to: https, or
/// plain http only to this Mac. No credentials, query or fragment, so the
/// saved value is exactly where requests go. Trailing slashes are dropped.
pub fn normalize_base_url(value: &str) -> Result<String, String> {
    let invalid = || "服務網址無效，請輸入以 https:// 開頭的完整網址。".to_string();
    let url = Url::parse(value.trim()).map_err(|_| invalid())?;
    let host = url.host_str().ok_or_else(invalid)?;
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1");
    let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    if !scheme_ok || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err(invalid());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub fn host(base_url: &str) -> String {
    Url::parse(base_url).ok().and_then(|u| u.host_str().map(str::to_lowercase)).unwrap_or_default()
}

/// Keychain account for a service's key. One key per host: Kimi's global
/// and China endpoints are separate accounts, as they are for the vendor.
pub fn key_account(base_url: &str) -> String {
    format!("compat_api_key:{}", host(base_url))
}

/// The registry provider whose host this base URL is on (Kimi, Qwen,
/// Gemini's compatible endpoint), matched on `host_suffixes`. None for any
/// other service.
pub fn provider_for(base_url: &str) -> Option<&'static str> {
    let host = host(base_url);
    let registry = super::registry::current();
    let found = registry.providers.iter().find(|p| p.host_suffixes.iter().any(|suffix| host.ends_with(suffix.as_str())))?;
    // A &'static name for the few ids this build has code for; any other
    // provider is "another service".
    ["kimi", "qwen", "gemini_compat"].into_iter().find(|id| *id == found.id)
}

/// Whether the base URL is OpenAI's own API (the registry's `openai` address).
pub fn is_openai(base_url: &str) -> bool {
    let registry = super::registry::current();
    registry.provider("openai").and_then(|p| p.base_url.as_deref()).is_some_and(|own| host(own) == host(base_url))
}

/// Name shown to the user and written to the cost table.
pub fn label(base_url: &str) -> String {
    match provider_for(base_url) {
        Some(id) => super::registry::current().provider(id).map(|p| p.display_name.en.clone()).unwrap_or_else(|| id.to_string()),
        None => host(base_url),
    }
}

/// The key for `base_url`, from `JARVIS_COMPAT_API_KEY` (development only)
/// or the Keychain. Blocking: call from spawn_blocking or before any lock.
pub fn resolve_key(base_url: &str) -> Option<String> {
    if let Some(key) = std::env::var("JARVIS_COMPAT_API_KEY").ok().filter(|k| !k.trim().is_empty()) {
        return Some(key.trim().to_string());
    }
    crate::security::keychain::KeychainStore::get(super::KEY_SERVICE, &key_account(base_url))
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(90))
        // A redirect would carry the key and the messages somewhere the user
        // never entered.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "未能建立網絡連線。".to_string())
}

/// Model ids the service offers to this key, from its own `/models` list,
/// so setup never guesses a model name.
pub async fn list_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let base = normalize_base_url(base_url)?;
    let response = client()?
        .get(format!("{base}/models"))
        .bearer_auth(api_key.trim())
        .send()
        .await
        .map_err(|_| format!("未能連接 {}，請檢查網絡及網址。", label(&base)))?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err(format!("{} 拒絕了這個 API 密鑰，請檢查後重試。", label(&base))),
        status => return Err(format!("{} 暫時未能列出模型（{status}）。", label(&base))),
    }
    let body: serde_json::Value = response.json().await.map_err(|_| "服務回覆的模型清單格式無效。".to_string())?;
    let mut ids: Vec<String> = body
        .get("data")
        .and_then(|d| d.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("id").and_then(|id| id.as_str()))
        .map(|id| id.trim_start_matches("models/").to_string())
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err("這個服務沒有列出可用模型。".into());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIMI_GLOBAL: &str = "https://api.moonshot.ai/v1";
    const KIMI_CHINA: &str = "https://api.moonshot.cn/v1";
    const QWEN_INTERNATIONAL: &str = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1";
    const QWEN_CHINA: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";

    #[test]
    fn base_urls_are_https_or_this_mac_and_nothing_else() {
        assert_eq!(normalize_base_url(" https://api.moonshot.ai/v1/ ").unwrap(), KIMI_GLOBAL);
        assert_eq!(normalize_base_url(QWEN_INTERNATIONAL).unwrap(), QWEN_INTERNATIONAL);
        assert_eq!(normalize_base_url("http://127.0.0.1:1234/v1").unwrap(), "http://127.0.0.1:1234/v1");
        for bad in ["http://api.moonshot.ai/v1", "https://user:pw@api.example.com/v1", "https://api.example.com/v1?x=1", "https://api.example.com/v1#f", "ftp://x", "api.moonshot.ai", ""] {
            assert!(normalize_base_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn keys_and_labels_follow_the_host() {
        assert_eq!(key_account(KIMI_GLOBAL), "compat_api_key:api.moonshot.ai");
        assert_ne!(key_account(KIMI_GLOBAL), key_account(KIMI_CHINA));
        assert_eq!(label(KIMI_CHINA), "Kimi");
        assert_eq!(label(QWEN_CHINA), "Qwen");
        assert_eq!(label("https://api.deepseek.com/v1"), "api.deepseek.com");
        assert_eq!(label("https://generativelanguage.googleapis.com/v1beta/openai"), "Gemini");
        assert_eq!(provider_for(QWEN_INTERNATIONAL), Some("qwen"));
        assert_eq!(provider_for("https://api.deepseek.com/v1"), None);
        assert!(is_openai("https://api.openai.com/v1") && !is_openai(KIMI_GLOBAL));
    }

    /// The services, their regions and their names are the registry's.
    #[test]
    fn the_services_follow_the_registry() {
        let registry = crate::ai::registry::current();
        for id in ["kimi", "qwen"] {
            let provider = registry.provider(id).unwrap();
            assert!(!provider.regions.is_empty(), "{id}");
            for region in &provider.regions {
                assert_eq!(provider_for(&region.base_url), Some(id), "{}", region.base_url);
                assert_eq!(label(&region.base_url), provider.display_name.en);
            }
        }
        let _renamed = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "kimi").unwrap().display_name.en = "Moonshot".into());
        assert_eq!(label(KIMI_GLOBAL), "Moonshot");
    }
}
