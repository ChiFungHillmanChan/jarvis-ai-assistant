-- The user chooses one AI in setup and every feature uses it
-- (docs/superpowers/plans/2026-09-26-choose-one-ai.md).
--
-- OpenAI-compatible cloud services (Kimi, Qwen and others) are `openai` chain
-- rows with their own base URL, so they get native tool calling and are never
-- labelled as on-device. NULL keeps the row on api.openai.com.
ALTER TABLE provider_chain ADD COLUMN base_url TEXT;

-- Mail analysis can use the AI chosen for the assistant ('assistant').
-- SQLite cannot alter a CHECK constraint in place, so rebuild the table.
CREATE TABLE mail_settings_assistant (
    id INTEGER PRIMARY KEY CHECK (id=1),
    processing_mode TEXT NOT NULL DEFAULT 'local' CHECK (processing_mode IN ('local','bedrock','openai','assistant')),
    local_model TEXT NOT NULL DEFAULT '',
    bedrock_model TEXT NOT NULL DEFAULT '',
    openai_model TEXT NOT NULL DEFAULT 'gpt-6-luna',
    auto_triage INTEGER NOT NULL DEFAULT 0
);
INSERT INTO mail_settings_assistant(id,processing_mode,local_model,bedrock_model,openai_model,auto_triage)
    SELECT id,processing_mode,local_model,bedrock_model,openai_model,auto_triage FROM mail_settings;
DROP TABLE mail_settings;
ALTER TABLE mail_settings_assistant RENAME TO mail_settings;
