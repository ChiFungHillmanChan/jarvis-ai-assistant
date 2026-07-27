-- Re-key daily_briefings from the old hardcoded UTC+8 day to the machine's
-- local calendar day.
--
-- scheduler/daily_briefing.rs used to compute the dedupe key as Utc::now() + 8h.
-- It now uses the machine's local day, like every other "today" in the app, but
-- nothing rewrote the rows written under the old scheme -- and date_hkt is
-- UNIQUE. On any install west of UTC+8 an evening briefing was stored under
-- tomorrow's key, so the next day the skip check found that row, logged
-- "already generated ... skipping" and returned Ok(()): the user got a success
-- and no briefing, once, deterministically.
--
-- created_at is the Unix timestamp the row was written at (V15), so the local
-- day each row belongs to can be recovered exactly, DST included. The
-- 'localtime' modifier resolves against the machine running this migration,
-- which is the same zone chrono::Local reads at runtime.

-- 1. Collapse duplicates first. Two rows can resolve to the same local day (one
--    written before the old key rolled over, one after). date_hkt is UNIQUE and
--    one briefing per day is all the column can hold, so keep the newest and
--    drop the older copy -- a briefing is regenerated from the inbox anyway. On
--    an install that really was in UTC+8 the resolved key equals the stored one
--    and this deletes nothing.
DELETE FROM daily_briefings
WHERE id NOT IN (
    SELECT MAX(id) FROM daily_briefings
    GROUP BY COALESCE(date(created_at, 'unixepoch', 'localtime'), date_hkt)
);

-- 2. Park the rows that need re-keying under a value no date can take. SQLite
--    enforces UNIQUE row by row as an UPDATE walks the table, so re-keying in a
--    single statement could collide with a row the statement has not reached
--    yet. COALESCE leaves a row with an unreadable created_at out of both
--    statements instead of risking a NULL write into a NOT NULL column.
UPDATE daily_briefings
SET date_hkt = 'v19-pending:' || id
WHERE date_hkt <> COALESCE(date(created_at, 'unixepoch', 'localtime'), date_hkt);

-- 3. Land them on the local day. Step 1 left at most one row per resolved day,
--    so every value written here is distinct and cannot collide with a row that
--    was already correct. Step 2 can only have parked rows whose
--    date(created_at, ...) is non-NULL.
UPDATE daily_briefings
SET date_hkt = date(created_at, 'unixepoch', 'localtime')
WHERE date_hkt LIKE 'v19-pending:%';
