//! Keychain-backed storage for the third-party integration credentials.
//!
//! The GitHub, Notion, Obsidian and MiniMax keys used to be written straight
//! into the plaintext `user_preferences` table. They now live in the macOS
//! Keychain under `crate::ai::KEY_SERVICE`, exactly like the AI provider keys
//! and the Google refresh token, and `user_preferences` keeps only a non-secret
//! marker so the `<key>_configured` flags produced by `commands::settings`
//! still report whether a credential is set.

use crate::db::Database;
use crate::security::keychain::KeychainStore;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// What is left in `user_preferences` once the secret has moved to the
/// Keychain. Same marker `auth::google` uses for the refresh token.
pub const KEYCHAIN_MARKER: &str = "__keychain__";

/// The integration credentials, as (preference key, environment variable).
/// The preference key doubles as the Keychain account, so there is exactly one
/// name per credential to keep straight.
pub const INTEGRATION_KEYS: [(&str, &str); 5] = [
    ("github_token", "GITHUB_TOKEN"),
    ("notion_api_key", "NOTION_API_KEY"),
    ("obsidian_api_key", "OBSIDIAN_API_KEY"),
    ("minimax_api_key", "MINIMAX_API_KEY"),
    ("typesafe_api_key", "TYPESAFE_API_KEY"),
];

fn env_var_for(key: &str) -> Option<&'static str> {
    INTEGRATION_KEYS
        .iter()
        .find(|(pref, _)| *pref == key)
        .map(|(_, var)| *var)
}

/// Plaintext credentials an older build left in `user_preferences` that the
/// Keychain refused to take during the startup migration.
///
/// `resolve` is called from command handlers that hold nothing but the key
/// name, so unlike `resolve_local_endpoint_key` it cannot read the row it is
/// falling back to. Without this the failed migration left the secret readable
/// in SQLite and unreadable by the app: the row stayed plaintext *and* every
/// integration reported "not configured". This makes that case degrade the way
/// the endpoint credential already does -- still works, still plaintext,
/// logged -- rather than silently breaking.
///
/// Process-local and written only by `migrate_plaintext_in`, so it holds a
/// secret for exactly as long as the plaintext row does: the next startup that
/// reaches an unlocked Keychain migrates the row, clears the entry, and the
/// fallback is never consulted again. Keyed by (service, key) so the service
/// stays the only namespace, as it is everywhere else in this module.
fn plaintext_fallback() -> &'static Mutex<HashMap<(String, String), String>> {
    static FALLBACK: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
    FALLBACK.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_plaintext_fallback(service: &str, key: &str, value: &str) {
    if let Ok(mut map) = plaintext_fallback().lock() {
        map.insert((service.to_string(), key.to_string()), value.to_string());
    }
}

/// Drop the fallback the moment the credential exists somewhere better, or has
/// been replaced or cleared by the user. A stale entry would otherwise keep
/// serving a token that is no longer configured.
fn forget_plaintext_fallback(service: &str, key: &str) {
    if let Ok(mut map) = plaintext_fallback().lock() {
        map.remove(&(service.to_string(), key.to_string()));
    }
}

fn plaintext_fallback_for(service: &str, key: &str) -> Option<String> {
    plaintext_fallback()
        .lock()
        .ok()?
        .get(&(service.to_string(), key.to_string()))
        .cloned()
}

/// Look up one integration credential. Precedence matches `ai::resolve_key`:
/// process environment first (the dev workflow, including the `.env` loaded at
/// startup), then the Keychain. A blank value counts as absent at both levels,
/// so a stray `NOTION_API_KEY=` in a dotfile cannot mask a key the user saved.
/// Last of all comes a plaintext row the Keychain refused to accept -- see
/// `plaintext_fallback`.
pub fn resolve(key: &str) -> Option<String> {
    resolve_in(crate::ai::KEY_SERVICE, key)
}

