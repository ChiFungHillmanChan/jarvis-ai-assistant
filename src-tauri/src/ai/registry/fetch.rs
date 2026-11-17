//! The signed remote copy: `GET /v1/registry` (infra/README.md). The body is
//! the canonical registry bytes, `X-Registry-Signature` the base64 Ed25519
//! signature over exactly those bytes, with a strong ETag; 304 when
//! `If-None-Match` matches; 503 when nothing is published.
//!
//! Fetched on launch, every six hours and after the Mac wakes, always in the
//! background: startup never waits for the network. A failed fetch keeps the
//! copy in use and tries again in ten minutes.

use super::load::{self, Cache, Rejected, Source};
use super::{Active, Holder, Registry};
use base64::Engine;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// One reply from the registry endpoint.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub signature: Option<String>,
    pub etag: Option<String>,
}

/// How the request is made, so tests never reach the network.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<Reply, String>;
}

use load::MAX_BODY_BYTES;

pub struct Http {
    client: reqwest::Client,
}

impl Http {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Http { client })
    }
}

#[async_trait::async_trait]
impl Transport for Http {
    async fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<Reply, String> {
        let mut request = self.client.get(url);
        if let Some(etag) = if_none_match {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(|e| format!("request failed: {}", e.without_url()))?;
        let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let signature = header("x-registry-signature");
        let etag = header("etag");
        let status = response.status().as_u16();
        let too_large = || "the reply is too large to be a registry".to_string();
        if response.content_length().is_some_and(|n| n as usize > MAX_BODY_BYTES) {
            return Err(too_large());
        }
        // Read piece by piece and stop at the cap: a reply without a length
        // (chunked) is never buffered beyond it.
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| format!("reading the reply failed: {}", e.without_url()))? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Reply { status, body, signature, etag })
    }
}

/// What one fetch did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// 304, or a signed copy of the version in use: the copy in use is
    /// what the endpoint serves.
    NotModified,
    /// A newer copy passed every check and is now in use and cached.
    Updated { version: u64 },
    /// The endpoint served a copy that is not used, with the reason.
    Rejected(Rejected),
    /// 503 or another status: nothing to use.
    Unavailable { status: u16 },
    /// The request did not complete.
    Failed(String),
}

impl Outcome {
    /// Whether to wait the full interval before the next try.
    fn settled(&self) -> bool {
        matches!(self, Outcome::NotModified | Outcome::Updated { .. } | Outcome::Rejected(_))
    }
}

/// Everything a fetch needs besides the transport.
pub struct Context<'a> {
    pub holder: &'a Holder,
    pub bundled: &'a Registry,
    pub cache_dir: &'a Path,
    pub key: [u8; 32],
    pub app_version: &'a str,
}

/// Fetch once and, when the reply passes every check, cache it and put it
/// in use. Logs the outcome; never logs the content.
pub async fn refresh(transport: &dyn Transport, url: &str, cx: &Context<'_>) -> Outcome {
    let active = cx.holder.get();
    let cache = load::read_cache(cx.cache_dir);
    // The ETag names the cached copy. Send it only while that copy's version
    // is the one in use: after an app update the cache may have been set
    // aside (min_app_version), and a 304 would then keep a copy this build
    // can now use from ever being fetched. A cache set aside because the
    // bundled copy caught up has the version in use, so a 304 for it is
    // still right: the endpoint has nothing newer.
    let etag = cache.as_ref().filter(|c| cache_version(c) == Some(active.registry.version)).and_then(|c| c.etag.clone());
    let outcome = match transport.get(url, etag.as_deref()).await {
        Err(why) => Outcome::Failed(why),
        Ok(reply) if reply.status == 304 => Outcome::NotModified,
        Ok(reply) if reply.status == 200 => {
            // The highest version accepted before is the cached body's,
            // counted only when its signature verifies.
            let highest = cache.as_ref().and_then(|c| load::verified_version(c, &cx.key)).unwrap_or(0);
            let floor = active.registry.version.max(highest).max(cx.bundled.version);
            let store = |reply: &Reply| Cache {
                body: base64::engine::general_purpose::STANDARD.encode(&reply.body),
                signature: reply.signature.clone().unwrap_or_default(),
                etag: reply.etag.clone(),
            };
            match load::accept(&reply.body, reply.signature.as_deref(), &cx.key, floor, cx.app_version, cx.bundled) {
                // Signed, readable and the version in use (a fresh install
                // whose bundled copy is the one published): nothing new.
                // Keep it with its ETag when no verified cache holds this
                // version or a later one (a later one is never overwritten:
                // its version stays the floor), so the next fetch can be
                // answered with a 304.
                Err(Rejected::NotNewer { version, .. }) if version == active.registry.version => {
                    if highest < version {
                        if let Err(why) = load::write_cache(cx.cache_dir, &store(&reply)) {
                            log::warn!("registry: version {version} could not be cached: {why}");
                        }
                    }
                    Outcome::NotModified
                }
                Err(why) => Outcome::Rejected(why),
                Ok(registry) => {
                    let version = registry.version;
                    if let Err(why) = load::write_cache(cx.cache_dir, &store(&reply)) {
                        // Still used for this launch; the next launch starts
                        // from the copy it can read.
                        log::warn!("registry: version {version} could not be cached: {why}");
                    }
                    cx.holder.set(Active { registry: Arc::new(registry), source: Source::Remote });
                    Outcome::Updated { version }
                }
            }
        }
        Ok(reply) => Outcome::Unavailable { status: reply.status },
    };
    match &outcome {
        Outcome::NotModified => log::info!("registry: version {} is current", active.registry.version),
        Outcome::Updated { version } => log::info!("registry: now using version {version} (was {})", active.registry.version),
        Outcome::Rejected(why) => log::warn!("registry: fetched copy not used: {why}; keeping version {}", active.registry.version),
        Outcome::Unavailable { status } => log::warn!("registry: endpoint answered {status}; keeping version {}", active.registry.version),
        Outcome::Failed(why) => log::warn!("registry: fetch failed: {why}; keeping version {}", active.registry.version),
    }
    outcome
}

