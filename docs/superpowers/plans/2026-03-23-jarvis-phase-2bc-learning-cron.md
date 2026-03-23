# JARVIS Phase 2b+2c: Email Learning & Custom Cron Jobs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add rule-based email auto-archive suggestions (when user archives same sender 3+ times) and natural language custom cron job creation (user says "every Monday check email" and AI parses it into a cron schedule).

**Architecture:** Email learning: track archive actions in a new `email_rules` table, increment a counter per sender, surface suggestions when threshold (3) is reached. Custom cron jobs: new Tauri command that sends the user's natural language description to the AI router, which returns a structured JSON with cron schedule + action type, persisted to `cron_jobs` table and registered with the running scheduler.

**Tech Stack:** Existing rusqlite, AI router (Claude/OpenAI), tokio-cron-scheduler, React + TypeScript.

**Spec:** `docs/superpowers/specs/2026-03-23-jarvis-assistant-design.md`

**Depends on:** Phase 1 + 2a complete.

---

## File Structure (new/modified files only)

```
jarvis/
├── src/
│   ├── lib/
│   │   ├── types.ts                          # + EmailRule, CustomCronRequest types
│   │   └── commands.ts                       # + email rule + custom cron commands
│   ├── components/
│   │   └── EmailRuleSuggestion.tsx            # NEW: banner suggesting auto-archive
│   └── pages/
│       ├── Settings.tsx                       # Update: show active email rules
│       └── CronDashboard.tsx                  # Update: add "Create Job" button
│
└── src-tauri/
    ├── migrations/
    │   └── V4__email_rules.sql               # NEW: email_rules table
    └── src/
        ├── commands/
        │   ├── email.rs                      # Update: track archives, get rules, apply/dismiss rules
        │   └── cron.rs                       # Update: create_custom_cron command
        └── scheduler/
            └── jobs.rs                       # Update: auto_archive_emails job type
```

---

## Task 1: V4 Migration -- Email Rules Table

**Files:**
- Create: `jarvis/src-tauri/migrations/V4__email_rules.sql`

- [ ] **Step 1: Create migration**

```sql
-- jarvis/src-tauri/migrations/V4__email_rules.sql

CREATE TABLE IF NOT EXISTS email_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sender TEXT NOT NULL UNIQUE,
    archive_count INTEGER NOT NULL DEFAULT 0,
    rule_status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- rule_status values: 'pending' (counting), 'suggested' (threshold reached), 'active' (user accepted), 'dismissed' (user rejected)
CREATE INDEX idx_email_rules_sender ON email_rules(sender);
CREATE INDEX idx_email_rules_status ON email_rules(rule_status);
```

- [ ] **Step 2: Verify**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant/jarvis
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant
git add jarvis/src-tauri/migrations/V4__email_rules.sql
git commit -m "feat: add V4 migration for email_rules table"
```

---

## Task 2: Email Archive Tracking & Rule Suggestions

**Files:**
- Modify: `jarvis/src-tauri/src/commands/email.rs`

- [ ] **Step 1: Add rule types and tracking logic**

Read `jarvis/src-tauri/src/commands/email.rs` and add these new structs and commands:

```rust
#[derive(Serialize)]
pub struct EmailRule {
    pub id: i64,
    pub sender: String,
    pub archive_count: i64,
    pub rule_status: String,
}

