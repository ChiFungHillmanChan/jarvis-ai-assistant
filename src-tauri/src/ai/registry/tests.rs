//! Load order, signatures, versions, min_app_version, the data-terms floor
//! and the fetch path. Every key pair here is made in memory for the test;
//! no test reaches the network (the transport is a fake).

use super::fetch::{self, Context, Outcome, Reply, Transport};
use super::load::{self, Cache, Rejected, Source};
use super::*;
use base64::Engine;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

struct Key(Ed25519KeyPair);

impl Key {
    fn new() -> Key {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        Key(Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap())
    }
    fn public(&self) -> [u8; 32] {
        self.0.public_key().as_ref().try_into().unwrap()
    }
    fn sign(&self, bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.0.sign(bytes).as_ref())
    }
}

/// The bundled registry as JSON, to change and sign.
fn bundled_value() -> Value {
    serde_json::from_str(load::BUNDLED_JSON).unwrap()
}

/// A copy of the bundled registry with a higher version and `change` applied,
/// as the bytes a server would send.
fn newer(by: u64, change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut doc = bundled_value();
    doc["version"] = json!(bundled().version + by);
    change(&mut doc);
    serde_json::to_vec(&doc).unwrap()
}

fn provider<'a>(doc: &'a mut Value, id: &str) -> &'a mut Value {
    doc["providers"].as_array_mut().unwrap().iter_mut().find(|p| p["id"] == id).unwrap()
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jarvis-registry-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cache_of(bytes: &[u8], signature: String) -> Cache {
    let version = serde_json::from_slice::<Value>(bytes).unwrap()["version"].as_u64().unwrap();
    Cache { body: base64::engine::general_purpose::STANDARD.encode(bytes), signature, etag: Some("\"etag-1\"".into()), highest_version: version }
}

const APP: &str = "0.5.11";

// ---------------------------------------------------------------------------
// The bundled copy and the compiled key.

#[test]
fn the_bundled_registry_is_usable() {
    let registry = bundled();
    assert!(registry.check().is_ok());
    assert!(load::app_satisfies(&registry.min_app_version, load::app_version()), "this build can use the copy it ships");
}

#[test]
fn the_bundled_registry_carries_every_limit_the_app_reads() {
    for registry in [bundled(), fixture()] {
        for (provider, field) in super::doc::REQUIRED_LIMITS {
            let limits = serde_json::to_value(&registry.provider(provider).unwrap_or_else(|| panic!("{provider}")).limits).unwrap();
            assert!(!limits[field].is_null(), "providers.{provider}.limits.{field} is missing in version {}", registry.version);
        }
    }
}

#[test]
fn the_compiled_key_is_the_committed_registry_key() {
    let pem = include_str!("../../../../registry/signing-public-key.pem");
    assert_eq!(load::compiled_key(), load::raw_public_key(pem).unwrap());
    assert!(load::raw_public_key("-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----\n").is_err());
}

#[test]
fn a_file_missing_what_the_app_reads_is_not_usable() {
    let mut doc = bundled_value();
    doc["features"]["chat"].as_object_mut().unwrap().remove("gemini");
    let err = Registry::parse(&serde_json::to_vec(&doc).unwrap()).unwrap_err();
    assert_eq!(err, "features.chat.gemini is missing");
    let mut doc = bundled_value();
    doc["presets"]["default"] = json!("nope");
    assert!(Registry::parse(&serde_json::to_vec(&doc).unwrap()).is_err());
    let mut doc = bundled_value();
    doc["models"][0]["status"] = json!("retired");
    let err = Registry::parse(&serde_json::to_vec(&doc).unwrap()).unwrap_err();
    assert!(err.starts_with("not a registry this build can read"), "{err}");
    assert!(!err.contains("retired"), "the reason never quotes the file: {err}");
}

#[test]
fn preset_chains_keep_the_file_order() {
    let registry = bundled();
    let names: Vec<&str> = registry.presets.chains.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names.len(), bundled_value()["presets"]["chains"].as_object().unwrap().len());
    let positions: Vec<usize> = names.iter().map(|n| load::BUNDLED_JSON.find(&format!("\"{n}\": {{")).unwrap()).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{names:?} is not the order of registry.json");
}

