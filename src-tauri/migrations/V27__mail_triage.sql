-- Automatic per-message triage for the mail workspace.
--
-- mail_triage holds one row per message that has been classified: Jev
-- (TypeSafe) answers the typed questions, code applies the policy, and
-- gpt-6-luna writes the summary only for messages that need one. Probabilities
-- are kept raw so thresholds can change without re-running inference.
--
-- mail_items are the dated follow-ups extracted from a message (tasks,
-- deadlines, events, replies). They are local suggestions: nothing is written
-- to Google Calendar until the user asks for it on a specific item.
CREATE TABLE mail_triage (
    message_id INTEGER PRIMARY KEY REFERENCES mail_messages(id) ON DELETE CASCADE,
    category TEXT NOT NULL,
    category_confidence REAL NOT NULL,
    needs_reply REAL NOT NULL,
    action_required REAL NOT NULL,
    has_date REAL NOT NULL,
    importance REAL NOT NULL,
    sent_by_owner INTEGER NOT NULL DEFAULT 0 CHECK (sent_by_owner IN (0,1)),
    attention INTEGER NOT NULL DEFAULT 0 CHECK (attention IN (0,1)),
    summary TEXT,
    decided_by TEXT NOT NULL DEFAULT 'jev',
    jev_model TEXT NOT NULL,
    luna_model TEXT,
    triaged_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX mail_triage_category ON mail_triage(category);
CREATE INDEX mail_triage_attention ON mail_triage(attention);

CREATE TABLE mail_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES mail_messages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('task','deadline','event','reply')),
    title TEXT NOT NULL,
    due_date TEXT,
    due_time TEXT,
    end_time TEXT,
    location TEXT,
    evidence TEXT NOT NULL DEFAULT '',
    evidence_verified INTEGER NOT NULL DEFAULT 0 CHECK (evidence_verified IN (0,1)),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','done','dismissed')),
    remind_at TEXT,
    reminded_at TEXT,
    calendar_event_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX mail_items_message ON mail_items(message_id);
CREATE INDEX mail_items_open_due ON mail_items(status, due_date);
CREATE INDEX mail_items_remind ON mail_items(status, remind_at);

-- Triage sends message text to TypeSafe and, for some messages, OpenAI, so it
-- stays off until the user turns it on in the workspace settings.
ALTER TABLE mail_settings ADD COLUMN auto_triage INTEGER NOT NULL DEFAULT 0;

-- gpt-6-luna replaced gpt-5.6-luna as the default (US$0.10/$0.50 per 1M
-- tokens against $0.20/$1.20). Only rows still on the old default move; a
-- model the user typed in stays as it is.
UPDATE mail_settings SET openai_model='gpt-6-luna' WHERE openai_model IN ('gpt-5.6-luna','');
