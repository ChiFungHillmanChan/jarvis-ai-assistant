# JARVIS UIUX Improvement: Chat, Voice, and Cron Scheduling

**Date:** 2026-03-25
**Status:** Approved
**Priority:** A (Chat) > B (Voice) > C (Cron)

## Overview

Three areas of the JARVIS desktop assistant need significant UIUX upgrades to match the holographic design vision: AI Chat with inline tool results, Voice integration into the 3D sphere, and Natural Language Cron Scheduling with animated conversion flow.

Design principle: **non-intrusive UI** -- nothing blocks the main content area unless the user explicitly requests it.

---

## A. AI Chat with Inline Tool Results

### A1. Message Rendering Pipeline

Currently all chat messages render as plain text. The upgrade introduces three new content types rendered inline within message bubbles:

**Markdown rendering:**
- Headings, bold, italic, bullet/numbered lists, links
- Implemented with a simple regex-based parser (no external library)
- Styled with holographic theme colors (cyan hierarchy)

**Data Charts (inline SVG):**
- Line charts, pie/donut charts, bar charts
- Hand-drawn SVG using holographic color palette. Multi-series data uses colors from `JarvisScene.tsx` TYPE_COLORS for consistency.
- Embedded directly inside message bubbles
- Use cases: email activity stats, calendar overview, GitHub contributions
- AI responses include chart data via `[CHART:type|data]` tags parsed by frontend

**Status Cards:**
- Displayed when AI executes an action (create task, sync email, set cron job)
- Card shows: icon, title, metadata (priority, due date), status badge (CREATED, SYNCED, etc.)
- Color-coded: green for success, red for failure, amber for pending
- Replaces plain text confirmations like "Task created successfully"

### A2. Content Parsing Architecture

```
AI response (plain text with embedded tags)
  |
  v
parseMessageContent() -- new function in MessageRenderer.tsx
  |
  +-- Plain text blocks --> markdown renderer
  +-- [CHART:line|{labels,data}] --> InlineChart component (SVG)
  +-- [STATUS:task_created|{name,priority,due}] --> StatusCard component
  |
  v
Rendered message bubble with mixed content
```

**Chart tag JSON schemas:**

Line chart: `[CHART:line|{"labels":["Mon","Tue",...],"series":[{"name":"Emails","data":[5,12,...]}]}]`

Pie chart: `[CHART:pie|{"segments":[{"label":"Read","value":75},{"label":"Unread","value":25}]}]`

Bar chart: `[CHART:bar|{"labels":["Jan","Feb",...],"series":[{"name":"Tasks","data":[3,7,...]}]}]`

Status card types: `[STATUS:type|{...}]` where type is one of:
- `task_created` -- fields: `name`, `priority`, `due`
- `task_completed` -- fields: `name`
- `email_synced` -- fields: `count`, `folder`
- `calendar_synced` -- fields: `count`
- `cron_created` -- fields: `name`, `schedule`, `description`
- `action_completed` -- fields: `action`, `result`

**AI prompt updates:**

Both Claude and OpenAI system prompts (in `src-tauri/src/ai/`) must be updated to instruct the model to emit these tags when presenting data or confirming actions. For **Claude**, the tags are embedded in the text response and parsed by the frontend. For **OpenAI**, since it uses native function calling, two new tool definitions are added to `src-tauri/src/ai/tools.rs`:
- `render_chart` -- parameters: `type` (line/pie/bar), `data` (matching the schemas above)
- `render_status` -- parameters: `type` (task_created/email_synced/etc.), `data` (matching the schemas above)

The `chat.rs` handler serializes OpenAI tool call results into the same `[CHART:...]` / `[STATUS:...]` tag format before storing in the conversation, ensuring a unified frontend parsing path regardless of AI provider.

### A3. Chat Panel Layout -- Dual Mode

**Overlay mode (enhanced):**
- Width increased from 380px to 440px
- Fixed position right side, only visible when user opens it
- No auto-open behavior -- user controls visibility entirely
- New input bar: rounded container + send button (play icon) on right
- Close button dismisses immediately

**Full view mode (new):**
- Added as a sidebar nav item "CHAT" -- same level as Email, Calendar, etc.
- Takes full main content area, more room for inline charts and status cards
- Shares same conversation state as overlay
- Expand button in overlay header switches to full view

**Non-intrusive rules:**
- Chat never auto-opens or persists uninvited
- No floating chat bars or persistent input areas on other views
- Overlay appears only on explicit sidebar click or keyboard shortcut
- Full view is just another page in the nav -- user navigates to it deliberately
- **Voice exception:** The existing auto-open on voice "thinking" state (App.tsx) is removed. Voice responses are communicated through the 3D sphere visual feedback + TTS audio. If the user wants to see the text response, they open chat manually.

### A4. Input Bar Design

