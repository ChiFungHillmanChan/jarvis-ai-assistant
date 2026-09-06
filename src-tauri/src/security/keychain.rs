use security_framework::passwords;

/// Storage behind [`KeychainStore`].
///
/// The only reason this seam exists is testability. `KeychainStore` guards every
/// API key and the Google refresh token, so its behaviour has to be covered --
/// but a test that writes to the real login Keychain mutates the developer's
/// machine, and on a headless runner (CI, ssh) the keychain is locked, so
/// `set_generic_password` returns errSecInteractionNotAllowed or blocks on a GUI
/// prompt. Under `cfg(test)` the store is wired to an in-memory backend instead;
/// the real path stays covered by an `#[ignore]`d test that a developer runs
/// deliberately on a Mac.
///
/// `get` returns bytes, not `String`, so the UTF-8 conversion stays in
/// `KeychainStore::get` where it can be exercised.
trait SecretBackend {
    fn put(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String>;
    fn get(&self, service: &str, account: &str) -> Result<Vec<u8>, String>;
    fn delete(&self, service: &str, account: &str) -> Result<(), String>;
}

/// The real macOS login Keychain.
struct RealKeychain;

impl SecretBackend for RealKeychain {
    fn put(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
        passwords::set_generic_password(service, account, secret).map_err(|e| e.to_string())
    }
    fn get(&self, service: &str, account: &str) -> Result<Vec<u8>, String> {
        passwords::get_generic_password(service, account).map_err(|e| e.to_string())
    }
    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        passwords::delete_generic_password(service, account).map_err(|e| e.to_string())
    }
}

#[cfg(not(test))]
fn backend() -> impl SecretBackend {
    RealKeychain
}

#[cfg(test)]
fn backend() -> impl SecretBackend {
    memory::MemoryKeychain
}

/// Process-local stand-in for the login Keychain, used only in test builds.
#[cfg(test)]
mod memory {
    use super::SecretBackend;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    /// `Ok(bytes)` is a stored secret; `Err(message)` is an entry seeded to fail
    /// the way a locked keychain does.
    type Entry = Result<Vec<u8>, String>;

    const NOT_FOUND: &str = "The specified item could not be found in the keychain.";

    fn store() -> &'static Mutex<HashMap<(String, String), Entry>> {
        static STORE: OnceLock<Mutex<HashMap<(String, String), Entry>>> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn key(service: &str, account: &str) -> (String, String) {
        (service.to_string(), account.to_string())
    }

    /// Store raw bytes without going through `KeychainStore::put`, so the UTF-8
    /// branch of `KeychainStore::get` is reachable from a test.
    pub(super) fn seed_raw(service: &str, account: &str, bytes: &[u8]) {
        store()
            .lock()
            .unwrap()
            .insert(key(service, account), Ok(bytes.to_vec()));
    }

    /// Make every operation on this one entry fail, standing in for a locked
    /// keychain returning errSecInteractionNotAllowed.
    pub(super) fn seed_failure(service: &str, account: &str, message: &str) {
        store()
            .lock()
            .unwrap()
            .insert(key(service, account), Err(message.to_string()));
    }

    pub(super) struct MemoryKeychain;

    impl SecretBackend for MemoryKeychain {
        fn put(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), String> {
            let mut entries = store().lock().unwrap();
            if let Some(Err(message)) = entries.get(&key(service, account)) {
                return Err(message.clone());
            }
            entries.insert(key(service, account), Ok(secret.to_vec()));
            Ok(())
        }
        fn get(&self, service: &str, account: &str) -> Result<Vec<u8>, String> {
            match store().lock().unwrap().get(&key(service, account)) {
                Some(Ok(bytes)) => Ok(bytes.clone()),
                Some(Err(message)) => Err(message.clone()),
                None => Err(NOT_FOUND.to_string()),
            }
        }
        fn delete(&self, service: &str, account: &str) -> Result<(), String> {
            let mut entries = store().lock().unwrap();
            match entries.get(&key(service, account)) {
                Some(Err(message)) => Err(message.clone()),
                Some(Ok(_)) => {
                    entries.remove(&key(service, account));
                    Ok(())
                }
                None => Err(NOT_FOUND.to_string()),
            }
        }
    }
}

pub struct KeychainStore;

