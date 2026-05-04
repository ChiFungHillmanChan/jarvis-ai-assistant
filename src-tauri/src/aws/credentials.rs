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
        let (akid, region, profile) = match row {
            Ok(t) => t,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(format!("db load: {e}")),
        };
        // Drop the lock before calling keychain (no mutex held across FFI boundary)
        drop(conn);
        let secret = KeychainStore::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)?;
        Ok(Some(AwsCredentials {
            access_key_id: akid,
            secret_access_key: secret,
            region,
            inference_profile: profile,
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

    fn fresh_db() -> Arc<Database> {
        let path = std::env::temp_dir()
            .join(format!("jarvis-test-{}.db", uuid::Uuid::new_v4()));
        let conn = Connection::open(&path).expect("open temp db");
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .expect("pragmas");
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
        Arc::new(Database {
            conn: Mutex::new(conn),
        })
    }

    #[tokio::test]
    async fn save_and_load_round_trip() {
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
}
