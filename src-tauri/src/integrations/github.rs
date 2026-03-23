// jarvis/src-tauri/src/integrations/github.rs
use reqwest::Client;
use serde::{Deserialize, Serialize};

const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GitHubItem {
    pub github_id: i64,
    pub item_type: String,
    pub title: String,
    pub repo: String,
    pub number: Option<i32>,
    pub state: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub updated_at: Option<String>,
    pub ci_status: Option<String>,
}

#[derive(Deserialize)]
struct IssueOrPR {
    id: i64,
    title: String,
    number: i32,
    state: String,
    html_url: String,
    user: Option<User>,
    updated_at: Option<String>,
    repository_url: Option<String>,
    pull_request: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

pub async fn fetch_assigned_items(token: &str) -> Result<Vec<GitHubItem>, String> {
    let client = Client::new();
    let mut items = Vec::new();

    // Fetch assigned issues
    let resp = client
        .get(&format!("{}/issues?filter=assigned&state=open&per_page=50", GITHUB_API))
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "JARVIS-App")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub issues error: {}", e))?;

    if resp.status() == 401 {
        return Err("UNAUTHORIZED: Invalid GitHub token".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("GitHub API error: {}", resp.status()));
    }

    let issues: Vec<IssueOrPR> = resp.json().await.map_err(|e| e.to_string())?;

    for issue in issues {
        let repo = issue.repository_url.as_deref()
            .and_then(|u| {
                let parts: Vec<&str> = u.rsplitn(3, '/').collect();
                if parts.len() >= 2 { Some(format!("{}/{}", parts[1], parts[0])) } else { None }
            })
            .unwrap_or_else(|| "unknown".to_string());

        let item_type = if issue.pull_request.is_some() { "pr" } else { "issue" };

        items.push(GitHubItem {
            github_id: issue.id,
            item_type: item_type.to_string(),
            title: issue.title,
            repo,
            number: Some(issue.number),
            state: issue.state,
            url: Some(issue.html_url),
            author: issue.user.map(|u| u.login),
            updated_at: issue.updated_at,
            ci_status: None,
        });
    }

    // Fetch PRs for review
    let resp = client
        .get(&format!("{}/search/issues?q=is:open+is:pr+review-requested:@me&per_page=50", GITHUB_API))
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "JARVIS-App")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub PR review error: {}", e))?;

    if resp.status().is_success() {
        let search: SearchResponse = resp.json().await.map_err(|e| e.to_string())?;
        for pr in search.items {
            let repo = pr.repository_url.as_deref()
                .and_then(|u| {
                    let parts: Vec<&str> = u.rsplitn(3, '/').collect();
                    if parts.len() >= 2 { Some(format!("{}/{}", parts[1], parts[0])) } else { None }
                })
                .unwrap_or_else(|| "unknown".to_string());

            // Skip if already in items
            if items.iter().any(|i| i.github_id == pr.id) { continue; }

            items.push(GitHubItem {
                github_id: pr.id,
                item_type: "pr_review".to_string(),
                title: pr.title,
                repo,
                number: Some(pr.number),
                state: "review_requested".to_string(),
                url: Some(pr.html_url),
                author: pr.user.map(|u| u.login),
                updated_at: pr.updated_at,
                ci_status: None,
            });
        }
    }

    Ok(items)
}

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<IssueOrPR>,
}

pub async fn create_issue(
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: Option<&str>,
) -> Result<String, String> {
    let client = Client::new();
    let payload = serde_json::json!({ "title": title, "body": body });

    let resp = client
        .post(&format!("{}/repos/{}/{}/issues", GITHUB_API, owner, repo))
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "JARVIS-App")
        .header("Accept", "application/vnd.github+json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("GitHub create issue error: {}", e))?;

    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub create issue failed {}: {}", s, t));
    }

    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(result["html_url"].as_str().unwrap_or("").to_string())
}

pub fn save_to_db(db: &crate::db::Database, items: &[GitHubItem]) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    for item in items {
        conn.execute(
            "INSERT INTO github_items (github_id, item_type, title, repo, number, state, url, author, updated_at, ci_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(github_id, item_type) DO UPDATE SET
                title = ?3, state = ?6, updated_at = ?9, ci_status = ?10, synced_at = datetime('now')",
            rusqlite::params![item.github_id, item.item_type, item.title, item.repo, item.number, item.state, item.url, item.author, item.updated_at, item.ci_status],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}
