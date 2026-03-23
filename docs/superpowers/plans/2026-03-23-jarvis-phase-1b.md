# JARVIS Phase 1b: Email, Calendar & Cron Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Gmail and Google Calendar integrations via OAuth2, and a background cron engine that monitors deadlines and syncs data on a schedule.

**Architecture:** Each integration is a Rust module under `src-tauri/src/integrations/` with a standard interface. OAuth2 uses Google's loopback redirect flow (localhost). The cron engine runs as a tokio background task managed via `tokio-cron-scheduler`, persisting jobs to SQLite. New Tauri IPC commands expose integration data to the frontend. Dashboard components updated to show email counts, calendar events, and cron status.

**Tech Stack:** reqwest + serde (Google API calls), tokio-cron-scheduler (background jobs), oauth2 crate (Google OAuth2 PKCE flow), rusqlite (new migration tables), React + TypeScript (frontend updates).

**Spec:** `docs/superpowers/specs/2026-03-23-jarvis-assistant-design.md`

**Depends on:** Phase 1a complete (Tauri shell, SQLite, AI router, holographic UI).

---

## File Structure (new/modified files only)

```
jarvis/
├── src/
│   ├── lib/
│   │   ├── types.ts                          # + CalendarEvent, Email, CronJob, CronRun types
│   │   └── commands.ts                       # + email, calendar, cron command wrappers
│   ├── components/
│   │   ├── StatsPanel.tsx                    # Update: show live email/calendar/cron stats
│   │   ├── CalendarCard.tsx                  # NEW: upcoming events card for stats panel
│   │   └── CronCard.tsx                      # NEW: cron job status card
│   └── pages/
│       ├── Dashboard.tsx                     # Update: pass calendar + cron data
│       └── Settings.tsx                      # Update: Google OAuth connect buttons
├── .env                                      # + GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET
├── .env.example                              # + GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET
│
└── src-tauri/
    ├── Cargo.toml                            # + oauth2, tokio-cron-scheduler, uuid
    ├── migrations/
    │   └── V2__email_calendar_cron.sql       # NEW: emails, calendar_events, cron_jobs, cron_runs
    └── src/
        ├── lib.rs                            # + integrations, scheduler modules; start scheduler
        ├── auth/
        │   ├── mod.rs                        # OAuth2 manager
        │   └── google.rs                     # Google OAuth2 PKCE flow with loopback redirect
        ├── integrations/
        │   ├── mod.rs                        # Integration trait + re-exports
        │   ├── gmail.rs                      # Gmail API: fetch inbox, list messages, archive
        │   └── calendar.rs                   # Google Calendar API: list events, create event
        ├── scheduler/
        │   ├── mod.rs                        # Cron engine setup, job registry
        │   └── jobs.rs                       # Built-in jobs: deadline_monitor, email_sync, calendar_sync
        └── commands/
            ├── mod.rs                        # + email, calendar, cron modules
            ├── email.rs                      # NEW: get_emails, sync_emails, archive_email
            ├── calendar.rs                   # NEW: get_events, sync_calendar, create_event
            └── cron.rs                       # NEW: get_cron_jobs, get_cron_runs
```

---

## Task 1: Database Migration -- New Tables

**Files:**
- Create: `jarvis/src-tauri/migrations/V2__email_calendar_cron.sql`

- [ ] **Step 1: Create the V2 migration**

```sql
-- jarvis/src-tauri/migrations/V2__email_calendar_cron.sql

CREATE TABLE IF NOT EXISTS emails (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    gmail_id TEXT UNIQUE NOT NULL,
    thread_id TEXT,
    subject TEXT,
    sender TEXT NOT NULL,
    snippet TEXT,
    labels TEXT,
    importance_score INTEGER DEFAULT 0,
    is_spam INTEGER DEFAULT 0,
    is_read INTEGER DEFAULT 0,
    received_at TEXT NOT NULL,
    synced_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_emails_received ON emails(received_at DESC);
CREATE INDEX idx_emails_sender ON emails(sender);

CREATE TABLE IF NOT EXISTS calendar_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    google_id TEXT UNIQUE NOT NULL,
    summary TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    attendees TEXT,
    status TEXT DEFAULT 'confirmed',
    synced_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_calendar_start ON calendar_events(start_time);

CREATE TABLE IF NOT EXISTS cron_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    schedule TEXT NOT NULL,
    action_type TEXT NOT NULL,
    parameters TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    last_run TEXT,
    next_run TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS cron_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id INTEGER NOT NULL REFERENCES cron_jobs(id),
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    result TEXT,
    error TEXT
);

CREATE INDEX idx_cron_runs_job ON cron_runs(job_id, started_at DESC);

-- Seed built-in cron jobs
INSERT OR IGNORE INTO cron_jobs (name, schedule, action_type, parameters, status)
VALUES
    ('Email Sync', '0 */5 * * * *', 'email_sync', NULL, 'active'),
    ('Calendar Sync', '0 */5 * * * *', 'calendar_sync', NULL, 'active'),
    ('Deadline Monitor', '0 0 9 * * *', 'deadline_monitor', NULL, 'active');
```

- [ ] **Step 2: Verify migration applies**

```bash
cd /Users/hillmanchan/Desktop/claude_assistant/jarvis
cargo check --manifest-path src-tauri/Cargo.toml
```

Then run the app briefly to trigger migration, and verify:
```bash
sqlite3 ~/Library/Application\ Support/jarvis/jarvis.db ".tables"
```
Expected: `calendar_events  conversations  cron_jobs  cron_runs  emails  refinery_schema_history  tasks  user_preferences`

- [ ] **Step 3: Commit**

```bash
git add jarvis/src-tauri/migrations/V2__email_calendar_cron.sql
git commit -m "feat: add V2 migration for emails, calendar_events, cron_jobs, cron_runs"
```

---

## Task 2: Add New Cargo Dependencies

**Files:**
- Modify: `jarvis/src-tauri/Cargo.toml`

- [ ] **Step 1: Add dependencies**

Add to `[dependencies]` in `jarvis/src-tauri/Cargo.toml`:

```toml
oauth2 = { version = "4.4", features = ["reqwest"] }
tokio-cron-scheduler = "0.13"
uuid = { version = "1", features = ["v4"] }
open = "5"
```

- `oauth2`: Google OAuth2 PKCE flow (pin 4.4 for stable `reqwest::async_http_client`)
- `tokio-cron-scheduler`: Background cron engine
- `uuid`: Unique IDs for OAuth state
- `open`: Open browser for OAuth consent

