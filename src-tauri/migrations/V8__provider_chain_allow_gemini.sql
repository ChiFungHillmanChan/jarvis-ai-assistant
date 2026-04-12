-- Allow 'gemini' in provider_chain.provider_type CHECK constraint.
-- SQLite cannot alter CHECK constraints in place, so rebuild the table.

CREATE TABLE provider_chain_new (
    position INTEGER NOT NULL PRIMARY KEY,
    provider_type TEXT NOT NULL
        CHECK(provider_type IN ('claude', 'openai', 'local', 'gemini')),
    endpoint_id TEXT,
    model_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (endpoint_id) REFERENCES local_endpoints(id) ON DELETE CASCADE
);

INSERT INTO provider_chain_new (position, provider_type, endpoint_id, model_id, enabled)
SELECT position, provider_type, endpoint_id, model_id, enabled FROM provider_chain;

DROP TABLE provider_chain;

ALTER TABLE provider_chain_new RENAME TO provider_chain;
