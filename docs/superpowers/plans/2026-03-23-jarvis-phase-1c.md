# JARVIS Phase 1c: Notion, GitHub & Cron Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Notion API and GitHub API integrations with token-based auth, periodic sync via existing cron engine, and a cron dashboard page showing job history and status.

**Architecture:** Notion uses an API key (simpler than OAuth). GitHub uses a personal access token. Both tokens stored in user_preferences table. Each integration follows the existing pattern: Rust module under `integrations/`, Tauri commands under `commands/`, cron sync jobs registered in scheduler. New frontend pages for Notion, GitHub, and a Cron dashboard showing job runs.

**Tech Stack:** reqwest + serde (API calls), existing rusqlite/SQLite, existing tokio-cron-scheduler, React + TypeScript (new pages).

**Spec:** `docs/superpowers/specs/2026-03-23-jarvis-assistant-design.md`

**Depends on:** Phase 1a + 1b complete.

---

## File Structure (new/modified files only)

```
jarvis/
├── src/
│   ├── lib/
│   │   ├── types.ts                          # + NotionPage, GitHubItem types
│   │   └── commands.ts                       # + notion, github command wrappers
│   ├── components/
│   │   ├── StatsPanel.tsx                    # Update: add GitHub stat card
│   │   ├── GitHubCard.tsx                    # NEW: PR/issue counts
│   │   └── NotionCard.tsx                    # NEW: synced pages count
│   └── pages/
│       ├── CronDashboard.tsx                 # NEW: full cron job list + run history
│       ├── Settings.tsx                      # Update: Notion + GitHub token inputs
│       └── Dashboard.tsx                     # Update: timeline includes calendar events
├── .env.example                              # + NOTION_API_KEY, GITHUB_TOKEN
│
└── src-tauri/
    ├── migrations/
    │   └── V3__notion_github.sql             # NEW: notion_pages, github_items tables
    └── src/
        ├── lib.rs                            # Register new commands
        ├── integrations/
        │   ├── mod.rs                        # + notion, github modules
        │   ├── notion.rs                     # NEW: Notion API client
        │   └── github.rs                     # NEW: GitHub API client
        ├── scheduler/
        │   └── jobs.rs                       # + notion_sync, github_digest jobs
        └── commands/
            ├── mod.rs                        # + notion, github modules
            ├── notion.rs                     # NEW: get_notion_pages, sync_notion, save_notion_token
            └── github.rs                     # NEW: get_github_items, sync_github, save_github_token
```

---

## Task 1: V3 Database Migration

**Files:**
- Create: `jarvis/src-tauri/migrations/V3__notion_github.sql`

- [ ] **Step 1: Create migration**

```sql
-- jarvis/src-tauri/migrations/V3__notion_github.sql

CREATE TABLE IF NOT EXISTS notion_pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    notion_id TEXT UNIQUE NOT NULL,
    title TEXT NOT NULL,
    url TEXT,
    parent_type TEXT,
    parent_title TEXT,
    last_edited TEXT,
    content_snippet TEXT,
    synced_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_notion_edited ON notion_pages(last_edited DESC);

CREATE TABLE IF NOT EXISTS github_items (
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
);

CREATE INDEX idx_github_type ON github_items(item_type, state);
CREATE INDEX idx_github_repo ON github_items(repo);

-- Add sync jobs for Notion and GitHub
INSERT OR IGNORE INTO cron_jobs (name, schedule, action_type, parameters, status)
VALUES
    ('Notion Sync', '0 */10 * * * *', 'notion_sync', NULL, 'active'),
    ('GitHub Digest', '0 */10 * * * *', 'github_digest', NULL, 'active');
```

- [ ] **Step 2: Verify migration compiles**

```bash
cargo check --manifest-path jarvis/src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/src-tauri/migrations/V3__notion_github.sql
git commit -m "feat: add V3 migration for notion_pages and github_items tables"
```

---

## Task 2: Notion API Integration

