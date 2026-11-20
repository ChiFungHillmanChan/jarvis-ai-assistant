//! api.openai.com on OpenAI's Responses API (task 74): chat turns and plain
//! requests. OpenAI-compatible services and local servers stay on Chat
//! Completions (`openai.rs`).
//!
//! Every request sends `store: false` and nothing that keeps state at
//! OpenAI: no `previous_response_id`, conversation, background mode or
//! hosted tool. The app holds the conversation. Within one turn each
//! round's output items go back with every field as it came, encrypted
//! reasoning included, as OpenAI documents for stateless use; they live
//! only in the turn's memory.

use super::bedrock::UiLanguage;
use super::complete::{Completion, Request as PlainRequest};
use super::provider::{Call, ChatRequest, Event, Events, Metering, Outcome, Round, RoundError, Usage, Wire, SKIPPED};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The service's name in errors and log lines.
const NAME: &str = "OpenAI";

/// The address of the Responses API (a loopback server in tests).
fn url() -> String {
    format!("{}/responses", super::api_base("https://api.openai.com/v1"))
}

/// api.openai.com on the provider interface.
pub(crate) struct ResponsesWire {
    key: String,
    model: String,
    deeper: Option<super::capability::Deeper>,
    /// The provider id a turn's usage is stored under.
    provider: String,
}

impl ResponsesWire {
    /// `model` is the chain row's; none or blank follows the registry's default.
    pub(crate) fn new(key: &str, model: Option<&str>, deeper: Option<super::capability::Deeper>) -> Self {
        let model = model.map(str::trim).filter(|m| !m.is_empty()).map(str::to_string).unwrap_or_else(super::models::openai_default);
        ResponsesWire { key: key.to_string(), model, deeper, provider: super::cost::provider_id("openai", None) }
    }

    /// An "Other service" row at OpenAI's own address (`is_openai_itself`):
    /// its turns are stored under that row's provider id, as its plain
    /// requests are (`complete::store_then_check`) and as before task 74.
    pub(crate) fn at(mut self, base_url: &str) -> Self {
        self.provider = super::cost::provider_id("openai", Some(base_url));
        self
    }

    /// The reasoning effort of a chat request, if any. "Think harder" (P06)
    /// asks for a high one; it is not offered on this wire yet
    /// (`capability::deeper`).
    fn effort(&self) -> Option<String> {
        match self.deeper {
            Some(deeper) if deeper.effort => Some("high".into()),
            _ => super::models::openai_chat_effort(&self.model),
        }
    }

    fn chat_body<'a>(&'a self, conversation: &'a Conversation, final_round: bool) -> ChatBody<'a> {
        ChatBody {
            model: &self.model,
            input: &conversation.input,
            tools: &conversation.tools,
            tool_choice: final_round.then_some("none"),
            max_output_tokens: super::models::chat_max_output_tokens("openai"),
            reasoning: self.effort().map(|effort| Reasoning { effort }),
            store: false,
            stream: true,
        }
    }
}

/// One chat request. Nothing else is sent: see the module comment.
#[derive(Serialize)]
struct ChatBody<'a> {
    model: &'a str,
    input: &'a [Value],
    tools: &'a [Value],
    /// "none" on the last allowed round, so the model answers from what the
    /// earlier rounds gathered instead of asking for another tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    max_output_tokens: u32,
    /// Left out for a model the registry says takes no effort.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<Reasoning>,
    store: bool,
    stream: bool,
}

#[derive(Serialize)]
struct Reasoning {
    effort: String,
}

/// One request's figures. `input_tokens` includes the cache reads and
/// writes; `output_tokens` includes the reasoning tokens.
fn figures(usage: &Value) -> Usage {
    let count = |pointer: &str| usage.pointer(pointer).and_then(Value::as_i64).unwrap_or(0);
    let long_writes = count("/input_tokens_details/cache_write_tokens_details/ttl_12h_tokens");
    if long_writes > 0 {
        // The app asks for the default 30-minute cache; the registry prices
        // that write only.
        log::warn!("[openai] {long_writes} cache-write tokens at the 12-hour rate are priced at the 30-minute rate");
    }
    Usage {
        input: count("/input_tokens"),
        output: count("/output_tokens"),
        cache_read: count("/input_tokens_details/cached_tokens"),
        cache_write: count("/input_tokens_details/cache_write_tokens"),
    }
}