/// Called after archiving an email -- increments the sender's archive count
/// and promotes to 'suggested' when threshold (3) is reached
fn track_archive(db: &Database, sender: &str) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;

    // Upsert: increment count or insert new
    conn.execute(
        "INSERT INTO email_rules (sender, archive_count, updated_at)
         VALUES (?1, 1, datetime('now'))
         ON CONFLICT(sender) DO UPDATE SET
            archive_count = archive_count + 1,
            updated_at = datetime('now')",
        rusqlite::params![sender],
    ).map_err(|e| e.to_string())?;

    // Promote to 'suggested' if count >= 3 and still 'pending'
    conn.execute(
        "UPDATE email_rules SET rule_status = 'suggested', updated_at = datetime('now')
         WHERE sender = ?1 AND archive_count >= 3 AND rule_status = 'pending'",
        rusqlite::params![sender],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn get_suggested_rules(db: State<Arc<Database>>) -> Result<Vec<EmailRule>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, sender, archive_count, rule_status FROM email_rules
         WHERE rule_status = 'suggested' ORDER BY archive_count DESC"
    ).map_err(|e| e.to_string())?;

    let rules = stmt.query_map([], |row| {
        Ok(EmailRule { id: row.get(0)?, sender: row.get(1)?, archive_count: row.get(2)?, rule_status: row.get(3)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(rules)
}

#[tauri::command]
pub fn accept_email_rule(db: State<Arc<Database>>, rule_id: i64) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE email_rules SET rule_status = 'active', updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![rule_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn dismiss_email_rule(db: State<Arc<Database>>, rule_id: i64) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE email_rules SET rule_status = 'dismissed', updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![rule_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_active_rules(db: State<Arc<Database>>) -> Result<Vec<EmailRule>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, sender, archive_count, rule_status FROM email_rules
         WHERE rule_status = 'active' ORDER BY sender ASC"
    ).map_err(|e| e.to_string())?;

    let rules = stmt.query_map([], |row| {
        Ok(EmailRule { id: row.get(0)?, sender: row.get(1)?, archive_count: row.get(2)?, rule_status: row.get(3)? })
    }).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(rules)
}
```

Also **modify the existing `archive_email` command** to track the sender after archiving. Read the current implementation. After the `gmail::archive_message` call succeeds, look up the sender from the emails table and call `track_archive`:

```rust
#[tauri::command]
pub async fn archive_email(
    auth: State<'_, Arc<GoogleAuth>>,
    db: State<'_, Arc<Database>>,
    gmail_id: String,
) -> Result<(), String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    gmail::archive_message(&token, &gmail_id).await?;

    // Track sender for rule learning
    let sender = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT sender FROM emails WHERE gmail_id = ?1",
            rusqlite::params![gmail_id], |row| row.get::<_, String>(0),
        ).ok()
    };
    if let Some(sender) = sender {
        track_archive(&db, &sender)?;
    }

    Ok(())
}
```

Note: the `archive_email` command signature now needs `db: State<'_, Arc<Database>>` added as a parameter.

- [ ] **Step 2: Register new commands in lib.rs**

Read `jarvis/src-tauri/src/lib.rs` and add to invoke_handler:
```rust
commands::email::get_suggested_rules,
commands::email::accept_email_rule,
commands::email::dismiss_email_rule,
commands::email::get_active_rules,
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 4: Commit**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant
git add jarvis/src-tauri/src/commands/email.rs jarvis/src-tauri/src/lib.rs
git commit -m "feat: add email archive tracking and rule suggestion system"
```

---

## Task 3: Auto-Archive Cron Job

**Files:**
- Modify: `jarvis/src-tauri/src/scheduler/jobs.rs`

- [ ] **Step 1: Add auto_archive job handler**

Read `jarvis/src-tauri/src/scheduler/jobs.rs` and add a new match arm and function.

Add to the match in `run_job`:
```rust
"auto_archive_emails" => run_auto_archive(db, auth).await,
```

Add the function:
```rust
async fn run_auto_archive(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let token = match auth.get_access_token() {
        Some(t) => t,
        None => return Ok("Skipped: not authenticated".to_string()),
    };

    // Get active rules (senders to auto-archive)
    let active_senders: Vec<String> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare("SELECT sender FROM email_rules WHERE rule_status = 'active'")
            .map_err(|e| e.to_string())?;
        stmt.query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    if active_senders.is_empty() {
        return Ok("No active archive rules".to_string());
    }

    // Find unarchived emails from those senders
    let to_archive: Vec<String> = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let placeholders: Vec<String> = active_senders.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT gmail_id FROM emails WHERE sender IN ({}) AND labels LIKE '%INBOX%'",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let params: Vec<&dyn rusqlite::types::ToSql> = active_senders.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        stmt.query_map(params.as_slice(), |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    let mut archived = 0;
    for gmail_id in &to_archive {
        match gmail::archive_message(&token, gmail_id).await {
            Ok(_) => archived += 1,
            Err(e) => log::warn!("Failed to auto-archive {}: {}", gmail_id, e),
        }
    }

    Ok(format!("Auto-archived {} emails from {} rules", archived, active_senders.len()))
}
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant
git add jarvis/src-tauri/src/scheduler/jobs.rs
git commit -m "feat: add auto-archive cron job for email rules"
```