fn resolve_in(service: &str, key: &str) -> Option<String> {
    let from_env = env_var_for(key)
        .and_then(|var| std::env::var(var).ok())
        .filter(|v| !v.trim().is_empty());
    if from_env.is_some() {
        return from_env;
    }
    let from_keychain = KeychainStore::get(service, key)
        .ok()
        .filter(|v| !v.trim().is_empty());
    if from_keychain.is_some() {
        return from_keychain;
    }
    plaintext_fallback_for(service, key).filter(|v| !v.trim().is_empty())
}

/// Save a credential: the secret goes to the Keychain, `user_preferences` gets
/// only the marker. An empty value clears both. A Keychain failure is returned
/// rather than swallowed -- silently falling back to the plaintext table is the
/// bug this module exists to remove.
pub fn store(db: &Database, key: &str, value: &str) -> Result<(), String> {
    store_in(crate::ai::KEY_SERVICE, db, key, value)
}

fn store_in(service: &str, db: &Database, key: &str, value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    // Keychain FFI first, and never while the DB lock is held.
    let marker = if trimmed.is_empty() {
        KeychainStore::delete(service, key).ok();
        ""
    } else {
        KeychainStore::put(service, key, trimmed)?;
        KEYCHAIN_MARKER
    };
    // The row is about to be overwritten with the marker (or cleared), so a
    // plaintext copy an older build left there must not keep answering for this
    // credential. A failed `put` returns above instead, leaving the old one
    // serving.
    forget_plaintext_fallback(service, key);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = datetime('now')",
        rusqlite::params![key, marker],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Keychain account for one local endpoint's API key.
///
/// Unlike the four credentials above this one is per-row, not global: every
/// `local_endpoints` row can carry its own bearer token, so the account is
/// scoped by endpoint id. The prefix keeps it from ever colliding with an
/// `INTEGRATION_KEYS` account name.
fn local_endpoint_account(endpoint_id: &str) -> String {
    format!("local_endpoint_api_key:{endpoint_id}")
}

/// Save one local endpoint's API key into the Keychain and return what belongs
/// in the `local_endpoints.api_key` column -- the marker, or NULL when there is
/// no key. Same contract as `store`, except the caller writes the row, because
/// the column is set by several different statements.
pub fn store_local_endpoint_key(
    endpoint_id: &str,
    value: Option<&str>,
) -> Result<Option<String>, String> {
    store_local_endpoint_key_in(crate::ai::KEY_SERVICE, endpoint_id, value)
}

fn store_local_endpoint_key_in(
    service: &str,
    endpoint_id: &str,
    value: Option<&str>,
) -> Result<Option<String>, String> {
    let account = local_endpoint_account(endpoint_id);
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(secret) => {
            KeychainStore::put(service, &account, secret)?;
            Ok(Some(KEYCHAIN_MARKER.to_string()))
        }
        None => {
            KeychainStore::delete(service, &account).ok();
            Ok(None)
        }
    }
}

/// Read one local endpoint's API key back, given whatever its
/// `local_endpoints.api_key` column holds.
///
/// The marker means "the secret is in the Keychain". Anything else is a
/// plaintext value written by a build older than this module: it is returned as
/// it stands so an existing endpoint keeps authenticating until
/// `migrate_plaintext_to_keychain` moves it -- and, unlike the four global
/// credentials, so that a failed Keychain write cannot silently take the
/// endpoint offline.
pub fn resolve_local_endpoint_key(endpoint_id: &str, stored: Option<&str>) -> Option<String> {
    resolve_local_endpoint_key_in(crate::ai::KEY_SERVICE, endpoint_id, stored)
}