**Files:**
- Create: `jarvis/src-tauri/src/integrations/notion.rs`
- Modify: `jarvis/src-tauri/src/integrations/mod.rs`

- [ ] **Step 1: Create integrations/notion.rs**

```rust
// jarvis/src-tauri/src/integrations/notion.rs
use reqwest::Client;
use serde::{Deserialize, Serialize};

const NOTION_API: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotionPage {
    pub notion_id: String,
    pub title: String,
    pub url: Option<String>,
    pub parent_type: Option<String>,
    pub parent_title: Option<String>,
    pub last_edited: Option<String>,
    pub content_snippet: Option<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    id: String,
    url: Option<String>,
    parent: Option<Parent>,
    last_edited_time: Option<String>,
    properties: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Parent {
    #[serde(rename = "type")]
    parent_type: Option<String>,
}

pub async fn search_pages(api_key: &str, query: Option<&str>) -> Result<Vec<NotionPage>, String> {
    let client = Client::new();
    let mut body = serde_json::json!({ "page_size": 50, "filter": { "property": "object", "value": "page" } });
    if let Some(q) = query {
        body["query"] = serde_json::Value::String(q.to_string());
    }

    let resp = client
        .post(&format!("{}/search", NOTION_API))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Notion-Version", NOTION_VERSION)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Notion search error: {}", e))?;

    if resp.status() == 401 {
        return Err("UNAUTHORIZED: Invalid Notion API key".to_string());
    }
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("Notion API error {}: {}", s, t));
    }

    let search: SearchResponse = resp.json().await.map_err(|e| e.to_string())?;

    Ok(search.results.into_iter().map(|r| {
        let title = extract_title(&r.properties).unwrap_or_else(|| "(Untitled)".to_string());
        NotionPage {
            notion_id: r.id,
            title,
            url: r.url,
            parent_type: r.parent.as_ref().and_then(|p| p.parent_type.clone()),
            parent_title: None,
            last_edited: r.last_edited_time,
            content_snippet: None,
        }
    }).collect())
}

pub async fn create_page(
    api_key: &str,
    parent_page_id: &str,
    title: &str,
    content: &str,
) -> Result<String, String> {
    let client = Client::new();
    let body = serde_json::json!({
        "parent": { "page_id": parent_page_id },
        "properties": {
            "title": [{ "text": { "content": title } }]
        },
        "children": [{
            "object": "block",
            "type": "paragraph",
            "paragraph": {
                "rich_text": [{ "text": { "content": content } }]
            }
        }]
    });

    let resp = client
        .post(&format!("{}/pages", NOTION_API))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Notion-Version", NOTION_VERSION)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Notion create error: {}", e))?;

    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("Notion create failed {}: {}", s, t));
    }

    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(result["id"].as_str().unwrap_or("").to_string())
}

pub fn save_to_db(db: &crate::db::Database, pages: &[NotionPage]) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    for page in pages {
        conn.execute(
            "INSERT INTO notion_pages (notion_id, title, url, parent_type, parent_title, last_edited, content_snippet)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(notion_id) DO UPDATE SET
                title = ?2, url = ?3, last_edited = ?6, content_snippet = ?7, synced_at = datetime('now')",
            rusqlite::params![page.notion_id, page.title, page.url, page.parent_type, page.parent_title, page.last_edited, page.content_snippet],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn extract_title(properties: &Option<serde_json::Value>) -> Option<String> {
    let props = properties.as_ref()?;
    // Try "title" property first (standard for pages)
    if let Some(title_prop) = props.get("title").or_else(|| props.get("Name")) {
        if let Some(arr) = title_prop.get("title").and_then(|t| t.as_array()) {
            let text: String = arr.iter()
                .filter_map(|item| item.get("plain_text").and_then(|t| t.as_str()))
                .collect();
            if !text.is_empty() { return Some(text); }
        }
    }
    None
}
```

- [ ] **Step 2: Add `pub mod notion;` to integrations/mod.rs**

Read and update `jarvis/src-tauri/src/integrations/mod.rs`.

