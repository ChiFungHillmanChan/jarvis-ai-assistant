//! Settings > Storage > "Delete all JARVIS data on this Mac".
//!
//! Disconnecting Google or deleting chat history each remove one thing. This
//! removes everything JARVIS keeps on this Mac, for someone leaving JARVIS or
//! passing the Mac on, then restarts it as a fresh install. In order:
//!
//! 1. Google access is revoked, while the tokens still exist.
//! 2. The data folder: the database with its -wal and -shm, downloaded
//!    models, backups. It goes before the Keychain because an encrypted
//!    database left without its key would stop JARVIS from opening again.
//! 3. Open at login is turned off, so the Mac does not open the emptied
//!    JARVIS at every login. It comes back on by default only when somebody
//!    finishes onboarding again: the record that the default was applied
//!    went with the database.
//! 4. Every Keychain item under every JARVIS service.
//! 5. The web cache and the web view's own storage.
//!
//! Nothing outside this Mac is touched: mail stays in Gmail, events in Google
//! Calendar, pages JARVIS made in Notion, and the JARVIS account stays open.
//! On this Mac, Ollama and its models are left alone: JARVIS never installs
//! Ollama, only links to its download page, so it is the user's own.

use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, State};

use super::storage;
use crate::db::Database;
use crate::security::keychain::KeychainStore;
use crate::system::login_item::{LoginItem, LoginItemState};

/// Every Keychain service JARVIS writes to.
/// `every_keychain_service_in_the_source_is_deleted` fails when a new one
/// appears without being listed here.
pub(crate) const KEYCHAIN_SERVICES: &[&str] = &[
    "com.jarvis.db",
    "com.jarvis.google",
    "com.jarvis.mail",
    "com.jarvis.account",
    "com.jarvis.ai",
    "com.jarvis.aws",
    "com.jarvis.notion",
    "com.jarvis.github.oauth",
];

/// Where Google refresh tokens live: the main account, and one per Mail
/// mailbox.
const GOOGLE_TOKEN_SERVICES: &[&str] = &["com.jarvis.google", "com.jarvis.mail"];

/// What the erase could not remove, for the user to remove by hand.
#[derive(Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftovers {
    /// "service / account" for each Keychain item that is still there.
    pub keychain: Vec<String>,
    /// JARVIS is still set to open at login.
    pub open_at_login: bool,
}

impl Leftovers {
    fn is_empty(&self) -> bool {
        self.keychain.is_empty() && !self.open_at_login
    }
}

/// Returns what could not be removed. When that is nothing the app restarts
/// and the caller never sees the result.
#[tauri::command]
pub async fn delete_all_data(
    app: AppHandle,
    db: State<'_, Arc<Database>>,
    login_item: State<'_, LoginItemState>,
) -> Result<Leftovers, String> {
    let data_dir = storage::jarvis_data_dir(&db)?;
    revoke_google(GOOGLE_TOKEN_SERVICES).await;

    // Held while the files go, so no job is midway through a write.
    let _conn = db.conn.lock().map_err(|e| e.to_string())?;
    let left = erase(&data_dir, KEYCHAIN_SERVICES, login_item.0.as_ref())?;
    if let Some(cache) = storage::jarvis_cache_dir() {
        if let Err(e) = storage::remove_dir_contents(&cache) {
            log::warn!("delete all: web cache: {e}");
        }
    }
    // A native QA build shares the installed app's web view store; like the
    // rest of storage.rs, QA must never clear production data.
    #[cfg(not(feature = "native-qa"))]
    for window in tauri::Manager::webview_windows(&app).values() {
        if let Err(e) = window.clear_all_browsing_data() {
            log::warn!("delete all: web view storage: {e}");
        }
    }
    if left.is_empty() {
        log::info!("delete all: everything deleted, restarting");
        app.request_restart();
    }
    Ok(left)
}

/// Steps 2 to 4. An error means nothing was deleted and nothing was changed.
/// After the data folder has gone, nothing stops the rest: what could not be
/// removed is returned, not raised.
fn erase(data_dir: &Path, keychain_services: &[&str], login_item: &dyn LoginItem) -> Result<Leftovers, String> {
    delete_data_folder(data_dir)?;
    let open_at_login = still_opens_at_login(login_item);
    let keychain = delete_keychain_items(keychain_services);
    Ok(Leftovers { keychain, open_at_login })
}

/// Turn open at login off, and say whether the Mac still has it on. Through
/// the same login item as the switch in Settings, so a development or QA
/// build changes only its in-memory stand-in.
fn still_opens_at_login(login_item: &dyn LoginItem) -> bool {
    match login_item.set_enabled(false).and_then(|()| login_item.is_enabled()) {
        Ok(false) => false,
        Ok(true) => {
            log::warn!("delete all: open at login is still on after it was turned off");
            true
        }
        Err(e) => {
            log::warn!("delete all: open at login: {e}");
            true
        }
    }
}

