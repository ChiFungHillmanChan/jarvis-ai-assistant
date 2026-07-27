use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// The Obsidian Local REST API plugin's shipped default: an HTTPS listener on
/// loopback port 27124. Its plaintext HTTP listener is port 27123 and is off
/// unless the user turns it on by hand, so plain HTTP is not a usable default --
/// speaking HTTP at the TLS port never completes a handshake.
pub const DEFAULT_OBSIDIAN_URL: &str = "https://127.0.0.1:27124";

/// The endpoint to talk to: the `obsidian_base_url` preference when the user
/// has set one (the plugin lets them change scheme and port), otherwise the
/// shipped default. A blank row is treated as unset.
pub fn resolve_base_url(db: &crate::db::Database) -> String {
    let configured = db.conn.lock().ok().and_then(|conn| {
        conn.query_row(
            "SELECT value FROM user_preferences WHERE key = 'obsidian_base_url'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
    });
    match configured {
        Some(url) if !url.trim().is_empty() => url.trim().trim_end_matches('/').to_string(),
        _ => DEFAULT_OBSIDIAN_URL.to_string(),
    }
}

/// Loopback-only client for the Obsidian plugin, which presents a self-signed
/// certificate for 127.0.0.1. Certificate validation is disabled for THIS client
/// only; every other integration keeps the verifying shared client in
/// `crate::util::http_client`.
static OBSIDIAN_HTTP: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .danger_accept_invalid_certs(true)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| Client::new())
});

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
pub async fn search_vault(
    base_url: &str,
    api_key: &str,
    query: &str,
) -> Result<Vec<ObsidianNote>, String> {
    let client = &*OBSIDIAN_HTTP;
    let url = format!("{}/search/simple/", base_url);

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
                Err(format!("Cannot connect to Obsidian at {}. Make sure the Local REST API plugin is enabled and Obsidian is running.", base_url))
            } else {
                Err(format!("Obsidian error: {}", e))
            }
        }
    }
}

/// Get a specific note's content
pub async fn get_note(base_url: &str, api_key: &str, path: &str) -> Result<String, String> {
    let client = &*OBSIDIAN_HTTP;
    let url = format!("{}/vault/{}", base_url, path);

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
pub async fn save_note(
    base_url: &str,
    api_key: &str,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let client = &*OBSIDIAN_HTTP;
    let url = format!("{}/vault/{}", base_url, path);

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
pub async fn list_files(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let client = &*OBSIDIAN_HTTP;
    let url = format!("{}/vault/", base_url);

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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    /// In-memory, not a file under /tmp: a file fixture leaks a database per
    /// test run because nothing deletes it, and the connection is all these
    /// tests need.
    fn temp_db() -> crate::db::Database {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE user_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .expect("create prefs table");
        crate::db::Database { conn: Mutex::new(conn) }
    }

    fn set_base_url(db: &crate::db::Database, value: &str) {
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO user_preferences (key, value) VALUES ('obsidian_base_url', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                rusqlite::params![value],
            )
            .unwrap();
    }

    #[test]
    fn absent_or_blank_override_falls_back_to_the_plugin_default() {
        let db = temp_db();
        assert_eq!(resolve_base_url(&db), DEFAULT_OBSIDIAN_URL);
        set_base_url(&db, "   ");
        assert_eq!(resolve_base_url(&db), DEFAULT_OBSIDIAN_URL);
    }

    /// F042: a user who moved the plugin off its default port needs a way in.
    #[test]
    fn a_configured_override_wins_and_loses_its_trailing_slash() {
        let db = temp_db();
        set_base_url(&db, "http://127.0.0.1:27123/");
        // A trailing slash would produce `//vault/`, which the plugin 404s.
        assert_eq!(resolve_base_url(&db), "http://127.0.0.1:27123");
    }
}
