use base64::Engine;
use futures_util::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    filename: Option<String>,
    headers: Option<Vec<Header>>,
    parts: Option<Vec<Part>>,
    body: Option<BodyData>,
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
}

#[derive(Deserialize)]
struct Part {
    filename: Option<String>,
    headers: Option<Vec<Header>>,
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

/// `emails.received_at` is compared with bytewise TEXT collation everywhere it
/// is sorted (`ORDER BY received_at DESC`), so the stored value has to already
/// be in a chronologically-sortable form. The raw RFC 2822 `Date` header
/// (`Thu, 4 Sep 2026 10:15:00 +0800`) starts with a weekday name, so that sort
/// was ordering mail by weekday, not by time.
///
/// Prefers Gmail's `internalDate` (epoch milliseconds as a string -- server
/// receipt time, not spoofable by a `Date` header) converted to UTC RFC 3339;
/// falls back to parsing the `Date` header as RFC 2822; falls back to the raw
/// header untouched if neither parses, so a malformed value is preserved
/// rather than silently dropped.
pub fn normalise_received_at(
    internal_date: Option<&str>,
    date_header: Option<&str>,
) -> Option<String> {
    if let Some(millis) = internal_date.and_then(|s| s.parse::<i64>().ok()) {
        if let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis) {
            return Some(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());
        }
    }
    if let Some(header) = date_header {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(header) {
            return Some(
                dt.with_timezone(&chrono::Utc)
                    .format("%Y-%m-%dT%H:%M:%SZ")
                    .to_string(),
            );
        }
    }
    date_header.map(|s| s.to_string())
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
        received_at: normalise_received_at(detail.internal_date.as_deref(), date.as_deref()),
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

/// The local inbox rows a fetched batch covers. `fetch_inbox` asks Gmail for the
/// newest `max_results` messages carrying the INBOX label, so every row at least
/// as new as the oldest message that came back was inside that window: one that
/// did not come back has left the INBOX -- archived on the phone, deleted, or
/// filtered -- and must stop counting as inbox mail. Rows older than the batch
/// were never in the window and are left alone.
///
/// datetime() on both sides rather than a text compare: `normalise_received_at`
/// falls back to the raw RFC2822 Date header when neither value parses, and that
/// shape does not sort against the normalised one. datetime() yields NULL for it,
/// so the comparison is NULL and the row is skipped -- the safe direction.
const INBOX_WINDOW_QUERY: &str =
    "SELECT gmail_id FROM emails WHERE is_archived = 0 AND datetime(received_at) >= datetime(?1)";

pub fn save_to_db(db: &crate::db::Database, fetched: &InboxFetch) -> Result<(), String> {
    let mut conn = db.conn.lock().map_err(|e| e.to_string())?;
    // One transaction: the upsert and the reconciliation below are one view of
    // the inbox, and a half-applied one archives mail that is still there.
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for msg in &fetched.messages {
        // is_archived = 0 on conflict, so a message moved back into the INBOX --
        // or one an earlier reconciliation archived -- reappears in the list.
        tx.execute(
            "INSERT INTO emails (gmail_id, thread_id, subject, sender, snippet, labels, is_read, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(gmail_id) DO UPDATE SET is_read = ?7, labels = ?6, is_archived = 0, synced_at = datetime('now')",
            rusqlite::params![msg.id, msg.thread_id, msg.subject, msg.sender, msg.snippet, msg.label_ids.join(","), msg.is_read as i32, msg.received_at],
        ).map_err(|e| e.to_string())?;
    }

    // An empty batch establishes no window. A batch that lost messages to failed
    // detail fetches (`warning`) is not the inbox either, so reconciling against
    // it would archive mail that is still sitting in it.
    if fetched.warning.is_none() {
        let oldest = fetched
            .messages
            .iter()
            .filter_map(|m| m.received_at.as_deref())
            .min();
        if let Some(oldest) = oldest {
            let in_batch: HashSet<&str> =
                fetched.messages.iter().map(|m| m.id.as_str()).collect();
            let left_the_inbox: Vec<String> = {
                let mut stmt = tx.prepare(INBOX_WINDOW_QUERY).map_err(|e| e.to_string())?;
                let covered = stmt
                    .query_map(rusqlite::params![oldest], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<String>, _>>()
                    .map_err(|e| e.to_string())?;
                covered
                    .into_iter()
                    .filter(|gmail_id| !in_batch.contains(gmail_id.as_str()))
                    .collect()
            };
            for gmail_id in left_the_inbox {
                tx.execute(
                    "UPDATE emails SET is_archived = 1 WHERE gmail_id = ?1",
                    rusqlite::params![gmail_id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }

    tx.commit().map_err(|e| e.to_string())
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
        received_at: normalise_received_at(detail.internal_date.as_deref(), date.as_deref()),
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
    if is_attachment(payload.filename.as_deref(), payload.headers.as_deref()) {
        return String::new();
    }
    // Try direct body on the payload itself
    let mime = payload.mime_type.as_deref().unwrap_or("");
    if let Some(ref body) = payload.body.as_ref().filter(|_| mime.eq_ignore_ascii_case("text/plain") || mime.eq_ignore_ascii_case("text/html")) {
        if let Some(ref data) = body.data {
            if !data.is_empty() {
                if let Ok(decoded) = decode_base64url(data) {
                    let is_html = mime.eq_ignore_ascii_case("text/html");
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
        if is_attachment(part.filename.as_deref(), part.headers.as_deref())
            || part.mime_type.as_deref().is_some_and(|mime| mime.eq_ignore_ascii_case("message/rfc822")) {
            continue;
        }
        if part.mime_type.as_deref().is_some_and(|mime| mime.eq_ignore_ascii_case(target_mime)) {
            if let Some(ref body) = part.body {
                if let Some(ref data) = body.data {
                    if !data.is_empty() {
                        if let Ok(decoded) = decode_base64url(data) {
                            if !decoded.trim().is_empty() {
                                return Some(decoded);
                            }
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

fn is_attachment(filename: Option<&str>, headers: Option<&[Header]>) -> bool {
    filename.is_some_and(|name| !name.trim().is_empty())
        || headers.unwrap_or(&[]).iter().any(|header| {
            header.name.eq_ignore_ascii_case("Content-Disposition")
                && header.value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("attachment")
        })
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
            let next = [("<script", "</script>"), ("<style", "</style>")]
                .into_iter()
                .filter_map(|(open, close)| rest_lower.find(open).map(|start| (start, close)))
                .min_by_key(|(start, _)| *start);
            if let Some((start, close)) = next {
                out.push_str(&rest[..start]);
                if let Some(end) = rest_lower[start..].find(close) {
                    i += start + end + close.len();
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
    let text = decode_html_entities(&text);

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

fn decode_html_entities(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        // Entity names and Unicode numeric references are short. Bound the
        // scan so malformed input with many ampersands stays linear.
        let end = rest.bytes().take(16).position(|byte| byte == b';');
        let decoded = end.and_then(|end| {
            let entity = &rest[1..end];
            match entity {
                "nbsp" => Some(' '), "amp" => Some('&'), "lt" => Some('<'),
                "gt" => Some('>'), "quot" => Some('"'), "apos" => Some('\''),
                _ => {
                    let value = if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        entity.strip_prefix('#').and_then(|digits| digits.parse::<u32>().ok())
                    };
                    value.and_then(char::from_u32).filter(|ch| *ch != '\0')
                }
            }
        });
        if let (Some(end), Some(ch)) = (end, decoded) {
            output.push(ch);
            rest = &rest[end + 1..];
        } else {
            output.push('&');
            rest = &rest[1..];
        }
    }
    output.push_str(rest);
    output
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

    fn body_fixture(value: serde_json::Value) -> String {
        extract_body_from_payload(&serde_json::from_value::<Payload>(value).unwrap())
    }

    fn encoded_body(text: &str) -> serde_json::Value {
        serde_json::json!({"data": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(text)})
    }

    #[test]
    fn mail_body_skips_text_attachments_and_attached_messages() {
        let result = body_fixture(serde_json::json!({"mimeType":"multipart/mixed", "parts":[
            {"mimeType":"text/plain", "filename":"notes.txt", "body":encoded_body("Attachment text")},
            {"mimeType":"multipart/mixed", "headers":[{"name":"content-disposition","value":"Attachment; filename=mail.eml"}],
             "parts":[{"mimeType":"text/plain","body":encoded_body("Forwarded attachment")}]},
            {"mimeType":"text/plain", "body":encoded_body("  \n ")},
            {"mimeType":"text/html", "body":encoded_body("<p>Actual message</p>")}
        ]}));
        assert_eq!(result, "Actual message");
    }

    #[test]
    fn mail_body_does_not_display_binary_or_named_root_attachments() {
        assert_eq!(body_fixture(serde_json::json!({"mimeType":"application/pdf", "body":encoded_body("%PDF-binary")})), "");
        assert_eq!(body_fixture(serde_json::json!({"mimeType":"text/plain", "filename":"notes.txt", "body":encoded_body("Private attachment")})), "");
    }

    #[test]
    fn mail_html_removes_scripts_and_styles_in_document_order() {
        assert_eq!(strip_html("<style>.hidden {color:red}</style><p>First</p><script>alert(1)</script><style>.later{}</style><p>Second</p>"), "First\nSecond");
    }

    #[test]
    fn mail_html_decodes_numeric_entities_once_and_keeps_unknown_entities() {
        assert_eq!(strip_html("<p>&#25991;&#x4EF6; &amp;lt; &unknown;</p>"), "文件 &lt; &unknown;");
    }

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

    #[test]
    fn internal_date_wins_and_is_utc_rfc3339() {
        // 1788488100000 ms since epoch is 2026-09-04T02:15:00Z. The header
        // below claims a different day and time entirely -- internalDate is
        // Gmail's own receipt timestamp and must win over a client-supplied
        // header.
        let internal_date = Some("1788488100000");
        let date_header = Some("Thu, 3 Sep 2026 05:00:00 +0000");
        assert_eq!(
            normalise_received_at(internal_date, date_header),
            Some("2026-09-04T02:15:00Z".to_string())
        );
    }

    #[test]
    fn rfc2822_header_is_normalised() {
        // No internalDate (e.g. the metadata-only fetch path); the RFC 2822
        // header must be parsed and converted to UTC RFC 3339, not stored raw.
        let date_header = Some("Fri, 4 Sep 2026 10:15:00 +0800");
        assert_eq!(
            normalise_received_at(None, date_header),
            Some("2026-09-04T02:15:00Z".to_string())
        );
    }

    #[test]
    fn garbage_falls_back_to_the_raw_header() {
        // Neither value parses: the function must not panic or return None,
        // it must preserve the original header text so no data is lost.
        let internal_date = Some("not-a-number");
        let date_header = Some("this is not a real date");
        assert_eq!(
            normalise_received_at(internal_date, date_header),
            Some("this is not a real date".to_string())
        );
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn emails_db() -> crate::db::Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE emails (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gmail_id TEXT UNIQUE NOT NULL,
                thread_id TEXT,
                subject TEXT,
                sender TEXT NOT NULL,
                snippet TEXT,
                labels TEXT,
                importance_score INTEGER DEFAULT 0,
                is_spam INTEGER DEFAULT 0,
                is_read INTEGER DEFAULT 0,
                received_at TEXT NOT NULL,
                synced_at TEXT NOT NULL DEFAULT (datetime('now')),
                is_archived INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        crate::db::Database { conn: Mutex::new(conn) }
    }

    fn seed(db: &crate::db::Database, gmail_id: &str, received_at: &str, is_archived: i32) {
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO emails (gmail_id, sender, labels, received_at, is_archived)
                 VALUES (?1, 'sender@example.com', 'INBOX', ?2, ?3)",
                rusqlite::params![gmail_id, received_at, is_archived],
            )
            .unwrap();
    }

    fn archived(db: &crate::db::Database, gmail_id: &str) -> i32 {
        db.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT is_archived FROM emails WHERE gmail_id = ?1",
                rusqlite::params![gmail_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn message(id: &str, received_at: &str) -> GmailMessage {
        GmailMessage {
            id: id.to_string(),
            thread_id: None,
            subject: Some("subject".to_string()),
            sender: Some("sender@example.com".to_string()),
            snippet: None,
            label_ids: vec!["INBOX".to_string()],
            is_read: false,
            received_at: Some(received_at.to_string()),
        }
    }

    fn complete(messages: Vec<GmailMessage>) -> InboxFetch {
        InboxFetch { messages, warning: None }
    }

    #[test]
    fn mail_older_than_the_batch_survives_the_sync() {
        // fetch_inbox returns the newest N: a message older than all of them was
        // never inside the fetched window, so its absence says nothing.
        let db = emails_db();
        seed(&db, "old", "2026-08-01T09:00:00Z", 0);
        save_to_db(&db, &complete(vec![message("new", "2026-09-04T09:00:00Z")])).unwrap();
        assert_eq!(
            archived(&db, "old"),
            0,
            "mail outside the fetched window must not be archived"
        );
    }

    #[test]
    fn mail_that_left_the_inbox_is_archived_locally() {
        // Archived on the phone: inside the window, absent from the batch.
        // Leaving it at is_archived = 0 kept it in every list and count.
        let db = emails_db();
        seed(&db, "gone", "2026-09-04T10:00:00Z", 0);
        save_to_db(&db, &complete(vec![message("kept", "2026-09-04T09:00:00Z")])).unwrap();
        assert_eq!(archived(&db, "gone"), 1);
        assert_eq!(archived(&db, "kept"), 0);
    }

    #[test]
    fn mail_moved_back_to_the_inbox_is_unarchived() {
        let db = emails_db();
        seed(&db, "back", "2026-09-04T10:00:00Z", 1);
        save_to_db(&db, &complete(vec![message("back", "2026-09-04T10:00:00Z")])).unwrap();
        assert_eq!(
            archived(&db, "back"),
            0,
            "a message that returned to the INBOX must reappear in the list"
        );
    }

    #[test]
    fn an_empty_batch_archives_nothing() {
        // No batch, no window: an empty fetch establishes no oldest message to
        // reconcile against.
        let db = emails_db();
        seed(&db, "still-here", "2026-09-04T10:00:00Z", 0);
        save_to_db(&db, &complete(vec![])).unwrap();
        assert_eq!(archived(&db, "still-here"), 0);
    }

    #[test]
    fn a_partial_fetch_archives_nothing() {
        // Detail fetches failed, so the batch is not the inbox. Reconciling
        // against it would hide mail that is still sitting there.
        let db = emails_db();
        seed(&db, "unlucky", "2026-09-04T10:00:00Z", 0);
        let fetched = InboxFetch {
            messages: vec![message("kept", "2026-09-04T09:00:00Z")],
            warning: Some("1 of 2 messages could not be fetched".to_string()),
        };
        save_to_db(&db, &fetched).unwrap();
        assert_eq!(archived(&db, "unlucky"), 0);
    }
}