- Rounded input container with `border-radius: 12px`
- Send button: circular, play-arrow icon, subtle cyan glow
- Auto-growing textarea behavior preserved (Enter to send, Shift+Enter for newline)
- Placeholder text: "Talk to JARVIS..." (consistent with existing)

### A5. Files

**New files:**
- `src/components/chat/MessageRenderer.tsx` -- markdown + chart + status card parsing and rendering
- `src/components/chat/InlineChart.tsx` -- SVG line/pie/bar chart components
- `src/components/chat/StatusCard.tsx` -- action result card component

**Modified files:**
- `ChatMessage.tsx` -- use MessageRenderer instead of plain text content
- `ChatPanel.tsx` -- new input bar design, width increase to 440px. Remove the existing fullscreen overlay mode (the `isFullScreen` prop and related code). Replace with a simple "expand" button that navigates to the full chat view page.
- `App.tsx` -- add `case "chat"` to `renderView()` switch to render full chat view. Remove voice auto-open (`setChatOpen(true)` on "thinking" state).
- `Sidebar.tsx` -- add CHAT nav item
- `src-tauri/src/ai/tools.rs` -- add `render_chart` and `render_status` tool definitions for OpenAI function calling

**No external dependencies.** Markdown parsed with regex. Charts are inline SVG.

---

## B. Voice Integration into 3D Sphere

### B1. Sphere State Responses

The 3D sphere (JarvisScene.tsx) already has an `activityLevel` system and a 48-bar radial waveform for TTS. This upgrade expands the sphere to be the primary voice state indicator.

**Idle state:**
- Gentle breathing pulse on core
- Slow orbital ring rotation
- Ambient particle field
- Existing behavior, no changes needed

**Listening state:**
- Core expands ~20% (scale the inner gradient radius)
- Orbital rings speed up to 1.5x
- Radial waveform appears, driven by microphone input amplitude (new)
- Background particles drawn inward toward core ("intake" effect)
- Color: cyan (existing palette)

**Processing state:**
- Core color shifts to amber `rgba(255, 180, 0, ...)`
- Rings spin at 3x speed
- Energy arcs fire outward from core (existing arc system, triggered more aggressively)
- Waveform freezes at last position and fades out
- Particles scatter outward

**Speaking state:**
- Core color shifts to green `rgba(16, 185, 129, ...)`
- 48 radial bars pulse with TTS amplitude (existing behavior, enhanced)
- Rings settle to gentle rhythmic rotation
- Particles orbit at medium speed

### B2. Enhanced Waveform

**Existing (TTS only):**
- 48 radial bars around sphere center
- 3 sine waves at different frequencies for procedural animation
- TTS amplitude from `ttsAmplitudeRef` drives bar heights

**New additions:**
- **Mic input waveform:** New `micAmplitudeRef` drives bars during listening state. Backend emits `mic-amplitude` events mirroring existing `tts-amplitude` pattern.
- **Color transitions:** Bar colors smoothly lerp between state colors (cyan -> amber -> green) using the same crossfade approach as `speakingAlpha`
- **Core glow pulse:** The central radial gradient intensity scales with voice amplitude
- **Particle attraction:** During listening, particle velocities gain an inward component proportional to mic amplitude. Simple force calculation: `velocity += normalize(center - position) * amplitude * attractionStrength`

### B3. Minimal Bottom Indicator

The bottom VoiceIndicator shrinks to a minimal secondary confirmation:
- Tiny dot (6px) + one-word label ("LISTENING", "PROCESSING", "SPEAKING")
- Minimal pill shape, semi-transparent background
- Shortened hint on hover: "Cmd+Shift+J" (keeps discoverability, minimal footprint)
- The sphere itself is the primary indicator -- the bottom dot is just text confirmation

### B4. Files

**Modified files:**
- `JarvisScene.tsx` -- mic amplitude waveform, color transitions per voice state, particle attraction during listening, core glow scaling
- `VoiceIndicator.tsx` -- shrink to minimal dot + label
- `App.tsx` -- create and pass `micAmplitudeRef` to JarvisScene (alongside existing `ttsAmplitudeRef`)

**Backend change:**
- `voice/mod.rs` -- emit `mic-amplitude` event during audio capture.

**Mic amplitude implementation approach:** The existing audio capture uses a lock-free SPSC ring buffer in the cpal callback. To avoid coupling the audio callback to the Tauri AppHandle, use a **shared `AtomicU32`** (storing `f32` bits via `to_bits`/`from_bits`). The audio callback computes RMS amplitude from each PCM buffer and writes to the atomic. A separate 50ms polling timer (spawned via `tokio::spawn`) reads the atomic and emits `mic-amplitude` events to the frontend. This preserves the lock-free audio architecture while providing real-time amplitude data.

**No new dependencies.** All effects are Canvas2D drawing within the existing animation loop.

---

## C. Natural Language Cron Scheduling

### C1. Animated Conversion Flow

When the user submits a natural language scheduling request, a three-stage animated flow visualizes the AI parsing process:

