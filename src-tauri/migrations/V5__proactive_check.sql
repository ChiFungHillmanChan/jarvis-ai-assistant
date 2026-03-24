INSERT OR IGNORE INTO cron_jobs (name, schedule, action_type, parameters, status)
VALUES ('Proactive Check', '0 */1 * * * *', 'proactive_check', NULL, 'active');
