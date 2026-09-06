use serde_json::json;

pub fn read_language_pref(db: &crate::db::Database) -> String {
    let conn = match db.conn.lock() {
        Ok(c) => c,
        Err(_) => return "en".into(),
    };
    conn.query_row(
        "SELECT value FROM user_preferences WHERE key = 'language'",
        [],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| "en".into())
}

/// Shared tool definitions used by Gemini, OpenAI-compatible local, and legacy APIs
pub fn get_tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "open_app".into(),
            description: "Open a macOS application. Use exact app names: 'Google Chrome', 'Visual Studio Code', 'Finder', 'Terminal', 'Safari', 'Spotify', 'Slack', 'Discord', 'Notion', 'Obsidian'".into(),
            parameters: json!({
                "type": "object",
                "properties": { "name": { "type": "string", "description": "Exact macOS application name" } },
                "required": ["name"]
            }),
        },
        // `open_url` is deliberately absent (removed 2026-07-27). It handed a
        // fully model-controlled string to macOS `open`, and a URL query string
        // is a raw outbound channel -- with `read_email` in the same turn, one
        // crafted email was a complete exfiltration chain. It only comes back
        // behind a real user approval round-trip, never behind a system-prompt
        // sentence. `crate::system::control::open_url` still serves non-model
        // callers.
        Tool {
            name: "run_command".into(),
            description: "Run one read-only allowlisted macOS command for system information, disk and memory usage, process listing, or directory listing (for example uname, sw_vers, df, ps, ls) and return its output, refusing anything else.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "command": { "type": "string", "description": "Shell command to execute" } },
                "required": ["command"]
            }),
        },
        Tool {
            name: "find_files".into(),
            description: "Search for files on the computer using macOS Spotlight".into(),
            parameters: json!({
                "type": "object",
                "properties": { "query": { "type": "string", "description": "Filename or content to search for" } },
                "required": ["query"]
            }),
        },
        Tool {
            name: "open_file".into(),
            description: "Open a file with its default application".into(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Full file path" } },
                "required": ["path"]
            }),
        },
        Tool {
            name: "create_task".into(),
            description: "Create a task/reminder in JARVIS task list".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "deadline": { "type": "string", "description": "YYYY-MM-DD format" },
                    "priority": { "type": "integer", "description": "0=low, 1=normal, 2=high, 3=urgent" }
                },
                "required": ["title"]
            }),
        },
        Tool {
            name: "system_info".into(),
            description: "Get system information: hostname, uptime, disk usage, memory".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "send_notification".into(),
            description: "Send a macOS desktop notification with a title and message".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Notification title" },
                    "message": { "type": "string", "description": "Notification body text" },
                    "sound": { "type": "boolean", "description": "Play notification sound (default false)" }
                },
                "required": ["title", "message"]
            }),
        },
        Tool {
            name: "search_emails".into(),
            description: "Search Gmail messages using Gmail search syntax. Examples: 'from:user@example.com', 'subject:meeting', 'is:unread', 'after:2026/01/01 before:2026/03/01', 'has:attachment'. Returns up to 10 results.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Gmail search query (supports Gmail search syntax)" }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "read_email".into(),
            description: "Read the full content of a Gmail message by its ID. Returns headers and body text.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "email_id": { "type": "string", "description": "Gmail message ID" }
                },
                "required": ["email_id"]
            }),
        },
        Tool {
            name: "draft_email".into(),
            description: "Create a Gmail draft for the user to review and send. JARVIS never sends mail on its own -- always tell the user the draft is waiting in Gmail.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "to": { "type": "string", "description": "Recipient email address" },
                    "subject": { "type": "string", "description": "Email subject line" },
                    "body": { "type": "string", "description": "Plain text email body" },
                    "cc": { "type": "string", "description": "CC email address (optional)" }
                },
                "required": ["to", "subject", "body"]
            }),
        },
        Tool {
            name: "archive_email".into(),
            description: "Archive a Gmail message by removing it from the inbox. The message is not deleted.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "email_id": { "type": "string", "description": "Gmail message ID to archive" }
                },
                "required": ["email_id"]
            }),
        },
        Tool {
            name: "list_events".into(),
            description: "List upcoming Google Calendar events. Defaults to next 7 days if no dates specified.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "date_from": { "type": "string", "description": "Start date in YYYY-MM-DD format (default: today)" },
                    "date_to": { "type": "string", "description": "End date in YYYY-MM-DD format (default: 7 days from today)" }
                }
            }),
        },
        // No `attendees` property (removed 2026-07-27). Google copies the event
        // -- title and description included -- onto every invited attendee's own
        // calendar, so a model-chosen address was a silent outbound channel that
        // an attacker reads back with events.list on their own account. The user
        // adds invitees in Google Calendar.
        Tool {
            name: "create_event".into(),
            description: "Create a new Google Calendar event. Times must be in RFC 3339 format using the UTC offset given in the system prompt (e.g. 2026-09-05T10:00:00+08:00 using the offset from the system prompt).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Event title/summary" },
                    "start": { "type": "string", "description": "Start time in RFC 3339 format with the UTC offset from the system prompt" },
                    "end": { "type": "string", "description": "End time in RFC 3339 format with the UTC offset from the system prompt" },
                    "location": { "type": "string", "description": "Event location (optional)" },
                    "description": { "type": "string", "description": "Event description (optional)" }
                },
                "required": ["title", "start", "end"]
            }),
        },
        Tool {
            name: "update_event".into(),
            description: "Update an existing Google Calendar event. Only provided fields will be changed. Times must be in RFC 3339 format using the UTC offset given in the system prompt (e.g. 2026-09-05T10:00:00+08:00 using the offset from the system prompt).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "event_id": { "type": "string", "description": "Google Calendar event ID" },
                    "title": { "type": "string", "description": "New event title/summary" },
                    "start": { "type": "string", "description": "New start time in RFC 3339 format with the UTC offset from the system prompt" },
                    "end": { "type": "string", "description": "New end time in RFC 3339 format with the UTC offset from the system prompt" },
                    "location": { "type": "string", "description": "New event location" },
                    "description": { "type": "string", "description": "New event description" }
                },
                "required": ["event_id"]
            }),
        },
        Tool {
            name: "delete_event".into(),
            description: "Delete a Google Calendar event by its ID.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "event_id": { "type": "string", "description": "Google Calendar event ID to delete" }
                },
                "required": ["event_id"]
            }),
        },
        Tool {
            name: "search_notion".into(),
            description: "Search Notion pages by query. Returns matching pages with IDs, titles, and URLs.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query to find Notion pages" }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "read_notion_page".into(),
            description: "Read the content of a Notion page by its ID. Returns the page content as markdown-like text.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "page_id": { "type": "string", "description": "Notion page ID" }
                },
                "required": ["page_id"]
            }),
        },
        Tool {
            name: "create_notion_page".into(),
            description: "Create a new Notion page under a parent page.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Page title" },
                    "content": { "type": "string", "description": "Page content (plain text)" },
                    "parent_id": { "type": "string", "description": "Parent page ID to create the new page under" }
                },
                "required": ["title", "content", "parent_id"]
            }),
        },
        Tool {
            name: "list_github_items".into(),
            description: "List GitHub issues and pull requests assigned to you or awaiting your review. Filter by type and optionally by repository.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "item_type": { "type": "string", "description": "Filter type: 'prs', 'issues', or 'all'", "enum": ["prs", "issues", "all"] },
                    "repo": { "type": "string", "description": "Optional repository filter in 'owner/repo' format" }
                },
                "required": ["item_type"]
            }),
        },
        Tool {
            name: "create_github_issue".into(),
            description: "Create a new GitHub issue. Only repositories the user already works with are accepted -- the ones list_github_items returns.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "Repository in 'owner/repo' format. Must be one the user already works with." },
                    "title": { "type": "string", "description": "Issue title" },
                    "body": { "type": "string", "description": "Issue body/description (optional)" },
                    "labels": { "type": "string", "description": "Comma-separated labels (optional)" }
                },
                "required": ["repo", "title"]
            }),
        },
        Tool {
            name: "search_notes".into(),
            description: "Search notes in the local Obsidian vault. Returns matching note paths and content snippets.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query to find notes" }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "read_note".into(),
            description: "Read the full content of a note from the local Obsidian vault by its file path.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Note file path within the vault (e.g. 'folder/note.md')" }
                },
                "required": ["path"]
            }),
        },
        Tool {
            name: "render_chart".into(),
            description: "Render a data chart inline in the chat. Use when presenting numerical data, trends, or comparisons.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chart_type": {
                        "type": "string",
                        "enum": ["line", "pie", "bar"],
                        "description": "Type of chart to render"
                    },
                    "data": {
                        "type": "object",
                        "description": "Chart data. For line/bar: {labels: string[], series: [{name: string, data: number[]}]}. For pie: {segments: [{label: string, value: number}]}"
                    }
                },
                "required": ["chart_type", "data"]
            }),
        },
        Tool {
            name: "render_status".into(),
            description: "Show a status card for a completed action. Use after creating tasks, syncing data, or completing any action.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "status_type": {
                        "type": "string",
                        "enum": ["task_created", "task_completed", "email_synced", "calendar_synced", "cron_created", "action_completed"],
                        "description": "Type of status to display"
                    },
                    "data": {
                        "type": "object",
                        "description": "Status data. Fields vary by type: task_created needs name/priority/due, email_synced needs count/folder, etc."
                    }
                },
                "required": ["status_type", "data"]
            }),
        },
        Tool {
            name: "remember".into(),
            description: "Save a durable fact about the user to persistent memory. Use when they tell you who they are, what they prefer, what they are working on, or anything worth recalling next time. Keep entries short and atomic (one fact each).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "The fact to remember, written in third person (e.g. 'Prefers dark mode in all apps')." },
                    "category": {
                        "type": "string",
                        "enum": ["identity", "preference", "project", "fact"],
                        "description": "identity = who he is; preference = how he likes things; project = what he's working on; fact = anything else"
                    }
                },
                "required": ["content", "category"]
            }),
        },
        Tool {
            name: "forget".into(),
            description: "Remove a fact from persistent memory. Use when the user says to forget something, or when a stored fact is no longer true. Pass either the exact id from list_memories OR a keyword that uniquely matches the entry.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Memory entry id (preferred when known)" },
                    "keyword": { "type": "string", "description": "Substring that matches the entry to remove" }
                }
            }),
        },
        Tool {
            name: "list_memories".into(),
            description: "List all stored persistent memory entries about the user. Use sparingly -- most facts are already in your system prompt. Only call when the user explicitly asks 'what do you remember about me'.".into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

#[derive(Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Format tools for OpenAI API format
pub fn to_openai_format(tools: &[Tool]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        })
        .collect()
}

