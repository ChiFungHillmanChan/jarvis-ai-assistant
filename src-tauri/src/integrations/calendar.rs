use reqwest::Client;
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

#[derive(Deserialize)]
struct EventsResponse { items: Option<Vec<GoogleEvent>> }

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
    let client = Client::new();
    let url = format!("{}/calendars/primary/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=50", CALENDAR_API, time_min, time_max);
    let resp = client.get(&url).bearer_auth(access_token).send().await.map_err(|e| format!("Calendar API error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { return Err(format!("Calendar API error: {}", resp.status())); }
    let body: EventsResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body.items.unwrap_or_default().into_iter().filter_map(|e| {
        let start = e.start.as_ref()?.to_string_repr();
        let end = e.end.as_ref()?.to_string_repr();
        if start.is_empty() { return None; }
        Some(CalendarEvent {
            id: e.id.unwrap_or_default(), summary: e.summary.unwrap_or_else(|| "(No title)".to_string()),
            description: e.description, location: e.location, start_time: start, end_time: end,
            attendees: e.attendees.unwrap_or_default().into_iter().filter_map(|a| a.email).collect(),
            status: e.status.unwrap_or_else(|| "confirmed".to_string()),
        })
    }).collect())
}

pub async fn create_event(
    access_token: &str,
    summary: &str,
    start: &str,
    end: &str,
    description: Option<&str>,
    location: Option<&str>,
    attendees: Option<&str>,
) -> Result<String, String> {
    let client = Client::new();
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
    let client = Client::new();
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
    let client = Client::new();
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
