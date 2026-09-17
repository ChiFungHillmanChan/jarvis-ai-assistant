-- Retention for the tables that grow without bound. Before this, only
-- cron_runs was ever pruned (scheduler/jobs.rs); mail_messages kept every
-- message body forever while sync.rs walks backwards through the mailbox one
-- persisted page token at a time, so a large Gmail account turns jarvis.db
-- into a multi-GB file with no way for the user to see it or stop it.
--
-- 0 days means "keep everything" so the sweep can be switched off without a
-- schema change. The sweep itself lives in commands/storage.rs.
INSERT OR IGNORE INTO user_preferences (key, value, updated_at) VALUES
    ('storage_retention_days', '90', datetime('now')),
    ('storage_retention_enabled', '1', datetime('now'));

-- The sweep filters on saved_at, which is the only mail_messages timestamp
-- written in datetime('now') format; received_at carries whatever the provider
-- sent. Without this index the sweep is a full table scan of the one table
-- that is allowed to get large.
CREATE INDEX IF NOT EXISTS idx_mail_messages_saved_at ON mail_messages(saved_at);
CREATE INDEX IF NOT EXISTS idx_conversations_created_at ON conversations(created_at);