- [ ] **Step 3: Verify compilation**

```bash
cargo check --manifest-path jarvis/src-tauri/Cargo.toml
```

- [ ] **Step 4: Commit**

```bash
git add jarvis/src-tauri/src/integrations/
git commit -m "feat: add Notion API integration with page search, create, and caching"
```

---

## Task 3: GitHub API Integration

**Files:**
- Create: `jarvis/src-tauri/src/integrations/github.rs`
- Modify: `jarvis/src-tauri/src/integrations/mod.rs`

- [ ] **Step 1: Create integrations/github.rs**

```rust
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
```

- [ ] **Step 2: Add `pub mod github;` to integrations/mod.rs**

- [ ] **Step 3: Verify compilation**

```bash
cargo check --manifest-path jarvis/src-tauri/Cargo.toml
```

- [ ] **Step 4: Commit**

```bash
git add jarvis/src-tauri/src/integrations/
git commit -m "feat: add GitHub API integration with assigned issues, review PRs, and issue creation"
```

---

## Task 4: Add Notion + GitHub Sync Jobs to Scheduler

**Files:**
- Modify: `jarvis/src-tauri/src/scheduler/jobs.rs`

- [ ] **Step 1: Add new job handlers**

Read `jarvis/src-tauri/src/scheduler/jobs.rs` and add `notion` and `github` to the imports and match arms.

Add to imports:
```rust
use crate::integrations::{calendar, gmail, notion, github};
```

Add these match arms in `run_job`:
```rust
"notion_sync" => run_notion_sync(db).await,
"github_digest" => run_github_digest(db).await,
```

Add these functions:
```rust
async fn run_notion_sync(db: &Arc<Database>) -> Result<String, String> {
    let token = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM user_preferences WHERE key = 'notion_api_key'", [], |row| row.get::<_, String>(0)).ok()
    };
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => return Ok("Skipped: Notion API key not configured".to_string()),
    };
    let pages = notion::search_pages(&token, None).await?;
    let count = pages.len();
    notion::save_to_db(db, &pages)?;
    Ok(format!("Synced {} Notion pages", count))
}

async fn run_github_digest(db: &Arc<Database>) -> Result<String, String> {
    let token = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM user_preferences WHERE key = 'github_token'", [], |row| row.get::<_, String>(0)).ok()
    };
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => return Ok("Skipped: GitHub token not configured".to_string()),
    };
    let items = github::fetch_assigned_items(&token).await?;
    let count = items.len();
    github::save_to_db(db, &items)?;
    Ok(format!("Synced {} GitHub items", count))
}
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check --manifest-path jarvis/src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/src-tauri/src/scheduler/jobs.rs
git commit -m "feat: add Notion sync and GitHub digest cron jobs"
```

---

## Task 5: New Tauri Commands (Notion + GitHub)

**Files:**
- Create: `jarvis/src-tauri/src/commands/notion.rs`
- Create: `jarvis/src-tauri/src/commands/github.rs`
- Modify: `jarvis/src-tauri/src/commands/mod.rs`
- Modify: `jarvis/src-tauri/src/lib.rs` (register commands)

- [ ] **Step 1: Create commands/notion.rs**

