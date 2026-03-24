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
}

impl DayContext {
    pub fn gather(db: &Arc<Database>) -> Result<Self, String> {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;

        // Greeting
        let hour: u32 = chrono::Local::now().hour();
        let greeting = match hour {
            0..=11 => "Good morning",
            12..=17 => "Good afternoon",
            _ => "Good evening",
        };

        // Tasks
        let pending_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status != 'completed'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let overdue_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline < date('now')",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let due_today: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline = date('now')",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let urgent_tasks = {
            let mut stmt = conn
                .prepare(
                    "SELECT title, deadline FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline <= date('now', '+3 days') ORDER BY deadline ASC LIMIT 5",
                )
                .map_err(|e| e.to_string())?;
            let tasks: Vec<String> = stmt
                .query_map([], |row| {
                    let title: String = row.get(0)?;
                    let deadline: String = row.get(1)?;
                    Ok(format!("- {} (due {})", title, deadline))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            tasks.join("\n")
        };

        let tasks_summary = format!(
            "{} pending tasks. {} overdue. {} due today.\n{}",
            pending_count, overdue_count, due_today, urgent_tasks
        );

        // Calendar
        let todays_events = {
            let mut stmt = conn
                .prepare(
                    "SELECT summary, start_time, end_time, location FROM calendar_events WHERE date(start_time) = date('now') ORDER BY start_time ASC",
                )
                .map_err(|e| e.to_string())?;
            let events: Vec<String> = stmt
                .query_map([], |row| {
                    let summary: String = row.get(0)?;
                    let start: String = row.get(1)?;
                    let location: Option<String> = row.get(3)?;
                    let loc = location
                        .map(|l| format!(" at {}", l))
                        .unwrap_or_default();
                    Ok(format!("- {} ({}{})", summary, start, loc))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            if events.is_empty() {
                "No meetings today.".to_string()
            } else {
                events.join("\n")
            }
        };

        // Email
        let unread: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM emails WHERE is_read = 0",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let important_senders = {
            let mut stmt = conn
                .prepare(
                    "SELECT sender, subject FROM emails WHERE is_read = 0 ORDER BY received_at DESC LIMIT 3",
                )
                .map_err(|e| e.to_string())?;
            let emails: Vec<String> = stmt
                .query_map([], |row| {
                    let sender: String = row.get(0)?;
                    let subject: Option<String> = row.get(1)?;
                    Ok(format!("- {} -- {}", sender, subject.unwrap_or_default()))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            emails.join("\n")
        };

        let email_summary = format!("{} unread emails.\n{}", unread, important_senders);

        // GitHub
        let open_prs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM github_items WHERE item_type IN ('pr', 'pr_review') AND state != 'closed'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let assigned_issues: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM github_items WHERE item_type = 'issue' AND state = 'open'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let github_summary = format!(
            "{} open PRs, {} assigned issues.",
            open_prs, assigned_issues
        );

        // Deadlines within 3 days
        let deadlines = urgent_tasks.clone();

        Ok(DayContext {
            greeting: format!("{}, Hillman.", greeting),
            tasks_summary,
            calendar_summary: todays_events,
            email_summary,
            github_summary,
            deadlines,
        })
    }

    pub fn to_prompt(&self) -> String {
        format!(
            "Here is the user's current status for today:\n\n\
             TASKS:\n{}\n\n\
             CALENDAR:\n{}\n\n\
             EMAIL:\n{}\n\n\
             GITHUB:\n{}\n\n\
             APPROACHING DEADLINES:\n{}\n\n\
             Based on this information, give a concise morning briefing. \
             Mention the most important things first. \
             If there are overdue items, flag them urgently. \
             Suggest what to focus on today. \
             Keep it under 150 words. Be direct, like JARVIS.",
            self.tasks_summary,
            self.calendar_summary,
            self.email_summary,
            self.github_summary,
            self.deadlines
        )
    }
}