impl KeychainStore {
    pub fn put(service: &str, account: &str, secret: &str) -> Result<(), String> {
        backend()
            .put(service, account, secret.as_bytes())
            .map_err(|e| format!("keychain put: {e}"))
    }
    pub fn get(service: &str, account: &str) -> Result<String, String> {
        let bytes = backend()
            .get(service, account)
            .map_err(|e| format!("keychain get: {e}"))?;
        String::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))
    }
    pub fn delete(service: &str, account: &str) -> Result<(), String> {
        backend()
            .delete(service, account)
            .map_err(|e| format!("keychain delete: {e}"))
    }

    /// True when a failure means "no such item", which is the ordinary case for
    /// a key the user has simply never saved. Anything else -- a locked
    /// keychain, a denied access prompt -- is a real fault that a caller
    /// treating every error as "not configured" would swallow silently.
    pub fn is_not_found(error: &str) -> bool {
        error.contains("could not be found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Under `cfg(test)` `KeychainStore` runs against the in-memory backend (see
    /// `backend`), so everything below passes on a headless runner without
    /// touching the developer's login Keychain.
    #[test]
    fn round_trip_secret() {
        let svc = "com.jarvis.test";
        let acct = "round-trip";
        KeychainStore::put(svc, acct, "AKIA-TEST-SECRET").unwrap();
        assert_eq!(KeychainStore::get(svc, acct).unwrap(), "AKIA-TEST-SECRET");
        // Re-saving a key must overwrite, not fail or keep the old value --
        // that is the rotate-your-API-key path in Settings.
        KeychainStore::put(svc, acct, "AKIA-ROTATED").unwrap();
        assert_eq!(KeychainStore::get(svc, acct).unwrap(), "AKIA-ROTATED");
        KeychainStore::delete(svc, acct).unwrap();
        assert!(KeychainStore::get(svc, acct).is_err());
    }

    /// A key that was never saved must be an error, not an empty string:
    /// `ai::resolve_key` treats `Err` as "no key here, try the next source".
    #[test]
    fn missing_item_is_an_error() {
        let err = KeychainStore::get("com.jarvis.test", "never-written").unwrap_err();
        assert!(err.starts_with("keychain get:"), "got {err}");
    }

    /// A locked keychain must surface the real cause to the caller rather than
    /// looking like an absent key.
    #[test]
    fn access_denied_is_surfaced_to_the_caller() {
        let svc = "com.jarvis.test";
        let acct = "denied";
        memory::seed_failure(svc, acct, "interaction with the Security Server is not allowed");
        let read = KeychainStore::get(svc, acct).unwrap_err();
        assert!(read.starts_with("keychain get:"), "got {read}");
        assert!(read.contains("not allowed"), "got {read}");
        let write = KeychainStore::put(svc, acct, "AKIA-TEST-SECRET").unwrap_err();
        assert!(write.starts_with("keychain put:"), "got {write}");
    }

    /// Callers use `is_not_found` to decide whether a `get` failure is worth
    /// logging. An absent item is routine; a locked keychain is not, and must
    /// not be classified as absent.
    #[test]
    fn only_a_missing_item_counts_as_not_found() {
        let svc = "com.jarvis.test";
        let missing = KeychainStore::get(svc, "never-written-not-found").unwrap_err();
        assert!(KeychainStore::is_not_found(&missing), "got {missing}");

        memory::seed_failure(svc, "locked", "interaction with the Security Server is not allowed");
        let locked = KeychainStore::get(svc, "locked").unwrap_err();
        assert!(!KeychainStore::is_not_found(&locked), "got {locked}");
    }

    /// Bytes that are not UTF-8 must be rejected, never lossily converted into a
    /// key the provider chain would then send as a credential.
    #[test]
    fn non_utf8_secret_is_rejected() {
        let svc = "com.jarvis.test";
        let acct = "binary";
        memory::seed_raw(svc, acct, &[0xff, 0xfe, 0x00]);
        let err = KeychainStore::get(svc, acct).unwrap_err();
        assert!(err.starts_with("utf8:"), "got {err}");
    }

    /// The real login Keychain. Ignored by default because it writes to the
    /// developer's machine and needs an unlocked GUI session; run it
    /// deliberately with
    /// `cargo test -- --ignored round_trip_against_the_real_keychain`.
    #[test]
    #[ignore]
    fn round_trip_against_the_real_keychain() {
        let svc = "com.jarvis.test";
        let acct = "test-aws-secret";
        let real = RealKeychain;
        real.put(svc, acct, b"AKIA-TEST-SECRET").unwrap();
        assert_eq!(real.get(svc, acct).unwrap(), b"AKIA-TEST-SECRET".to_vec());
        real.delete(svc, acct).unwrap();
        assert!(real.get(svc, acct).is_err());
    }
}
