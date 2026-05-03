use crate::db::Database;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn check_onboarding_status(db: State<Arc<Database>>) -> Result<bool, String> {
    read_onboarding_complete(&db)
}

#[tauri::command]
pub fn mark_onboarding_complete(db: State<Arc<Database>>) -> Result<(), String> {
    write_onboarding_complete(&db)
}

fn read_onboarding_complete(db: &Arc<Database>) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM user_preferences WHERE key='onboarding_complete'",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(matches!(value.as_deref(), Some("true")))
}

fn write_onboarding_complete(db: &Arc<Database>) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value) VALUES ('onboarding_complete', 'true')
         ON CONFLICT(key) DO UPDATE SET value='true'",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn make_db_with_pref(value: Option<&str>) -> Arc<Database> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE user_preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        if let Some(v) = value {
            conn.execute(
                "INSERT INTO user_preferences (key, value) VALUES ('onboarding_complete', ?1)",
                rusqlite::params![v],
            )
            .unwrap();
        }
        Arc::new(Database {
            conn: Mutex::new(conn),
        })
    }

    #[test]
    fn check_returns_false_when_value_missing() {
        let db = make_db_with_pref(None);
        assert!(!read_onboarding_complete(&db).unwrap());
    }

    #[test]
    fn check_returns_false_when_value_is_false() {
        let db = make_db_with_pref(Some("false"));
        assert!(!read_onboarding_complete(&db).unwrap());
    }

    #[test]
    fn check_returns_true_when_value_is_true() {
        let db = make_db_with_pref(Some("true"));
        assert!(read_onboarding_complete(&db).unwrap());
    }

    #[test]
    fn mark_sets_value_to_true() {
        let db = make_db_with_pref(Some("false"));
        write_onboarding_complete(&db).unwrap();
        assert!(read_onboarding_complete(&db).unwrap());
    }
}