/// One chat turn's requests on the Responses API.
pub(crate) struct Conversation {
    client: Client,
    /// Everything sent so far, oldest first: the developer message, the
    /// history, then each tool round's output items and results.
    input: Vec<Value>,
    tools: Vec<Value>,
    /// The output items of the round in progress, as OpenAI sent them, kept
    /// for the next request when the round's calls run.
    reply: Vec<Value>,
}

/// A function call as it streams.
#[derive(Default)]
struct Slot {
    call_id: String,
    name: String,
    arguments: String,
}

impl Slot {
    /// What a `function_call` item says, field by field: an item that
    /// leaves a field out keeps what streamed.
    fn take(&mut self, item: &Value) {
        let field = |name: &str| item[name].as_str().map(str::to_string);
        if let Some(call_id) = field("call_id") {
            self.call_id = call_id;
        }
        if let Some(name) = field("name") {
            self.name = name;
        }
        if let Some(arguments) = field("arguments") {
            self.arguments = arguments;
        }
    }
}

/// What one streamed reply has said so far, held on the heap by `round` so
/// the chat future stays small (clippy::large_futures).
#[derive(Default)]
struct Reply {
    /// Function calls by `output_index`.
    calls: BTreeMap<u64, Slot>,
    /// Each finished output item by `output_index`, as OpenAI sent it.
    items: BTreeMap<u64, Value>,
    usage: Usage,
    /// The stop word, once the terminal event came.
    stop: Option<String>,
}

impl Reply {
    /// Read one event. Ok(true) once the reply has ended.
    fn read(&mut self, data: &str, events: &mut Events<'_>) -> Result<bool, RoundError> {
        let event: Value = match serde_json::from_str(data) {
            Ok(event) => event,
            Err(e) => {
                log::warn!("{}", super::failure::unparsed_event_line(NAME, &e, data.len()));
                return Ok(false);
            }
        };
        let index = event["output_index"].as_u64().unwrap_or_default();
        let text = |field: &str| event[field].as_str().unwrap_or_default().to_string();
        match event["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" => events.send(Event::Text(text("delta"))),
            "response.output_item.added" if event["item"]["type"] == "function_call" => {
                let slot = self.calls.entry(index).or_default();
                slot.take(&event["item"]);
                events.send(Event::ToolStarted { key: index.to_string(), name: slot.name.clone() });
            }
            "response.function_call_arguments.delta" => {
                if let Some(slot) = self.calls.get_mut(&index) {
                    slot.arguments.push_str(&text("delta"));
                }
            }
            "response.function_call_arguments.done" => {
                if let (Some(slot), Some(arguments)) = (self.calls.get_mut(&index), event["arguments"].as_str()) {
                    slot.arguments = arguments.to_string();
                }
            }
            "response.output_item.done" => {
                let item = event["item"].clone();
                if item["type"] == "function_call" {
                    let announced = self.calls.contains_key(&index);
                    let slot = self.calls.entry(index).or_default();
                    slot.take(&item);
                    if !announced {
                        events.send(Event::ToolStarted { key: index.to_string(), name: slot.name.clone() });
                    }
                }
                self.items.insert(index, item);
            }
            kind @ ("response.completed" | "response.incomplete") => {
                let response = &event["response"];
                self.usage = figures(&response["usage"]);
                self.stop = Some(match (kind, response.pointer("/incomplete_details/reason").and_then(Value::as_str)) {
                    ("response.incomplete", Some("max_output_tokens")) => "length".into(),
                    ("response.incomplete", Some(reason)) => reason.into(),
                    ("response.incomplete", None) => "incomplete".into(),
                    _ if !self.calls.is_empty() => "tool_calls".into(),
                    _ => "stop".into(),
                });
                return Ok(true);
            }
            "response.failed" | "error" => {
                self.usage = figures(&event["response"]["usage"]);
                // Only the error's own short code goes to the log and into
                // the error: never its message, which can quote the request,
                // nor the partial reply a failed response carries in
                // `output`. The router reads the error's words to class it
                // (`failure::code_of`), so what the model was writing must
                // not be among them.
                let code = event.pointer("/response/error/code").or_else(|| event.get("code")).cloned().unwrap_or(Value::Null);
                let error = json!({"error": {"code": code}}).to_string();
                log::error!("{}", super::failure::stream_error_line(NAME, &error));
                return Err(RoundError::broken(format!("{NAME} stream error: {error}")));
            }
            // Refusal text is not shown, as on Chat Completions: a reply
            // that only refuses is an empty reply. Reasoning summaries are
            // not asked for.
            _ => {}
        }
        Ok(false)
    }

