// jarvis/src-tauri/src/integrations/notion.rs
use serde::{Deserialize, Serialize};

const NOTION_API: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";

/// Hard ceiling on search pagination: 10 pages of 50 results. Without it a
/// workspace that keeps returning `has_more` would turn one sync into an
/// unbounded request storm.
const MAX_SEARCH_PAGES: usize = 10;

/// Explicitly disconnected OAuth/token connections never revive an old
/// environment credential. MCP credentials are never sent to the REST API.
fn connection_method(db: &crate::db::Database) -> Result<Option<String>, String> {
    let method = super::notion_mcp::method(db)?;
    if method.as_deref() == Some("disconnected") {
        return Err("Notion 連接已解除，請先重新連接。".into());
    }
    if !matches!(method.as_deref(), None | Some("mcp") | Some("token")) {
        return Err("Notion 連接設定無效，請重新連接。".into());
    }
    Ok(method)
}

fn rest_token() -> Result<String, String> {
    crate::integration_keys::resolve("notion_api_key")
        .ok_or_else(|| "尚未連接 Notion，請先在設定中完成授權。".into())
}

pub async fn search_pages_connected(
    db: &crate::db::Database,
    query: Option<&str>,
    max_pages: usize,
) -> Result<Vec<NotionPage>, String> {
    let generation = super::notion_mcp::generation(db)?;
    let pages = if connection_method(db)?.as_deref() == Some("mcp") {
        super::notion_mcp::search_pages(db, query).await
    } else {
        search_pages_bounded(&rest_token()?, query, max_pages).await
    }?;
    super::notion_mcp::ensure_generation(db, &generation)?;
    Ok(pages)
}

pub async fn get_page_content_connected(
    db: &crate::db::Database,
    page_id: &str,
) -> Result<String, String> {
    let generation = super::notion_mcp::generation(db)?;
    let content = if connection_method(db)?.as_deref() == Some("mcp") {
        super::notion_mcp::get_page_content(db, page_id).await
    } else {
        get_page_content(&rest_token()?, page_id).await
    }?;
    super::notion_mcp::ensure_generation(db, &generation)?;
    Ok(content)
}

pub async fn create_page_connected(
    db: &crate::db::Database,
    parent_page_id: &str,
    title: &str,
    content: &str,
) -> Result<String, String> {
    if connection_method(db)?.as_deref() == Some("mcp") {
        Err("此 Notion 連接目前僅支援搜尋及讀取。請在 Notion 建立或編輯頁面。".into())
    } else {
        create_page(&rest_token()?, parent_page_id, title, content).await
    }
}

pub fn is_configured(db: &crate::db::Database) -> Result<bool, String> {
    match super::notion_mcp::method(db)?.as_deref() {
        Some("disconnected") => Ok(false),
        Some("mcp") => Ok(true),
        None | Some("token") => Ok(crate::integration_keys::resolve("notion_api_key").is_some()),
        _ => Err("Notion 連接設定無效，請重新連接。".into()),
    }
}

pub async fn sync_connected(db: &crate::db::Database) -> Result<String, String> {
    let generation = super::notion_mcp::generation(db)?;
    let pages = search_pages_connected(db, None, MAX_SEARCH_PAGES).await?;
    let count = pages.len();
    save_to_db_checked(db, &pages, Some(&generation))?;
    if super::notion_mcp::method(db)?.as_deref() == Some("mcp") {
        Ok(format!(
            "已同步 {} 個最近使用的 Notion 頁面；每次最多 50 個，並非完整工作區備份。",
            count
        ))
    } else {
        Ok(format!("已同步 {} 個 Notion 頁面。", count))
    }
}

/// Page budget for a search issued inside a chat turn. The model-facing
/// `search_notion` tool truncates its result to a few thousand bytes anyway, so
/// every page past the first is a sequential round-trip of latency the user
/// waits through and Notion quota spent on rows the model never sees.
pub const INTERACTIVE_SEARCH_PAGES: usize = 1;

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
    has_more: Option<bool>,
    next_cursor: Option<String>,
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

