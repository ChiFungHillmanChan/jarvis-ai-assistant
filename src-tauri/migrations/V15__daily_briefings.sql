CREATE TABLE IF NOT EXISTS daily_briefings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date_hkt TEXT NOT NULL UNIQUE,
    summary_md TEXT NOT NULL,
    actions_json TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT 'en',
    created_at INTEGER NOT NULL
);

INSERT OR IGNORE INTO user_preferences (key, value, updated_at) VALUES
    ('briefing_enabled',  '0', datetime('now')),
    ('briefing_time_hkt', '08:00', datetime('now'));