```rust
use crate::db::Database;
use crate::integrations::notion;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct NotionPageView {
    pub id: i64,
    pub notion_id: String,
    pub title: String,
    pub url: Option<String>,
    pub parent_type: Option<String>,
    pub last_edited: Option<String>,
}

#[tauri::command]
pub fn get_notion_pages(db: State<Arc<Database>>, limit: Option<u32>) -> Result<Vec<NotionPageView>, String> {
    let limit = limit.unwrap_or(50);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, notion_id, title, url, parent_type, last_edited FROM notion_pages ORDER BY last_edited DESC LIMIT ?1"
    ).map_err(|e| e.to_string())?;
    let pages = stmt.query_map(rusqlite::params![limit], |row| {
        Ok(NotionPageView { id: row.get(0)?, notion_id: row.get(1)?, title: row.get(2)?, url: row.get(3)?, parent_type: row.get(4)?, last_edited: row.get(5)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(pages)
}

#[tauri::command]
pub async fn sync_notion(db: State<'_, Arc<Database>>) -> Result<String, String> {
    let token = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT value FROM user_preferences WHERE key = 'notion_api_key'", [], |row| row.get::<_, String>(0))
            .map_err(|_| "Notion API key not configured".to_string())?
    };
    let pages = notion::search_pages(&token, None).await?;
    let count = pages.len();
    notion::save_to_db(&db, &pages)?;
    Ok(format!("Synced {} pages", count))
}

#[tauri::command]
pub fn save_notion_token(db: State<Arc<Database>>, token: String) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO user_preferences (key, value, updated_at) VALUES ('notion_api_key', ?1, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
        rusqlite::params![token],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_notion_stats(db: State<Arc<Database>>) -> Result<i64, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM notion_pages", [], |r| r.get(0)).map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Create commands/github.rs**

```rust
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
```

- [ ] **Step 3: Update commands/mod.rs**

Add `pub mod notion;` and `pub mod github;`.

- [ ] **Step 4: Register commands in lib.rs**

Add to `invoke_handler`:
```rust
commands::notion::get_notion_pages,
commands::notion::sync_notion,
commands::notion::save_notion_token,
commands::notion::get_notion_stats,
commands::github::get_github_items,
commands::github::sync_github,
commands::github::save_github_token,
commands::github::get_github_stats,
```

- [ ] **Step 5: Verify compilation**

```bash
cargo check --manifest-path jarvis/src-tauri/Cargo.toml
```

- [ ] **Step 6: Commit**

```bash
git add jarvis/src-tauri/src/commands/ jarvis/src-tauri/src/lib.rs
git commit -m "feat: add Notion and GitHub Tauri commands with token management"
```

---

## Task 6: Frontend Types & Command Wrappers

**Files:**
- Modify: `jarvis/src/lib/types.ts`
- Modify: `jarvis/src/lib/commands.ts`

- [ ] **Step 1: Add types to types.ts**

Append to `jarvis/src/lib/types.ts`:
```ts
export interface NotionPageView {
  id: number;
  notion_id: string;
  title: string;
  url: string | null;
  parent_type: string | null;
  last_edited: string | null;
}

export interface GitHubItemView {
  id: number;
  item_type: string;
  title: string;
  repo: string;
  number: number | null;
  state: string;
  url: string | null;
  author: string | null;
  updated_at: string | null;
}

export interface GitHubStats {
  open_prs: number;
  assigned_issues: number;
  review_requested: number;
}
```

- [ ] **Step 2: Add commands to commands.ts**

Append to `jarvis/src/lib/commands.ts` (update the type import line too):
```ts
// Notion
export async function getNotionPages(limit?: number): Promise<NotionPageView[]> { return invoke("get_notion_pages", { limit }); }
export async function syncNotion(): Promise<string> { return invoke("sync_notion"); }
export async function saveNotionToken(token: string): Promise<void> { return invoke("save_notion_token", { token }); }
export async function getNotionStats(): Promise<number> { return invoke("get_notion_stats"); }

// GitHub
export async function getGitHubItems(itemType?: string): Promise<GitHubItemView[]> { return invoke("get_github_items", { item_type: itemType }); }
export async function syncGitHub(): Promise<string> { return invoke("sync_github"); }
export async function saveGitHubToken(token: string): Promise<void> { return invoke("save_github_token", { token }); }
export async function getGitHubStats(): Promise<GitHubStats> { return invoke("get_github_stats"); }
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/src/lib/
git commit -m "feat: add frontend types and commands for Notion and GitHub"
```

---

## Task 7: Dashboard Components (GitHub + Notion Cards)

**Files:**
- Create: `jarvis/src/components/GitHubCard.tsx`
- Create: `jarvis/src/components/NotionCard.tsx`
- Modify: `jarvis/src/components/StatsPanel.tsx`

- [ ] **Step 1: Create GitHubCard.tsx**

```tsx
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { GitHubStats } from "../lib/types";