/// The cursor to request next, or None once Notion says the result set is
/// complete. A missing or empty `next_cursor` ends the loop even if `has_more`
/// claims otherwise, so a malformed response cannot spin the pagination loop.
fn next_search_cursor(has_more: Option<bool>, next_cursor: Option<String>) -> Option<String> {
    match (has_more, next_cursor) {
        (Some(true), Some(next)) if !next.is_empty() => Some(next),
        _ => None,
    }
}

/// How many pages a search may actually request: at least one, never more than
/// the hard ceiling, whatever the caller asks for.
fn effective_page_budget(max_pages: usize) -> usize {
    max_pages.clamp(1, MAX_SEARCH_PAGES)
}

/// Full-workspace search, used by the background syncs that need every page.
/// An interactive caller should use `search_pages_bounded` with a small budget:
/// this one can cost up to `MAX_SEARCH_PAGES` sequential round-trips.
pub async fn search_pages(api_key: &str, query: Option<&str>) -> Result<Vec<NotionPage>, String> {
    search_pages_bounded(api_key, query, MAX_SEARCH_PAGES).await
}

/// Search with an explicit page budget, so the caller decides how much of the
/// workspace is worth the round-trips: a background sync wants everything, a
/// tool call inside a chat turn wants the first page.
pub async fn search_pages_bounded(
    api_key: &str,
    query: Option<&str>,
    max_pages: usize,
) -> Result<Vec<NotionPage>, String> {
    let client = crate::util::http_client();
    let mut pages: Vec<NotionPage> = Vec::new();
    // Notion caps a search at 100 results per request and paginates the rest, so
    // a single call indexes only the first slice of the workspace.
    let mut cursor: Option<String> = None;
    let budget = effective_page_budget(max_pages);

    for _ in 0..budget {
        let mut body = serde_json::json!({ "page_size": 50, "filter": { "property": "object", "value": "page" } });
        if let Some(q) = query {
            body["query"] = serde_json::Value::String(q.to_string());
        }
        if let Some(ref c) = cursor {
            body["start_cursor"] = serde_json::Value::String(c.clone());
        }

        let resp = client
            .post(&format!("{}/search", NOTION_API))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Notion-Version", NOTION_VERSION)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|_| "無法連線至 Notion 進行搜尋，請檢查網絡後重試。")?;

        if resp.status() == 401 {
            return Err("Notion API 權杖無效或已失效，請更新連接設定。".to_string());
        }
        if !resp.status().is_success() {
            let s = resp.status();
            return Err(format!(
                "Notion 搜尋失敗（HTTP {}）。請確認頁面分享權限。",
                s.as_u16()
            ));
        }

        let search: SearchResponse = resp.json().await.map_err(|_| "Notion 搜尋回應格式無效。")?;

        pages.extend(search.results.into_iter().map(|r| {
            let title = extract_title(&r.properties).unwrap_or_else(|| "未命名頁面".to_string());
            NotionPage {
                notion_id: r.id,
                title,
                url: r.url,
                parent_type: r.parent.as_ref().and_then(|p| p.parent_type.clone()),
                parent_title: None,
                last_edited: r.last_edited_time,
                content_snippet: None,
            }
        }));

        cursor = next_search_cursor(search.has_more, search.next_cursor);
        if cursor.is_none() {
            break;
        }
    }

    if cursor.is_some() {
        log::warn!(
            "[notion] search stopped at the {}-page budget; {} pages indexed, workspace has more",
            budget,
            pages.len()
        );
    }

    Ok(pages)
}

