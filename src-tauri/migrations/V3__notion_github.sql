CREATE TABLE IF NOT EXISTS notion_pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    notion_id TEXT UNIQUE NOT NULL,
    title TEXT NOT NULL,
    url TEXT,
    parent_type TEXT,
    parent_title TEXT,
    last_edited TEXT,
    content_snippet TEXT,
    synced_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_notion_edited ON notion_pages(last_edited DESC);

CREATE TABLE IF NOT EXISTS github_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    github_id INTEGER NOT NULL,
    item_type TEXT NOT NULL,
    title TEXT NOT NULL,
    repo TEXT NOT NULL,
    number INTEGER,
    state TEXT NOT NULL,
    url TEXT,
    author TEXT,
    updated_at TEXT,
    ci_status TEXT,
    synced_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(github_id, item_type)
);
CREATE INDEX idx_github_type ON github_items(item_type, state);
CREATE INDEX idx_github_repo ON github_items(repo);

INSERT OR IGNORE INTO cron_jobs (name, schedule, action_type, parameters, status)
VALUES
    ('Notion Sync', '0 */10 * * * *', 'notion_sync', NULL, 'active'),
    ('GitHub Digest', '0 */10 * * * *', 'github_digest', NULL, 'active');