export default function GitHubCard() {
  const { data: stats } = useTauriCommand<GitHubStats>("get_github_stats");
  if (!stats) {
    return (
      <div className="panel" style={{ padding: 12 }}>
        <div className="label">GITHUB</div>
        <div style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 }}>--</div>
        <div style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 }}>not connected</div>
      </div>
    );
  }
  const total = stats.open_prs + stats.assigned_issues + stats.review_requested;
  return (
    <div className="panel" style={{ padding: 12 }}>
      <div className="label">GITHUB</div>
      <div style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 }}>{total}</div>
      <div style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 }}>
        {stats.open_prs} PRs / {stats.assigned_issues} issues
      </div>
      {stats.review_requested > 0 && (
        <div style={{ color: "rgba(255, 180, 0, 0.7)", fontSize: 9, marginTop: 4 }}>
          {stats.review_requested} review requested
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 2: Create NotionCard.tsx**

```tsx
import { useTauriCommand } from "../hooks/useTauriCommand";

export default function NotionCard() {
  const { data: count } = useTauriCommand<number>("get_notion_stats");
  return (
    <div className="panel" style={{ padding: 12 }}>
      <div className="label">NOTION</div>
      <div style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 }}>{count ?? "--"}</div>
      <div style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 }}>
        {count != null ? `page${count !== 1 ? "s" : ""} synced` : "not connected"}
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Update StatsPanel.tsx**

Replace `jarvis/src/components/StatsPanel.tsx`:
```tsx
import StatCard from "./StatCard";
import CalendarCard from "./CalendarCard";
import CronCard from "./CronCard";
import GitHubCard from "./GitHubCard";
import NotionCard from "./NotionCard";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { EmailStats } from "../lib/types";

interface StatsPanelProps { taskCount: number; }

export default function StatsPanel({ taskCount }: StatsPanelProps) {
  const { data: emailStats } = useTauriCommand<EmailStats>("get_email_stats");
  return (
    <div style={styles.container}>
      <StatCard label="TASKS" value={taskCount} detail="pending" />
      <StatCard label="EMAIL" value={emailStats?.unread ?? "--"} detail={emailStats ? `${emailStats.unread} unread` : "not connected"} />
      <CalendarCard />
      <GitHubCard />
      <NotionCard />
      <CronCard />
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  container: { width: 160, display: "flex", flexDirection: "column", gap: 10, overflowY: "auto" },
};
```

- [ ] **Step 4: Commit**

```bash
git add jarvis/src/components/GitHubCard.tsx jarvis/src/components/NotionCard.tsx jarvis/src/components/StatsPanel.tsx
git commit -m "feat: add GitHub and Notion cards to StatsPanel"
```

---

## Task 8: Cron Dashboard Page

**Files:**
- Create: `jarvis/src/pages/CronDashboard.tsx`
- Modify: `jarvis/src/App.tsx` (add route)

- [ ] **Step 1: Create CronDashboard.tsx**

```tsx
import { useState } from "react";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CronJobView, CronRunView } from "../lib/types";

export default function CronDashboard() {
  const { data: jobs } = useTauriCommand<CronJobView[]>("get_cron_jobs");
  const [selectedJob, setSelectedJob] = useState<number | null>(null);

  return (
    <div style={styles.container}>
      <div className="system-text" style={{ marginBottom: 16 }}>CRON JOBS</div>
      <div style={styles.grid}>
        <div style={styles.jobList}>
          {jobs?.map((job) => (
            <button key={job.id} onClick={() => setSelectedJob(job.id)}
              style={{ ...styles.jobCard, borderColor: selectedJob === job.id ? "rgba(0, 180, 255, 0.4)" : "rgba(0, 180, 255, 0.12)" }}>
              <div style={styles.jobHeader}>
                <span style={styles.jobName}>{job.name}</span>
                <span style={{ ...styles.jobStatus, color: job.status === "active" ? "rgba(16, 185, 129, 0.7)" : "rgba(255, 100, 100, 0.7)" }}>
                  {job.status.toUpperCase()}
                </span>
              </div>
              <div style={styles.jobMeta}>Schedule: {job.schedule}</div>
              {job.last_run && <div style={styles.jobMeta}>Last run: {new Date(job.last_run).toLocaleString()}</div>}
            </button>
          ))}
        </div>
        <div style={styles.runHistory}>
          {selectedJob ? <RunHistory jobId={selectedJob} /> : (
            <div style={styles.placeholder}>Select a job to view run history</div>
          )}
        </div>
      </div>
    </div>
  );
}

function RunHistory({ jobId }: { jobId: number }) {
  const { data: runs } = useTauriCommand<CronRunView[]>("get_cron_runs", { job_id: jobId, limit: 20 });
  if (!runs || runs.length === 0) {
    return <div style={styles.placeholder}>No runs yet</div>;
  }
  return (
    <div>
      <div className="label" style={{ marginBottom: 12 }}>RUN HISTORY</div>
      {runs.map((run) => (
        <div key={run.id} style={styles.runItem}>
          <div style={styles.runHeader}>
            <span style={{ ...styles.runStatus,
              color: run.status === "completed" ? "rgba(16, 185, 129, 0.7)" : run.status === "failed" ? "rgba(255, 100, 100, 0.7)" : "rgba(255, 180, 0, 0.7)"
            }}>{run.status.toUpperCase()}</span>
            <span style={styles.runTime}>{new Date(run.started_at).toLocaleString()}</span>
          </div>
          {run.result && <div style={styles.runDetail}>{run.result}</div>}
          {run.error && <div style={{ ...styles.runDetail, color: "rgba(255, 100, 100, 0.7)" }}>{run.error}</div>}
        </div>
      ))}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 24, height: "100%", overflowY: "auto" },
  grid: { display: "flex", gap: 16, height: "calc(100% - 40px)" },
  jobList: { width: 300, display: "flex", flexDirection: "column", gap: 8, overflowY: "auto" },
  jobCard: { background: "rgba(0, 180, 255, 0.02)", border: "1px solid", borderRadius: 8, padding: 12, cursor: "pointer", textAlign: "left" as const, width: "100%" },
  jobHeader: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6 },
  jobName: { color: "rgba(0, 180, 255, 0.8)", fontSize: 12, fontWeight: 500 },
  jobStatus: { fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1 },
  jobMeta: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 2 },
  runHistory: { flex: 1, overflowY: "auto" },
  placeholder: { color: "rgba(0, 180, 255, 0.3)", fontSize: 12, fontStyle: "italic", padding: 20 },
  runItem: { borderBottom: "1px solid rgba(0, 180, 255, 0.08)", padding: "10px 0" },
  runHeader: { display: "flex", justifyContent: "space-between", alignItems: "center" },
  runStatus: { fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1 },
  runTime: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10 },
  runDetail: { color: "rgba(0, 180, 255, 0.5)", fontSize: 11, marginTop: 4 },
};
```

- [ ] **Step 2: Add route to App.tsx**

Read `jarvis/src/App.tsx` and:
1. Add import: `import CronDashboard from "./pages/CronDashboard";`
2. In the `renderView` switch, add a case before `default`:
```tsx
case "cron": return <CronDashboard />;
```

Also read `jarvis/src/components/CronCard.tsx` and wrap the card content in a clickable div. The component currently renders a `<div className="panel">`. We need to make clicking it navigate to the cron page. The simplest approach: accept an `onNavigate` prop and call it on click.

Update `CronCard.tsx` to accept `onNavigate?: () => void` and add `onClick={onNavigate}` with `cursor: "pointer"` to the panel div.

Update `StatsPanel.tsx` to pass `onNavigate` from a new prop. Update `Dashboard.tsx` to pass a `setActiveView` callback down through StatsPanel to CronCard.

Alternatively (simpler): just add the route in App.tsx for now. Users can reach it via the sidebar -- read `Sidebar.tsx` and check if there's a suitable nav item to repurpose or add one. The cleanest option: keep the existing sidebar items and add a new case in App.tsx's `renderView` for `case "cron"`. Then the CronCard in StatsPanel is informational only.

- [ ] **Step 3: Commit**

```bash
git add jarvis/src/pages/CronDashboard.tsx jarvis/src/App.tsx
git commit -m "feat: add Cron Dashboard page with job list and run history"
```

---

## Task 9: Settings Page -- Notion & GitHub Token Inputs

**Files:**
- Modify: `jarvis/src/pages/Settings.tsx`

- [ ] **Step 1: Add Notion and GitHub token input panels**

Read `jarvis/src/pages/Settings.tsx` and add two new panels after the GOOGLE SERVICES panel.

Add imports:
```tsx
import { saveNotionToken, saveGitHubToken } from "../lib/commands";
```

Add state:
```tsx
const [notionToken, setNotionToken] = useState("");
const [githubToken, setGithubToken] = useState("");
const [notionSaved, setNotionSaved] = useState(false);
const [githubSaved, setGithubSaved] = useState(false);
```

Add handlers:
```tsx
async function handleSaveNotion() {
  if (!notionToken.trim()) return;
  await saveNotionToken(notionToken);
  setNotionSaved(true);
}
async function handleSaveGitHub() {
  if (!githubToken.trim()) return;
  await saveGitHubToken(githubToken);
  setGithubSaved(true);
}
```

Add JSX panels:
```tsx
<div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
  <div className="label" style={{ marginBottom: 12 }}>NOTION</div>
  <div style={styles.hint}>Enter your Notion integration token</div>
  <div style={{ display: "flex", gap: 8 }}>
    <input type="password" value={notionToken} onChange={(e) => { setNotionToken(e.target.value); setNotionSaved(false); }}
      placeholder="ntn_..." style={styles.tokenInput} />
    <button onClick={handleSaveNotion} style={styles.saveBtn}>{notionSaved ? "SAVED" : "SAVE"}</button>
  </div>
