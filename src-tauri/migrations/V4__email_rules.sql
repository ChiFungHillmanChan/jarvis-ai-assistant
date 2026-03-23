CREATE TABLE IF NOT EXISTS email_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sender TEXT NOT NULL UNIQUE,
    archive_count INTEGER NOT NULL DEFAULT 0,
    rule_status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_email_rules_sender ON email_rules(sender);
CREATE INDEX idx_email_rules_status ON email_rules(rule_status);