- [ ] **Step 2: Verify it compiles**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/src-tauri/Cargo.toml
git commit -m "feat: add oauth2, tokio-cron-scheduler, uuid, open dependencies"
```

---

## Task 3: Google OAuth2 Authentication

**Files:**
- Create: `jarvis/src-tauri/src/auth/mod.rs`
- Create: `jarvis/src-tauri/src/auth/google.rs`
- Modify: `jarvis/src-tauri/src/lib.rs` (add `pub mod auth;`)
- Modify: `jarvis/.env.example` (add Google credentials)

- [ ] **Step 1: Update .env.example**

Add to `jarvis/.env.example`:
```
GOOGLE_CLIENT_ID=your-google-client-id
GOOGLE_CLIENT_SECRET=your-google-client-secret
```

- [ ] **Step 2: Create auth/mod.rs**

```rust
// jarvis/src-tauri/src/auth/mod.rs
pub mod google;
```

- [ ] **Step 3: Create auth/google.rs**

This implements Google OAuth2 with PKCE using the loopback redirect flow (http://127.0.0.1). It starts a temporary local HTTP server to receive the callback, opens the browser for consent, exchanges the code for tokens, and stores refresh tokens in the database.

```rust
// jarvis/src-tauri/src/auth/google.rs
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenUrl,
    AuthorizationCode, TokenResponse, RefreshToken, reqwest::async_http_client,
};
use std::sync::Mutex;
use tokio::sync::oneshot;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub struct GoogleAuth {
    client_id: String,
    client_secret: String,
    pub access_token: Mutex<Option<String>>,
    pub refresh_token: Mutex<Option<String>>,
}

impl GoogleAuth {
    pub fn new() -> Option<Self> {
        let client_id = std::env::var("GOOGLE_CLIENT_ID").ok()?;
        let client_secret = std::env::var("GOOGLE_CLIENT_SECRET").ok()?;
        Some(GoogleAuth {
            client_id,
            client_secret,
            access_token: Mutex::new(None),
            refresh_token: Mutex::new(None),
        })
    }

    fn build_client(&self, redirect_port: u16) -> Result<BasicClient, String> {
        let client = BasicClient::new(
            ClientId::new(self.client_id.clone()),
            Some(ClientSecret::new(self.client_secret.clone())),
            AuthUrl::new(AUTH_URL.to_string()).map_err(|e| e.to_string())?,
            Some(TokenUrl::new(TOKEN_URL.to_string()).map_err(|e| e.to_string())?),
        )
        .set_redirect_uri(
            RedirectUrl::new(format!("http://127.0.0.1:{}", redirect_port))
                .map_err(|e| e.to_string())?,
        );
        Ok(client)
    }

    pub async fn start_auth_flow(&self, scopes: Vec<String>) -> Result<(), String> {
        // Bind to a random available port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind listener: {}", e))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();

        let client = self.build_client(port)?;

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let mut auth_request = client.authorize_url(CsrfToken::new_random);
        for scope in &scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }
        auth_request = auth_request.add_extra_param("access_type", "offline");
        auth_request = auth_request.add_extra_param("prompt", "consent");

        let (auth_url, _csrf_token) = auth_request
            .set_pkce_challenge(pkce_challenge)
            .url();

        // Open browser
        open::that(auth_url.to_string()).map_err(|e| format!("Failed to open browser: {}", e))?;

        log::info!("Opened browser for Google OAuth at port {}", port);

        // Wait for the callback
        let (tx, rx) = oneshot::channel::<String>();
        let tx = std::sync::Mutex::new(Some(tx));

        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                if let Ok(n) = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await {
                    let request = String::from_utf8_lossy(&buf[..n]);
                    // Extract code from GET /?code=XXX&...
                    if let Some(code) = extract_code(&request) {
                        // Send success response to browser
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>JARVIS</h1><p>Authentication successful. You can close this tab.</p></body></html>";
                        let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                        if let Some(tx) = tx.lock().unwrap().take() {
                            let _ = tx.send(code);
                        }
                    }
                }
            }
        });

        let code = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            rx,
        )
        .await
        .map_err(|_| "OAuth timeout: no response within 120 seconds".to_string())?
        .map_err(|_| "OAuth channel closed".to_string())?;

        // Exchange code for tokens
        let token_result = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client)
            .await
            .map_err(|e| format!("Token exchange failed: {}", e))?;

        let access = token_result.access_token().secret().clone();
        let refresh = token_result.refresh_token().map(|t| t.secret().clone());

        *self.access_token.lock().unwrap() = Some(access);
        *self.refresh_token.lock().unwrap() = refresh;

        log::info!("Google OAuth completed successfully");
        Ok(())
    }

    pub async fn refresh_access_token(&self) -> Result<(), String> {
        let refresh = self.refresh_token.lock().unwrap().clone()
            .ok_or("No refresh token available")?;

        let client = self.build_client(0)?;

        let token_result = client
            .exchange_refresh_token(&RefreshToken::new(refresh))
            .request_async(async_http_client)
            .await
            .map_err(|e| format!("Token refresh failed: {}", e))?;

        *self.access_token.lock().unwrap() = Some(token_result.access_token().secret().clone());

        if let Some(new_refresh) = token_result.refresh_token() {
            *self.refresh_token.lock().unwrap() = Some(new_refresh.secret().clone());
        }

        log::info!("Google access token refreshed");
        Ok(())
    }

    pub fn get_access_token(&self) -> Option<String> {
        self.access_token.lock().unwrap().clone()
    }

    pub fn is_authenticated(&self) -> bool {
        self.access_token.lock().unwrap().is_some()
    }

    /// Load saved tokens from DB
    pub fn load_from_db(&self, db: &crate::db::Database) {
        let conn = db.conn.lock().unwrap();
        if let Ok(token) = conn.query_row(
            "SELECT value FROM user_preferences WHERE key = 'google_refresh_token'",
            [], |row| row.get::<_, String>(0),
        ) {
            *self.refresh_token.lock().unwrap() = Some(token);
        }
    }

    /// Save refresh token to DB
    pub fn save_to_db(&self, db: &crate::db::Database) {
        if let Some(ref token) = *self.refresh_token.lock().unwrap() {
            let conn = db.conn.lock().unwrap();
            let _ = conn.execute(
                "INSERT INTO user_preferences (key, value, updated_at) VALUES ('google_refresh_token', ?1, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = ?1, updated_at = datetime('now')",
                rusqlite::params![token],
            );
        }
    }
}

