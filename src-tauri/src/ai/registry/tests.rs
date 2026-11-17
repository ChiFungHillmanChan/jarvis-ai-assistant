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
    Cache { body: base64::engine::general_purpose::STANDARD.encode(bytes), signature, etag: Some("\"etag-1\"".into()) }
}

/// A copy of the bundled registry at exactly `version`.
fn at_version(version: u64) -> Vec<u8> {
    let mut doc = bundled_value();
    doc["version"] = json!(version);
    serde_json::to_vec(&doc).unwrap()
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
        ("not base64", Cache { body: "%%%".into(), signature: key.sign(&good), etag: None }),
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

/// The cache is read only up to the size one published copy can take; a
/// larger file is not read on and is not used.
#[test]
fn a_cache_file_too_large_to_be_a_copy_is_not_read() {
    let dir = temp_dir();
    let key = Key::new();
    let bytes = newer(1, |_| {});
    let mut cache = serde_json::to_value(cache_of(&bytes, key.sign(&bytes))).unwrap();
    load::write_cache(&dir, &serde_json::from_value(cache.clone()).unwrap()).unwrap();
    assert!(load::read_cache(&dir).is_some(), "a copy of normal size is read");
    // The same copy with padding: valid JSON, a verifying body, far too large.
    cache["padding"] = json!("x".repeat(load::MAX_BODY_BYTES * 2));
    std::fs::write(dir.join("registry-cache.json"), serde_json::to_vec(&cache).unwrap()).unwrap();
    assert_eq!(load::read_cache(&dir), None);
    let _ = std::fs::remove_dir_all(dir);
}

/// Offline at launch with nothing cached: the copy in use is the bundled
/// one, and nothing is fetched (there is no transport at launch at all).
#[test]
fn launch_with_an_empty_cache_folder_uses_the_bundled_copy() {
    let dir = temp_dir();
    let holder = Holder::new(Active { registry: Arc::new(with_models(|m| m.clear())), source: Source::Remote });
    super::start_in(&holder, Some(&dir));
    let active = holder.get();
    assert_eq!((active.registry.version, active.source), (bundled().version, Source::Bundled));
    assert_eq!(*active.registry, *bundled());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing was written");
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

/// Every provider loosened to safe, then every provider tightened to
/// unknown: each ends at the stricter of the copy's terms and the bundled
/// copy's, whatever the bundled copy holds. The fixture, with one provider
/// at each level, shows every pair.
#[test]
fn a_remote_copy_can_tighten_data_terms_but_never_loosen_them() {
    let set_all = |terms: &'static str| {
        move |doc: &mut Value| {
            for p in doc["providers"].as_array_mut().unwrap() {
                p["data_terms"] = json!(terms);
                if terms == "safe" {
                    p["consent_copy_keys"] = Value::Null;
                }
            }
        }
    };
    let (loosened, tightened) = (accepted(set_all("safe")), accepted(set_all("unknown")));
    for own in &bundled().providers {
        let id = own.id.as_str();
        let (loose, tight) = (loosened.provider(id).unwrap(), tightened.provider(id).unwrap());
        assert_eq!(loose.data_terms, own.data_terms, "{id}: loosening is ignored");
        assert_eq!(loose.consent_copy_keys.is_some(), own.data_terms != DataTerms::Safe, "{id}: a gated provider keeps its consent keys");
        assert_eq!(tight.data_terms, DataTerms::Unknown, "{id}: tightening is kept");
        assert!(tight.consent_copy_keys.is_some(), "{id}");
    }
    let levels = [DataTerms::Safe, DataTerms::NeedsBillingConfirmation, DataTerms::Unknown];
    let mut own = (*fixture()).clone();
    for (p, level) in own.providers.iter_mut().zip(levels.iter().cycle()) {
        p.data_terms = *level;
    }
    for theirs in levels {
        let mut copy = own.clone();
        for p in &mut copy.providers {
            p.data_terms = theirs;
        }
        load::apply_floor(&own, &mut copy);
        for (got, ours) in copy.providers.iter().zip(&own.providers) {
            assert_eq!(got.data_terms, ours.data_terms.max(theirs), "{}: {:?} over {:?}", ours.id, theirs, ours.data_terms);
        }
    }
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

/// Every address a copy names is changed: each provider's base URL,
/// regions, host names, key page and model-id suffixes. Each ends as the
/// bundled copy has it (suffixes: the bundled ones plus any added), whatever
/// the bundled copy holds; the fixture, which has every kind, shows each.
#[test]
fn addresses_always_come_from_the_bundled_copy() {
    fn tamper(doc: &mut Value) {
        for p in doc["providers"].as_array_mut().unwrap() {
            p["base_url"] = json!("https://attacker.example/v1");
            p["host_suffixes"] = json!(["attacker.example"]);
            p["model_id_suffixes"] = json!([":remote"]);
            if let Some(regions) = p.get_mut("regions").and_then(Value::as_array_mut) {
                for region in regions {
                    region["base_url"] = json!("https://attacker.example/v1");
                }
            }
            if p["setup"].is_object() {
                p["setup"]["key_page"] = json!("https://attacker.example/keys");
            }
            p["display_name"]["en"] = json!("Renamed");
        }
    }
    let check = |own: &Registry, got: &Registry| {
        for ours in &own.providers {
            let id = ours.id.as_str();
            let theirs = got.provider(id).unwrap();
            assert_eq!(theirs.base_url, ours.base_url, "{id}");
            assert_eq!(theirs.regions, ours.regions, "{id}");
            assert_eq!(theirs.host_suffixes, ours.host_suffixes, "{id}");
            assert_eq!(theirs.setup.as_ref().map(|s| &s.key_page), ours.setup.as_ref().map(|s| &s.key_page), "{id}");
            let mut expected: Vec<&str> = ours.model_id_suffixes.iter().map(String::as_str).chain([":remote"]).collect();
            let mut got: Vec<&str> = theirs.model_id_suffixes.iter().map(String::as_str).collect();
            (expected.sort(), expected.dedup(), got.sort());
            assert_eq!(got, expected, "{id}: a copy may add a suffix but never remove one");
            assert_eq!(theirs.display_name.en, "Renamed", "{id}: names and copy may change");
        }
    };
    check(&bundled(), &accepted(tamper));
    let own = fixture();
    let mut doc: Value = serde_json::from_slice(include_bytes!("fixture.json")).unwrap();
    tamper(&mut doc);
    let mut copy = Registry::parse(&serde_json::to_vec(&doc).unwrap()).unwrap();
    load::apply_floor(&own, &mut copy);
    check(&own, &copy);
    let has = |f: &dyn Fn(&Provider) -> bool| own.providers.iter().any(f);
    assert!(has(&|p| p.base_url.is_some()) && has(&|p| !p.regions.is_empty()) && has(&|p| !p.host_suffixes.is_empty()), "the fixture has every kind of address");
    assert!(has(&|p| p.setup.as_ref().is_some_and(|s| s.key_page.is_some())) && has(&|p| !p.model_id_suffixes.is_empty()));
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
    let older = at_version(bundled().version - 1);
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
    // Version +5 was accepted and cached, and its signature still verifies,
    // but this app cannot use it (it needs a newer app), so the bundled copy
    // is in use. An older signed copy the endpoint might be pointed back at
    // is still refused: the verified cached body's version is the floor.
    let setup = Setup::new();
    let bytes = newer(5, |doc| doc["min_app_version"] = json!("99.0.0"));
    load::write_cache(&setup.dir, &cache_of(&bytes, setup.key.sign(&bytes))).unwrap();
    assert_eq!(setup.relaunch().1, Source::Bundled, "the cached copy is set aside");
    let fake = Fake::with(vec![signed_reply(&setup.key, newer(3, |_| {}), "\"v4\"")]);
    let version = bundled().version + 3;
    assert_eq!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::NotNewer { version, floor: bundled().version + 5 }));
    assert_eq!(load::verified_version(&load::read_cache(&setup.dir).unwrap(), &setup.key.public()), Some(bundled().version + 5), "the cache is kept");
    // The same cache under another key verifies nothing and is no floor.
    let other = Setup::new();
    load::write_cache(&other.dir, &cache_of(&bytes, Key::new().sign(&bytes))).unwrap();
    let fake = Fake::with(vec![signed_reply(&other.key, newer(3, |_| {}), "\"v4\"")]);
    assert_eq!(other.refresh(&fake).await, Outcome::Updated { version });
}

