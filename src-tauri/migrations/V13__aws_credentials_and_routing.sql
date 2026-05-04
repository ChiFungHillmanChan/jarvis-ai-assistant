-- AWS credentials (singleton row, secret encrypted by app keychain wrapper)
CREATE TABLE IF NOT EXISTS aws_credentials (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    access_key_id TEXT NOT NULL,
    secret_access_key TEXT NOT NULL,
    region TEXT NOT NULL DEFAULT 'ap-southeast-1',
    inference_profile TEXT NOT NULL DEFAULT 'apac',
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Workload → model routing (3 fixed workload tiers)
CREATE TABLE IF NOT EXISTS workload_routing (
    workload TEXT PRIMARY KEY CHECK (workload IN ('volume', 'tools', 'premium')),
    model_id TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1
);

INSERT OR IGNORE INTO workload_routing (workload, model_id) VALUES
    ('volume',  'apac.amazon.nova-micro-v1:0'),
    ('tools',   'global.anthropic.claude-haiku-4-5-20251001-v1:0'),
    ('premium', 'global.anthropic.claude-sonnet-4-6');

-- Inference metrics for cache-hit verification (R4 mitigation)
CREATE TABLE IF NOT EXISTS inference_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    workload TEXT NOT NULL CHECK (workload IN ('volume', 'tools', 'premium', 'unknown')),
    model_id TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    latency_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_inference_metrics_ts ON inference_metrics(ts);
