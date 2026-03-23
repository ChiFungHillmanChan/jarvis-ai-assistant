use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

#[derive(Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub deadline: Option<String>,
    pub priority: i32,
    pub status: String,
    pub source: String,
    pub created_at: Option<String>,
}

#[tauri::command]
pub fn get_tasks(db: State<Arc<Database>>) -> Result<Vec<Task>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, title, description, deadline, priority, status, source, created_at FROM tasks ORDER BY priority DESC, deadline ASC")
        .map_err(|e| e.to_string())?;
    let tasks = stmt
        .query_map([], |row| {
            Ok(Task {
                id: row.get(0)?, title: row.get(1)?, description: row.get(2)?,
                deadline: row.get(3)?, priority: row.get(4)?, status: row.get(5)?,
                source: row.get(6)?, created_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(tasks)
}

#[tauri::command]
pub fn create_task(db: State<Arc<Database>>, title: String, description: Option<String>, deadline: Option<String>, priority: i32) -> Result<i64, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO tasks (title, description, deadline, priority) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![title, description, deadline, priority],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn update_task(db: State<Arc<Database>>, id: i64, status: String) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE tasks SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
        rusqlite::params![status, id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}
