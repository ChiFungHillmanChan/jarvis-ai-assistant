# JARVIS Project Guide

## What is this
Personal AI assistant desktop app inspired by Iron Man's JARVIS. Built with Tauri v2 (Rust) + React + TypeScript. Holographic 3D UI.

## Tech Stack
- **Backend:** Rust (Tauri v2), SQLite (rusqlite + refinery), tokio async runtime
- **Frontend:** React 18 + TypeScript + Vite, Canvas2D for 3D scene
- **AI:** Claude API (primary) + OpenAI API (fallback), configurable via Settings
- **Voice:** OpenAI Whisper API (STT), macOS `say` command (TTS), cpal 0.17 (audio capture)

## Project Structure
```
src/                    # React frontend
  components/           # UI components (holographic theme)
  components/3d/        # 3D holographic sphere scene
  hooks/                # useChat, useVoiceState, useKeyboard, useTauriCommand
  lib/                  # types.ts + commands.ts (Tauri IPC wrappers)
  pages/                # Dashboard, Settings, EmailPage, CalendarPage, GitHubPage, NotionPage, CronDashboard
  styles/               # global.css (holographic theme), animations.css

src-tauri/              # Rust backend
  src/ai/               # Claude + OpenAI API clients with system prompts
  src/assistant/        # Smart assistant: context builder, briefing, actions parser
  src/auth/             # Google OAuth2 PKCE flow
  src/commands/         # All Tauri IPC commands (40+)
  src/db.rs             # SQLite with refinery migrations
  src/integrations/     # Gmail, Calendar, Notion, GitHub, Obsidian API clients
  src/scheduler/        # tokio-cron-scheduler with background jobs
  src/system/           # Computer control: open apps, URLs, shell, files, notes
  src/voice/            # Audio capture, transcription, TTS
  src/tray.rs           # System tray
  migrations/           # V1-V5 SQL schema files
```

## Database
SQLite at `~/Library/Application Support/jarvis/jarvis.db`
- 10 tables: tasks, conversations, emails, calendar_events, github_items, notion_pages, email_rules, cron_jobs, cron_runs, user_preferences
- 5 migrations (V1-V5)
- Managed by refinery, runs on startup

## Key Patterns
- Database wrapped in `Arc<Database>` -- all commands use `State<Arc<Database>>`
- AiRouter NOT wrapped in Arc -- use `State<'_, AiRouter>` for async commands
- GoogleAuth wrapped in `Arc<GoogleAuth>`
- Action tags in AI responses: `[OPEN_APP:Name]`, `[OPEN_URL:url]`, `[TASK:title|desc|deadline|priority]`, etc. Parsed by `assistant/actions.rs`
- System prompts in `ai/claude.rs` and `ai/openai.rs` -- includes macOS app names and action tag instructions
- AI provider read from `user_preferences` table on startup

## .env Required
```
ANTHROPIC_API_KEY=     # Claude API
OPENAI_API_KEY=        # OpenAI (fallback AI + voice STT)
GOOGLE_CLIENT_ID=      # Gmail + Calendar OAuth
GOOGLE_CLIENT_SECRET=  # Gmail + Calendar OAuth
```
Notion, GitHub, Obsidian tokens set in app Settings page (stored in user_preferences DB).

## Running
```bash
npm run tauri dev
```

## No emojis
The user does not want emojis in UI, code, or output.

## Design Style
Full holographic: dark background (#060a14), cyan glow (rgba 0,180,255), glassmorphism panels, 3D interactive data sphere with Canvas2D.