/// Format tools for Claude API format
pub fn to_claude_format(tools: &[Tool]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        })
        .collect()
}

/// Format tools for Gemini API format (functionDeclarations)
pub fn to_gemini_format(tools: &[Tool]) -> Vec<serde_json::Value> {
    let declarations: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
            })
        })
        .collect();
    vec![json!({ "functionDeclarations": declarations })]
}

/// Map tool names to user-friendly status labels for streaming UI.
pub fn tool_status_label(name: &str) -> &'static str {
    match name {
        "search_emails" => "Searching emails...",
        "read_email" => "Reading email...",
        "draft_email" => "Drafting email...",
        "archive_email" => "Archiving email...",
        "list_events" => "Checking calendar...",
        "create_event" => "Creating event...",
        "update_event" => "Updating event...",
        "delete_event" => "Deleting event...",
        "search_notion" => "Searching Notion...",
        "read_notion_page" => "Reading Notion page...",
        "create_notion_page" => "Creating Notion page...",
        "list_github_items" => "Checking GitHub...",
        "create_github_issue" => "Creating GitHub issue...",
        "search_notes" => "Searching notes...",
        "read_note" => "Reading note...",
        "open_app" => "Opening app...",
        "run_command" => "Running command...",
        "find_files" => "Finding files...",
        "open_file" => "Opening file...",
        "create_task" => "Creating task...",
        "system_info" => "Checking system...",
        "send_notification" => "Sending notification...",
        "render_chart" => "Rendering chart...",
        "render_status" => "Showing result...",
        "remember" => "Saving to memory...",
        "forget" => "Removing from memory...",
        "list_memories" => "Checking memory...",
        _ => "Processing...",
    }
}

/// Conversational narration for TTS during tool execution.
pub fn tool_voice_narration(name: &str) -> &'static str {
    match name {
        "search_emails" => "Searching through your emails now.",
        "read_email" => "Pulling up that email.",
        "draft_email" => "Putting a draft together for you.",
        "archive_email" => "Archiving that email.",
        "list_events" => "Let me check your calendar.",
        "create_event" => "Setting that up on your calendar.",
        "update_event" => "Updating that event now.",
        "delete_event" => "Removing that event.",
        "search_notion" => "Searching your Notion workspace.",
        "read_notion_page" => "Reading that Notion page.",
        "create_notion_page" => "Creating a new page in Notion.",
        "list_github_items" => "Checking your GitHub activity.",
        "create_github_issue" => "Filing that GitHub issue.",
        "search_notes" => "Looking through your notes.",
        "read_note" => "Reading that note.",
        "open_app" => "Opening that up for you.",
        "run_command" => "Running that command.",
        "find_files" => "Looking for those files.",
        "open_file" => "Opening that file.",
        "create_task" => "Creating that task for you.",
        "system_info" => "Checking your system.",
        "send_notification" => "Sending a notification.",
        "render_chart" => "Putting together a chart for you.",
        "render_status" => "Here are the results.",
        "remember" => "Noting that down.",
        "forget" => "Letting that one go.",
        "list_memories" => "Pulling up what I remember.",
        _ => "Working on it.",
    }
}

