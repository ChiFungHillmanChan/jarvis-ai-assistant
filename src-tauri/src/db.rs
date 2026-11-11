use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("migrations");
}

/// Periodically checkpoint the WAL and limit the file retained when it resets.
/// This is not a hard disk quota: large transactions or readers that prevent a
/// complete checkpoint can keep the WAL above `journal_size_limit` temporarily.
pub const CONNECTION_PRAGMAS: &str = "PRAGMA journal_mode=WAL; \
     PRAGMA foreign_keys=ON; \
     PRAGMA journal_size_limit=67108864; \
     PRAGMA wal_autocheckpoint=1000;";

pub struct Database {
    pub conn: Mutex<Connection>,
}

/// What mail sync reports while the database on disk is not encrypted.
pub const UNENCRYPTED_SYNC_PAUSED: &str =
    "此 Mac 上的 JARVIS 資料庫未能加密，郵件同步已暫停。請重新啟動 JARVIS，並在 macOS 要求存取鑰匙圈時選擇「允許」。";

static STORED_UNENCRYPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
tokio::task_local! {
    /// Tests say how the database was opened, without a real one on disk.
    pub static TEST_STORED_UNENCRYPTED: bool;
}

/// True when this launch opened the database without encryption because its
/// key could not be created or read (see `encryption::open`).
pub fn stored_unencrypted() -> bool {
    #[cfg(test)]
    if let Ok(unencrypted) = TEST_STORED_UNENCRYPTED.try_with(|unencrypted| *unencrypted) {
        return unencrypted;
    }
    STORED_UNENCRYPTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Mail is not written to a database that is not encrypted.
pub fn require_encrypted_for_mail() -> Result<(), String> {
    if stored_unencrypted() {
        Err(UNENCRYPTED_SYNC_PAUSED.to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn test_database() -> Database {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    embedded::migrations::runner().run(&mut conn).unwrap();
    Database {
        conn: Mutex::new(conn),
    }
}

#[cfg(test)]
pub(crate) fn test_database_at(path: &std::path::Path) -> Database {
    let mut conn = Connection::open(path).unwrap();
    // Live tests run against a copy of the real, encrypted database. The key:
    // security find-generic-password -s com.jarvis.db -a sqlcipher-key -w
    if let Ok(key) = std::env::var("JARVIS_DB_KEY") {
        encryption::apply_key(&conn, &key).unwrap();
    }
    conn.execute_batch(CONNECTION_PRAGMAS).unwrap();
    embedded::migrations::runner().run(&mut conn).unwrap();
    Database {
        conn: Mutex::new(conn),
    }
}

#[cfg(test)]
pub(crate) fn test_database_at_version(path: &std::path::Path, version: i32) -> Database {
    let mut conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .unwrap();
    embedded::migrations::runner()
        .set_target(refinery::Target::Version(version.try_into().unwrap()))
        .run(&mut conn)
        .unwrap();
    Database {
        conn: Mutex::new(conn),
    }
}

/// Owner-only access to the data folder and the database files, so other
/// accounts on this Mac cannot list or read local mail. SQLite creates later
/// -wal/-shm files with the database file's permissions.
fn restrict_to_owner(db_path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let set = |path: &std::path::Path, mode: u32| {
        if path.exists() {
            if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)) {
                log::warn!("Could not restrict {:?}: {}", path, e);
            }
        }
    };
    if let Some(dir) = db_path.parent() {
        set(dir, 0o700);
    }
    for suffix in ["", "-wal", "-shm"] {
        set(std::path::Path::new(&format!("{}{}", db_path.display(), suffix)), 0o600);
    }
}

impl Database {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = Self::db_path()?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        #[cfg(not(feature = "native-qa"))]
        let mut conn = {
            let (conn, encrypted) = encryption::open(&db_path)?;
            STORED_UNENCRYPTED.store(!encrypted, std::sync::atomic::Ordering::Relaxed);
            conn
        };
        // Native QA runs on a throwaway folder with an in-memory Keychain, so a
        // key would not survive a relaunch; its data is synthetic.
        #[cfg(feature = "native-qa")]
        let mut conn = Connection::open(&db_path)?;
        conn.execute_batch(CONNECTION_PRAGMAS)?;
        restrict_to_owner(&db_path);
        embedded::migrations::runner().run(&mut conn)?;
        #[cfg(feature = "native-qa")]
        crate::native_qa::seed_once(&mut conn)?;
        log::info!("Database initialized at {:?}", db_path);
        Ok(Database {
            conn: Mutex::new(conn),
        })
    }

    fn db_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        #[cfg(feature = "native-qa")]
        return Ok(crate::native_qa::directory()?.join("jarvis.db"));
        #[cfg(not(feature = "native-qa"))]
        {
            let data_dir =
                dirs::data_dir().ok_or("Could not find application support directory")?;
            Ok(data_dir.join("jarvis").join("jarvis.db"))
        }
    }
}

