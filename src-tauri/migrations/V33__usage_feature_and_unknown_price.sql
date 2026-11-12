-- Usage rows say what they were spent on, store an unknown price as unknown,
-- and name each provider one way. Rebuilt rather than altered because
-- usd_cost_micro was NOT NULL and an unknown price is now NULL.
--
-- feature: chat, tools, mail_analysis, mail_sort, briefing, stt, tts, memory,
-- other (ai/cost.rs Feature). No CHECK, so a later feature needs no rebuild.
-- Rows from before this migration cannot be attributed and become 'other'.
--
-- price_unknown = 1 marks a cloud model missing from the price list; its cost
-- is NULL. Older rows keep the cost they were stored with (an unknown model
-- was billed at a guessed US$3/US$15 per 1M) and price_unknown = 0: the rows
-- do not record whether the guess was used.
--
-- provider: openai, claude, gemini, bedrock, local, typesafe, minimax, or
-- compat:<host> for an OpenAI-compatible service. 'openai-mail' was OpenAI
-- itself, and a bare host was a custom compatible URL. 'kimi' and 'qwen' are
-- left alone: each has two hosts (global and China) and the row does not say
-- which one answered.
CREATE TABLE inference_metrics_v33 (
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
    CHECK (price_unknown = 0 OR usd_cost_micro IS NULL)
);

INSERT INTO inference_metrics_v33
    (id, ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens,
     cache_write_tokens, latency_ms, provider, usd_cost_micro)
SELECT id, ts, workload, model_id, input_tokens, output_tokens, cache_read_tokens,
       cache_write_tokens, latency_ms,
       CASE
           WHEN provider = 'openai-mail' THEN 'openai'
           WHEN provider LIKE '%.%' OR provider IN ('localhost', '[::1]') THEN 'compat:' || provider
           ELSE provider
       END,
       usd_cost_micro
FROM inference_metrics;

DROP TABLE inference_metrics;
ALTER TABLE inference_metrics_v33 RENAME TO inference_metrics;

CREATE INDEX idx_inference_metrics_ts ON inference_metrics(ts);
CREATE INDEX idx_inference_metrics_provider_ts ON inference_metrics(provider, ts);
