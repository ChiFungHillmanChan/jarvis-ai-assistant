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
