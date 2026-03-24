# JARVIS Smart Assistant Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform JARVIS from a data dashboard into a proactive personal assistant that briefs you each morning, suggests what to work on, preps you before meetings, handles natural language actions, and notifies you when things need attention.

**Architecture:** A new `assistant/` Rust module with an `AssistantBrain` that gathers data from all sources, sends context to Claude for synthesis, and returns actionable insights. A startup briefing cron job runs on app launch. Proactive notifications use Tauri's notification API. Natural language actions are parsed by the AI with structured response patterns (like task creation).

**Tech Stack:** Existing AI router, SQLite, cron engine, Tauri event system + notifications.

**Depends on:** All prior phases complete.

---

## File Structure

```
jarvis/
├── src/
│   ├── components/
│   │   ├── Briefing.tsx                    # NEW: morning briefing overlay
│   │   └── NotificationBanner.tsx          # NEW: proactive notification bar
│   ├── pages/
│   │   └── Dashboard.tsx                   # Update: show briefing on load
│   └── lib/
│       ├── types.ts                        # + Briefing, Notification types
│       └── commands.ts                     # + briefing, action commands
│
└── src-tauri/
    └── src/
        ├── assistant/
        │   ├── mod.rs                      # AssistantBrain -- gathers context, synthesizes
        │   ├── briefing.rs                 # Morning briefing generator
        │   ├── actions.rs                  # Natural language action parser + executor
        │   └── context.rs                  # Context builder -- collects all data for AI
        ├── commands/
        │   └── assistant.rs                # NEW: Tauri commands for briefing + actions
        ├── scheduler/
        │   └── jobs.rs                     # + briefing job, notification checks
        └── lib.rs                          # + assistant module, register commands
```

---

## Task 1: Context Builder

**Files:**
- Create: `jarvis/src-tauri/src/assistant/context.rs`
- Create: `jarvis/src-tauri/src/assistant/mod.rs`

- [ ] **Step 1: Create assistant/context.rs**

This gathers all relevant data from SQLite into a single context string that gets sent to the AI.

```rust
// jarvis/src-tauri/src/assistant/context.rs
use crate::db::Database;
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
        let pending_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status != 'completed'", [], |r| r.get(0)
        ).unwrap_or(0);

        let overdue_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline < date('now')",
            [], |r| r.get(0)
        ).unwrap_or(0);

        let due_today: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline = date('now')",
            [], |r| r.get(0)
        ).unwrap_or(0);

        let urgent_tasks = {
            let mut stmt = conn.prepare(
                "SELECT title, deadline FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline <= date('now', '+3 days') ORDER BY deadline ASC LIMIT 5"
            ).map_err(|e| e.to_string())?;
            let tasks: Vec<String> = stmt.query_map([], |row| {
                let title: String = row.get(0)?;
                let deadline: String = row.get(1)?;
                Ok(format!("- {} (due {})", title, deadline))
            }).map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            tasks.join("\n")
        };

        let tasks_summary = format!(
            "{} pending tasks. {} overdue. {} due today.\n{}",
            pending_count, overdue_count, due_today, urgent_tasks
        );

        // Calendar
        let todays_events = {
            let mut stmt = conn.prepare(
                "SELECT summary, start_time, end_time, location FROM calendar_events WHERE date(start_time) = date('now') ORDER BY start_time ASC"
            ).map_err(|e| e.to_string())?;
            let events: Vec<String> = stmt.query_map([], |row| {
                let summary: String = row.get(0)?;
                let start: String = row.get(1)?;
                let location: Option<String> = row.get(3)?;
                let loc = location.map(|l| format!(" at {}", l)).unwrap_or_default();
                Ok(format!("- {} ({}{})", summary, start, loc))
            }).map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            if events.is_empty() { "No meetings today.".to_string() } else { events.join("\n") }
        };

        // Email
        let unread: i64 = conn.query_row(
            "SELECT COUNT(*) FROM emails WHERE is_read = 0", [], |r| r.get(0)
        ).unwrap_or(0);

        let important_senders = {
            let mut stmt = conn.prepare(
                "SELECT sender, subject FROM emails WHERE is_read = 0 ORDER BY received_at DESC LIMIT 3"
            ).map_err(|e| e.to_string())?;
            let emails: Vec<String> = stmt.query_map([], |row| {
                let sender: String = row.get(0)?;
                let subject: Option<String> = row.get(1)?;
                Ok(format!("- {} -- {}", sender, subject.unwrap_or_default()))
            }).map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            emails.join("\n")
        };

        let email_summary = format!("{} unread emails.\n{}", unread, important_senders);

        // GitHub
        let open_prs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM github_items WHERE item_type IN ('pr', 'pr_review') AND state != 'closed'",
            [], |r| r.get(0)
        ).unwrap_or(0);

        let assigned_issues: i64 = conn.query_row(
            "SELECT COUNT(*) FROM github_items WHERE item_type = 'issue' AND state = 'open'",
            [], |r| r.get(0)
        ).unwrap_or(0);

        let github_summary = format!("{} open PRs, {} assigned issues.", open_prs, assigned_issues);

        // Deadlines within 3 days
        let deadlines = urgent_tasks.clone();

        use chrono::Timelike;

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
            self.tasks_summary, self.calendar_summary, self.email_summary,
            self.github_summary, self.deadlines
        )
    }
}
```