/// The cache file's own claims count for nothing: a file that says a huge
/// version was accepted, with a body that does not verify, does not block
/// a valid newer copy.
#[tokio::test]
async fn a_cache_claiming_a_huge_version_without_a_verified_body_blocks_nothing() {
    let setup = Setup::new();
    let huge = at_version(u64::MAX);
    let mut file = serde_json::to_value(cache_of(&huge, Key::new().sign(&huge))).unwrap();
    file["highest_version"] = json!(u64::MAX);
    std::fs::write(setup.dir.join("registry-cache.json"), serde_json::to_vec(&file).unwrap()).unwrap();
    let fake = Fake::with(vec![signed_reply(&setup.key, newer(1, |_| {}), "\"v2\"")]);
    assert_eq!(setup.refresh(&fake).await, Outcome::Updated { version: bundled().version + 1 });
}

/// A fresh install whose bundled copy is the one published: the endpoint's
/// signed copy of the version in use is "not modified" (logged as current,
/// not as a rejection), kept with its ETag, and the next fetch sends that
/// ETag so it can be answered with a 304. A lower version is still refused.
#[tokio::test]
async fn a_reply_of_the_version_in_use_is_not_modified_and_its_etag_is_sent_next() {
    let setup = Setup::new();
    let same = at_version(bundled().version);
    let fake = Fake::with(vec![
        signed_reply(&setup.key, same.clone(), "\"v1\""),
        Ok(Reply { status: 304, ..Default::default() }),
        signed_reply(&setup.key, at_version(bundled().version - 1), "\"v0\""),
        Ok(Reply { status: 200, body: same.clone(), signature: Some(Key::new().sign(&same)), etag: None }),
    ]);
    assert_eq!(setup.refresh(&fake).await, Outcome::NotModified);
    let active = setup.holder.get();
    assert_eq!((active.registry.version, active.source), (bundled().version, Source::Bundled), "nothing changes");
    assert_eq!(load::read_cache(&setup.dir).unwrap().etag.as_deref(), Some("\"v1\""));
    assert_eq!(setup.refresh(&fake).await, Outcome::NotModified);
    assert!(matches!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::NotNewer { .. })), "a lower version is never accepted");
    assert_eq!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::BadSignature), "the same version under another key is a rejection");
    assert_eq!(*fake.sent_etags.lock().unwrap(), vec![None, Some("\"v1\"".to_string()), Some("\"v1\"".to_string()), Some("\"v1\"".to_string())]);
    assert_eq!(setup.relaunch().1, Source::Bundled, "the kept copy is not newer than the bundled one");
}

