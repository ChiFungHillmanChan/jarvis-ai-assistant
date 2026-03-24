use serde_json::json;

/// Shared tool definitions used by both OpenAI and Claude APIs
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
        Tool {
            name: "open_url".into(),
            description: "Open a URL in the default browser. Construct full URLs with search queries when needed.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "url": { "type": "string", "description": "Full URL to open" } },
                "required": ["url"]
            }),
        },
        Tool {
            name: "run_command".into(),
            description: "Run a shell command on macOS and return the output. Use for system info, file operations, git commands, etc. Do NOT run destructive commands.".into(),
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
            name: "write_note".into(),
            description: "Write or append to a text file (for notes, logs, etc)".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path (use ~ for home)" },
                    "content": { "type": "string" },
                    "append": { "type": "boolean", "description": "true to append, false to overwrite" }
                },
                "required": ["path", "content"]
            }),
        },
        Tool {
            name: "system_info".into(),
            description: "Get system information: hostname, uptime, disk usage, memory".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "clipboard_read".into(),
            description: "Read the current contents of the macOS clipboard".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        Tool {
            name: "clipboard_write".into(),
            description: "Write text content to the macOS clipboard".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "Text to copy to clipboard" }
                },
                "required": ["content"]
            }),
        },
        Tool {
            name: "screenshot".into(),
            description: "Take a screenshot on macOS. Region can be 'fullscreen', 'selection', or 'window'.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "region": { "type": "string", "description": "Screenshot region: fullscreen (default), selection, or window" }
                }
            }),
        },
        Tool {
            name: "manage_window".into(),
            description: "Manage application windows: list all visible windows, focus an app, resize or move a window.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Action: list, focus, resize, move" },
                    "app_name": { "type": "string", "description": "Application name (required for focus/resize/move)" },
                    "width": { "type": "integer", "description": "Window width in pixels (for resize)" },
                    "height": { "type": "integer", "description": "Window height in pixels (for resize)" },
                    "x": { "type": "integer", "description": "X position in pixels (for move)" },
                    "y": { "type": "integer", "description": "Y position in pixels (for move)" }
                },
                "required": ["action"]
            }),
        },
        Tool {
            name: "system_controls".into(),
            description: "Control system settings: volume_set, volume_get, volume_mute, volume_unmute, brightness_set, dark_mode_on, dark_mode_off, dark_mode_toggle".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Control action: volume_set, volume_get, volume_mute, volume_unmute, brightness_set, dark_mode_on, dark_mode_off, dark_mode_toggle" },
                    "value": { "type": "integer", "description": "Value for volume_set (0-100) or brightness_set (0-100)" }
                },
                "required": ["action"]
            }),
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
            name: "list_processes".into(),
            description: "List running processes on the system with optional name filter".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filter": { "type": "string", "description": "Optional process name filter" }
                }
            }),
        },
        Tool {
            name: "kill_process".into(),
            description: "Kill a running process by PID. Refuses to kill system-critical processes.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pid": { "type": "integer", "description": "Process ID to kill" }
                },
                "required": ["pid"]
            }),
        },
        Tool {
            name: "read_file".into(),
            description: "Read a text file and return its contents. Max 100KB, refuses binary files.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path (use ~ for home directory)" },
                    "max_lines": { "type": "integer", "description": "Maximum lines to return (default 200)" }
                },
                "required": ["path"]
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
            name: "send_email".into(),
            description: "Send an email via Gmail. Composes and sends immediately.".into(),
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
        Tool {
            name: "create_event".into(),
            description: "Create a new Google Calendar event. Times should be in ISO 8601 / RFC 3339 format (e.g. 2026-03-25T10:00:00+00:00).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Event title/summary" },
                    "start": { "type": "string", "description": "Start time in RFC 3339 format" },
                    "end": { "type": "string", "description": "End time in RFC 3339 format" },
                    "location": { "type": "string", "description": "Event location (optional)" },
                    "description": { "type": "string", "description": "Event description (optional)" },
                    "attendees": { "type": "string", "description": "Comma-separated email addresses of attendees (optional)" }
                },
                "required": ["title", "start", "end"]
            }),
        },
        Tool {
            name: "update_event".into(),
            description: "Update an existing Google Calendar event. Only provided fields will be changed.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "event_id": { "type": "string", "description": "Google Calendar event ID" },
                    "title": { "type": "string", "description": "New event title/summary" },
                    "start": { "type": "string", "description": "New start time in RFC 3339 format" },
                    "end": { "type": "string", "description": "New end time in RFC 3339 format" },
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
            description: "Create a new GitHub issue in a repository.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "Repository in 'owner/repo' format" },
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
    ]
}

pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Format tools for OpenAI API format
pub fn to_openai_format(tools: &[Tool]) -> Vec<serde_json::Value> {
    tools.iter().map(|t| json!({
        "type": "function",
        "function": {
            "name": t.name,
            "description": t.description,
            "parameters": t.parameters,
        }
    })).collect()
}

/// Format tools for Claude API format
pub fn to_claude_format(tools: &[Tool]) -> Vec<serde_json::Value> {
    tools.iter().map(|t| json!({
        "name": t.name,
        "description": t.description,
        "input_schema": t.parameters,
    })).collect()
}

/// Map tool names to user-friendly status labels for streaming UI.
pub fn tool_status_label(name: &str) -> &'static str {
    match name {
        "search_emails" => "Searching emails...",
        "read_email" => "Reading email...",
        "send_email" => "Sending email...",
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
        "open_url" => "Opening URL...",
        "run_command" => "Running command...",
        "find_files" => "Finding files...",
        "open_file" => "Opening file...",
        "create_task" => "Creating task...",
        "write_note" => "Writing note...",
        "system_info" => "Checking system...",
        "clipboard_read" => "Reading clipboard...",
        "clipboard_write" => "Copying to clipboard...",
        "screenshot" => "Taking screenshot...",
        "manage_window" => "Managing windows...",
        "system_controls" => "Adjusting system...",
        "send_notification" => "Sending notification...",
        "list_processes" => "Listing processes...",
        "kill_process" => "Stopping process...",
        "read_file" => "Reading file...",
        _ => "Processing...",
    }
}

/// Truncate tool results to avoid blowing up context windows.
fn truncate_result(s: String) -> String {
    if s.len() > 4000 {
        format!("{}... [truncated, {} total chars]", &s[..4000], s.len())
    } else {
        s
    }
}

