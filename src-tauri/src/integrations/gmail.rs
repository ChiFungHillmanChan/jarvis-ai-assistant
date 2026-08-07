use base64::Engine;
use futures_util::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

/// Concurrent `messages.get` calls during an inbox fetch. Gmail bills a metadata
/// get at 5 quota units against a per-user ceiling of 250 units/second, so an
/// unbounded 50-wide burst sits exactly on the limit and starts drawing 429s.
const MAX_CONCURRENT_DETAIL_FETCHES: usize = 8;

/// Attempts, including the first, for a single `messages.get`.
const DETAIL_MAX_ATTEMPTS: u32 = 3;

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

/// An inbox fetch that may have lost messages. `messages` holds every message
/// whose detail fetch succeeded; `warning` is set when at least one did not, so
/// a sync that silently dropped mail cannot be reported as a complete one.
/// Mirrors `github::AssignedItemsFetch`.
pub struct InboxFetch {
    pub messages: Vec<GmailMessage>,
    pub warning: Option<String>,
}

/// Inbox messages, discarding the partial-failure warning (which
/// `fetch_inbox_detailed` has already logged per message). Callers that report a
/// sync result to the user should use the detailed form instead, so a sync that
/// lost messages is not presented as a clean one.
pub async fn fetch_inbox(
    access_token: &str,
    max_results: u32,
) -> Result<Vec<GmailMessage>, String> {
    Ok(fetch_inbox_detailed(access_token, max_results)
        .await?
        .messages)
}

pub async fn fetch_inbox_detailed(
    access_token: &str,
    max_results: u32,
) -> Result<InboxFetch, String> {
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

    // Fetch message details with a bounded fan-out so a large inbox cannot burst
    // past Gmail's per-user quota (see MAX_CONCURRENT_DETAIL_FETCHES). `buffered`
    // preserves input order, so results still line up with `refs` by index.
    let ids: Vec<String> = refs
        .iter()
        .take(max_results as usize)
        .map(|msg_ref| msg_ref.id.clone())
        .collect();
    let results: Vec<Result<GmailMessage, String>> = futures_util::stream::iter(
        ids.into_iter()
            .map(|id| async move { fetch_message_detail(access_token, &id).await }),
    )
    .buffered(MAX_CONCURRENT_DETAIL_FETCHES)
    .collect()
    .await;
    let total = results.len();
    let mut messages = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(msg) => messages.push(msg),
            Err(e) => {
                log::warn!("Failed to fetch message {}: {}", refs[i].id, e);
                failures.push(e);
            }
        }
    }
    let warning = detail_fetch_outcome(total, &failures)?;
    Ok(InboxFetch { messages, warning })
}

/// What a fan-out of `messages.get` failures means for the batch as a whole.
///
/// Every message failing with the 401 sentinel is a credential problem, not a
/// partial sync: it propagates as `UNAUTHORIZED` so the caller's existing
/// refresh-and-retry runs, rather than reporting "Synced 0 emails" as a success
/// on a token revoked between the list call and the gets. Anything else that
/// lost messages becomes a warning carried alongside the ones that did arrive,
/// because a sync that dropped mail must not be indistinguishable from a
/// complete one.
fn detail_fetch_outcome(total: usize, failures: &[String]) -> Result<Option<String>, String> {
    if failures.is_empty() {
        return Ok(None);
    }
    if failures.len() == total && failures.iter().all(|e| e == "UNAUTHORIZED") {
        return Err("UNAUTHORIZED".to_string());
    }
    // Distinct reasons only: 20 messages failing the same way is one fact, and
    // the full per-message detail is already in the log.
    let mut reasons: Vec<&str> = Vec::new();
    for failure in failures {
        if !reasons.contains(&failure.as_str()) {
            reasons.push(failure.as_str());
        }
    }
    let shown = reasons.iter().take(2).copied().collect::<Vec<_>>().join("; ");
    let more = if reasons.len() > 2 {
        format!(" (+{} other reasons)", reasons.len() - 2)
    } else {
        String::new()
    };
    Ok(Some(format!(
        "{} of {} messages could not be fetched and are missing from this sync: {}{}",
        failures.len(),
        total,
        shown,
        more
    )))
}

/// Gmail error reasons that mean "you are being rate limited". Google's guidance
/// for these is to back off and retry; `dailyLimitExceeded` is deliberately not
/// here, because a 24-hour quota is not something a backoff clears.
const RATE_LIMIT_REASONS: [&str; 2] = ["ratelimitexceeded", "userratelimitexceeded"];

/// Whether a failed Gmail response is worth retrying. 429 and 5xx are transient
/// on the status alone. A 403 is ambiguous: Gmail answers 403 both for a
/// permission failure (permanent) and for rate limiting, where the error body
/// carries reason `rateLimitExceeded` or `userRateLimitExceeded`. Rate limiting
/// is how Gmail throttles a fan-out of `messages.get` in practice, so the body
/// decides, and a permission 403 still fails fast. A 400 or 401 is permanent.
fn is_retryable_status(status: u16, body: &str) -> bool {
    if status == 429 || (500..600).contains(&status) {
        return true;
    }
    status == 403 && is_rate_limit_body(body)
}