// ---------------------------------------------------------------------------
// Load order at launch.

#[test]
fn launch_without_a_cache_uses_the_bundled_copy() {
    let (registry, source) = load::at_launch(&bundled(), None, &Key::new().public(), APP);
    assert_eq!((registry.version, source), (bundled().version, Source::Bundled));
}

#[test]
fn launch_uses_a_cached_copy_that_passes_every_check() {
    let key = Key::new();
    let bytes = newer(1, |doc| doc["models"][0]["display_name"]["en"] = json!("Renamed"));
    let cache = cache_of(&bytes, key.sign(&bytes));
    let (registry, source) = load::at_launch(&bundled(), Some(&cache), &key.public(), APP);
    assert_eq!((registry.version, source), (bundled().version + 1, Source::Cached));
    assert_eq!(registry.models[0].display_name.en, "Renamed");
}

#[test]
fn launch_falls_back_to_the_bundled_copy_for_each_failed_check() {
    let key = Key::new();
    let good = newer(1, |_| {});
    let cases: Vec<(&str, Cache)> = vec![
        ("signed by another key", cache_of(&good, Key::new().sign(&good))),
        ("unsigned", cache_of(&good, String::new())),
        ("changed after signing", {
            let mut cache = cache_of(&good, key.sign(&good));
            let mut tampered = good.clone();
            let at = tampered.len() - 2;
            tampered[at] ^= 1;
            cache.body = base64::engine::general_purpose::STANDARD.encode(&tampered);
            cache
        }),
        ("not newer than the bundled copy", {
            let same = newer(0, |_| {});
            cache_of(&same, key.sign(&same))
        }),
        ("needs a newer app", {
            let bytes = newer(1, |doc| doc["min_app_version"] = json!("99.0.0"));
            cache_of(&bytes, key.sign(&bytes))
        }),
        ("not base64", Cache { body: "%%%".into(), signature: key.sign(&good), etag: None, highest_version: 9 }),
    ];
    for (case, cache) in cases {
        let (registry, source) = load::at_launch(&bundled(), Some(&cache), &key.public(), APP);
        assert_eq!((registry.version, source), (bundled().version, Source::Bundled), "{case}");
    }
}