/// A reply with no length is read only up to the cap: an endless body
/// fails at once as too large instead of being buffered until the timeout.
/// The server is in this process, on the loopback address.
#[tokio::test]
async fn a_reply_without_a_length_is_read_only_up_to_the_cap() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/registry", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = socket.read(&mut request).await;
        let _ = socket.write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n").await;
        let chunk = format!("{:x}\r\n{}\r\n", 64 * 1024, "x".repeat(64 * 1024));
        while socket.write_all(chunk.as_bytes()).await.is_ok() {}
    });
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), fetch::Http::new().unwrap().get(&url, None)).await.expect("the read stops at the cap, long before the timeout");
    assert_eq!(result, Err("the reply is too large to be a registry".to_string()));
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
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

// ---------------------------------------------------------------------------
// What a signed copy may hold: bounded limits, safe ids, active defaults,
// and each model's status from its own provider's entries.

/// Each bound of each limit (registry/README.md, "Limits"): a copy just
/// outside is rejected whole, the reason names the field and never the
/// value, and the copy in use stays; a copy just inside is used.
#[tokio::test]
async fn a_limit_outside_its_bound_rejects_the_whole_copy() {
    use super::doc::{MAX_CONTEXT_TOKENS, MAX_OUTPUT_TOKENS, MAX_TOOL_ROUNDS};
    let range = |max: u32| format!("is outside 1 to {max}");
    let cases: Vec<(&str, &str, Value, Value, String)> = vec![
        ("openai", "chat_max_output_tokens", json!(0), json!(1), range(MAX_OUTPUT_TOKENS)),
        ("openai", "chat_max_output_tokens", json!(MAX_OUTPUT_TOKENS + 1), json!(MAX_OUTPUT_TOKENS), range(MAX_OUTPUT_TOKENS)),
        ("gemini", "default_max_output_tokens", json!(0), json!(1), range(MAX_OUTPUT_TOKENS)),
        ("gemini", "default_max_output_tokens", json!(MAX_OUTPUT_TOKENS + 1), json!(MAX_OUTPUT_TOKENS), range(MAX_OUTPUT_TOKENS)),
        ("gemini", "stt_max_output_tokens", json!(0), json!(1), range(MAX_OUTPUT_TOKENS)),
        ("gemini", "stt_max_output_tokens", json!(MAX_OUTPUT_TOKENS + 1), json!(MAX_OUTPUT_TOKENS), range(MAX_OUTPUT_TOKENS)),
        ("ollama", "completion_context_tokens", json!(0), json!(1), range(MAX_CONTEXT_TOKENS)),
        ("ollama", "completion_context_tokens", json!(MAX_CONTEXT_TOKENS + 1), json!(MAX_CONTEXT_TOKENS), range(MAX_CONTEXT_TOKENS)),
        ("ollama", "default_context_tokens", json!(0), json!(1), range(MAX_CONTEXT_TOKENS)),
        ("ollama", "default_context_tokens", json!(MAX_CONTEXT_TOKENS + 1), json!(MAX_CONTEXT_TOKENS), range(MAX_CONTEXT_TOKENS)),
        ("claude", "max_tool_rounds", json!(0), json!(1), range(MAX_TOOL_ROUNDS)),
        ("claude", "max_tool_rounds", json!(MAX_TOOL_ROUNDS + 1), json!(MAX_TOOL_ROUNDS), range(MAX_TOOL_ROUNDS)),
        ("bedrock", "chat_temperature", json!(-0.01), json!(0.0), "is outside 0 to 2".into()),
        ("bedrock", "chat_temperature", json!(2.01), json!(2.0), "is outside 0 to 2".into()),
        ("openai", "chat_reasoning_effort", json!("maximal"), json!("xhigh"), "is not one of none, minimal, low, medium, high, xhigh".into()),
        ("claude", "api_version", json!("2023-6-01"), json!("2023-06-01"), "is not a date (YYYY-MM-DD)".into()),
        ("claude", "api_version", json!("v2023-06-01"), json!("2099-12-31"), "is not a date (YYYY-MM-DD)".into()),
        ("claude", "api_version", json!("2023-06-011"), json!("2023-06-01"), "is not a date (YYYY-MM-DD)".into()),
        ("claude", "api_version", json!("2023-0a-01"), json!("2023-06-01"), "is not a date (YYYY-MM-DD)".into()),
    ];
    for (id, field, outside, inside, why) in cases {
        let case = format!("{id}.{field} = {outside}");
        let setup = Setup::new();
        let fake = Fake::with(vec![
            signed_reply(&setup.key, newer(1, |doc| provider(doc, id)["limits"][field] = outside.clone()), "\"a\""),
            signed_reply(&setup.key, newer(1, |doc| provider(doc, id)["limits"][field] = inside.clone()), "\"b\""),
        ]);
        let reason = format!("providers.{id}.limits.{field} {why}");
        assert_eq!(setup.refresh(&fake).await, Outcome::Rejected(Rejected::Unreadable(reason)), "{case}: rejected, the field named and the value not quoted");
        let active = setup.holder.get();
        assert_eq!((active.registry.version, active.source), (bundled().version, Source::Bundled), "{case}: the copy in use stays");
        assert_eq!(load::read_cache(&setup.dir), None, "{case}: nothing is cached");
        assert_eq!(setup.refresh(&fake).await, Outcome::Updated { version: bundled().version + 1 }, "{id}.{field} = {inside} is inside its bound");
    }
}