    /// The round's output items in order, for the next request. A call
    /// whose finished item never came goes back as it streamed.
    fn items_to_replay(&mut self) -> Vec<Value> {
        let mut items = std::mem::take(&mut self.items);
        for (index, slot) in &self.calls {
            items.entry(*index).or_insert_with(|| json!({"type": "function_call", "call_id": slot.call_id, "name": slot.name, "arguments": slot.arguments}));
        }
        items.into_values().collect()
    }
}

#[async_trait::async_trait]
impl Wire for ResponsesWire {
    type Conversation = Conversation;

    fn max_rounds(&self) -> usize {
        super::models::max_tool_rounds("openai")
    }

    fn metering(&self) -> Metering {
        Metering::Turn { provider: self.provider.clone(), model: self.model.clone() }
    }

    /// A 429 or 5xx is sent once more here, as on Chat Completions.
    fn resends_transient(&self) -> bool {
        true
    }

    async fn start(&self, request: ChatRequest) -> Result<Conversation, String> {
        let client = Client::builder()
            .read_timeout(std::time::Duration::from_secs(60))
            // A redirect would carry the key and the conversation elsewhere.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        // The standing rules as the first developer message: models from
        // GPT-6.2 put top-level `instructions` after the input, uncached.
        let mut input = vec![json!({"role": "developer", "content": request.system.joined()})];
        input.extend(request.messages.into_iter().filter(|(_, content)| !content.trim().is_empty()).map(|(role, content)| json!({"role": role, "content": content})));
        Ok(Conversation { client, input, tools: super::tools::to_responses_format(&request.tools), reply: Vec::new() })
    }

    async fn round(&self, conversation: &mut Conversation, final_round: bool, events: &mut Events<'_>) -> Result<(), RoundError> {
        let body = serde_json::to_vec(&self.chat_body(conversation, final_round)).map_err(|e| RoundError::refused(e.to_string()))?;
        let url = url();
        let client = &conversation.client;
        let build = || client.post(&url).header("Content-Type", "application/json").header("Authorization", format!("Bearer {}", self.key)).body(body.clone());
        let response = super::openai::send_retrying(build).await.map_err(|e| RoundError::refused(e.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RoundError::refused(format!("{NAME} API error {status}: {body}")));
        }

        let mut reply = Box::<Reply>::default();
        let mut stream = response.bytes_stream();
        let mut buffer = super::SseByteBuffer::new();
        let mut ended = false;
        while !ended {
            let Some(chunk) = stream.next().await else { break };
            let chunk = match chunk {
                Ok(bytes) => bytes,
                Err(e) => {
                    events.send(Event::Usage(reply.usage));
                    return Err(RoundError::broken(format!("Stream error: {}", e)));
                }
            };
            buffer.extend(&chunk);
            while let Some(block) = buffer.next_event() {
                let data = block.lines().filter(|line| !line.starts_with(':')).filter_map(|line| line.strip_prefix("data: ")).next_back().unwrap_or_default();
                // OpenAI ends a Responses stream with its last event, not
                // `[DONE]`; one is skipped all the same.
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                match reply.read(data, events) {
                    Ok(true) => {
                        ended = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        events.send(Event::Usage(reply.usage));
                        return Err(error);
                    }
                }
            }
        }
        events.send(Event::Usage(reply.usage));
        if !ended {
            // Every Responses stream ends with a terminal event; one without
            // it broke off.
            return Err(RoundError::broken("Stream error: the reply ended before it finished"));
        }
        for slot in reply.calls.values() {
            events.send(Event::ToolCall(Call { id: slot.call_id.clone(), name: slot.name.clone(), arguments: slot.arguments.clone() }));
        }
        conversation.reply = if reply.calls.is_empty() { Vec::new() } else { reply.items_to_replay() };
        events.send(Event::Done { stop: reply.stop.take() });
        Ok(())
    }

