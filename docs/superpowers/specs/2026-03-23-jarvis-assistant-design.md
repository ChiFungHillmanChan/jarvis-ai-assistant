# JARVIS -- Personal AI Assistant

## Overview

A macOS desktop application that serves as a personal AI assistant inspired by JARVIS from Marvel's Iron Man. The app launches on startup, provides a holographic-styled dashboard with real-time information from connected services, and supports both voice and text interaction. Built for personal use first, with architecture designed to support multiple users later.

## Tech Stack

- **Framework:** Tauri v2 (Rust backend + WebView frontend)
- **Frontend:** React + TypeScript
- **Backend:** Rust
- **Database:** SQLite (local-first, with optional cloud sync path via cr-sqlite)
- **AI:** Claude API (primary) + OpenAI API (secondary/fallback)
- **Voice (Phase 2):** Porcupine (wake word) + Whisper tiny/base (STT) + macOS system TTS
- **Migrations:** refinery (Rust-native SQLite migration tool)

## Architecture

Monolithic Tauri application. Single binary, all logic in one process.

```
+--------------------------------------------------+
|                 JARVIS App (Tauri)                |
|                                                  |
|  +--------------------------------------------+  |
|  |           React Frontend (WebView)         |  |
|  |                                            |  |
|  |  +----------+ +----------+ +-----------+   |  |
|  |  |Dashboard | |Chat Panel| |Settings   |   |  |
|  |  |  View    | |(floating)| |  View     |   |  |
|  |  +----------+ +----------+ +-----------+   |  |
|  +--------------------------------------------+  |
|                    | Tauri IPC                    |
|  +--------------------------------------------+  |
|  |            Rust Backend                    |  |
|  |                                            |  |
|  |  +---------+  +----------+  +----------+   |  |
|  |  |AI Router|  |Scheduler |  |Data Store|   |  |
|  |  |Claude/  |  |(Cron     |  |(SQLite)  |   |  |
|  |  |OpenAI   |  | Engine)  |  |          |   |  |
|  |  +---------+  +----------+  +----------+   |  |
|  |                                            |  |
|  |  +-------------------------------------+   |  |
|  |  |        Integration Modules          |   |  |
|  |  |  Email | Calendar | Notion | GitHub  |   |  |
|  |  +-------------------------------------+   |  |
|  +--------------------------------------------+  |
|                                                  |
|  System Tray Icon    Launch on Startup           |
+--------------------------------------------------+
```

### Component Responsibilities

**React Frontend:**
- Renders holographic UI in Tauri's WebView
- Three main views: Dashboard, Chat (floating overlay), Settings
- Communicates with backend exclusively through Tauri IPC commands

**Rust Backend -- AI Router:**
- Claude is the primary model for all requests
- OpenAI used as fallback when Claude is unavailable (rate limit, downtime)
- Routing logic: try Claude first, if error/timeout after 10s, retry with OpenAI
- User can override in settings (e.g., "always use OpenAI" or "Claude only")

**Rust Backend -- Scheduler:**
- Cron engine using tokio-cron-scheduler
- Jobs persist in SQLite, survive restarts
- Manages all background automation (email cleanup, deadline monitoring, syncs)

**Rust Backend -- Data Store:**
- SQLite database at ~/Library/Application Support/jarvis/jarvis.db
- API tokens encrypted via macOS Keychain
- All integration data is cached locally; source of truth remains in external services

**Rust Backend -- Integration Modules:**
- Each integration (Email, Calendar, Notion, GitHub) is a Rust module
- Standard interface per module: fetch, sync, act
- OAuth2/API key auth per service, tokens in Keychain

## UI Design

### Visual Style: Full Holographic

