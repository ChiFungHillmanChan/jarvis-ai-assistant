CREATE TABLE IF NOT EXISTS beta_testers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT,
    email TEXT NOT NULL UNIQUE,
    segment TEXT,
    use_case TEXT,
    status TEXT NOT NULL DEFAULT 'waitlisted',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS beta_feedback (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tester_email TEXT,
    category TEXT NOT NULL,
    rating INTEGER NOT NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO user_preferences (key, value) VALUES ('launch_stage', 'private_beta');
INSERT OR IGNORE INTO user_preferences (key, value) VALUES ('pro_price_hkd', '148');
INSERT OR IGNORE INTO user_preferences (key, value) VALUES ('team_price_hkd', '298');