**Phase 1 -- Input glow (0-300ms):**
- Input field border glow intensifies
- Text slightly scales up
- CSS transition on border-color and box-shadow

**Phase 2 -- AI Parsing (300ms until response arrives):**
- "AI PARSING" label fades in below the input
- Pulsing loading animation (subtle glow pulse)
- Waits for backend `createCustomCron` response

**Phase 3 -- Result reveal (on response, +500ms):**
- Two cards expand from center: cron expression (left) + human-readable schedule (right)
- Cards have glow effect on borders
- Cron expression displayed in large monospace with text-shadow glow
- Human-readable schedule in smaller text below (e.g., "Every Friday at 00:00")

**Phase 4 -- Job created (+500ms after Phase 3):**
- Job created confirmation card slides up from bottom
- Green status badge fades in as "ACTIVE"
- Card shows job name, action type, status

**Error handling:** If AI parsing fails, Phase 3 shows a red-bordered error card with the error message instead of the cron result cards.

### C2. Human-Readable Schedule

The backend AI prompt is updated to return an additional `description` field:
```json
{
  "name": "Run backup",
  "schedule": "0 0 * * 5",
  "action_type": "email_sync",
  "description": "Every Friday at midnight"
}
```

This description is stored in the database and displayed alongside the raw cron expression everywhere in the UI.

### C3. Timeline View

Each job's detail view shows upcoming and recent runs side by side:

**Upcoming runs (left):**
- Next 3 scheduled run times computed from the cron expression
- Timeline format: dots connected by vertical lines, fading opacity for further dates
- Next run highlighted with larger dot, glow, and "in Xd" countdown
- Computed by Rust backend using `cron` crate's `Schedule::upcoming()` iterator

**Recent runs (right):**
- Last N runs from `cron_runs` table (existing)
- Each row: status badge (DONE/FAIL), timestamp, result/error message
- Color-coded: green for completed, red for failed

### C4. Dashboard Layout Redesign

**Current:** Two fixed columns (300px job list | flex run history)

**New layout:**
- **Top section:** Conversion flow area -- natural language input bar + animated parsing visualization
- **Middle section:** Job cards in responsive grid (2-3 columns depending on width). Each card shows: job name, human-readable schedule, status dot, next run countdown
- **Bottom / expandable:** Selected job expands inline to show timeline (upcoming + recent runs)

The layout shifts from a "list + detail" split to a "create -> browse -> inspect" vertical flow that matches the natural user workflow.

### C5. Files

**New files:**
- `src/components/cron/ConversionFlow.tsx` -- animated parsing flow with 4 phases
- `src/components/cron/CronTimeline.tsx` -- upcoming runs timeline + recent runs
- `src/components/cron/CronJobCard.tsx` -- redesigned card with human-readable schedule and next-run countdown

**Modified files:**
- `CronDashboard.tsx` -- new vertical layout with conversion flow, grid, expandable detail. **Remove all `window.location.reload()` calls** -- use React state updates (re-fetch job list) after CRUD operations so animations are not destroyed.
- `src-tauri/src/commands/cron.rs` -- AI prompt returns `description` field; add `get_upcoming_runs` command using cron crate. Update the Rust `CronJobView` struct to include `description: Option<String>`. Update SQL SELECT to include the new column.
- `src-tauri/src/lib.rs` -- register new `get_upcoming_runs` command in invoke_handler
- `src/lib/types.ts` -- CronJobView adds `description: string | null` and `upcoming_runs: string[]`
- `src/lib/commands.ts` -- add `getUpcomingRuns(jobId, count)` wrapper

**Database migration (V6):**
- File: `src-tauri/migrations/V6__cron_description.sql`
- SQL: `ALTER TABLE cron_jobs ADD COLUMN description TEXT;`

**Backend dependency:**
- Add `cron` as an **explicit** dependency in `Cargo.toml` (e.g., `cron = "0.12"`) -- do not rely on the transitive dependency from `tokio-cron-scheduler`. Use `Schedule::from_str()` and `upcoming()` to compute next N run times.

**Timezone handling:**
- `Schedule::upcoming()` uses the user's **local timezone** (`chrono::Local`) so displayed times match the user's expectations. Cron expressions like "0 0 9 * * *" mean 9 AM local time.

---

## Cross-Cutting Concerns

**No external frontend dependencies added.** All rendering uses inline SVG, CSS transitions, and Canvas2D.

**Holographic theme consistency:** All new components use existing CSS variables (`--bg-panel`, `--border-primary`, `--text-primary`, `--accent-success`, etc.) and follow the established color hierarchy.

**Performance:** Charts are lightweight SVG (no canvas overhead). Voice amplitude uses refs (not state) to avoid re-renders. Cron animations use CSS transitions where possible, requestAnimationFrame only for the conversion flow sequence.

**No emojis** anywhere in the UI.