#[cfg(test)]
mod permission_tests {
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn data_folder_and_database_files_are_owner_only() {
        let dir = std::env::temp_dir().join(format!("jarvis-perm-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let db = dir.join("jarvis.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t(x);").unwrap();
        super::restrict_to_owner(&db);
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&db), 0o600);
        assert_eq!(mode(&dir.join("jarvis.db-wal")), 0o600);
        drop(conn);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// Encryption at rest. The database holds synced mail, so it is a SQLCipher
/// file (AES-256) keyed with 256 random bits kept in the macOS Keychain.
/// Databases written by earlier builds are converted once, on first launch.
pub(crate) mod encryption {
    use crate::security::keychain::KeychainStore;
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};

    const KEY_SERVICE: &str = "com.jarvis.db";
    const KEY_ACCOUNT: &str = "sqlcipher-key";
    const PLAINTEXT_HEADER: &[u8; 16] = b"SQLite format 3\0";

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    /// Open the app database, encrypting a plaintext one first.
    ///
    /// Nothing here may leave someone unable to open JARVIS over something
    /// recoverable: if a plaintext database cannot be converted, or a new one
    /// cannot get a key, it opens unencrypted and the next launch tries again.
    /// Only an encrypted database whose key is gone is a hard failure, because
    /// its contents are unreadable without that key.
    ///
    /// The flag is whether the database is encrypted on disk.
    pub fn open(path: &Path) -> Result<(Connection, bool)> {
        open_with(path, key)
    }

    /// `open` with the Keychain lookup passed in: `key(create)` returns the
    /// key, generating it first when `create` allows.
    fn open_with(path: &Path, key: impl Fn(bool) -> Result<String>) -> Result<(Connection, bool)> {
        settle_interrupted_conversion(path)?;
        if is_plaintext(path)? {
            match key(true).and_then(|key| encrypt_in_place(path, &key).map(|_| key)) {
                Ok(key) => return Ok((open_with_key(path, &key)?, true)),
                Err(e) => {
                    log::error!("Database encryption failed; opening it unencrypted: {e}");
                    return Ok((Connection::open(path)?, false));
                }
            }
        }
        let is_new = !path.exists() || std::fs::metadata(path)?.len() == 0;
        match key(is_new) {
            Ok(key) => Ok((open_with_key(path, &key)?, true)),
            Err(e) if is_new => {
                log::error!("No database key; creating the database unencrypted: {e}");
                Ok((Connection::open(path)?, false))
            }
            Err(e) => Err(format!(
                "The database is encrypted, but its key could not be read from the Keychain ({e})."
            )
            .into()),
        }
    }

    /// True for a database file an earlier build wrote without encryption.
    fn is_plaintext(path: &Path) -> Result<bool> {
        use std::io::Read;
        let mut header = [0u8; 16];
        match std::fs::File::open(path) {
            Ok(mut file) => Ok(file.read_exact(&mut header).is_ok() && &header == PLAINTEXT_HEADER),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// The key from the Keychain, generated first when `create` allows it.
    fn key(create: bool) -> Result<String> {
        match KeychainStore::get(KEY_SERVICE, KEY_ACCOUNT) {
            Ok(key) if is_key(&key) => return Ok(key),
            Ok(_) => return Err("the stored database key is malformed".into()),
            Err(e) if !KeychainStore::is_not_found(&e) || !create => return Err(e.into()),
            Err(_) => {}
        }
        let mut bytes = [0u8; 32];
        security_framework::random::SecRandom::default().copy_bytes(&mut bytes)?;
        let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        KeychainStore::put(KEY_SERVICE, KEY_ACCOUNT, &key)?;
        Ok(key)
    }

    fn is_key(key: &str) -> bool {
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
    }

    /// Key a connection. A raw 256-bit key skips SQLCipher's passphrase
    /// derivation; `key` is validated hex, so the statement cannot be altered.
    pub fn apply_key(conn: &Connection, key: &str) -> Result<()> {
        if !is_key(key) {
            return Err("invalid database key".into());
        }
        conn.execute_batch(&format!("PRAGMA key = \"x'{key}'\";"))?;
        Ok(())
    }

    fn open_with_key(path: &Path, key: &str) -> Result<Connection> {
        let conn = Connection::open(path)?;
        apply_key(&conn, key)?;
        // The first read fails with "file is not a database" on a wrong key.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get::<_, i64>(0))?;
        Ok(conn)
    }

    fn sibling(path: &Path, suffix: &str) -> PathBuf {
        PathBuf::from(format!("{}{}", path.display(), suffix))
    }

    fn remove_with_sidecars(path: &Path) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(sibling(path, suffix));
        }
    }

    fn row_counts(conn: &Connection) -> Result<Vec<(String, i64)>> {
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        tables
            .into_iter()
            .map(|table| {
                let quoted = table.replace('"', "\"\"");
                let count = conn.query_row(&format!("SELECT count(*) FROM \"{quoted}\""), [], |row| row.get(0))?;
                Ok((table, count))
            })
            .collect()
    }

    /// Copy a plaintext database into an encrypted one next to it, check that
    /// every table arrived intact, then put the encrypted copy in its place.
    /// The plaintext file is only removed after the swap.
    pub(crate) fn encrypt_in_place(path: &Path, key: &str) -> Result<()> {
        let staged = sibling(path, ".encrypting");
        let retired = sibling(path, ".unencrypted");
        remove_with_sidecars(&staged);
        let expected = {
            let plain = Connection::open(path)?;
            plain.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            let target = staged.display().to_string().replace('\'', "''");
            apply_attach(&plain, &target, key)?;
            plain.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))?;
            let version: i64 = plain.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            plain.execute_batch(&format!("PRAGMA encrypted.user_version = {version}; DETACH DATABASE encrypted;"))?;
            row_counts(&plain)?
        };
        let copied = row_counts(&open_with_key(&staged, key)?)?;
        if copied != expected {
            remove_with_sidecars(&staged);
            return Err("the encrypted copy does not match the original".into());
        }
        std::fs::rename(path, &retired)?;
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(sibling(path, suffix));
        }
        if let Err(e) = std::fs::rename(&staged, path) {
            // Put the original back so nothing is lost.
            let _ = std::fs::rename(&retired, path);
            return Err(e.into());
        }
        remove_with_sidecars(&retired);
        log::info!("Database encrypted");
        Ok(())
    }

    /// Leave exactly one database behind a conversion that was cut short (app
    /// quit, crash, power loss), so no second copy of the mail -- and no
    /// plaintext one -- sits on disk until the user finds it.
    fn settle_interrupted_conversion(path: &Path) -> Result<()> {
        let staged = sibling(path, ".encrypting");
        let retired = sibling(path, ".unencrypted");
        if !retired.exists() {
            // Stopped mid-export: the original was never touched.
            remove_with_sidecars(&staged);
        } else if !path.exists() {
            // Stopped between the two renames. The original is the copy known
            // to be complete; put it back and let this launch convert it again.
            std::fs::rename(&retired, path)?;
            remove_with_sidecars(&staged);
            log::warn!("Database restored from an interrupted encryption");
        } else if !staged.exists() {
            // Stopped after the swap: the encrypted copy was checked before it
            // replaced the original, so the original is surplus.
            remove_with_sidecars(&retired);
        } else {
            log::error!(
                "Database encryption was interrupted and a new database was opened since; \
                 {} is kept because it may be the only copy of older data",
                retired.display()
            );
        }
        Ok(())
    }

    fn apply_attach(conn: &Connection, target: &str, key: &str) -> Result<()> {
        if !is_key(key) {
            return Err("invalid database key".into());
        }
        conn.execute_batch(&format!("ATTACH DATABASE '{target}' AS encrypted KEY \"x'{key}'\";"))?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

        fn temp_db() -> PathBuf {
            let dir = std::env::temp_dir().join(format!("jarvis-enc-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            dir.join("jarvis.db")
        }

        #[test]
        fn a_keyed_database_is_not_readable_as_plain_sqlite() {
            let path = temp_db();
            let conn = open_with_key(&path, KEY).unwrap();
            conn.execute_batch("CREATE TABLE mail(body TEXT); INSERT INTO mail VALUES ('secret body');").unwrap();
            drop(conn);
            let bytes = std::fs::read(&path).unwrap();
            assert_ne!(&bytes[..16], PLAINTEXT_HEADER);
            assert!(!bytes.windows(11).any(|w| w == b"secret body"));
            assert!(open_with_key(&path, &KEY.replace('0', "1")).is_err(), "a wrong key must not open it");
            std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }

        #[test]
        fn a_plaintext_database_is_encrypted_with_its_data_intact() {
            let path = temp_db();
            {
                let conn = Connection::open(&path).unwrap();
                conn.execute_batch(
                    "PRAGMA journal_mode=WAL; PRAGMA user_version=7;
                     CREATE TABLE mail(body TEXT); INSERT INTO mail VALUES ('first'),('second');
                     CREATE TABLE \"odd\"\"name\"(x); INSERT INTO \"odd\"\"name\" VALUES (1);",
                )
                .unwrap();
            }
            assert!(is_plaintext(&path).unwrap());
            encrypt_in_place(&path, KEY).unwrap();
            assert!(!is_plaintext(&path).unwrap());
            let conn = open_with_key(&path, KEY).unwrap();
            let bodies: Vec<String> = conn
                .prepare("SELECT body FROM mail ORDER BY body").unwrap()
                .query_map([], |row| row.get(0)).unwrap()
                .collect::<std::result::Result<_, _>>().unwrap();
            assert_eq!(bodies, ["first", "second"]);
            let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
            assert_eq!(version, 7);
            let dir = path.parent().unwrap();
            let leftovers: Vec<_> = std::fs::read_dir(dir).unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                .filter(|name| name.contains("encrypting") || name.contains("unencrypted"))
                .collect();
            assert!(leftovers.is_empty(), "no staged or retired copies left: {leftovers:?}");
            drop(conn);
            std::fs::remove_dir_all(dir).unwrap();
        }

        fn plaintext_db(path: &Path, body: &str) {
            let conn = Connection::open(path).unwrap();
            conn.execute_batch(&format!("CREATE TABLE mail(body TEXT); INSERT INTO mail VALUES ('{body}');")).unwrap();
        }

        fn files_in(dir: &Path) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(dir).unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                .collect();
            names.sort();
            names
        }

        #[test]
        fn a_conversion_stopped_between_the_renames_puts_the_original_back() {
            let path = temp_db();
            plaintext_db(&sibling(&path, ".unencrypted"), "kept");
            drop(open_with_key(&sibling(&path, ".encrypting"), KEY).unwrap());
            settle_interrupted_conversion(&path).unwrap();
            let dir = path.parent().unwrap();
            assert_eq!(files_in(dir), ["jarvis.db"], "one database, nothing beside it");
            assert!(is_plaintext(&path).unwrap(), "the original, to be converted again");
            let body: String = Connection::open(&path).unwrap()
                .query_row("SELECT body FROM mail", [], |row| row.get(0)).unwrap();
            assert_eq!(body, "kept");
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn leftovers_beside_a_converted_database_are_removed() {
            let path = temp_db();
            drop(open_with_key(&path, KEY).unwrap().execute_batch("CREATE TABLE t(x)"));
            plaintext_db(&sibling(&path, ".unencrypted"), "plaintext mail");
            settle_interrupted_conversion(&path).unwrap();
            let dir = path.parent().unwrap();
            assert_eq!(files_in(dir), ["jarvis.db"]);

            // A partial copy from a conversion cut short mid-export.
            std::fs::write(sibling(&path, ".encrypting"), b"partial").unwrap();
            std::fs::write(sibling(&path, ".encrypting-journal"), b"partial").unwrap();
            settle_interrupted_conversion(&path).unwrap();
            assert_eq!(files_in(dir), ["jarvis.db"]);
            std::fs::remove_dir_all(dir).unwrap();
        }

        /// An earlier build that was stopped between the renames opened a new,
        /// empty database on its next launch. The plaintext original is then the
        /// only copy of the user's mail and must not be deleted.
        #[test]
        fn an_ambiguous_leftover_is_kept_rather_than_deleted() {
            let path = temp_db();
            drop(open_with_key(&path, KEY).unwrap().execute_batch("CREATE TABLE t(x)"));
            plaintext_db(&sibling(&path, ".unencrypted"), "only copy");
            drop(open_with_key(&sibling(&path, ".encrypting"), KEY).unwrap());
            settle_interrupted_conversion(&path).unwrap();
            let dir = path.parent().unwrap();
            assert!(files_in(dir).contains(&"jarvis.db.unencrypted".to_string()));
            std::fs::remove_dir_all(dir).unwrap();
        }

        /// A locked or refused Keychain must not lock anyone out of JARVIS,
        /// but the app has to know the data on disk is unprotected.
        #[test]
        fn a_database_opened_without_a_key_is_reported_as_unencrypted() {
            let no_key = |_create: bool| -> Result<String> { Err("User interaction is not allowed.".into()) };
            let fresh = temp_db();
            assert!(!open_with(&fresh, no_key).unwrap().1, "a new database with no key");
            let old = temp_db();
            Connection::open(&old).unwrap().execute_batch("CREATE TABLE mail(body TEXT);").unwrap();
            assert!(!open_with(&old, no_key).unwrap().1, "an old database that could not be converted");
            assert!(is_plaintext(&old).unwrap());
            let keyed = temp_db();
            assert!(open(&keyed).unwrap().1, "a database opened with its key");
            for p in [&fresh, &old, &keyed] {
                std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
            }
        }

        #[test]
        fn open_encrypts_old_databases_and_reopens_them_with_the_stored_key() {
            let path = temp_db();
            {
                let conn = Connection::open(&path).unwrap();
                conn.execute_batch("CREATE TABLE mail(body TEXT); INSERT INTO mail VALUES ('kept');").unwrap();
            }
            drop(open(&path).unwrap());
            assert!(!is_plaintext(&path).unwrap(), "first launch converts it");
            let (conn, _) = open(&path).unwrap();
            let body: String = conn.query_row("SELECT body FROM mail", [], |row| row.get(0)).unwrap();
            assert_eq!(body, "kept");
            drop(conn);

            let fresh = temp_db();
            drop(open(&fresh).unwrap().0.execute_batch("CREATE TABLE t(x)"));
            assert!(!is_plaintext(&fresh).unwrap(), "a new database starts encrypted");
            for p in [&path, &fresh] {
                std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
            }
        }

        /// Live: convert a copy of a real database (never the original):
        /// sqlite3 jarvis.db ".backup /tmp/copy.db"
        /// JARVIS_ENCRYPT_DB_COPY=/tmp/copy.db cargo test live_encrypt -- --ignored --nocapture
        #[test]
        #[ignore]
        fn live_encrypt_a_copy_of_a_real_database() {
            let path = PathBuf::from(std::env::var("JARVIS_ENCRYPT_DB_COPY").expect("JARVIS_ENCRYPT_DB_COPY"));
            let before = row_counts(&Connection::open(&path).unwrap()).unwrap();
            encrypt_in_place(&path, KEY).unwrap();
            assert!(!is_plaintext(&path).unwrap());
            let mut conn = open_with_key(&path, KEY).unwrap();
            assert_eq!(row_counts(&conn).unwrap(), before);
            // The app's own migrations still run on the encrypted file.
            crate::db::embedded::migrations::runner().run(&mut conn).unwrap();
            let rows: i64 = before.iter().map(|(_, n)| n).sum();
            println!("encrypted {} tables, {} rows, all counts match", before.len(), rows);
        }

        #[test]
        fn keys_are_64_hex_characters_only() {
            assert!(is_key(KEY));
            assert!(!is_key("x'00'; DROP TABLE mail; --"));
            assert!(!is_key(&KEY[..63]));
        }
    }
}