fn resolve_local_endpoint_key_in(
    service: &str,
    endpoint_id: &str,
    stored: Option<&str>,
) -> Option<String> {
    let stored = stored.map(str::trim).filter(|v| !v.is_empty())?;
    if stored != KEYCHAIN_MARKER {
        return Some(stored.to_string());
    }
    KeychainStore::get(service, &local_endpoint_account(endpoint_id))
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// Drop one endpoint's Keychain item, so deleting the row does not leave the
/// credential behind under an id nothing references any more.
pub fn delete_local_endpoint_key(endpoint_id: &str) {
    KeychainStore::delete(crate::ai::KEY_SERVICE, &local_endpoint_account(endpoint_id)).ok();
}

/// Move any plaintext credential an older build left in `user_preferences` into
/// the Keychain and overwrite the row with the marker. Mirrors what
/// `auth::google::load_from_db` does for the refresh token.
///
/// Idempotent, and deliberately infallible: if the Keychain is unavailable the
/// row is left exactly as it was and startup continues, so a Keychain prompt
/// the user dismisses cannot stop the app from launching.
pub fn migrate_plaintext_to_keychain(db: &Database) {
    migrate_plaintext_in(crate::ai::KEY_SERVICE, db)
}

fn migrate_plaintext_in(service: &str, db: &Database) {
    for (key, _) in INTEGRATION_KEYS {
        // Tight lock scope -- no Keychain FFI while the connection is held.
        let stored: Option<String> = {
            let Ok(conn) = db.conn.lock() else { return };
            conn.query_row(
                "SELECT value FROM user_preferences WHERE key = ?1",
                rusqlite::params![key],
                |row| row.get::<_, String>(0),
            )
            .ok()
        };
        let Some(plaintext) = stored else { continue };
        if plaintext.trim().is_empty() || plaintext == KEYCHAIN_MARKER {
            continue;
        }
        if let Err(e) = KeychainStore::put(service, key, &plaintext) {
            // The row stays plaintext, so keep serving it rather than letting
            // every integration report "not configured" while the secret sits
            // there. The next startup that reaches an unlocked Keychain runs
            // this sweep again and clears both.
            log::warn!(
                "Could not move '{}' into the Keychain ({}); it stays in plaintext SQLite and \
                 is served from there until the next successful attempt",
                key,
                e
            );
            remember_plaintext_fallback(service, key, &plaintext);
            continue;
        }
        forget_plaintext_fallback(service, key);
        let Ok(conn) = db.conn.lock() else { return };
        match conn.execute(
            "UPDATE user_preferences SET value = ?2, updated_at = datetime('now') WHERE key = ?1",
            rusqlite::params![key, KEYCHAIN_MARKER],
        ) {
            // The secret is in the Keychain now, but the plaintext copy is not
            // gone, which is the whole point -- say so.
            Err(e) => log::warn!("Could not clear the plaintext '{}' row: {}", key, e),
            Ok(_) => log::info!("Moved '{}' out of plaintext SQLite into the Keychain", key),
        }
    }
    migrate_local_endpoint_keys_in(service, db);
}

/// The same sweep for `local_endpoints.api_key`, which older builds wrote as a
/// plain column. Runs from the same startup call, so there is one place that
/// clears plaintext credentials.
fn migrate_local_endpoint_keys_in(service: &str, db: &Database) {
    // Tight lock scope -- no Keychain FFI while the connection is held.
    let rows: Vec<(String, String)> = {
        let Ok(conn) = db.conn.lock() else { return };
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, api_key FROM local_endpoints \
             WHERE api_key IS NOT NULL AND trim(api_key) <> '' AND api_key <> ?1",
        ) else {
            // No such table (or column): nothing to migrate.
            return;
        };
        let Ok(mapped) = stmt.query_map(rusqlite::params![KEYCHAIN_MARKER], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }) else {
            return;
        };
        mapped.filter_map(|r| r.ok()).collect()
    };

    for (id, plaintext) in rows {
        if let Err(e) = KeychainStore::put(service, &local_endpoint_account(&id), &plaintext) {
            log::warn!("Could not move endpoint '{}' key into the Keychain: {}", id, e);
            continue;
        }
        let Ok(conn) = db.conn.lock() else { return };
        match conn.execute(
            "UPDATE local_endpoints SET api_key = ?2 WHERE id = ?1",
            rusqlite::params![id, KEYCHAIN_MARKER],
        ) {
            Err(e) => log::warn!("Could not clear endpoint '{}' plaintext key: {}", id, e),
            Ok(_) => log::info!(
                "Moved endpoint '{}' API key out of plaintext SQLite into the Keychain",
                id
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    /// In-memory, not a temp file: nothing here exercises file-backed
    /// behaviour, and the file-backed version left a `.db` behind in $TMPDIR on
    /// every run. Same pattern as `ai::routing` and `aws::credentials`.
    fn temp_db() -> Database {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE user_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE local_endpoints (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                url TEXT NOT NULL,
                api_key TEXT
            );",
        )
        .expect("create fixture tables");
        Database { conn: Mutex::new(conn) }
    }

    fn read_pref(db: &Database, key: &str) -> Option<String> {
        db.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT value FROM user_preferences WHERE key = ?1",
                rusqlite::params![key],
                |r| r.get::<_, String>(0),
            )
            .ok()
    }

    /// F004: after a save the live token must not be anywhere in SQLite.
    #[test]
    fn store_keeps_the_secret_out_of_user_preferences() {
        let service = "com.jarvis.test.intkeys.store";
        let db = temp_db();
        store_in(service, &db, "github_token", "ghp_realsecret").unwrap();

        assert_eq!(read_pref(&db, "github_token").as_deref(), Some(KEYCHAIN_MARKER));
        assert_eq!(
            KeychainStore::get(service, "github_token").unwrap(),
            "ghp_realsecret"
        );
        KeychainStore::delete(service, "github_token").ok();
    }

    #[test]
    fn store_with_a_blank_value_clears_both_stores() {
        let service = "com.jarvis.test.intkeys.clear";
        let db = temp_db();
        store_in(service, &db, "notion_api_key", "ntn_realsecret").unwrap();
        store_in(service, &db, "notion_api_key", "   ").unwrap();

        assert_eq!(read_pref(&db, "notion_api_key").as_deref(), Some(""));
        assert!(KeychainStore::get(service, "notion_api_key").is_err());
    }

    /// F004(c): existing installs already have a plaintext row. Startup must
    /// move it, and running again must not clobber the Keychain item.
    #[test]
    fn migration_moves_plaintext_into_the_keychain_and_is_idempotent() {
        let service = "com.jarvis.test.intkeys.migrate";
        let db = temp_db();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO user_preferences (key, value) VALUES ('obsidian_api_key', 'obs_legacy_secret')",
                [],
            )
            .unwrap();
        }

        migrate_plaintext_in(service, &db);
        assert_eq!(
            read_pref(&db, "obsidian_api_key").as_deref(),
            Some(KEYCHAIN_MARKER)
        );
        assert_eq!(
            KeychainStore::get(service, "obsidian_api_key").unwrap(),
            "obs_legacy_secret"
        );

        migrate_plaintext_in(service, &db);
        assert_eq!(
            KeychainStore::get(service, "obsidian_api_key").unwrap(),
            "obs_legacy_secret",
            "a second pass must not overwrite the Keychain item with the marker"
        );
        assert_eq!(
            read_pref(&db, "obsidian_api_key").as_deref(),
            Some(KEYCHAIN_MARKER)
        );

        KeychainStore::delete(service, "obsidian_api_key").ok();
    }

    #[test]
    fn resolve_prefers_the_environment_over_the_keychain() {
        let service = "com.jarvis.test.intkeys.env";
        KeychainStore::put(service, "notion_api_key", "from-keychain").unwrap();

        std::env::set_var("NOTION_API_KEY", "from-env");
        assert_eq!(
            resolve_in(service, "notion_api_key").as_deref(),
            Some("from-env")
        );

        // A blank env var is absent, so it cannot mask a saved key.
        std::env::set_var("NOTION_API_KEY", "  ");
        assert_eq!(
            resolve_in(service, "notion_api_key").as_deref(),
            Some("from-keychain")
        );

        std::env::remove_var("NOTION_API_KEY");
        KeychainStore::delete(service, "notion_api_key").ok();
    }

    /// F004: a Keychain that refuses the write at startup must not take every
    /// integration offline. The row is left plaintext on purpose, and `resolve`
    /// -- which has no database handle -- still has to find it, the same way
    /// `resolve_local_endpoint_key` reads the endpoint column.
    #[test]
    fn a_credential_the_keychain_refused_is_still_resolvable() {
        let service = "com.jarvis.test.intkeys.fallback";
        std::env::remove_var("GITHUB_TOKEN");
        assert_eq!(resolve_in(service, "github_token"), None);

        // What migrate_plaintext_in records when KeychainStore::put fails.
        remember_plaintext_fallback(service, "github_token", "ghp_left_in_sqlite");
        assert_eq!(
            resolve_in(service, "github_token").as_deref(),
            Some("ghp_left_in_sqlite"),
            "a failed Keychain write must degrade to plaintext, not to nothing"
        );

        // The Keychain still wins the moment it holds the credential.
        KeychainStore::put(service, "github_token", "ghp_from_keychain").unwrap();
        assert_eq!(
            resolve_in(service, "github_token").as_deref(),
            Some("ghp_from_keychain")
        );
        KeychainStore::delete(service, "github_token").ok();

        // And a save through Settings replaces the credential outright, so the
        // remembered copy must not come back afterwards.
        let db = temp_db();
        store_in(service, &db, "github_token", "ghp_rotated").unwrap();
        KeychainStore::delete(service, "github_token").ok();
        assert_eq!(
            resolve_in(service, "github_token"),
            None,
            "a stale plaintext copy must not outlive the credential it stood for"
        );
    }

    /// The fallback is temporary by construction: the next startup that reaches
    /// an unlocked Keychain has to move the row and stop serving the copy.
    #[test]
    fn a_later_successful_migration_drops_the_plaintext_fallback() {
        let service = "com.jarvis.test.intkeys.fallback.migrate";
        std::env::remove_var("MINIMAX_API_KEY");
        let db = temp_db();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO user_preferences (key, value) VALUES ('minimax_api_key', 'mm_legacy_secret')",
                [],
            )
            .unwrap();
        }

        // Stand in for the pass where the Keychain was locked: the row is still
        // plaintext and the fallback is the only thing keeping MiniMax alive.
        remember_plaintext_fallback(service, "minimax_api_key", "mm_legacy_secret");
        assert_eq!(
            resolve_in(service, "minimax_api_key").as_deref(),
            Some("mm_legacy_secret")
        );

        migrate_plaintext_in(service, &db);
        assert_eq!(
            read_pref(&db, "minimax_api_key").as_deref(),
            Some(KEYCHAIN_MARKER),
            "the plaintext row must be gone once the Keychain accepts the secret"
        );
        assert_eq!(
            KeychainStore::get(service, "minimax_api_key").unwrap(),
            "mm_legacy_secret"
        );

        // With the Keychain item removed there must be nothing left to serve.
        KeychainStore::delete(service, "minimax_api_key").ok();
        assert_eq!(resolve_in(service, "minimax_api_key"), None);
    }

    #[test]
    fn resolve_returns_none_when_nothing_is_configured() {
        let service = "com.jarvis.test.intkeys.missing";
        std::env::remove_var("MINIMAX_API_KEY");
        assert_eq!(resolve_in(service, "minimax_api_key"), None);
    }

    fn read_endpoint_key(db: &Database, id: &str) -> Option<String> {
        db.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT api_key FROM local_endpoints WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
    }

    fn insert_endpoint(db: &Database, id: &str, api_key: Option<&str>) {
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO local_endpoints (id, name, url, api_key) VALUES (?1, ?1, 'http://x', ?2)",
                rusqlite::params![id, api_key],
            )
            .unwrap();
    }

    /// F004: the endpoint bearer token must round-trip through the Keychain,
    /// and the column must keep only the marker.
    #[test]
    fn a_local_endpoint_key_round_trips_through_the_keychain() {
        let service = "com.jarvis.test.intkeys.endpoint";
        let column = store_local_endpoint_key_in(service, "ep-1", Some("sk-endpoint-secret")).unwrap();

        assert_eq!(column.as_deref(), Some(KEYCHAIN_MARKER));
        assert_eq!(
            resolve_local_endpoint_key_in(service, "ep-1", column.as_deref()).as_deref(),
            Some("sk-endpoint-secret")
        );
        KeychainStore::delete(service, &local_endpoint_account("ep-1")).ok();
    }

    /// Two endpoints must not share one Keychain item, or saving a key on the
    /// second silently rewrites the first.
    #[test]
    fn endpoint_keys_are_scoped_per_endpoint() {
        let service = "com.jarvis.test.intkeys.endpoint.scope";
        let a = store_local_endpoint_key_in(service, "ep-a", Some("key-a")).unwrap();
        let b = store_local_endpoint_key_in(service, "ep-b", Some("key-b")).unwrap();

        assert_eq!(
            resolve_local_endpoint_key_in(service, "ep-a", a.as_deref()).as_deref(),
            Some("key-a")
        );
        assert_eq!(
            resolve_local_endpoint_key_in(service, "ep-b", b.as_deref()).as_deref(),
            Some("key-b")
        );
        assert_ne!(local_endpoint_account("ep-a"), local_endpoint_account("ep-b"));

        KeychainStore::delete(service, &local_endpoint_account("ep-a")).ok();
        KeychainStore::delete(service, &local_endpoint_account("ep-b")).ok();
    }

    #[test]
    fn an_endpoint_without_a_key_stores_nothing_and_resolves_to_none() {
        let service = "com.jarvis.test.intkeys.endpoint.blank";
        assert_eq!(store_local_endpoint_key_in(service, "ep-blank", None).unwrap(), None);
        assert_eq!(
            store_local_endpoint_key_in(service, "ep-blank", Some("   ")).unwrap(),
            None
        );
        assert_eq!(resolve_local_endpoint_key_in(service, "ep-blank", None), None);
        assert_eq!(resolve_local_endpoint_key_in(service, "ep-blank", Some("")), None);
    }

    /// A row written by a build older than this module holds the live key in
    /// the column. It has to keep working until the migration moves it.
    #[test]
    fn a_legacy_plaintext_column_still_resolves() {
        let service = "com.jarvis.test.intkeys.endpoint.legacy";
        assert_eq!(
            resolve_local_endpoint_key_in(service, "ep-legacy", Some("sk-plaintext")).as_deref(),
            Some("sk-plaintext")
        );
    }

    /// F004: `sqlite3 jarvis.db "select api_key from local_endpoints"` must not
    /// print a live key after startup, and a second pass must not clobber it.
    #[test]
    fn migration_moves_endpoint_keys_into_the_keychain_and_is_idempotent() {
        let service = "com.jarvis.test.intkeys.endpoint.migrate";
        let db = temp_db();
        insert_endpoint(&db, "ep-legacy-1", Some("sk-legacy-secret"));
        insert_endpoint(&db, "ep-nokey", None);

        migrate_plaintext_in(service, &db);

        assert_eq!(
            read_endpoint_key(&db, "ep-legacy-1").as_deref(),
            Some(KEYCHAIN_MARKER),
            "the plaintext key must be gone from SQLite"
        );
        assert_eq!(read_endpoint_key(&db, "ep-nokey"), None);
        assert_eq!(
            resolve_local_endpoint_key_in(service, "ep-legacy-1", Some(KEYCHAIN_MARKER)).as_deref(),
            Some("sk-legacy-secret")
        );

        migrate_plaintext_in(service, &db);
        assert_eq!(
            resolve_local_endpoint_key_in(service, "ep-legacy-1", Some(KEYCHAIN_MARKER)).as_deref(),
            Some("sk-legacy-secret"),
            "a second pass must not overwrite the Keychain item with the marker"
        );

        KeychainStore::delete(service, &local_endpoint_account("ep-legacy-1")).ok();
    }

    /// These names must stay in step with `SECRET_PREFERENCE_KEYS` in
    /// `commands::settings`, which is what keeps them out of `get_settings`.
    #[test]
    fn the_integration_credential_set_is_exactly_these() {
        let names: Vec<&str> = INTEGRATION_KEYS.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            names,
            vec![
                "github_token",
                "notion_api_key",
                "obsidian_api_key",
                "minimax_api_key",
                "typesafe_api_key",
            ]
        );
    }
}