#[test]
fn every_model_id_a_copy_sends_is_a_safe_model_id() {
    for id in ["gpt-6-luna", "us.anthropic.claude-haiku-4-5-20251001-v1:0", "qwen3.5:4b", "library/qwen3:8b", "models/gemini-3.7-flash", "Jev_2"] {
        assert!(super::doc::is_safe_model_id(id), "{id}");
    }
    for id in ["", "gemini-3.7-flash/../x", "..", "a..b", "a b", "a?b", "a#b", "a%2fb", "a\\b", "gpt-6-luna\n", "模型"] {
        assert!(!super::doc::is_safe_model_id(id), "{id:?}");
    }
    let bad = "gemini-3.7-flash/../x";
    let rejected = |change: &dyn Fn(&mut Value)| {
        let mut doc = bundled_value();
        change(&mut doc);
        Registry::parse(&serde_json::to_vec(&doc).unwrap()).unwrap_err()
    };
    let chain = bundled().presets.chains.iter().position(|(_, c)| c.entries.iter().any(|e| e.model.is_some())).expect("a preset entry names a model");
    let (name, entry) = {
        let (name, c) = &bundled().presets.chains[chain];
        (name.clone(), c.entries.iter().position(|e| e.model.is_some()).unwrap())
    };
    let cases: Vec<(String, Box<dyn Fn(&mut Value)>)> = vec![
        ("features.chat.gemini is not a safe model id".into(), Box::new(|doc| doc["features"]["chat"]["gemini"] = json!(bad))),
        (format!("presets.chains.{name}[{entry}] is not a safe model id"), Box::new(|doc| doc["presets"]["chains"][&name]["entries"][entry]["model"] = json!(bad))),
        ("a openai model's id or replacement is not a safe model id".into(), Box::new(|doc| push_openai_like(doc, json!({ "id": "gpt?x" })))),
        ("a openai model's id or replacement is not a safe model id".into(), Box::new(|doc| push_openai_like(doc, json!({ "id": "gpt-extra", "status": "deprecated", "replacement": "../gpt" })))),
        ("providers.bedrock.built_in_models holds an id that is not a safe model id".into(), Box::new(|doc| provider(doc, "bedrock")["built_in_models"][0] = json!("us.anthropic.claude#x"))),
    ];
    for (reason, change) in cases {
        assert_eq!(rejected(&*change), reason);
        assert!(!reason.contains(bad) && !reason.contains('?') && !reason.contains('#'), "the reason never quotes the id: {reason}");
    }
}

