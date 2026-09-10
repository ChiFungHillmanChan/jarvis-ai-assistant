-- A user-owned local archive, separate from the legacy Google inbox cache.
CREATE TABLE mail_accounts (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL COLLATE NOCASE UNIQUE,
    connected INTEGER NOT NULL DEFAULT 1 CHECK (connected IN (0,1)),
    last_synced TEXT,
    next_page_token TEXT
);
CREATE TABLE mail_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES mail_accounts(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL,
    thread_id TEXT,
    subject TEXT NOT NULL DEFAULT '',
    sender TEXT NOT NULL DEFAULT '',
    snippet TEXT NOT NULL DEFAULT '',
    body TEXT,
    is_read INTEGER NOT NULL DEFAULT 0,
    is_inbox INTEGER NOT NULL DEFAULT 0,
    received_at TEXT NOT NULL,
    saved_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(account_id,provider_id)
);
CREATE INDEX mail_messages_account_date ON mail_messages(account_id,received_at DESC,id DESC);
CREATE INDEX mail_messages_date ON mail_messages(received_at DESC,id DESC);
CREATE TABLE mail_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK (length(trim(name)) > 0),
    instructions TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE mail_group_members (
    group_id INTEGER NOT NULL REFERENCES mail_groups(id) ON DELETE CASCADE,
    message_id INTEGER NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
    PRIMARY KEY(group_id,message_id)
);
CREATE INDEX mail_members_message ON mail_group_members(message_id);
CREATE TABLE mail_memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id INTEGER NOT NULL REFERENCES mail_groups(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('fact','preference','commitment')),
    source_message_id INTEGER REFERENCES mail_messages(id) ON DELETE SET NULL,
    expires_at TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','done')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX mail_memories_group ON mail_memories(group_id);
CREATE TABLE mail_settings (
    id INTEGER PRIMARY KEY CHECK (id=1),
    processing_mode TEXT NOT NULL DEFAULT 'local' CHECK (processing_mode IN ('local','bedrock')),
    local_model TEXT NOT NULL DEFAULT '',
    bedrock_model TEXT NOT NULL DEFAULT ''
);
INSERT INTO mail_settings(id) VALUES (1);