/// Best effort, like disconnect: offline, the tokens are still deleted with
/// everything else, and Settings tells the user where to remove the access.
async fn revoke_google(services: &[&str]) {
    let mut tokens = Vec::new();
    for service in services {
        let accounts = KeychainStore::accounts(service).unwrap_or_else(|e| {
            log::warn!("delete all: {e}");
            Vec::new()
        });
        for account in accounts {
            match KeychainStore::get(service, &account) {
                Ok(token) => tokens.push(token),
                Err(e) => log::warn!("delete all: {service}: {e}"),
            }
        }
    }
    // A mailbox on the main account shares its grant; revoke it once.
    tokens.sort();
    tokens.dedup();
    for token in tokens {
        if let Err(e) = crate::auth::google::revoke_grant(&token).await {
            log::warn!("delete all: {e}");
        }
    }
}

/// Refused unless the folder holds the JARVIS database, so a wrong path can
/// never take a folder JARVIS does not own.
fn delete_data_folder(dir: &Path) -> Result<(), String> {
    if !dir.join("jarvis.db").is_file() {
        return Err(format!(
            "{} does not hold the JARVIS database; nothing was deleted.",
            dir.display()
        ));
    }
    std::fs::remove_dir_all(dir).map_err(|e| format!("delete {}: {e}", dir.display()))
}