fn extract_code(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for param in query.split('&') {
        let mut parts = param.splitn(2, '=');
        if parts.next()? == "code" {
            return parts.next().map(|s| s.to_string());
        }
    }
    None
}
```

- [ ] **Step 4: Add `pub mod auth;` to lib.rs**

Read `jarvis/src-tauri/src/lib.rs` and add `pub mod auth;` to the module list.

- [ ] **Step 5: Verify compilation**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 6: Commit**

```bash
git add jarvis/src-tauri/src/auth/ jarvis/src-tauri/src/lib.rs jarvis/.env.example
git commit -m "feat: add Google OAuth2 PKCE authentication with loopback redirect"
```

---

## Task 4: Gmail Integration

**Files:**
- Create: `jarvis/src-tauri/src/integrations/mod.rs`
- Create: `jarvis/src-tauri/src/integrations/gmail.rs`
- Modify: `jarvis/src-tauri/src/lib.rs` (add `pub mod integrations;`)

- [ ] **Step 1: Create integrations/mod.rs**

```rust
// jarvis/src-tauri/src/integrations/mod.rs
pub mod gmail;
pub mod calendar;
```

Note: `calendar` module will be created in Task 5. For now, create a placeholder:
```rust
// jarvis/src-tauri/src/integrations/calendar.rs
// Placeholder -- implemented in Task 5
```

- [ ] **Step 2: Create integrations/gmail.rs**

```rust
// jarvis/src-tauri/src/integrations/gmail.rs
use reqwest::Client;
use serde::{Deserialize, Serialize};

const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

#[derive(Debug, Serialize, Deserialize)]
pub struct GmailMessage {
    pub id: String,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub snippet: Option<String>,
    pub label_ids: Vec<String>,
    pub is_read: bool,
    pub received_at: Option<String>,
}

#[derive(Deserialize)]
struct ListResponse {
    messages: Option<Vec<MessageRef>>,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

#[derive(Deserialize)]
struct MessageDetail {
    id: String,
    #[serde(rename = "threadId")]
    thread_id: Option<String>,
    snippet: Option<String>,
    #[serde(rename = "labelIds")]
    label_ids: Option<Vec<String>>,
    payload: Option<Payload>,
    #[serde(rename = "internalDate")]
    internal_date: Option<String>,
}

#[derive(Deserialize)]
struct Payload {
    headers: Option<Vec<Header>>,
}

#[derive(Deserialize)]
struct Header {
    name: String,
    value: String,
}

pub async fn fetch_inbox(
    access_token: &str,
    max_results: u32,
) -> Result<Vec<GmailMessage>, String> {
    let client = Client::new();
    let url = format!("{}/messages?maxResults={}&labelIds=INBOX", GMAIL_API, max_results);

    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Gmail list error: {}", e))?;

    if resp.status() == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("Gmail API error: {}", resp.status()));
    }

    let list: ListResponse = resp.json().await.map_err(|e| e.to_string())?;
    let refs = list.messages.unwrap_or_default();

    let mut messages = Vec::new();
    for msg_ref in refs.iter().take(max_results as usize) {
        match fetch_message_detail(access_token, &msg_ref.id).await {
            Ok(msg) => messages.push(msg),
            Err(e) => log::warn!("Failed to fetch message {}: {}", msg_ref.id, e),
        }
    }

    Ok(messages)
}

async fn fetch_message_detail(
    access_token: &str,
    message_id: &str,
) -> Result<GmailMessage, String> {
    let client = Client::new();
    let url = format!("{}/messages/{}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date", GMAIL_API, message_id);

    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let detail: MessageDetail = resp.json().await.map_err(|e| e.to_string())?;

    let headers = detail.payload.and_then(|p| p.headers).unwrap_or_default();
    let subject = headers.iter().find(|h| h.name == "Subject").map(|h| h.value.clone());
    let sender = headers.iter().find(|h| h.name == "From").map(|h| h.value.clone());
    let date = headers.iter().find(|h| h.name == "Date").map(|h| h.value.clone());

    let labels = detail.label_ids.unwrap_or_default();
    let is_read = !labels.contains(&"UNREAD".to_string());

    Ok(GmailMessage {
        id: detail.id,
        thread_id: detail.thread_id,
        subject,
        sender,
        snippet: detail.snippet,
        label_ids: labels,
        is_read,
        received_at: date.or(detail.internal_date),
    })
}

pub async fn archive_message(access_token: &str, message_id: &str) -> Result<(), String> {
    let client = Client::new();
    let url = format!("{}/messages/{}/modify", GMAIL_API, message_id);

    let body = serde_json::json!({
        "removeLabelIds": ["INBOX"]
    });

    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Gmail archive error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Gmail archive failed: {}", resp.status()));
    }

    Ok(())
}