fn execute_memory_tool(name: &str, args: &serde_json::Value, db: &crate::db::Database) -> String {
    let conn = match db.conn.lock() {
        Ok(c) => c,
        Err(e) => return format!("Memory error: {}", e),
    };
    match name {
        "remember" => {
            let content = match args["content"].as_str() {
                Some(s) if !s.trim().is_empty() => s.trim(),
                _ => return "Nothing to remember -- content was empty.".to_string(),
            };
            let category = args["category"].as_str().unwrap_or("fact");
            let category = match category {
                "identity" | "preference" | "project" | "fact" => category,
                _ => "fact",
            };
            let preview = content.chars().take(120).collect::<String>();
            // De-duplicate: if the same fact (case-insensitive) is already
            // stored, refresh it instead of appending a duplicate. Otherwise the
            // memory block injected into every prompt grows with repeats and
            // wastes tokens on the very hot path.
            let existing: Option<i64> = conn
                .query_row(
                    "SELECT id FROM user_memory WHERE content = ?1 COLLATE NOCASE LIMIT 1",
                    rusqlite::params![content],
                    |row| row.get(0),
                )
                .ok();
            if let Some(id) = existing {
                let _ = conn.execute(
                    "UPDATE user_memory SET category = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
                    rusqlite::params![category, id],
                );
                return format!("Already remembered ({}): {}", category, preview);
            }
            match conn.execute(
                "INSERT INTO user_memory (content, category) VALUES (?1, ?2)",
                rusqlite::params![content, category],
            ) {
                Ok(_) => format!("Remembered ({}): {}", category, preview),
                Err(e) => format!("Memory error: {}", e),
            }
        }
        "forget" => {
            if let Some(id) = args["id"].as_i64() {
                match conn.execute(
                    "DELETE FROM user_memory WHERE id = ?1",
                    rusqlite::params![id],
                ) {
                    Ok(0) => format!("No memory entry with id {}.", id),
                    Ok(_) => format!("Forgot memory entry #{}.", id),
                    Err(e) => format!("Memory error: {}", e),
                }
            } else if let Some(keyword) = args["keyword"].as_str() {
                let pattern = format!("%{}%", keyword);
                match conn.execute(
                    "DELETE FROM user_memory WHERE content LIKE ?1",
                    rusqlite::params![pattern],
                ) {
                    Ok(0) => format!("No memory entry matched '{}'.", keyword),
                    Ok(n) => format!(
                        "Forgot {} memory entr{}.",
                        n,
                        if n == 1 { "y" } else { "ies" }
                    ),
                    Err(e) => format!("Memory error: {}", e),
                }
            } else {
                "Need either an id or a keyword to forget.".to_string()
            }
        }
        "list_memories" => {
            let mut stmt = match conn
                .prepare("SELECT id, category, content FROM user_memory ORDER BY category, id")
            {
                Ok(s) => s,
                Err(e) => return format!("Memory error: {}", e),
            };
            let rows: Vec<(i64, String, String)> =
                match stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))) {
                    Ok(r) => r.filter_map(|x| x.ok()).collect(),
                    Err(e) => return format!("Memory error: {}", e),
                };
            if rows.is_empty() {
                return "No memories stored yet.".to_string();
            }
            rows.iter()
                .map(|(id, cat, content)| format!("#{} [{}] {}", id, cat, content))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => format!("Unknown memory tool: {}", name),
    }
}

/// Truncate tool results to avoid blowing up context windows.
fn truncate_result(s: String) -> String {
    if s.len() > 4000 {
        format!("{}... [truncated, {} total chars]", crate::util::truncate_bytes(&s, 4000), s.len())
    } else {
        s
    }
}

