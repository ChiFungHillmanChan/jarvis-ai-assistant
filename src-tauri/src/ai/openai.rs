use super::tools;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use super::provider::{Call, ChatRequest, Event, Events, Metering, Outcome, Round, RoundError, Wire, SKIPPED};
use super::complete::{Completion, Request as PlainRequest};
use super::bedrock::UiLanguage;

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    tools: Vec<serde_json::Value>,
    /// api.openai.com only; compatible services take `max_tokens`.
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// gpt-6-luna rejects function tools on Chat Completions unless reasoning
    /// is off ("Function tools with reasoning_effort are not supported"),
    /// verified live 2026-09-25. Not sent to compatible services.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    /// "none" on the last allowed round, so the model answers from what the
    /// earlier rounds gathered instead of asking for another tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    stream: bool,
    stream_options: StreamOptions,
}

/// Where a chat turn goes: api.openai.com, or an OpenAI-compatible service
/// such as Kimi or Qwen (a chain row with a `base_url`).
#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    /// None is api.openai.com.
    pub base_url: Option<&'a str>,
    /// The chain row's model. On api.openai.com, None follows the registry's default.
    pub model: Option<&'a str>,
    /// "Think harder" (P06), as far as the model takes it. Not offered on
    /// this wire yet (capability::deeper), so None outside tests.
    pub deeper: Option<super::capability::Deeper>,
}

impl Target<'_> {
    fn url(&self) -> String {
        match self.base_url {
            Some(base) => format!("{base}/chat/completions"),
            None => format!("{}/chat/completions", super::api_base("https://api.openai.com/v1")),
        }
    }

    fn model(&self) -> Result<String, String> {
        match self.base_url {
            None => Ok(self.model.map(str::trim).filter(|m| !m.is_empty()).map(str::to_string).unwrap_or_else(crate::ai::models::openai_default)),
            Some(base) => self
                .model
                .map(str::to_string)
                .filter(|m| !m.trim().is_empty())
                .ok_or_else(|| format!("尚未為 {} 選擇模型，請在設定中選擇。", super::compat::label(base))),
        }
    }

    /// Provider id in the cost table.
    fn provider(&self) -> String {
        super::cost::provider_id("openai", self.base_url)
    }

    fn request(&self, model: String, messages: Vec<OpenAIMessage>, tools: Vec<serde_json::Value>, final_round: bool) -> OpenAIRequest {
        let official = self.base_url.is_none();
        // The registry's limits: OpenAI's own, or the compatible service's.
        let limits = crate::ai::models::limits(crate::ai::models::openai_format_provider(self.base_url));
        let cap = limits.chat_max_output_tokens.unwrap_or_default();
        OpenAIRequest {
            model,
            messages,
            tools,
            max_completion_tokens: official.then_some(cap),
            max_tokens: (!official).then_some(cap),
            reasoning_effort: match (official, self.deeper) {
                (true, Some(deeper)) if deeper.effort => Some("high".into()),
                (true, _) => limits.chat_reasoning_effort,
                (false, _) => None,
            },
            tool_choice: final_round.then_some("none"),
            stream: true,
            stream_options: StreamOptions { include_usage: true },
        }
    }
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub(crate) struct OpenAIUsage {
    /// Includes the cached tokens below.
    pub(crate) prompt_tokens: Option<i64>,
    pub(crate) completion_tokens: Option<i64>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize, Debug, Default, Clone)]
struct PromptTokensDetails {
    cached_tokens: Option<i64>,
}

impl OpenAIUsage {
    /// One request's figures. OpenAI caches long prompt prefixes on its own
    /// and bills them at the cached rate; prompt_tokens includes them.
    fn figures(&self) -> super::provider::Usage {
        let cached = self.prompt_tokens_details.as_ref().and_then(|d| d.cached_tokens).unwrap_or(0);
        super::provider::Usage { input: self.prompt_tokens.unwrap_or(0), output: self.completion_tokens.unwrap_or(0), cache_read: cached, cache_write: 0 }
    }

