ALTER TABLE conversations ADD COLUMN session_id TEXT NOT NULL DEFAULT 'legacy';
CREATE INDEX conversations_session_id ON conversations(session_id, id);
