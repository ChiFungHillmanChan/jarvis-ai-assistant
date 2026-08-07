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

/// The overdue-first sort key. tasks.deadline is a plain local calendar date
/// ("YYYY-MM-DD", see the create_task tool schema in ai/tools.rs), so only the
/// 'now' side takes the modifier. date('now') on its own is UTC: for a UTC+8
/// user a task due today only floated to the top after 08:00 local, while
/// assistant::context already counted it as due today. Same definition as
/// assistant::context and scheduler::jobs. Shared with the test below.
const PENDING_TASKS_QUERY: &str = "SELECT id, title, description, deadline, priority, status, source, created_at
         FROM tasks WHERE status != 'completed'
         ORDER BY CASE WHEN deadline IS NOT NULL AND deadline <= date('now', 'localtime') THEN 0 ELSE 1 END, deadline ASC, priority DESC
         LIMIT 10";

#[tauri::command]
pub fn get_dashboard_data(db: State<Arc<Database>>) -> Result<DashboardData, String> {
    let name = crate::util::display_name(&db);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let hour: u32 = chrono::Local::now().hour();
    let greeting = match hour {
        0..=11 => format!("Good morning, {}.", name),
        12..=17 => format!("Good afternoon, {}.", name),
        _ => format!("Good evening, {}.", name),
    };
    let task_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks WHERE status != 'completed'", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(PENDING_TASKS_QUERY).map_err(|e| e.to_string())?;
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
    Ok(DashboardData { greeting, task_count, pending_tasks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// The dashboard's "overdue or due today floats to the top" bucket must use
    /// the same local day the assistant and the scheduler use. On the UTC day a
    /// UTC+8 user's task due today sat below tasks due later, all morning.
    #[test]
    fn overdue_bucket_follows_the_local_day() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 title TEXT NOT NULL,
                 description TEXT,
                 deadline TEXT,
                 priority INTEGER DEFAULT 2,
                 status TEXT DEFAULT 'pending',
                 source TEXT,
                 created_at TEXT NOT NULL DEFAULT (datetime('now')));
             INSERT INTO tasks (title, deadline, status) VALUES
                 ('Board deck', '2026-07-30', 'pending'),
                 ('File HKSTP milestone', '2026-07-27', 'pending'),
                 ('Yesterday invoice', '2026-07-26', 'pending'),
                 ('Draft pitch', NULL, 'pending');",
        )
        .unwrap();

        fn titles(conn: &Connection, sql: &str) -> Vec<String> {
            let mut stmt = conn.prepare(sql).unwrap();
            let rows: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            rows
        }

        // 'localtime' resolves against whatever machine runs the test, so pin
        // it to +08:00. Clock: 07:00 HK on 2026-07-27, still 07-26 in UTC.
        let pinned = PENDING_TASKS_QUERY
            .replace("'localtime'", "'+8 hours'")
            .replace("'now'", "'2026-07-26 23:00:00'");
        assert_eq!(
            titles(&conn, &pinned),
            vec![
                "Yesterday invoice".to_string(),
                "File HKSTP milestone".to_string(),
                "Draft pitch".to_string(),
                "Board deck".to_string()
            ],
            "today's task belongs in the overdue-first bucket from 00:00 local"
        );

        // On the UTC day the task due today fell out of that bucket and sorted
        // below an undated one, for the whole 00:00-08:00 local window.
        let utc_day =
            PENDING_TASKS_QUERY.replace("date('now', 'localtime')", "date('2026-07-26 23:00:00')");
        assert_eq!(
            titles(&conn, &utc_day),
            vec![
                "Yesterday invoice".to_string(),
                "Draft pitch".to_string(),
                "File HKSTP milestone".to_string(),
                "Board deck".to_string()
            ]
        );
    }
}