---

## Task 4: Custom Cron Job Creation via Natural Language

**Files:**
- Modify: `jarvis/src-tauri/src/commands/cron.rs`
- Modify: `jarvis/src-tauri/src/lib.rs` (register command)

- [ ] **Step 1: Add create_custom_cron command**

Read `jarvis/src-tauri/src/commands/cron.rs` and add:

```rust
use crate::ai::AiRouter;

#[tauri::command]
pub async fn create_custom_cron(
    db: State<'_, Arc<Database>>,
    router: State<'_, AiRouter>,
    description: String,
) -> Result<CronJobView, String> {
    // Ask AI to parse natural language into cron schedule + action
    let prompt = format!(
        "Parse this scheduling request into a JSON object. Supported schedule patterns: daily, weekly, monthly, every N hours/days. \
         Supported action_types: email_sync, calendar_sync, deadline_monitor, notion_sync, github_digest, auto_archive_emails. \
         Return ONLY valid JSON with these fields: \
         {{\"name\": \"short name\", \"schedule\": \"cron expression (6-field: sec min hour day month weekday)\", \"action_type\": \"one of the supported types\"}} \
         \nRequest: \"{}\"", description
    );

    let messages = vec![("user".to_string(), prompt)];
    let response = router.send(messages).await?;

    // Parse AI response as JSON
    let parsed: serde_json::Value = serde_json::from_str(response.trim().trim_start_matches("```json").trim_end_matches("```").trim())
        .map_err(|e| format!("Failed to parse AI response as JSON: {}. Response was: {}", e, response))?;

    let name = parsed["name"].as_str().ok_or("Missing 'name' in AI response")?.to_string();
    let schedule = parsed["schedule"].as_str().ok_or("Missing 'schedule' in AI response")?.to_string();
    let action_type = parsed["action_type"].as_str().ok_or("Missing 'action_type' in AI response")?.to_string();

    // Validate action_type
    let valid_actions = ["email_sync", "calendar_sync", "deadline_monitor", "notion_sync", "github_digest", "auto_archive_emails"];
    if !valid_actions.contains(&action_type.as_str()) {
        return Err(format!("Invalid action_type '{}'. Must be one of: {}", action_type, valid_actions.join(", ")));
    }

    // Insert into database
    let job_id = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO cron_jobs (name, schedule, action_type, status) VALUES (?1, ?2, ?3, 'active')",
            rusqlite::params![name, schedule, action_type],
        ).map_err(|e| e.to_string())?;
        conn.last_insert_rowid()
    };

    log::info!("Created custom cron job: {} ({}) -> {}", name, schedule, action_type);

    Ok(CronJobView {
        id: job_id,
        name,
        schedule,
        action_type,
        status: "active".to_string(),
        last_run: None,
        next_run: None,
    })
}

#[tauri::command]
pub fn delete_cron_job(db: State<Arc<Database>>, job_id: i64) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM cron_jobs WHERE id = ?1", rusqlite::params![job_id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM cron_runs WHERE job_id = ?1", rusqlite::params![job_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn toggle_cron_job(db: State<Arc<Database>>, job_id: i64) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let current: String = conn.query_row(
        "SELECT status FROM cron_jobs WHERE id = ?1", rusqlite::params![job_id], |row| row.get(0)
    ).map_err(|e| e.to_string())?;

    let new_status = if current == "active" { "paused" } else { "active" };
    conn.execute(
        "UPDATE cron_jobs SET status = ?1 WHERE id = ?2",
        rusqlite::params![new_status, job_id],
    ).map_err(|e| e.to_string())?;

    Ok(new_status.to_string())
}
```

- [ ] **Step 2: Register new commands in lib.rs**