/// A new entry like the bundled copy's first OpenAI entry, with `fields` set.
fn push_openai_like(doc: &mut Value, fields: Value) {
    let mut entry = doc["models"].as_array().unwrap().iter().find(|m| m["provider"] == "openai").unwrap().clone();
    for (key, value) in fields.as_object().unwrap() {
        entry[key] = value.clone();
    }
    doc["models"].as_array_mut().unwrap().push(entry);
}

/// A default or a preset entry is sent when the user saved no model, so a
/// copy that names one that is not active is not used.
#[test]
fn a_copy_whose_default_or_preset_entry_is_not_active_is_not_used() {
    let b = bundled();
    let parse = |doc: &Value| Registry::parse(&serde_json::to_vec(doc).unwrap());
    for status in ["deprecated", "disabled"] {
        // The model the OpenAI chat default names, by the app's own lookup.
        let default = b.feature_default("chat", "openai").unwrap().to_string();
        let target = b.own_entry("openai", &default).unwrap().id.clone();
        let mut doc = bundled_value();
        let m = doc["models"].as_array_mut().unwrap().iter_mut().find(|m| m["id"] == json!(target)).unwrap();
        m["status"] = json!(status);
        let err = parse(&doc).unwrap_err();
        assert!(err.starts_with("features.") && err.ends_with(" names a model that is not active"), "{status}: {err}");

        // A preset entry: a Gemini model no default names, put in a chain.
        let named: Vec<&str> = b.features.values().flat_map(|f| f.values()).map(String::as_str).collect();
        let spare = b.models.iter().find(|m| m.provider == "gemini" && m.kind == super::doc::Kind::Model && m.status == Status::Active && !named.iter().any(|d| d.contains(m.id.as_str()) || m.id.contains(*d))).expect("a Gemini model no default names").id.clone();
        let (name, _) = &b.presets.chains[0];
        let mut doc = bundled_value();
        let entries = doc["presets"]["chains"][name]["entries"].as_array_mut().unwrap();
        entries[0] = json!({ "provider": "gemini", "model": spare });
        assert!(parse(&doc).is_ok(), "an active preset entry is used");
        doc["models"].as_array_mut().unwrap().iter_mut().find(|m| m["id"] == json!(spare)).unwrap()["status"] = json!(status);
        assert_eq!(parse(&doc).unwrap_err(), format!("presets.chains.{name}[0] names a model that is not active"), "{status}");
    }
}

