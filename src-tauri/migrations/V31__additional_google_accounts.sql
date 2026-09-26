-- Additional Google accounts. A Mail mailbox now also asks for read-only
-- Calendar access, so the user's second and third Google accounts bring their
-- mail and their calendar; the main account (Settings > Google) stays the only
-- one JARVIS writes to. 1 when the mailbox's credential carries Calendar
-- access; a mailbox connected before this, or one whose owner unticked
-- Calendar on Google's consent screen, stays 0 until it is connected again.
ALTER TABLE mail_accounts ADD COLUMN calendar INTEGER NOT NULL DEFAULT 0 CHECK (calendar IN (0,1));

-- Events now come from more than one calendar. An invitation between two of
-- the user's own accounts sits in both calendars under the same event id, so
-- an event is unique per account rather than by id alone. account_id is the
-- mailbox's mail_accounts.id, or '' for the main account.
CREATE TABLE calendar_events_v31 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL DEFAULT '',
    google_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    description TEXT,
    location TEXT,
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    attendees TEXT,
    status TEXT DEFAULT 'confirmed',
    synced_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(account_id, google_id)
);
INSERT INTO calendar_events_v31 (id, account_id, google_id, summary, description, location, start_time, end_time, attendees, status, synced_at)
    SELECT id, '', google_id, summary, description, location, start_time, end_time, attendees, status, synced_at
    FROM calendar_events;
DROP TABLE calendar_events;
ALTER TABLE calendar_events_v31 RENAME TO calendar_events;
CREATE INDEX idx_calendar_start ON calendar_events(start_time);
