use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::calendar as cal;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct CalendarEventView { pub id: i64, pub google_id: String, pub summary: String, pub description: Option<String>, pub location: Option<String>, pub start_time: String, pub end_time: String, pub attendees: String, pub status: String }

const EVENT_COLUMNS: &str = "id, google_id, summary, description, location, start_time, end_time, attendees, status";

/// calendar_events.start_time carries two different shapes and a date predicate
/// has to be correct for both:
///
/// * a **timed** event holds Google's raw RFC3339 value
///   ('2026-07-27T14:30:00+08:00'). That is an instant; SQLite normalises the
///   offset to UTC, so it must be converted with 'localtime' before it can be
///   compared against a local calendar day. It is also TEXT with a 'T' where
///   datetime('now') has a space, so a raw comparison byte-compares and 'T' >
///   ' ' -- hence datetime() on both sides of the timed branch.
/// * an **all-day** event holds a bare 'YYYY-MM-DD' (integrations/calendar.rs,
///   `to_string_repr` falls back to Google's `date` field). That value is
///   already a local calendar date. SQLite reads it as midnight UTC, so
///   applying 'localtime' to it shifts it: harmless for a positive offset,
///   but it rolls into the previous day for every negative one.
///
/// A bare date is exactly 10 characters, which is how the two are told apart.
/// Shared with assistant::context so the definition of "today" cannot drift
/// between the Calendar page and the assistant's day context.
pub const TODAYS_EVENT_PREDICATE: &str = "CASE WHEN length(start_time) = 10 \
     THEN start_time \
     ELSE date(start_time, 'localtime') END = date('now', 'localtime')";

/// The 7-day window, split by the same two shapes (see TODAYS_EVENT_PREDICATE).
/// The timed branch compares instants, so a meeting that ended this morning is
/// not upcoming and a meeting on day 7 is. The all-day branch compares calendar
/// days, which keeps an all-day event dated today in the list for the whole
/// local day instead of dropping it the moment 'now' passes its midnight.
/// Shared with the tests below so neither branch can drift back.
const UPCOMING_EVENTS_PREDICATE: &str = "(CASE WHEN length(start_time) = 10 \
     THEN start_time >= date('now', 'localtime') \
     ELSE datetime(start_time) >= datetime('now') END) \
     AND (CASE WHEN length(start_time) = 10 \
     THEN start_time <= date('now', 'localtime', ?1) \
     ELSE datetime(start_time) <= datetime('now', ?1) END)";

fn upcoming_events_query() -> String {
    format!(
        "SELECT {} FROM calendar_events WHERE {} ORDER BY datetime(start_time) ASC",
        EVENT_COLUMNS, UPCOMING_EVENTS_PREDICATE
    )
}

fn todays_events_query() -> String {
    format!(
        "SELECT {} FROM calendar_events WHERE {} ORDER BY datetime(start_time) ASC",
        EVENT_COLUMNS, TODAYS_EVENT_PREDICATE
    )
}

fn date_range_query() -> String {
    format!("SELECT {} FROM calendar_events WHERE (CASE WHEN length(start_time)=10 THEN start_time ELSE date(start_time,'localtime') END) BETWEEN ?1 AND ?2 ORDER BY datetime(start_time) ASC", EVENT_COLUMNS)
}
fn validate_date_range(from: &str, to: &str) -> Result<(), String> {
    let parse = |value: &str| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok().filter(|d| d.format("%Y-%m-%d").to_string() == value);
    let from = parse(from).ok_or("Invalid start date")?;
    let to = parse(to).ok_or("Invalid end date")?;
    if to < from || (to-from).num_days() > 366 { return Err("Date range must be ordered and at most 366 days".into()); }
    Ok(())
}
#[tauri::command]
pub fn get_events(db: State<Arc<Database>>, days: Option<i32>, date_from: Option<String>, date_to: Option<String>) -> Result<Vec<CalendarEventView>, String> {
    let (query, params) = match (date_from, date_to) {
        (Some(from),Some(to)) => { validate_date_range(&from,&to)?; (date_range_query(),vec![from,to]) },
        (None,None) => (upcoming_events_query(),vec![format!("+{} days",days.unwrap_or(7))]),
        _ => return Err("Both dateFrom and dateTo are required".into()),
    };
    let conn = db.conn.lock().map_err(|e|e.to_string())?;
    let mut stmt = conn.prepare(&query).map_err(|e|e.to_string())?;
    let events = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
        Ok(CalendarEventView { id: row.get(0)?, google_id: row.get(1)?, summary: row.get(2)?, description: row.get(3)?, location: row.get(4)?, start_time: row.get(5)?, end_time: row.get(6)?, attendees: row.get(7)?, status: row.get(8)? })
    }).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    Ok(events)
}

