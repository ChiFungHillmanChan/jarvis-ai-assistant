# Local LLM Integration Design

**Date:** 2026-03-26
**Status:** Draft
**Scope:** Add local model backend support (Ollama, vLLM, generic OpenAI-compatible) to JARVIS

---

## 1. Goals

- **Privacy/offline:** Run JARVIS without sending data to cloud APIs
- **Cost reduction:** Avoid per-token charges by running inference locally
- **Experimentation:** Test and compare open-source models (DeepSeek, Qwen, Llama, etc.)
- **Hardware-agnostic:** Works on Apple Silicon, NVIDIA GPUs, cloud GPU instances, or any combination
- **Broad compatibility:** Support Ollama, vLLM, and any OpenAI-compatible endpoint (LM Studio, llama.cpp, TabbyAPI, etc.)

## 2. Architecture: Unified Client + Backend Trait Layer

### Approach

A `LocalBackend` trait handles endpoint-specific operations (list models, health check, capability detection). Actual chat inference goes through a single unified path using the OpenAI-compatible `/v1/chat/completions` protocol, which Ollama, vLLM, and most local servers all support.

### Why This Approach

- **Single inference path** -- one SSE streaming implementation for all local backends, fewer bugs
- **Backend-aware management** -- Ollama model listing, vLLM health checks, etc. still work through the trait
- **Extensible** -- adding a new backend means implementing one small trait, not a whole client
- **Minimal code** -- ~450 lines of new Rust, reuses existing OpenAI SSE parsing patterns

## 3. Core Data Model

### Endpoint Configuration

```rust
pub struct LocalEndpoint {
    pub id: String,                    // UUID v4
    pub name: String,                  // User label, e.g. "My Mac Ollama", "GPU Server"
    pub url: String,                   // e.g. "http://localhost:11434", "http://192.168.1.50:8000"
    pub backend: BackendType,          // Ollama | Vllm | Generic
    pub api_key: Option<String>,       // For authenticated remote endpoints
    pub use_tls: bool,                 // HTTPS requirement flag
    pub connection_timeout_ms: u32,    // Default 5000, configurable per endpoint
    pub keep_alive_minutes: u32,       // Ollama keep_alive, default 30
    pub is_active: bool,               // Include in provider chain
    pub last_health_check: Option<DateTime>,
    pub last_health_status: Option<bool>,
}

pub enum BackendType {
    Ollama,
    Vllm,
    Generic,  // Any OpenAI-compatible server
}
```

### Model Metadata (fetched at runtime, not persisted)

```rust
pub struct LocalModel {
    pub id: String,              // e.g. "qwen2.5:72b", "deepseek-v3"
    pub endpoint_id: String,
    pub context_length: u32,     // Detected or default 4096
    pub supports_tools: ToolCapability,
}

pub enum ToolCapability {
    Native,          // Model supports OpenAI-style tool calling
    PromptInjected,  // Tool schemas injected into system prompt
    ChatOnly,        // No tool support
}
```

### Provider Chain (replaces AiProvider enum)

```rust
pub struct ProviderChainEntry {
    pub position: i32,
    pub provider_type: ProviderKind,
    pub endpoint_id: Option<String>,  // Only for Local
    pub model_id: Option<String>,     // Only for Local
    pub enabled: bool,
}

pub enum ProviderKind {
    Claude,
    OpenAI,
    Local,
}
```

## 4. Backend Trait

```rust
#[async_trait]
pub trait LocalBackend: Send + Sync {
    async fn list_models(&self, url: &str, api_key: Option<&str>) -> Result<Vec<ModelInfo>, String>;
    async fn health_check(&self, url: &str, api_key: Option<&str>) -> Result<bool, String>;
    async fn detect_tool_capability(&self, url: &str, model: &str) -> ToolCapability;
}
```

### Implementation per backend

| Method | Ollama | vLLM | Generic |
|--------|--------|------|---------|
| `list_models` | `GET /api/tags` | `GET /v1/models` | `GET /v1/models` |
| `health_check` | `GET /` (returns "Ollama is running") | `GET /health` | `GET /v1/models` (200 = alive) |
| `detect_tool_capability` | Check model name prefix against known tool-capable families (qwen2.5, llama3.1+, mistral, deepseek-v3, command-r); fall back to a single test probe with a trivial tool call | Check model metadata for `--enable-auto-tool-choice` flag presence | Default to `PromptInjected` |