/// Returns "service / account" for each item that is still there. An item
/// already gone is not a failure.
fn delete_keychain_items(services: &[&str]) -> Vec<String> {
    let mut left = Vec::new();
    for service in services {
        let accounts = match KeychainStore::accounts(service) {
            Ok(accounts) => accounts,
            Err(e) => {
                log::warn!("delete all: {e}");
                left.push(service.to_string());
                continue;
            }
        };
        for account in accounts {
            if let Err(e) = KeychainStore::delete(service, &account) {
                if !KeychainStore::is_not_found(&e) {
                    log::warn!("delete all: {service}: {e}");
                    left.push(format!("{service} / {account}"));
                }
            }
        }
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::keychain::memory;
    use crate::system::login_item::{apply_default, MemoryLoginItem};

    // The in-memory Keychain is shared by every test in the process, so these
    // use their own services and never the real ones in KEYCHAIN_SERVICES.

    #[test]
    fn every_item_under_the_listed_services_is_deleted_and_nothing_else() {
        let main = "com.jarvis.test.erase.main";
        let mail = "com.jarvis.test.erase.mail";
        let other = "com.jarvis.test.erase.other";
        KeychainStore::put(main, "refresh", "main-token").unwrap();
        KeychainStore::put(mail, "mailbox-1", "first").unwrap();
        KeychainStore::put(mail, "mailbox-2", "second").unwrap();
        KeychainStore::put(other, "kept", "not ours").unwrap();

        assert!(delete_keychain_items(&[main, mail]).is_empty());
        assert!(KeychainStore::accounts(main).unwrap().is_empty());
        assert!(KeychainStore::accounts(mail).unwrap().is_empty());
        assert_eq!(KeychainStore::get(other, "kept").unwrap(), "not ours");
        KeychainStore::delete(other, "kept").unwrap();
    }

    /// A locked item must be reported, never counted as deleted: the user is
    /// told what is left instead of being told everything is gone.
    #[test]
    fn an_item_that_cannot_be_deleted_is_reported() {
        let svc = "com.jarvis.test.erase.locked";
        KeychainStore::put(svc, "gone", "secret").unwrap();
        memory::seed_failure(svc, "locked", "interaction with the Security Server is not allowed");

        let left = delete_keychain_items(&[svc]);
        assert_eq!(left, [format!("{svc} / locked")]);
        assert!(KeychainStore::is_not_found(&KeychainStore::get(svc, "gone").unwrap_err()));
    }

    #[tokio::test]
    async fn every_google_token_is_revoked_before_it_is_deleted() {
        let main = "com.jarvis.test.revoke.main";
        let mail = "com.jarvis.test.revoke.mail";
        KeychainStore::put(main, "refresh", "erase-main-token").unwrap();
        KeychainStore::put(mail, "mailbox-1", "erase-mailbox-token").unwrap();
        KeychainStore::put(mail, "mailbox-2", "erase-main-token").unwrap();

        revoke_google(&[main, mail]).await;

        let revoked = crate::auth::google::REVOKED_IN_TESTS.lock().unwrap().clone();
        assert_eq!(revoked.iter().filter(|t| *t == "erase-main-token").count(), 1, "a shared grant is revoked once");
        assert!(revoked.iter().any(|t| t == "erase-mailbox-token"));
        assert!(delete_keychain_items(&[main, mail]).is_empty());
    }

    #[test]
    fn the_whole_data_folder_goes() {
        let root = std::env::temp_dir().join(format!("jarvis-erase-{}", uuid::Uuid::new_v4()));
        let dir = root.join("jarvis");
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::create_dir_all(dir.join("backups")).unwrap();
        for file in ["jarvis.db", "jarvis.db-wal", "jarvis.db-shm", ".env", "models/ggml-base.bin", "backups/jarvis-1.db"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }

        delete_data_folder(&dir).unwrap();
        assert!(!dir.exists());
        assert!(root.exists(), "only the JARVIS folder, not its parent");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_without_the_jarvis_database_is_left_alone() {
        let dir = std::env::temp_dir().join(format!("jarvis-erase-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), b"someone else's").unwrap();

        assert!(delete_data_folder(&dir).is_err());
        assert!(dir.join("notes.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A folder that passes for the JARVIS data folder. Returns its parent,
    /// to clean up, and the folder.
    fn data_folder() -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("jarvis-erase-{}", uuid::Uuid::new_v4()));
        let dir = root.join("jarvis");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("jarvis.db"), b"x").unwrap();
        (root, dir)
    }

    /// Somebody leaving JARVIS, or passing the Mac on, does not want the
    /// emptied app opened at every login. It is opened at login again only
    /// once somebody has set JARVIS up again.
    #[test]
    fn open_at_login_is_turned_off_with_the_data_until_onboarding_is_finished_again() {
        let (root, dir) = data_folder();
        let svc = "com.jarvis.test.erase.login";
        KeychainStore::put(svc, "refresh", "token").unwrap();
        let login_item = MemoryLoginItem::default();
        login_item.set_enabled(true).unwrap();

        let left = erase(&dir, &[svc], &login_item).unwrap();
        assert!(!dir.exists());
        assert!(!login_item.is_enabled().unwrap(), "open at login is off");
        assert_eq!(left, Leftovers::default());
        assert!(left.is_empty(), "nothing is left, so JARVIS restarts");

        // The restart finds a new, empty database.
        let db = crate::db::test_database();
        assert!(!apply_default(&db, &login_item).unwrap());
        assert!(!login_item.is_enabled().unwrap(), "still off: nobody has set JARVIS up again");
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO user_preferences (key, value) VALUES ('onboarding_complete', 'true') \
                 ON CONFLICT(key) DO UPDATE SET value = 'true'",
                [],
            )
            .unwrap();
        assert!(apply_default(&db, &login_item).unwrap(), "the default applies again when onboarding is finished again");
        assert!(login_item.is_enabled().unwrap());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A Mac that will not let go of the login item.
    struct StuckLoginItem;

    impl LoginItem for StuckLoginItem {
        fn is_enabled(&self) -> Result<bool, String> {
            Ok(true)
        }
        fn set_enabled(&self, _enabled: bool) -> Result<(), String> {
            Err("Operation not permitted (os error 1)".to_string())
        }
    }

    /// The erase goes through all the same, and the user is told what is
    /// left instead of being told everything is gone.
    #[test]
    fn a_login_item_that_cannot_be_turned_off_is_reported_and_does_not_stop_the_erase() {
        let (root, dir) = data_folder();
        let svc = "com.jarvis.test.erase.login.stuck";
        KeychainStore::put(svc, "refresh", "token").unwrap();

        let left = erase(&dir, &[svc], &StuckLoginItem).unwrap();
        assert!(!dir.exists(), "the data is gone");
        assert!(KeychainStore::accounts(svc).unwrap().is_empty(), "and so is the Keychain item");
        assert_eq!(left, Leftovers { keychain: Vec::new(), open_at_login: true });
        assert!(!left.is_empty(), "so JARVIS stays open to say so");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// An erase that is refused changes nothing, the login item included.
    #[test]
    fn open_at_login_is_left_alone_when_nothing_is_deleted() {
        let dir = std::env::temp_dir().join(format!("jarvis-erase-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let login_item = MemoryLoginItem::default();
        login_item.set_enabled(true).unwrap();

        assert!(erase(&dir, &["com.jarvis.test.erase.login.refused"], &login_item).is_err());
        assert!(login_item.is_enabled().unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A Keychain service added anywhere in the source must be added to
    /// KEYCHAIN_SERVICES, or "delete all" would leave its secrets behind.
    #[test]
    fn every_keychain_service_in_the_source_is_deleted() {
        fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    rust_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);

        let mut found = std::collections::BTreeSet::new();
        for file in &files {
            let source = std::fs::read_to_string(file).unwrap();
            for (start, _) in source.match_indices("\"com.jarvis.") {
                let literal = &source[start + 1..];
                let service = &literal[..literal.find('"').unwrap()];
                // The bare prefix is this test's own search string.
                if service != "com.jarvis." && !service.starts_with("com.jarvis.test") {
                    found.insert(service.to_string());
                }
            }
        }
        assert!(found.contains("com.jarvis.mail"), "the scan must see the real services");
        for service in found {
            assert!(KEYCHAIN_SERVICES.contains(&service.as_str()), "{service} is not in KEYCHAIN_SERVICES");
        }
    }
}