#[tauri::command]
pub async fn sync_calendar(db: State<'_, Arc<Database>>, auth: State<'_, Arc<GoogleAuth>>, date_from: Option<String>, date_to: Option<String>) -> Result<String, String> {
    let token = auth.ensure_access_token().await?;
    let (time_min,time_max) = match (date_from,date_to) {
        (Some(from),Some(to)) => {
            validate_date_range(&from,&to)?;
            use chrono::TimeZone;
            let midnight = |day: chrono::NaiveDate| chrono::Local.from_local_datetime(&day.and_hms_opt(0,0,0).unwrap()).earliest()
                .map(|v|v.to_rfc3339()).ok_or("Local midnight is unavailable for this date".to_string());
            let from = chrono::NaiveDate::parse_from_str(&from,"%Y-%m-%d").map_err(|e|e.to_string())?;
            let to = chrono::NaiveDate::parse_from_str(&to,"%Y-%m-%d").map_err(|e|e.to_string())?.succ_opt().ok_or("Invalid end date")?;
            (midnight(from)?,midnight(to)?)
        },
        (None,None) => { let now = chrono::Utc::now(); (now.to_rfc3339(),(now+chrono::TimeDelta::days(7)).to_rfc3339()) },
        _ => return Err("Both dateFrom and dateTo are required".into()),
    };
    let fetched = cal::fetch_events_detailed(&token, &time_min, &time_max).await?;
    let count = fetched.events.len();
    cal::save_to_db(&db, &fetched, &time_min, &time_max)?;
    Ok(format!("Synced {} events", count))
}

#[tauri::command]
pub async fn create_event(auth: State<'_, Arc<GoogleAuth>>, summary: String, start: String, end: String, description: Option<String>) -> Result<String, String> {
    let token = auth.ensure_access_token().await?;
    cal::create_event(&token, &summary, &start, &end, description.as_deref(), None, None).await
}

