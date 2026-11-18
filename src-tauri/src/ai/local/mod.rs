pub mod backend;
pub mod context;
pub mod generic;
pub mod ollama;
pub mod prompt_inject;
pub mod vllm;

use backend::{BackendType, LocalBackend, LocalEndpoint, ToolCapability};
use context::ContextManager;
pub(crate) use prompt_inject::parse_tool_calls_from_response;
use prompt_inject::{build_tool_injection_prompt, effective_tool_capability, select_relevant_tools};

use crate::ai::tools;
use reqwest::Client;
use serde::Serialize;
use serde_json::json;
// Local servers' streamed replies are read by the OpenAI wire's parser.
#[cfg(test)]
use crate::ai::openai::SseChunk;

/// A chat request to a local server's OpenAI-compatible endpoint.
#[derive(Serialize)]
pub(crate) struct ChatCompletionRequest<M> {
    model: String,
    messages: Vec<M>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    /// "none" on the last allowed round when native tools are sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<serde_json::Value>,
}

/// Get the appropriate backend implementation
pub fn get_backend(backend_type: &BackendType) -> Box<dyn LocalBackend> {
    match backend_type {
        BackendType::Ollama => Box::new(ollama::OllamaBackend),
        BackendType::Vllm => Box::new(vllm::VllmBackend),
        BackendType::Generic => Box::new(generic::GenericBackend),
    }
}

/// Auto-detect the backend type of a given URL
pub async fn detect_backend(url: &str) -> BackendType {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    // Try Ollama first
    if let Ok(resp) = client
        .get(format!("{}/", url.trim_end_matches('/')))
        .send()
        .await
    {
        if let Ok(body) = resp.text().await {
            if body.contains("Ollama") {
                return BackendType::Ollama;
            }
        }
    }

    // Try vLLM health endpoint
    if let Ok(resp) = client
        .get(format!("{}/health", url.trim_end_matches('/')))
        .send()
        .await
    {
        if resp.status().is_success() {
            return BackendType::Vllm;
        }
    }

    BackendType::Generic
}

/// Told to a model that takes no tools (chat only, set by the user or a
/// context under 4,096 tokens). The prompt above it says to use tools and
/// to try them before saying anything is out of reach; without this the
/// model may say it checked or saved something it could not.
pub(crate) const NO_TOOLS_NOTE: &str = "\n\nIn this conversation you have no tools: the model JARVIS is using cannot call them. The instructions above about calling tools do not apply. Never say or suggest that you checked, opened, created, saved or remembered anything. When the user asks for something that needs a tool (their mail, calendar, tasks, files, apps or your memory), say in one sentence that this model cannot use tools, so it cannot do that here, and that they can choose a model that supports tools in Settings.";

/// A chat model on a local server, which the OpenAI wire talks to: how it
/// takes tools (natively, described in the system prompt, or not at all),
/// its context window and where it is.
pub(crate) struct LocalChat {
    endpoint: LocalEndpoint,
    capability: ToolCapability,
    context_length: u32,
}

impl LocalChat {
    /// The model's tool mode: the user's setting, else what was detected; a
    /// context under 4,096 tokens takes no tools.
    pub(crate) fn new(endpoint: LocalEndpoint, detected: ToolCapability, context_length: u32, tool_capability_override: Option<&str>) -> Self {
        let capability = effective_tool_capability(&detected, tool_capability_override, context_length);
        LocalChat { endpoint, capability, context_length }
    }

