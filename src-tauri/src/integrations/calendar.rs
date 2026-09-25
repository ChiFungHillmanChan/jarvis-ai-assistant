use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

/// A window's events plus whether the paging loop actually reached the end.
/// `complete` is false when the page budget ran out with a `nextPageToken`
/// still outstanding, which matters because `save_to_db` deletes covered rows
/// that did not come back: reconciling against a truncated view would drop
/// real events from the local mirror. Mirrors `gmail::InboxFetch` and
/// `github::AssignedItemsFetch`.
pub struct EventsFetch {
    pub events: Vec<CalendarEvent>,
    pub complete: bool,
}

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

/// Events for the window, discarding the completeness flag. For read-only
/// callers that never reconcile; anything that calls `save_to_db` must use
/// `fetch_events_detailed` so a truncated fetch does not delete live rows.
pub async fn fetch_events(access_token: &str, time_min: &str, time_max: &str) -> Result<Vec<CalendarEvent>, String> {
    Ok(fetch_events_detailed(access_token, time_min, time_max).await?.events)
}

pub async fn fetch_events_detailed(access_token: &str, time_min: &str, time_max: &str) -> Result<EventsFetch, String> {
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

    // A token still outstanding means the budget ran out mid-window, not that
    // the window ended.
    Ok(EventsFetch { complete: next_token.is_none(), events })
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

/// An all-day event on `date` (YYYY-MM-DD). Google treats the end date as
/// exclusive, so the event ends on the following day. No attendees.
pub async fn create_all_day_event(
    access_token: &str,
    summary: &str,
    date: chrono::NaiveDate,
    description: Option<&str>,
) -> Result<String, String> {
    let client = crate::util::http_client();
    let url = format!("{}/calendars/primary/events", CALENDAR_API);
    let end = date.succ_opt().ok_or("Invalid date")?;
    let mut body = serde_json::json!({
        "summary": summary,
        "start": { "date": date.to_string() },
        "end": { "date": end.to_string() },
    });
    if let Some(desc) = description {
        body["description"] = serde_json::Value::String(desc.to_string());
    }
    let resp = client.post(&url).bearer_auth(access_token).json(&body).send().await.map_err(|e| format!("Create event error: {}", e))?;
    if resp.status() == 401 { return Err("UNAUTHORIZED".to_string()); }
    if !resp.status().is_success() { return Err(format!("Create event failed {}", resp.status())); }
    let created: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(created["id"].as_str().unwrap_or("").to_string())
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

/// The rows a fetched window covers, so a covered row Google did not return has
/// left the calendar. `start_time` carries two shapes (see the note on
/// TODAYS_EVENT_PREDICATE in commands/calendar.rs): a timed event holds Google's
/// RFC3339 instant, an all-day event a bare 'YYYY-MM-DD'. datetime() normalises
/// the first to UTC but reads the second as UTC midnight, which can sit up to 14
/// hours away from the instant Google actually filtered on -- so an all-day row
/// counts as covered only when its whole day fits inside the window. Erring
/// outwards is deliberate: a row left behind is merely stale, a row deleted in
/// error is an event the user cannot see anywhere until the window moves.
const COVERED_BY_WINDOW: &str = "datetime(start_time) >= datetime(?1) \
     AND CASE WHEN length(start_time) = 10 \
         THEN datetime(start_time, '+1 day') <= datetime(?2) \
         ELSE datetime(start_time) < datetime(?2) END";

fn covered_events_query() -> String {
    format!(
        "SELECT google_id FROM calendar_events WHERE {}",
        COVERED_BY_WINDOW
    )
}

/// `time_min` and `time_max` must be the window `fetch_events_detailed` was
/// called with:
/// the reconciliation below reads them as the range Google was asked about, and
/// a wider one would delete events that were never requested.
pub fn save_to_db(
    db: &crate::db::Database,
    fetched: &EventsFetch,
    time_min: &str,
    time_max: &str,
) -> Result<(), String> {
    let events = &fetched.events;
    let mut conn = db.conn.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for event in events {
        tx.execute(
            "INSERT INTO calendar_events (google_id, summary, description, location, start_time, end_time, attendees, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(google_id) DO UPDATE SET summary = ?2, description = ?3, location = ?4, start_time = ?5, end_time = ?6, attendees = ?7, status = ?8, synced_at = datetime('now')",
            rusqlite::params![event.id, event.summary, event.description, event.location, event.start_time, event.end_time, event.attendees.join(","), event.status],
        ).map_err(|e| e.to_string())?;
    }

    // Google returns every event overlapping the window, so a covered row that
    // did not come back was cancelled or deleted at the source. Upserting alone
    // kept it on the Calendar page and in the briefing forever.
    // A truncated fetch is not evidence of absence: the events past the page
    // budget are missing from this batch but alive at the source. Upsert them
    // and leave the mirror alone until a complete fetch can reconcile it.
    let seen: HashSet<&str> = events.iter().map(|e| e.id.as_str()).collect();
    let gone: Vec<String> = if !fetched.complete {
        Vec::new()
    } else {
        let mut stmt = tx
            .prepare(&covered_events_query())
            .map_err(|e| e.to_string())?;
        let covered = stmt
            .query_map(rusqlite::params![time_min, time_max], |r| {
                r.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| e.to_string())?;
        covered
            .into_iter()
            .filter(|google_id| !seen.contains(google_id.as_str()))
            .collect()
    };
    for google_id in gone {
        tx.execute(
            "DELETE FROM calendar_events WHERE google_id = ?1",
            rusqlite::params![google_id],
        )
        .map_err(|e| e.to_string())?;
    }

    tx.commit().map_err(|e| e.to_string())
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

#[cfg(test)]
mod sync_tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    // The window a 7-day sync asks Google about. The upper bound is mid-day on
    // purpose: it is what separates an all-day row the window fully covers from
    // one hanging over its edge.
    const TIME_MIN: &str = "2026-09-04T00:00:00Z";
    const TIME_MAX: &str = "2026-09-11T10:00:00Z";

    fn events_db() -> crate::db::Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE calendar_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                google_id TEXT UNIQUE NOT NULL,
                summary TEXT NOT NULL,
                description TEXT,
                location TEXT,
                start_time TEXT NOT NULL,
                end_time TEXT NOT NULL,
                attendees TEXT,
                status TEXT DEFAULT 'confirmed',
                synced_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        crate::db::Database { conn: Mutex::new(conn) }
    }

    fn seed(db: &crate::db::Database, google_id: &str, start_time: &str) {
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO calendar_events (google_id, summary, start_time, end_time)
                 VALUES (?1, 'meeting', ?2, ?2)",
                rusqlite::params![google_id, start_time],
            )
            .unwrap();
    }

    fn stored_ids(db: &crate::db::Database) -> Vec<String> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT google_id FROM calendar_events ORDER BY google_id")
            .unwrap();
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        ids
    }

    /// A fetch that reached the end of the window, which is what every
    /// reconciliation test below assumes unless it says otherwise.
    fn complete(events: Vec<CalendarEvent>) -> EventsFetch {
        EventsFetch { events, complete: true }
    }

    /// A fetch that ran out of page budget with more events outstanding.
    fn truncated(events: Vec<CalendarEvent>) -> EventsFetch {
        EventsFetch { events, complete: false }
    }

    fn event(id: &str, start_time: &str) -> CalendarEvent {
        CalendarEvent {
            id: id.to_string(),
            summary: "meeting".to_string(),
            description: None,
            location: None,
            start_time: start_time.to_string(),
            end_time: start_time.to_string(),
            attendees: vec![],
            status: "confirmed".to_string(),
        }
    }

    #[test]
    fn an_event_outside_the_fetched_window_survives() {
        // Google was never asked about next month, so a row there carries no
        // evidence that it was cancelled.
        let db = events_db();
        seed(&db, "next-month", "2026-10-02T09:00:00Z");
        save_to_db(
            &db,
            &complete(vec![event("in-window", "2026-09-05T09:00:00Z")]),
            TIME_MIN,
            TIME_MAX,
        )
        .unwrap();
        assert_eq!(stored_ids(&db), vec!["in-window", "next-month"]);
    }

    #[test]
    fn an_event_deleted_at_the_source_is_dropped() {
        // Inside the window and not returned: cancelled or deleted. Upsert alone
        // kept it on the Calendar page and in the briefing forever.
        let db = events_db();
        seed(&db, "cancelled", "2026-09-05T09:00:00Z");
        save_to_db(
            &db,
            &complete(vec![event("still-on", "2026-09-06T09:00:00Z")]),
            TIME_MIN,
            TIME_MAX,
        )
        .unwrap();
        assert_eq!(stored_ids(&db), vec!["still-on"]);
    }

    #[test]
    fn a_partial_fetch_deletes_nothing() {
        // Ten pages of 50 is the budget; a window busier than that came back
        // truncated. Reconciling against it would delete every event past the
        // cutoff from the mirror even though all of them are alive at the
        // source. Mirrors gmail's `a_partial_fetch_archives_nothing`.
        let db = events_db();
        seed(&db, "past-the-page-budget", "2026-09-05T09:00:00Z");
        save_to_db(
            &db,
            &truncated(vec![event("on-page-one", "2026-09-06T09:00:00Z")]),
            TIME_MIN,
            TIME_MAX,
        )
        .unwrap();
        assert_eq!(
            stored_ids(&db),
            vec!["on-page-one", "past-the-page-budget"],
            "a truncated fetch is not evidence that the missing events are gone"
        );
    }

    #[test]
    fn a_re_created_event_comes_back() {
        let db = events_db();
        save_to_db(
            &db,
            &complete(vec![event("meeting", "2026-09-05T09:00:00Z")]),
            TIME_MIN,
            TIME_MAX,
        )
        .unwrap();
        save_to_db(&db, &complete(vec![]), TIME_MIN, TIME_MAX).unwrap();
        assert!(stored_ids(&db).is_empty(), "an empty window clears it");
        save_to_db(
            &db,
            &complete(vec![event("meeting", "2026-09-05T09:00:00Z")]),
            TIME_MIN,
            TIME_MAX,
        )
        .unwrap();
        assert_eq!(stored_ids(&db), vec!["meeting"]);
    }

    #[test]
    fn an_all_day_row_hanging_over_the_window_edge_is_left_alone() {
        // 'YYYY-MM-DD' is a local calendar day and datetime() reads it as UTC
        // midnight, up to 14 hours from the instant Google filtered on. The day
        // on the far edge may never have been requested, so it must not be
        // deleted for failing to come back -- while one fully inside must be.
        let db = events_db();
        seed(&db, "edge-day", "2026-09-11");
        seed(&db, "inside-day", "2026-09-05");
        save_to_db(&db, &complete(vec![]), TIME_MIN, TIME_MAX).unwrap();
        assert_eq!(stored_ids(&db), vec!["edge-day"]);
    }
}
