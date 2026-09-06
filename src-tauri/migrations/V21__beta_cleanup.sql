-- Beta cleanup: rewrites/removes rows left behind by fixes that shipped
-- after they were originally written, and seeds the cron job that makes
-- accepted email rules actually run.

-- Pre-fix rows stored the raw RFC 2822 Date header (e.g. "Thu, 4 Sep 2026
-- 10:15:00 +0800"); received_at is compared with bytewise TEXT collation
-- everywhere, so these sort wrong and cannot be repaired in place.
DELETE FROM emails WHERE received_at NOT LIKE '____-__-__T%';

-- 'claude_primary' was the V1 seed value; no provider code recognises it.
UPDATE user_preferences SET value = 'gemini_primary', updated_at = datetime('now')
 WHERE key = 'ai_provider' AND value = 'claude_primary';

-- Retired Gemini Flash id, still pinned in provider_chain rows persisted
-- before the model id moved on.
UPDATE provider_chain SET model_id = 'gemini-3.7-flash'
 WHERE provider_type = 'gemini' AND model_id = 'gemini-3-flash-preview';

-- The installed macOS Cantonese voice is "Sinji"; the V14 seed had a hyphen
-- that never matches an actual `say -v` voice name.
UPDATE user_preferences SET value = 'Sinji', updated_at = datetime('now')
 WHERE key = 'tts_voice_yue' AND value = 'Sin-ji';

-- Accepted email rules only take effect once something runs them. Seeded
-- once, guarded so re-running this migration body never duplicates the job.
INSERT INTO cron_jobs (name, schedule, action_type, status, description)
SELECT 'Auto Archive', '0 */10 * * * *', 'auto_archive_emails', 'active',
       'Archives new mail from senders whose rules you accepted'
 WHERE NOT EXISTS (SELECT 1 FROM cron_jobs WHERE action_type = 'auto_archive_emails');