    pub(crate) fn client(&self) -> Result<Client, String> {
        Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_millis(self.endpoint.connection_timeout_ms as u64))
            .build()
            .map_err(|e| e.to_string())
    }

    pub(crate) fn url(&self) -> String {
        format!("{}/v1/chat/completions", self.endpoint.url.trim_end_matches('/'))
    }

    pub(crate) fn api_key(&self) -> Option<&str> {
        self.endpoint.api_key.as_deref()
    }

    /// The model calls tools itself.
    pub(crate) fn sends_tools(&self) -> bool {
        matches!(self.capability, ToolCapability::Native)
    }

    /// The tools are described in the system prompt and the model answers
    /// in JSON, calls included.
    pub(crate) fn injects_tools(&self) -> bool {
        matches!(self.capability, ToolCapability::PromptInjected)
    }

    /// The system prompt, with the tools that fit described for a model that
    /// cannot call them itself, or a model that takes no tools told so; and
    /// the conversation cut to fit the context.
    pub(crate) fn prepare(&self, mut system_prompt: String, messages: Vec<(String, String)>, tools: &[tools::Tool]) -> (String, Vec<(String, String)>) {
        if self.injects_tools() {
            let max_tools = ContextManager::max_tools_for_context(self.context_length);
            let user_msg = messages.last().map(|(_, c)| c.as_str()).unwrap_or("");
            let relevant = select_relevant_tools(tools, user_msg, max_tools);
            system_prompt.push_str(&build_tool_injection_prompt(&relevant));
        }
        if matches!(self.capability, ToolCapability::ChatOnly) {
            system_prompt.push_str(NO_TOOLS_NOTE);
        }
        let ctx_manager = ContextManager::new(self.context_length, self.injects_tools());
        let system_tokens = ContextManager::count_tokens(&system_prompt);
        let truncated = ctx_manager.truncate_messages(&messages, system_tokens);
        (system_prompt, truncated)
    }

    /// One request. Ollama and vLLM honour stream_options.include_usage on
    /// their OpenAI-compatible endpoint, which is what lets the cost
    /// dashboard show real local token counts. A Generic server may reject
    /// the unknown field, so it is left off there and the turn is recorded
    /// with zero tokens.
    pub(crate) fn request<M>(&self, model: &str, messages: Vec<M>, tools: Option<Vec<serde_json::Value>>, final_round: bool) -> ChatCompletionRequest<M> {
        ChatCompletionRequest {
            model: model.to_string(),
            messages,
            tool_choice: (final_round && tools.is_some()).then_some("none"),
            tools,
            max_tokens: Some(crate::ai::models::local_chat_max_output_tokens(self.context_length)),
            stream: true,
            response_format: self.injects_tools().then(|| json!({"type": "json_object"})),
            stream_options: (self.endpoint.backend_type != BackendType::Generic).then(|| json!({"include_usage": true})),
        }
    }
}

/// The reply when the last round's text asks for tools: none are run, as on
/// the native path (a tool run there was followed by "ran out of steps", and
/// the user's retry repeated it). Plain text around the call is kept. None
/// when the text asks for no tool.
pub(crate) fn last_round_reply(full_text: &str, language: &str) -> Option<String> {
    let (calls, text) = parse_tool_calls_from_response(full_text);
    calls.filter(|calls| !calls.is_empty())?;
    Some(if text.trim().is_empty() { tools::tool_limit_fallback(language).to_string() } else { text })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The last round runs no tools, as on the native path: a tool run there
    /// was followed by "ran out of steps", and the user's retry repeated it.
    #[test]
    fn the_last_round_answers_instead_of_running_an_injected_tool() {
        let call = r#"{"tool_calls":[{"name":"create_task","arguments":{"title":"Call Mary"}}]}"#;
        let reply = last_round_reply(call, "en").expect("a tool call on the last round is answered, not run");
        assert_eq!(reply, tools::tool_limit_fallback("en"));
        assert!(!reply.contains("create_task"), "{reply}");
        assert_eq!(last_round_reply("You have two meetings today.", "en"), None, "plain text goes the usual way");
    }

    #[test]
    fn sse_chunk_parses_usage_and_tolerates_its_absence() {
        let with_usage: SseChunk = serde_json::from_str(
            r#"{"choices":[],"usage":{"prompt_tokens":128,"completion_tokens":64}}"#,
        )
        .unwrap();
        let u = with_usage.usage.unwrap();
        assert_eq!(u.prompt_tokens, Some(128));
        assert_eq!(u.completion_tokens, Some(64));

        // A backend that never sends usage must still stream normally.
        let without: SseChunk =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"hi"}}]}"#).unwrap();
        assert!(without.usage.is_none());
    }

    /// A local turn must cost 0 because it ran on the user's own hardware --
    /// which is a fact about the provider, not about whether the model id
    /// happens to be missing from the cloud price table. An unrecognised cloud
    /// id is stored with the cost unknown, so this asserts the local-provider
    /// rule rather than the table lookup.
    #[test]
    fn local_turns_are_recorded_at_zero_cost() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        crate::ai::cost::record_inference(
            &db,
            "local",
            crate::ai::cost::Feature::Chat,
            crate::ai::Workload::Tools,
            "llama3.1:8b-instruct-q4_K_M",
            4000,
            900,
            0,
            0,
            120,
        );
        let conn = db.conn.lock().unwrap();
        let (cost, turns, unknown): (i64, i64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(usd_cost_micro), 0), COUNT(*), SUM(price_unknown) FROM inference_metrics",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(cost, 0, "local inference must cost nothing");
        assert_eq!(turns, 1, "but the turn must still be counted");
        assert_eq!(unknown, 0, "free is a known price");
    }
}