/// Save fetched messages to the local database
pub fn save_to_db(db: &crate::db::Database, messages: &[GmailMessage]) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    for msg in messages {
        conn.execute(
            "INSERT INTO emails (gmail_id, thread_id, subject, sender, snippet, labels, is_read, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(gmail_id) DO UPDATE SET
                is_read = ?7, labels = ?6, synced_at = datetime('now')",
            rusqlite::params![
                msg.id,
                msg.thread_id,
                msg.subject,
                msg.sender,
                msg.snippet,
                msg.label_ids.join(","),
                msg.is_read as i32,
                msg.received_at,
            ],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}
```

- [ ] **Step 3: Add `pub mod integrations;` to lib.rs**

- [ ] **Step 4: Verify compilation**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 5: Commit**

```bash
git add jarvis/src-tauri/src/integrations/ jarvis/src-tauri/src/lib.rs
git commit -m "feat: add Gmail integration with inbox fetch, archive, and local caching"
```

---

## Task 5: Google Calendar Integration

**Files:**
- Replace: `jarvis/src-tauri/src/integrations/calendar.rs` (was placeholder)

- [ ] **Step 1: Create integrations/calendar.rs**

```rust
// jarvis/src-tauri/src/integrations/calendar.rs
use reqwest::Client;
use serde::{Deserialize, Serialize};

const CALENDAR_API: &str = "https://www.googleapis.com/calendar/v3";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CalendarEvent {
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_time: String,
    pub end_time: String,
    pub attendees: Vec<String>,
    pub status: String,
}

#[derive(Deserialize)]
struct EventsResponse {
    items: Option<Vec<GoogleEvent>>,
}

#[derive(Deserialize)]
struct GoogleEvent {
    id: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    start: Option<EventTime>,
    end: Option<EventTime>,
    attendees: Option<Vec<Attendee>>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct EventTime {
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
    date: Option<String>,
}

#[derive(Deserialize)]
struct Attendee {
    email: Option<String>,
}

impl EventTime {
    fn to_string_repr(&self) -> String {
        self.date_time.clone().or(self.date.clone()).unwrap_or_default()
    }
}

pub async fn fetch_events(
    access_token: &str,
    time_min: &str,
    time_max: &str,
) -> Result<Vec<CalendarEvent>, String> {
    let client = Client::new();
    let url = format!(
        "{}/calendars/primary/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=50",
        CALENDAR_API, time_min, time_max
    );

    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Calendar API error: {}", e))?;

    if resp.status() == 401 {
        return Err("UNAUTHORIZED".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("Calendar API error: {}", resp.status()));
    }

    let body: EventsResponse = resp.json().await.map_err(|e| e.to_string())?;
    let events = body.items.unwrap_or_default();

    Ok(events
        .into_iter()
        .filter_map(|e| {
            let start = e.start.as_ref()?.to_string_repr();
            let end = e.end.as_ref()?.to_string_repr();
            if start.is_empty() { return None; }
            Some(CalendarEvent {
                id: e.id.unwrap_or_default(),
                summary: e.summary.unwrap_or_else(|| "(No title)".to_string()),
                description: e.description,
                location: e.location,
                start_time: start,
                end_time: end,
                attendees: e.attendees.unwrap_or_default()
                    .into_iter()
                    .filter_map(|a| a.email)
                    .collect(),
                status: e.status.unwrap_or_else(|| "confirmed".to_string()),
            })
        })
        .collect())
}

pub async fn create_event(
    access_token: &str,
    summary: &str,
    start: &str,
    end: &str,
    description: Option<&str>,
) -> Result<String, String> {
    let client = Client::new();
    let url = format!("{}/calendars/primary/events", CALENDAR_API);

    let body = serde_json::json!({
        "summary": summary,
        "description": description,
        "start": { "dateTime": start },
        "end": { "dateTime": end },
    });

    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Create event error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Create event failed {}: {}", status, text));
    }

    let created: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(created["id"].as_str().unwrap_or("").to_string())
}

pub fn save_to_db(db: &crate::db::Database, events: &[CalendarEvent]) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    for event in events {
        conn.execute(
            "INSERT INTO calendar_events (google_id, summary, description, location, start_time, end_time, attendees, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(google_id) DO UPDATE SET
                summary = ?2, description = ?3, location = ?4, start_time = ?5, end_time = ?6, attendees = ?7, status = ?8, synced_at = datetime('now')",
            rusqlite::params![
                event.id, event.summary, event.description, event.location,
                event.start_time, event.end_time, event.attendees.join(","), event.status,
            ],
        ).map_err(|e| e.to_string())?;
    }
    Ok(())
}
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/src-tauri/src/integrations/calendar.rs
git commit -m "feat: add Google Calendar integration with event fetch, create, and caching"
```

---

## Task 6: Cron Engine (Scheduler)

**Files:**
- Create: `jarvis/src-tauri/src/scheduler/mod.rs`
- Create: `jarvis/src-tauri/src/scheduler/jobs.rs`
- Modify: `jarvis/src-tauri/src/lib.rs` (add `pub mod scheduler;`, start scheduler in setup)

- [ ] **Step 1: Create scheduler/mod.rs**

```rust
// jarvis/src-tauri/src/scheduler/mod.rs
pub mod jobs;

use crate::auth::google::GoogleAuth;
use crate::db::Database;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

pub struct Scheduler {
    scheduler: JobScheduler,
}

impl Scheduler {
    pub async fn new(
        db: Arc<Database>,
        google_auth: Arc<GoogleAuth>,
    ) -> Result<Self, String> {
        let scheduler = JobScheduler::new()
            .await
            .map_err(|e| format!("Failed to create scheduler: {}", e))?;

        // Load active jobs from DB and register them
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, name, schedule, action_type, parameters FROM cron_jobs WHERE status = 'active'")
            .map_err(|e| e.to_string())?;

        let job_rows: Vec<(i64, String, String, String, Option<String>)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(conn);

        for (job_id, name, schedule, action_type, _params) in job_rows {
            let db_clone = Arc::clone(&db);
            let auth_clone = Arc::clone(&google_auth);

            let job = Job::new_async(schedule.as_str(), move |_uuid, _lock| {
                let db = Arc::clone(&db_clone);
                let auth = Arc::clone(&auth_clone);
                let action = action_type.clone();
                let jid = job_id;
                Box::pin(async move {
                    log::info!("Running cron job {}: {}", jid, action);
                    let result = jobs::run_job(&db, &auth, &action, jid).await;
                    match &result {
                        Ok(msg) => log::info!("Job {} completed: {}", jid, msg),
                        Err(e) => log::error!("Job {} failed: {}", jid, e),
                    }
                })
            })
            .map_err(|e| format!("Failed to create job '{}': {}", name, e))?;

            scheduler.add(job).await.map_err(|e| e.to_string())?;
            log::info!("Registered cron job: {} ({})", name, schedule);
        }

        Ok(Scheduler { scheduler })
    }

    pub async fn start(&self) -> Result<(), String> {
        self.scheduler
            .start()
            .await
            .map_err(|e| format!("Failed to start scheduler: {}", e))?;
        log::info!("Cron scheduler started");
        Ok(())
    }
}
```

- [ ] **Step 2: Create scheduler/jobs.rs**

```rust
// jarvis/src-tauri/src/scheduler/jobs.rs
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::{calendar, gmail};
use std::sync::Arc;

