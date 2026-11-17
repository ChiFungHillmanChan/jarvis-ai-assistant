use crate::db::Database;
use chrono::Timelike;
use std::sync::Arc;

pub struct DayContext {
    pub greeting: String,
    pub tasks_summary: String,
    pub calendar_summary: String,
    pub email_summary: String,
    pub github_summary: String,
    pub deadlines: String,
    /// Whether the local mirror has ever been synced. False means the table is
    /// empty because nothing has been fetched yet, which is not the same claim
    /// as "no unread mail" / "no meetings" -- see `inline_context`.
    pub has_synced_email: bool,
    pub has_synced_calendar: bool,
}

/// Every context block ends with this. The numbers come out of SQLite, not out
/// of Gmail, and the model has live tools it should reach for when the answer
/// has to be current.
pub const CACHE_NOTE: &str =
    "This is a local cache from the last sync. Call search_emails or list_events for live data.";

/// A mirror with no rows has never been synced. Google connect used to schedule
/// nothing, so for up to five minutes -- until the first scheduler tick -- these
/// were both false on a freshly connected account. Shared with the test below.
pub(crate) const HAS_EMAIL_ROWS_QUERY: &str = "SELECT EXISTS(SELECT 1 FROM emails)";
pub(crate) const HAS_CALENDAR_ROWS_QUERY: &str = "SELECT EXISTS(SELECT 1 FROM calendar_events)";

/// Archived mail is excluded from the count and the sender list, so the
/// assistant reports the same number the Email page shows. Shared with the test
/// below so the V17 filter cannot drift out again.
pub(crate) const UNREAD_COUNT_QUERY: &str = "SELECT COUNT(*) FROM emails WHERE is_read = 0 AND is_archived = 0";
const UNREAD_SENDERS_QUERY: &str = "SELECT sender, subject FROM emails WHERE is_read = 0 AND is_archived = 0 ORDER BY received_at DESC LIMIT 3";

/// "Today" is the user's local day. The predicate is shared with the Calendar
/// page (`commands::calendar::TODAYS_EVENT_PREDICATE`) so the assistant's
/// [CONTEXT] block and the page can never disagree about which events are
/// today's; read the doc comment there for why the two storage shapes of
/// start_time need different handling.
pub(crate) fn todays_events_query() -> String {
    format!(
        "SELECT summary, start_time, end_time, location FROM calendar_events WHERE {} ORDER BY datetime(start_time) ASC",
        crate::commands::calendar::TODAYS_EVENT_PREDICATE
    )
}

/// Task deadlines are plain local calendar dates ("YYYY-MM-DD", see the
/// create_task tool schema in ai/tools.rs), so only the 'now' side takes the
/// modifier. Without it a UTC+8 user gets yesterday's day boundary until 08:00
/// local: today's tasks are not counted as due today, yesterday's are.
const OVERDUE_TASKS_QUERY: &str = "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline < date('now', 'localtime')";
const DUE_TODAY_QUERY: &str = "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline = date('now', 'localtime')";
/// ?1 is the newest task the chosen AI must not be shown (ai::mail_policy::withheld).
const URGENT_TASKS_QUERY: &str = "SELECT title, deadline FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline <= date('now', 'localtime', '+3 days') AND id > ?1 ORDER BY deadline ASC LIMIT 5";