async fn execute_gmail_tool(
    name: &str,
    args: &serde_json::Value,
    token: &str,
    db: &crate::db::Database,
) -> Result<String, String> {
    use crate::integrations::gmail;
    match name {
        "search_emails" => {
            let query = args["query"].as_str().unwrap_or("");
            let messages = gmail::search_messages(token, query).await?;
            if messages.is_empty() {
                return Ok("No emails found matching that query.".to_string());
            }
            let lines: Vec<String> = messages.iter().map(|m| {
                format!(
                    "ID: {} | From: {} | Subject: {} | Date: {}",
                    m.id,
                    m.sender.as_deref().unwrap_or("unknown"),
                    m.subject.as_deref().unwrap_or("(no subject)"),
                    m.received_at.as_deref().unwrap_or("unknown"),
                )
            }).collect();
            Ok(lines.join("\n"))
        }
        "read_email" => {
            let email_id = args["email_id"].as_str().unwrap_or("");
            let (_msg, full_text) = gmail::get_message_full(token, email_id).await?;
            Ok(full_text)
        }
        "send_email" => {
            let to = args["to"].as_str().unwrap_or("");
            let subject = args["subject"].as_str().unwrap_or("");
            let body = args["body"].as_str().unwrap_or("");
            let cc = args["cc"].as_str();

            // Log outgoing email to DB before sending
            let now = chrono::Local::now().to_rfc3339();
            let outgoing_id = format!("outgoing_{}", now);
            let snippet = if body.len() > 100 { &body[..100] } else { body };
            if let Ok(conn) = db.conn.lock() {
                let _ = conn.execute(
                    "INSERT INTO emails (gmail_id, subject, sender, snippet, labels, is_read, received_at) VALUES (?1, ?2, 'me (outgoing)', ?3, 'SENT', 1, ?4)",
                    rusqlite::params![outgoing_id, subject, snippet, now],
                );
            }

            let msg_id = gmail::send_message(token, to, subject, body, cc).await?;
            Ok(format!("Email sent successfully (ID: {})", msg_id))
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
) -> Result<String, String> {
    use crate::integrations::calendar;
    match name {
        "list_events" => {
            let now = chrono::Local::now();
            let tz_offset = now.format("%:z").to_string();
            let date_from = args["date_from"].as_str().map(|s| s.to_string()).unwrap_or_else(|| {
                now.format("%Y-%m-%d").to_string()
            });
            let date_to = args["date_to"].as_str().map(|s| s.to_string()).unwrap_or_else(|| {
                (now + chrono::TimeDelta::days(7)).format("%Y-%m-%d").to_string()
            });
            let time_min = format!("{}T00:00:00{}", date_from, tz_offset);
            let time_max = format!("{}T23:59:59{}", date_to, tz_offset);
            let events = calendar::fetch_events(token, &time_min, &time_max).await?;
            if events.is_empty() {
                return Ok("No events found in the specified date range.".to_string());
            }
            let lines: Vec<String> = events.iter().map(|e| {
                let loc = e.location.as_deref().unwrap_or("none");
                format!(
                    "ID: {} | {} | {} - {} | Location: {}",
                    e.id, e.summary, e.start_time, e.end_time, loc,
                )
            }).collect();
            Ok(lines.join("\n"))
        }
        "create_event" => {
            let title = args["title"].as_str().unwrap_or("");
            let start = args["start"].as_str().unwrap_or("");
            let end = args["end"].as_str().unwrap_or("");
            let location = args["location"].as_str();
            let description = args["description"].as_str();
            let attendees = args["attendees"].as_str();
            let result = calendar::create_event(token, title, start, end, description, location, attendees).await?;
            Ok(format!("Event created: {}", result))
        }
        "update_event" => {
            let event_id = args["event_id"].as_str().unwrap_or("");
            let title = args["title"].as_str();
            let start = args["start"].as_str();
            let end = args["end"].as_str();
            let location = args["location"].as_str();
            let description = args["description"].as_str();
            let result = calendar::update_event(token, event_id, title, start, end, location, description).await?;
            Ok(format!("Event updated: {}", result))
        }
        "delete_event" => {
            let event_id = args["event_id"].as_str().unwrap_or("");
            calendar::delete_event(token, event_id).await
        }
        _ => Err(format!("Unknown Calendar tool: {}", name)),
    }
}

async fn execute_obsidian_tool(
    name: &str,
    args: &serde_json::Value,
    api_key: &str,
) -> Result<String, String> {
    use crate::integrations::obsidian;
    match name {
        "search_notes" => {
            let query = args["query"].as_str().unwrap_or("");
            let notes = obsidian::search_vault(api_key, query).await?;
            if notes.is_empty() {
                return Ok("No notes found matching that query.".to_string());
            }
            let lines: Vec<String> = notes.iter().map(|n| {
                let snippet = n.content.as_deref().unwrap_or("");
                let truncated = if snippet.len() > 100 { &snippet[..100] } else { snippet };
                format!("{} | {}", n.path, truncated)
            }).collect();
            Ok(lines.join("\n"))
        }
        "read_note" => {
            let path = args["path"].as_str().unwrap_or("");
            obsidian::get_note(api_key, path).await
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
            let lines: Vec<String> = items.iter().map(|i| {
                format!(
                    "[{}] {} - {} ({}) | {}",
                    i.item_type,
                    i.repo,
                    i.title,
                    i.state,
                    i.url.as_deref().unwrap_or("none"),
                )
            }).collect();
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
            let pages = notion::search_pages(token, Some(query)).await?;
            if pages.is_empty() {
                return Ok("No Notion pages found matching that query.".to_string());
            }
            let lines: Vec<String> = pages.iter().map(|p| {
                format!(
                    "ID: {} | {} | Edited: {} | URL: {}",
                    p.notion_id,
                    p.title,
                    p.last_edited.as_deref().unwrap_or("unknown"),
                    p.url.as_deref().unwrap_or("none"),
                )
            }).collect();
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

fn google_token_or_err(
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    service: &str,
) -> Result<String, String> {
    google_auth.get_access_token()
        .ok_or_else(|| format!("Google account not connected. Please connect in Settings to use {} tools.", service))
}

async fn refresh_google_token(
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
) -> Result<String, String> {
    google_auth.refresh_access_token().await
        .map_err(|e| format!("Google token refresh failed: {}. Please re-connect in Settings.", e))?;
    google_auth.get_access_token()
        .ok_or_else(|| "Token refresh succeeded but no token available.".to_string())
}

fn get_preference(db: &crate::db::Database, key: &str) -> Option<String> {
    db.conn.lock().ok()?.query_row(
        "SELECT value FROM user_preferences WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    ).ok()
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
        "open_url" => {
            let url = args["url"].as_str().unwrap_or("");
            crate::system::control::open_url(url).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
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
        "write_note" => {
            let path = args["path"].as_str().unwrap_or("~/jarvis-notes.md");
            let content = args["content"].as_str().unwrap_or("");
            let append = args["append"].as_bool().unwrap_or(true);
            crate::system::control::write_note(path, content, append).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "system_info" => {
            crate::system::control::system_info().await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "clipboard_read" => {
            crate::system::control::clipboard_read().await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "clipboard_write" => {
            let content = args["content"].as_str().unwrap_or("");
            crate::system::control::clipboard_write(content).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "screenshot" => {
            let region = args["region"].as_str().unwrap_or("fullscreen");
            crate::system::control::screenshot(region).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "manage_window" => {
            let action = args["action"].as_str().unwrap_or("list");
            let app_name = args["app_name"].as_str();
            let width = args["width"].as_i64();
            let height = args["height"].as_i64();
            let x = args["x"].as_i64();
            let y = args["y"].as_i64();
            crate::system::control::manage_window(action, app_name, width, height, x, y).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "system_controls" => {
            let action = args["action"].as_str().unwrap_or("");
            let value = args["value"].as_i64();
            crate::system::control::system_controls(action, value).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "send_notification" => {
            let title = args["title"].as_str().unwrap_or("JARVIS");
            let message = args["message"].as_str().unwrap_or("");
            let sound = args["sound"].as_bool().unwrap_or(false);
            crate::system::control::send_notification(title, message, sound).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "list_processes" => {
            let filter = args["filter"].as_str();
            crate::system::control::list_processes(filter).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "kill_process" => {
            let pid = args["pid"].as_i64().unwrap_or(0);
            crate::system::control::kill_process(pid).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "read_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let max_lines = args["max_lines"].as_i64().map(|v| v as usize);
            crate::system::control::read_file(path, max_lines).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "search_emails" | "read_email" | "send_email" | "archive_email" => {
            let token = match google_token_or_err(google_auth, "Gmail") {
                Ok(t) => t,
                Err(msg) => return msg,
            };
            match execute_gmail_tool(name, &args, &token, db).await {
                Ok(r) => r,
                Err(e) if e.contains("UNAUTHORIZED") => match refresh_google_token(google_auth).await {
                    Ok(t) => execute_gmail_tool(name, &args, &t, db).await.unwrap_or_else(|e| format!("Gmail error: {}", e)),
                    Err(e) => e,
                },
                Err(e) => format!("Gmail error: {}", e),
            }
        }
        "list_events" | "create_event" | "update_event" | "delete_event" => {
            let token = match google_token_or_err(google_auth, "Calendar") {
                Ok(t) => t,
                Err(msg) => return msg,
            };
            match execute_calendar_tool(name, &args, &token).await {
                Ok(r) => r,
                Err(e) if e.contains("UNAUTHORIZED") => match refresh_google_token(google_auth).await {
                    Ok(t) => execute_calendar_tool(name, &args, &t).await.unwrap_or_else(|e| format!("Calendar error: {}", e)),
                    Err(e) => e,
                },
                Err(e) => format!("Calendar error: {}", e),
            }
        }
        "list_github_items" | "create_github_issue" => {
            match get_preference(db, "github_token") {
                Some(t) => execute_github_tool(name, &args, &t).await
                    .unwrap_or_else(|e| format!("GitHub error: {}", e)),
                None => "GitHub token not configured. Please add your GitHub token in Settings.".to_string(),
            }
        }
        "search_notion" | "read_notion_page" | "create_notion_page" => {
            match get_preference(db, "notion_api_key") {
                Some(t) => execute_notion_tool(name, &args, &t).await
                    .unwrap_or_else(|e| format!("Notion error: {}", e)),
                None => "Notion API key not configured. Please add your Notion API key in Settings.".to_string(),
            }
        }
        "search_notes" | "read_note" => {
            match get_preference(db, "obsidian_api_key") {
                Some(k) => execute_obsidian_tool(name, &args, &k).await
                    .unwrap_or_else(|e| format!("Obsidian error: {}", e)),
                None => "Obsidian API key not configured. Please add your Obsidian REST API key in Settings.".to_string(),
            }
        }
        _ => format!("Unknown tool: {}", name),
    };
    truncate_result(result)
}

pub const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant on macOS for Hillman Chan (GitHub: ChiFungHillmanChan). Be concise and direct like the JARVIS from Iron Man.

You have 32 tools to control the computer and manage integrations. Use them proactively when the user asks you to do something.

Capabilities:
- System control: open apps, URLs, files, run commands, clipboard, screenshots, window management, volume/brightness, notifications, process management
- Gmail: search, read, send, and archive emails
- Google Calendar: list, create, update, and delete events
- Notion: search, read, and create pages
- GitHub: list PRs/issues, create issues
- Obsidian: search and read notes
- Tasks: create tasks and reminders
- File I/O: read file contents, write notes

You can chain multiple tools in sequence. Think step by step -- gather information first, then act. Always confirm destructive actions in your response text before executing them.

Always attempt tool calls when the user asks about their data (emails, calendar, notes, etc). Do not preemptively refuse -- if authentication is missing, the tool will return a clear error message that guides the user. Never say \"I don't have access\" without first trying the tool.";