/// The version of the copy a cache holds, without checking it.
fn cache_version(cache: &Cache) -> Option<u64> {
    let bytes = cache.bytes()?;
    serde_json::from_slice::<serde_json::Value>(&bytes).ok()?.get("version")?.as_u64()
}

/// Six hours between fetches that settled.
pub const INTERVAL: Duration = Duration::from_secs(6 * 3600);
/// After a failure or an unavailable endpoint.
pub const RETRY: Duration = Duration::from_secs(10 * 60);
/// How often the loop looks at the clocks.
const TICK: Duration = Duration::from_secs(60);

/// When the next fetch is due after one that ended with `outcome`.
pub fn next_due(outcome: &Outcome, now: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    let wait = if outcome.settled() { INTERVAL } else { RETRY };
    now + chrono::TimeDelta::from_std(wait).unwrap_or_default()
}

/// Whether to fetch on this tick: the next fetch is due, or the Mac woke
/// (P01's wake signal: the wall clock moved further than the awake clock).
pub fn due(now: crate::scheduler::engine::Reading, previous: crate::scheduler::engine::Reading, next: chrono::DateTime<chrono::Utc>) -> bool {
    now.wall >= next || now.jumped_since(previous)
}

/// The endpoint this build fetches from: `JARVIS_REGISTRY_URL` baked in at
/// compile time, which a debug build (the native QA app included) can
/// replace at launch. None: this build uses the bundled and cached copies
/// only. Release builds carry none until there is a prod endpoint.
pub fn endpoint() -> Option<String> {
    let runtime = if cfg!(debug_assertions) { std::env::var("JARVIS_REGISTRY_URL").ok() } else { None };
    endpoint_from(runtime, option_env!("JARVIS_REGISTRY_URL"))
}

/// The launch-time value when set, else the compiled-in one; https only.
pub(crate) fn endpoint_from(runtime: Option<String>, compiled: Option<&str>) -> Option<String> {
    runtime
        .filter(|url| !url.trim().is_empty())
        .or_else(|| compiled.map(str::to_string))
        .map(|url| url.trim().to_string())
        .filter(|url| url.starts_with("https://"))
}

/// Run the fetch loop for the life of the app. `notify` is called after a
/// newer copy is put in use (the frontend reloads the catalog).
pub fn spawn(notify: impl Fn(u64) + Send + Sync + 'static) {
    let Some(url) = endpoint() else {
        log::info!("registry: no endpoint in this build; using version {}", super::current().version);
        return;
    };
    let Some(cache_dir) = load::cache_dir() else {
        log::warn!("registry: no data folder; using version {}", super::current().version);
        return;
    };
    tauri::async_runtime::spawn(run(url, cache_dir, notify));
}

async fn run(url: String, cache_dir: PathBuf, notify: impl Fn(u64) + Send + Sync + 'static) {
    use crate::scheduler::engine::{Reading, SystemClock};
    let transport = match Http::new() {
        Ok(http) => http,
        Err(why) => {
            log::warn!("registry: no HTTP client ({why}); using version {}", super::current().version);
            return;
        }
    };
    let clock = SystemClock::new();
    let bundled = super::bundled();
    let mut previous = Reading::of(&clock);
    // Launch: the first fetch is due at once.
    let mut next = previous.wall;
    loop {
        let now = Reading::of(&clock);
        if due(now, previous, next) {
            if now.jumped_since(previous) {
                // After a wake the network needs a moment (P01 waits too).
                tokio::time::sleep(Duration::from_secs(crate::scheduler::network::NETWORK_SETTLE_SECS)).await;
            }
            let cx = Context { holder: super::holder(), bundled: &bundled, cache_dir: &cache_dir, key: load::compiled_key(), app_version: load::app_version() };
            let outcome = refresh(&transport, &url, &cx).await;
            if let Outcome::Updated { version } = outcome {
                notify(version);
            }
            next = next_due(&outcome, chrono::Utc::now());
        }
        previous = Reading::of(&clock);
        tokio::time::sleep(TICK).await;
    }
}