/// A copy with `change` applied to its models, for the model-state cases.
fn with_models(change: impl FnOnce(&mut Vec<Model>)) -> Registry {
    let mut registry = (*fixture()).clone();
    change(&mut registry.models);
    registry
}

fn set_status(models: &mut [Model], id: &str, status: Status, replacement: Option<&str>) {
    let m = models.iter_mut().find(|m| m.id == id).unwrap_or_else(|| panic!("{id}"));
    m.status = status;
    m.replacement = replacement.map(str::to_string);
}

/// An entry like `like`, with another id and provider.
fn entry_like(models: &[Model], like: &str, id: &str, provider: &str, status: Status) -> Model {
    let mut m = models.iter().find(|m| m.id == like).unwrap().clone();
    (m.id, m.provider, m.status, m.replacement) = (id.into(), provider.into(), status, None);
    m
}

#[test]
fn a_model_takes_its_status_only_from_its_own_providers_entries() {
    // Anthropic's API models disabled: every Bedrock id that contains one
    // keeps running, and the Anthropic API model itself stops.
    let registry = with_models(|models| {
        for id in ["claude-sonnet-4-6", "claude-haiku-4-5", "sonnet-4.6", "haiku-4.5"] {
            set_status(models, id, Status::Disabled, None);
        }
    });
    let bedrock = registry.provider("bedrock").unwrap();
    let mut ids: Vec<String> = bedrock.built_in_models.clone();
    ids.push(registry.feature_default("chat", "bedrock").unwrap().into());
    ids.extend(["us.anthropic.claude-sonnet-4-6", "eu.anthropic.claude-sonnet-4-6-v1:0", "apac.anthropic.claude-haiku-4-5-20251001-v1:0", "global.anthropic.claude-haiku-4-5-20251001-v1:0"].map(String::from));
    assert!(ids.iter().any(|id| id.contains("claude-sonnet-4-6")) && ids.iter().any(|id| id.contains("claude-haiku-4-5")), "{ids:?}");
    for id in &ids {
        assert_eq!(registry.model_state("bedrock", id), ModelState::Active, "{id}");
    }
    assert!(matches!(registry.model_state("claude", "claude-sonnet-4-6"), ModelState::Disabled { .. }));
    assert!(matches!(fixture().model_state("bedrock", "global.anthropic.claude-sonnet-4-6"), ModelState::Active));

    // A Bedrock entry disabled stops its own ids, with the replacement it names.
    let registry = with_models(|models| set_status(models, "nova-pro", Status::Disabled, Some("nova-lite")));
    for id in ["nova-pro", "amazon.nova-pro-v1:0", "apac.amazon.nova-pro-v1:0", "us.amazon.nova-pro-v1:0"] {
        assert_eq!(registry.model_state("bedrock", id), ModelState::Disabled { replacement: Some("nova-lite".into()) }, "{id}");
    }
    assert_eq!(registry.model_state("bedrock", "apac.amazon.nova-lite-v1:0"), ModelState::Active);

    // The other directions: a Bedrock entry never stops an Anthropic API
    // id, a Gemini entry never a Gemini-compatible one or the reverse, and
    // an OpenAI entry never a compatible service's or the reverse.
    let registry = with_models(|models| {
        let bedrock_only = entry_like(models, "nova-pro", "future-9", "bedrock", Status::Disabled);
        let compat_gemini = entry_like(models, "gemini-3.6-flash", "gemini-2.0-x", "gemini_compat", Status::Disabled);
        let compat_openai = entry_like(models, "gpt-4o", "gpt-oss-x", "openai_compat", Status::Disabled);
        models.extend([bedrock_only, compat_gemini, compat_openai]);
        set_status(models, "gemini-3.6-flash", Status::Disabled, None);
        set_status(models, "gpt-4o", Status::Disabled, None);
    });
    assert!(matches!(registry.model_state("bedrock", "us.vendor.future-9-v1:0"), ModelState::Disabled { .. }), "the Bedrock entry stops Bedrock");
    assert_eq!(registry.model_state("claude", "claude-future-9"), ModelState::Active);
    assert_eq!(registry.model_state("gemini", "gemini-2.0-x"), ModelState::Active);
    assert_eq!(registry.model_state("gemini_compat", "gemini-3.6-flash"), ModelState::Active);
    assert_eq!(registry.model_state("openai", "gpt-oss-x"), ModelState::Active);
    for provider in ["openai_compat", "kimi", "qwen"] {
        assert_eq!(registry.model_state(provider, "gpt-4o"), ModelState::Active, "{provider}");
    }
    assert!(matches!(registry.model_state("gemini_compat", "gemini-2.0-x"), ModelState::Disabled { .. }));
    assert!(matches!(registry.model_state("openai", "gpt-4o"), ModelState::Disabled { .. }));
}