impl DayContext {
    pub fn gather(db: &Arc<Database>) -> Result<Self, String> {
        // Greeting
        let hour: u32 = chrono::Local::now().hour();
        let greeting = match hour {
            0..=11 => "Good morning",
            12..=17 => "Good afternoon",
            _ => "Good evening",
        };

        // A task made from an email carries its words in the title. Read
        // before the lock below: `withheld` takes it too.
        let withheld = crate::ai::mail_policy::withheld(db);

        // Gather all data in one short lock to minimize mutex contention
        let (pending_count, overdue_count, due_today, urgent_tasks, todays_events, unread, important_senders, open_prs, assigned_issues, has_synced_email, has_synced_calendar) = {
            let conn = db.conn.lock().map_err(|e| e.to_string())?;

            let pending_count: i64 = conn.query_row("SELECT COUNT(*) FROM tasks WHERE status != 'completed'", [], |r| r.get(0)).unwrap_or(0);
            let overdue_count: i64 = conn.query_row(OVERDUE_TASKS_QUERY, [], |r| r.get(0)).unwrap_or(0);
            let due_today: i64 = conn.query_row(DUE_TODAY_QUERY, [], |r| r.get(0)).unwrap_or(0);

            let urgent_tasks = {
                let mut stmt = conn.prepare(URGENT_TASKS_QUERY).map_err(|e| e.to_string())?;
                let tasks: Vec<String> = stmt.query_map([withheld.tasks], |row| {
                    let title: String = row.get(0)?;
                    let deadline: String = row.get(1)?;
                    Ok(format!("- {} (due {})", title, deadline))
                }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
                tasks.join("\n")
            };

            let todays_events = {
                let mut stmt = conn.prepare(&todays_events_query()).map_err(|e| e.to_string())?;
                let events: Vec<String> = stmt.query_map([], |row| {
                    let summary: String = row.get(0)?;
                    let start: String = row.get(1)?;
                    let start = crate::ai::tools::to_local_time(&start, &chrono::Local);
                    let location: Option<String> = row.get(3)?;
                    let loc = location.map(|l| format!(" at {}", l)).unwrap_or_default();
                    Ok(format!("- {} ({}{})", summary, start, loc))
                }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
                events
            };

            let unread: i64 = conn.query_row(UNREAD_COUNT_QUERY, [], |r| r.get(0)).unwrap_or(0);

            let important_senders = {
                let mut stmt = conn.prepare(UNREAD_SENDERS_QUERY).map_err(|e| e.to_string())?;
                let emails: Vec<String> = stmt.query_map([], |row| {
                    let sender: String = row.get(0)?;
                    let subject: Option<String> = row.get(1)?;
                    Ok(format!("- {} -- {}", sender, subject.unwrap_or_default()))
                }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
                emails.join("\n")
            };

            let open_prs: i64 = conn.query_row("SELECT COUNT(*) FROM github_items WHERE item_type IN ('pr', 'pr_review') AND state != 'closed'", [], |r| r.get(0)).unwrap_or(0);
            let assigned_issues: i64 = conn.query_row("SELECT COUNT(*) FROM github_items WHERE item_type = 'issue' AND state = 'open'", [], |r| r.get(0)).unwrap_or(0);

            // Default false: if the count cannot be read, stay quiet rather than
            // assert an empty inbox the model would repeat back as fact.
            let has_synced_email: bool = conn.query_row(HAS_EMAIL_ROWS_QUERY, [], |r| r.get(0)).unwrap_or(false);
            let has_synced_calendar: bool = conn.query_row(HAS_CALENDAR_ROWS_QUERY, [], |r| r.get(0)).unwrap_or(false);

            (pending_count, overdue_count, due_today, urgent_tasks, todays_events, unread, important_senders, open_prs, assigned_issues, has_synced_email, has_synced_calendar)
        }; // Lock released here -- scheduler and other jobs can proceed

        let tasks_summary = format!("{} pending tasks. {} overdue. {} due today.\n{}", pending_count, overdue_count, due_today, urgent_tasks);
        // Senders, subjects and event titles are Google data: only for an AI
        // allowed to see them (ai::mail_policy).
        let google_data_allowed = crate::ai::mail_policy::check(db).is_ok();
        let email_summary = if google_data_allowed {
            format!("{} unread emails.\n{}", unread, important_senders)
        } else {
            format!("{} unread emails.", unread)
        };
        let calendar_summary = match todays_events.len() {
            0 => "No meetings today.".to_string(),
            _ if google_data_allowed => todays_events.join("\n"),
            1 => "1 meeting today.".to_string(),
            n => format!("{} meetings today.", n),
        };
        let github_summary = format!("{} open PRs, {} assigned issues.", open_prs, assigned_issues);
        let deadlines = urgent_tasks.clone();

        Ok(DayContext {
            greeting: format!("{}, {}.", greeting, crate::util::display_name(db)),
            tasks_summary,
            calendar_summary,
            email_summary,
            github_summary,
            deadlines,
            has_synced_email,
            has_synced_calendar,
        })
    }

    /// The one-line [CONTEXT] block injected ahead of a chat turn. Every line
    /// of a section is kept: with only the first, three meetings read as
    /// "just standup today".
    ///
    /// A section whose mirror has never been synced is omitted rather than
    /// reported as zero: "EMAIL: 0 unread emails." is a statement about the
    /// user's inbox, and before the first sync it is only a statement about an
    /// empty table. Omitting it leaves the model to call the live tool instead
    /// of confidently answering "you have no unread mail".
    pub fn inline_context(&self) -> String {
        fn one_line(s: &str) -> String {
            s.lines()
                .map(|line| line.trim().trim_start_matches("- "))
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("; ")
        }
        let mut parts = vec![format!("TASKS: {}", one_line(&self.tasks_summary))];
        if self.has_synced_calendar {
            parts.push(format!("CALENDAR: {}", one_line(&self.calendar_summary)));
        }
        if self.has_synced_email {
            parts.push(format!("EMAIL: {}", one_line(&self.email_summary)));
        }
        parts.push(format!("GITHUB: {}", self.github_summary));
        format!("[CONTEXT] {}\n{}", parts.join(" | "), CACHE_NOTE)
    }

    pub fn to_prompt(&self) -> String {
        let mut prompt = format!(
            "Here is the user's current status for today:\n\n\
             TASKS:\n{}\n\n",
            self.tasks_summary
        );
        // Same rule as inline_context: an unsynced mirror is not evidence of an
        // empty inbox or an empty day, so it does not go in the briefing.
        if self.has_synced_calendar {
            prompt.push_str(&format!("CALENDAR:\n{}\n\n", self.calendar_summary));
        }
        if self.has_synced_email {
            prompt.push_str(&format!("EMAIL:\n{}\n\n", self.email_summary));
        }
        prompt.push_str(&format!(
            "GITHUB:\n{}\n\n\
             APPROACHING DEADLINES:\n{}\n\n\
             {}\n\n\
             Based on this information, give a concise morning briefing. \
             Mention the most important things first. \
             If there are overdue items, flag them urgently. \
             Suggest what to focus on today. \
             Keep it under 150 words. Be direct, like JARVIS.",
            self.github_summary, self.deadlines, CACHE_NOTE
        ));
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// The briefing and the injected [CONTEXT] block must report the same
    /// unread mail the Email page shows, otherwise archiving nine of twelve
    /// mails leaves the assistant insisting there are still twelve.
    #[test]
    fn unread_context_excludes_archived_mail() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE emails (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 gmail_id TEXT UNIQUE NOT NULL,
                 subject TEXT,
                 sender TEXT NOT NULL,
                 is_read INTEGER DEFAULT 0,
                 received_at TEXT NOT NULL,
                 is_archived INTEGER NOT NULL DEFAULT 0);
             INSERT INTO emails (gmail_id, subject, sender, is_read, received_at, is_archived)
                 VALUES ('open', 'Invoice', 'billing@example.com', 0, '2026-07-27', 0),
                        ('filed', 'Newsletter', 'news@example.com', 0, '2026-07-26', 1);",
        )
        .unwrap();

        let unread: i64 = conn.query_row(UNREAD_COUNT_QUERY, [], |r| r.get(0)).unwrap();
        assert_eq!(unread, 1, "archived mail must not be counted as unread");

        let mut stmt = conn.prepare(UNREAD_SENDERS_QUERY).unwrap();
        let senders: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(senders, vec!["billing@example.com".to_string()], "archived senders must not be named");
    }

    /// Zero rows in the mirror means "never synced", not "there is nothing
    /// there". Nothing synced on connect, so for the up-to-five-minute window
    /// before the first scheduler tick the block stated "EMAIL: 0 unread
    /// emails." as fact and the model answered the user's inbox question from
    /// an empty table.
    #[test]
    fn context_omits_email_line_when_never_synced() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE emails (id INTEGER PRIMARY KEY AUTOINCREMENT, gmail_id TEXT UNIQUE NOT NULL,
                 sender TEXT NOT NULL, is_read INTEGER DEFAULT 0, received_at TEXT NOT NULL,
                 is_archived INTEGER NOT NULL DEFAULT 0);",
        )
        .unwrap();
        let synced: bool = conn.query_row(HAS_EMAIL_ROWS_QUERY, [], |r| r.get(0)).unwrap();
        assert!(!synced, "an empty mirror is a mirror that has never been synced");
        conn.execute_batch(
            "INSERT INTO emails (gmail_id, sender, is_read, received_at, is_archived)
                 VALUES ('filed', 'news@example.com', 1, '2026-09-04', 1);",
        )
        .unwrap();
        let synced: bool = conn.query_row(HAS_EMAIL_ROWS_QUERY, [], |r| r.get(0)).unwrap();
        assert!(synced, "a synced mirror counts even when every mail is read and archived");

        let never_synced = DayContext {
            greeting: "Good morning, Hillman.".to_string(),
            tasks_summary: "3 pending tasks. 1 overdue. 0 due today.".to_string(),
            calendar_summary: "No meetings today.".to_string(),
            email_summary: "0 unread emails.".to_string(),
            github_summary: "0 open PRs, 0 assigned issues.".to_string(),
            deadlines: String::new(),
            has_synced_email: false,
            has_synced_calendar: false,
        };

        let inline = never_synced.inline_context();
        assert!(!inline.contains("EMAIL:"), "unsynced mail must not be reported as zero: {}", inline);
        assert!(!inline.contains("CALENDAR:"), "unsynced events must not be reported as none: {}", inline);
        assert!(inline.contains("TASKS:"), "local tables are still reportable: {}", inline);
        assert!(inline.contains(CACHE_NOTE), "the block must say the numbers are a cache: {}", inline);

        let prompt = never_synced.to_prompt();
        assert!(!prompt.contains("EMAIL:"), "the briefing prompt has the same defect: {}", prompt);
        assert!(!prompt.contains("CALENDAR:"), "the briefing prompt has the same defect: {}", prompt);
        assert!(prompt.contains(CACHE_NOTE));

        // Once a sync has landed the numbers are real and both lines return.
        let after_sync = DayContext {
            has_synced_email: true,
            has_synced_calendar: true,
            ..never_synced
        };
        let inline = after_sync.inline_context();
        assert!(inline.contains("EMAIL: 0 unread emails."), "{}", inline);
        assert!(inline.contains("CALENDAR: No meetings today."), "{}", inline);
        assert!(after_sync.to_prompt().contains("EMAIL:"));
    }

