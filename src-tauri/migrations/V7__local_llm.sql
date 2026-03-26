-- Local LLM endpoint management
CREATE TABLE IF NOT EXISTS local_endpoints (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    backend_type TEXT NOT NULL DEFAULT 'generic'
        CHECK(backend_type IN ('ollama', 'vllm', 'generic')),
    api_key TEXT,
    use_tls INTEGER NOT NULL DEFAULT 0,
    connection_timeout_ms INTEGER NOT NULL DEFAULT 5000,
    keep_alive_minutes INTEGER NOT NULL DEFAULT 30,
    is_active INTEGER NOT NULL DEFAULT 1,
    last_health_check DATETIME,
    last_health_status INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Per-model configuration overrides
CREATE TABLE IF NOT EXISTS local_model_overrides (
    endpoint_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    context_length INTEGER,
    tool_capability TEXT DEFAULT 'auto'
        CHECK(tool_capability IN ('auto', 'native', 'prompt_injected', 'chat_only')),
    system_prompt_suffix TEXT,
    PRIMARY KEY (endpoint_id, model_id),
    FOREIGN KEY (endpoint_id) REFERENCES local_endpoints(id) ON DELETE CASCADE
);

-- Provider priority chain (replaces single ai_provider preference)
CREATE TABLE IF NOT EXISTS provider_chain (
    position INTEGER NOT NULL PRIMARY KEY,
    provider_type TEXT NOT NULL
        CHECK(provider_type IN ('claude', 'openai', 'local')),
    endpoint_id TEXT,
    model_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (endpoint_id) REFERENCES local_endpoints(id) ON DELETE CASCADE
);