    /// Fold this request into a turn, as the usage path does.
    #[cfg(test)]
    fn add_to(&self, turn: &mut crate::ai::cost::TurnUsage) {
        let figures = self.figures();
        turn.add_request(figures.input, figures.output, figures.cache_read, figures.cache_write);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: FunctionCall,
    /// Provider data that must go back with the call, e.g. Gemini's
    /// `thought_signature` on its OpenAI-compatible endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra_content: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FunctionCall {
    name: String,
    arguments: String,
}

// OpenAIResponse and OpenAIChoice are no longer needed (SSE replaces JSON parsing)

/// One streamed chunk, from OpenAI, a compatible service or a local server.
#[derive(Deserialize)]
pub(crate) struct SseChunk {
    /// Absent, or null from some local servers, on a usage-only chunk.
    #[serde(default, deserialize_with = "null_as_empty")]
    choices: Vec<SseChoice>,
    pub(crate) usage: Option<OpenAIUsage>,
}

fn null_as_empty<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<SseChoice>, D::Error> {
    Ok(Option::<Vec<SseChoice>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
struct SseChoice {
    delta: Option<SseDeltaContent>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct SseDeltaContent {
    content: Option<String>,
    tool_calls: Option<Vec<SseToolCallDelta>>,
}

#[derive(Deserialize)]
struct SseToolCallDelta {
    /// Some compatible services (Gemini's) leave it out; the position in
    /// the list stands in.
    #[serde(default)]
    index: Option<usize>,
    id: Option<String>,
    function: Option<SseFunctionDelta>,
    #[serde(default)]
    extra_content: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct SseFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

/// The OpenAI format on the provider interface: api.openai.com, an
/// OpenAI-compatible service at the chain row's address, or a model on the
/// user's own server.
pub(crate) struct OpenAiWire {
    key: String,
    base_url: Option<String>,
    model: String,
    deeper: Option<super::capability::Deeper>,
    /// A local server's own options: its address, key and tool mode.
    local: Option<super::local::LocalChat>,
}

impl OpenAiWire {
    /// Fails when a compatible service has no model chosen.
    pub(crate) fn new(key: &str, target: Target<'_>) -> Result<Self, String> {
        let model = target.model()?;
        Ok(OpenAiWire { key: key.to_string(), base_url: target.base_url.map(str::to_string), model, deeper: target.deeper, local: None })
    }

    /// A model on a local server.
    pub(crate) fn local(chat: super::local::LocalChat, model: &str) -> Self {
        OpenAiWire { key: String::new(), base_url: None, model: model.to_string(), deeper: None, local: Some(chat) }
    }

    /// The tools are described in the system prompt and the calls come back
    /// in the reply's JSON text.
    fn injects_tools(&self) -> bool {
        self.local.as_ref().is_some_and(super::local::LocalChat::injects_tools)
    }

    /// One request's figures. A local turn stores no cache figures.
    fn figures(&self, usage: &OpenAIUsage) -> super::provider::Usage {
        let figures = usage.figures();
        match self.local {
            Some(_) => super::provider::Usage { cache_read: 0, cache_write: 0, ..figures },
            None => figures,
        }
    }

    fn target(&self) -> Target<'_> {
        Target { base_url: self.base_url.as_deref(), model: Some(&self.model), deeper: self.deeper }
    }

    /// The service's name in an error.
    fn name(&self) -> String {
        if self.local.is_some() {
            return "Local LLM".into();
        }
        self.base_url.as_deref().map(super::compat::label).unwrap_or_else(|| "OpenAI".into())
    }
}

/// One chat turn's requests in the OpenAI format.
pub(crate) struct Conversation {
    client: Client,
    messages: Vec<OpenAIMessage>,
    tools: Vec<serde_json::Value>,
    /// The assistant message of the round in progress, sent back with the
    /// results of its calls. None when the round's calls, if any, came in
    /// the text of a model the tools are described to.
    reply: Option<OpenAIMessage>,
}

#[async_trait::async_trait]
impl Wire for OpenAiWire {
    type Conversation = Conversation;

    fn max_rounds(&self) -> usize {
        if self.local.is_some() {
            return crate::ai::models::max_tool_rounds("ollama");
        }
        crate::ai::models::max_tool_rounds(crate::ai::models::openai_format_provider(self.base_url.as_deref()))
    }

    /// A local turn costs nothing and is stored only when it succeeds, so
    /// a failed request does not inflate the turn count.
    fn metering(&self) -> Metering {
        if self.local.is_some() {
            return Metering::AnsweredTurn { provider: "local".into(), model: self.model.clone() };
        }
        Metering::Turn { provider: self.target().provider(), model: self.model.clone() }
    }

    async fn start(&self, request: ChatRequest) -> Result<Conversation, String> {
        let (client, system, history) = match &self.local {
            Some(local) => {
                let (system, history) = local.prepare(request.system.joined(), request.messages, &request.tools);
                (local.client()?, system, history)
            }
            None => {
                let client = Client::builder()
                    .read_timeout(std::time::Duration::from_secs(60))
                    // A redirect would carry the key and the conversation elsewhere.
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|e| e.to_string())?;
                (client, request.system.joined(), request.messages)
            }
        };
        let mut messages = vec![OpenAIMessage { role: "system".into(), content: Some(system), tool_calls: None, tool_call_id: None }];
        messages.extend(
            history
                .into_iter()
                .filter(|(_, content)| !content.trim().is_empty())
                .map(|(role, content)| OpenAIMessage { role, content: Some(content), tool_calls: None, tool_call_id: None }),
        );
        Ok(Conversation { client, messages, tools: tools::to_openai_format(&request.tools), reply: None })
    }

    async fn round(&self, conversation: &mut Conversation, final_round: bool, events: &mut Events<'_>) -> Result<(), RoundError> {
        let client = &conversation.client;
        let response = match &self.local {
            // Sent once: a local server that failed is not busy elsewhere.
            Some(local) => {
                let tools = local.sends_tools().then(|| conversation.tools.clone());
                let request = local.request(&self.model, conversation.messages.clone(), tools, final_round);
                let mut builder = client.post(local.url()).header("Content-Type", "application/json");
                if let Some(key) = local.api_key() {
                    builder = builder.header("Authorization", format!("Bearer {}", key));
                }
                builder.json(&request).send().await
            }
            None => {
                let target = self.target();
                let request = target.request(self.model.clone(), conversation.messages.clone(), conversation.tools.clone(), final_round);
                send_retrying(|| {
                    client
                        .post(target.url())
                        .header("Authorization", format!("Bearer {}", self.key))
                        .header("Content-Type", "application/json")
                        .json(&request)
                })
                .await
            }
        }
        .map_err(|e| RoundError::refused(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RoundError::refused(format!("{} API error {}: {}", self.name(), status, body)));
        }

        // One usage chunk per request: the last one stands.
        let mut usage = OpenAIUsage::default();
        let mut stream = response.bytes_stream();
        let mut buffer = super::SseByteBuffer::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_map: ToolSlots = HashMap::new(); // slot -> (call_id, fn_name, arguments, extra_content)
        let mut finish_reason: Option<String> = None;

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => bytes,
                Err(e) => {
                    events.send(Event::Usage(self.figures(&usage)));
                    return Err(RoundError::broken(format!("Stream error: {}", e)));
                }
            };
            buffer.extend(&chunk);

            while let Some(event_block) = buffer.next_event() {
                let mut data_str = String::new();
                for line in event_block.lines() {
                    if line.starts_with(':') {
                        continue;
                    }
                    if let Some(rest) = line.strip_prefix("data: ") {
                        data_str = rest.to_string();
                    }
                }
                if data_str.is_empty() || data_str == "[DONE]" {
                    continue;
                }
                let chunk: SseChunk = match serde_json::from_str(&data_str) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if let Some(u) = chunk.usage.clone() {
                    usage = u;
                }
                for choice in &chunk.choices {
                    if let Some(ref delta) = choice.delta {
                        if let Some(ref content) = delta.content {
                            text_parts.push(content.clone());
                            events.send(Event::Text(content.clone()));
                        }
                        if let Some(ref tool_calls) = delta.tool_calls {
                            for (position, tc) in tool_calls.iter().enumerate() {
                                let index = tool_slot(&tool_map, tc.index, tc.id.as_deref(), position);
                                let entry = tool_map.entry(index).or_insert_with(|| (String::new(), String::new(), String::new(), None));
                                if let Some(ref id) = tc.id {
                                    entry.0 = id.clone();
                                }
                                if tc.extra_content.is_some() {
                                    entry.3 = tc.extra_content.clone();
                                }
                                if let Some(ref func) = tc.function {
                                    if let Some(ref name) = func.name {
                                        entry.1 = name.clone();
                                        events.send(Event::ToolStarted { key: index.to_string(), name: name.clone() });
                                    }
                                    if let Some(ref args) = func.arguments {
                                        entry.2.push_str(args);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(ref fr) = choice.finish_reason {
                        finish_reason = Some(fr.clone());
                    }
                }
            }
        }
        events.send(Event::Usage(self.figures(&usage)));

        let mut slots: Vec<usize> = tool_map.keys().copied().collect();
        slots.sort();
        let tool_calls: Vec<ToolCall> = slots
            .iter()
            .map(|slot| {
                let (ref id, ref name, ref arguments, ref extra) = tool_map[slot];
                ToolCall { id: id.clone(), call_type: "function".into(), function: FunctionCall { name: name.clone(), arguments: arguments.clone() }, extra_content: extra.clone() }
            })
            .collect();
        for call in &tool_calls {
            events.send(Event::ToolCall(Call { id: call.id.clone(), name: call.function.name.clone(), arguments: call.function.arguments.clone() }));
        }
        let text = text_parts.join("");
        if tool_calls.is_empty() && self.injects_tools() {
            // Shown as it streamed; not announced before it runs.
            for call in super::local::parse_tool_calls_from_response(&text).0.into_iter().flatten() {
                let arguments = serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".into());
                events.send(Event::ToolCall(Call { id: String::new(), name: call.name, arguments }));
            }
        }
        conversation.reply = (!tool_calls.is_empty()).then(|| OpenAIMessage {
            role: "assistant".into(),
            content: (!text.is_empty()).then_some(text),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        });
        events.send(Event::Done { stop: finish_reason });
        Ok(())
    }

    /// Some compatible services finish a tool-call reply with "stop", so any
    /// complete call counts; a reply cut off by the length limit may carry
    /// half-written arguments. A local server's calls run only on
    /// "tool_calls"; calls written in the text of a model the tools are
    /// described to always run. (The loop runs none on the last round: a
    /// service that ignores tool_choice gets the fallback.)
    fn runs_calls(&self, conversation: &Conversation, round: &Round) -> bool {
        if round.calls.is_empty() {
            return false;
        }
        match (&self.local, &conversation.reply) {
            (None, _) => round.stop.as_deref() != Some("length"),
            (Some(_), Some(_)) => round.stop.as_deref() == Some("tool_calls"),
            (Some(_), None) => true,
        }
    }

    fn add_results(&self, conversation: &mut Conversation, round: Round, results: Vec<(Call, Outcome)>, _next_is_final: bool) -> Result<(), String> {
        if conversation.reply.is_none() {
            // Calls written in the text: each result goes back as the
            // model's text and a user message, and a repeated call is
            // dropped without a word.
            let text = round.text();
            for (call, outcome) in results {
                let Outcome::Ran(result) = outcome else { continue };
                conversation.messages.push(OpenAIMessage { role: "assistant".into(), content: Some(text.clone()), tool_calls: None, tool_call_id: None });
                conversation.messages.push(OpenAIMessage {
                    role: "user".into(),
                    content: Some(format!(
                        "Tool '{}' returned: {}\n\nNow respond to the user based on this result. Do NOT call any more tools. Respond in plain text.",
                        call.name, result
                    )),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            return Ok(());
        }
        conversation.messages.extend(conversation.reply.take());
        for (call, outcome) in results {
            let content = match outcome {
                Outcome::Ran(result) => result,
                Outcome::Duplicate => SKIPPED.to_string(),
            };
            conversation.messages.push(OpenAIMessage { role: "tool".into(), content: Some(content), tool_calls: None, tool_call_id: Some(call.id) });
        }
        Ok(())
    }

    /// A model the tools are described to answers in JSON: its "response"
    /// is the reply, and a call on the last round is answered, not run.
    fn reply(&self, conversation: &Conversation, round: Round, final_round: bool, language: &str) -> Result<String, String> {
        let text = round.text();
        if self.injects_tools() && conversation.reply.is_none() {
            if final_round {
                if let Some(reply) = super::local::last_round_reply(&text, language) {
                    return Ok(reply);
                }
            }
            let (_, response) = super::local::parse_tool_calls_from_response(&text);
            if !response.is_empty() {
                return Ok(response);
            }
        }
        if final_round && text.trim().is_empty() {
            return Ok(tools::tool_limit_fallback(language).into());
        }
        Ok(text)
    }

    async fn complete(&self, request: &PlainRequest<'_>, _language: UiLanguage) -> Result<Completion, String> {
        let client = super::compat::client()?;
        if let Some(local) = &self.local {
            return local.complete(&client, &self.model, request).await;
        }
        let name = self.name();
        let body = chat_completions_body(&self.model, request, self.base_url.is_none());
        let value = super::complete::post(&client, &self.target().url(), &name, |r| r.bearer_auth(&self.key), &body).await?;
        chat_completions_result(&value, name, self.model.clone())
    }
}

/// OpenAI Chat Completions body. api.openai.com takes
/// `max_completion_tokens`; compatible services and Ollama take `max_tokens`.
pub(crate) fn chat_completions_body(model: &str, request: &PlainRequest<'_>, official: bool) -> serde_json::Value {
    let mut messages = Vec::new();
    if !request.system.is_empty() {
        messages.push(json!({"role": "system", "content": request.system}));
    }
    messages.push(json!({"role": "user", "content": request.user}));
    let mut body = json!({"model": model, "messages": messages});
    body[if official { "max_completion_tokens" } else { "max_tokens" }] = json!(request.max_tokens);
    if request.json {
        body["response_format"] = json!({"type": "json_object"});
    }
    body
}

pub(crate) fn chat_completions_result(value: &serde_json::Value, provider: String, model: String) -> Result<Completion, String> {
    let text = value.pointer("/choices/0/message/content").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let mut done = super::complete::done(text, provider, model, value.pointer("/usage/prompt_tokens"), value.pointer("/usage/completion_tokens"))?;
    done.cached_tokens = value.pointer("/usage/prompt_tokens_details/cached_tokens").and_then(serde_json::Value::as_i64).unwrap_or(0);
    Ok(done)
}

type ToolSlots = HashMap<usize, (String, String, String, Option<serde_json::Value>)>;

/// Which accumulating call a streamed tool-call piece belongs to. OpenAI
/// numbers them with `index`. Gemini's compatible endpoint sends each call
/// whole, without an index, one per chunk: a new `id` is a new call.
fn tool_slot(slots: &ToolSlots, index: Option<usize>, id: Option<&str>, position: usize) -> usize {
    if let Some(index) = index {
        return index;
    }
    match id {
        Some(id) => slots
            .iter()
            .find(|(_, slot)| slot.0 == id)
            .map(|(existing, _)| *existing)
            .unwrap_or_else(|| slots.keys().max().map_or(0, |max| max + 1)),
        None => position,
    }
}

/// Send once more after a transient failure (429 or 5xx): with "Choose one
/// AI" there is no other provider to fall back to, and a resend usually works.
async fn send_retrying(build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, reqwest::Error> {
    let response = build().send().await?;
    let retry_after = response.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok());
    match retry_delay(response.status().as_u16(), retry_after) {
        Some(delay) => {
            log::warn!("[openai] {} -- retrying once in {:?}", response.status(), delay);
            drop(response);
            tokio::time::sleep(delay).await;
            build().send().await
        }
        None => Ok(response),
    }
}

/// The wait before the one retry, or None when the status is not transient or
/// the service asks for a longer wait than the user should sit through.
fn retry_delay(status: u16, retry_after: Option<&str>) -> Option<std::time::Duration> {
    if !matches!(status, 429 | 500 | 502 | 503 | 504) {
        return None;
    }
    match retry_after.map(|v| v.trim().parse::<f64>()) {
        None | Some(Err(_)) => Some(std::time::Duration::from_secs(1)),
        Some(Ok(secs)) if (0.0..=5.0).contains(&secs) => Some(std::time::Duration::from_secs_f64(secs)),
        Some(Ok(_)) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The output cap and reasoning effort are the registry's: OpenAI's own
    /// for api.openai.com, the compatible service's for any other address.
    #[test]
    fn request_limits_follow_the_registry() {
        let _registry = crate::ai::registry::test_override::with(|r| {
            fn limits<'a>(r: &'a mut crate::ai::registry::Registry, id: &str) -> &'a mut crate::ai::registry::doc::Limits {
                &mut r.providers.iter_mut().find(|p| p.id == id).unwrap().limits
            }
            limits(r, "openai").chat_max_output_tokens = Some(1234);
            limits(r, "openai").chat_reasoning_effort = Some("low".into());
            limits(r, "kimi").chat_max_output_tokens = Some(777);
            limits(r, "openai_compat").chat_max_output_tokens = Some(555);
        });
        let wire = |base_url: Option<&str>| {
            let target = Target { base_url, model: Some("m"), deeper: None };
            serde_json::to_value(target.request("m".into(), Vec::new(), Vec::new(), false)).unwrap()
        };
        let official = wire(None);
        assert_eq!((official["max_completion_tokens"].as_u64(), official["reasoning_effort"].as_str()), (Some(1234), Some("low")));
        let kimi = wire(Some("https://api.moonshot.ai/v1"));
        assert_eq!((kimi["max_tokens"].as_u64(), kimi.get("reasoning_effort")), (Some(777), None));
        assert_eq!(wire(Some("https://api.deepseek.com/v1"))["max_tokens"].as_u64(), Some(555));
        let target = Target { base_url: None, model: None, deeper: None };
        assert_eq!(target.model().unwrap(), crate::ai::models::OPENAI_DEFAULT);
    }

    /// "Think harder" maps to a high reasoning effort on api.openai.com, for
    /// a model that takes one. Written and tested, not offered yet: the chat
    /// wire refuses tools with reasoning on (capability::deeper).
    #[test]
    fn think_harder_asks_for_a_high_reasoning_effort_on_openai_only() {
        use crate::ai::capability::Deeper;
        let effort = |base_url: Option<&'static str>, deeper| {
            let target = Target { base_url, model: Some("m"), deeper };
            serde_json::to_value(target.request("m".into(), Vec::new(), Vec::new(), false)).unwrap().get("reasoning_effort").cloned()
        };
        let high = Some(Deeper { thinking: true, effort: true });
        assert_eq!(effort(None, high), Some(json!("high")));
        assert_eq!(effort(None, None), crate::ai::models::limits("openai").chat_reasoning_effort.map(serde_json::Value::from));
        assert_eq!(effort(None, Some(Deeper { thinking: true, effort: false })), effort(None, None), "no effort flag, no change");
        assert_eq!(effort(Some("https://api.moonshot.ai/v1"), high), None, "compatible services get no reasoning_effort");
    }

    #[test]
    fn calls_without_an_index_are_kept_apart_by_id() {
        let mut slots: ToolSlots = HashMap::new();
        // OpenAI: the index decides, and argument pieces share a slot.
        assert_eq!(tool_slot(&slots, Some(2), None, 0), 2);
        // Gemini: two whole calls in two chunks, both at position 0.
        let first = tool_slot(&slots, None, Some("call_a"), 0);
        slots.insert(first, ("call_a".into(), "list_tasks".into(), "{}".into(), None));
        let second = tool_slot(&slots, None, Some("call_b"), 0);
        assert_ne!(first, second, "a second call must not append to the first");
        slots.insert(second, ("call_b".into(), "list_follow_ups".into(), "{}".into(), None));
        assert_eq!(tool_slot(&slots, None, Some("call_a"), 0), first, "the same id is the same call");
    }

    #[test]
    fn a_tool_call_without_an_index_still_parses() {
        // Gemini's OpenAI-compatible stream, 2026-09-26: no `index` on the
        // call, a thought signature to send back, finish_reason "stop".
        let data = r#"{"choices":[{"delta":{"role":"assistant","tool_calls":[{"extra_content":{"google":{"thought_signature":"sig"}},"function":{"arguments":"{}","name":"list_tasks"},"id":"call_1","type":"function"}]},"index":0}],"usage":{"completion_tokens":10,"prompt_tokens":31}}"#;
        let chunk: SseChunk = serde_json::from_str(data).unwrap();
        let call = &chunk.choices[0].delta.as_ref().unwrap().tool_calls.as_ref().unwrap()[0];
        assert_eq!((call.index, call.id.as_deref()), (None, Some("call_1")));
        assert_eq!(call.extra_content.as_ref().unwrap()["google"]["thought_signature"], "sig");
        let echoed = serde_json::to_value(ToolCall { id: "call_1".into(), call_type: "function".into(), function: FunctionCall { name: "list_tasks".into(), arguments: "{}".into() }, extra_content: call.extra_content.clone() }).unwrap();
        assert_eq!(echoed["extra_content"]["google"]["thought_signature"], "sig");
        let plain = serde_json::to_value(ToolCall { id: "c".into(), call_type: "function".into(), function: FunctionCall { name: "x".into(), arguments: "{}".into() }, extra_content: None }).unwrap();
        assert!(plain.get("extra_content").is_none(), "OpenAI itself never sees the field");
    }

    /// The last allowed round must get an answer, not another tool call
    /// that nothing will run.
    #[test]
    fn only_the_last_round_turns_tools_off() {
        let official = Target { base_url: None, model: None, deeper: None };
        let compatible = Target { base_url: Some("https://api.moonshot.ai/v1"), model: Some("kimi-k2"), deeper: None };
        for target in [official, compatible] {
            let last = serde_json::to_value(target.request("m".into(), Vec::new(), Vec::new(), true)).unwrap();
            assert_eq!(last["tool_choice"], "none");
            let earlier = serde_json::to_value(target.request("m".into(), Vec::new(), Vec::new(), false)).unwrap();
            assert!(earlier.get("tool_choice").is_none(), "{earlier}");
        }
    }

    #[test]
    fn transient_failures_are_retried_once_after_a_short_wait() {
        use std::time::Duration;
        for status in [429, 500, 502, 503, 504] {
            assert_eq!(retry_delay(status, None), Some(Duration::from_secs(1)), "{status}");
        }
        assert_eq!(retry_delay(429, Some("2")), Some(Duration::from_secs(2)));
        assert_eq!(retry_delay(503, Some("0.5")), Some(Duration::from_millis(500)));
        assert_eq!(retry_delay(429, Some("60")), None, "a long wait fails now instead of hanging the turn");
        assert_eq!(retry_delay(429, Some("Wed, 21 Oct 2026 07:28:00 GMT")), Some(Duration::from_secs(1)));
        for status in [200, 400, 401, 403, 404] {
            assert_eq!(retry_delay(status, None), None, "{status}");
        }
    }

    /// prompt_tokens includes the cached prefix; dropping
    /// prompt_tokens_details billed every cached token at 10x its price.
    #[test]
    fn cached_prompt_tokens_reach_the_turn() {
        let chunk: SseChunk = serde_json::from_str(r#"{"choices":[],"usage":{"prompt_tokens":20173,"completion_tokens":195,"prompt_tokens_details":{"cached_tokens":19824}}}"#).unwrap();
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let mut turn = crate::ai::cost::TurnUsage::new("openai", "gpt-6-luna");
        chunk.usage.unwrap().add_to(&mut turn);
        turn.record(&db, crate::ai::Workload::Tools, 1);
        let (cached, micro): (i64, i64) = db.conn.lock().unwrap().query_row("SELECT cache_read_tokens, usd_cost_micro FROM inference_metrics", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((cached, micro), (19_824, 331));
    }

    #[test]
    fn sse_chunk_parses_final_usage_chunk_with_empty_choices() {
        let json = r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":42}}"#;
        let chunk: SseChunk = serde_json::from_str(json).unwrap();
        let u = chunk.usage.unwrap();
        assert_eq!(u.prompt_tokens, Some(100));
        assert_eq!(u.completion_tokens, Some(42));
        assert!(chunk.choices.is_empty());
    }

    #[test]
    fn sse_chunk_tolerates_null_missing_usage_and_missing_choices() {
        let with_null: SseChunk = serde_json::from_str(r#"{"choices":[],"usage":null}"#).unwrap();
        assert!(with_null.usage.is_none());
        let missing_usage: SseChunk = serde_json::from_str(r#"{"choices":[]}"#).unwrap();
        assert!(missing_usage.usage.is_none());
        let missing_choices: SseChunk =
            serde_json::from_str(r#"{"usage":{"prompt_tokens":1,"completion_tokens":2}}"#).unwrap();
        assert!(missing_choices.choices.is_empty());
    }

    /// The exact chat request a compatible service (Kimi, Qwen) receives, with
    /// every JARVIS tool, sent to Gemini's OpenAI-compatible endpoint: it
    /// must accept the request and answer with a tool call.
    /// GEMINI_API_KEY=... cargo test live_compatible_service_takes_the_tool_request -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_compatible_service_takes_the_tool_request() {
        let key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY");
        let target = Target { base_url: Some(crate::ai::complete::tests::GEMINI_COMPAT), model: Some(crate::ai::models::GEMINI_FLASH), deeper: None };
        let messages = vec![
            OpenAIMessage { role: "system".into(), content: Some("You are JARVIS. Use tools to answer.".into()), tool_calls: None, tool_call_id: None },
            OpenAIMessage { role: "user".into(), content: Some("What is on my calendar today? Use the calendar tool.".into()), tool_calls: None, tool_call_id: None },
        ];
        let tools = tools::to_openai_format(&tools::get_tool_definitions());
        let request = target.request(target.model().unwrap(), messages, tools, false);
        let wire = serde_json::to_value(&request).unwrap();
        assert!(wire.get("reasoning_effort").is_none() && wire.get("max_completion_tokens").is_none() && wire["max_tokens"] == 4096);
        let response = Client::new().post(target.url()).bearer_auth(&key).json(&request).send().await.unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert!(status.is_success(), "{status}: {body}");
        let calls: Vec<String> = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
            .filter_map(|chunk| chunk.pointer("/choices/0/delta/tool_calls/0/function/name").and_then(|n| n.as_str()).map(str::to_string))
            .collect();
        println!("tool calls: {calls:?}; usage chunk: {}", body.contains("\"usage\""));
        assert!(calls.iter().any(|c| c == "list_events"), "{body}");
    }
}