## 5. Unified Inference Path

### send_local()

```rust
pub async fn send_local(
    endpoint: &LocalEndpoint,
    model: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<serde_json::Value>,
    tool_capability: ToolCapability,
    app_handle: &AppHandle,
    tts_tx: Option<&StreamingTtsTx>,
) -> Result<String, String>
```

Flow:
1. Format messages into OpenAI-compatible `/v1/chat/completions` request
2. Handle tools based on capability tier (see below)
3. Stream SSE using `bytes_stream()` + event parsing (same pattern as `openai.rs`)
4. Run agentic tool loop (up to 5 iterations) for Native and PromptInjected modes
5. Manage context window via ContextManager

### Tool Calling Tiers

**Native:** Pass tools array in request body. Parse `tool_calls` from streamed response. Execute via existing `execute_tool()`. Same flow as `openai.rs`.

**PromptInjected:** Omit tools from request body. Inject schemas + calling instructions into system prompt. Use `response_format: {"type": "json_object"}` to constrain output. Parse `{"tool_calls": [...]}` from response. Fall back to regex extraction if json_object mode unavailable. Execute via `execute_tool()`.

**ChatOnly:** Omit tools entirely. Pure conversational response.

### Prompt Injection Format

When `ToolCapability::PromptInjected`, append to system prompt:

```
You have access to tools. To call a tool, respond with valid JSON:
{"tool_calls": [{"name": "tool_name", "arguments": {...}}]}

To respond without tools, use:
{"response": "your message here"}

Available tools:
[category-filtered tool schemas]
```

Tool selection for PromptInjected mode: group tools by category (system, email, calendar, github, notion, obsidian, render) and match on category keywords in the user's message. Only for PromptInjected mode -- Native mode sends all tools that fit the context window.

## 6. Context Window Management

### ContextManager

```rust
pub struct ContextManager {
    max_tokens: u32,
    reserve_for_response: u32,     // Default 1024
    reserve_for_tools: u32,        // Default 2000 (PromptInjected only)
}
```

### Token Counting

- **Pre-flight estimation:** `tiktoken-rs` crate for accurate token counting before sending
- **Post-response tracking:** Read `usage.prompt_tokens` and `usage.completion_tokens` from API response (Ollama returns `prompt_eval_count`/`eval_count`, vLLM returns standard `usage` object)
- **Calibration:** Compare estimates vs actuals over time to improve accuracy per model

### Context Length Detection

| Backend | API | Field |
|---------|-----|-------|
| Ollama | `GET /api/show` | `num_ctx` parameter |
| vLLM | `GET /v1/models` | `max_model_len` |
| Generic | User-configured | Default 4096 |

### Truncation Strategy

Priority order (trim least important first):

1. **Oldest conversation messages** -- always preserve system prompt + latest user message
2. **Day context** (tasks, calendar, emails) -- summarize or skip if context < 8K
3. **Tool schemas** (PromptInjected) -- send category-relevant subset, not all 34

### Rolling Summary

When messages are pushed out of the context window, instead of discarding:
1. Summarize evicted messages into a "conversation so far" block
2. Place summary after system prompt, before active messages
3. Re-summarize the summary block when it grows beyond ~500 tokens

This costs one extra inference call when the window fills but preserves conversation continuity.

### Prompt Cache Optimization

Structure prompts for KV-cache efficiency:
1. **System prompt** (stable, cached across requests)
2. **Tool schemas** (stable per conversation)
3. **Rolling summary** (changes slowly)
4. **Active messages** (changes every turn)

vLLM has automatic prefix caching. Ollama's `keep_alive` keeps model + KV cache in memory.

### Behavior by Context Size

| Context Length | Behavior |
|---------------|----------|
| < 4K | Chat-only, no tools, last 3 messages |
| 4K - 8K | Chat + 5 category-relevant tools (PromptInjected), last 5 messages |
| 8K - 32K | Full tool support, last 10 messages, day context included |
| 32K+ | Full feature parity with cloud models, last 20 messages |

