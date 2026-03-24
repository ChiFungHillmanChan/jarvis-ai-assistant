# JARVIS Function Calling Upgrade

**Date:** 2026-03-24
**Status:** Approved
**Scope:** Upgrade AI models, expand function tools, add integration + system tools

---

## 1. Overview

Upgrade JARVIS to use GPT-5 as the primary AI provider with Claude Sonnet 4.6 as fallback. Expand the tool system from 8 basic tools to 32 tools covering integrations (Gmail, Calendar, Notion, GitHub, Obsidian) and system control (clipboard, screenshots, window management, notifications, processes).

## 2. Model Changes

| Provider | Current Model | New Model |
|----------|--------------|-----------|
| OpenAI (primary) | `gpt-4o-mini` | `gpt-5` |
| Claude (fallback) | `claude-sonnet-4-20250514` | `claude-sonnet-4-6-20250610` |

- Default provider changes from `ClaudePrimary` to `OpenAIPrimary` in `AiRouter::new()`
- User can still override in Settings
- Both providers continue to use native function calling (tool use) with 5-iteration agentic loop

## 3. Architecture Change: Stateful Tool Execution

### Problem

Current `execute_tool()` is stateless -- it has no access to the Tauri-managed Database, GoogleAuth, or API tokens. The `create_task` tool works around this by shelling out to `sqlite3`, which is fragile and has SQL injection risk.

### Solution

Expand `execute_tool()` signature to accept shared state:

```rust
pub async fn execute_tool(
    name: &str,
    args_str: &str,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::GoogleAuth>,
) -> String
```

- Both `claude.rs` and `openai.rs` pass state through from their Tauri command context
- The `send()` functions on both providers gain `db` and `google_auth` parameters
- `AiRouter::send()` also gains these parameters to pass through
- `chat.rs` already has access to both via Tauri `State<>` -- passes them into `router.send()`
- `commands/assistant.rs` (`ask_jarvis`, `get_briefing`, `speak_briefing`) and `assistant/briefing.rs` (`generate_briefing`) also updated to pass `db`/`google_auth` through to `router.send()`
- Integration tools call Rust functions in `src-tauri/src/integrations/` (some existing, some new -- see Section 4.6)
- System tools call functions in `src-tauri/src/system/control.rs`
- `create_task` tool switches from `sqlite3` shell command to direct `db.conn` SQL execution
- `max_tokens` increased from 1024 to 4096 in both `claude.rs` and `openai.rs` to support 30-tool agentic loops

### Credential Access

- Google OAuth tokens: via `Arc<GoogleAuth>` state (always present, may be unconfigured)
- Token refresh: integration tools attempt `google_auth.refresh_access_token().await` on 401 responses before retrying once
- Notion token: read from `user_preferences` table in DB
- GitHub token: read from `user_preferences` table in DB
- Obsidian vault path: read from `user_preferences` table in DB

### Tool Result Truncation

Tool results are truncated to 4000 characters before being fed back into the AI context. This prevents a single `search_emails` or `list_github_items` call from consuming the entire context window. The truncation appends `... [truncated, showing first 4000 chars]` when applied.

## 4. New Integration Tools

### 4.1 Gmail (4 tools)

**`search_emails`**
- Parameters: `query` (string, Gmail search syntax)
- Returns: List of emails (id, from, subject, snippet, date)
- Calls: **NEW** `integrations::gmail::search_messages(token, query)` -- hits `messages?q={query}` endpoint (existing `fetch_inbox()` only lists inbox by label, does not support query)

**`read_email`**
- Parameters: `email_id` (string)
- Returns: Full email content (from, to, subject, body, date)
- Calls: **NEW** `integrations::gmail::get_message_full(token, id)` -- fetches with `format=full`, decodes base64url body parts (existing `fetch_message_detail()` is private and only returns metadata headers)