- [ ] **Step 2: Create assistant/mod.rs**

```rust
pub mod actions;
pub mod briefing;
pub mod context;
```

Create placeholder files:
- `jarvis/src-tauri/src/assistant/briefing.rs` -- empty for now
- `jarvis/src-tauri/src/assistant/actions.rs` -- empty for now

- [ ] **Step 3: Commit**

```bash
git add jarvis/src-tauri/src/assistant/
git commit -m "feat: add assistant context builder for gathering daily status"
```

---

## Task 2: Morning Briefing

**Files:**
- Create: `jarvis/src-tauri/src/assistant/briefing.rs`
- Create: `jarvis/src-tauri/src/commands/assistant.rs`
- Modify: `jarvis/src-tauri/src/commands/mod.rs`
- Modify: `jarvis/src-tauri/src/lib.rs`

- [ ] **Step 1: Create assistant/briefing.rs**

```rust
// jarvis/src-tauri/src/assistant/briefing.rs
use crate::ai::AiRouter;
use crate::assistant::context::DayContext;
use crate::db::Database;
use std::sync::Arc;

pub async fn generate_briefing(
    db: &Arc<Database>,
    router: &AiRouter,
) -> Result<BriefingResult, String> {
    let context = DayContext::gather(db)?;
    let prompt = context.to_prompt();

    let messages = vec![("user".to_string(), prompt)];
    let briefing_text = router.send(messages).await?;

    Ok(BriefingResult {
        greeting: context.greeting,
        briefing: briefing_text,
        has_overdue: context.tasks_summary.contains("overdue"),
        task_count: extract_number(&context.tasks_summary),
    })
}

#[derive(serde::Serialize, Clone)]
pub struct BriefingResult {
    pub greeting: String,
    pub briefing: String,
    pub has_overdue: bool,
    pub task_count: i64,
}

fn extract_number(s: &str) -> i64 {
    s.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0)
}
```

- [ ] **Step 2: Create commands/assistant.rs**

```rust
// jarvis/src-tauri/src/commands/assistant.rs
use crate::ai::AiRouter;
use crate::assistant::{briefing, context::DayContext};
use crate::db::Database;
use crate::voice::tts::TextToSpeech;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn get_briefing(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
) -> Result<briefing::BriefingResult, String> {
    briefing::generate_briefing(&db, &router).await
}

#[tauri::command]
pub async fn speak_briefing(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
) -> Result<briefing::BriefingResult, String> {
    let result = briefing::generate_briefing(&db, &router).await?;

    // Speak the briefing
    let tts = TextToSpeech::new();
    let speech = format!("{}. {}", result.greeting, result.briefing);
    if let Err(e) = tts.speak(&speech).await {
        log::warn!("Briefing TTS failed: {}", e);
    }

    Ok(result)
}

#[tauri::command]
pub async fn ask_jarvis(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    question: String,
) -> Result<String, String> {
    // Build context + question
    let context = DayContext::gather(&db)?;
    let prompt = format!(
        "Here is the user's current status:\n\n\
         TASKS:\n{}\n\nCALENDAR:\n{}\n\nEMAIL:\n{}\n\nGITHUB:\n{}\n\n\
         The user asks: \"{}\"\n\n\
         Answer based on the data above. Be specific and actionable. \
         If the user asks what to work on, prioritize by urgency and deadlines. \
         Keep response concise.",
        context.tasks_summary, context.calendar_summary,
        context.email_summary, context.github_summary, question
    );

    let messages = vec![("user".to_string(), prompt)];
    router.send(messages).await
}
```

