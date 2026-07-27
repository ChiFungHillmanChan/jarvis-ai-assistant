use crate::db::Database;
use crate::security::keychain::KeychainStore;
use std::sync::Arc;

const KEYCHAIN_SERVICE: &str = "com.jarvis.aws";
const KEYCHAIN_ACCOUNT: &str = "secret_access_key";

#[derive(Clone, Debug)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    pub inference_profile: String,
}

pub struct AwsCredsRepo {
    db: Arc<Database>,
}

/// Bedrock cross-region inference profile ids carry the geography they run in
/// (`apac.`, `us.`, `eu.`), and a profile from one geography is rejected by
/// another geography's endpoint, so the profile is fully derived from the
/// region. Unrecognised regions keep "apac", the column default in V13.
///
/// Builds before 2026-07 stored a hardcoded "apac" next to whatever region the
/// user picked. V18 repairs those rows on upgrade; deriving it here as well
/// means a row this binary did not write -- an older build run against an
/// already-migrated database, for instance -- cannot resurrect the mismatch.
pub fn inference_profile_for_region(region: &str) -> &'static str {
    if region.starts_with("us-") {
        "us"
    } else if region.starts_with("eu-") {
        "eu"
    } else {
        "apac"
    }
}

impl AwsCredsRepo {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn save(&self, c: &AwsCredentials) -> Result<(), String> {
        // SQL row first; if it fails we never wrote a secret to the keychain that
        // points at no row. Keychain second; if it fails the SQL row reflects
        // intent and the user can re-save to repair.
        let now = chrono::Utc::now().timestamp();
        {
            let conn = self.db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
            conn.execute(
                "INSERT INTO aws_credentials (id, access_key_id, secret_access_key, region, inference_profile, updated_at)
                 VALUES (1, ?1, '__keychain__', ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET
                   access_key_id = excluded.access_key_id,
                   region = excluded.region,
                   inference_profile = excluded.inference_profile,
                   updated_at = excluded.updated_at",
                rusqlite::params![&c.access_key_id, &c.region, &c.inference_profile, now],
            )
            .map_err(|e| format!("db save: {e}"))?;
        }
        KeychainStore::put(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, &c.secret_access_key)?;
        Ok(())
    }