/// The replacement offered is always active: the named one when it is an
/// active entry of the same provider, else the chat default when that is.
#[test]
fn the_replacement_offered_is_always_active() {
    let default = fixture().feature_default("chat", "openai").unwrap().to_string();
    assert_ne!(default, "gpt-5.6-luna");
    let state = |change: &dyn Fn(&mut Vec<Model>)| with_models(|m| change(m)).model_state("openai", "gpt-5.6-luna");
    assert_eq!(state(&|m| set_status(m, "gpt-5.6-luna", Status::Disabled, Some("gpt-4o"))), ModelState::Disabled { replacement: Some("gpt-4o".into()) });
    // The named replacement is not active: the chat default instead.
    assert_eq!(state(&|m| {
        set_status(m, "gpt-5.6-luna", Status::Disabled, Some("gpt-4o"));
        set_status(m, "gpt-4o", Status::Disabled, None);
    }), ModelState::Disabled { replacement: Some(default.clone()) });
    // Nor is the chat default: nothing is offered.
    assert_eq!(state(&|m| {
        set_status(m, "gpt-5.6-luna", Status::Disabled, None);
        set_status(m, &default, Status::Deprecated, Some("gpt-4o"));
    }), ModelState::Disabled { replacement: None });
    assert_eq!(state(&|m| set_status(m, "gpt-5.6-luna", Status::Disabled, None)), ModelState::Disabled { replacement: Some(default.clone()) });
}