- Dark background (#0a0e1a)
- Glowing cyan/blue color palette (primary: rgba(0, 180, 255))
- Subtle grid lines overlay
- Translucent panels with border glow
- Monospace typography for system labels
- Light weight (200-300) fonts for data display
- No emojis anywhere -- use text symbols and CSS/SVG icons only

### Dashboard Layout: Timeline + Sidebar

Three-column layout:

**Left -- Icon Sidebar:**
- Navigation icons for: Home, Email, Calendar, GitHub, Notion, Settings
- Chat activation button at bottom
- Narrow (50-60px), always visible

**Center -- AI-Curated Timeline:**
- JARVIS greeting with contextual summary ("Good morning, Hillman. You have 3 meetings and 7 tasks today.")
- Chronological timeline of the day's events, tasks, and alerts
- Priority-ordered: urgent items (overdue deadlines, critical notifications) surface to top with warning color (rgba(255, 100, 100))
- Regular items in chronological order with time markers
- Timeline ranking: urgent/overdue items first (sorted by deadline proximity), then today's calendar events chronologically, then pending tasks by priority. No AI call needed for ranking -- pure rule-based sorting. If all data sources fail to load, show cached data with a "last synced X ago" indicator.

**Right -- Quick Stats Panel:**
- Stacked cards showing at-a-glance numbers: unread emails, pending PRs, active cron jobs
- Each card shows: metric name, count, last activity
- Cron job card shows: status, last run result, next run time

### Chat Panel

- Activated via Cmd+K (configurable) or voice
- Slides in as floating overlay on the right side of the dashboard
- Can expand to full screen (replaces dashboard view, Esc to return)
- Shows conversation thread with JARVIS
- Input field at bottom for typing
- Voice indicator when listening

## Interaction Model

### Voice (Phase 2 -- Primary, if permitted)

**Flow:**
1. Wake word detected (e.g., "Hey JARVIS") -- runs locally, always listening
2. Listening indicator appears on screen
3. Speech-to-text processes user speech (Whisper local or cloud STT)
4. AI generates response
5. Dashboard updates + JARVIS speaks response via TTS

**Voice Settings:**
- Wake word: customizable
- Voice toggle: on/off
- Active hours: configurable window (e.g., 9am-6pm)
- Mute hotkey: instant toggle
- STT engine: local (Whisper) or cloud
- TTS voice: system or cloud voice API

### Text (Cmd+K Command Bar)

- For when voice is not appropriate
- Natural language input, same AI processing as voice
- Results shown in chat panel and reflected on dashboard

### Direct Interaction

- Click/interact with dashboard cards, tasks, calendar items directly
- Context menus for quick actions (mark done, reschedule, archive)

## Scheduler & Cron Jobs

### Built-in Job Types

- **Email cleanup** -- scan inbox on schedule, flag/archive spam, surface important emails
- **Deadline monitor** -- daily check, warns at 3 days, 1 day, and same day before deadlines
- **GitHub digest** -- periodic check for new PRs, assigned issues, CI failures
- **Notion sync** -- pull latest from specified Notion databases/pages
- **Calendar prep** -- 15 minutes before meetings, surface related tasks, notes, attendee context

### Custom Jobs (Phase 2)

- Created via voice or chat: "Every Monday morning, check my email for spam and clean it"
- JARVIS parses natural language into cron schedule + action using AI
- Supported patterns: daily, weekly, monthly, every N hours/days. Complex patterns ("every other Thursday except holidays") are out of scope.
- Always confirms before activating, shows the parsed cron expression for verification

### Dashboard Display

- Cron jobs visible in right panel: job name, status (active/paused), last run, next run
- Failed jobs show warning indicator
- JARVIS proactively notifies on failures

## Data Model

### SQLite Tables

- **user_preferences** -- settings, active hours, voice config, wake word
- **tasks** -- title, description, deadline, priority, status, source (manual/Notion/GitHub)
- **calendar_events** -- synced from Google Calendar, local metadata (prep notes, related tasks)
- **emails** -- cached summaries (not full content), labels, importance score, spam flag
- **github_items** -- PRs, issues, CI status, synced periodically
- **notion_pages** -- synced page metadata and content summaries
- **conversations** -- chat/voice history with JARVIS, searchable
- **cron_jobs** -- schedule, action type, parameters, status
- **cron_runs** -- execution log (timestamp, result, errors)

### Security

- API tokens encrypted at rest using macOS Keychain
- Conversation history never sent to AI providers unless user explicitly asks about past conversations
- Integration data is cached; source of truth remains in external services

### Schema Migrations

- Managed by refinery (Rust-native migration tool)
- Migration files in src-tauri/migrations/, numbered sequentially
- Run automatically on app startup before any database access

### Future Sync Path

- Optional cloud sync via cr-sqlite (conflict-free replication) to a personal server or managed service
- Will require adding user_id columns to all tables and a sync conflict resolution strategy
- This is a significant change -- architectural simplicity is prioritized now, sync is a Phase 3 concern

## Integrations (Phase 1)

### Email (Gmail API)

- **Auth:** OAuth2, token in Keychain
- **Read:** fetch inbox, AI-powered importance scoring, spam detection
- **Write:** archive, label (requires explicit permission toggle)
- **Learning (Phase 2):** Rule-based pattern learning -- when you archive the same sender 3+ times, JARVIS suggests an auto-archive rule. Stored as simple rules in SQLite, not model fine-tuning.
- **Default:** read-only

### Calendar (Google Calendar API)

- **Auth:** OAuth2
- **Read:** fetch events, detect conflicts, surface prep context
- **Write:** create/move events via voice or chat command
- **Pre-meeting briefing:** 15 min before, gather related tasks, notes, attendee context

### Notion API

- **Auth:** API key or OAuth
- **Scope:** user selects which databases/pages JARVIS can access
- **Read:** sync specified content
- **Write:** create/update pages ("add meeting notes to Notion")

### GitHub API

- **Auth:** personal access token or OAuth
- **Monitor:** PRs (assigned/reviewing), issues (assigned), CI status
- **Actions:** comment on PRs, create issues when asked
- **Digest:** periodic summary of activity

### Permission Model

- Each integration has granular read/write toggles in Settings
- First-time setup wizard for connecting services
- JARVIS always confirms before write actions ("Archive 12 spam emails? Confirm.")

## Response Style

JARVIS responds concisely in a technical assistant tone. Direct, not chatty.

- Good: "Standup rescheduled to 10:00. Calendar updated."
- Bad: "Sure! I've gone ahead and moved your standup meeting for you!"

## Startup Flow

1. App registered as macOS login item
2. On boot: load SQLite -> start scheduler -> show system tray icon
3. Dashboard window opens (or stays minimized based on user preference)
4. Voice listener activates (if permitted and within active hours)
5. JARVIS greets user with daily summary

## Error Handling & Resilience

- **AI provider failure:** Try Claude first, fall back to OpenAI. If both fail, show a "JARVIS offline" indicator and queue the request for retry.
- **Integration API failures:** Show cached data with a "last synced X ago" label. Retry on next scheduled sync. Log errors to ~/Library/Application Support/jarvis/logs/.
- **OAuth token expiry:** Detect 401 responses, prompt user to re-authenticate via Settings. Do not silently fail.
- **Network outage:** All dashboard data comes from local SQLite cache. App remains fully usable with stale data. Sync resumes when connectivity returns.
- **Cron job failures:** Log to cron_runs table with error details. Show warning indicator on the cron card. Retry once after 5 minutes, then mark as failed.

## Logging

- Log file at ~/Library/Application Support/jarvis/logs/jarvis.log
- Log levels: ERROR, WARN, INFO, DEBUG (configurable in settings)
- Default: INFO in production, DEBUG during development
- Logs rotate daily, kept for 7 days
- Debug view accessible in Settings for inspecting recent logs in-app

## Resource Budget

- **Target idle CPU:** < 2% (dashboard open, no active AI calls)
- **Target RAM:** < 150MB (WebView + Rust backend + SQLite)
- **Integration syncs:** throttled to once per 5 minutes per service to avoid API rate limits
- **Battery consideration:** reduce sync frequency to once per 15 minutes when on battery power
- **Voice (Phase 2):** wake word detection (Porcupine) adds ~10MB RAM, < 1% CPU. Whisper transcription is on-demand only (not continuous).

## Known Risks

- **Google OAuth for desktop apps:** Redirect URI handling and sensitive scope verification can be complex. Plan to use loopback redirect (http://localhost) flow. May need Google Cloud project verification for Gmail scopes.
- **Tauri v2 maturity:** Tauri v2 is relatively new. Pin to a stable release and monitor for breaking changes.

## Phasing

- **Phase 1a:** Core app shell (Tauri + React), holographic dashboard UI, chat/command bar, SQLite with migrations, AI router (Claude primary + OpenAI fallback), system tray + launch on startup
- **Phase 1b:** Email + Calendar integrations (OAuth2), built-in cron engine with deadline monitor and sync jobs
- **Phase 1c:** Notion + GitHub integrations, cron dashboard display
- **Phase 2:** Voice activation (Porcupine wake word + Whisper STT + macOS TTS), rule-based email learning, custom cron jobs via natural language
- **Phase 3:** Optional cloud sync, multi-device support, plugin architecture for community integrations
