-- Add an explicit OpenAI route without changing any existing processing choice.
CREATE TABLE mail_settings_openai (
    id INTEGER PRIMARY KEY CHECK (id=1),
    processing_mode TEXT NOT NULL DEFAULT 'local' CHECK (processing_mode IN ('local','bedrock','openai')),
    local_model TEXT NOT NULL DEFAULT '',
    bedrock_model TEXT NOT NULL DEFAULT '',
    openai_model TEXT NOT NULL DEFAULT 'gpt-5.6-luna'
);
INSERT INTO mail_settings_openai(id,processing_mode,local_model,bedrock_model)
    SELECT id,processing_mode,local_model,bedrock_model FROM mail_settings;
DROP TABLE mail_settings;
ALTER TABLE mail_settings_openai RENAME TO mail_settings;