- [ ] **Step 3: Register in commands/mod.rs and lib.rs**

Add `pub mod assistant;` to `commands/mod.rs`.

Add to lib.rs invoke_handler:
```rust
commands::assistant::get_briefing,
commands::assistant::speak_briefing,
commands::assistant::ask_jarvis,
```

Also add `pub mod assistant;` to the module list in lib.rs.

- [ ] **Step 4: Verify**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 5: Commit**

```bash
git add jarvis/src-tauri/src/assistant/ jarvis/src-tauri/src/commands/assistant.rs jarvis/src-tauri/src/commands/mod.rs jarvis/src-tauri/src/lib.rs
git commit -m "feat: add morning briefing generator with AI-synthesized daily summary"
```

---

## Task 3: Natural Language Actions

**Files:**
- Create: `jarvis/src-tauri/src/assistant/actions.rs`

- [ ] **Step 1: Create actions.rs**

This parses structured action responses from the AI and executes them.

```rust
// jarvis/src-tauri/src/assistant/actions.rs
use crate::db::Database;
use std::sync::Arc;

#[derive(serde::Serialize, Clone, Debug)]
pub struct ActionResult {
    pub action_taken: String,
    pub details: String,
    pub success: bool,
}

/// Parse and execute actions from AI response text
/// Supported patterns:
/// [TASK:title|description|deadline|priority]  -- create task
/// [REMIND:title|datetime]                     -- create task with deadline
/// [NOTE:content]                              -- save to conversations as a note
pub fn execute_actions(response: &str, db: &Arc<Database>) -> (String, Vec<ActionResult>) {
    let mut clean_lines = Vec::new();
    let mut actions = Vec::new();

    for line in response.lines() {
        if line.starts_with("[TASK:") && line.ends_with("]") {
            let inner = &line[6..line.len()-1];
            let parts: Vec<&str> = inner.splitn(4, '|').collect();
            if let Some(title) = parts.first() {
                let description = parts.get(1).and_then(|d| if d.is_empty() { None } else { Some(d.to_string()) });
                let deadline = parts.get(2).and_then(|d| if d.is_empty() { None } else { Some(d.to_string()) });
                let priority: i32 = parts.get(3).and_then(|p| p.parse().ok()).unwrap_or(1);

                match create_task(db, title, description.as_deref(), deadline.as_deref(), priority) {
                    Ok(_) => actions.push(ActionResult { action_taken: "task_created".into(), details: title.to_string(), success: true }),
                    Err(e) => actions.push(ActionResult { action_taken: "task_created".into(), details: e, success: false }),
                }
            }
        } else if line.starts_with("[REMIND:") && line.ends_with("]") {
            let inner = &line[8..line.len()-1];
            let parts: Vec<&str> = inner.splitn(2, '|').collect();
            if let Some(title) = parts.first() {
                let deadline = parts.get(1).map(|d| d.to_string());
                match create_task(db, title, None, deadline.as_deref(), 2) {
                    Ok(_) => actions.push(ActionResult { action_taken: "reminder_created".into(), details: title.to_string(), success: true }),
                    Err(e) => actions.push(ActionResult { action_taken: "reminder_created".into(), details: e, success: false }),
                }
            }
        } else {
            clean_lines.push(line);
        }
    }

    (clean_lines.join("\n"), actions)
}

fn create_task(db: &Arc<Database>, title: &str, description: Option<&str>, deadline: Option<&str>, priority: i32) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO tasks (title, description, deadline, priority) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![title, description, deadline, priority],
    ).map_err(|e| e.to_string())?;
    log::info!("Action: created task '{}'", title);
    Ok(())
}
```

- [ ] **Step 2: Update chat.rs to use actions module**

Read `jarvis/src-tauri/src/commands/chat.rs`. Replace the existing `parse_task_from_response` function and its usage with the new actions module:

Replace the call to `parse_task_from_response` with:
```rust
let (clean_response, actions) = crate::assistant::actions::execute_actions(&response, &db);
let final_response = clean_response;
```

Remove the old `parse_task_from_response` function.

