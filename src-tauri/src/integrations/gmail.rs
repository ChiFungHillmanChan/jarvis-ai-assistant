use base64::Engine;
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
struct Payload {
    headers: Option<Vec<Header>>,
    parts: Option<Vec<Part>>,
    body: Option<BodyData>,
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
}

#[derive(Deserialize)]
struct Part {
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
    body: Option<BodyData>,
    parts: Option<Vec<Part>>,
}

#[derive(Deserialize)]
struct BodyData {
    data: Option<String>,
}

#[derive(Deserialize)]
struct Header { name: String, value: String }

pub async fn fetch_inbox(access_token: &str, max_results: u32) -> Result<Vec<GmailMessage>, String> {
    let client = Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;
    let url = format!("{}/messages?maxResults={}&labelIds=INBOX", GMAIL_API, max_results);
    let resp = client.get(&url).bearer_auth(access_token).send().await.map_err(|e| format!("Gmail list error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { return Err(format!("Gmail API error: {}", resp.status())); }
    let list: ListResponse = resp.json().await.map_err(|e| e.to_string())?;
    let refs = list.messages.unwrap_or_default();

    // Fetch message details in parallel (up to max_results concurrently)
    let futures: Vec<_> = refs.iter().take(max_results as usize)
        .map(|msg_ref| fetch_message_detail(access_token, &msg_ref.id))
        .collect();
    let results = futures_util::future::join_all(futures).await;
    let mut messages = Vec::new();
    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(msg) => messages.push(msg),
            Err(e) => log::warn!("Failed to fetch message {}: {}", refs[i].id, e),
        }
    }
    Ok(messages)
}

pub async fn fetch_message_detail(access_token: &str, message_id: &str) -> Result<GmailMessage, String> {
    let client = Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;
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

pub async fn search_messages(access_token: &str, query: &str) -> Result<Vec<GmailMessage>, String> {
    let client = Client::new();
    let encoded_query = urlencoding::encode(query);
    let url = format!("{}/messages?q={}&maxResults=10", GMAIL_API, encoded_query);
    let resp = client.get(&url).bearer_auth(access_token).send().await
        .map_err(|e| format!("Gmail search error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { return Err(format!("Gmail API error: {}", resp.status())); }
    let list: ListResponse = resp.json().await.map_err(|e| e.to_string())?;
    let refs = list.messages.unwrap_or_default();
    let mut messages = Vec::new();
    for msg_ref in refs.iter().take(10) {
        match fetch_message_detail(access_token, &msg_ref.id).await {
            Ok(msg) => messages.push(msg),
            Err(e) => log::warn!("Failed to fetch message {}: {}", msg_ref.id, e),
        }
    }
    Ok(messages)
}

pub async fn get_message_full(access_token: &str, message_id: &str) -> Result<(GmailMessage, String), String> {
    let client = Client::new();
    let url = format!("{}/messages/{}?format=full", GMAIL_API, message_id);
    let resp = client.get(&url).bearer_auth(access_token).send().await
        .map_err(|e| format!("Gmail read error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { return Err(format!("Gmail API error: {}", resp.status())); }
    let detail: MessageDetail = resp.json().await.map_err(|e| e.to_string())?;

    let payload = detail.payload.as_ref();
    let headers = payload.and_then(|p| p.headers.as_ref()).map(|h| h.as_slice()).unwrap_or(&[]);
    let subject = headers.iter().find(|h| h.name == "Subject").map(|h| h.value.clone());
    let sender = headers.iter().find(|h| h.name == "From").map(|h| h.value.clone());
    let to = headers.iter().find(|h| h.name == "To").map(|h| h.value.clone());
    let date = headers.iter().find(|h| h.name == "Date").map(|h| h.value.clone());
    let labels = detail.label_ids.unwrap_or_default();
    let is_read = !labels.contains(&"UNREAD".to_string());

    // Extract body from payload
    let body_text = if let Some(ref pl) = detail.payload {
        extract_body_from_payload(pl)
    } else {
        String::new()
    };

    let msg = GmailMessage {
        id: detail.id,
        thread_id: detail.thread_id,
        subject: subject.clone(),
        sender: sender.clone(),
        snippet: detail.snippet,
        label_ids: labels,
        is_read,
        received_at: date.clone().or(detail.internal_date),
    };

    let formatted = format!(
        "From: {}\nTo: {}\nDate: {}\nSubject: {}\n\n{}",
        sender.as_deref().unwrap_or("unknown"),
        to.as_deref().unwrap_or("unknown"),
        date.as_deref().unwrap_or("unknown"),
        subject.as_deref().unwrap_or("(no subject)"),
        body_text,
    );

    Ok((msg, formatted))
}

fn extract_body_from_payload(payload: &Payload) -> String {
    // Try direct body on the payload itself
    if let Some(ref body) = payload.body {
        if let Some(ref data) = body.data {
            if !data.is_empty() {
                if let Ok(decoded) = decode_base64url(data) {
                    let is_html = payload.mime_type.as_deref() == Some("text/html");
                    return if is_html { strip_html(&decoded) } else { decoded };
                }
            }
        }
    }
    // Recursively search parts for text/plain first, then text/html
    if let Some(ref parts) = payload.parts {
        if let Some(text) = find_body_in_parts(parts, "text/plain") {
            return text;
        }
        if let Some(text) = find_body_in_parts(parts, "text/html") {
            return strip_html(&text);
        }
    }
    String::new()
}

fn find_body_in_parts(parts: &[Part], target_mime: &str) -> Option<String> {
    for part in parts {
        if part.mime_type.as_deref() == Some(target_mime) {
            if let Some(ref body) = part.body {
                if let Some(ref data) = body.data {
                    if !data.is_empty() {
                        if let Ok(decoded) = decode_base64url(data) {
                            return Some(decoded);
                        }
                    }
                }
            }
        }
        // Recurse into nested parts
        if let Some(ref sub_parts) = part.parts {
            if let Some(found) = find_body_in_parts(sub_parts, target_mime) {
                return Some(found);
            }
        }
    }
    None
}

fn decode_base64url(data: &str) -> Result<String, String> {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let bytes = engine.decode(data).map_err(|e| format!("Base64 decode error: {}", e))?;
    String::from_utf8(bytes).map_err(|e| format!("UTF-8 decode error: {}", e))
}

fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut inside_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => result.push(ch),
            _ => {}
        }
    }
    // Collapse excessive whitespace
    let mut cleaned = String::with_capacity(result.len());
    let mut prev_newline = false;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_newline {
                cleaned.push('\n');
                prev_newline = true;
            }
        } else {
            cleaned.push_str(trimmed);
            cleaned.push('\n');
            prev_newline = false;
        }
    }
    cleaned.trim().to_string()
}

pub async fn send_message(access_token: &str, to: &str, subject: &str, body: &str, cc: Option<&str>) -> Result<String, String> {
    let mut raw_msg = format!(
        "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n",
        to, subject
    );
    if let Some(cc_addr) = cc {
        if !cc_addr.is_empty() {
            raw_msg.push_str(&format!("Cc: {}\r\n", cc_addr));
        }
    }
    raw_msg.push_str(&format!("\r\n{}", body));

    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let encoded = engine.encode(raw_msg.as_bytes());

    let client = Client::new();
    let url = format!("{}/messages/send", GMAIL_API);
    let payload = serde_json::json!({ "raw": encoded });
    let resp = client.post(&url).bearer_auth(access_token).json(&payload).send().await
        .map_err(|e| format!("Gmail send error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail send failed ({}): {}", status, body_text));
    }
    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(result["id"].as_str().unwrap_or("unknown").to_string())
}
