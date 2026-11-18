-- The user's consent to send email and calendar content to an AI provider
-- whose data terms need it (P06): Gemini, whose free tier may use what it
-- is sent to improve Google's models, and every provider JARVIS cannot
-- vouch for (compatible services, Ollama Cloud, an Ollama on another
-- machine, a provider new in a fetched registry). See ai/consent.rs.
--
-- One row per provider and address: provider is the registry provider id,
-- base_url the address the user entered for a service that has one ('' for
-- a provider at its own address). data_terms and registry_version are the
-- terms in force, and the registry version, when the user agreed; the
-- consent holds while the terms in force are not stricter. Revoking it in
-- Settings deletes the row.
CREATE TABLE ai_consent (
    provider TEXT NOT NULL,
    base_url TEXT NOT NULL DEFAULT '',
    data_terms TEXT NOT NULL CHECK (data_terms IN ('needs_billing_confirmation', 'unknown')),
    registry_version INTEGER NOT NULL,
    consented_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (provider, base_url)
);

-- A Gemini billing confirmation given before this table existed is that
-- consent. Registry version 0: given before the app read a registry's terms.
INSERT INTO ai_consent (provider, base_url, data_terms, registry_version, consented_at)
SELECT 'gemini', '', 'needs_billing_confirmation', 0, COALESCE(updated_at, datetime('now'))
FROM user_preferences
WHERE key = 'gemini_billing_confirmed' AND value = 'true';
