use crate::db::Database;
use chrono::Timelike;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct DashboardData {
    pub greeting: String,
    pub task_count: i64,
    pub pending_tasks: Vec<crate::commands::tasks::Task>,
}

#[tauri::command]
pub fn get_dashboard_data(db: State<Arc<Database>>) -> Result<DashboardData, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let hour: u32 = chrono::Local::now().hour();
    let greeting = match hour {
        0..=11 => "Good morning, Hillman.",
        12..=17 => "Good afternoon, Hillman.",
        _ => "Good evening, Hillman.",
    };
    let task_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks WHERE status != 'completed'", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, title, description, deadline, priority, status, source, created_at
         FROM tasks WHERE status != 'completed'
         ORDER BY CASE WHEN deadline IS NOT NULL AND deadline <= date('now') THEN 0 ELSE 1 END, deadline ASC, priority DESC
         LIMIT 10"
    ).map_err(|e| e.to_string())?;
    let pending_tasks = stmt
        .query_map([], |row| {
            Ok(crate::commands::tasks::Task {
                id: row.get(0)?, title: row.get(1)?, description: row.get(2)?,
                deadline: row.get(3)?, priority: row.get(4)?, status: row.get(5)?,
                source: row.get(6)?, created_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(DashboardData { greeting: greeting.to_string(), task_count, pending_tasks })
}
