use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

/// Shared, connection-pooled Gmail HTTP client with timeouts so no request can
/// hang a background sync or a chat turn indefinitely. Reused across all calls
/// to avoid a fresh TLS handshake per request.
static HTTP: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| Client::new())
});

#[derive(Debug, Clone, Serialize, Deserialize)]
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
struct ListResponse {
    messages: Option<Vec<MessageRef>>,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

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
struct Header {
    name: String,
    value: String,
}

pub async fn fetch_inbox(
    access_token: &str,
    max_results: u32,
) -> Result<Vec<GmailMessage>, String> {
    let client = &*HTTP;
    let url = format!(
        "{}/messages?maxResults={}&labelIds=INBOX",
        GMAIL_API, max_results
    );
    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Gmail list error: {}", e))?;
    if resp.status() == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("Gmail API error: {}", resp.status()));
    }
    let list: ListResponse = resp.json().await.map_err(|e| e.to_string())?;
    let refs = list.messages.unwrap_or_default();

    // Fetch message details in parallel (up to max_results concurrently)
    let futures: Vec<_> = refs
        .iter()
        .take(max_results as usize)
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

pub async fn fetch_message_detail(
    access_token: &str,
    message_id: &str,
) -> Result<GmailMessage, String> {
    let client = &*HTTP;
    let url = format!("{}/messages/{}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date", GMAIL_API, message_id);
    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let detail: MessageDetail = resp.json().await.map_err(|e| e.to_string())?;
    let headers = detail.payload.and_then(|p| p.headers).unwrap_or_default();
    let subject = headers
        .iter()
        .find(|h| h.name == "Subject")
        .map(|h| h.value.clone());
    let sender = headers
        .iter()
        .find(|h| h.name == "From")
        .map(|h| h.value.clone());
    let date = headers
        .iter()
        .find(|h| h.name == "Date")
        .map(|h| h.value.clone());
    let labels = detail.label_ids.unwrap_or_default();
    let is_read = !labels.contains(&"UNREAD".to_string());
    Ok(GmailMessage {
        id: detail.id,
        thread_id: detail.thread_id,
        subject,
        sender,
        snippet: detail.snippet,
        label_ids: labels,
        is_read,
        received_at: date.or(detail.internal_date),
    })
}

pub async fn archive_message(access_token: &str, message_id: &str) -> Result<(), String> {
    let client = &*HTTP;
    let url = format!("{}/messages/{}/modify", GMAIL_API, message_id);
    let body = serde_json::json!({ "removeLabelIds": ["INBOX"] });
    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Gmail archive error: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Gmail archive failed: {}", resp.status()));
    }
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
    let client = &*HTTP;
    let encoded_query = urlencoding::encode(query);
    let url = format!("{}/messages?q={}&maxResults=10", GMAIL_API, encoded_query);
    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Gmail search error: {}", e))?;
    if resp.status() == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("Gmail API error: {}", resp.status()));
    }
    let list: ListResponse = resp.json().await.map_err(|e| e.to_string())?;
    let refs = list.messages.unwrap_or_default();

    // Fetch the matched message details in parallel (mirrors fetch_inbox) so a
    // 10-result search is one round-trip of latency, not ten sequential ones.
    let futures: Vec<_> = refs
        .iter()
        .take(10)
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

