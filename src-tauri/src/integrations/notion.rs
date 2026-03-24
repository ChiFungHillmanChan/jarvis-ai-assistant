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

pub async fn get_page_content(api_key: &str, page_id: &str) -> Result<String, String> {
    let client = Client::new();

    let resp = client
        .get(&format!("{}/blocks/{}/children?page_size=100", NOTION_API, page_id))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Notion-Version", NOTION_VERSION)
        .send()
        .await
        .map_err(|e| format!("Notion blocks error: {}", e))?;

    if resp.status() == 401 {
        return Err("UNAUTHORIZED: Invalid Notion API key".to_string());
    }
    if !resp.status().is_success() {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("Notion API error {}: {}", s, t));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
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
            let text: String = arr.iter()
                .filter_map(|item| item.get("plain_text").and_then(|t| t.as_str()))
                .collect();
            if !text.is_empty() { return Some(text); }
        }
    }
    None
}
