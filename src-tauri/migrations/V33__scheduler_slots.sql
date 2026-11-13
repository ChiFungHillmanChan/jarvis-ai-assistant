-- Scheduler reliability. A scheduled occurrence (a "slot") is now tracked per
-- job, so work missed while the Mac slept or JARVIS was quit runs once when it
-- can, and never twice.
--
-- handled_slot: the last slot whose run reached a final state (completed,
--   failed or skipped_offline_final), as local time with its UTC offset
--   ('2026-11-12T09:00:00+08:00'). NULL until the first one.
-- baseline_at:  the earliest moment a missed slot can count (UTC). Set when a
--   job is created and again whenever it goes from paused to active.
ALTER TABLE cron_jobs ADD COLUMN handled_slot TEXT;
ALTER TABLE cron_jobs ADD COLUMN baseline_at TEXT;

-- Existing jobs start counting from this moment, so the first launch after
-- updating does not catch up history.
UPDATE cron_jobs SET baseline_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now');

-- trigger: why the run started (scheduled, catch_up, manual,
--   retry_interrupted). NULL on rows written before this migration and on
--   rows that record a slot that did not run (collapsed, or skipped while
--   waiting for the network).
-- slot:    the slot the row is about. NULL for a manual run.
ALTER TABLE cron_runs ADD COLUMN "trigger" TEXT;
ALTER TABLE cron_runs ADD COLUMN slot TEXT;

-- The scheduled daily briefing. It replaces the "speak a briefing when JARVIS
-- starts" setting (auto_briefing): it is on only for someone who had that
-- setting on, and off for everyone else until they turn it on in Settings.
INSERT INTO cron_jobs (name, schedule, action_type, status, description, baseline_at)
SELECT 'Daily Briefing', '0 30 8 * * *', 'daily_briefing',
       CASE WHEN EXISTS (SELECT 1 FROM user_preferences WHERE key = 'auto_briefing' AND value = 'true')
            THEN 'active' ELSE 'paused' END,
       'Shows your day at the time you chose, from what is saved on this Mac',
       strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
 WHERE NOT EXISTS (SELECT 1 FROM cron_jobs WHERE action_type = 'daily_briefing');

-- Someone the old setting already briefed today is not briefed a second time
-- by the new one.
INSERT OR IGNORE INTO user_preferences (key, value, updated_at)
SELECT 'daily_briefing_last_date', value, datetime('now')
  FROM user_preferences
 WHERE key = 'last_briefing_date'
   AND EXISTS (SELECT 1 FROM user_preferences WHERE key = 'auto_briefing' AND value = 'true');

DELETE FROM user_preferences WHERE key = 'auto_briefing';