pub async fn get_message_full(
    access_token: &str,
    message_id: &str,
) -> Result<(GmailMessage, String), String> {
    let client = &*HTTP;
    let url = format!("{}/messages/{}?format=full", GMAIL_API, message_id);
    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Gmail read error: {}", e))?;
    if resp.status() == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("Gmail API error: {}", resp.status()));
    }
    let detail: MessageDetail = resp.json().await.map_err(|e| e.to_string())?;

    let payload = detail.payload.as_ref();
    let headers = payload
        .and_then(|p| p.headers.as_ref())
        .map(|h| h.as_slice())
        .unwrap_or(&[]);
    let subject = headers
        .iter()
        .find(|h| h.name == "Subject")
        .map(|h| h.value.clone());
    let sender = headers
        .iter()
        .find(|h| h.name == "From")
        .map(|h| h.value.clone());
    let to = headers
        .iter()
        .find(|h| h.name == "To")
        .map(|h| h.value.clone());
    let date = headers
        .iter()
        .find(|h| h.name == "Date")
        .map(|h| h.value.clone());
    let labels = detail.label_ids.unwrap_or_default();
    let is_read = !labels.contains(&"UNREAD".to_string());

    // Extract body from payload; fall back to the Gmail-provided snippet if extraction yields nothing.
    let mut body_text = if let Some(ref pl) = detail.payload {
        extract_body_from_payload(pl)
    } else {
        String::new()
    };
    if body_text.trim().is_empty() {
        if let Some(ref snippet) = detail.snippet {
            if !snippet.is_empty() {
                body_text = snippet.clone();
                log::warn!(
                    "[gmail] body extraction empty for {}, falling back to snippet ({} chars)",
                    detail.id,
                    snippet.len()
                );
            }
        }
    }

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
                    return if is_html {
                        strip_html(&decoded)
                    } else {
                        decoded
                    };
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
    // Gmail sometimes includes padding, sometimes not. Strip whitespace and any
    // trailing '=' then decode with NO_PAD to accept both forms.
    let cleaned: String = data
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .trim_end_matches('=')
        .to_string();
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let bytes = engine
        .decode(&cleaned)
        .map_err(|e| format!("Base64 decode error: {}", e))?;
    // Lossy UTF-8 so weird encodings still give us readable text
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn strip_html(html: &str) -> String {
    // Drop <script>/<style> blocks wholesale -- they contain noise that blows up context.
    let cleaned_blocks = {
        let mut out = String::with_capacity(html.len());
        let lower = html.to_ascii_lowercase();
        let mut i = 0;
        while i < html.len() {
            let rest_lower = &lower[i..];
            let rest = &html[i..];
            if let Some(start) = rest_lower.find("<script") {
                out.push_str(&rest[..start]);
                if let Some(end) = rest_lower[start..].find("</script>") {
                    i += start + end + "</script>".len();
                    continue;
                }
                break;
            }
            if let Some(start) = rest_lower.find("<style") {
                out.push_str(&rest[..start]);
                if let Some(end) = rest_lower[start..].find("</style>") {
                    i += start + end + "</style>".len();
                    continue;
                }
                break;
            }
            out.push_str(rest);
            break;
        }
        out
    };

    // Convert block-level tags into newlines so paragraphs survive.
    let block_tags = [
        "</p>", "<br>", "<br/>", "<br />", "</div>", "</h1>", "</h2>", "</h3>", "</h4>", "</li>",
        "</tr>", "</td>",
    ];
    let mut normalized = cleaned_blocks;
    for tag in block_tags.iter() {
        normalized = normalized.replace(tag, "\n");
        let upper = tag.to_uppercase();
        normalized = normalized.replace(&upper, "\n");
    }

    // Strip remaining tags.
    let mut text = String::with_capacity(normalized.len());
    let mut inside_tag = false;
    for ch in normalized.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => text.push(ch),
            _ => {}
        }
    }

    // Decode the common HTML entities we care about.
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'");

    // Collapse runs of blank lines to at most one blank line so paragraphs remain visible.
    let mut cleaned = String::with_capacity(text.len());
    let mut blank_streak = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_streak += 1;
            if blank_streak == 1 {
                cleaned.push('\n');
            }
        } else {
            blank_streak = 0;
            cleaned.push_str(trimmed);
            cleaned.push('\n');
        }
    }
    cleaned.trim().to_string()
}

/// Remove CR/LF (and surrounding whitespace) from a value destined for an
/// email header, defeating header-injection via newline characters.
fn strip_header_value(value: &str) -> String {
    value.replace(['\r', '\n'], " ").trim().to_string()
}

pub async fn send_message(
    access_token: &str,
    to: &str,
    subject: &str,
    body: &str,
    cc: Option<&str>,
) -> Result<String, String> {
    // Strip CR/LF from header fields so AI- or content-derived values can't
    // inject extra headers (e.g. a hidden Bcc) into the raw RFC 822 message.
    let to = strip_header_value(to);
    let subject = strip_header_value(subject);
    let mut raw_msg = format!(
        "To: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n",
        to, subject
    );
    if let Some(cc_addr) = cc {
        let cc_addr = strip_header_value(cc_addr);
        if !cc_addr.is_empty() {
            raw_msg.push_str(&format!("Cc: {}\r\n", cc_addr));
        }
    }
    raw_msg.push_str(&format!("\r\n{}", body));

    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let encoded = engine.encode(raw_msg.as_bytes());

    let client = &*HTTP;
    let url = format!("{}/messages/send", GMAIL_API);
    let payload = serde_json::json!({ "raw": encoded });
    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Gmail send error: {}", e))?;
    if resp.status() == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail send failed ({}): {}", status, body_text));
    }
    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(result["id"].as_str().unwrap_or("unknown").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_header_value_removes_crlf_injection() {
        // A subject trying to smuggle a Bcc header must be flattened to one line.
        let malicious = "Hello\r\nBcc: attacker@evil.com";
        let cleaned = strip_header_value(malicious);
        assert!(!cleaned.contains('\r'));
        assert!(!cleaned.contains('\n'));
        assert_eq!(cleaned, "Hello  Bcc: attacker@evil.com");
    }

    #[test]
    fn strip_header_value_keeps_plain_text() {
        assert_eq!(strip_header_value("HKSTP invoice"), "HKSTP invoice");
    }
}
