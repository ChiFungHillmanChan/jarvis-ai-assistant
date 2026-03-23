# JARVIS - Personal AI Assistant

A macOS desktop application inspired by Iron Man's JARVIS. Built with Tauri v2 (Rust backend) + React + TypeScript frontend, featuring a holographic dark UI with glowing cyan accents.

JARVIS connects to your email, calendar, notes, and code -- then proactively helps you stay on top of everything through an AI-powered dashboard, voice commands, and automated background tasks.

## Features

### Dashboard
- Three-column holographic layout: sidebar navigation, AI-curated timeline, quick stats
- Time-based greeting with task/deadline summaries
- Live stats for email, calendar, GitHub, Notion, and cron jobs

### AI Chat
- **Cmd+K** opens the chat panel
- Claude API (primary) with OpenAI fallback
- Concise, technical JARVIS-style responses
- Conversation history stored locally

### Voice
- **Cmd+Shift+J** push-to-talk activation
- Speech-to-text via OpenAI Whisper API
- Text-to-speech via macOS `say` command with configurable voice
- Visual indicator overlay (listening / processing / speaking)

### Integrations
- **Gmail** -- inbox sync, email summaries, archive with learning
- **Google Calendar** -- event sync, meeting prep, event creation
- **Notion** -- page sync, search, create pages
- **GitHub** -- assigned issues, PRs for review, CI status

### Automation
- **Cron engine** with 5 built-in jobs (email sync, calendar sync, deadline monitor, Notion sync, GitHub digest)
- **Email learning** -- suggests auto-archive rules after you archive the same sender 3+ times
- **Custom cron jobs** -- describe in natural language (e.g., "every Monday check email for spam") and AI creates the schedule

### System
- System tray icon with show/quit menu
- Launch on startup (macOS login item)
- Local-first SQLite database (4 migrations, 10 tables)
- All data cached locally; source of truth remains in external services

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | Tauri v2 |
| Frontend | React 18 + TypeScript + Vite |
| Backend | Rust |
| Database | SQLite (rusqlite + refinery migrations) |
| AI | Claude API + OpenAI API |
| Voice STT | OpenAI Whisper API |
| Voice TTS | macOS `say` command |
| Audio | cpal 0.17 |
| Auth | OAuth2 (Google), API keys (Notion, GitHub) |
| Scheduling | tokio-cron-scheduler |

## Setup

### Prerequisites

- [Rust](https://rustup.rs/) (1.70+)
- [Node.js](https://nodejs.org/) (18+)
- [cmake](https://cmake.org/) (`brew install cmake`)
- macOS (Tauri v2 desktop app)

### Install

```bash
git clone https://github.com/ChiFungHillmanChan/jarvis-ai-assistant.git
cd jarvis-ai-assistant
npm install
```

### Configure

Copy the example env file and fill in your keys:

```bash
cp .env.example .env
```

Required keys:
```
ANTHROPIC_API_KEY=sk-ant-...      # Claude API (primary AI)
OPENAI_API_KEY=sk-...              # OpenAI (fallback AI + voice STT)
```

Optional keys (add via Settings page in-app):
```
GOOGLE_CLIENT_ID=...               # Gmail + Calendar OAuth
GOOGLE_CLIENT_SECRET=...           # Gmail + Calendar OAuth
```

Notion and GitHub tokens are configured in the Settings page within the app.

### Google OAuth Setup (for Gmail + Calendar)

1. Go to [Google Cloud Console](https://console.cloud.google.com)
2. Create a project, enable **Gmail API** and **Google Calendar API**
3. Configure OAuth consent screen (add scopes: `gmail.readonly`, `gmail.modify`, `calendar`)
4. Create OAuth credentials (Desktop app type)
5. Add your email as a test user
6. Put Client ID and Secret in `.env`

### Run

```bash
npm run tauri dev
```

First build compiles ~530 Rust crates (takes a few minutes). Subsequent builds are fast.

## Usage

| Action | Shortcut |
|--------|----------|
| Open chat | Cmd+K |
| Close chat | Esc |
| Voice input | Cmd+Shift+J (toggle) |

### Dashboard
The home screen shows your day at a glance -- upcoming deadlines, meetings, email count, GitHub activity, and cron job status.

### Settings (sidebar "S")
- AI provider selection (Claude/OpenAI)
- Google account connection
- Notion API key
- GitHub personal access token
- Voice/TTS configuration

### Cron Dashboard
Navigate to the cron view to see all scheduled jobs, their run history, and create custom jobs using natural language.

## Project Structure

```
src/                          # React frontend
  components/                 # UI components (holographic theme)
  hooks/                      # React hooks (useChat, useVoiceState, etc.)
  lib/                        # Types and Tauri command wrappers
  pages/                      # Dashboard, Settings, CronDashboard
  styles/                     # Global CSS with holographic theme variables

src-tauri/                    # Rust backend
  src/
    ai/                       # Claude + OpenAI API clients with fallback
    auth/                     # Google OAuth2 PKCE flow
    commands/                 # Tauri IPC commands (30+)
    integrations/             # Gmail, Calendar, Notion, GitHub API clients
    scheduler/                # Cron engine with background jobs
    voice/                    # Audio capture, STT, TTS
    db.rs                     # SQLite with refinery migrations
    tray.rs                   # System tray
  migrations/                 # SQL schema (V1-V4)
```

## Design

Visual style: full holographic -- dark background (#0a0e1a), glowing cyan (rgba(0, 180, 255)), subtle grid overlay, translucent panels, monospace system labels. No emojis.

See `docs/superpowers/specs/2026-03-23-jarvis-assistant-design.md` for the full design spec.

## License

MIT