#[test]
fn an_unreadable_cache_file_is_ignored() {
    let dir = temp_dir();
    std::fs::write(dir.join("registry-cache.json"), b"{ not json").unwrap();
    assert_eq!(load::read_cache(&dir), None);
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// What a copy must prove.

#[test]
fn signature_rejection_unsigned_badly_signed_or_not_over_the_exact_bytes() {
    let key = Key::new();
    let bytes = newer(1, |_| {});
    let b = bundled();
    let floor = b.version;
    assert_eq!(load::accept(&bytes, None, &key.public(), floor, APP, &b).unwrap_err(), Rejected::Unsigned);
    assert_eq!(load::accept(&bytes, Some("  "), &key.public(), floor, APP, &b).unwrap_err(), Rejected::Unsigned);
    assert_eq!(load::accept(&bytes, Some(&Key::new().sign(&bytes)), &key.public(), floor, APP, &b).unwrap_err(), Rejected::BadSignature);
    assert_eq!(load::accept(&bytes, Some("AAAA"), &key.public(), floor, APP, &b).unwrap_err(), Rejected::BadSignature);
    assert_eq!(load::accept(&bytes, Some("not base64!"), &key.public(), floor, APP, &b).unwrap_err(), Rejected::BadSignature);
    // The same registry written differently is other bytes: the signature
    // covers the bytes received, never a re-serialised form.
    let pretty = serde_json::to_vec_pretty(&serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap();
    assert_eq!(load::accept(&pretty, Some(&key.sign(&bytes)), &key.public(), floor, APP, &b).unwrap_err(), Rejected::BadSignature);
    assert!(load::accept(&bytes, Some(&key.sign(&bytes)), &key.public(), floor, APP, &b).is_ok());
}

#[test]
fn lower_or_equal_versions_are_rejected() {
    let key = Key::new();
    let b = bundled();
    for (by, floor) in [(0, b.version), (1, b.version + 1), (1, b.version + 5)] {
        let bytes = newer(by, |_| {});
        let version = b.version + by;
        assert_eq!(
            load::accept(&bytes, Some(&key.sign(&bytes)), &key.public(), floor, APP, &b).unwrap_err(),
            Rejected::NotNewer { version, floor },
            "version {version} against floor {floor}"
        );
    }
}

#[test]
fn min_app_version_gates_a_copy() {
    assert!(load::app_satisfies("0.5.11", "0.5.11"));
    assert!(load::app_satisfies("0.5.11", "0.5.12"));
    assert!(load::app_satisfies("0.5.11", "0.6.0"));
    assert!(load::app_satisfies("0.5.11", "1.0.0-beta.1"));
    assert!(!load::app_satisfies("0.5.12", "0.5.11"));
    assert!(!load::app_satisfies("0.10.0", "0.9.99"), "numbers, not text");
    assert!(!load::app_satisfies("garbage", "0.5.11"), "an unreadable requirement is not met");
    let key = Key::new();
    let b = bundled();
    let bytes = newer(1, |doc| doc["min_app_version"] = json!("0.5.12"));
    assert_eq!(load::accept(&bytes, Some(&key.sign(&bytes)), &key.public(), b.version, "0.5.11", &b).unwrap_err(), Rejected::AppTooOld { needs: "0.5.12".into() });
    assert!(load::accept(&bytes, Some(&key.sign(&bytes)), &key.public(), b.version, "0.5.12", &b).is_ok());
}

// ---------------------------------------------------------------------------
// The policy floor.

fn accepted(change: impl FnOnce(&mut Value)) -> Registry {
    let key = Key::new();
    let bytes = newer(1, change);
    let b = bundled();
    load::accept(&bytes, Some(&key.sign(&bytes)), &key.public(), b.version, APP, &b).unwrap()
}

#[test]
fn a_remote_copy_can_tighten_data_terms_but_never_loosen_them() {
    let registry = accepted(|doc| {
        provider(doc, "gemini")["data_terms"] = json!("safe");
        provider(doc, "gemini")["consent_copy_keys"] = Value::Null;
        provider(doc, "kimi")["data_terms"] = json!("safe");
        provider(doc, "ollama_cloud")["data_terms"] = json!("needs_billing_confirmation");
        provider(doc, "openai")["data_terms"] = json!("unknown");
    });
    let terms = |id: &str| registry.provider(id).unwrap().data_terms;
    assert_eq!(terms("gemini"), DataTerms::NeedsBillingConfirmation, "loosening is ignored");
    assert!(registry.provider("gemini").unwrap().consent_copy_keys.is_some(), "a gated provider keeps its consent keys");
    assert_eq!(terms("kimi"), DataTerms::Unknown);
    assert_eq!(terms("ollama_cloud"), DataTerms::Unknown);
    assert_eq!(terms("openai"), DataTerms::Unknown, "tightening is kept");
    assert!(registry.provider("openai").unwrap().consent_copy_keys.is_some());
    assert_eq!(terms("claude"), DataTerms::Safe);
}

#[test]
fn a_provider_new_in_a_remote_copy_is_unknown_and_has_no_addresses() {
    let registry = accepted(|doc| {
        let mut new = provider(doc, "openai").clone();
        new["id"] = json!("newcomer");
        new["data_terms"] = json!("safe");
        doc["providers"].as_array_mut().unwrap().push(new);
    });
    let newcomer = registry.provider("newcomer").unwrap();
    assert_eq!(newcomer.data_terms, DataTerms::Unknown);
    assert_eq!((newcomer.base_url.clone(), newcomer.setup.clone()), (None, None));
    assert!(newcomer.consent_copy_keys.is_some());
}

#[test]
fn a_limit_left_out_of_a_remote_copy_keeps_the_bundled_value() {
    let registry = accepted(|doc| {
        provider(doc, "claude")["limits"].as_object_mut().unwrap().remove("api_version");
        provider(doc, "bedrock")["limits"]["chat_temperature"] = json!(0.7);
    });
    assert_eq!(registry.limits("claude").api_version, bundled().limits("claude").api_version);
    assert_eq!(registry.limits("bedrock").chat_temperature, Some(0.7), "a copy may change a limit");
}

#[test]
fn downloads_and_the_whisper_model_always_come_from_the_bundled_copy() {
    let whisper = bundled().feature_default("stt", "whisper").unwrap().to_string();
    let registry = accepted(|doc| {
        let entry = doc["models"].as_array_mut().unwrap().iter_mut().find(|m| m["id"] == json!(whisper)).unwrap();
        entry["download"]["url"] = json!("https://attacker.example/model.bin");
        entry["download"]["sha256"] = json!("0".repeat(64));
        let mut other = entry.clone();
        other["id"] = json!("ggml-other");
        doc["models"].as_array_mut().unwrap().push(other);
        doc["features"]["stt"]["whisper"] = json!("ggml-other");
    });
    assert_eq!(registry.feature_default("stt", "whisper"), Some(whisper.as_str()));
    let own = |r: &Registry, id: &str| r.models.iter().find(|m| m.id == id).and_then(|m| m.download.clone());
    assert_eq!(own(&registry, &whisper), own(&bundled(), &whisper));
    assert_eq!(own(&registry, "ggml-other"), None, "no new download address");
}

#[test]
fn a_provider_left_out_of_a_remote_copy_keeps_the_bundled_terms() {
    let registry = accepted(|doc| doc["providers"].as_array_mut().unwrap().retain(|p| p["id"] != "kimi"));
    assert_eq!(registry.provider("kimi").unwrap().data_terms, DataTerms::Unknown);
}

#[test]
fn addresses_always_come_from_the_bundled_copy() {
    let registry = accepted(|doc| {
        provider(doc, "openai")["base_url"] = json!("https://attacker.example/v1");
        provider(doc, "openai")["setup"]["key_page"] = json!("https://attacker.example/keys");
        provider(doc, "kimi")["regions"][0]["base_url"] = json!("https://attacker.example/v1");
        provider(doc, "kimi")["host_suffixes"] = json!(["attacker.example"]);
        provider(doc, "ollama_cloud")["model_id_suffixes"] = json!([":remote"]);
        provider(doc, "openai")["display_name"]["en"] = json!("OpenAI renamed");
    });
    let b = bundled();
    for id in ["openai", "kimi"] {
        let (ours, theirs) = (b.provider(id).unwrap(), registry.provider(id).unwrap());
        assert_eq!(theirs.base_url, ours.base_url, "{id}");
        assert_eq!(theirs.regions, ours.regions, "{id}");
        assert_eq!(theirs.host_suffixes, ours.host_suffixes, "{id}");
        assert_eq!(theirs.setup.as_ref().map(|s| &s.key_page), ours.setup.as_ref().map(|s| &s.key_page), "{id}");
    }
    let suffixes = &registry.provider("ollama_cloud").unwrap().model_id_suffixes;
    assert!(suffixes.contains(&":cloud".to_string()) && suffixes.contains(&"-cloud".to_string()) && suffixes.contains(&":remote".to_string()), "{suffixes:?}");
    assert_eq!(registry.provider("openai").unwrap().display_name.en, "OpenAI renamed", "names and copy may change");
}

// ---------------------------------------------------------------------------
// Fetching.

#[derive(Default)]
struct Fake {
    replies: Mutex<Vec<Result<Reply, String>>>,
    sent_etags: Mutex<Vec<Option<String>>>,
}

impl Fake {
    fn with(replies: Vec<Result<Reply, String>>) -> Fake {
        Fake { replies: Mutex::new(replies.into_iter().rev().collect()), ..Default::default() }
    }
}

#[async_trait::async_trait]
impl Transport for Fake {
    async fn get(&self, _url: &str, if_none_match: Option<&str>) -> Result<Reply, String> {
        self.sent_etags.lock().unwrap().push(if_none_match.map(str::to_string));
        self.replies.lock().unwrap().pop().expect("a reply for every request")
    }
}

fn signed_reply(key: &Key, bytes: Vec<u8>, etag: &str) -> Result<Reply, String> {
    Ok(Reply { status: 200, signature: Some(key.sign(&bytes)), body: bytes, etag: Some(etag.into()) })
}

struct Setup {
    holder: Holder,
    dir: PathBuf,
    key: Key,
}

impl Setup {
    fn new() -> Setup {
        Setup { holder: Holder::new(Active { registry: bundled(), source: Source::Bundled }), dir: temp_dir(), key: Key::new() }
    }
    async fn refresh(&self, transport: &Fake) -> Outcome {
        let b = bundled();
        let cx = Context { holder: &self.holder, bundled: &b, cache_dir: &self.dir, key: self.key.public(), app_version: APP };
        fetch::refresh(transport, "https://registry.test/v1/registry", &cx).await
    }
    /// What the next launch would use.
    fn relaunch(&self) -> (Registry, Source) {
        load::at_launch(&bundled(), load::read_cache(&self.dir).as_ref(), &self.key.public(), APP)
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn offline_startup_uses_the_bundled_copy() {
    let setup = Setup::new();
    let fake = Fake::with(vec![Err("request failed: dns error".into())]);
    let outcome = setup.refresh(&fake).await;
    assert_eq!(outcome, Outcome::Failed("request failed: dns error".into()));
    let active = setup.holder.get();
    assert_eq!((active.registry.version, active.source), (bundled().version, Source::Bundled));
    assert_eq!(setup.relaunch().1, Source::Bundled, "nothing was cached");
    let now = chrono::Utc::now();
    assert_eq!(fetch::next_due(&outcome, now), now + chrono::TimeDelta::minutes(10), "a failure is retried soon");
}

#[tokio::test]
async fn a_newer_signed_copy_is_used_cached_and_used_again_at_the_next_launch() {
    let setup = Setup::new();
    let bytes = newer(1, |doc| doc["models"][0]["prices"]["input"] = json!(0.42));
    let fake = Fake::with(vec![signed_reply(&setup.key, bytes, "\"v2\"")]);
    assert_eq!(setup.refresh(&fake).await, Outcome::Updated { version: bundled().version + 1 });
    let active = setup.holder.get();
    assert_eq!(active.source, Source::Remote);
    assert_eq!(active.registry.models[0].prices.as_ref().unwrap().input, 0.42);
    let (registry, source) = setup.relaunch();
    assert_eq!((registry.version, source), (bundled().version + 1, Source::Cached));
    assert_eq!(load::read_cache(&setup.dir).unwrap().etag.as_deref(), Some("\"v2\""));
}

#[tokio::test]
async fn rejected_copies_change_nothing() {
    let setup = Setup::new();
    let older = newer(0, |_| {});
    let unsigned = newer(1, |_| {});
    let too_new_for_this_app = newer(1, |doc| doc["min_app_version"] = json!("99.0.0"));
    let fake = Fake::with(vec![
        signed_reply(&setup.key, older, "\"a\""),
        Ok(Reply { status: 200, body: unsigned, signature: None, etag: None }),
        signed_reply(&Key::new(), newer(1, |_| {}), "\"b\""),
        signed_reply(&setup.key, too_new_for_this_app, "\"c\""),
        Ok(Reply { status: 503, body: br#"{"error":"registry_unavailable"}"#.to_vec(), signature: None, etag: None }),
    ]);
    assert!(matches!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::NotNewer { .. })));
    assert_eq!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::Unsigned));
    assert_eq!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::BadSignature));
    assert_eq!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::AppTooOld { needs: "99.0.0".into() }));
    assert_eq!(setup.refresh(&fake).await, Outcome::Unavailable { status: 503 });
    let active = setup.holder.get();
    assert_eq!((active.registry.version, active.source), (bundled().version, Source::Bundled));
    assert_eq!(load::read_cache(&setup.dir), None, "nothing was cached");
}