**`send_email`**
- Parameters: `to` (string), `subject` (string), `body` (string), `cc` (optional string)
- Returns: Confirmation with message ID
- Calls: **NEW** `integrations::gmail::send_message(token, to, subject, body, cc)` -- constructs RFC 2822 message, base64url-encodes, POSTs to `messages/send`
- Safety: logs outgoing email to `emails` table before sending

**`archive_email`**
- Parameters: `email_id` (string)
- Returns: Confirmation
- Calls: existing `integrations::gmail::archive_message()` (already implemented)

### 4.2 Google Calendar (4 tools)

**`list_events`**
- Parameters: `date_from` (optional, YYYY-MM-DD), `date_to` (optional, YYYY-MM-DD)
- Defaults: today to 7 days ahead
- Returns: List of events (id, title, start, end, location, attendees)
- Calls: existing `integrations::calendar::fetch_events()` (rename reference from `list_events`)

**`create_event`**
- Parameters: `title` (string), `start` (ISO datetime), `end` (ISO datetime), `location` (optional), `description` (optional), `attendees` (optional, comma-separated emails)
- Returns: Confirmation with event ID and link
- Calls: existing `integrations::calendar::create_event()` -- **EXTEND** to support `location` and `attendees` parameters (currently only takes summary, start, end, description)

**`update_event`**
- Parameters: `event_id` (string), `title` (optional), `start` (optional), `end` (optional), `location` (optional), `description` (optional)
- Returns: Confirmation
- Calls: **NEW** `integrations::calendar::update_event()` -- PATCH to `events/{eventId}`

**`delete_event`**
- Parameters: `event_id` (string)
- Returns: Confirmation
- Calls: **NEW** `integrations::calendar::delete_event()` -- DELETE to `events/{eventId}`

### 4.3 Notion (3 tools)

**`search_notion`**
- Parameters: `query` (string)
- Returns: List of pages (id, title, last_edited, url)
- Calls: existing `integrations::notion::search_pages()` (not `search()`)

**`read_notion_page`**
- Parameters: `page_id` (string)
- Returns: Page content as markdown text
- Calls: **NEW** `integrations::notion::get_page_content(token, page_id)` -- fetches block children, renders as markdown

**`create_notion_page`**
- Parameters: `title` (string), `content` (string, markdown), `parent_id` (required, page ID)
- Returns: Confirmation with page URL
- Calls: existing `integrations::notion::create_page()` -- `parent_id` is mandatory (workspace root creation uses a different payload structure not worth supporting)

### 4.4 GitHub (2 tools)

**`list_github_items`**
- Parameters: `item_type` (string: "prs", "issues"), `repo` (optional, "owner/repo")
- Returns: List of items (id, title, state, url, updated_at)
- Calls: **EXTEND** existing `integrations::github::fetch_assigned_items()` to accept `item_type` and optional `repo` filter (currently returns all assigned items unfiltered)

**`create_github_issue`**
- Parameters: `repo` (string, "owner/repo"), `title` (string), `body` (optional string), `labels` (optional, comma-separated)
- Returns: Confirmation with issue URL
- Calls: **EXTEND** existing `integrations::github::create_issue()` to support `labels` parameter

### 4.5 Obsidian (2 tools)

**`search_notes`**
- Parameters: `query` (string)
- Returns: List of matching notes (path, title, snippet)
- Calls: existing `integrations::obsidian::search_vault()` (not `search()`)

**`read_note`**
- Parameters: `path` (string, relative to vault root)
- Returns: Full note content as markdown
- Calls: existing `integrations::obsidian::get_note()` (not `read_note()`)

### 4.6 New Integration Functions Required

The following new or extended functions must be written:

