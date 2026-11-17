//! Which copy of the registry the app uses, and what a newer copy must prove
//! before it replaces it (registry/README.md, "rules for readers").
//!
//! - The bundled copy is compiled in and always usable. It is the floor: a
//!   copy with a version lower than the bundled one, or not higher than the
//!   one in use, is never used.
//! - The cached copy is the last remote copy that passed every check. At
//!   launch it is used when its signature still verifies, its
//!   `min_app_version` is satisfied and its version is higher than the
//!   bundled one; otherwise the bundled copy is used.
//! - A remote copy replaces the cache only when its signature verifies over
//!   the exact bytes received, it parses and holds everything this build
//!   reads, its version is higher than the copy in use and than any copy
//!   accepted before, and its `min_app_version` is satisfied.
//!
//! Whatever copy is used, two things always come from the bundled copy:
//! - data terms: a copy may make a provider stricter but never looser, and a
//!   provider the bundled copy does not know is `unknown` ([`apply_floor`]);
//! - every address a key, a message or a download could be sent to (base
//!   URLs, regions, the host names that identify a service, key pages):
//!   README, "the registry's base_url must not change where a user's key is
//!   sent".
//!
//! Rejections are logged with the reason only, never the file's content.

use super::doc::{parse_version, ConsentKeys, DataTerms, Registry};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The registry signing key (registry/signing-public-key.pem, SPKI PEM,
/// Ed25519). Not the app updater's key.
const PUBLIC_KEY_PEM: &str = include_str!("../../../../registry/signing-public-key.pem");

/// registry/registry.json as it was when this build was made.
pub(crate) const BUNDLED_JSON: &str = include_str!("../../../../registry/registry.json");

/// The DER prefix of an Ed25519 SubjectPublicKeyInfo; the 32-byte key follows.
const ED25519_SPKI_PREFIX: [u8; 12] = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];

/// The raw 32-byte key inside an SPKI PEM.
pub(crate) fn raw_public_key(pem: &str) -> Result<[u8; 32], String> {
    let body: String = pem.lines().filter(|line| !line.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(body.trim()).map_err(|_| "the public key is not base64".to_string())?;
    if der.len() != 44 || der[..12] != ED25519_SPKI_PREFIX {
        return Err("the public key is not an Ed25519 key".into());
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&der[12..]);
    Ok(key)
}

/// The key this build trusts.
pub(crate) fn compiled_key() -> [u8; 32] {
    raw_public_key(PUBLIC_KEY_PEM).expect("registry/signing-public-key.pem is an Ed25519 SPKI key")
}

/// Why a copy was not used. Says nothing about the copy's content.
#[derive(Debug, Clone, PartialEq)]
pub enum Rejected {
    Unsigned,
    BadSignature,
    Unreadable(String),
    NotNewer { version: u64, floor: u64 },
    AppTooOld { needs: String },
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejected::Unsigned => write!(f, "no signature"),
            Rejected::BadSignature => write!(f, "the signature does not verify with the key built into this app"),
            Rejected::Unreadable(why) => write!(f, "{why}"),
            Rejected::NotNewer { version, floor } => write!(f, "version {version} is not higher than {floor}"),
            Rejected::AppTooOld { needs } => write!(f, "it needs app version {needs} or later; this is {}", app_version()),
        }
    }
}

pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether `min_app_version` allows this app version.
pub(crate) fn app_satisfies(min_app_version: &str, app: &str) -> bool {
    match (parse_version(min_app_version), parse_version(app)) {
        (Some(min), Some(app)) => app >= min,
        _ => false,
    }
}

/// Verify the exact bytes against a base64 Ed25519 signature.
pub(crate) fn verify(bytes: &[u8], signature_b64: Option<&str>, key: &[u8; 32]) -> Result<(), Rejected> {
    let signature = signature_b64.map(str::trim).filter(|s| !s.is_empty()).ok_or(Rejected::Unsigned)?;
    let signature = base64::engine::general_purpose::STANDARD.decode(signature).map_err(|_| Rejected::BadSignature)?;
    if signature.len() != 64 {
        return Err(Rejected::BadSignature);
    }
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key).verify(bytes, &signature).map_err(|_| Rejected::BadSignature)
}

/// A signed copy, checked in the order the README gives: signature first,
/// over the bytes as received, then the content.
pub(crate) fn accept(bytes: &[u8], signature_b64: Option<&str>, key: &[u8; 32], floor_version: u64, app: &str, bundled: &Registry) -> Result<Registry, Rejected> {
    verify(bytes, signature_b64, key)?;
    let mut registry = Registry::parse(bytes).map_err(Rejected::Unreadable)?;
    if registry.version <= floor_version {
        return Err(Rejected::NotNewer { version: registry.version, floor: floor_version });
    }
    if !app_satisfies(&registry.min_app_version, app) {
        return Err(Rejected::AppTooOld { needs: registry.min_app_version.clone() });
    }
    apply_floor(bundled, &mut registry);
    Ok(registry)
}