#[tokio::test]
async fn the_etag_is_sent_only_for_the_cached_copy_in_use() {
    let setup = Setup::new();
    let fake = Fake::with(vec![
        signed_reply(&setup.key, newer(1, |_| {}), "\"v2\""),
        Ok(Reply { status: 304, ..Default::default() }),
    ]);
    assert!(matches!(setup.refresh(&fake).await, Outcome::Updated { .. }));
    assert_eq!(setup.refresh(&fake).await, Outcome::NotModified);
    assert_eq!(*fake.sent_etags.lock().unwrap(), vec![None, Some("\"v2\"".to_string())]);

    // A new launch whose cache is set aside (here: a newer app version is
    // needed) sends no ETag, so a 304 cannot keep it from fetching.
    let relaunched = Setup::new();
    let bytes = newer(1, |doc| doc["min_app_version"] = json!("99.0.0"));
    load::write_cache(&relaunched.dir, &cache_of(&bytes, relaunched.key.sign(&bytes))).unwrap();
    let fake = Fake::with(vec![Ok(Reply { status: 304, ..Default::default() })]);
    relaunched.refresh(&fake).await;
    assert_eq!(*fake.sent_etags.lock().unwrap(), vec![None]);
}

#[tokio::test]
async fn a_copy_lower_than_one_accepted_before_is_rejected_even_from_the_bundled_copy() {
    // The cache recorded version +5 as accepted but no longer verifies (here:
    // another key), so the bundled copy is in use. An older signed copy the
    // endpoint might be pointed back at is still refused.
    let setup = Setup::new();
    let bytes = newer(5, |_| {});
    load::write_cache(&setup.dir, &cache_of(&bytes, Key::new().sign(&bytes))).unwrap();
    let fake = Fake::with(vec![signed_reply(&setup.key, newer(3, |_| {}), "\"v4\"")]);
    let version = bundled().version + 3;
    assert_eq!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::NotNewer { version, floor: bundled().version + 5 }));
}

