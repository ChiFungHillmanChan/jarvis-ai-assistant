use reqwest::Client;
use serde::{Deserialize, Serialize};

const DEFAULT_OBSIDIAN_URL: &str = "http://localhost:27124";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ObsidianNote {
    pub path: String,
    pub content: Option<String>,
}

#[derive(Deserialize)]
struct SearchResult {
    filename: Option<String>,
    path: Option<String>,
    #[allow(dead_code)]
    score: Option<f64>,
    matches: Option<Vec<SearchMatch>>,
}

#[derive(Deserialize)]
struct SearchMatch {
    #[serde(rename = "match")]
    match_text: Option<SearchMatchText>,
}

#[derive(Deserialize)]
struct SearchMatchText {
    content: Option<String>,
}

/// Search notes in the Obsidian vault
pub async fn search_vault(api_key: &str, query: &str) -> Result<Vec<ObsidianNote>, String> {
    let client = Client::new();
    let url = format!("{}/search/simple/", DEFAULT_OBSIDIAN_URL);

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "query": query }))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let results: Vec<SearchResult> = r.json().await.map_err(|e| e.to_string())?;
            Ok(results
                .into_iter()
                .map(|r| {
                    let snippet = r.matches.and_then(|m| {
                        m.first()
                            .and_then(|m| m.match_text.as_ref().and_then(|t| t.content.clone()))
                    });
                    ObsidianNote {
                        path: r.path.or(r.filename).unwrap_or_default(),
                        content: snippet,
                    }
                })
                .collect())
        }
        Ok(r) => Err(format!("Obsidian API error: {}", r.status())),
        Err(e) => {
            if e.is_connect() {
                Err("Cannot connect to Obsidian. Make sure the Local REST API plugin is enabled and Obsidian is running.".to_string())
            } else {
                Err(format!("Obsidian error: {}", e))
            }
        }
    }
}

/// Get a specific note's content
pub async fn get_note(api_key: &str, path: &str) -> Result<String, String> {
    let client = Client::new();
    let url = format!("{}/vault/{}", DEFAULT_OBSIDIAN_URL, path);

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Accept", "text/markdown")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Obsidian error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Note not found: {}", path));
    }

    resp.text().await.map_err(|e| e.to_string())
}

/// Create or update a note
pub async fn save_note(api_key: &str, path: &str, content: &str) -> Result<(), String> {
    let client = Client::new();
    let url = format!("{}/vault/{}", DEFAULT_OBSIDIAN_URL, path);

    let resp = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "text/markdown")
        .body(content.to_string())
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Obsidian error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Failed to save note: {}", resp.status()));
    }
    Ok(())
}

/// List all files in vault
pub async fn list_files(api_key: &str) -> Result<Vec<String>, String> {
    let client = Client::new();
    let url = format!("{}/vault/", DEFAULT_OBSIDIAN_URL);

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Obsidian error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Obsidian API error: {}", resp.status()));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let files = body["files"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(files)
}