    /// A reply cut off by the length limit may carry half-written arguments.
    fn stopped_at_limit(&self, round: &Round) -> bool {
        round.stop.as_deref() == Some("length")
    }

    /// The round's output items as they came, then each call's result by
    /// its `call_id`. Nothing sent earlier is changed: on GPT-6.2 an edit
    /// before a reasoning item drops that reasoning.
    fn add_results(&self, conversation: &mut Conversation, _round: Round, results: Vec<(Call, Outcome)>, _next_is_final: bool) -> Result<(), String> {
        conversation.input.append(&mut conversation.reply);
        for (call, outcome) in results {
            let output = match outcome {
                Outcome::Ran(result) => result,
                Outcome::Duplicate => SKIPPED.to_string(),
            };
            conversation.input.push(json!({"type": "function_call_output", "call_id": call.id, "output": output}));
        }
        Ok(())
    }

    async fn complete(&self, request: &PlainRequest<'_>, _language: UiLanguage) -> Result<Completion, String> {
        let client = super::compat::client()?;
        let value = super::complete::post(&client, &url(), NAME, |r| r.bearer_auth(&self.key), &plain_body(&self.model, request)).await?;
        plain_result(&value, NAME.into(), self.model.clone())
    }
}

/// A plain request: not streamed, no tools, and, as on Chat Completions,
/// no reasoning effort (the model's default).
pub(crate) fn plain_body(model: &str, request: &PlainRequest<'_>) -> Value {
    let mut input = Vec::new();
    if !request.system.is_empty() {
        input.push(json!({"role": "developer", "content": request.system}));
    }
    input.push(json!({"role": "user", "content": request.user}));
    let mut body = json!({"model": model, "input": input, "max_output_tokens": request.max_tokens, "store": false});
    if request.json {
        body["text"] = json!({"format": {"type": "json_object"}});
    }
    body
}