Add to invoke_handler:
```rust
commands::cron::create_custom_cron,
commands::cron::delete_cron_job,
commands::cron::toggle_cron_job,
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 4: Commit**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant
git add jarvis/src-tauri/src/commands/cron.rs jarvis/src-tauri/src/lib.rs
git commit -m "feat: add custom cron job creation via natural language AI parsing"
```

---

## Task 5: Frontend Types & Commands

**Files:**
- Modify: `jarvis/src/lib/types.ts`
- Modify: `jarvis/src/lib/commands.ts`

- [ ] **Step 1: Add types**

Append to `jarvis/src/lib/types.ts`:
```ts
export interface EmailRule {
  id: number;
  sender: string;
  archive_count: number;
  rule_status: string;
}
```

- [ ] **Step 2: Add commands**

Append to `jarvis/src/lib/commands.ts` (update import line for `EmailRule`):
```ts
// Email Rules
export async function getSuggestedRules(): Promise<EmailRule[]> { return invoke("get_suggested_rules"); }
export async function acceptEmailRule(ruleId: number): Promise<void> { return invoke("accept_email_rule", { rule_id: ruleId }); }
export async function dismissEmailRule(ruleId: number): Promise<void> { return invoke("dismiss_email_rule", { rule_id: ruleId }); }
export async function getActiveRules(): Promise<EmailRule[]> { return invoke("get_active_rules"); }

// Custom Cron
export async function createCustomCron(description: string): Promise<CronJobView> { return invoke("create_custom_cron", { description }); }
export async function deleteCronJob(jobId: number): Promise<void> { return invoke("delete_cron_job", { job_id: jobId }); }
export async function toggleCronJob(jobId: number): Promise<string> { return invoke("toggle_cron_job", { job_id: jobId }); }
```

- [ ] **Step 3: Commit**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant
git add jarvis/src/lib/types.ts jarvis/src/lib/commands.ts
git commit -m "feat: add frontend types and commands for email rules and custom cron"
```

---

## Task 6: Email Rule Suggestion Banner

**Files:**
- Create: `jarvis/src/components/EmailRuleSuggestion.tsx`
- Modify: `jarvis/src/pages/Dashboard.tsx`

- [ ] **Step 1: Create EmailRuleSuggestion.tsx**

```tsx
import { useEffect, useState } from "react";
import type { EmailRule } from "../lib/types";
import { getSuggestedRules, acceptEmailRule, dismissEmailRule } from "../lib/commands";

