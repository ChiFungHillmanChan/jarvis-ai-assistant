-- Archiving previously only called the Gmail API and never touched the local
-- row, so an archived message stayed visible in JARVIS forever. Track it
-- locally so the list reflects the action the user just took.
ALTER TABLE emails ADD COLUMN is_archived INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_emails_archived ON emails(is_archived, received_at DESC);