- [ ] **Step 3: Verify and commit**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
git add jarvis/src-tauri/src/assistant/actions.rs jarvis/src-tauri/src/commands/chat.rs
git commit -m "feat: add natural language action parser with task and reminder creation"
```

---

## Task 4: Proactive Notification Job

**Files:**
- Modify: `jarvis/src-tauri/src/scheduler/jobs.rs`

- [ ] **Step 1: Add notification check job**

Read `jarvis/src-tauri/src/scheduler/jobs.rs` and add a new match arm:
```rust
"proactive_check" => run_proactive_check(db).await,
```

Add the function:
```rust
async fn run_proactive_check(db: &Arc<Database>) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut alerts = Vec::new();

    // Check overdue tasks
    let overdue: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline IS NOT NULL AND deadline < date('now')",
        [], |r| r.get(0)
    ).unwrap_or(0);
    if overdue > 0 {
        alerts.push(format!("{} overdue task(s)", overdue));
    }

    // Check tasks due today
    let due_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status != 'completed' AND deadline = date('now')",
        [], |r| r.get(0)
    ).unwrap_or(0);
    if due_today > 0 {
        alerts.push(format!("{} task(s) due today", due_today));
    }

    // Check upcoming meeting (within 15 min)
    let upcoming: Option<String> = conn.query_row(
        "SELECT summary FROM calendar_events WHERE start_time > datetime('now') AND start_time <= datetime('now', '+15 minutes') ORDER BY start_time ASC LIMIT 1",
        [], |row| row.get(0)
    ).ok();
    if let Some(meeting) = upcoming {
        alerts.push(format!("Meeting in 15 min: {}", meeting));
    }

    // Check failed cron jobs
    let failed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cron_runs WHERE status = 'failed' AND started_at > datetime('now', '-1 hour')",
        [], |r| r.get(0)
    ).unwrap_or(0);
    if failed > 0 {
        alerts.push(format!("{} cron job(s) failed in the last hour", failed));
    }

    if alerts.is_empty() {
        Ok("No alerts".to_string())
    } else {
        // Store alerts for frontend to pick up
        let alert_text = alerts.join("; ");
        conn.execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES ('last_alerts', ?1, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
            rusqlite::params![alert_text],
        ).map_err(|e| e.to_string())?;

        Ok(format!("Alerts: {}", alert_text))
    }
}
```

- [ ] **Step 2: Add V5 migration to seed the proactive check job**

Create `jarvis/src-tauri/migrations/V5__proactive_check.sql`:
```sql
INSERT OR IGNORE INTO cron_jobs (name, schedule, action_type, parameters, status)
VALUES ('Proactive Check', '0 */1 * * * *', 'proactive_check', NULL, 'active');
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/src-tauri/src/scheduler/jobs.rs jarvis/src-tauri/migrations/V5__proactive_check.sql
git commit -m "feat: add proactive notification check job (overdue, meetings, failures)"
```

---

## Task 5: Frontend -- Briefing + Notifications + Smart Chat

**Files:**
- Create: `jarvis/src/components/Briefing.tsx`
- Create: `jarvis/src/components/NotificationBanner.tsx`
- Modify: `jarvis/src/lib/types.ts`
- Modify: `jarvis/src/lib/commands.ts`
- Modify: `jarvis/src/pages/Dashboard.tsx`
- Modify: `jarvis/src/hooks/useChat.ts`

- [ ] **Step 1: Add types**

Append to `jarvis/src/lib/types.ts`:
```ts
export interface BriefingResult {
  greeting: string;
  briefing: string;
  has_overdue: boolean;
  task_count: number;
}
```

- [ ] **Step 2: Add commands**

Append to `jarvis/src/lib/commands.ts`:
```ts
// Assistant
export async function getBriefing(): Promise<BriefingResult> { return invoke("get_briefing"); }
export async function speakBriefing(): Promise<BriefingResult> { return invoke("speak_briefing"); }
export async function askJarvis(question: string): Promise<string> { return invoke("ask_jarvis", { question }); }
```

Update the import line to include `BriefingResult`.

- [ ] **Step 3: Create Briefing.tsx**

```tsx
// jarvis/src/components/Briefing.tsx
import { useState, useEffect } from "react";
import type { BriefingResult } from "../lib/types";
import { getBriefing, speakBriefing } from "../lib/commands";

