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

INSERT OR IGNORE INTO cron_jobs (name, schedule, action_type, parameters, status)
VALUES
    ('Email Sync', '0 */5 * * * *', 'email_sync', NULL, 'active'),
    ('Calendar Sync', '0 */5 * * * *', 'calendar_sync', NULL, 'active'),
    ('Deadline Monitor', '0 0 9 * * *', 'deadline_monitor', NULL, 'active');