/// A plain reply's text, from its messages in order, and its usage. A
/// refusal is not text, as on Chat Completions; a reply cut off by the
/// output limit keeps what it said.
pub(crate) fn plain_result(value: &Value, provider: String, model: String) -> Result<Completion, String> {
    let list = |value: &Value| value.as_array().cloned().unwrap_or_default();
    let text: String = list(&value["output"])
        .iter()
        .filter(|item| item["type"] == "message")
        .flat_map(|item| list(&item["content"]))
        .filter(|part| part["type"] == "output_text")
        .filter_map(|part| part["text"].as_str().map(str::to_string))
        .collect();
    let usage = figures(&value["usage"]);
    let mut done = super::complete::done(text, provider, model, value.pointer("/usage/input_tokens"), value.pointer("/usage/output_tokens"))?;
    done.cached_tokens = usage.cache_read;
    done.cache_write_tokens = usage.cache_write;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::capability::Deeper;
    use crate::ai::cost::Feature;

    fn conversation(input: Vec<Value>) -> Conversation {
        Conversation { client: Client::new(), input, tools: vec![json!({"type": "function", "name": "t"})], reply: Vec::new() }
    }

    fn body(wire: &ResponsesWire, final_round: bool) -> Value {
        let conversation = conversation(vec![json!({"role": "user", "content": "hi"})]);
        serde_json::to_value(wire.chat_body(&conversation, final_round)).unwrap()
    }

    /// The whole chat body: nothing that keeps state at OpenAI, and the
    /// registry's cap and effort.
    #[test]
    fn a_chat_request_keeps_no_state_at_openai() {
        let wire = ResponsesWire::new("k", Some("gpt-6-luna"), None);
        assert_eq!(
            body(&wire, false),
            json!({
                "model": "gpt-6-luna",
                "input": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function", "name": "t"}],
                "max_output_tokens": crate::ai::models::chat_max_output_tokens("openai"),
                "reasoning": {"effort": crate::ai::models::limits("openai").chat_reasoning_effort.unwrap()},
                "store": false,
                "stream": true
            })
        );
    }

    /// The last allowed round must get an answer, not another tool call
    /// that nothing will run.
    #[test]
    fn only_the_last_round_turns_tools_off() {
        let wire = ResponsesWire::new("k", None, None);
        assert_eq!(body(&wire, true)["tool_choice"], "none");
        assert!(body(&wire, false).get("tool_choice").is_none());
    }

    /// The cap and the effort are the registry's; a model the registry says
    /// takes no effort is sent none; a blank model is the registry default.
    #[test]
    fn request_limits_follow_the_registry() {
        let _registry = crate::ai::registry::test_override::with(|r| {
            let limits = &mut r.providers.iter_mut().find(|p| p.id == "openai").unwrap().limits;
            limits.chat_max_output_tokens = Some(1234);
            limits.chat_reasoning_effort = Some("low".into());
            r.models.iter_mut().find(|m| m.provider == "openai" && m.id == "gpt-4o").unwrap().capabilities.reasoning_effort = false;
        });
        let sent = body(&ResponsesWire::new("k", Some("gpt-6-luna"), None), false);
        assert_eq!((sent["max_output_tokens"].as_u64(), sent["reasoning"]["effort"].as_str()), (Some(1234), Some("low")));
        assert!(body(&ResponsesWire::new("k", Some("gpt-4o"), None), false).get("reasoning").is_none(), "no effort for a model that takes none");
        assert_eq!(body(&ResponsesWire::new("k", Some("gpt-4o-2024-08-06"), None), false).get("reasoning"), None, "a dated id is its entry's");
        assert_eq!(body(&ResponsesWire::new("k", Some("a-model-the-registry-lacks"), None), false)["reasoning"]["effort"], "low", "an unlisted id gets the provider's");
        assert_eq!(ResponsesWire::new("k", Some("  "), None).model, crate::ai::models::OPENAI_DEFAULT);
    }

    /// "Think harder" asks for a high effort, for a model that takes one.
    /// Written and kept, not offered on this wire yet (capability::deeper).
    #[test]
    fn think_harder_asks_for_a_high_reasoning_effort() {
        let effort = |deeper| body(&ResponsesWire::new("k", Some("gpt-6-luna"), deeper), false).get("reasoning").cloned();
        assert_eq!(effort(Some(Deeper { thinking: true, effort: true })), Some(json!({"effort": "high"})));
        assert_eq!(effort(Some(Deeper { thinking: true, effort: false })), effort(None), "no effort flag, no change");
    }

    /// The tools in Responses' flat form, non-strict as on Chat Completions.
    #[test]
    fn tools_go_as_flat_non_strict_functions() {
        let tools = crate::ai::tools::get_tool_definitions();
        let sent = crate::ai::tools::to_responses_format(&tools);
        assert_eq!(sent.len(), tools.len());
        assert_eq!(sent[0], json!({"type": "function", "name": tools[0].name, "description": tools[0].description, "parameters": tools[0].parameters, "strict": false}));
    }

    /// Cache reads and writes are inside `input_tokens`; reasoning is inside
    /// `output_tokens`. Both cache figures reach the row and its price.
    #[test]
    fn cache_reads_and_writes_reach_the_turn() {
        let usage = json!({
            "input_tokens": 12000,
            "input_tokens_details": {"cached_tokens": 4000, "cache_write_tokens": 7000, "cache_write_tokens_details": {"ttl_30m_tokens": 7000, "ttl_12h_tokens": 0}},
            "output_tokens": 1186,
            "output_tokens_details": {"reasoning_tokens": 1024},
            "total_tokens": 13186
        });
        assert_eq!(figures(&usage), Usage { input: 12000, output: 1186, cache_read: 4000, cache_write: 7000 });
        assert_eq!(figures(&Value::Null), Usage::default(), "no usage reads as zeros");
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let mut turn = crate::ai::cost::TurnUsage::new("openai", "gpt-6-luna");
        let f = figures(&usage);
        turn.add_request(f.input, f.output, f.cache_read, f.cache_write);
        turn.record(&db, crate::ai::Workload::Tools, 1);
        let (read, write, micro): (i64, i64, i64) = db.conn.lock().unwrap().query_row("SELECT cache_read_tokens, cache_write_tokens, usd_cost_micro FROM inference_metrics", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        let expected = crate::ai::models::usd_cost_micro("openai", "gpt-6-luna", 12000, 1186, 4000, 7000).unwrap();
        assert_eq!((read, write, micro), (4000, 7000, expected));
        let without_writes = crate::ai::models::usd_cost_micro("openai", "gpt-6-luna", 12000, 1186, 4000, 0).unwrap();
        assert!(expected > without_writes, "a write costs more than ordinary input");
    }

    /// The plain body: the system text as a developer message only when
    /// there is one, JSON asked for by `text.format`, and no effort.
    #[test]
    fn plain_bodies() {
        let request = PlainRequest { system: "rules", user: "hi", json: true, max_tokens: 100, feature: Feature::Other };
        assert_eq!(
            plain_body("gpt-6-luna", &request),
            json!({
                "model": "gpt-6-luna",
                "input": [{"role": "developer", "content": "rules"}, {"role": "user", "content": "hi"}],
                "max_output_tokens": 100,
                "store": false,
                "text": {"format": {"type": "json_object"}}
            })
        );
        let plain = plain_body("gpt-6-luna", &PlainRequest { system: "", json: false, ..request });
        assert_eq!(plain, json!({"model": "gpt-6-luna", "input": [{"role": "user", "content": "hi"}], "max_output_tokens": 100, "store": false}));
    }

    /// A plain reply's text comes from its messages, after any reasoning;
    /// a refusal is no text; usage carries both cache figures.
    #[test]
    fn plain_replies() {
        let reply = json!({
            "status": "completed",
            "output": [
                {"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "opaque"},
                {"type": "message", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": "O", "annotations": []}, {"type": "output_text", "text": "K"}]}
            ],
            "usage": {"input_tokens": 10, "output_tokens": 2, "input_tokens_details": {"cached_tokens": 4, "cache_write_tokens": 3}}
        });
        let done = plain_result(&reply, "OpenAI".into(), "m".into()).unwrap();
        assert_eq!((done.text.as_str(), done.input_tokens, done.output_tokens, done.cached_tokens, done.cache_write_tokens), ("OK", 10, 2, 4, 3));
        let refused = json!({"output": [{"type": "message", "role": "assistant", "content": [{"type": "refusal", "refusal": "I can't help with that."}]}]});
        assert_eq!(plain_result(&refused, "OpenAI".into(), "m".into()).unwrap_err(), "OpenAI 沒有回覆內容。");
        let cut = json!({"status": "incomplete", "incomplete_details": {"reason": "max_output_tokens"}, "output": [{"type": "message", "content": [{"type": "output_text", "text": "Partial"}]}]});
        assert_eq!(plain_result(&cut, "OpenAI".into(), "m".into()).unwrap().text, "Partial");
    }
}