    /// "Today's meetings" must follow the user's local day, for both shapes
    /// start_time comes in: a bare 'YYYY-MM-DD' for an all-day event (already a
    /// local calendar date) and an RFC3339 instant for a timed one. The UTC day
    /// gets the timed rows wrong; applying 'localtime' to the all-day rows gets
    /// those wrong for every negative UTC offset.
    #[test]
    fn todays_events_follow_the_local_day() {
        fn events_db(tz: &str) -> Connection {
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
            conn.execute_batch(&format!(
                "INSERT INTO calendar_events (google_id, summary, start_time, end_time) VALUES
                     ('allday_today', 'HKSTP demo day', '2026-07-27', '2026-07-28'),
                     ('allday_tomorrow', 'Cohort review', '2026-07-28', '2026-07-29'),
                     ('timed_today', 'Standup', '2026-07-27T09:00:00{tz}', '2026-07-27T09:30:00{tz}'),
                     ('timed_tomorrow', 'Mentor sync', '2026-07-28T14:00:00{tz}', '2026-07-28T15:00:00{tz}');",
                tz = tz
            ))
            .unwrap();
            conn
        }
        fn summaries(conn: &Connection, sql: &str) -> Vec<String> {
            let mut stmt = conn.prepare(sql).unwrap();
            let rows: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            rows
        }

        // 'localtime' resolves against whatever machine runs the test, so pin
        // it. Hong Kong, UTC+8, at 17:00 local on 2026-07-27.
        let hk = events_db("+08:00");
        let pinned = todays_events_query()
            .replace("'localtime'", "'+8 hours'")
            .replace("'now'", "'2026-07-27 09:00:00'");
        assert_eq!(
            summaries(&hk, &pinned),
            vec!["HKSTP demo day".to_string(), "Standup".to_string()]
        );

        // New York, UTC-4, at 10:00 local on the same date.
        let ny = events_db("-04:00");
        let pinned = todays_events_query()
            .replace("'localtime'", "'-4 hours'")
            .replace("'now'", "'2026-07-27 14:00:00'");
        assert_eq!(
            summaries(&ny, &pinned),
            vec!["HKSTP demo day".to_string(), "Standup".to_string()]
        );

        // Shifting the all-day column too dropped today's all-day event and
        // presented tomorrow's as today's, for the whole day.
        let over_shifted = "SELECT summary FROM calendar_events \
                            WHERE date(start_time, '-4 hours') = date('2026-07-27 14:00:00', '-4 hours') \
                            ORDER BY datetime(start_time) ASC";
        assert_eq!(
            summaries(&ny, over_shifted),
            vec!["Standup".to_string(), "Cohort review".to_string()]
        );

        // The pre-fix predicate used the UTC day: for a UTC+8 user at 07:00
        // local it reported nothing at all as today's.
        let utc_day = "SELECT summary FROM calendar_events \
                       WHERE date(start_time) = date('2026-07-26 23:00:00') \
                       ORDER BY datetime(start_time) ASC";
        assert_eq!(summaries(&hk, utc_day), Vec::<String>::new());
    }

