use crate::db::Database;
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
pub fn get_github_items(
    db: State<Arc<Database>>,
    item_type: Option<String>,
) -> Result<Vec<GitHubItemView>, String> {
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
    let items = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(GitHubItemView {
                id: row.get(0)?,
                item_type: row.get(1)?,
                title: row.get(2)?,
                repo: row.get(3)?,
                number: row.get(4)?,
                state: row.get(5)?,
                url: row.get(6)?,
                author: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(items)
}

/// The sync result string. A sync where only the review-requested search failed
/// still saved real data, so the items are reported -- but it is not a clean
/// success, and the message has to say so rather than let a stale review queue
/// pass for an empty one.
fn sync_summary(count: usize, warning: Option<&str>) -> String {
    match warning {
        Some(w) => format!("Partial sync: {} items saved, but {}", count, w),
        None => format!("Synced {} items", count),
    }
}

#[tauri::command]
pub async fn sync_github(db: State<'_, Arc<Database>>) -> Result<String, String> {
    let fetched = crate::integrations::github_auth::sync_connected(&db).await?;
    let count = fetched.items.len();
    Ok(sync_summary(count, fetched.warning.as_deref()))
}

#[tauri::command]
pub async fn save_github_token(db: State<'_, Arc<Database>>, token: String) -> Result<(), String> {
    crate::integrations::github_auth::save_manual(&db, &token).await
}

#[tauri::command]
pub async fn github_begin_connect(
    db: State<'_, Arc<Database>>,
) -> Result<crate::integrations::github_auth::DevicePrompt, String> {
    crate::integrations::github_auth::begin(&db).await
}
#[tauri::command]
pub async fn github_complete_connect(
    db: State<'_, Arc<Database>>,
    flow_id: String,
) -> Result<crate::integrations::github_auth::ConnectionStatus, String> {
    crate::integrations::github_auth::complete(&db, &flow_id).await
}
#[tauri::command]
pub fn github_cancel_connect() -> Result<(), String> {
    crate::integrations::github_auth::cancel()
}
#[tauri::command]
pub async fn github_connection_status(
    db: State<'_, Arc<Database>>,
) -> Result<crate::integrations::github_auth::ConnectionStatus, String> {
    crate::integrations::github_auth::status(&db).await
}
#[tauri::command]
pub async fn github_disconnect(db: State<'_, Arc<Database>>) -> Result<(), String> {
    crate::integrations::github_auth::disconnect(&db).await
}
#[tauri::command]
pub fn github_set_client_id(db: State<Arc<Database>>, client_id: String) -> Result<(), String> {
    crate::integrations::github_auth::configure_client(&db, &client_id)
}

#[tauri::command]
pub fn get_github_stats(db: State<Arc<Database>>) -> Result<GitHubStats, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let open_prs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM github_items WHERE item_type = 'pr' AND state = 'open'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let assigned_issues: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM github_items WHERE item_type = 'issue' AND state = 'open'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let review_requested: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM github_items WHERE item_type = 'pr_review'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(GitHubStats {
        open_prs,
        assigned_issues,
        review_requested,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_sync_reports_only_the_count() {
        assert_eq!(sync_summary(12, None), "Synced 12 items");
    }

    #[test]
    fn a_half_failed_sync_is_never_reported_as_a_clean_one() {
        // Keeping the 12 assigned issues is the point -- but the caller must be
        // able to tell this apart from a sync where nothing failed.
        let summary = sync_summary(
            12,
            Some("the PR review list was not updated: GitHub search is rate limited (403), retry after 60s"),
        );
        assert!(summary.contains("Partial sync"));
        assert!(summary.contains("12 items saved"));
        assert!(summary.contains("PR review list was not updated"));
        assert_ne!(summary, sync_summary(12, None));
    }
}