    pub async fn load(&self) -> Result<Option<AwsCredentials>, String> {
        let conn = self.db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
        let row = conn.query_row(
            "SELECT access_key_id, region, inference_profile FROM aws_credentials WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        );
        let (akid, region, stored_profile) = match row {
            Ok(t) => t,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(format!("db load: {e}")),
        };
        // Drop the lock before calling keychain (no mutex held across FFI boundary)
        drop(conn);
        let secret = KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?;
        let profile = inference_profile_for_region(&region);
        if profile != stored_profile {
            log::warn!(
                "aws_credentials.inference_profile '{}' does not match region '{}'; using '{}'",
                stored_profile,
                region,
                profile
            );
        }
        Ok(Some(AwsCredentials {
            access_key_id: akid,
            secret_access_key: secret,
            region,
            inference_profile: profile.to_string(),
        }))
    }

    pub async fn clear(&self) -> Result<(), String> {
        {
            let conn = self.db.conn.lock().map_err(|e| format!("db lock: {e}"))?;
            conn.execute("DELETE FROM aws_credentials WHERE id = 1", [])
                .map_err(|e| format!("db clear: {e}"))?;
        }
        let _ = KeychainStore::delete(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    /// The backfill migration, read from the file refinery actually embeds, so
    /// the test cannot drift from the SQL that ships.
    const V18_BACKFILL: &str = include_str!("../../migrations/V18__inference_profile_backfill.sql");

    /// Under `cfg(test)` the keychain backend is a process-global map, and every
    /// test here writes the same (service, account) entry and then clears it.
    /// Without this, two of them running concurrently delete each other's secret.
    static KEYCHAIN_GUARD: Mutex<()> = Mutex::new(());

    fn fresh_conn() -> Connection {
        // In-memory, not a temp file: this fixture exercises no file-backed
        // behaviour, and the previous version left a .db (plus -wal and -shm,
        // because it enabled WAL) behind in $TMPDIR on every single run.
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys=ON;").expect("pragmas");
        // Apply only the aws_credentials table (subset of V13 needed for this test)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS aws_credentials (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                access_key_id TEXT NOT NULL,
                secret_access_key TEXT NOT NULL,
                region TEXT NOT NULL DEFAULT 'ap-southeast-1',
                inference_profile TEXT NOT NULL DEFAULT 'apac',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )
        .expect("create aws_credentials table");
        conn
    }

    fn fresh_db() -> Arc<Database> {
        Arc::new(Database {
            conn: Mutex::new(fresh_conn()),
        })
    }

    #[tokio::test]
    async fn save_and_load_round_trip() {
        let _guard = KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let db = fresh_db();
        let repo = AwsCredsRepo::new(db);
        let creds = AwsCredentials {
            access_key_id: "AKIATEST".to_string(),
            secret_access_key: "SECRETTEST".to_string(),
            region: "ap-southeast-1".to_string(),
            inference_profile: "apac".to_string(),
        };
        repo.save(&creds).await.unwrap();
        let loaded = repo.load().await.unwrap().unwrap();
        assert_eq!(loaded.access_key_id, "AKIATEST");
        assert_eq!(loaded.secret_access_key, "SECRETTEST");
        assert_eq!(loaded.region, "ap-southeast-1");
        assert_eq!(loaded.inference_profile, "apac");
        repo.clear().await.unwrap();
        assert!(repo.load().await.unwrap().is_none());
    }

    #[test]
    fn v18_backfill_matches_the_stored_profile_to_the_stored_region() {
        // A pre-fix install stored 'apac' regardless of region. After the
        // upgrade the value is consumed, so a us-east-1 row must be repaired or
        // every volume-tier Bedrock request fails with a ValidationException.
        for (region, expected) in [
            ("us-east-1", "us"),
            ("eu-west-1", "eu"),
            ("ap-southeast-1", "apac"),
            ("ap-northeast-1", "apac"),
            ("sa-east-1", "apac"),
        ] {
            let conn = fresh_conn();
            conn.execute(
                "INSERT INTO aws_credentials (id, access_key_id, secret_access_key, region, inference_profile, updated_at)
                 VALUES (1, 'AKIATEST', '__keychain__', ?1, 'apac', 0)",
                rusqlite::params![region],
            )
            .expect("seed legacy row");
            conn.execute_batch(V18_BACKFILL).expect("run V18");
            let got: String = conn
                .query_row("SELECT inference_profile FROM aws_credentials WHERE id = 1", [], |r| r.get(0))
                .expect("read back");
            assert_eq!(got, expected, "region {region}");
            // The migration is the same mapping the write path uses.
            assert_eq!(got, inference_profile_for_region(region), "region {region}");
        }
    }

    #[test]
    fn v18_backfill_is_a_no_op_on_an_empty_table() {
        let conn = fresh_conn();
        conn.execute_batch(V18_BACKFILL).expect("run V18");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM aws_credentials", [], |r| r.get(0))
            .expect("count");
        assert_eq!(rows, 0);
    }

    #[tokio::test]
    async fn load_derives_the_profile_from_the_region() {
        let _guard = KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        // A row this binary did not write -- an older build saving over an
        // already-migrated database -- must not put an APAC profile back in
        // front of a US endpoint.
        let db = fresh_db();
        let repo = AwsCredsRepo::new(db);
        let legacy = AwsCredentials {
            access_key_id: "AKIATEST".to_string(),
            secret_access_key: "SECRETTEST".to_string(),
            region: "us-east-1".to_string(),
            inference_profile: "apac".to_string(),
        };
        repo.save(&legacy).await.unwrap();
        let loaded = repo.load().await.unwrap().unwrap();
        assert_eq!(loaded.region, "us-east-1");
        assert_eq!(loaded.inference_profile, "us");
        repo.clear().await.unwrap();
    }
}
