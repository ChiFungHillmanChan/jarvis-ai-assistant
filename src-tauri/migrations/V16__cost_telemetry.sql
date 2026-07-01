-- Extend inference_metrics with provider + USD cost so every chat turn becomes
-- a costed data point. usd_cost_micro stores micro-USD (USD * 1e6) as INTEGER
-- to avoid float drift during aggregation.
ALTER TABLE inference_metrics ADD COLUMN provider TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE inference_metrics ADD COLUMN usd_cost_micro INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_inference_metrics_provider_ts
    ON inference_metrics(provider, ts);