</div>

<div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
  <div className="label" style={{ marginBottom: 12 }}>GITHUB</div>
  <div style={styles.hint}>Enter your GitHub personal access token</div>
  <div style={{ display: "flex", gap: 8 }}>
    <input type="password" value={githubToken} onChange={(e) => { setGithubToken(e.target.value); setGithubSaved(false); }}
      placeholder="ghp_..." style={styles.tokenInput} />
    <button onClick={handleSaveGitHub} style={styles.saveBtn}>{githubSaved ? "SAVED" : "SAVE"}</button>
  </div>
</div>
```

Add these to the styles object:
```tsx
tokenInput: { flex: 1, background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "6px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)", outline: "none" },
saveBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 10 },
```

- [ ] **Step 2: Commit**

```bash
git add jarvis/src/pages/Settings.tsx
git commit -m "feat: add Notion and GitHub token inputs to Settings page"
```

---

## Summary

After completing all 9 tasks, Phase 1c delivers:

- **V3 migration** with notion_pages and github_items tables
- **Notion integration** -- search pages, create pages, cache locally, periodic sync
- **GitHub integration** -- fetch assigned issues, PRs for review, create issues, periodic sync
- **Cron sync jobs** -- Notion sync (10 min) and GitHub digest (10 min) added to scheduler
- **8 new Tauri commands** for Notion (4) and GitHub (4)
- **Dashboard** -- GitHub card, Notion card added to StatsPanel
- **Cron Dashboard** -- full page with job list and run history viewer
- **Settings** -- Notion API key and GitHub PAT input fields

**Phase 1 is now complete.** All 4 integrations (Email, Calendar, Notion, GitHub) are connected, the cron engine runs background syncs, and the holographic dashboard shows everything at a glance.