#[tauri::command]
pub fn get_todays_events(db: State<Arc<Database>>) -> Result<Vec<CalendarEventView>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(&todays_events_query()).map_err(|e| e.to_string())?;
    let events = stmt.query_map([], |row| {
        Ok(CalendarEventView { id: row.get(0)?, google_id: row.get(1)?, summary: row.get(2)?, description: row.get(3)?, location: row.get(4)?, start_time: row.get(5)?, end_time: row.get(6)?, attendees: row.get(7)?, status: row.get(8)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn events_db(inserts: &str) -> Connection {
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
                 status TEXT DEFAULT 'confirmed');",
        )
        .unwrap();
        conn.execute_batch(inserts).unwrap();
        conn
    }

    /// Collect the google_id column of whatever the query returns. `col` is 1
    /// for the shipped queries (which select id first) and 0 for the pre-fix
    /// queries spelled out below.
    fn ids(conn: &Connection, sql: &str, params: &[&dyn rusqlite::types::ToSql], col: usize) -> Vec<String> {
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map(params, |r| r.get::<_, String>(col))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// The four rows every date predicate here has to get right: an all-day and
    /// a timed event on today's local date, and one of each on tomorrow's.
    /// `tz` is the RFC3339 offset written into the timed rows.
    fn four_shapes(tz: &str) -> Connection {
        events_db(&format!(
            "INSERT INTO calendar_events (google_id, summary, start_time, end_time) VALUES
                 ('allday_today', 'HKSTP demo day', '2026-07-27', '2026-07-28'),
                 ('allday_tomorrow', 'Cohort review', '2026-07-28', '2026-07-29'),
                 ('timed_today', 'Standup', '2026-07-27T09:00:00{tz}', '2026-07-27T09:30:00{tz}'),
                 ('timed_tomorrow', 'Mentor sync', '2026-07-28T14:00:00{tz}', '2026-07-28T15:00:00{tz}');",
            tz = tz
        ))
    }

    #[test]
    fn selected_date_range_includes_elapsed_and_all_day_events() {
        let conn = four_shapes("+00:00");
        let query = date_range_query().replace("'localtime'", "'+0 hours'");
        assert_eq!(ids(&conn,&query,&[&"2026-07-27",&"2026-07-27"],1),vec!["allday_today","timed_today"]);
        assert_eq!(ids(&conn,&query,&[&"2026-07-27",&"2026-07-28"],1).len(),4);
        assert!(validate_date_range("2026-07-27","2026-07-28").is_ok());
        assert!(validate_date_range("2026-07-28","2026-07-27").is_err());
        assert!(validate_date_range("2026-02-30","2026-03-01").is_err());
    }

    /// The 7-day window must compare instants, not raw text: a meeting that
    /// ended this morning is not upcoming, and a meeting on day 7 is.
    #[test]
    fn upcoming_events_compare_as_timestamps_not_text() {
        let conn = events_db(
            "INSERT INTO calendar_events (google_id, summary, start_time, end_time) VALUES
                 ('past', 'Standup', '2026-07-27T09:00:00+08:00', '2026-07-27T09:30:00+08:00'),
                 ('allday', 'HKSTP demo day', '2026-07-27', '2026-07-28'),
                 ('tonight', 'Investor call', '2026-07-27T23:00:00+08:00', '2026-07-27T23:30:00+08:00'),
                 ('day7', 'Cohort review', '2026-08-03T09:00:00+08:00', '2026-08-03T10:00:00+08:00');",
        );

        // Pin the clock to 2026-07-27 10:00:00 UTC (18:00 in HK) so the
        // assertions do not depend on when the test runs, and pin 'localtime'
        // to +08:00 so they do not depend on the machine's timezone either.
        let pinned = upcoming_events_query()
            .replace("'localtime'", "'+8 hours'")
            .replace("'now'", "'2026-07-27 10:00:00'");
        assert_eq!(
            ids(&conn, &pinned, &[&"+7 days"], 1),
            vec!["allday".to_string(), "tonight".to_string(), "day7".to_string()],
            "the morning meeting is over, the all-day event is still today"
        );

        // The pre-fix predicate compared the stored TEXT directly, where
        // 'T' > ' ': the morning meeting still counted as upcoming at 18:00 and
        // the day-7 meeting was dropped.
        let old = "SELECT google_id FROM calendar_events \
                   WHERE start_time >= datetime('2026-07-27 10:00:00') \
                   AND start_time <= datetime('2026-07-27 10:00:00', '+7 days') \
                   ORDER BY start_time ASC";
        assert_eq!(ids(&conn, old, &[], 0), vec!["past".to_string(), "tonight".to_string()]);
    }

    /// An all-day event dated today must stay in the "NEXT 7 DAYS" list for the
    /// whole local day. Wrapping a bare 'YYYY-MM-DD' in datetime() yields
    /// midnight, so an instant comparison dropped it from 00:00 UTC onward --
    /// the surface the original report named.
    #[test]
    fn upcoming_events_keep_todays_all_day_event() {
        for (tz, offset, now_utc) in [
            ("+08:00", "'+8 hours'", "'2026-07-27 10:00:00'"),
            ("-04:00", "'-4 hours'", "'2026-07-27 14:00:00'"),
        ] {
            let conn = four_shapes(tz);
            let pinned = upcoming_events_query()
                .replace("'localtime'", offset)
                .replace("'now'", now_utc);
            let got = ids(&conn, &pinned, &[&"+7 days"], 1);
            assert!(
                got.contains(&"allday_today".to_string()),
                "today's all-day event must still be upcoming at {} (got {:?})",
                offset,
                got
            );
            assert!(
                got.contains(&"allday_tomorrow".to_string()) && got.contains(&"timed_tomorrow".to_string()),
                "tomorrow's events are inside the 7-day window at {} (got {:?})",
                offset,
                got
            );
        }
    }

    /// "Today" must be the user's local day, for both storage shapes. Applying
    /// 'localtime' to an all-day row shifts a value that is already a local
    /// calendar date; the UTC day gets timed rows wrong instead.
    #[test]
    fn todays_events_follow_the_local_day() {
        // Hong Kong, UTC+8. Clock: 17:00 local on 2026-07-27.
        let hk = four_shapes("+08:00");
        let pinned = todays_events_query()
            .replace("'localtime'", "'+8 hours'")
            .replace("'now'", "'2026-07-27 09:00:00'");
        assert_eq!(
            ids(&hk, &pinned, &[], 1),
            vec!["allday_today".to_string(), "timed_today".to_string()]
        );

        // New York, UTC-4. Clock: 10:00 local on 2026-07-27. This is the
        // direction the previous fix got wrong.
        let ny = four_shapes("-04:00");
        let pinned = todays_events_query()
            .replace("'localtime'", "'-4 hours'")
            .replace("'now'", "'2026-07-27 14:00:00'");
        assert_eq!(
            ids(&ny, &pinned, &[], 1),
            vec!["allday_today".to_string(), "timed_today".to_string()]
        );

        // Shifting the all-day column too hid today's all-day event and
        // presented tomorrow's as today's, all day, for any negative offset.
        let over_shifted = "SELECT google_id FROM calendar_events \
                            WHERE date(start_time, '-4 hours') = date('2026-07-27 14:00:00', '-4 hours') \
                            ORDER BY datetime(start_time) ASC";
        assert_eq!(
            ids(&ny, over_shifted, &[], 0),
            vec!["timed_today".to_string(), "allday_tomorrow".to_string()]
        );

        // The pre-fix predicate used the UTC day, which for a UTC+8 user still
        // reads as yesterday until 08:00 local: at 07:00 HK on the 27th it
        // reported nothing at all as today.
        let utc_day = "SELECT google_id FROM calendar_events \
                       WHERE date(start_time) = date('2026-07-26 23:00:00') \
                       ORDER BY datetime(start_time) ASC";
        assert_eq!(ids(&hk, utc_day, &[], 0), Vec::<String>::new());
    }
}
