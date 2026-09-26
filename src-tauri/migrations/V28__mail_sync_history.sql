-- Gmail's history cursor for each mailbox. A normal sync used to re-list every
-- message since a day before the last sync and re-read the labels of the 50
-- newest stored ones: about 1,000 quota units a sync (messages.get costs 20)
-- against a per-user ceiling of 6,000 a minute. history.list costs 2 and
-- returns only what changed. NULL means no cursor yet or an expired one; the
-- sync then falls back to the date window and sets it again.
ALTER TABLE mail_accounts ADD COLUMN history_id TEXT;

-- When a complete sync window first came back with a message that would not
-- download. The sync gives up on that message only after retrying this long;
-- measuring the window's age instead gave up on the first failure whenever
-- the user had not synced for a week.
ALTER TABLE mail_accounts ADD COLUMN retry_since TEXT;

-- Workspace mail synced only when the user pressed "Sync". Run it in the
-- background too, offset 30 seconds from the legacy inbox sync so the two do
-- not start in the same second. Guarded so it is seeded once.
INSERT INTO cron_jobs (name, schedule, action_type, status, description)
SELECT 'Mail Workspace Sync', '30 */5 * * * *', 'mail_workspace_sync', 'active',
       'Downloads new mail for connected mailboxes and sorts it if AI sorting is on'
 WHERE NOT EXISTS (SELECT 1 FROM cron_jobs WHERE action_type = 'mail_workspace_sync');
