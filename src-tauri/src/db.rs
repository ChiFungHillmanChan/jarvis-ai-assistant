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
