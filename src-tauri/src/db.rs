use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("migrations");
}

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = Self::db_path()?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        embedded::migrations::runner().run(&mut conn)?;
        log::info!("Database initialized at {:?}", db_path);
        Ok(Database { conn: Mutex::new(conn) })
    }

    fn db_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let data_dir = dirs::data_dir()
            .ok_or("Could not find application support directory")?;
        Ok(data_dir.join("jarvis").join("jarvis.db"))
    }
}
