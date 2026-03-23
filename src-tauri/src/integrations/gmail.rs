use reqwest::Client;
use serde::{Deserialize, Serialize};

const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

#[derive(Debug, Serialize, Deserialize)]
pub struct GmailMessage {
    pub id: String,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub snippet: Option<String>,
    pub label_ids: Vec<String>,
    pub is_read: bool,
    pub received_at: Option<String>,
}

#[derive(Deserialize)]
struct ListResponse { messages: Option<Vec<MessageRef>> }

#[derive(Deserialize)]
struct MessageRef { id: String }

#[derive(Deserialize)]
struct MessageDetail {
    id: String,
    #[serde(rename = "threadId")]
    thread_id: Option<String>,
    snippet: Option<String>,
    #[serde(rename = "labelIds")]
    label_ids: Option<Vec<String>>,
    payload: Option<Payload>,
    #[serde(rename = "internalDate")]
    internal_date: Option<String>,
}

#[derive(Deserialize)]
struct Payload { headers: Option<Vec<Header>> }

#[derive(Deserialize)]
struct Header { name: String, value: String }

pub async fn fetch_inbox(access_token: &str, max_results: u32) -> Result<Vec<GmailMessage>, String> {
    let client = Client::new();
    let url = format!("{}/messages?maxResults={}&labelIds=INBOX", GMAIL_API, max_results);
    let resp = client.get(&url).bearer_auth(access_token).send().await.map_err(|e| format!("Gmail list error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { return Err(format!("Gmail API error: {}", resp.status())); }
    let list: ListResponse = resp.json().await.map_err(|e| e.to_string())?;
    let refs = list.messages.unwrap_or_default();
    let mut messages = Vec::new();
    for msg_ref in refs.iter().take(max_results as usize) {
        match fetch_message_detail(access_token, &msg_ref.id).await {
            Ok(msg) => messages.push(msg),
            Err(e) => log::warn!("Failed to fetch message {}: {}", msg_ref.id, e),
        }
    }
    Ok(messages)
}

async fn fetch_message_detail(access_token: &str, message_id: &str) -> Result<GmailMessage, String> {
    let client = Client::new();
    let url = format!("{}/messages/{}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date", GMAIL_API, message_id);
    let resp = client.get(&url).bearer_auth(access_token).send().await.map_err(|e| e.to_string())?;
    let detail: MessageDetail = resp.json().await.map_err(|e| e.to_string())?;
    let headers = detail.payload.and_then(|p| p.headers).unwrap_or_default();
    let subject = headers.iter().find(|h| h.name == "Subject").map(|h| h.value.clone());
    let sender = headers.iter().find(|h| h.name == "From").map(|h| h.value.clone());
    let date = headers.iter().find(|h| h.name == "Date").map(|h| h.value.clone());
    let labels = detail.label_ids.unwrap_or_default();
    let is_read = !labels.contains(&"UNREAD".to_string());
    Ok(GmailMessage { id: detail.id, thread_id: detail.thread_id, subject, sender, snippet: detail.snippet, label_ids: labels, is_read, received_at: date.or(detail.internal_date) })
}

pub async fn archive_message(access_token: &str, message_id: &str) -> Result<(), String> {
    let client = Client::new();
    let url = format!("{}/messages/{}/modify", GMAIL_API, message_id);
    let body = serde_json::json!({ "removeLabelIds": ["INBOX"] });
    let resp = client.post(&url).bearer_auth(access_token).json(&body).send().await.map_err(|e| format!("Gmail archive error: {}", e))?;
    if !resp.status().is_success() { return Err(format!("Gmail archive failed: {}", resp.status())); }
    Ok(())
}

pub fn save_to_db(db: &crate::db::Database, messages: &[GmailMessage]) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    for msg in messages {
        conn.execute(
            "INSERT INTO emails (gmail_id, thread_id, subject, sender, snippet, labels, is_read, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(gmail_id) DO UPDATE SET is_read = ?7, labels = ?6, synced_at = datetime('now')",
            rusqlite::params![msg.id, msg.thread_id, msg.subject, msg.sender, msg.snippet, msg.label_ids.join(","), msg.is_read as i32, msg.received_at],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}