pub async fn run_job(
    db: &Arc<Database>,
    auth: &Arc<GoogleAuth>,
    action_type: &str,
    job_id: i64,
) -> Result<String, String> {
    // Log run start
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO cron_runs (job_id, status) VALUES (?1, 'running')",
            rusqlite::params![job_id],
        ).map_err(|e| e.to_string())?;
    }

    let result = match action_type {
        "email_sync" => run_email_sync(db, auth).await,
        "calendar_sync" => run_calendar_sync(db, auth).await,
        "deadline_monitor" => run_deadline_monitor(db).await,
        other => Err(format!("Unknown job type: {}", other)),
    };

    // Log run result
    {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        let (status, result_text, error_text) = match &result {
            Ok(msg) => ("completed", Some(msg.as_str()), None),
            Err(e) => ("failed", None, Some(e.as_str())),
        };
        conn.execute(
            "UPDATE cron_runs SET finished_at = datetime('now'), status = ?1, result = ?2, error = ?3
             WHERE id = (SELECT id FROM cron_runs WHERE job_id = ?4 AND status = 'running' ORDER BY id DESC LIMIT 1)",
            rusqlite::params![status, result_text, error_text, job_id],
        ).map_err(|e| e.to_string())?;

        // Update last_run on the job
        conn.execute(
            "UPDATE cron_jobs SET last_run = datetime('now') WHERE id = ?1",
            rusqlite::params![job_id],
        ).map_err(|e| e.to_string())?;
    }

    result
}

async fn run_email_sync(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let token = match auth.get_access_token() {
        Some(t) => t,
        None => return Ok("Skipped: not authenticated with Google".to_string()),
    };

    let messages = gmail::fetch_inbox(&token, 20).await;

    match messages {
        Ok(msgs) => {
            let count = msgs.len();
            gmail::save_to_db(db, &msgs)?;
            Ok(format!("Synced {} emails", count))
        }
        Err(ref e) if e == "UNAUTHORIZED" => {
            auth.refresh_access_token().await?;
            let token = auth.get_access_token().ok_or("No token after refresh")?;
            let msgs = gmail::fetch_inbox(&token, 20).await?;
            let count = msgs.len();
            gmail::save_to_db(db, &msgs)?;
            Ok(format!("Synced {} emails (after token refresh)", count))
        }
        Err(e) => Err(e),
    }
}

async fn run_calendar_sync(db: &Arc<Database>, auth: &Arc<GoogleAuth>) -> Result<String, String> {
    let token = match auth.get_access_token() {
        Some(t) => t,
        None => return Ok("Skipped: not authenticated with Google".to_string()),
    };

    let now = chrono::Utc::now();
    let time_min = now.to_rfc3339();
    let time_max = (now + chrono::TimeDelta::days(7)).to_rfc3339();

    let events = calendar::fetch_events(&token, &time_min, &time_max).await;

    match events {
        Ok(evts) => {
            let count = evts.len();
            calendar::save_to_db(db, &evts)?;
            Ok(format!("Synced {} calendar events", count))
        }
        Err(ref e) if e == "UNAUTHORIZED" => {
            auth.refresh_access_token().await?;
            let token = auth.get_access_token().ok_or("No token after refresh")?;
            let evts = calendar::fetch_events(&token, &time_min, &time_max).await?;
            let count = evts.len();
            calendar::save_to_db(db, &evts)?;
            Ok(format!("Synced {} events (after token refresh)", count))
        }
        Err(e) => Err(e),
    }
}

async fn run_deadline_monitor(db: &Arc<Database>) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, title, deadline FROM tasks
         WHERE status != 'completed' AND deadline IS NOT NULL
         AND deadline <= date('now', '+3 days')
         ORDER BY deadline ASC"
    ).map_err(|e| e.to_string())?;

    let warnings: Vec<(i64, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    if warnings.is_empty() {
        return Ok("No upcoming deadlines".to_string());
    }

    // Log warnings (frontend will read tasks with approaching deadlines)
    for (id, title, deadline) in &warnings {
        log::warn!("Deadline approaching: '{}' (id={}) due {}", title, id, deadline);
    }

    Ok(format!("{} tasks with deadlines within 3 days", warnings.len()))
}
```

- [ ] **Step 3: Update lib.rs -- add scheduler module and start it**

Read `jarvis/src-tauri/src/lib.rs` and update:
1. Add `pub mod scheduler;` to module list
2. In the `setup` closure, after managing db and router:
   - Create `GoogleAuth` and load tokens from DB
   - Manage `GoogleAuth` as Tauri state (wrapped in `Arc`)
   - Spawn the scheduler as a background task

The setup closure should be updated to:
```rust
.setup(|app| {
    let db = Database::new().expect("Failed to initialize database");
    let claude_key = std::env::var("ANTHROPIC_API_KEY").ok();
    let openai_key = std::env::var("OPENAI_API_KEY").ok();
    let router = AiRouter::new(claude_key, openai_key, "claude_primary");

    // Google Auth
    let google_auth = auth::google::GoogleAuth::new()
        .unwrap_or_else(|| {
            log::warn!("Google credentials not configured. Set GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET in .env");
            // Create a dummy auth that will fail gracefully
            auth::google::GoogleAuth::new_empty()
        });
    google_auth.load_from_db(&db);

    let db_arc = std::sync::Arc::new(db);
    let auth_arc = std::sync::Arc::new(google_auth);

    // Start cron scheduler in background
    let db_for_scheduler = std::sync::Arc::clone(&db_arc);
    let auth_for_scheduler = std::sync::Arc::clone(&auth_arc);
    tauri::async_runtime::spawn(async move {
        match scheduler::Scheduler::new(db_for_scheduler, auth_for_scheduler).await {
            Ok(sched) => {
                if let Err(e) = sched.start().await {
                    log::error!("Scheduler start failed: {}", e);
                }
            }
            Err(e) => log::error!("Scheduler init failed: {}", e),
        }
    });

    app.manage(db_arc);
    app.manage(auth_arc);
    app.manage(router);

    tray::create_tray(app).expect("Failed to create system tray");
    log::info!("JARVIS started successfully");
    Ok(())
})
```

**IMPORTANT:** Since `Database` is now wrapped in `Arc`, all `State<Database>` in existing commands must change. Update ALL 4 existing command files:

In `commands/tasks.rs`: Replace all `State<Database>` with `State<Arc<Database>>` and add `use std::sync::Arc;` at the top.

In `commands/settings.rs`: Replace all `State<Database>` with `State<Arc<Database>>` and add `use std::sync::Arc;`.

In `commands/chat.rs`: Replace `State<'_, Database>` with `State<'_, Arc<Database>>` and add `use std::sync::Arc;`.

In `commands/dashboard.rs`: Replace `State<Database>` with `State<Arc<Database>>` and add `use std::sync::Arc;`.

The `db.conn.lock()` calls remain the same since `Arc<Database>` auto-derefs to `Database`.

Also add a `new_empty()` constructor to `GoogleAuth` for when credentials aren't configured:
```rust
pub fn new_empty() -> Self {
    GoogleAuth {
        client_id: String::new(),
        client_secret: String::new(),
        access_token: Mutex::new(None),
        refresh_token: Mutex::new(None),
    }
}
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 5: Commit**