const UNKNOWN_TERMS_KEYS: (&str, &str) = ("consentUnknownTermsBlocked", "consentUnknownTermsConfirm");

/// The bundled copy's data terms are the floor, and its addresses are the
/// only addresses. See the module comment.
pub(crate) fn apply_floor(bundled: &Registry, registry: &mut Registry) {
    for provider in &mut registry.providers {
        match bundled.provider(&provider.id) {
            Some(own) => {
                provider.data_terms = provider.data_terms.max(own.data_terms);
                provider.base_url = own.base_url.clone();
                provider.regions = own.regions.clone();
                provider.host_suffixes = own.host_suffixes.clone();
                // A suffix marks a model as not on this Mac; a copy may add
                // one but never remove one.
                for suffix in &own.model_id_suffixes {
                    if !provider.model_id_suffixes.contains(suffix) {
                        provider.model_id_suffixes.push(suffix.clone());
                    }
                }
                if let Some(setup) = provider.setup.as_mut() {
                    setup.key_page = own.setup.as_ref().and_then(|s| s.key_page.clone());
                }
                if provider.data_terms != DataTerms::Safe && provider.consent_copy_keys.is_none() {
                    provider.consent_copy_keys = own.consent_copy_keys.clone();
                }
            }
            None => {
                provider.data_terms = DataTerms::Unknown;
                provider.base_url = None;
                provider.regions.clear();
                provider.host_suffixes.clear();
                // Setup offers only the services this build has code for.
                provider.setup = None;
            }
        }
        if provider.data_terms != DataTerms::Safe && provider.consent_copy_keys.is_none() {
            provider.consent_copy_keys = Some(ConsentKeys { blocked: UNKNOWN_TERMS_KEYS.0.into(), confirm: UNKNOWN_TERMS_KEYS.1.into() });
        }
    }
    // A provider the copy left out keeps the bundled entry's terms and
    // addresses: dropping the entry must not make it safe.
    for own in &bundled.providers {
        if registry.provider(&own.id).is_none() {
            registry.providers.push(own.clone());
        }
    }
}

/// The last remote copy that passed every check, as stored on disk: the
/// bytes exactly as received (base64), the signature header, the ETag, and
/// the highest version ever accepted. One file, replaced by rename, so a
/// crash never leaves a body without its signature.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct Cache {
    pub body: String,
    pub signature: String,
    pub etag: Option<String>,
    pub highest_version: u64,
}

const CACHE_FILE: &str = "registry-cache.json";

pub(crate) fn read_cache(dir: &Path) -> Option<Cache> {
    let text = std::fs::read(dir.join(CACHE_FILE)).ok()?;
    match serde_json::from_slice::<Cache>(&text) {
        Ok(cache) => Some(cache),
        Err(_) => {
            log::warn!("registry: the cached copy could not be read; using the bundled copy until the next fetch");
            None
        }
    }
}

pub(crate) fn write_cache(dir: &Path, cache: &Cache) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(CACHE_FILE);
    let temporary = dir.join(format!("{CACHE_FILE}.tmp"));
    let text = serde_json::to_vec(cache).map_err(|e| e.to_string())?;
    std::fs::write(&temporary, text).map_err(|e| format!("write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, &path).map_err(|e| format!("rename to {}: {e}", path.display()))
}

impl Cache {
    pub fn bytes(&self) -> Option<Vec<u8>> {
        base64::engine::general_purpose::STANDARD.decode(&self.body).ok()
    }
}

/// Where the cache lives: the app's data folder, or the native QA folder.
pub(crate) fn cache_dir() -> Option<PathBuf> {
    #[cfg(feature = "native-qa")]
    return crate::native_qa::directory().ok().map(|dir| dir.join("registry"));
    #[cfg(not(feature = "native-qa"))]
    dirs::data_dir().map(|dir| dir.join("jarvis").join("registry"))
}

/// Where the copy in use came from, for Settings > About.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Bundled,
    /// The cached copy, read at launch.
    Cached,
    /// Fetched while this launch was running.
    Remote,
}

/// The copy to start with: the cached copy when it passes every check,
/// else the bundled copy.
pub(crate) fn at_launch(bundled: &Registry, cache: Option<&Cache>, key: &[u8; 32], app: &str) -> (Registry, Source) {
    let Some(cache) = cache else {
        return (bundled.clone(), Source::Bundled);
    };
    let Some(bytes) = cache.bytes() else {
        log::warn!("registry: cached copy not used: it is not stored as base64");
        return (bundled.clone(), Source::Bundled);
    };
    match accept(&bytes, Some(&cache.signature), key, bundled.version, app, bundled) {
        Ok(registry) => (registry, Source::Cached),
        Err(why) => {
            log::warn!("registry: cached copy not used: {why}; using the bundled copy (version {})", bundled.version);
            (bundled.clone(), Source::Bundled)
        }
    }
}
