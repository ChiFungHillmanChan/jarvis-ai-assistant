// jarvis/src-tauri/src/integrations/github.rs
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

fn extract_owner_repo(repo_url: Option<&str>) -> String {
    repo_url
        .and_then(|u| {
            let parts: Vec<&str> = u.rsplitn(3, '/').collect();
            if parts.len() >= 2 { Some(format!("{}/{}", parts[1], parts[0])) } else { None }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn issue_to_item(issue: IssueOrPR, item_type: &str, state_override: Option<&str>) -> GitHubItem {
    GitHubItem {
        github_id: issue.id,
        item_type: item_type.to_string(),
        title: issue.title,
        repo: extract_owner_repo(issue.repository_url.as_deref()),
        number: Some(issue.number),
        state: state_override.map(String::from).unwrap_or(issue.state),
        url: Some(issue.html_url),
        author: issue.user.map(|u| u.login),
        updated_at: issue.updated_at,
        ci_status: None,
    }
}

/// A fetch that got at least its required half. `warning` is set when the
/// review-requested search failed: `items` still holds the assigned issues that
/// were retrieved and are worth saving, but the PR-review half is missing.
pub struct AssignedItemsFetch {
    pub items: Vec<GitHubItem>,
    pub warning: Option<String>,
}

/// Assigned issues plus review-requested PRs, discarding the partial-failure
/// warning (which `fetch_assigned_items_detailed` has already logged). Callers
/// that report a sync result to the user should use the detailed form instead,
/// so a half-failed sync is not presented as a clean one.
pub async fn fetch_assigned_items(token: &str) -> Result<Vec<GitHubItem>, String> {
    Ok(fetch_assigned_items_detailed(token).await?.items)
}

pub async fn fetch_assigned_items_detailed(token: &str) -> Result<AssignedItemsFetch, String> {
    let client = crate::util::http_client();
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
        let item_type = if issue.pull_request.is_some() { "pr" } else { "issue" };
        items.push(issue_to_item(issue, item_type, None));
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

    // The search API has its own budget (30 requests/minute) separate from the
    // core API, so it is the flakier half. Its failure must not be swallowed --
    // an empty review queue reported as a complete sync is a lie -- but it must
    // not take the already-fetched assigned issues down with it either, so it
    // becomes a warning carried alongside the results.
    let status = resp.status();
    if !status.is_success() {
        let remaining = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = resp.text().await.unwrap_or_default();
        log::warn!(
            "[github] review-requested search failed: {} (x-ratelimit-remaining={}, retry-after={}) {}",
            status,
            remaining.as_deref().unwrap_or("unknown"),
            retry_after.as_deref().unwrap_or("none"),
            body
        );
        let warning = search_failure_warning(
            status.as_u16(),
            remaining.as_deref(),
            retry_after.as_deref(),
            &body,
        );
        return Ok(AssignedItemsFetch { items, warning: Some(warning) });
    }

    let search = match resp.json::<SearchResponse>().await {
        Ok(search) => search,
        Err(e) => {
            log::warn!("[github] review-requested search body unreadable: {}", e);
            return Ok(AssignedItemsFetch {
                items,
                warning: Some(format!(
                    "the PR review list was not updated: GitHub search response was unreadable: {}",
                    e
                )),
            });
        }
    };
    for pr in search.items {
        if items.iter().any(|i| i.github_id == pr.id) { continue; }
        items.push(issue_to_item(pr, "pr_review", Some("review_requested")));
    }

    Ok(AssignedItemsFetch { items, warning: None })
}

/// The user-facing half of a review-search failure. The full status, headers and
/// body go to the log; this string is what a sync surfaces, so it has to name
/// what is missing as well as why, and it has to keep a rate limit ("wait")
/// distinct from a permission or credential failure ("waiting will not help").
fn search_failure_warning(
    status: u16,
    rate_limit_remaining: Option<&str>,
    retry_after: Option<&str>,
    body: &str,
) -> String {
    let cause = if status == 401 {
        "the GitHub token was rejected (401)".to_string()
    } else if is_rate_limited(status, rate_limit_remaining, retry_after) {
        format!(
            "GitHub search is rate limited ({}), retry after {}s",
            status,
            retry_after.unwrap_or("60")
        )
    } else {
        format!("GitHub search failed ({}): {}", status, body)
    };
    format!("the PR review list was not updated: {}", cause)
}

/// GitHub answers 403 for both a permissions failure and a rate limit, so the
/// two are indistinguishable without reading the headers: a primary limit sets
/// `x-ratelimit-remaining: 0`, a secondary limit sets `retry-after`. A 429 is
/// unconditionally a rate limit.
fn is_rate_limited(
    status: u16,
    rate_limit_remaining: Option<&str>,
    retry_after: Option<&str>,
) -> bool {
    if status == 429 {
        return true;
    }
    status == 403 && (rate_limit_remaining == Some("0") || retry_after.is_some())
}

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<IssueOrPR>,
}

pub async fn fetch_items_filtered(
    token: &str,
    item_type: &str,
    repo: Option<&str>,
) -> Result<Vec<GitHubItem>, String> {
    let all_items = fetch_assigned_items(token).await?;

    let filtered: Vec<GitHubItem> = all_items
        .into_iter()
        .filter(|item| {
            let type_match = match item_type {
                "prs" => item.item_type == "pr" || item.item_type == "pr_review",
                "issues" => item.item_type == "issue",
                _ => true,
            };
            let repo_match = match repo {
                Some(r) => item.repo == r,
                None => true,
            };
            type_match && repo_match
        })
        .collect();

    Ok(filtered)
}

/// Fetch the repository metadata JSON as raw text. `ai/tools.rs` reads the
/// `private` flag out of it before filing a model-authored issue -- a public
/// repo would publish that body to the world.
pub async fn get_repo(token: &str, owner: &str, repo: &str) -> Result<String, String> {
    let client = crate::util::http_client();
    let resp = client
        .get(&format!("{}/repos/{}/{}", GITHUB_API, owner, repo))
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "JARVIS-App")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub repo lookup error: {}", e))?;

    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub repo lookup failed {}: {}", s, t));
    }

    resp.text().await.map_err(|e| e.to_string())
}