    /// Overdue / due-today counts and the 3-day deadline window must follow the
    /// user's local day too. For a UTC+8 user, date('now') on its own is still
    /// yesterday until 08:00 local, so today's tasks were reported as neither
    /// due nor overdue while yesterday's were reported as due today.
    #[test]
    fn task_deadlines_follow_the_local_day() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 title TEXT NOT NULL,
                 description TEXT,
                 deadline TEXT,
                 priority INTEGER DEFAULT 2,
                 status TEXT DEFAULT 'pending');
             INSERT INTO tasks (title, deadline, status) VALUES
                 ('File HKSTP milestone', '2026-07-27', 'pending'),
                 ('Call mentor', '2026-07-27', 'pending'),
                 ('Yesterday invoice', '2026-07-26', 'pending'),
                 ('Older chase-up', '2026-07-20', 'pending'),
                 ('Board deck', '2026-07-30', 'pending'),
                 ('Already sent', '2026-07-27', 'completed');",
        )
        .unwrap();

        // 'localtime' resolves against whatever machine runs the test, so pin it
        // to +08:00, the same way the calendar test above does. Clock: 07:00 HK
        // on 2026-07-27, which is still 2026-07-26 in UTC.
        let pin = |q: &str| {
            q.replace("'localtime'", "'+8 hours'")
                .replace("'now'", "'2026-07-26 23:00:00'")
        };

        let overdue: i64 = conn
            .query_row(&pin(OVERDUE_TASKS_QUERY), [], |r| r.get(0))
            .unwrap();
        assert_eq!(overdue, 2, "yesterday's and older open tasks are overdue");

        let due_today: i64 = conn
            .query_row(&pin(DUE_TODAY_QUERY), [], |r| r.get(0))
            .unwrap();
        assert_eq!(due_today, 2, "tasks dated today must count as due today");

        let mut stmt = conn.prepare(&pin(URGENT_TASKS_QUERY)).unwrap();
        let urgent: Vec<String> = stmt
            .query_map([0], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            urgent.contains(&"Board deck".to_string()),
            "the 3-day window is measured from the local day"
        );

        // The pre-fix predicates used the UTC day, which was still 2026-07-26.
        let old_overdue: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status != 'completed' \
                 AND deadline IS NOT NULL AND deadline < date('2026-07-26 23:00:00')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_overdue, 1);

        let old_due_today: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status != 'completed' \
                 AND deadline = date('2026-07-26 23:00:00')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_due_today, 1, "the UTC day counted yesterday's task");
    }

    #[test]
    fn context_keeps_every_meeting_and_task_on_one_line() {
        let ctx = DayContext {
            greeting: String::new(),
            tasks_summary: "2 pending tasks. 0 overdue. 1 due today.\n- Call Mary (due 2026-10-31)\n- File VAT (due 2026-11-02)".into(),
            calendar_summary: "- Standup (09:00)\n- Design review (11:00)\n- Investor call (15:00 at Cyberport)".into(),
            email_summary: "1 unread emails.\n- sam@example.com -- Q3 report".into(),
            github_summary: "0 open PRs, 0 assigned issues.".into(),
            deadlines: String::new(),
            has_synced_email: true,
            has_synced_calendar: true,
        };
        let block = ctx.inline_context();
        let first = block.lines().next().unwrap();
        for expected in ["Call Mary", "File VAT", "Standup", "Design review", "Investor call (15:00 at Cyberport)", "Q3 report"] {
            assert!(first.contains(expected), "{expected} missing from {first}");
        }
        assert!(first.contains("CALENDAR: Standup (09:00); Design review (11:00); Investor call"), "{first}");
    }

    /// Event titles and places are Google Calendar data, under the same rule
    /// as mail (ai::mail_policy): an AI that may train on them only learns how
    /// many meetings there are.
    #[test]
    fn calendar_titles_reach_only_an_ai_allowed_to_see_google_data() {
        let db = Arc::new(crate::db::test_database());
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO calendar_events (google_id, summary, start_time, end_time, location) VALUES ('e1', 'Term sheet call', ?1, ?1, 'Cyberport')",
                [chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)],
            )
            .unwrap();
            conn.execute_batch(
                "DELETE FROM provider_chain;
                 INSERT INTO provider_chain(position, provider_type, enabled, base_url) VALUES (0, 'openai', 1, 'https://api.moonshot.ai/v1');",
            )
            .unwrap();
        }
        // Kimi: not even after the user agreed to its terms (DECISIONS, P06
        // "Google Limited Use", 2026-11-17).
        crate::ai::mail_policy::test_support::agree_to_kimi(&db);
        let blocked = DayContext::gather(&db).unwrap();
        assert_eq!(blocked.calendar_summary, "1 meeting today.");
        let sent = format!("{}{}", blocked.inline_context(), blocked.to_prompt());
        assert!(!sent.contains("Term sheet") && !sent.contains("Cyberport"), "{sent}");

        db.conn.lock().unwrap().execute("UPDATE provider_chain SET base_url = NULL", []).unwrap();
        let allowed = DayContext::gather(&db).unwrap();
        assert!(allowed.calendar_summary.contains("Term sheet call") && allowed.calendar_summary.contains("Cyberport"), "{}", allowed.calendar_summary);

        // Gemini once its billing is confirmed.
        crate::ai::mail_policy::test_support::use_ai(&db, "gemini", None);
        assert_eq!(DayContext::gather(&db).unwrap().calendar_summary, "1 meeting today.", "billing not confirmed yet");
        crate::ai::mail_policy::test_support::agree_to_gemini(&db);
        assert!(DayContext::gather(&db).unwrap().calendar_summary.contains("Term sheet call"));
    }

    /// Google stores a timed event in the calendar's own zone. The model reads
    /// these times and answers or reschedules in the Mac's zone (see
    /// ai::tools::with_local_offset), so they have to arrive in that zone.
    #[test]
    fn todays_event_times_reach_the_model_in_the_macs_zone() {
        let now = chrono::Local::now();
        let other = if now.offset().local_minus_utc() == 8 * 3600 { 9 } else { 8 };
        let stored = now
            .with_timezone(&chrono::FixedOffset::east_opt(other * 3600).unwrap())
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        let db = Arc::new(crate::db::test_database());
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO calendar_events (google_id, summary, start_time, end_time) VALUES ('e1', 'Standup', ?1, ?1)",
                [&stored],
            )
            .unwrap();
            // An AI allowed to see event titles (see the test above).
            conn.execute_batch(
                "DELETE FROM provider_chain;
                 INSERT INTO provider_chain(position, provider_type, enabled, base_url) VALUES (0, 'openai', 1, NULL);",
            )
            .unwrap();
        }
        let summary = DayContext::gather(&db).unwrap().calendar_summary;
        let local = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        assert!(summary.contains(&format!("Standup ({local})")), "stored {stored}, got {summary}");
    }

    /// A task made from an email carries the email's words in its title. The
    /// count stays, as it does for unread mail.
    #[test]
    fn task_titles_saved_before_a_move_to_an_ai_that_may_not_read_email_are_left_out() {
        let db = Arc::new(crate::ai::mail_policy::test_support::moved_to_kimi_after(|db| {
            db.conn
                .lock()
                .unwrap()
                .execute("INSERT INTO tasks(title, deadline) VALUES ('Pay Kenny HK$12,000', date('now', 'localtime'))", [])
                .unwrap();
        }));
        // The first turn after the move draws the line; this task is added after it.
        crate::ai::mail_policy::withheld_as_turn_begins(&db);
        db.conn.lock().unwrap().execute("INSERT INTO tasks(title, deadline) VALUES ('Buy milk', date('now', 'localtime'))", []).unwrap();
        let context = DayContext::gather(&db).unwrap();
        let sent = format!("{}{}", context.inline_context(), context.to_prompt());
        assert!(sent.contains("2 pending tasks.") && sent.contains("Buy milk"), "{sent}");
        assert!(!sent.contains("Kenny"), "{sent}");
    }
}