```bash
git add jarvis/src-tauri/src/scheduler/ jarvis/src-tauri/src/lib.rs jarvis/src-tauri/src/auth/google.rs jarvis/src-tauri/src/commands/
git commit -m "feat: add cron engine with email sync, calendar sync, and deadline monitor jobs"
```

---

## Task 7: New Tauri IPC Commands (Email, Calendar, Cron)

**Files:**
- Create: `jarvis/src-tauri/src/commands/email.rs`
- Create: `jarvis/src-tauri/src/commands/calendar.rs`
- Create: `jarvis/src-tauri/src/commands/cron.rs`
- Modify: `jarvis/src-tauri/src/commands/mod.rs` (add modules)
- Modify: `jarvis/src-tauri/src/lib.rs` (register new commands)

- [ ] **Step 1: Create commands/email.rs**

```rust
// jarvis/src-tauri/src/commands/email.rs
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::gmail;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct EmailSummary {
    pub id: i64,
    pub gmail_id: String,
    pub subject: Option<String>,
    pub sender: String,
    pub snippet: Option<String>,
    pub is_read: bool,
    pub is_spam: bool,
    pub received_at: String,
}

#[tauri::command]
pub fn get_emails(db: State<Arc<Database>>, limit: Option<u32>) -> Result<Vec<EmailSummary>, String> {
    let limit = limit.unwrap_or(50);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, gmail_id, subject, sender, snippet, is_read, is_spam, received_at
         FROM emails ORDER BY received_at DESC LIMIT ?1"
    ).map_err(|e| e.to_string())?;

    let emails = stmt.query_map(rusqlite::params![limit], |row| {
        Ok(EmailSummary {
            id: row.get(0)?, gmail_id: row.get(1)?, subject: row.get(2)?,
            sender: row.get(3)?, snippet: row.get(4)?,
            is_read: row.get::<_, i32>(5)? != 0,
            is_spam: row.get::<_, i32>(6)? != 0,
            received_at: row.get(7)?,
        })
    }).map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(emails)
}

#[tauri::command]
pub async fn sync_emails(
    db: State<'_, Arc<Database>>,
    auth: State<'_, Arc<GoogleAuth>>,
) -> Result<String, String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    let messages = gmail::fetch_inbox(&token, 20).await?;
    let count = messages.len();
    gmail::save_to_db(&db, &messages)?;
    Ok(format!("Synced {} emails", count))
}

#[tauri::command]
pub async fn archive_email(
    auth: State<'_, Arc<GoogleAuth>>,
    gmail_id: String,
) -> Result<(), String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    gmail::archive_message(&token, &gmail_id).await
}

#[tauri::command]
pub fn get_email_stats(db: State<Arc<Database>>) -> Result<EmailStats, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let unread: i64 = conn.query_row(
        "SELECT COUNT(*) FROM emails WHERE is_read = 0", [], |r| r.get(0)
    ).map_err(|e| e.to_string())?;
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM emails", [], |r| r.get(0)
    ).map_err(|e| e.to_string())?;
    let spam: i64 = conn.query_row(
        "SELECT COUNT(*) FROM emails WHERE is_spam = 1", [], |r| r.get(0)
    ).map_err(|e| e.to_string())?;

    Ok(EmailStats { unread, total, spam })
}

#[derive(Serialize)]
pub struct EmailStats {
    pub unread: i64,
    pub total: i64,
    pub spam: i64,
}
```

- [ ] **Step 2: Create commands/calendar.rs**

```rust
// jarvis/src-tauri/src/commands/calendar.rs
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use crate::integrations::calendar as cal;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct CalendarEventView {
    pub id: i64,
    pub google_id: String,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_time: String,
    pub end_time: String,
    pub attendees: String,
    pub status: String,
}

#[tauri::command]
pub fn get_events(db: State<Arc<Database>>, days: Option<i32>) -> Result<Vec<CalendarEventView>, String> {
    let days = days.unwrap_or(7);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, google_id, summary, description, location, start_time, end_time, attendees, status
         FROM calendar_events
         WHERE start_time >= datetime('now') AND start_time <= datetime('now', ?1)
         ORDER BY start_time ASC"
    ).map_err(|e| e.to_string())?;

    let param = format!("+{} days", days);
    let events = stmt.query_map(rusqlite::params![param], |row| {
        Ok(CalendarEventView {
            id: row.get(0)?, google_id: row.get(1)?, summary: row.get(2)?,
            description: row.get(3)?, location: row.get(4)?, start_time: row.get(5)?,
            end_time: row.get(6)?, attendees: row.get(7)?, status: row.get(8)?,
        })
    }).map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(events)
}

#[tauri::command]
pub async fn sync_calendar(
    db: State<'_, Arc<Database>>,
    auth: State<'_, Arc<GoogleAuth>>,
) -> Result<String, String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    let now = chrono::Utc::now();
    let time_min = now.to_rfc3339();
    let time_max = (now + chrono::TimeDelta::days(7)).to_rfc3339();
    let events = cal::fetch_events(&token, &time_min, &time_max).await?;
    let count = events.len();
    cal::save_to_db(&db, &events)?;
    Ok(format!("Synced {} events", count))
}

#[tauri::command]
pub async fn create_event(
    auth: State<'_, Arc<GoogleAuth>>,
    summary: String,
    start: String,
    end: String,
    description: Option<String>,
) -> Result<String, String> {
    let token = auth.get_access_token().ok_or("Not authenticated with Google")?;
    cal::create_event(&token, &summary, &start, &end, description.as_deref()).await
}

#[tauri::command]
pub fn get_todays_events(db: State<Arc<Database>>) -> Result<Vec<CalendarEventView>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, google_id, summary, description, location, start_time, end_time, attendees, status
         FROM calendar_events
         WHERE date(start_time) = date('now')
         ORDER BY start_time ASC"
    ).map_err(|e| e.to_string())?;

    let events = stmt.query_map([], |row| {
        Ok(CalendarEventView {
            id: row.get(0)?, google_id: row.get(1)?, summary: row.get(2)?,
            description: row.get(3)?, location: row.get(4)?, start_time: row.get(5)?,
            end_time: row.get(6)?, attendees: row.get(7)?, status: row.get(8)?,
        })
    }).map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(events)
}
```

- [ ] **Step 3: Create commands/cron.rs**