pub async fn create_issue(
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: Option<&str>,
    labels: Option<&str>,
) -> Result<String, String> {
    let client = crate::util::http_client();
    let mut payload = serde_json::json!({ "title": title, "body": body });
    if let Some(labels_str) = labels {
        let label_vec: Vec<&str> = labels_str.split(',').map(|s| s.trim()).collect();
        payload["labels"] = serde_json::json!(label_vec);
    }

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

/// The item types the assigned-issues call covers. Both come out of the same
/// `/issues?filter=assigned&state=open` request, so either it succeeded and both
/// are complete or `fetch_assigned_items_detailed` returned Err and nothing
/// reaches this function at all.
const ASSIGNED_TYPES: [&str; 2] = ["issue", "pr"];

pub fn save_to_db(db: &crate::db::Database, fetched: &AssignedItemsFetch) -> Result<(), String> {
    let mut conn = db.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for item in &fetched.items {
        tx.execute(
            "INSERT INTO github_items (github_id, item_type, title, repo, number, state, url, author, updated_at, ci_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(github_id, item_type) DO UPDATE SET
                title = ?3, state = ?6, updated_at = ?9, ci_status = ?10, synced_at = datetime('now')",
            rusqlite::params![item.github_id, item.item_type, item.title, item.repo, item.number, item.state, item.url, item.author, item.updated_at, item.ci_status],
        ).map_err(|e| e.to_string())?;
    }

    // Reconcile only the halves that actually came back. The review-requested
    // search runs on its own rate budget and fails independently (`warning`);
    // its rows are then stale rather than gone, and deleting them would empty
    // the review queue on every rate limit. The assigned half is always complete
    // here, so an issue that was closed or unassigned stops inflating the counts
    // instead of sitting in github_items forever.
    let mut reconciled: Vec<&str> = ASSIGNED_TYPES.to_vec();
    if fetched.warning.is_none() {
        reconciled.push("pr_review");
    }
    let live: HashSet<(i64, &str)> = fetched
        .items
        .iter()
        .map(|item| (item.github_id, item.item_type.as_str()))
        .collect();
    let gone: Vec<(i64, String)> = {
        let mut stmt = tx
            .prepare("SELECT github_id, item_type FROM github_items")
            .map_err(|e| e.to_string())?;
        let stored = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<(i64, String)>, _>>()
            .map_err(|e| e.to_string())?;
        stored
            .into_iter()
            .filter(|(github_id, item_type)| {
                reconciled.contains(&item_type.as_str())
                    && !live.contains(&(*github_id, item_type.as_str()))
            })
            .collect()
    };
    for (github_id, item_type) in gone {
        tx.execute(
            "DELETE FROM github_items WHERE github_id = ?1 AND item_type = ?2",
            rusqlite::params![github_id, item_type],
        )
        .map_err(|e| e.to_string())?;
    }

    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_and_secondary_search_limits_are_recognised() {
        // Primary limit: 403 with the remaining budget exhausted.
        assert!(is_rate_limited(403, Some("0"), None));
        // Secondary (abuse) limit: 403 with a retry-after and budget left.
        assert!(is_rate_limited(403, Some("27"), Some("60")));
        // 429 needs no headers at all.
        assert!(is_rate_limited(429, None, None));
    }

    #[test]
    fn a_failed_review_search_says_what_is_missing_and_why() {
        // The search half failing must stay visible -- the assigned issues are
        // still returned, so nothing else would tell the user the review queue
        // is stale rather than empty.
        let limited = search_failure_warning(403, Some("0"), None, "");
        assert!(limited.contains("PR review list was not updated"));
        assert!(limited.contains("rate limited"));

        // A scope failure is also a 403, but waiting does not fix it, so it must
        // not be dressed up as a rate limit.
        let forbidden = search_failure_warning(
            403,
            Some("29"),
            None,
            "Resource not accessible by personal access token",
        );
        assert!(forbidden.contains("PR review list was not updated"));
        assert!(!forbidden.contains("rate limited"));
        assert!(forbidden.contains("Resource not accessible"));

        let rejected = search_failure_warning(401, None, None, "Bad credentials");
        assert!(rejected.contains("PR review list was not updated"));
        assert!(rejected.contains("401"));

        let unavailable = search_failure_warning(503, None, None, "search backend unavailable");
        assert!(unavailable.contains("PR review list was not updated"));
        assert!(unavailable.contains("503"));
    }

    #[test]
    fn permission_failures_are_not_reported_as_rate_limits() {
        // A token without the right scope also gets 403, but with budget left
        // and no retry-after. Telling the user to wait would be wrong.
        assert!(!is_rate_limited(403, Some("29"), None));
        assert!(!is_rate_limited(403, None, None));
        assert!(!is_rate_limited(422, None, None));
        assert!(!is_rate_limited(503, None, None));
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn github_db() -> crate::db::Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE github_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                github_id INTEGER NOT NULL,
                item_type TEXT NOT NULL,
                title TEXT NOT NULL,
                repo TEXT NOT NULL,
                number INTEGER,
                state TEXT NOT NULL,
                url TEXT,
                author TEXT,
                updated_at TEXT,
                ci_status TEXT,
                synced_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(github_id, item_type)
            );",
        )
        .unwrap();
        crate::db::Database { conn: Mutex::new(conn) }
    }

    fn seed(db: &crate::db::Database, github_id: i64, item_type: &str) {
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO github_items (github_id, item_type, title, repo, state)
                 VALUES (?1, ?2, 'title', 'owner/repo', 'open')",
                rusqlite::params![github_id, item_type],
            )
            .unwrap();
    }

    fn stored(db: &crate::db::Database) -> Vec<(i64, String)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT github_id, item_type FROM github_items ORDER BY github_id, item_type")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<Vec<(i64, String)>, _>>()
            .unwrap();
        rows
    }

    fn item(github_id: i64, item_type: &str) -> GitHubItem {
        GitHubItem {
            github_id,
            item_type: item_type.to_string(),
            title: "title".to_string(),
            repo: "owner/repo".to_string(),
            number: Some(1),
            state: "open".to_string(),
            url: None,
            author: None,
            updated_at: None,
            ci_status: None,
        }
    }

    fn complete(items: Vec<GitHubItem>) -> AssignedItemsFetch {
        AssignedItemsFetch { items, warning: None }
    }

    #[test]
    fn an_item_that_left_the_fetch_is_dropped() {
        // The assigned query asks for state=open, so a closed or unassigned
        // issue simply stops coming back. Upsert alone left it inflating the
        // open-issue count forever.
        let db = github_db();
        seed(&db, 1, "issue");
        seed(&db, 10, "pr_review");
        save_to_db(&db, &complete(vec![item(2, "issue")])).unwrap();
        assert_eq!(stored(&db), vec![(2, "issue".to_string())]);
    }

    #[test]
    fn a_failed_review_search_leaves_the_review_queue_alone() {
        // The review search has its own rate budget and fails independently.
        // Its rows are then stale, not gone -- deleting them would empty the
        // review queue on every 403 while the assigned half still reconciles.
        let db = github_db();
        seed(&db, 1, "issue");
        seed(&db, 10, "pr_review");
        let fetched = AssignedItemsFetch {
            items: vec![item(2, "issue")],
            warning: Some(search_failure_warning(403, Some("0"), None, "")),
        };
        save_to_db(&db, &fetched).unwrap();
        assert_eq!(
            stored(&db),
            vec![(2, "issue".to_string()), (10, "pr_review".to_string())]
        );
    }

    #[test]
    fn a_re_fetched_item_is_restored() {
        let db = github_db();
        save_to_db(&db, &complete(vec![item(1, "issue")])).unwrap();
        save_to_db(&db, &complete(vec![])).unwrap();
        assert!(stored(&db).is_empty(), "an empty fetch clears the mirror");
        save_to_db(&db, &complete(vec![item(1, "issue")])).unwrap();
        assert_eq!(stored(&db), vec![(1, "issue".to_string())]);
    }
}