### Ollama Keep-Alive

Default to 30 minutes (configurable per endpoint). For an always-on assistant, longer is better to avoid cold-start latency on each request.

## 7. AiRouter Evolution

### New Structure

```rust
pub struct AiRouter {
    claude_key: Option<String>,
    openai_key: Option<String>,
    provider_chain: Vec<ProviderChainEntry>,
    local_endpoints: Vec<LocalEndpoint>,
    backends: HashMap<BackendType, Box<dyn LocalBackend>>,
}
```

### Dispatch Logic

```
for each provider in chain (where enabled):
    match provider.provider_type:
        Claude   -> claude::send(...)
        OpenAI   -> openai::send(...)
        Local    ->
            1. Look up endpoint from local_endpoints by endpoint_id
            2. Skip if endpoint.last_health_status == false and last_health_check < 60s ago
            3. Detect or read cached tool_capability for model
            4. Call send_local(endpoint, model, messages, tools, capability, ...)

    if success -> emit chat-provider { provider, status: "active" }, return response
    if failure -> emit chat-provider { provider, status: "failed" }
                  emit chat-status "Retrying with next provider..."
                  continue to next

if all exhausted -> return error with list of failures
```

### Backward Compatibility

On first startup after migration, if `provider_chain` table is empty, read old `ai_provider` from `user_preferences` and convert:

| Old value | New chain entries |
|-----------|-------------------|
| `claude_primary` | `[Claude(pos=0), OpenAI(pos=1)]` |
| `openai_primary` | `[OpenAI(pos=0), Claude(pos=1)]` |
| `claude_only` | `[Claude(pos=0)]` |
| `openai_only` | `[OpenAI(pos=0)]` |

Old key left in place but ignored.

## 8. Database Migration (V6)

```sql
CREATE TABLE local_endpoints (
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

CREATE TABLE local_model_overrides (
    endpoint_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    context_length INTEGER,
    tool_capability TEXT DEFAULT 'auto'
        CHECK(tool_capability IN ('auto', 'native', 'prompt_injected', 'chat_only')),
    system_prompt_suffix TEXT,
    PRIMARY KEY (endpoint_id, model_id),
    FOREIGN KEY (endpoint_id) REFERENCES local_endpoints(id) ON DELETE CASCADE
);

CREATE TABLE provider_chain (
    position INTEGER NOT NULL PRIMARY KEY,
    provider_type TEXT NOT NULL
        CHECK(provider_type IN ('claude', 'openai', 'local')),
    endpoint_id TEXT,
    model_id TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (endpoint_id) REFERENCES local_endpoints(id) ON DELETE CASCADE
);
```

## 9. Tauri Commands

New file: `src-tauri/src/commands/local_llm.rs`

| Command | Purpose |
|---------|---------|
| `list_local_endpoints` | Return all configured endpoints |
| `add_local_endpoint(name, url, backend_type?, api_key?)` | Auto-detect backend, health check, fetch models, save |
| `update_local_endpoint(id, ...)` | Update endpoint config fields |
| `remove_local_endpoint(id)` | Delete endpoint, CASCADE cleans provider chain |
| `test_endpoint_connection(endpoint_id)` | Returns `{ reachable, model_count, latency_ms }` |
| `list_endpoint_models(endpoint_id)` | Fetch models from endpoint, detect tool capabilities |
| `get_provider_chain` | Return ordered chain entries |
| `update_provider_chain(chain)` | Validate references, update positions |
| `update_model_override(endpoint_id, model_id, ...)` | Set per-model context/tool/prompt overrides |

All registered in `lib.rs` invoke_handler. Typed wrappers in `src/lib/commands.ts`.

### New Event

```
chat-provider: { provider: string, status: "trying" | "failed" | "active" }
```

Frontend shows: "Responding via qwen2.5:72b (local)" or "Falling back to Claude..."

## 10. Frontend Changes

### New Components

