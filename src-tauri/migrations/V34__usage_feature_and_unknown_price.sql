-- Usage rows say what they were spent on, store an unknown price as unknown,
-- and name each provider one way. Rebuilt rather than altered because
-- usd_cost_micro was NOT NULL and an unknown price is now NULL.
--
-- feature: chat, tools, mail_analysis, mail_sort, briefing, stt, tts, memory,
-- other (ai/cost.rs Feature). No CHECK, so a later feature needs no rebuild.
-- Rows from before this migration cannot be attributed and become 'other'.
--
-- price_unknown = 1 marks a cloud model missing from the price list; its cost
-- is NULL, and only then, the same rule as usage_daily below. Older rows are
-- copied with the cost they were stored with and price_unknown = 0. Every one
-- of them has a cost: V16 added the column as NOT NULL DEFAULT 0, the only
-- migration that defines it in every release from v0.1.3 to v0.5.11, so no
-- row can stop this copy. The step that runs once after the migrations
-- (ai/cost.rs correct_old_rows) then corrects the ones earlier builds got
-- wrong: the guessed US$3/US$15 per 1M an unknown model was billed at, an
-- unknown model billed at zero, a local model's row that holds a cost, and
-- Claude and Bedrock input stored without the cache.
--
-- provider: openai, claude, gemini, bedrock, local, typesafe, minimax, or
-- compat:<host> for an OpenAI-compatible service. 'openai-mail' was OpenAI
-- itself, and a bare host was a custom compatible URL. 'ollama' and '本機 ai'
-- were a local model's plain completions from v0.5.0 to v0.5.4, so they
-- become 'local' and count in the local share. 'kimi' and 'qwen' are left
-- alone: each has two hosts (global and China) and the row does not say
-- which one answered.
CREATE TABLE inference_metrics_v34 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    workload TEXT NOT NULL CHECK (workload IN ('volume', 'tools', 'premium', 'unknown')),
    model_id TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    latency_ms INTEGER NOT NULL,
    provider TEXT NOT NULL DEFAULT 'unknown',
    usd_cost_micro INTEGER DEFAULT 0,
    feature TEXT NOT NULL DEFAULT 'other',
    price_unknown INTEGER NOT NULL DEFAULT 0 CHECK (price_unknown IN (0, 1)),
    CHECK ((price_unknown = 0) = (usd_cost_micro IS NOT NULL))
);

INSERT INTO inference_metrics_v34
    (id, ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens,
     cache_write_tokens, latency_ms, provider, usd_cost_micro)
SELECT id, ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens,
       cache_write_tokens, latency_ms,
       CASE
           WHEN provider = 'openai-mail' THEN 'openai'
           WHEN lower(provider) IN ('ollama', '本機 ai') THEN 'local'
           WHEN provider LIKE '%.%' OR provider IN ('localhost', '[::1]') THEN 'compat:' || provider
           ELSE provider
       END,
       usd_cost_micro
FROM inference_metrics;

DROP TABLE inference_metrics;
ALTER TABLE inference_metrics_v34 RENAME TO inference_metrics;

CREATE INDEX idx_inference_metrics_ts ON inference_metrics(ts);
CREATE INDEX idx_inference_metrics_provider_ts ON inference_metrics(provider, ts);

-- Daily totals of the usage older than the per-request window. ai/usage.rs
-- moves those requests here and deletes them from inference_metrics in one
-- transaction, so each request is in exactly one of the two tables, and
-- deletes totals older than two years. One row per UTC day (ts / 86400),
-- provider, model, feature and price-known flag. input_tokens includes the
-- cache reads and writes, as in inference_metrics. The cost is NULL when the
-- price is unknown, and only then. No prompt, reply, mail, key or latency.
CREATE TABLE usage_daily (
    day INTEGER NOT NULL,
    provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    feature TEXT NOT NULL,
    price_unknown INTEGER NOT NULL CHECK (price_unknown IN (0, 1)),
    requests INTEGER NOT NULL CHECK (requests > 0),
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    cache_write_tokens INTEGER NOT NULL,
    usd_cost_micro INTEGER,
    CHECK ((price_unknown = 0) = (usd_cost_micro IS NOT NULL)),
    PRIMARY KEY (day, provider, model_id, feature, price_unknown)
) WITHOUT ROWID;