```rust
// jarvis/src-tauri/src/commands/cron.rs
use crate::db::Database;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct CronJobView {
    pub id: i64,
    pub name: String,
    pub schedule: String,
    pub action_type: String,
    pub status: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
}

#[derive(Serialize)]
pub struct CronRunView {
    pub id: i64,
    pub job_id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_cron_jobs(db: State<Arc<Database>>) -> Result<Vec<CronJobView>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, name, schedule, action_type, status, last_run, next_run FROM cron_jobs ORDER BY id ASC"
    ).map_err(|e| e.to_string())?;

    let jobs = stmt.query_map([], |row| {
        Ok(CronJobView {
            id: row.get(0)?, name: row.get(1)?, schedule: row.get(2)?,
            action_type: row.get(3)?, status: row.get(4)?,
            last_run: row.get(5)?, next_run: row.get(6)?,
        })
    }).map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(jobs)
}

#[tauri::command]
pub fn get_cron_runs(db: State<Arc<Database>>, job_id: i64, limit: Option<u32>) -> Result<Vec<CronRunView>, String> {
    let limit = limit.unwrap_or(10);
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, job_id, started_at, finished_at, status, result, error
         FROM cron_runs WHERE job_id = ?1 ORDER BY started_at DESC LIMIT ?2"
    ).map_err(|e| e.to_string())?;

    let runs = stmt.query_map(rusqlite::params![job_id, limit], |row| {
        Ok(CronRunView {
            id: row.get(0)?, job_id: row.get(1)?, started_at: row.get(2)?,
            finished_at: row.get(3)?, status: row.get(4)?,
            result: row.get(5)?, error: row.get(6)?,
        })
    }).map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;

    Ok(runs)
}
```

- [ ] **Step 4: Update commands/mod.rs**

Add to existing `jarvis/src-tauri/src/commands/mod.rs`:
```rust
pub mod calendar;
pub mod cron;
pub mod email;
```

- [ ] **Step 5: Add a Google auth command**

Create `jarvis/src-tauri/src/commands/google_auth.rs`:
```rust
use crate::auth::google::GoogleAuth;
use crate::db::Database;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn google_connect(
    auth: State<'_, Arc<GoogleAuth>>,
    db: State<'_, Arc<Database>>,
) -> Result<String, String> {
    let scopes = vec![
        "https://www.googleapis.com/auth/gmail.readonly".to_string(),
        "https://www.googleapis.com/auth/gmail.modify".to_string(),
        "https://www.googleapis.com/auth/calendar".to_string(),
    ];
    auth.start_auth_flow(scopes).await?;
    auth.save_to_db(&db);
    Ok("Connected to Google".to_string())
}

#[tauri::command]
pub fn google_status(auth: State<Arc<GoogleAuth>>) -> bool {
    auth.is_authenticated()
}
```

Add `pub mod google_auth;` to `commands/mod.rs`.

- [ ] **Step 6: Register all new commands in lib.rs**

Add to the `invoke_handler` in lib.rs:
```rust
commands::email::get_emails,
commands::email::sync_emails,
commands::email::archive_email,
commands::email::get_email_stats,
commands::calendar::get_events,
commands::calendar::sync_calendar,
commands::calendar::create_event,
commands::calendar::get_todays_events,
commands::cron::get_cron_jobs,
commands::cron::get_cron_runs,
commands::google_auth::google_connect,
commands::google_auth::google_status,
```

- [ ] **Step 7: Verify compilation**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

- [ ] **Step 8: Commit**

```bash
git add jarvis/src-tauri/src/commands/
git commit -m "feat: add email, calendar, cron, and google auth Tauri commands"
```

---

## Task 8: Frontend Types & Command Wrappers

**Files:**
- Modify: `jarvis/src/lib/types.ts`
- Modify: `jarvis/src/lib/commands.ts`

- [ ] **Step 1: Add new types to types.ts**

Add to `jarvis/src/lib/types.ts`:
```ts
export interface EmailSummary {
  id: number;
  gmail_id: string;
  subject: string | null;
  sender: string;
  snippet: string | null;
  is_read: boolean;
  is_spam: boolean;
  received_at: string;
}

export interface EmailStats {
  unread: number;
  total: number;
  spam: number;
}

export interface CalendarEventView {
  id: number;
  google_id: string;
  summary: string;
  description: string | null;
  location: string | null;
  start_time: string;
  end_time: string;
  attendees: string;
  status: string;
}

export interface CronJobView {
  id: number;
  name: string;
  schedule: string;
  action_type: string;
  status: string;
  last_run: string | null;
  next_run: string | null;
}

export interface CronRunView {
  id: number;
  job_id: number;
  started_at: string;
  finished_at: string | null;
  status: string;
  result: string | null;
  error: string | null;
}
```

- [ ] **Step 2: Add new command wrappers to commands.ts**

Add to `jarvis/src/lib/commands.ts`:
```ts
import type { EmailSummary, EmailStats, CalendarEventView, CronJobView, CronRunView } from "./types";

// Email
export async function getEmails(limit?: number): Promise<EmailSummary[]> {
  return invoke("get_emails", { limit });
}
export async function syncEmails(): Promise<string> {
  return invoke("sync_emails");
}
export async function archiveEmail(gmailId: string): Promise<void> {
  return invoke("archive_email", { gmail_id: gmailId });
}
export async function getEmailStats(): Promise<EmailStats> {
  return invoke("get_email_stats");
}

// Calendar
export async function getEvents(days?: number): Promise<CalendarEventView[]> {
  return invoke("get_events", { days });
}
export async function syncCalendar(): Promise<string> {
  return invoke("sync_calendar");
}
export async function createEvent(summary: string, start: string, end: string, description?: string): Promise<string> {
  return invoke("create_event", { summary, start, end, description });
}
export async function getTodaysEvents(): Promise<CalendarEventView[]> {
  return invoke("get_todays_events");
}

// Cron
export async function getCronJobs(): Promise<CronJobView[]> {
  return invoke("get_cron_jobs");
}
export async function getCronRuns(jobId: number, limit?: number): Promise<CronRunView[]> {
  return invoke("get_cron_runs", { jobId, limit });
}

// Google Auth
export async function googleConnect(): Promise<string> {
  return invoke("google_connect");
}
export async function googleStatus(): Promise<boolean> {
  return invoke("google_status");
}
```

- [ ] **Step 3: Commit**

```bash
git add jarvis/src/lib/types.ts jarvis/src/lib/commands.ts
git commit -m "feat: add frontend types and command wrappers for email, calendar, cron"
```