pub async fn create_page(
    api_key: &str,
    parent_page_id: &str,
    title: &str,
    content: &str,
) -> Result<String, String> {
    let client = crate::util::http_client();
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
        .map_err(|_| "無法連線至 Notion 建立頁面。請先檢查 Notion，避免重複建立。")?;

    if !resp.status().is_success() {
        let s = resp.status();
        return Err(format!(
            "Notion 建立頁面失敗（HTTP {}）。請確認父頁面的寫入權限。",
            s.as_u16()
        ));
    }

    let result: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| "Notion 未回傳有效的建立結果。請先檢查 Notion，避免重複建立。")?;
    Ok(result["id"].as_str().unwrap_or("").to_string())
}

pub fn save_to_db(db: &crate::db::Database, pages: &[NotionPage]) -> Result<(), String> {
    save_to_db_checked(db, pages, None)
}

fn save_to_db_checked(
    db: &crate::db::Database,
    pages: &[NotionPage],
    expected: Option<&Option<String>>,
) -> Result<(), String> {
    use rusqlite::OptionalExtension;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    if let Some(expected) = expected {
        let current: Option<String> = conn
            .query_row(
                "SELECT value FROM user_preferences WHERE key='notion_connection_generation'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| "無法核對 Notion 連接狀態。")?;
        if &current != expected {
            return Err("Notion 連接已變更，已停止儲存舊連接的同步結果。".into());
        }
    }
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

pub async fn get_page_content(api_key: &str, page_id: &str) -> Result<String, String> {
    let client = crate::util::http_client();

    let resp = client
        .get(&format!(
            "{}/blocks/{}/children?page_size=100",
            NOTION_API, page_id
        ))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Notion-Version", NOTION_VERSION)
        .send()
        .await
        .map_err(|_| "無法連線至 Notion 讀取頁面，請檢查網絡後重試。")?;

    if resp.status() == 401 {
        return Err("Notion API 權杖無效或已失效，請更新連接設定。".to_string());
    }
    if !resp.status().is_success() {
        let s = resp.status();
        return Err(format!(
            "Notion 讀取頁面失敗（HTTP {}）。請確認頁面分享權限。",
            s.as_u16()
        ));
    }

    let body: serde_json::Value = resp.json().await.map_err(|_| "Notion 頁面回應格式無效。")?;
    let results = body["results"].as_array().unwrap_or(&Vec::new()).clone();

    let mut output = String::new();
    for block in &results {
        let block_type = block["type"].as_str().unwrap_or("");
        match block_type {
            "paragraph" => {
                let text = extract_rich_text(&block["paragraph"]["rich_text"]);
                output.push_str(&text);
                output.push('\n');
            }
            "heading_1" => {
                let text = extract_rich_text(&block["heading_1"]["rich_text"]);
                output.push_str(&format!("# {}\n", text));
            }
            "heading_2" => {
                let text = extract_rich_text(&block["heading_2"]["rich_text"]);
                output.push_str(&format!("## {}\n", text));
            }
            "heading_3" => {
                let text = extract_rich_text(&block["heading_3"]["rich_text"]);
                output.push_str(&format!("### {}\n", text));
            }
            "bulleted_list_item" => {
                let text = extract_rich_text(&block["bulleted_list_item"]["rich_text"]);
                output.push_str(&format!("- {}\n", text));
            }
            "numbered_list_item" => {
                let text = extract_rich_text(&block["numbered_list_item"]["rich_text"]);
                output.push_str(&format!("1. {}\n", text));
            }
            "to_do" => {
                let text = extract_rich_text(&block["to_do"]["rich_text"]);
                let checked = block["to_do"]["checked"].as_bool().unwrap_or(false);
                if checked {
                    output.push_str(&format!("- [x] {}\n", text));
                } else {
                    output.push_str(&format!("- [ ] {}\n", text));
                }
            }
            "code" => {
                let text = extract_rich_text(&block["code"]["rich_text"]);
                let lang = block["code"]["language"].as_str().unwrap_or("");
                output.push_str(&format!("```{}\n{}\n```\n", lang, text));
            }
            "divider" => {
                output.push_str("---\n");
            }
            _ => {
                // Skip unknown block types
            }
        }
    }

    Ok(output)
}

fn extract_rich_text(rich_text: &serde_json::Value) -> String {
    match rich_text.as_array() {
        Some(arr) => arr
            .iter()
            .filter_map(|item| item["plain_text"].as_str())
            .collect::<Vec<&str>>()
            .join(""),
        None => String::new(),
    }
}

fn extract_title(properties: &Option<serde_json::Value>) -> Option<String> {
    let props = properties.as_ref()?;
    // Try "title" property first (standard for pages)
    if let Some(title_prop) = props.get("title").or_else(|| props.get("Name")) {
        if let Some(arr) = title_prop.get("title").and_then(|t| t.as_array()) {
            let text: String = arr
                .iter()
                .filter_map(|item| item.get("plain_text").and_then(|t| t.as_str()))
                .collect();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_response_models_the_pagination_fields() {
        // Dropping has_more/next_cursor is what made a 300-page workspace look
        // like a 50-page one, so the schema shape is the regression guard.
        let raw = r#"{"results":[],"has_more":true,"next_cursor":"abc123"}"#;
        let parsed: SearchResponse = serde_json::from_str(raw).unwrap();
        assert!(parsed.results.is_empty());
        assert_eq!(parsed.has_more, Some(true));
        assert_eq!(parsed.next_cursor.as_deref(), Some("abc123"));
    }

    #[test]
    fn final_page_stops_the_cursor_loop() {
        let raw = r#"{"results":[],"has_more":false,"next_cursor":null}"#;
        let parsed: SearchResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(
            next_search_cursor(parsed.has_more, parsed.next_cursor),
            None
        );
        // A response that claims more pages but supplies no usable cursor must
        // still terminate rather than repeat the same request forever.
        assert_eq!(next_search_cursor(Some(true), None), None);
        assert_eq!(next_search_cursor(Some(true), Some(String::new())), None);
        assert_eq!(next_search_cursor(None, Some("abc".to_string())), None);
    }

    #[test]
    fn an_interactive_search_costs_one_round_trip() {
        // The tool result is truncated a few thousand bytes later, so paging the
        // whole workspace inside a chat turn buys nothing and costs the user the
        // wait plus Notion's 3-req/s budget.
        assert_eq!(INTERACTIVE_SEARCH_PAGES, 1);
        assert!(INTERACTIVE_SEARCH_PAGES < MAX_SEARCH_PAGES);
        assert_eq!(effective_page_budget(INTERACTIVE_SEARCH_PAGES), 1);
    }

    #[test]
    fn the_page_budget_is_clamped_at_both_ends() {
        // A zero budget must still make one request rather than silently return
        // an empty result set, and no caller may exceed the hard ceiling.
        assert_eq!(effective_page_budget(0), 1);
        assert_eq!(effective_page_budget(3), 3);
        assert_eq!(effective_page_budget(MAX_SEARCH_PAGES), MAX_SEARCH_PAGES);
        assert_eq!(effective_page_budget(1_000), MAX_SEARCH_PAGES);
    }

    #[test]
    fn more_pages_continue_the_cursor_loop() {
        assert_eq!(
            next_search_cursor(Some(true), Some("cursor-2".to_string())),
            Some("cursor-2".to_string())
        );
    }

    #[test]
    fn an_old_sync_cannot_write_into_a_reconnected_workspace() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE user_preferences(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE notion_pages(id INTEGER PRIMARY KEY,notion_id TEXT,title TEXT,url TEXT,parent_type TEXT,parent_title TEXT,last_edited TEXT,content_snippet TEXT,synced_at TEXT);").unwrap();
        let db = crate::db::Database {
            conn: std::sync::Mutex::new(conn),
        };
        super::super::notion_mcp::set_method(&db, "mcp", false).unwrap();
        let original = super::super::notion_mcp::generation(&db).unwrap();
        super::super::notion_mcp::set_method(&db, "mcp", true).unwrap();
        let error = save_to_db_checked(&db, &[], Some(&original)).unwrap_err();
        assert!(error.contains("已停止儲存舊連接"));
    }
}