**`src/components/settings/EndpointManager.tsx`**
- List of configured endpoints with status badges
- Each card shows: name, URL, backend type, status dot ("Last seen: 3min ago"), model count, authenticated badge
- Actions: Test Connection, Refresh Models, Remove
- "Add Endpoint" form: URL input, auto-detects backend, fetches models

**`src/components/settings/ProviderChainBuilder.tsx`**
- Ordered list of providers with drag handles + up/down arrow buttons
- Each entry: provider name/model, enabled toggle
- "Add Provider" dropdown: Claude, OpenAI, or pick model from any active endpoint
- Visual indicator of currently active provider

**`src/components/settings/ModelOverridePanel.tsx`**
- Expandable row per local model in the chain
- Fields: context length override, tool capability override (Auto/Native/PromptInjected/ChatOnly), keep_alive duration, custom system prompt suffix

### Modified Components

**`src/pages/Settings.tsx`** -- add two new sections for Endpoints and Provider Chain

**`src/components/ChatPanel.tsx`** -- listen to `chat-provider` event, show active provider indicator

### New Hook

**`src/hooks/useProviderChain.ts`** -- fetch/update provider chain with optimistic updates

### New Types (src/lib/types.ts)

```typescript
interface LocalEndpoint {
  id: string;
  name: string;
  url: string;
  backend_type: 'ollama' | 'vllm' | 'generic';
  api_key?: string;
  use_tls: boolean;
  connection_timeout_ms: number;
  keep_alive_minutes: number;
  is_active: boolean;
  last_health_check?: string;
  last_health_status?: boolean;
}

interface LocalModel {
  id: string;
  endpoint_id: string;
  context_length: number;
  supports_tools: 'native' | 'prompt_injected' | 'chat_only';
}

interface ProviderChainEntry {
  position: number;
  provider_type: 'claude' | 'openai' | 'local';
  endpoint_id?: string;
  model_id?: string;
  enabled: boolean;
}

interface EndpointHealth {
  reachable: boolean;
  model_count: number;
  latency_ms: number;
}
```

## 11. File Structure

### New Files

```
src-tauri/src/ai/local/
  mod.rs              -- send_local(), LocalLlm client
  backend.rs          -- LocalBackend trait definition
  ollama.rs           -- Ollama trait implementation
  vllm.rs             -- vLLM trait implementation
  generic.rs          -- Generic OpenAI-compatible implementation
  context.rs          -- ContextManager, token counting, truncation, summarization
  prompt_inject.rs    -- Tool schema injection, JSON response parsing

src-tauri/src/commands/
  local_llm.rs        -- All local LLM Tauri commands

src-tauri/migrations/
  V6__local_llm.sql   -- 3 new tables

src/components/settings/
  EndpointManager.tsx
  ProviderChainBuilder.tsx
  ModelOverridePanel.tsx

src/hooks/
  useProviderChain.ts
```

### Modified Files

```
src-tauri/src/ai/mod.rs        -- AiRouter with provider chain
src-tauri/src/lib.rs            -- Register new commands, init backends
src-tauri/Cargo.toml            -- Add tiktoken-rs, uuid
src/lib/types.ts                -- Add local LLM types
src/lib/commands.ts             -- Add local LLM command wrappers
src/pages/Settings.tsx          -- Add endpoint + chain sections
src/components/ChatPanel.tsx    -- Provider indicator
```

### Unchanged

- `ai/claude.rs`, `ai/openai.rs`, `ai/tools.rs`
- All integration modules (Gmail, Calendar, GitHub, Notion, Obsidian)
- Voice, scheduler, 3D scene, all other pages
- All existing commands except minor changes to `chat.rs`

## 12. Dependencies

```toml
# src-tauri/Cargo.toml additions
tiktoken-rs = "0.6"     # Token counting (pure Rust, no C bindings)
uuid = { version = "1", features = ["v4"] }  # Endpoint IDs
```

No frontend dependency additions needed. Drag-to-reorder uses native HTML5 drag and drop.

## 13. Not In Scope

- Model downloading/pulling from within JARVIS (use Ollama CLI or vLLM directly)
- Fine-tuning or LoRA adapter management
- GPU monitoring or resource usage display
- Batch inference or background processing
- Embedding model support (for future RAG features)
- Changes to the existing Claude or OpenAI client code
