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