---

## Task 9: Update Dashboard & StatsPanel with Live Data

**Files:**
- Modify: `jarvis/src/components/StatsPanel.tsx`
- Create: `jarvis/src/components/CalendarCard.tsx`
- Create: `jarvis/src/components/CronCard.tsx`
- Modify: `jarvis/src/pages/Dashboard.tsx`

- [ ] **Step 1: Create CalendarCard.tsx**

```tsx
// jarvis/src/components/CalendarCard.tsx
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CalendarEventView } from "../lib/types";

export default function CalendarCard() {
  const { data: events } = useTauriCommand<CalendarEventView[]>("get_todays_events");

  const count = events?.length ?? 0;
  const next = events?.[0];

  return (
    <div className="panel" style={styles.card}>
      <div className="label">CALENDAR</div>
      <div style={styles.value}>{count}</div>
      <div style={styles.detail}>
        {count === 0 ? "no meetings today" : `meeting${count !== 1 ? "s" : ""} today`}
      </div>
      {next && (
        <div style={styles.next}>
          Next: {next.summary}
          <br />
          {new Date(next.start_time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  card: { padding: 12 },
  value: { color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 },
  detail: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 },
  next: { color: "rgba(0, 180, 255, 0.5)", fontSize: 9, marginTop: 8, borderTop: "1px solid rgba(0, 180, 255, 0.1)", paddingTop: 6 },
};
```

- [ ] **Step 2: Create CronCard.tsx**

```tsx
// jarvis/src/components/CronCard.tsx
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CronJobView } from "../lib/types";

export default function CronCard() {
  const { data: jobs } = useTauriCommand<CronJobView[]>("get_cron_jobs");

  const active = jobs?.filter((j) => j.status === "active").length ?? 0;
  const lastRun = jobs?.find((j) => j.last_run)?.last_run;

  return (
    <div className="panel" style={styles.card}>
      <div className="label">CRON JOBS</div>
      <div style={styles.value}>{active}</div>
      <div style={styles.detail}>{active === 0 ? "none active" : "active"}</div>
      {lastRun && (
        <div style={styles.last}>
          Last run: {new Date(lastRun).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  card: { padding: 12 },
  value: { color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 },
  detail: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 },
  last: { color: "rgba(16, 185, 129, 0.6)", fontSize: 9, marginTop: 6 },
};
```

- [ ] **Step 3: Update StatsPanel.tsx**

Replace the static email/GitHub/cron cards with live components:

```tsx
// jarvis/src/components/StatsPanel.tsx
import StatCard from "./StatCard";
import CalendarCard from "./CalendarCard";
import CronCard from "./CronCard";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { EmailStats } from "../lib/types";

interface StatsPanelProps {
  taskCount: number;
}

export default function StatsPanel({ taskCount }: StatsPanelProps) {
  const { data: emailStats } = useTauriCommand<EmailStats>("get_email_stats");

  return (
    <div style={styles.container}>
      <StatCard label="TASKS" value={taskCount} detail="pending" />
      <StatCard
        label="EMAIL"
        value={emailStats?.unread ?? "--"}
        detail={emailStats ? `${emailStats.unread} unread` : "not connected"}
      />
      <CalendarCard />
      <CronCard />
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { width: 160, display: "flex", flexDirection: "column", gap: 10, overflowY: "auto" },
};
```

- [ ] **Step 4: Commit**

```bash
git add jarvis/src/components/CalendarCard.tsx jarvis/src/components/CronCard.tsx jarvis/src/components/StatsPanel.tsx
git commit -m "feat: update StatsPanel with live email, calendar, and cron data"
```

---

## Task 10: Settings Page -- Google Connect Button

**Files:**
- Modify: `jarvis/src/pages/Settings.tsx`

- [ ] **Step 1: Add Google connection UI to Settings**

Read the current `jarvis/src/pages/Settings.tsx` and add a new panel section after the API KEYS panel:

```tsx
// Add these imports at top
import { googleConnect, googleStatus } from "../lib/commands";
import { useEffect, useState } from "react";

// Inside the component, add state:
const [googleConnected, setGoogleConnected] = useState(false);
const [connecting, setConnecting] = useState(false);

useEffect(() => { googleStatus().then(setGoogleConnected); }, []);

async function handleGoogleConnect() {
  setConnecting(true);
  try {
    await googleConnect();
    setGoogleConnected(true);
  } catch (e) {
    console.error(e);
  } finally {
    setConnecting(false);
  }
}

// Add this JSX after the API KEYS panel:
<div className="panel" style={{ maxWidth: 500, padding: 16, marginTop: 12 }}>
  <div className="label" style={{ marginBottom: 12 }}>GOOGLE SERVICES</div>
  <div style={styles.hint}>Connect Gmail and Google Calendar</div>
  {googleConnected ? (
    <div style={{ color: "rgba(16, 185, 129, 0.7)", fontSize: 12 }}>Connected</div>
  ) : (
    <button
      onClick={handleGoogleConnect}
      disabled={connecting}
      style={{
        background: "rgba(0, 180, 255, 0.08)",
        border: "1px solid rgba(0, 180, 255, 0.3)",
        borderRadius: 6,
        padding: "8px 16px",
        color: "rgba(0, 180, 255, 0.8)",
        cursor: connecting ? "wait" : "pointer",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
      }}
    >
      {connecting ? "CONNECTING..." : "CONNECT GOOGLE"}
    </button>
  )}
</div>
```

- [ ] **Step 2: Commit**

```bash
git add jarvis/src/pages/Settings.tsx
git commit -m "feat: add Google OAuth connect button to Settings page"
```

---

## Summary

After completing all 10 tasks, Phase 1b delivers:

- **V2 migration** with emails, calendar_events, cron_jobs, cron_runs tables
- **Google OAuth2** with PKCE loopback redirect flow, token persistence in DB
- **Gmail integration** -- fetch inbox, cache locally, archive messages
- **Google Calendar integration** -- fetch events, cache locally, create events
- **Cron engine** -- tokio-cron-scheduler running email sync (every 5 min), calendar sync (every 5 min), deadline monitor (daily at 9am)
- **12 new Tauri IPC commands** for email, calendar, cron, and Google auth
- **Updated dashboard** with live email stats, calendar card, cron job status
- **Settings page** with Google connect button

**Next:** Phase 1c -- Notion + GitHub integrations, cron dashboard display.