| Module | Function | Type | Description |
|--------|----------|------|-------------|
| `gmail` | `search_messages(token, query)` | NEW | Gmail search API with query string |
| `gmail` | `get_message_full(token, id)` | NEW | Full message fetch with body decode |
| `gmail` | `send_message(token, to, subject, body, cc)` | NEW | RFC 2822 compose + base64url encode + send |
| `calendar` | `create_event()` | EXTEND | Add `location` and `attendees` params |
| `calendar` | `update_event(token, event_id, fields)` | NEW | PATCH event endpoint |
| `calendar` | `delete_event(token, event_id)` | NEW | DELETE event endpoint |
| `notion` | `get_page_content(token, page_id)` | NEW | Fetch blocks, render markdown |
| `github` | `fetch_assigned_items()` | EXTEND | Add item_type + repo filtering |
| `github` | `create_issue()` | EXTEND | Add labels support |

## 5. New System Tools

### 5.1 Clipboard (2 tools)

**`clipboard_read`**
- Parameters: none
- Returns: Current clipboard text content
- Implementation: `pbpaste` command

**`clipboard_write`**
- Parameters: `content` (string)
- Returns: Confirmation
- Implementation: pipe to `pbcopy`

### 5.2 Screenshot (1 tool)

**`screenshot`**
- Parameters: `region` (optional: "full", "window", or "selection", defaults to "full")
- Returns: File path to saved screenshot
- Implementation: `screencapture` command, saves to temp directory

### 5.3 Window Management (1 tool)

**`manage_window`**
- Parameters: `action` (string: "focus", "resize", "move", "list"), `app_name` (optional), `width` (optional), `height` (optional), `x` (optional), `y` (optional)
- Returns: Result of action or list of windows
- Implementation: AppleScript via `osascript`

### 5.4 System Controls (1 tool)

**`system_controls`**
- Parameters: `action` (string: "get_volume", "set_volume", "get_brightness", "set_brightness", "toggle_dark_mode"), `value` (optional integer 0-100)
- Returns: Current value or confirmation
- Implementation: AppleScript for volume/dark mode, `brightness` CLI for display

### 5.5 Notifications (1 tool)

**`send_notification`**
- Parameters: `title` (string), `message` (string), `sound` (optional boolean, defaults to true)
- Returns: Confirmation
- Implementation: `osascript -e 'display notification'`

### 5.6 Process Management (2 tools)

**`list_processes`**
- Parameters: `filter` (optional string to filter by name)
- Returns: List of processes (pid, name, cpu%, memory)
- Implementation: `ps aux` for listing

**`kill_process`**
- Parameters: `pid` (integer)
- Returns: Confirmation or error
- Implementation: `kill` command
- Safety: refuses to kill system processes (pid < 100, kernel_task, WindowServer, loginwindow)

### 5.7 File Reading (1 tool)

**`read_file`**
- Parameters: `path` (string), `max_lines` (optional integer, defaults to 100)
- Returns: File contents as text (truncated if too large)
- Implementation: reads file directly in Rust, returns content
- Safety: max 100KB read limit, refuses binary files

## 6. Tool Count Summary

| Category | Tools | Count |
|----------|-------|-------|
| Existing system | open_app, open_url, run_command, find_files, open_file, create_task, write_note, system_info | 8 |
| Gmail | search_emails, read_email, send_email, archive_email | 4 |
| Google Calendar | list_events, create_event, update_event, delete_event | 4 |
| Notion | search_notion, read_notion_page, create_notion_page | 3 |
| GitHub | list_github_items, create_github_issue | 2 |
| Obsidian | search_notes, read_note | 2 |
| New system | clipboard_read, clipboard_write, screenshot, manage_window, system_controls, send_notification, list_processes, kill_process, read_file | 9 |
| **Total** | | **32** |

## 7. System Prompt Update

Update `SYSTEM_PROMPT` in `tools.rs` to reflect expanded capabilities:

```
You are JARVIS, a personal AI assistant on macOS for Hillman Chan (GitHub: ChiFungHillmanChan). Be concise and direct like the JARVIS from Iron Man.

You have 32 tools to control the computer and manage integrations. Use them proactively when the user asks you to do something.

Capabilities:
- System control: open apps, URLs, files, run commands, clipboard, screenshots, window management, volume/brightness, notifications, process management
- Gmail: search, read, send, and archive emails
- Google Calendar: list, create, update, and delete events
- Notion: search, read, and create pages
- GitHub: list PRs/issues/notifications, create issues
- Obsidian: search and read notes
- Tasks: create tasks and reminders

You can chain multiple tools in sequence. Think step by step -- gather information first, then act. Always confirm destructive actions in your response text before executing them.
```

## 8. Files to Modify

| File | Change |
|------|--------|
| `src-tauri/src/ai/openai.rs` | Model `gpt-4o-mini` -> `gpt-5`, pass db/auth to execute_tool, max_tokens 1024 -> 4096 |
| `src-tauri/src/ai/claude.rs` | Model -> `claude-sonnet-4-6-20250610`, pass db/auth to execute_tool, max_tokens 1024 -> 4096 |
| `src-tauri/src/ai/mod.rs` | Default provider -> `OpenAIPrimary`, add db/auth params to `send()` |
| `src-tauri/src/ai/tools.rs` | Add 24 new tool definitions, update execute_tool signature with db/auth, update SYSTEM_PROMPT, add result truncation (4000 chars) |
| `src-tauri/src/commands/chat.rs` | Pass db/google_auth to router.send() |
| `src-tauri/src/commands/assistant.rs` | Add `State<Arc<GoogleAuth>>` param to `get_briefing`, `speak_briefing`, `ask_jarvis`; pass to router.send() |
| `src-tauri/src/assistant/briefing.rs` | Add google_auth param to `generate_briefing()`; pass to router.send() |
| `src-tauri/src/integrations/gmail.rs` | Add `search_messages()`, `get_message_full()`, `send_message()` |
| `src-tauri/src/integrations/calendar.rs` | Extend `create_event()` (location/attendees), add `update_event()`, `delete_event()` |
| `src-tauri/src/integrations/notion.rs` | Add `get_page_content()` |
| `src-tauri/src/integrations/github.rs` | Extend `fetch_assigned_items()` (filtering), extend `create_issue()` (labels) |
| `src-tauri/src/system/control.rs` | Add clipboard_read, clipboard_write, screenshot, manage_window, system_controls, send_notification, list_processes, kill_process, read_file functions |

## 9. Safety Measures

- `send_email`: all outgoing emails logged to DB before sending
- `run_command`: best-effort destructive command blocking (blocks `rm`, `sudo`, `mkfs`, `dd`, `> /dev`, `chmod 777`, `curl|sh`, `wget|sh`; does not claim full read-only safety)
- `kill_process`: refuses system-critical processes (pid < 100, kernel_task, WindowServer, loginwindow)
- `read_file`: 100KB limit, binary file rejection
- `screenshot`: returns file path only (AI cannot see the image, but can tell user where it's saved or open it)
- All tool executions logged (name, args, truncated result)
- Tool results truncated to 4000 chars before feeding back into AI context
- Token refresh: integration tools retry once after 401 with `google_auth.refresh_access_token()`
- System prompt instructs AI to confirm destructive actions in text before executing

## 10. Dependencies

All new tools use:
- Existing integration clients in the codebase (with extensions and new functions per Section 4.6)
- macOS built-in commands (pbcopy, pbpaste, screencapture, osascript, ps, kill)
- Rust std::fs for file reading
- **2 new crates:** `base64` (for Gmail body decoding) and `urlencoding` (for Gmail search query encoding)

## 11. Testing Strategy

- Unit test each new tool function in `system/control.rs`
- Integration test tool execution with mock DB
- Manual test: ask JARVIS to perform multi-step tasks (e.g., "check my calendar and email me a summary")
- Verify fallback: disable OpenAI key, confirm Claude handles tools correctly