/// Whether a Google error envelope names a rate limit. Both the classic
/// (`error.errors[].reason`) and the newer (`error.status`, `error.details[]`)
/// shapes are checked; anything unparseable is treated as not a rate limit
/// rather than guessed at.
fn is_rate_limit_body(body: &str) -> bool {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let error = &parsed["error"];
    if error["status"].as_str() == Some("RESOURCE_EXHAUSTED") {
        return true;
    }
    ["errors", "details"].iter().any(|field| {
        error[*field]
            .as_array()
            .map(|entries| {
                entries.iter().any(|entry| {
                    entry["reason"]
                        .as_str()
                        .map(|reason| {
                            RATE_LIMIT_REASONS.contains(&reason.to_ascii_lowercase().as_str())
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    })
}

/// Cheap jitter source. The retry schedule only needs to stop a fan-out of
/// concurrent gets from retrying in lockstep, not cryptographic randomness, so
/// the clock's sub-second component is enough and avoids a new dependency.
fn jitter_ms(bound_ms: u64) -> u64 {
    if bound_ms == 0 {
        return 0;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_nanos()) % bound_ms)
        .unwrap_or(0)
}

/// Backoff before retry `attempt` (1-based): 250ms, 500ms, 1s, 2s, capped there,
/// plus up to 250ms of jitter.
fn retry_delay(attempt: u32) -> std::time::Duration {
    let base_ms = 250u64 << (attempt.clamp(1, 4) - 1);
    std::time::Duration::from_millis(base_ms + jitter_ms(250))
}

pub async fn fetch_message_detail(
    access_token: &str,
    message_id: &str,
) -> Result<GmailMessage, String> {
    let client = &*HTTP;
    let url = format!("{}/messages/{}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date", GMAIL_API, message_id);
    let mut attempt = 0;
    let detail: MessageDetail = loop {
        attempt += 1;
        let resp = client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        // Check the status before parsing. A Google error envelope has no `id`
        // field, so feeding it to serde yields "missing field `id`" and hides the
        // real HTTP status, and a 401 here would never produce the UNAUTHORIZED
        // sentinel the refresh-and-retry call sites match on.
        if status == 401 {
            return Err("UNAUTHORIZED".to_string());
        }
        if !status.is_success() {
            // Read the body before deciding: a 403 is only distinguishable as a
            // rate limit (retry) from a permission failure (fail fast) by the
            // reason inside it.
            let body_text = resp.text().await.unwrap_or_default();
            if is_retryable_status(status.as_u16(), &body_text) && attempt < DETAIL_MAX_ATTEMPTS {
                tokio::time::sleep(retry_delay(attempt)).await;
                continue;
            }
            return Err(format!("Gmail API error: {}: {}", status, body_text));
        }
        break resp.json::<MessageDetail>().await.map_err(|e| e.to_string())?;
    };
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
    if resp.status() == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
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

/// Build the base64url-encoded RFC 822 payload shared by send and draft.
/// Header fields have CR/LF stripped so AI- or content-derived values cannot
/// inject extra headers (e.g. a hidden Bcc) into the raw message.
fn encode_raw_message(to: &str, subject: &str, body: &str, cc: Option<&str>) -> String {
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
    engine.encode(raw_msg.as_bytes())
}

/// Create a Gmail draft. This is the only compose path exposed to the model:
/// a draft lands in the user's own account for review, so a prompt-injected
/// email cannot turn JARVIS into an outbound channel.
pub async fn create_draft(
    access_token: &str,
    to: &str,
    subject: &str,
    body: &str,
    cc: Option<&str>,
) -> Result<String, String> {
    let encoded = encode_raw_message(to, subject, body, cc);

    let client = &*HTTP;
    let url = format!("{}/drafts", GMAIL_API);
    let payload = serde_json::json!({ "message": { "raw": encoded } });
    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Gmail draft error: {}", e))?;
    if resp.status() == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail draft failed ({}): {}", status, body_text));
    }
    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(result["id"].as_str().unwrap_or("unknown").to_string())
}

pub async fn send_message(
    access_token: &str,
    to: &str,
    subject: &str,
    body: &str,
    cc: Option<&str>,
) -> Result<String, String> {
    let encoded = encode_raw_message(to, subject, body, cc);

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

    #[test]
    fn only_transient_statuses_are_retried() {
        // Retrying a 401 would bypass the refresh-and-retry contract, and
        // retrying a 400 just burns quota against a permanent failure.
        assert!(is_retryable_status(429, ""));
        assert!(is_retryable_status(500, ""));
        assert!(is_retryable_status(503, ""));
        assert!(!is_retryable_status(400, ""));
        assert!(!is_retryable_status(401, ""));
        assert!(!is_retryable_status(404, ""));
        assert!(!is_retryable_status(200, ""));
    }

    #[test]
    fn a_403_is_retried_only_when_the_body_says_rate_limit() {
        // Gmail throttles a metadata fan-out with a 403 far more often than a
        // 429, so treating every 403 as permanent means the retry never fires
        // in the one scenario it was added for.
        let user_rate_limit = r#"{"error":{"code":403,"message":"User-rate limit exceeded",
            "errors":[{"domain":"usageLimits","reason":"userRateLimitExceeded"}]}}"#;
        assert!(is_retryable_status(403, user_rate_limit));
        let project_rate_limit =
            r#"{"error":{"code":403,"errors":[{"domain":"usageLimits","reason":"rateLimitExceeded"}]}}"#;
        assert!(is_retryable_status(403, project_rate_limit));
        // The newer envelope carries the same signal in `status`.
        let resource_exhausted = r#"{"error":{"code":403,"status":"RESOURCE_EXHAUSTED"}}"#;
        assert!(is_retryable_status(403, resource_exhausted));

        // A permission 403 must still fail fast: retrying cannot fix a missing
        // scope, it only delays the real error by two backoffs.
        let insufficient_scope = r#"{"error":{"code":403,"status":"PERMISSION_DENIED",
            "message":"Request had insufficient authentication scopes.",
            "errors":[{"domain":"global","reason":"forbidden"}]}}"#;
        assert!(!is_retryable_status(403, insufficient_scope));
        // A daily quota is not cleared by a sub-second backoff either.
        let daily_quota = r#"{"error":{"code":403,"errors":[{"reason":"dailyLimitExceeded"}]}}"#;
        assert!(!is_retryable_status(403, daily_quota));
        // An empty or non-JSON body is not guessed at.
        assert!(!is_retryable_status(403, ""));
        assert!(!is_retryable_status(403, "<html>403 Forbidden</html>"));
    }

    #[test]
    fn a_complete_fan_out_carries_no_warning() {
        assert_eq!(detail_fetch_outcome(20, &[]), Ok(None));
        assert_eq!(detail_fetch_outcome(0, &[]), Ok(None));
    }

    #[test]
    fn dropped_messages_are_reported_rather_than_swallowed() {
        // 20 listed, 4 permission failures: a sync that silently lost a fifth of
        // the inbox used to be indistinguishable from a complete one, so the
        // briefing and the "Synced 16 emails" toast both lied.
        let denied = "Gmail API error: 403 Forbidden: PERMISSION_DENIED".to_string();
        let failures = vec![denied.clone(), denied.clone(), denied.clone(), denied];
        let warning = detail_fetch_outcome(20, &failures)
            .expect("a partial loss is not a hard failure")
            .expect("a partial loss must not be silent");
        assert!(warning.contains("4 of 20"), "warning: {warning}");
        assert!(warning.contains("PERMISSION_DENIED"), "warning: {warning}");
    }

    #[test]
    fn distinct_failure_reasons_are_summarised_not_repeated() {
        let failures = vec![
            "reason A".to_string(),
            "reason A".to_string(),
            "reason B".to_string(),
            "reason C".to_string(),
            "reason D".to_string(),
        ];
        let warning = detail_fetch_outcome(10, &failures).unwrap().unwrap();
        assert!(warning.contains("5 of 10"), "warning: {warning}");
        assert!(warning.contains("reason A"), "warning: {warning}");
        assert!(warning.contains("reason B"), "warning: {warning}");
        assert!(warning.contains("+2 other reasons"), "warning: {warning}");
    }

    #[test]
    fn a_token_revoked_mid_fan_out_propagates_instead_of_syncing_nothing() {
        // Every detail get answering 401 means the token died between the list
        // call and the gets. Reporting that as "Synced 0 emails" hides it; the
        // sentinel is what the callers' refresh-and-retry matches on.
        let all_401 = vec!["UNAUTHORIZED".to_string(), "UNAUTHORIZED".to_string()];
        assert_eq!(
            detail_fetch_outcome(2, &all_401),
            Err("UNAUTHORIZED".to_string())
        );

        // A single 401 among successes is not a dead token -- the rest of the
        // batch is real data, so it stays a warning.
        let one_401 = vec!["UNAUTHORIZED".to_string()];
        let warning = detail_fetch_outcome(5, &one_401).unwrap().unwrap();
        assert!(warning.contains("1 of 5"), "warning: {warning}");
    }

    #[test]
    fn retry_delay_grows_and_stays_bounded() {
        assert!((250..500).contains(&retry_delay(1).as_millis()));
        assert!((500..750).contains(&retry_delay(2).as_millis()));
        // Capped, so a retrying fan-out can never stall a sync for many seconds.
        assert!(retry_delay(9).as_millis() < 2250);
    }
}