async fn execute_gmail_tool(
    name: &str,
    args: &serde_json::Value,
    token: &str,
) -> Result<String, String> {
    use crate::integrations::gmail;
    match name {
        "search_emails" => {
            let query = args["query"].as_str().unwrap_or("");
            let messages = gmail::search_messages(token, query).await?;
            if messages.is_empty() {
                return Ok("No emails found matching that query.".to_string());
            }
            let lines: Vec<String> = messages
                .iter()
                .map(|m| {
                    format!(
                        "ID: {} | From: {} | Subject: {} | Date: {}",
                        m.id,
                        m.sender.as_deref().unwrap_or("unknown"),
                        m.subject.as_deref().unwrap_or("(no subject)"),
                        m.received_at.as_deref().unwrap_or("unknown"),
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }
        "read_email" => {
            let email_id = args["email_id"].as_str().unwrap_or("");
            let (_msg, full_text) = gmail::get_message_full(token, email_id).await?;
            Ok(full_text)
        }
        "draft_email" => {
            let to = args["to"].as_str().unwrap_or("");
            let subject = args["subject"].as_str().unwrap_or("");
            let body = args["body"].as_str().unwrap_or("");
            let cc = args["cc"].as_str();

            let draft_id = gmail::create_draft(token, to, subject, body, cc).await?;
            Ok(format!(
                "Draft saved to Gmail for {} (ID: {}). Review and send it from Gmail.",
                to, draft_id
            ))
        }
        "archive_email" => {
            let email_id = args["email_id"].as_str().unwrap_or("");
            gmail::archive_message(token, email_id).await?;
            Ok(format!("Email {} archived successfully.", email_id))
        }
        _ => Err(format!("Unknown Gmail tool: {}", name)),
    }
}

async fn execute_calendar_tool(
    name: &str,
    args: &serde_json::Value,
    token: &str,
    db: &crate::db::Database,
) -> Result<String, String> {
    use crate::integrations::calendar;
    match name {
        "list_events" => {
            let now = chrono::Local::now();
            let tz_offset = now.format("%:z").to_string();
            let date_from = args["date_from"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| now.format("%Y-%m-%d").to_string());
            let date_to = args["date_to"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    (now + chrono::TimeDelta::days(7))
                        .format("%Y-%m-%d")
                        .to_string()
                });
            let time_min = format!("{}T00:00:00{}", date_from, tz_offset);
            let time_max = format!("{}T23:59:59{}", date_to, tz_offset);
            let events = calendar::fetch_events(token, &time_min, &time_max).await?;
            if events.is_empty() {
                return Ok("No events found in the specified date range.".to_string());
            }
            let lines: Vec<String> = events
                .iter()
                .map(|e| {
                    let loc = e.location.as_deref().unwrap_or("none");
                    format!(
                        "ID: {} | {} | {} - {} | Location: {}",
                        e.id, e.summary, e.start_time, e.end_time, loc,
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }
        "create_event" => {
            let title = args["title"].as_str().unwrap_or("");
            let offset = *chrono::Local::now().offset();
            let start = with_local_offset(args["start"].as_str().unwrap_or(""), offset);
            let end = with_local_offset(args["end"].as_str().unwrap_or(""), offset);
            let location = args["location"].as_str();
            let description = args["description"].as_str();
            // Attendees are never model-chosen -- inviting an address copies the
            // event body onto that person's calendar. Always None from here.
            let result =
                calendar::create_event(token, title, &start, &end, description, location, None)
                    .await?;
            Ok(format!("Event created: {}", result))
        }
        "update_event" => {
            let event_id = args["event_id"].as_str().unwrap_or("");
            let title = args["title"].as_str();
            let offset = *chrono::Local::now().offset();
            let start = args["start"].as_str().map(|s| with_local_offset(s, offset));
            let end = args["end"].as_str().map(|s| with_local_offset(s, offset));
            let location = args["location"].as_str();
            let description = args["description"].as_str();
            let result = calendar::update_event(
                token,
                event_id,
                title,
                start.as_deref(),
                end.as_deref(),
                location,
                description,
            )
            .await?;
            Ok(format!("Event updated: {}", result))
        }
        "delete_event" => {
            let event_id = args["event_id"].as_str().unwrap_or("");
            let result = calendar::delete_event(token, event_id).await?;
            // Google has confirmed the deletion, so drop the local mirror row
            // too. Without this the event stayed on the Calendar page and in the
            // briefing until a sync whose window still covered it ran.
            match db.conn.lock() {
                Ok(conn) => {
                    if let Err(e) = conn.execute(
                        "DELETE FROM calendar_events WHERE google_id = ?1",
                        rusqlite::params![event_id],
                    ) {
                        log::warn!("[calendar] local row for deleted event {} not removed: {}", event_id, e);
                    }
                }
                Err(e) => log::warn!("[calendar] could not lock db to remove deleted event {}: {}", event_id, e),
            }
            Ok(result)
        }
        _ => Err(format!("Unknown Calendar tool: {}", name)),
    }
}

async fn execute_obsidian_tool(
    name: &str,
    args: &serde_json::Value,
    api_key: &str,
    base_url: &str,
) -> Result<String, String> {
    use crate::integrations::obsidian;
    match name {
        "search_notes" => {
            let query = args["query"].as_str().unwrap_or("");
            let notes = obsidian::search_vault(base_url, api_key, query).await?;
            if notes.is_empty() {
                return Ok("No notes found matching that query.".to_string());
            }
            let lines: Vec<String> = notes
                .iter()
                .map(|n| {
                    let snippet = n.content.as_deref().unwrap_or("");
                    let truncated = crate::util::truncate_bytes(snippet, 100);
                    format!("{} | {}", n.path, truncated)
                })
                .collect();
            Ok(lines.join("\n"))
        }
        "read_note" => {
            let path = args["path"].as_str().unwrap_or("");
            obsidian::get_note(base_url, api_key, path).await
        }
        _ => Err(format!("Unknown Obsidian tool: {}", name)),
    }
}

async fn execute_github_tool(
    name: &str,
    args: &serde_json::Value,
    token: &str,
) -> Result<String, String> {
    use crate::integrations::github;
    match name {
        "list_github_items" => {
            let item_type = args["item_type"].as_str().unwrap_or("all");
            let repo = args["repo"].as_str();
            let items = github::fetch_items_filtered(token, item_type, repo).await?;
            if items.is_empty() {
                return Ok("No GitHub items found matching that filter.".to_string());
            }
            let lines: Vec<String> = items
                .iter()
                .map(|i| {
                    format!(
                        "[{}] {} - {} ({}) | {}",
                        i.item_type,
                        i.repo,
                        i.title,
                        i.state,
                        i.url.as_deref().unwrap_or("none"),
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }
        "create_github_issue" => {
            let repo_str = args["repo"].as_str().unwrap_or("");
            let title = args["title"].as_str().unwrap_or("");
            let body = args["body"].as_str();
            let labels = args["labels"].as_str();
            let parts: Vec<&str> = repo_str.splitn(2, '/').collect();
            if parts.len() != 2 {
                return Err("Invalid repo format. Use 'owner/repo'.".to_string());
            }
            let url = github::create_issue(token, parts[0], parts[1], title, body, labels).await?;
            Ok(format!("GitHub issue created: {}", url))
        }
        _ => Err(format!("Unknown GitHub tool: {}", name)),
    }
}

async fn execute_notion_tool(
    name: &str,
    args: &serde_json::Value,
    token: &str,
) -> Result<String, String> {
    use crate::integrations::notion;
    match name {
        "search_notion" => {
            let query = args["query"].as_str().unwrap_or("");
            // Inside a chat turn the user waits through every round-trip, and the
            // result is truncated to a few thousand bytes anyway, so take one page.
            let pages = notion::search_pages_bounded(
                token,
                Some(query),
                notion::INTERACTIVE_SEARCH_PAGES,
            )
            .await?;
            if pages.is_empty() {
                return Ok("No Notion pages found matching that query.".to_string());
            }
            let lines: Vec<String> = pages
                .iter()
                .map(|p| {
                    format!(
                        "ID: {} | {} | Edited: {} | URL: {}",
                        p.notion_id,
                        p.title,
                        p.last_edited.as_deref().unwrap_or("unknown"),
                        p.url.as_deref().unwrap_or("none"),
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }
        "read_notion_page" => {
            let page_id = args["page_id"].as_str().unwrap_or("");
            notion::get_page_content(token, page_id).await
        }
        "create_notion_page" => {
            let title = args["title"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            let parent_id = args["parent_id"].as_str().unwrap_or("");
            let page_id = notion::create_page(token, parent_id, title, content).await?;
            Ok(format!("Notion page created (ID: {})", page_id))
        }
        _ => Err(format!("Unknown Notion tool: {}", name)),
    }
}

async fn google_token_or_err(
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    service: &str,
) -> Result<String, String> {
    google_auth.ensure_access_token().await.map_err(|_| {
        format!(
            "Google account not connected. Please connect in Settings to use {} tools.",
            service
        )
    })
}

async fn refresh_google_token(
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
) -> Result<String, String> {
    google_auth.refresh_access_token().await.map_err(|e| {
        format!(
            "Google token refresh failed: {}. Please re-connect in Settings.",
            e
        )
    })?;
    google_auth
        .get_access_token()
        .ok_or_else(|| "Token refresh succeeded but no token available.".to_string())
}

/// macOS `open` dispatches its argument by URL scheme, so an unconstrained
/// `open_file` path is the same outbound channel `open_url` was removed for.
/// Only accept something that is unambiguously a local filesystem path.
fn is_local_file_path(path: &str) -> bool {
    path.starts_with('/') || path.starts_with("~/")
}

/// True when the repository already appears in the local `github_items` table,
/// i.e. the user actually works with it. The model picks `repo` for
/// `create_github_issue` while attacker-authored email text sits in the same
/// context, and the issue is posted with the user's personal token -- an
/// unconstrained destination would be a publish-anywhere outbound channel.
fn github_repo_is_known(db: &crate::db::Database, repo: &str) -> bool {
    if repo.trim().is_empty() {
        return false;
    }
    let conn = match db.conn.lock() {
        Ok(c) => c,
        Err(_) => return false,
    };
    conn.query_row(
        "SELECT 1 FROM github_items WHERE repo = ?1 COLLATE NOCASE LIMIT 1",
        rusqlite::params![repo.trim()],
        |row| row.get::<_, i64>(0),
    )
    .is_ok()
}

/// Execute a tool by name with JSON arguments. Shared by both APIs.
pub async fn execute_tool(
    name: &str,
    args_str: &str,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
) -> String {
    let args: serde_json::Value = serde_json::from_str(args_str).unwrap_or_default();

    let result = match name {
        "open_app" => {
            let app = args["name"].as_str().unwrap_or("");
            crate::system::control::open_app(app).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        // No "open_url" arm: see the removal note in get_tool_definitions().
        "run_command" => {
            let cmd = args["command"].as_str().unwrap_or("");
            crate::system::control::run_command(cmd).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "find_files" => {
            let query = args["query"].as_str().unwrap_or("");
            match crate::system::control::find_files(query, None).await {
                Ok(files) => if files.is_empty() { "No files found.".into() } else { files.join("\n") },
                Err(e) => format!("Error: {}", e),
            }
        }
        "open_file" => {
            let path = args["path"].as_str().unwrap_or("");
            if !is_local_file_path(path) {
                return "Error: open_file takes a full file path starting with / or ~/.".to_string();
            }
            crate::system::control::open_file(path).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "create_task" => {
            let title = args["title"].as_str().unwrap_or("Untitled");
            let desc = args["description"].as_str().unwrap_or("");
            let deadline = args["deadline"].as_str();
            let priority = args["priority"].as_i64().unwrap_or(1) as i32;
            match db.conn.lock() {
                Ok(conn) => {
                    match conn.execute(
                        "INSERT INTO tasks (title, description, deadline, priority) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![title, desc, deadline, priority],
                    ) {
                        Ok(_) => format!("Task created: {}", title),
                        Err(e) => format!("Failed to create task: {}", e),
                    }
                }
                Err(e) => format!("Failed to lock database: {}", e),
            }
        }
        "system_info" => {
            crate::system::control::system_info().await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "send_notification" => {
            let title = args["title"].as_str().unwrap_or("JARVIS");
            let message = args["message"].as_str().unwrap_or("");
            let sound = args["sound"].as_bool().unwrap_or(false);
            crate::system::control::send_notification(title, message, sound).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "search_emails" | "read_email" | "draft_email" | "archive_email" => {
            let token = match google_token_or_err(google_auth, "Gmail").await {
                Ok(t) => t,
                Err(msg) => return msg,
            };
            match execute_gmail_tool(name, &args, &token).await {
                Ok(r) => r,
                Err(e) if e.contains("UNAUTHORIZED") => match refresh_google_token(google_auth).await {
                    Ok(t) => execute_gmail_tool(name, &args, &t).await.unwrap_or_else(|e| format!("Gmail error: {}", e)),
                    Err(e) => e,
                },
                Err(e) => format!("Gmail error: {}", e),
            }
        }
        "list_events" | "create_event" | "update_event" | "delete_event" => {
            let token = match google_token_or_err(google_auth, "Calendar").await {
                Ok(t) => t,
                Err(msg) => return msg,
            };
            match execute_calendar_tool(name, &args, &token, db).await {
                Ok(r) => r,
                Err(e) if e.contains("UNAUTHORIZED") => match refresh_google_token(google_auth).await {
                    Ok(t) => execute_calendar_tool(name, &args, &t, db).await.unwrap_or_else(|e| format!("Calendar error: {}", e)),
                    Err(e) => e,
                },
                Err(e) => format!("Calendar error: {}", e),
            }
        }
        "list_github_items" | "create_github_issue" => {
            match crate::integration_keys::resolve("github_token") {
                // The model picks the destination repo and the body, and the
                // issue is posted with the user's personal token -- so the
                // destination is constrained to repos already in github_items.
                Some(_) if name == "create_github_issue"
                    && !github_repo_is_known(db, args["repo"].as_str().unwrap_or("")) =>
                {
                    "I can only file issues in repositories you already work with. That one hasn't come up in your GitHub activity, so open it yourself if it really is yours.".to_string()
                }
                Some(t) => execute_github_tool(name, &args, &t).await
                    .unwrap_or_else(|e| format!("GitHub error: {}", e)),
                None => "GitHub token not configured. Please add your GitHub token in Settings.".to_string(),
            }
        }
        "search_notion" | "read_notion_page" | "create_notion_page" => {
            match crate::integration_keys::resolve("notion_api_key") {
                Some(t) => execute_notion_tool(name, &args, &t).await
                    .unwrap_or_else(|e| format!("Notion error: {}", e)),
                None => "Notion API key not configured. Please add your Notion API key in Settings.".to_string(),
            }
        }
        "search_notes" | "read_note" => {
            match crate::integration_keys::resolve("obsidian_api_key") {
                Some(k) => {
                    let base_url = crate::integrations::obsidian::resolve_base_url(db);
                    execute_obsidian_tool(name, &args, &k, &base_url).await
                        .unwrap_or_else(|e| format!("Obsidian error: {}", e))
                }
                None => "Obsidian API key not configured. Please add your Obsidian REST API key in Settings.".to_string(),
            }
        }
        "render_chart" => {
            let chart_type = args["chart_type"].as_str().unwrap_or("line");
            let data = &args["data"];
            format!("[CHART:{}|{}]", chart_type, serde_json::to_string(data).unwrap_or_default())
        }
        "render_status" => {
            let status_type = args["status_type"].as_str().unwrap_or("action_completed");
            let data = &args["data"];
            format!("[STATUS:{}|{}]", status_type, serde_json::to_string(data).unwrap_or_default())
        }
        "remember" => execute_memory_tool("remember", &args, db),
        "forget" => execute_memory_tool("forget", &args, db),
        "list_memories" => execute_memory_tool("list_memories", &args, db),
        _ => format!("Unknown tool: {}", name),
    };
    truncate_result(result)
}

pub const SYSTEM_PROMPT_TEMPLATE: &str = "You are JARVIS, a personal AI assistant on macOS for {user}. You speak exactly like JARVIS from Iron Man -- polished, witty, and effortlessly conversational.

CRITICAL -- YOUR OUTPUT IS SPOKEN ALOUD:
Everything you write is read by text-to-speech. You are having a live voice conversation. Write EXACTLY as you would speak. Follow these rules strictly:

Address the user as {user} when it feels natural (\"Right away, {user}\", \"Of course, {user}\"), never as \"sir\" or \"madam\".

Speech style:
- Short, flowing sentences. Never more than one or two clauses per sentence.
- Use contractions: \"I'll\", \"you've\", \"that's\", \"there's\", not \"I will\", \"you have\".
- Sound like a person talking, not a document being read. Think butler, not encyclopedia.
- When listing items, weave them into natural speech: \"You have a standup at 9, then a design review at 11, and lunch with Sarah at noon.\" Never use bullet points or numbered lists.
- Say numbers naturally: \"3 emails\" not \"three (3) emails\". Say times like \"9 AM\" not \"09:00\".
- Never output markdown formatting: no asterisks, no headers, no backticks, no brackets, no dashes as bullets.
- Never read out URLs or file paths. Say \"your YouTube link\" or \"that file\" instead.
- Keep responses concise. Two to four sentences for simple answers. A short paragraph for complex ones.

Before tool calls:
- ALWAYS say something before calling a tool. One short sentence: \"On it.\", \"Let me check.\", \"Right away, {user}.\"
- NEVER call a tool silently.
- When chaining tools, bridge naturally: \"Got it. Now let me also check your calendar.\"

After tool calls:
- Summarize results as spoken conversation, not a report.
- Bad: \"Here are your upcoming events: - 9:00 AM: Standup - 11:00 AM: Design Review\"
- Good: \"You've got two meetings today. Standup at 9, then a design review at 11.\"

When a tool fails:
- Respond conversationally and apologetically. Briefly name what failed, without quoting raw error text.
- Examples: \"Apologies, {user} -- I can't reach Gmail right now.\" / \"That one's not going through, {user}. Looks like Notion isn't connected.\" / \"Hmm, the calendar's unreachable at the moment.\"
- Do NOT dump raw error messages, stack traces, or technical details.

You have tools to open things, read the user's connected accounts, and maintain your own memory of {user}. Use them proactively. You cannot read arbitrary local files, the clipboard, or the screen, and you cannot send mail -- you draft it.

IMPORTANT -- be minimal with tool calls:
- Never call the same tool more than once in a single response unless the user explicitly asked for multiple items.
- Prefer quality over quantity. One precise action beats four scattered ones.

MEMORY:
You have persistent memory about {user}. When they tell you durable facts about themselves (their role, preferences, projects, people they work with, things they always/never want), call the `remember` tool so you recall next time. When they explicitly say \"forget X\" or a stored fact becomes wrong, call `forget`. Use memory naturally -- do not recite all stored facts unprompted. Typical categories: identity (who they are), preference (how they like things done), project (what they are working on), fact (anything else worth remembering).

Capabilities: opening apps, read-only system commands, finding and opening files, notifications, Gmail (search, read, draft, archive), Google Calendar, Notion, GitHub, Obsidian, tasks, persistent memory (remember/forget/list_memories), and data visualization (render inline charts and status cards in chat using render_chart and render_status tools -- use these when presenting data, stats, or confirming completed actions).

You can chain different tools in sequence. Think step by step -- gather information first, then act. Always confirm destructive actions before executing them.

Always attempt tool calls when the user asks about their data. Do not preemptively refuse -- if authentication is missing, the tool will return a clear error. Never say \"I don't have access\" without trying first.";

/// Build system prompt with current user_memory and the current local time
/// injected. Called per request.
pub fn build_system_prompt(db: &crate::db::Database) -> String {
    let base = system_prompt_for(&crate::util::display_name(db));
    let memory_block = load_memory_block(db);
    let time_line = current_time_line(chrono::Local::now());
    if memory_block.is_empty() {
        format!("{}\n\n{}", base, time_line)
    } else {
        format!("{}\n\n{}\n{}", base, memory_block, time_line)
    }
}

/// Render the base system prompt for a specific user. Kept separate from
/// `build_system_prompt` so it is testable without a database.
pub fn system_prompt_for(name: &str) -> String {
    SYSTEM_PROMPT_TEMPLATE.replace("{user}", name)
}

/// One line telling the model the wall-clock time and UTC offset to resolve
/// relative dates against. Changes every request, so it must never be part
/// of the cacheable static prompt.
pub fn current_time_line(now: chrono::DateTime<chrono::Local>) -> String {
    format!(
        "Current local time: {} ({}, UTC offset {}). Resolve relative dates such as today, \
         tomorrow and next week against this, and always send RFC 3339 times with this \
         offset to create_event and update_event.",
        now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        now.format("%A"),
        now.format("%:z"),
    )
}

/// Attach `offset` to a naive `YYYY-MM-DDTHH:MM[:SS]` value so Google does not
/// read it as UTC. A value that already carries `Z` or an `+hh:mm`/`-hh:mm`
/// offset, a date-only value, or anything else is returned unchanged.
pub fn with_local_offset(value: &str, offset: chrono::FixedOffset) -> String {
    use chrono::TimeZone;
    let naive = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M"));
    match naive.ok().and_then(|n| offset.from_local_datetime(&n).single()) {
        Some(dt) => dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        None => value.to_string(),
    }
}

fn load_memory_block(db: &crate::db::Database) -> String {
    let conn = match db.conn.lock() {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let mut stmt =
        match conn.prepare("SELECT category, content FROM user_memory ORDER BY category, id") {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
    let rows = match stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) {
        Ok(r) => r,
        Err(_) => return String::new(),
    };
    let entries: Vec<(String, String)> = rows.filter_map(|r| r.ok()).collect();
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from("KNOWN FACTS ABOUT THE USER (your persistent memory):\n");
    for (cat, content) in entries {
        out.push_str(&format!("- [{}] {}\n", cat, content));
    }
    out
}

/// Returns the static portion of the system prompt that should be cached.
/// Must be byte-stable across requests for cache hits.
pub fn build_cacheable_system_prompt() -> String {
    include_str!("system_prompt_static.md").to_string()
}

/// Returns the per-request dynamic suffix appended (as a separate SystemContentBlock
/// AFTER the cachePoint) so it doesn't break the cached prefix.
/// Includes user_name, the current local time and offset, language preference,
/// and any persisted memory.
pub fn build_dynamic_suffix(
    user_name: &str,
    now: chrono::DateTime<chrono::Local>,
    language: &str, // "en" | "yue" | "bilingual" -- used by B4
    db: &crate::db::Database,
) -> String {
    let header = format!(
        "Current user: {user_name}\n{time_line}\nResponse language: {language}\n",
        time_line = current_time_line(now),
    );
    let memory_block = load_memory_block(db);
    if memory_block.is_empty() {
        header
    } else {
        format!("{}\n{}", header, memory_block)
    }
}

#[cfg(test)]
mod injection_tests {
    use super::*;

    // The model reads attacker-controlled text (`read_email`). If it also holds
    // a way to read local secrets or to send data out, one crafted email is a
    // complete exfiltration chain -- and a sentence in the system prompt is not
    // an access control. A name denylist cannot enforce that on its own: it
    // stayed green while `open_url` sat in the registry next to `read_email`.
    // So the surface is checked as a property over the live registry, with
    // every tool classified by capability below.

    /// Capability classification for one live tool: `(reads_private, emits_outbound)`.
    ///
    /// - `reads_private`: returns local or private user data into the model's context.
    /// - `emits_outbound`: hands model-authored content to a destination off this device.
    ///
    /// Exhaustive by name on purpose. `None` means "this test has never heard of
    /// that tool", which fails the test -- adding a tool to
    /// `get_tool_definitions` without classifying it here is not allowed to pass
    /// silently.
    fn capabilities(name: &str) -> Option<(bool, bool)> {
        Some(match name {
            // Local actions: no private data in, nothing off-device out.
            "open_app" => (false, false),
            // `open` dispatches by URL scheme, so the dispatch arm rejects
            // anything that is not a local path (see `is_local_file_path`).
            "open_file" => (false, false),
            "create_task" => (false, false),
            "send_notification" => (false, false),
            "render_chart" => (false, false),
            "render_status" => (false, false),
            "remember" => (false, false),
            "forget" => (false, false),
            "archive_email" => (false, false),
            "delete_event" => (false, false),

            // Reads private or local data into context.
            // `run_command` is shell-shaped but argv-only against a read-only
            // allowlist in system/control.rs -- no network client is on it.
            "run_command" => (true, false),
            "find_files" => (true, false),
            "system_info" => (true, false),
            "search_emails" => (true, false),
            "read_email" => (true, false),
            "list_events" => (true, false),
            "search_notion" => (true, false),
            "read_notion_page" => (true, false),
            "list_github_items" => (true, false),
            "search_notes" => (true, false),
            "read_note" => (true, false),
            "list_memories" => (true, false),

            // Emits outbound. Every one of these must be justified in
            // ALLOWED_OUTBOUND below.
            "draft_email" => (false, true),
            "create_event" => (false, true),
            "update_event" => (false, true),
            "create_notion_page" => (false, true),
            "create_github_issue" => (false, true),

            _ => return None,
        })
    }

    /// Outbound tools that are allowed to ship, each with the constraint that
    /// makes it safe. Anything classified `emits_outbound` and missing from
    /// this list fails the test, which forces the constraint to be written down
    /// before the tool ships.
    const ALLOWED_OUTBOUND: &[(&str, &str)] = &[
        // Writes a Gmail draft the user reviews and sends by hand. Nothing
        // leaves the account on its own -- gmail::send_message has no callers.
        ("draft_email", "draft only, never sent"),
        // Fixed destination: the user's own primary calendar. The `attendees`
        // property was removed so the model cannot copy the event body onto
        // anyone else's calendar.
        ("create_event", "no attendees, lands only on the user's own calendar"),
        // Same fixed destination, and it can only touch an event that already
        // exists on that calendar.
        ("update_event", "edits an existing event on the user's own calendar"),
        // The Notion token only reaches pages the user explicitly shared with
        // the integration, so the destination stays inside their workspace.
        ("create_notion_page", "confined to the user's own Notion workspace"),
        // The destination repo is validated against the local github_items
        // table, so it must be one the user already works with.
        ("create_github_issue", "repo must already appear in github_items"),
    ];

    /// The named tools that were removed on 2026-07-26 must stay removed. This
    /// is the narrow half of the guard -- it catches a re-add under the same
    /// name, and nothing else.
    #[test]
    fn no_tool_can_read_local_data_or_send_mail() {
        let names: Vec<String> = get_tool_definitions()
            .iter()
            .map(|t| t.name.clone())
            .collect();

        for banned in [
            "read_file",
            "clipboard_read",
            "clipboard_write",
            "write_note",
            "screenshot",
            "kill_process",
            "list_processes",
            "manage_window",
            "system_controls",
            "send_email",
        ] {
            assert!(
                !names.iter().any(|n| n == banned),
                "{banned} re-opens the prompt-injection exfiltration chain"
            );
        }

        assert!(
            names.iter().any(|n| n == "draft_email"),
            "the compose path must still exist, as a draft"
        );
    }

    #[test]
    fn every_live_tool_is_classified_and_no_tool_is_both_halves_of_the_chain() {
        let mut reads_private: Vec<String> = Vec::new();
        let mut emits_outbound: Vec<String> = Vec::new();

        for tool in get_tool_definitions() {
            let (reads, emits) = capabilities(&tool.name).unwrap_or_else(|| {
                panic!(
                    "{} is in the registry but not classified in injection_tests::capabilities -- \
                     classify it before it ships",
                    tool.name
                )
            });
            assert!(
                !(reads && emits),
                "{} both reads private data and emits it outbound -- that is the whole chain in one tool",
                tool.name
            );
            if reads {
                reads_private.push(tool.name.clone());
            }
            if emits {
                emits_outbound.push(tool.name.clone());
            }
        }

        // Both halves exist in the same context, so every outbound tool has to
        // carry a written constraint.
        if !reads_private.is_empty() && !emits_outbound.is_empty() {
            for name in &emits_outbound {
                assert!(
                    ALLOWED_OUTBOUND
                        .iter()
                        .any(|(allowed, _)| *allowed == name.as_str()),
                    "{name} emits data outbound and is not on ALLOWED_OUTBOUND -- add it with the \
                     constraint that makes it safe, or take the tool out of the registry"
                );
            }
        }

        // The classification must not rot in the other direction either.
        let live: Vec<String> = get_tool_definitions().iter().map(|t| t.name.clone()).collect();
        for (allowed, _) in ALLOWED_OUTBOUND {
            assert!(
                live.iter().any(|n| n.as_str() == *allowed),
                "{allowed} is on ALLOWED_OUTBOUND but no longer exists -- drop the stale entry"
            );
        }
    }

    #[test]
    fn open_url_stays_out_of_the_registry() {
        let names: Vec<String> = get_tool_definitions()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert!(
            !names.iter().any(|n| n == "open_url"),
            "open_url hands a model-controlled string to macOS `open`; a URL query string is a raw \
             outbound channel"
        );
    }

    #[test]
    fn create_event_cannot_invite_anyone() {
        let tool = get_tool_definitions()
            .into_iter()
            .find(|t| t.name == "create_event")
            .expect("create_event must still exist");
        assert!(
            tool.parameters["properties"]["attendees"].is_null(),
            "an invite copies the event body onto the attendee's own calendar"
        );
    }

    #[test]
    fn open_file_rejects_urls() {
        assert!(!is_local_file_path("https://collect.attacker.tld/c?d=stolen"));
        assert!(!is_local_file_path("mailto:attacker@example.com?body=stolen"));
        assert!(!is_local_file_path("slack://open?team=stolen"));
        assert!(is_local_file_path("/Users/ada/Documents/report.pdf"));
        assert!(is_local_file_path("~/Downloads/invoice.pdf"));
    }
}

#[cfg(test)]
mod prompt_tests {
    use super::*;

    #[test]
    fn system_prompt_addresses_the_actual_user() {
        let prompt = system_prompt_for("Ada");
        assert!(prompt.contains("Ada"), "prompt must address the signed-in user");
        assert!(
            !prompt.contains("Hillman"),
            "prompt must not hardcode the developer's name"
        );
        assert!(
            !prompt.contains("{user}"),
            "every placeholder must be substituted"
        );
    }

    #[test]
    fn cacheable_prompt_carries_no_hardcoded_identity() {
        let prompt = build_cacheable_system_prompt();
        assert!(!prompt.contains("Hillman"));
    }

    #[test]
    fn cacheable_prompt_is_byte_stable() {
        let a = build_cacheable_system_prompt();
        let b = build_cacheable_system_prompt();
        assert_eq!(a, b, "cacheable prompt must be byte-stable for cache hits");
        assert!(a.len() > 500, "expected non-trivial static prompt");
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;
    use crate::db::Database;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn memory_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE user_memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT 'fact',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .unwrap();
        Database { conn: Mutex::new(conn) }
    }

    fn count(db: &Database) -> i64 {
        db.conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM user_memory", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn remember_deduplicates_identical_facts() {
        let db = memory_db();
        let args = serde_json::json!({ "content": "Hillman 揸緊 JARVIS", "category": "project" });
        execute_memory_tool("remember", &args, &db);
        execute_memory_tool("remember", &args, &db); // exact repeat
        // Case-insensitive repeat with surrounding whitespace should also dedupe.
        let args2 = serde_json::json!({ "content": "  hillman 揸緊 JARVIS  " });
        execute_memory_tool("remember", &args2, &db);
        assert_eq!(count(&db), 1, "identical facts must not be stored twice");
    }

    #[test]
    fn remember_stores_distinct_facts() {
        let db = memory_db();
        execute_memory_tool("remember", &serde_json::json!({ "content": "fact A" }), &db);
        execute_memory_tool("remember", &serde_json::json!({ "content": "fact B" }), &db);
        assert_eq!(count(&db), 2);
    }
}

#[cfg(test)]
mod github_scope_tests {
    use super::*;
    use crate::db::Database;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn github_db(repos: &[&str]) -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE github_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                github_id INTEGER NOT NULL,
                item_type TEXT NOT NULL,
                title TEXT NOT NULL,
                repo TEXT NOT NULL,
                state TEXT NOT NULL
            );",
        )
        .unwrap();
        for (i, repo) in repos.iter().enumerate() {
            conn.execute(
                "INSERT INTO github_items (github_id, item_type, title, repo, state)
                 VALUES (?1, 'issues', 'seen', ?2, 'open')",
                rusqlite::params![i as i64, *repo],
            )
            .unwrap();
        }
        Database { conn: Mutex::new(conn) }
    }

    #[test]
    fn injected_repo_is_refused() {
        // The destination an injected email would name has never appeared in
        // the user's GitHub activity, so it must not be publishable.
        let db = github_db(&["ChiFungHillmanChan/jarvis-ai-assistant"]);
        assert!(!github_repo_is_known(&db, "attacker-acct/inbox-log"));
        assert!(!github_repo_is_known(&db, ""));
        assert!(!github_repo_is_known(&db, "   "));
    }

    #[test]
    fn repo_the_user_already_works_with_is_allowed() {
        let db = github_db(&["ChiFungHillmanChan/jarvis-ai-assistant"]);
        assert!(github_repo_is_known(
            &db,
            "ChiFungHillmanChan/jarvis-ai-assistant"
        ));
        // Same repo, model-supplied casing and stray whitespace.
        assert!(github_repo_is_known(
            &db,
            "  chifunghillmanchan/JARVIS-AI-ASSISTANT  "
        ));
    }
}

#[cfg(test)]
mod local_time_tests {
    use super::*;
    use crate::db::Database;
    use chrono::{DateTime, FixedOffset, Local, NaiveDate};
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn empty_db() -> Database {
        Database { conn: Mutex::new(Connection::open_in_memory().unwrap()) }
    }

    fn hong_kong() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    /// 2026-09-05T00:30:12+08:00, a Saturday in Hong Kong while it is still
    /// Friday in UTC -- so the weekday only matches if it is taken from local time.
    fn fixed_now() -> DateTime<Local> {
        let naive_utc = NaiveDate::from_ymd_opt(2026, 9, 4)
            .unwrap()
            .and_hms_opt(16, 30, 12)
            .unwrap();
        DateTime::<Local>::from_naive_utc_and_offset(naive_utc, hong_kong())
    }

    #[test]
    fn current_time_line_carries_offset_and_weekday() {
        let line = current_time_line(fixed_now());
        assert!(
            line.starts_with("Current local time: 2026-09-05T00:30:12+08:00"),
            "got: {line}"
        );
        assert!(line.contains("Saturday"), "weekday must be the local one, got: {line}");
        assert!(line.contains("UTC offset +08:00"), "got: {line}");
        assert!(line.contains("create_event"), "got: {line}");
        assert!(line.contains("update_event"), "got: {line}");
        assert!(!line.contains('\n'), "must stay a single line, got: {line}");
    }

    #[test]
    fn with_local_offset_appends_only_when_missing() {
        let hk = hong_kong();
        assert_eq!(
            with_local_offset("2026-09-05T10:00:00", hk),
            "2026-09-05T10:00:00+08:00"
        );
        assert_eq!(
            with_local_offset("2026-09-05T10:00", hk),
            "2026-09-05T10:00:00+08:00"
        );
        assert_eq!(
            with_local_offset("2026-09-05T02:00:00Z", hk),
            "2026-09-05T02:00:00Z"
        );
        assert_eq!(
            with_local_offset("2026-09-05T10:00:00+01:00", hk),
            "2026-09-05T10:00:00+01:00"
        );
        assert_eq!(
            with_local_offset("2026-09-05T10:00:00-05:00", hk),
            "2026-09-05T10:00:00-05:00"
        );
        assert_eq!(with_local_offset("2026-09-05", hk), "2026-09-05");
        assert_eq!(with_local_offset("", hk), "");
    }

    #[test]
    fn system_prompt_includes_current_time() {
        let prompt = build_system_prompt(&empty_db());
        assert!(
            prompt.contains("Current local time:"),
            "every non-Bedrock provider reads this prompt, so the time must be in it"
        );
        assert!(prompt.contains("UTC offset "), "got tail: {}", &prompt[prompt.len() - 200..]);
    }

    #[test]
    fn dynamic_suffix_carries_current_time_line() {
        let suffix = build_dynamic_suffix("Ada", fixed_now(), "en", &empty_db());
        assert!(suffix.contains("Current user: Ada"), "got: {suffix}");
        assert!(
            suffix.contains("Current local time: 2026-09-05T00:30:12+08:00"),
            "got: {suffix}"
        );
        assert!(!suffix.contains("Today:"), "the bare date line is replaced, got: {suffix}");
    }
}
