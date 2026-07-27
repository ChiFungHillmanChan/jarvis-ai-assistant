use serde::{Deserialize, Serialize};

const CALENDAR_API: &str = "https://www.googleapis.com/calendar/v3";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CalendarEvent {
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_time: String,
    pub end_time: String,
    pub attendees: Vec<String>,
    pub status: String,
}

/// Hard page budget for `fetch_events`. Google caps a page at `maxResults`, so
/// without a ceiling a pathological account could page forever.
const MAX_EVENT_PAGES: usize = 10;

#[derive(Deserialize)]
struct EventsResponse {
    items: Option<Vec<GoogleEvent>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct GoogleEvent {
    id: Option<String>, summary: Option<String>, description: Option<String>,
    location: Option<String>, start: Option<EventTime>, end: Option<EventTime>,
    attendees: Option<Vec<Attendee>>, status: Option<String>,
}

#[derive(Deserialize)]
struct EventTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

#[derive(Deserialize)]
struct Attendee { email: Option<String> }

impl EventTime {
    fn to_string_repr(&self) -> String {
        self.date_time.clone().or(self.date.clone()).unwrap_or_default()
    }
}

pub async fn fetch_events(access_token: &str, time_min: &str, time_max: &str) -> Result<Vec<CalendarEvent>, String> {
    let client = crate::util::http_client();
    let url = format!("{}/calendars/primary/events", CALENDAR_API);
    let mut events: Vec<CalendarEvent> = Vec::new();
    let mut next_token: Option<String> = None;

    // Follow nextPageToken. Without this a busy week past maxResults silently
    // truncated to the first page while still reporting success.
    for _ in 0..MAX_EVENT_PAGES {
        let page_token = next_token.take();
        let mut query: Vec<(&str, &str)> = vec![
            ("timeMin", time_min),
            ("timeMax", time_max),
            ("singleEvents", "true"),
            ("orderBy", "startTime"),
            ("maxResults", "50"),
        ];
        if let Some(tok) = page_token.as_deref() {
            query.push(("pageToken", tok));
        }
        let resp = client.get(&url)
            .bearer_auth(access_token)
            .query(&query)
            .send().await.map_err(|e| format!("Calendar API error: {}", e))?;
        if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
        if !resp.status().is_success() { return Err(format!("Calendar API error: {}", resp.status())); }
        let body: EventsResponse = resp.json().await.map_err(|e| e.to_string())?;
        events.extend(body.items.unwrap_or_default().into_iter().filter_map(|e| {
            let start = e.start.as_ref()?.to_string_repr();
            let end = e.end.as_ref()?.to_string_repr();
            if start.is_empty() { return None; }
            Some(CalendarEvent {
                id: e.id.unwrap_or_default(), summary: e.summary.unwrap_or_else(|| "(No title)".to_string()),
                description: e.description, location: e.location, start_time: start, end_time: end,
                attendees: e.attendees.unwrap_or_default().into_iter().filter_map(|a| a.email).collect(),
                status: e.status.unwrap_or_else(|| "confirmed".to_string()),
            })
        }));
        match body.next_page_token {
            Some(tok) if !tok.is_empty() => next_token = Some(tok),
            _ => break,
        }
    }

    Ok(events)
}

/// `attendees` is for user-initiated callers only. It must never carry a
/// model-chosen address: Google copies the event -- summary and description
/// included -- onto every invited attendee's own calendar, which turns this into
/// an outbound channel the attacker reads back with events.list. The tool
/// dispatch in `ai/tools.rs` always passes None.
pub async fn create_event(
    access_token: &str,
    summary: &str,
    start: &str,
    end: &str,
    description: Option<&str>,
    location: Option<&str>,
    attendees: Option<&str>,
) -> Result<String, String> {
    let client = crate::util::http_client();
    let url = format!("{}/calendars/primary/events", CALENDAR_API);
    let mut body = serde_json::json!({
        "summary": summary,
        "start": { "dateTime": start },
        "end": { "dateTime": end },
    });
    if let Some(desc) = description {
        body["description"] = serde_json::Value::String(desc.to_string());
    }
    if let Some(loc) = location {
        body["location"] = serde_json::Value::String(loc.to_string());
    }
    if let Some(att) = attendees {
        let emails: Vec<serde_json::Value> = att
            .split(',')
            .map(|e| serde_json::json!({ "email": e.trim() }))
            .collect();
        body["attendees"] = serde_json::Value::Array(emails);
    }
    let resp = client.post(&url).bearer_auth(access_token).json(&body).send().await.map_err(|e| format!("Create event error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { let s = resp.status(); let t = resp.text().await.unwrap_or_default(); return Err(format!("Create event failed {}: {}", s, t)); }
    let created: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let id = created["id"].as_str().unwrap_or("");
    let link = created["htmlLink"].as_str().unwrap_or("");
    Ok(format!("{} | {}", id, link))
}

pub async fn update_event(
    access_token: &str,
    event_id: &str,
    title: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    location: Option<&str>,
    description: Option<&str>,
) -> Result<String, String> {
    let client = crate::util::http_client();
    let url = format!("{}/calendars/primary/events/{}", CALENDAR_API, event_id);
    let mut body = serde_json::json!({});
    if let Some(t) = title {
        body["summary"] = serde_json::Value::String(t.to_string());
    }
    if let Some(s) = start {
        body["start"] = serde_json::json!({ "dateTime": s });
    }
    if let Some(e) = end {
        body["end"] = serde_json::json!({ "dateTime": e });
    }
    if let Some(loc) = location {
        body["location"] = serde_json::Value::String(loc.to_string());
    }
    if let Some(desc) = description {
        body["description"] = serde_json::Value::String(desc.to_string());
    }
    let resp = client.patch(&url).bearer_auth(access_token).json(&body).send().await.map_err(|e| format!("Update event error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { let s = resp.status(); let t = resp.text().await.unwrap_or_default(); return Err(format!("Update event failed {}: {}", s, t)); }
    let updated: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let id = updated["id"].as_str().unwrap_or("");
    let link = updated["htmlLink"].as_str().unwrap_or("");
    Ok(format!("{} | {}", id, link))
}

pub async fn delete_event(access_token: &str, event_id: &str) -> Result<String, String> {
    let client = crate::util::http_client();
    let url = format!("{}/calendars/primary/events/{}", CALENDAR_API, event_id);
    let resp = client.delete(&url).bearer_auth(access_token).send().await.map_err(|e| format!("Delete event error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if resp.status() == 204 || resp.status().is_success() {
        Ok(format!("Event {} deleted successfully.", event_id))
    } else {
        let s = resp.status();
        let t = resp.text().await.unwrap_or_default();
        Err(format!("Delete event failed {}: {}", s, t))
    }
}

pub fn save_to_db(db: &crate::db::Database, events: &[CalendarEvent]) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    for event in events {
        conn.execute(
            "INSERT INTO calendar_events (google_id, summary, description, location, start_time, end_time, attendees, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(google_id) DO UPDATE SET summary = ?2, description = ?3, location = ?4, start_time = ?5, end_time = ?6, attendees = ?7, status = ?8, synced_at = datetime('now')",
            rusqlite::params![event.id, event.summary, event.description, event.location, event.start_time, event.end_time, event.attendees.join(","), event.status],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_response_captures_next_page_token() {
        // A week past maxResults comes back with nextPageToken. Dropping it was
        // a silent truncation reported as a successful full sync.
        let json = r#"{"items": [], "nextPageToken": "CigKGjBqM3BhZ2U"}"#;
        let parsed: EventsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.next_page_token.as_deref(), Some("CigKGjBqM3BhZ2U"));
    }

    #[test]
    fn last_page_has_no_next_page_token() {
        let json = r#"{"items": []}"#;
        let parsed: EventsResponse = serde_json::from_str(json).unwrap();
        assert!(
            parsed.next_page_token.is_none(),
            "absence of the token is what ends the paging loop"
        );
    }
}