export default function Briefing() {
  const [briefing, setBriefing] = useState<BriefingResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [dismissed, setDismissed] = useState(false);
  const [speaking, setSpeaking] = useState(false);

  useEffect(() => {
    getBriefing()
      .then(setBriefing)
      .catch((e) => console.error("Briefing failed:", e))
      .finally(() => setLoading(false));
  }, []);

  if (dismissed || (!loading && !briefing)) return null;

  async function handleSpeak() {
    setSpeaking(true);
    try { await speakBriefing(); }
    catch (e) { console.error(e); }
    finally { setSpeaking(false); }
  }

  return (
    <div style={styles.container} className="animate-fade-in">
      <div style={styles.header}>
        <span className="system-text">DAILY BRIEFING</span>
        <div style={styles.actions}>
          <button onClick={handleSpeak} disabled={speaking} style={styles.speakBtn}>
            {speaking ? "SPEAKING..." : "SPEAK"}
          </button>
          <button onClick={() => setDismissed(true)} style={styles.dismissBtn}>DISMISS</button>
        </div>
      </div>
      {loading ? (
        <div className="system-text animate-glow" style={{ padding: 12 }}>GENERATING BRIEFING...</div>
      ) : briefing && (
        <div style={styles.body}>
          <div style={styles.greeting}>{briefing.greeting}</div>
          <div style={styles.text}>{briefing.briefing}</div>
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { border: "1px solid rgba(0, 180, 255, 0.2)", borderRadius: 8, background: "rgba(0, 180, 255, 0.03)", marginBottom: 16, overflow: "hidden" },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "8px 12px", borderBottom: "1px solid rgba(0, 180, 255, 0.1)" },
  actions: { display: "flex", gap: 6 },
  speakBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 4, padding: "3px 8px", color: "rgba(0, 180, 255, 0.8)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  dismissBtn: { background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "3px 8px", color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  body: { padding: 12 },
  greeting: { color: "rgba(0, 180, 255, 0.7)", fontSize: 14, fontWeight: 300, marginBottom: 8 },
  text: { color: "rgba(0, 180, 255, 0.6)", fontSize: 12, lineHeight: 1.6, whiteSpace: "pre-wrap" as const },
};
```

- [ ] **Step 4: Create NotificationBanner.tsx**

```tsx
// jarvis/src/components/NotificationBanner.tsx
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { Settings } from "../lib/types";

export default function NotificationBanner() {
  const { data: settings } = useTauriCommand<Settings>("get_settings");
  const alerts = settings?.values["last_alerts"];

  if (!alerts) return null;

  return (
    <div style={styles.banner}>
      <span style={styles.label}>ALERT</span>
      <span style={styles.text}>{alerts}</span>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  banner: { display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", background: "rgba(255, 100, 100, 0.04)", border: "1px solid rgba(255, 100, 100, 0.15)", borderRadius: 6, marginBottom: 8 },
  label: { color: "rgba(255, 100, 100, 0.8)", fontSize: 8, fontFamily: "var(--font-mono)", letterSpacing: 1.5, flexShrink: 0 },
  text: { color: "rgba(255, 100, 100, 0.7)", fontSize: 11 },
};
```

- [ ] **Step 5: Update Dashboard.tsx**

Read `jarvis/src/pages/Dashboard.tsx` and add:
1. Import `Briefing` and `NotificationBanner`
2. Render `<Briefing />` at the top of the main area (before GreetingHeader)
3. Render `<NotificationBanner />` between GreetingHeader and EmailRuleSuggestion

- [ ] **Step 6: Commit**

```bash
git add jarvis/src/components/Briefing.tsx jarvis/src/components/NotificationBanner.tsx jarvis/src/lib/types.ts jarvis/src/lib/commands.ts jarvis/src/pages/Dashboard.tsx
git commit -m "feat: add morning briefing, notification banner, and smart assistant UI"
```

---

## Summary

After completing all 5 tasks:

- **Morning Briefing**: On app open, JARVIS gathers all your data and generates an AI-synthesized briefing. Click "SPEAK" to hear it aloud.
- **Smart Context Chat**: `ask_jarvis` command sends your question with full context (tasks, calendar, email, GitHub), so JARVIS can answer "what should I work on?" with actual data.
- **Natural Language Actions**: AI responses can create tasks and reminders automatically via `[TASK:]` and `[REMIND:]` patterns.
- **Proactive Alerts**: Every minute, checks for overdue tasks, upcoming meetings (15 min), due-today items, and failed cron jobs. Shows a red alert banner on the dashboard.
- **Briefing job**: Seeded as V5 migration, runs every minute for proactive checks.
