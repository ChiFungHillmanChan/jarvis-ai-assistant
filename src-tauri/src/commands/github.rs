use crate::db::Database;
use crate::integrations::github;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct GitHubItemView {
    pub id: i64,
    pub item_type: String,
    pub title: String,
    pub repo: String,
    pub number: Option<i32>,
    pub state: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Serialize)]
pub struct GitHubStats {
    pub open_prs: i64,
    pub assigned_issues: i64,
    pub review_requested: i64,
}

#[tauri::command]
pub fn get_github_items(db: State<Arc<Database>>, item_type: Option<String>) -> Result<Vec<GitHubItemView>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match item_type.as_deref() {
        Some(t) => (
            "SELECT id, item_type, title, repo, number, state, url, author, updated_at FROM github_items WHERE item_type = ?1 ORDER BY updated_at DESC LIMIT 50",
            vec![Box::new(t.to_string()) as Box<dyn rusqlite::types::ToSql>],
        ),
        None => (
            "SELECT id, item_type, title, repo, number, state, url, author, updated_at FROM github_items ORDER BY updated_at DESC LIMIT 50",
            vec![],
        ),
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let items = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(GitHubItemView { id: row.get(0)?, item_type: row.get(1)?, title: row.get(2)?, repo: row.get(3)?, number: row.get(4)?, state: row.get(5)?, url: row.get(6)?, author: row.get(7)?, updated_at: row.get(8)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(items)
}

#[tauri::command]
pub async fn sync_github(db: State<'_, Arc<Database>>) -> Result<String, String> {
    let token = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM user_preferences WHERE key = 'github_token'", [], |row| row.get::<_, String>(0))
            .map_err(|_| "GitHub token not configured".to_string())?
    };
    let items = github::fetch_assigned_items(&token).await?;
    let count = items.len();
    github::save_to_db(&db, &items)?;
    Ok(format!("Synced {} items", count))
}

#[tauri::command]
pub fn save_github_token(db: State<Arc<Database>>, token: String) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES ('github_token', ?1, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
        rusqlite::params![token],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_github_stats(db: State<Arc<Database>>) -> Result<GitHubStats, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let open_prs: i64 = conn.query_row("SELECT COUNT(*) FROM github_items WHERE item_type = 'pr' AND state = 'open'", [], |r| r.get(0)).unwrap_or(0);
    let assigned_issues: i64 = conn.query_row("SELECT COUNT(*) FROM github_items WHERE item_type = 'issue' AND state = 'open'", [], |r| r.get(0)).unwrap_or(0);
    let review_requested: i64 = conn.query_row("SELECT COUNT(*) FROM github_items WHERE item_type = 'pr_review'", [], |r| r.get(0)).unwrap_or(0);
    Ok(GitHubStats { open_prs, assigned_issues, review_requested })
}
