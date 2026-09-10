# Workspace redesign

2026-09-10 完成工作區改版，保留原有本機 Mail 功能。用戶已批准提交、推送及發佈；正式版本記錄見 RELEASE_NOTES.md。

## Product behaviour

- Automation is the default, permanent tab. The existing rotating JARVIS model remains in the centre of this page. Only enabled routines appear alongside their actual run state and output. Opening the page does not create, enable, or execute a routine.
- Mail, Calendar, Notes, GitHub, Automations, Connections, and Settings have opaque, readable working surfaces. The 3D canvas is unmounted outside Automation and stops drawing while the document is hidden. Reduced-motion preference is respected.
- The sidebar has names and recent tasks. Tool tabs reuse their existing workspace; tasks have independent histories. Tabs can be closed, reopened, pinned by double-click, and selected with keyboard controls. Command search includes saved conversations and the legacy voice history.
- Local workspace state keeps open tabs, selected records, searches, assistant/calendar drafts, Mail group analysis, and local edits to saved draft results. State is debounced into SQLite settings; API credentials continue to use the existing Keychain path.
- A successful Assistant tool result opens a suitable result workspace beside the originating conversation. Repeated provider objects reuse their tab. Results arriving while another workspace is active add a notice without taking focus. Legacy Google email references remain separate from native Mail account/message IDs.
- Foreground chat events carry both task and run identity. Old conversation rows remain under the legacy session. One foreground assistant run is accepted at a time; another task can retain its draft while waiting.
- Automations show real status, schedules and recorded outputs. Pause/enable sets a requested state explicitly. Natural-language creation first interprets a schedule with tool execution disabled; only the separate Create action saves/enables it.
- Calendar queries and sync support the displayed week, including elapsed events today. New personal events do not send invitations. Notion metadata does not pretend to contain a page body; provider links are explicit user actions.
- Light, dark and system appearance are available in Settings. Existing provider, key, voice, wallpaper and integration controls remain available.

## Verification

- `npm run build`: TypeScript and production Vite build passed.
- `node --experimental-strip-types --test tests/*.test.ts`: 11 tests passed, covering tab identity, restoration, retained history, event/run isolation and provider result routing.
- `cargo test --offline --manifest-path src-tauri/Cargo.toml --quiet`: 355 passed, 0 failed, 7 ignored. Loopback fixture tests were run with sandbox permission.
- `git diff --check`: passed.
- Two code-review passes; fixed history hydration, stale automation status, stale run history, calendar range and pre-save automation interpretation issues.
- Actual React UI inspected in the browser with development-only synthetic data: Mail reading, close/reopen with selection and analysis retained, task submission and tool-result handoff, result tab reuse, background result focus, enabled-only overview, pause state synchronization, schedule review followed by explicit creation, and dark/light Settings.
- Preview was inspected at 1145px and 1280px widths. Working pages use container queries to adapt when the assistant dock reduces available width.

## Running the preview

Run `npm run dev`, then open `http://localhost:1420/?workspace-preview`. It renders the actual app components using synthetic local data and an isolated development IPC mock. It does not access real email, models, calendars, credentials, or scheduler jobs. Preview code is excluded from the production build.

Use `npm run tauri dev` to run the native application with its actual local services. Browser UI verification does not replace a native end-to-end pass for OAuth, speech, system window controls or paid provider calls.

## Current boundaries

- Native Mail and Assistant's legacy Google integration still use separate account stores, explicitly identified in the interface.
- Gmail draft results can be edited as a local working copy. Copy text and open Gmail to apply edits or send; JARVIS does not claim local edits have been saved remotely.
- Voice continues to use the retained legacy conversation. New foreground text tasks have scoped tool-result workspaces.
- Notion's list workspace displays synced metadata; actual page content appears when obtained through the Assistant tool or opened in Notion.
- 原生安裝、簽署、公證及公開更新渠道要按每次發佈記錄核對；以上瀏覽器驗證唔代表已通過所有真實服務流程。