#[test]
fn a_fetch_is_due_on_launch_every_six_hours_and_after_a_wake() {
    use crate::scheduler::engine::Reading;
    use std::time::Duration;
    let start = chrono::Utc::now();
    let at = |wall_secs: i64, awake_secs: u64| Reading { wall: start + chrono::TimeDelta::seconds(wall_secs), awake: Duration::from_secs(awake_secs) };
    // Launch: due at once.
    assert!(fetch::due(at(0, 0), at(0, 0), start));
    let next = fetch::next_due(&Outcome::NotModified, start);
    assert_eq!(next, start + chrono::TimeDelta::hours(6));
    // A minute later, awake the whole time: not due.
    assert!(!fetch::due(at(60, 60), at(0, 0), next));
    // Six hours later: due.
    assert!(fetch::due(at(6 * 3600, 6 * 3600), at(6 * 3600 - 60, 6 * 3600 - 60), next));
    // A wake: wall time moved an hour, awake time one minute.
    assert!(fetch::due(at(3660, 120), at(60, 60), next));
    // An update and a rejected copy wait the full interval; an unavailable endpoint retries soon.
    assert_eq!(fetch::next_due(&Outcome::Updated { version: 2 }, start), next);
    assert_eq!(fetch::next_due(&Outcome::Rejected(Rejected::Unsigned), start), next);
    assert_eq!(fetch::next_due(&Outcome::Unavailable { status: 503 }, start), start + chrono::TimeDelta::minutes(10));
}

#[test]
fn the_endpoint_is_the_launch_value_else_the_compiled_one_and_https_only() {
    let compiled = Some("https://compiled.example/v1/registry");
    assert_eq!(fetch::endpoint_from(None, compiled).as_deref(), compiled);
    assert_eq!(fetch::endpoint_from(Some(" https://dev.example/v1/registry ".into()), compiled).as_deref(), Some("https://dev.example/v1/registry"));
    assert_eq!(fetch::endpoint_from(Some("  ".into()), compiled).as_deref(), compiled, "an empty variable counts as unset");
    assert_eq!(fetch::endpoint_from(Some("http://dev.example/v1/registry".into()), compiled), None, "never plain http");
    assert_eq!(fetch::endpoint_from(None, None), None, "a build without an endpoint fetches nothing");
}
