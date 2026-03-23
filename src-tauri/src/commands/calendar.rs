use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::calendar as cal;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct CalendarEventView { pub id: i64, pub google_id: String, pub summary: String, pub description: Option<String>, pub location: Option<String>, pub start_time: String, pub end_time: String, pub attendees: String, pub status: String }

#[tauri::command]
pub fn get_events(db: State<Arc<Database>>, days: Option<i32>) -> Result<Vec<CalendarEventView>, String> {
    let days = days.unwrap_or(7);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let param = format!("+{} days", days);
    let mut stmt = conn.prepare("SELECT id, google_id, summary, description, location, start_time, end_time, attendees, status FROM calendar_events WHERE start_time >= datetime('now') AND start_time <= datetime('now', ?1) ORDER BY start_time ASC").map_err(|e| e.to_string())?;
    let events = stmt.query_map(rusqlite::params![param], |row| {
        Ok(CalendarEventView { id: row.get(0)?, google_id: row.get(1)?, summary: row.get(2)?, description: row.get(3)?, location: row.get(4)?, start_time: row.get(5)?, end_time: row.get(6)?, attendees: row.get(7)?, status: row.get(8)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(events)
}

#[tauri::command]
pub async fn sync_calendar(db: State<'_, Arc<Database>>, auth: State<'_, Arc<GoogleAuth>>) -> Result<String, String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    let now = chrono::Utc::now();
    let time_min = now.to_rfc3339();
    let time_max = (now + chrono::TimeDelta::days(7)).to_rfc3339();
    let events = cal::fetch_events(&token, &time_min, &time_max).await?;
    let count = events.len();
    cal::save_to_db(&db, &events)?;
    Ok(format!("Synced {} events", count))
}

#[tauri::command]
pub async fn create_event(auth: State<'_, Arc<GoogleAuth>>, summary: String, start: String, end: String, description: Option<String>) -> Result<String, String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    cal::create_event(&token, &summary, &start, &end, description.as_deref()).await
}

#[tauri::command]
pub fn get_todays_events(db: State<Arc<Database>>) -> Result<Vec<CalendarEventView>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id, google_id, summary, description, location, start_time, end_time, attendees, status FROM calendar_events WHERE date(start_time) = date('now') ORDER BY start_time ASC").map_err(|e| e.to_string())?;
    let events = stmt.query_map([], |row| {
        Ok(CalendarEventView { id: row.get(0)?, google_id: row.get(1)?, summary: row.get(2)?, description: row.get(3)?, location: row.get(4)?, start_time: row.get(5)?, end_time: row.get(6)?, attendees: row.get(7)?, status: row.get(8)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(events)
}
