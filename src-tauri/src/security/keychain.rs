use security_framework::passwords;

pub struct KeychainStore;

impl KeychainStore {
    pub fn put(service: &str, account: &str, secret: &str) -> Result<(), String> {
        passwords::set_generic_password(service, account, secret.as_bytes())
            .map_err(|e| format!("keychain put: {e}"))
    }
    pub fn get(service: &str, account: &str) -> Result<String, String> {
        let bytes = passwords::get_generic_password(service, account)
            .map_err(|e| format!("keychain get: {e}"))?;
        String::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))
    }
    pub fn delete(service: &str, account: &str) -> Result<(), String> {
        passwords::delete_generic_password(service, account)
            .map_err(|e| format!("keychain delete: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_secret() {
        let svc = "com.jarvis.test";
        let acct = "test-aws-secret";
        KeychainStore::put(svc, acct, "AKIA-TEST-SECRET").unwrap();
        let got = KeychainStore::get(svc, acct).unwrap();
        assert_eq!(got, "AKIA-TEST-SECRET");
        KeychainStore::delete(svc, acct).unwrap();
        assert!(KeychainStore::get(svc, acct).is_err());
    }
}