export default function EmailRuleSuggestion() {
  const [rules, setRules] = useState<EmailRule[]>([]);

  useEffect(() => { getSuggestedRules().then(setRules); }, []);

  if (rules.length === 0) return null;

  async function handleAccept(id: number) {
    await acceptEmailRule(id);
    setRules((prev) => prev.filter((r) => r.id !== id));
  }

  async function handleDismiss(id: number) {
    await dismissEmailRule(id);
    setRules((prev) => prev.filter((r) => r.id !== id));
  }

  return (
    <div style={styles.container}>
      {rules.map((rule) => (
        <div key={rule.id} style={styles.banner}>
          <div style={styles.text}>
            <span style={styles.label}>AUTO-ARCHIVE SUGGESTION</span>
            <span style={styles.sender}>
              Emails from <strong>{rule.sender}</strong> archived {rule.archive_count} times
            </span>
          </div>
          <div style={styles.actions}>
            <button onClick={() => handleAccept(rule.id)} style={styles.acceptBtn}>ENABLE</button>
            <button onClick={() => handleDismiss(rule.id)} style={styles.dismissBtn}>DISMISS</button>
          </div>
        </div>
      ))}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", flexDirection: "column", gap: 6, marginBottom: 12 },
  banner: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "8px 12px", border: "1px solid rgba(255, 180, 0, 0.2)", borderRadius: 8, background: "rgba(255, 180, 0, 0.04)" },
  text: { display: "flex", flexDirection: "column", gap: 2 },
  label: { color: "rgba(255, 180, 0, 0.7)", fontSize: 8, fontFamily: "var(--font-mono)", letterSpacing: 1.5 },
  sender: { color: "rgba(0, 180, 255, 0.7)", fontSize: 11 },
  actions: { display: "flex", gap: 6 },
  acceptBtn: { background: "rgba(16, 185, 129, 0.1)", border: "1px solid rgba(16, 185, 129, 0.3)", borderRadius: 4, padding: "4px 10px", color: "rgba(16, 185, 129, 0.8)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  dismissBtn: { background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "4px 10px", color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
};
```

- [ ] **Step 2: Add to Dashboard**

Read `jarvis/src/pages/Dashboard.tsx` and:
1. Import `EmailRuleSuggestion`
2. Render `<EmailRuleSuggestion />` between GreetingHeader and Timeline

- [ ] **Step 3: Commit**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant
git add jarvis/src/components/EmailRuleSuggestion.tsx jarvis/src/pages/Dashboard.tsx
git commit -m "feat: add email auto-archive suggestion banner on dashboard"
```

---

## Task 7: Custom Cron Creation UI in CronDashboard

**Files:**
- Modify: `jarvis/src/pages/CronDashboard.tsx`

- [ ] **Step 1: Add create job form and management buttons**

Read `jarvis/src/pages/CronDashboard.tsx` and add:

1. Import `createCustomCron`, `deleteCronJob`, `toggleCronJob` from commands
2. Add state for the input: `const [newJobDesc, setNewJobDesc] = useState(""); const [creating, setCreating] = useState(false);`
3. Add a create handler:
```tsx
async function handleCreateJob() {
  if (!newJobDesc.trim() || creating) return;
  setCreating(true);
  try {
    await createCustomCron(newJobDesc);
    setNewJobDesc("");
    // Refetch jobs -- simplest: reload the component by toggling a key or calling refetch
    window.location.reload();
  } catch (e) {
    console.error(e);
  } finally {
    setCreating(false);
  }
}
```

4. Add a create form at the top of the job list section:
```tsx
<div style={{ marginBottom: 12, display: "flex", gap: 6 }}>
  <input type="text" value={newJobDesc} onChange={(e) => setNewJobDesc(e.target.value)}
    onKeyDown={(e) => e.key === "Enter" && handleCreateJob()}
    placeholder="e.g. Every Monday check email for spam..."
    style={{ flex: 1, background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "6px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)", outline: "none" }} />
  <button onClick={handleCreateJob} disabled={creating}
    style={{ background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: creating ? "wait" : "pointer", fontFamily: "var(--font-mono)", fontSize: 10, whiteSpace: "nowrap" }}>
    {creating ? "CREATING..." : "+ NEW JOB"}
  </button>
</div>
```

5. Add toggle/delete buttons to each job card (after the schedule info):
```tsx
<div style={{ display: "flex", gap: 4, marginTop: 6 }}>
  <button onClick={(e) => { e.stopPropagation(); toggleCronJob(job.id).then(() => window.location.reload()); }}
    style={{ background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "2px 6px", color: "rgba(0, 180, 255, 0.5)", fontSize: 8, fontFamily: "var(--font-mono)", cursor: "pointer" }}>
    {job.status === "active" ? "PAUSE" : "RESUME"}
  </button>
  <button onClick={(e) => { e.stopPropagation(); deleteCronJob(job.id).then(() => window.location.reload()); }}
    style={{ background: "transparent", border: "1px solid rgba(255, 100, 100, 0.2)", borderRadius: 4, padding: "2px 6px", color: "rgba(255, 100, 100, 0.5)", fontSize: 8, fontFamily: "var(--font-mono)", cursor: "pointer" }}>
    DELETE
  </button>
</div>
```

- [ ] **Step 2: Commit**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant
git add jarvis/src/pages/CronDashboard.tsx
git commit -m "feat: add custom cron job creation UI with natural language input"
```

---

## Summary

After completing all 7 tasks, Phase 2b+2c delivers:

- **Email learning**: archives tracked per sender, auto-archive suggested at 3+ archives, user can accept/dismiss
- **Auto-archive cron job**: runs active rules automatically when scheduled
- **Custom cron jobs**: type natural language like "Every Monday check email for spam" -- AI parses into cron schedule
- **Job management**: pause, resume, delete cron jobs from the UI
- **Dashboard banner**: shows email rule suggestions when threshold is reached
- **4 new email rule commands** + **3 new cron management commands**

**Phase 2 is now fully complete** after this plan executes.
