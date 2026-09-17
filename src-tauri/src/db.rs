use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("migrations");
}

/// WAL mode with no ceiling lets `jarvis.db-wal` outgrow the database itself --
/// it was already 4 MB against a 768 KB database on the author's machine, and
/// JARVIS is a login-item process that stays open for days. `journal_size_limit`
/// truncates the WAL back down after each checkpoint; `wal_autocheckpoint` keeps
/// those checkpoints frequent enough that the limit is actually reached.
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

impl Database {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = Self::db_path()?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(&db_path)?;
        conn.execute_batch(CONNECTION_PRAGMAS)?;
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
