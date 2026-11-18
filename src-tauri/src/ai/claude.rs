use super::tools;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use super::provider::{Call, ChatRequest, Event, Events, Metering, Outcome, Round, RoundError, Wire, SKIPPED};
use super::capability::Deeper;

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    system: String,
    tools: Vec<serde_json::Value>,
    /// {"type":"none"} on the last allowed round, so the model answers from
    /// what the earlier rounds gathered instead of asking for another tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
    messages: Vec<ClaudeMessage>,
    stream: bool,
    /// "Think harder" (P06): adaptive thinking. Not a `budget_tokens`
    /// budget: the current Claude models refuse one with a 400.
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
    /// "Think harder" (P06): a higher effort.
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<serde_json::Value>,
}

/// One chat turn's requests. The system text, the tools and every earlier
/// message go out byte for byte the same in each tool round, and a round only
/// appends its reply and its tool results: Anthropic refuses a later round
/// whose earlier parts differ from the round that produced its thinking
/// blocks (accounts created on or after 2026-08-31).
struct TurnRequests {
    model: String,
    max_tokens: u32,
    system: String,
    tools: Vec<serde_json::Value>,
    messages: Vec<ClaudeMessage>,
    thinking: Option<serde_json::Value>,
    output_config: Option<serde_json::Value>,
}

impl TurnRequests {
    fn new(model: String, max_tokens: u32, system: String, tools: Vec<serde_json::Value>, messages: Vec<(String, String)>, deeper: Option<Deeper>) -> Self {
        let (thinking, output_config) = think_fields(deeper);
        let messages = messages
            .into_iter()
            .filter(|(_, content)| !content.trim().is_empty())
            .map(|(role, content)| ClaudeMessage { role, content: ClaudeContent::Text(content) })
            .collect();
        TurnRequests { model, max_tokens, system, tools, messages, thinking, output_config }
    }

    fn request(&self, final_round: bool) -> ClaudeRequest {
        ClaudeRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: self.system.clone(),
            tools: self.tools.clone(),
            tool_choice: final_round.then(|| json!({"type": "none"})),
            messages: self.messages.clone(),
            stream: true,
            thinking: self.thinking.clone(),
            output_config: self.output_config.clone(),
        }
    }

    /// The assistant's reply to a round that called tools. A Think harder
    /// turn sends it back as it streamed, thinking blocks included, which
    /// Anthropic requires with tools; any other turn sends its text and tool
    /// calls only, as main does (P06 fix round 4, review A I-3 not changed).
    fn add_reply(&mut self, replay: Replay, text_parts: &[String], tool_uses: &[(String, String, serde_json::Value)]) {
        let blocks: Vec<ContentBlock> = if self.thinking.is_some() || self.output_config.is_some() {
            replay.into_blocks()
        } else {
            let texts = text_parts.iter().map(|text| ContentBlock::Text { text: text.clone() });
            let calls = tool_uses.iter().map(|(id, name, input)| ContentBlock::ToolUse { id: id.clone(), name: name.clone(), input: input.clone() });
            texts.chain(calls).collect()
        };
        self.messages.push(ClaudeMessage { role: "assistant".into(), content: ClaudeContent::Blocks(blocks) });
    }

    /// The results of the round's tool calls.
    fn add_results(&mut self, results: Vec<ContentBlock>) {
        self.messages.push(ClaudeMessage { role: "user".into(), content: ClaudeContent::Blocks(results) });
    }
}

/// What a "Think harder" turn adds to the request, from what the registry
/// says the model takes ([`Deeper`]). Nothing for any other turn.
fn think_fields(deeper: Option<Deeper>) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    let Some(deeper) = deeper else { return (None, None) };
    (deeper.thinking.then(|| json!({"type": "adaptive"})), deeper.effort.then(|| json!({"effort": "high"})))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ClaudeMessage {
    role: String,
    content: ClaudeContent,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
enum ClaudeContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    /// Sent back unchanged, signature included, when thinking was asked for.
    #[serde(rename = "thinking")]
    Thinking { thinking: String, signature: String },
    #[serde(rename = "redacted_thinking")]
    RedactedThinking { data: String },
}

/// One streamed reply's blocks in the order they came, for the next round
/// of a turn that asked for thinking: the thinking blocks go back unchanged
/// (their signatures too), before the tool calls they led to.
#[derive(Default)]
struct Replay {
    blocks: BTreeMap<usize, ContentBlock>,
    tool_inputs: BTreeMap<usize, String>,
}

impl Replay {
    fn start(&mut self, index: usize, block: &SseContentBlock) {
        let started = match block.block_type.as_str() {
            "text" => ContentBlock::Text { text: block.text.clone().unwrap_or_default() },
            "thinking" => ContentBlock::Thinking { thinking: block.thinking.clone().unwrap_or_default(), signature: block.signature.clone().unwrap_or_default() },
            "redacted_thinking" => ContentBlock::RedactedThinking { data: block.data.clone().unwrap_or_default() },
            "tool_use" => ContentBlock::ToolUse { id: block.id.clone().unwrap_or_default(), name: block.name.clone().unwrap_or_default(), input: json!({}) },
            _ => return,
        };
        self.blocks.insert(index, started);
    }

    fn delta(&mut self, index: usize, delta: &SseDelta) {
        match (self.blocks.get_mut(&index), delta.delta_type.as_deref()) {
            (Some(ContentBlock::Text { text }), Some("text_delta")) => text.push_str(delta.text.as_deref().unwrap_or_default()),
            (Some(ContentBlock::Thinking { thinking, .. }), Some("thinking_delta")) => thinking.push_str(delta.thinking.as_deref().unwrap_or_default()),
            (Some(ContentBlock::Thinking { signature, .. }), Some("signature_delta")) => signature.push_str(delta.signature.as_deref().unwrap_or_default()),
            (Some(ContentBlock::ToolUse { .. }), Some("input_json_delta")) => self.tool_inputs.entry(index).or_default().push_str(delta.partial_json.as_deref().unwrap_or_default()),
            _ => {}
        }
    }

    /// The assistant turn to send back, every block in its place. An empty
    /// text block is left out: the API refuses one.
    fn into_blocks(self) -> Vec<ContentBlock> {
        let Replay { blocks, tool_inputs } = self;
        blocks
            .into_iter()
            .filter(|(_, block)| !matches!(block, ContentBlock::Text { text } if text.is_empty()))
            .map(|(index, block)| match block {
                ContentBlock::ToolUse { id, name, .. } => {
                    let input = tool_inputs.get(&index).and_then(|raw| serde_json::from_str(raw).ok()).unwrap_or_else(|| json!({}));
                    ContentBlock::ToolUse { id, name, input }
                }
                other => other,
            })
            .collect()
    }
}

// ClaudeResponse and ResponseBlock are no longer needed (SSE replaces JSON parsing)

#[derive(Deserialize)]
struct SseEvent {
    #[serde(rename = "type")]
    event_type: String,
    index: Option<usize>,
    delta: Option<SseDelta>,
    content_block: Option<SseContentBlock>,
    message: Option<SseMessageStart>,
    usage: Option<SseUsage>,
}

#[derive(Deserialize)]
struct SseDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
    thinking: Option<String>,
    signature: Option<String>,
    partial_json: Option<String>,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct SseContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    id: Option<String>,
    name: Option<String>,
    text: Option<String>,
    thinking: Option<String>,
    signature: Option<String>,
    data: Option<String>,
}

#[derive(Deserialize)]
struct SseMessageStart {
    usage: Option<SseUsage>,
}

#[derive(Deserialize)]
struct SseUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
}

/// One request's usage. Anthropic's input_tokens leaves out the cached
/// tokens, so they are added back: the cost table counts input inclusive.
#[derive(Default)]
struct RoundUsage {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
}

impl RoundUsage {
    fn figures(&self) -> super::provider::Usage {
        super::provider::Usage { input: self.input + self.cache_read + self.cache_write, output: self.output, cache_read: self.cache_read, cache_write: self.cache_write }
    }

    /// Fold this request into a turn, as the usage path does.
    #[cfg(test)]
    fn add_to(&self, turn: &mut crate::ai::cost::TurnUsage) {
        let figures = self.figures();
        turn.add_request(figures.input, figures.output, figures.cache_read, figures.cache_write);
    }
}


/// Anthropic's Messages API on the provider interface.
pub(crate) struct ClaudeWire {
    key: String,
    model: String,
}

impl ClaudeWire {
    pub(crate) fn new(key: &str, model: &str) -> Self {
        ClaudeWire { key: key.to_string(), model: model.to_string() }
    }
}

/// One chat turn's requests to Claude.
pub(crate) struct Conversation {
    client: Client,
    api_version: String,
    requests: TurnRequests,
    /// The round in progress, as it streamed: sent back with the results of
    /// its calls.
    reply: Option<(Replay, Vec<String>, Vec<(String, String, serde_json::Value)>)>,
}

#[async_trait::async_trait]
impl Wire for ClaudeWire {
    type Conversation = Conversation;

    fn max_rounds(&self) -> usize {
        crate::ai::models::limits("claude").max_tool_rounds.unwrap_or_default() as usize
    }

    fn metering(&self) -> Metering {
        Metering::PerTurn { provider: "claude".into(), model: self.model.clone() }
    }

    async fn start(&self, request: ChatRequest) -> Result<Conversation, String> {
        let client = Client::builder().read_timeout(std::time::Duration::from_secs(60)).build().map_err(|e| e.to_string())?;
        let limits = crate::ai::models::limits("claude");
        let requests = TurnRequests::new(
            self.model.clone(),
            limits.chat_max_output_tokens.unwrap_or_default(),
            request.system.joined(),
            tools::to_claude_format(&request.tools),
            request.messages,
            request.deeper,
        );
        Ok(Conversation { client, api_version: limits.api_version.unwrap_or_default(), requests, reply: None })
    }

    async fn round(&self, conversation: &mut Conversation, final_round: bool, events: &mut Events<'_>) -> Result<(), RoundError> {
        let request = conversation.requests.request(final_round);
        let response = conversation
            .client
            .post(format!("{}/v1/messages", super::api_base("https://api.anthropic.com")))
            .header("x-api-key", &self.key)
            .header("anthropic-version", &conversation.api_version)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| RoundError::refused(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RoundError::refused(format!("Claude API error {}: {}", status, body)));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = super::SseByteBuffer::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_map: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
        let mut stop_reason: Option<String> = None;
        let mut round_usage = RoundUsage::default();
        let mut replay = Replay::default();

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => bytes,
                Err(e) => {
                    events.send(Event::Usage(round_usage.figures()));
                    return Err(RoundError::broken(format!("Stream error: {}", e)));
                }
            };
            buffer.extend(&chunk);

            // Complete SSE events (separated by a blank line).
            while let Some(event_block) = buffer.next_event() {
                let mut event_type = String::new();
                let mut data_str = String::new();
                for line in event_block.lines() {
                    if line.starts_with(':') {
                        continue;
                    }
                    if let Some(rest) = line.strip_prefix("event: ") {
                        event_type = rest.to_string();
                    } else if let Some(rest) = line.strip_prefix("data: ") {
                        data_str = rest.to_string();
                    }
                }
                if data_str.is_empty() || data_str == "[DONE]" {
                    continue;
                }
                if event_type == "error" {
                    log::error!("{}", super::failure::stream_error_line("Claude", &data_str));
                    events.send(Event::Usage(round_usage.figures()));
                    return Err(RoundError::broken(format!("Claude stream error: {}", data_str)));
                }
                let sse: SseEvent = match serde_json::from_str(&data_str) {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!("{}", super::failure::unparsed_event_line("Claude", &e, data_str.len()));
                        continue;
                    }
                };
                log::debug!("[claude] SSE: {}", sse.event_type);
                match sse.event_type.as_str() {
                    "message_start" => {
                        if let Some(u) = sse.message.as_ref().and_then(|m| m.usage.as_ref()) {
                            round_usage.input = u.input_tokens.unwrap_or(0);
                            round_usage.cache_read = u.cache_read_input_tokens.unwrap_or(0);
                            round_usage.cache_write = u.cache_creation_input_tokens.unwrap_or(0);
                        }
                    }
                    "content_block_start" => {
                        if let Some(ref cb) = sse.content_block {
                            if let Some(idx) = sse.index {
                                replay.start(idx, cb);
                            }
                            if cb.block_type == "tool_use" {
                                if let (Some(ref id), Some(ref name), Some(idx)) = (&cb.id, &cb.name, sse.index) {
                                    tool_map.insert(idx, (id.clone(), name.clone(), String::new()));
                                    events.send(Event::ToolStarted { key: idx.to_string(), name: name.clone() });
                                }
                            }
                        }
                    }
                    "content_block_delta" => {
                        if let Some(ref delta) = sse.delta {
                            if let Some(idx) = sse.index {
                                replay.delta(idx, delta);
                            }
                            match delta.delta_type.as_deref() {
                                Some("text_delta") => {
                                    if let Some(ref text) = delta.text {
                                        text_parts.push(text.clone());
                                        events.send(Event::Text(text.clone()));
                                    }
                                }
                                Some("thinking_delta") => {
                                    if let Some(ref text) = delta.thinking {
                                        events.send(Event::Thinking(text.clone()));
                                    }
                                }
                                Some("input_json_delta") => {
                                    if let (Some(ref pj), Some(idx)) = (&delta.partial_json, sse.index) {
                                        if let Some(entry) = tool_map.get_mut(&idx) {
                                            entry.2.push_str(pj);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    "message_delta" => {
                        if let Some(ref sr) = sse.delta.as_ref().and_then(|d| d.stop_reason.clone()) {
                            stop_reason = Some(sr.clone());
                        }
                        // Cumulative for the message, so it replaces.
                        if let Some(ref u) = sse.usage {
                            round_usage.output = u.output_tokens.unwrap_or(0);
                        }
                    }
                    _ => {}
                }
            }
        }
        events.send(Event::Usage(round_usage.figures()));

        // In the order the calls came.
        let tool_uses: Vec<(String, String, serde_json::Value)> =
            tool_map.into_values().map(|(id, name, json_str)| (id, name, serde_json::from_str(&json_str).unwrap_or_default())).collect();
        for (id, name, input) in &tool_uses {
            events.send(Event::ToolCall(Call { id: id.clone(), name: name.clone(), arguments: serde_json::to_string(input).unwrap_or_default() }));
        }
        conversation.reply = Some((replay, text_parts, tool_uses));
        events.send(Event::Done { stop: stop_reason });
        Ok(())
    }

    fn add_results(&self, conversation: &mut Conversation, _round: Round, results: Vec<(Call, Outcome)>, _next_is_final: bool) -> Result<(), String> {
        let (replay, text_parts, tool_uses) = conversation.reply.take().unwrap_or_default();
        conversation.requests.add_reply(replay, &text_parts, &tool_uses);
        let blocks = results
            .into_iter()
            .map(|(call, outcome)| ContentBlock::ToolResult {
                tool_use_id: call.id,
                content: match outcome {
                    Outcome::Ran(result) => result,
                    Outcome::Duplicate => SKIPPED.to_string(),
                },
            })
            .collect();
        conversation.requests.add_results(blocks);
        Ok(())
    }

    /// The reply said it was done as well as calling tools: its text is the
    /// answer once they ran.
    fn answers_after_tools(&self, round: &Round) -> bool {
        round.stop.as_deref() == Some("end_turn") && !round.texts.is_empty()
    }

    fn reply(&self, _conversation: &Conversation, round: Round, final_round: bool, language: &str) -> Result<String, String> {
        if !round.texts.is_empty() {
            return Ok(round.text());
        }
        if final_round {
            return Ok(tools::tool_limit_fallback(language).into());
        }
        Ok(String::new())
    }

    fn shows_thinking(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sonnet 4.6, a cache hit: 2,400 uncached in, 6,400 read from cache,
    /// 250 out = $0.0072 + $0.00192 + $0.00375. Passing input_tokens as the
    /// total let the cost table subtract the cache from it a second time.
    #[test]
    fn cached_input_is_not_subtracted_twice() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let mut turn = crate::ai::cost::TurnUsage::new("claude", "claude-sonnet-4-6");
        RoundUsage { input: 2_400, output: 250, cache_read: 6_400, cache_write: 0 }.add_to(&mut turn);
        turn.record(&db, crate::ai::Workload::Tools, 1);
        let micro: i64 = db.conn.lock().unwrap().query_row("SELECT usd_cost_micro FROM inference_metrics", [], |r| r.get(0)).unwrap();
        assert_eq!(micro, 12_870);
    }

    /// The last allowed round must get an answer, not another tool call
    /// that nothing will run.
    #[test]
    fn only_the_last_round_turns_tools_off() {
        let request = |tool_choice| {
            serde_json::to_value(ClaudeRequest { model: "m".into(), max_tokens: 1, system: String::new(), tools: Vec::new(), tool_choice, messages: Vec::new(), stream: true, thinking: None, output_config: None }).unwrap()
        };
        assert_eq!(request(Some(json!({"type": "none"})))["tool_choice"]["type"], "none");
        assert!(request(None).get("tool_choice").is_none());
    }

    /// "Think harder" maps to adaptive thinking and a higher effort, as far
    /// as the registry says the model takes them; never a budget_tokens
    /// budget (refused by the current models). Other turns send neither.
    #[test]
    fn think_harder_asks_for_adaptive_thinking_and_a_higher_effort() {
        let body = |deeper| {
            let (thinking, output_config) = think_fields(deeper);
            serde_json::to_value(ClaudeRequest { model: "m".into(), max_tokens: 1, system: String::new(), tools: Vec::new(), tool_choice: None, messages: Vec::new(), stream: true, thinking, output_config }).unwrap()
        };
        let both = body(Some(Deeper { thinking: true, effort: true }));
        assert_eq!(both["thinking"], json!({"type": "adaptive"}));
        assert_eq!(both["output_config"], json!({"effort": "high"}));
        assert!(!both.to_string().contains("budget_tokens"));
        let effort = body(Some(Deeper { thinking: false, effort: true }));
        assert!(effort.get("thinking").is_none() && effort["output_config"]["effort"] == "high");
        let plain = body(None);
        assert!(plain.get("thinking").is_none() && plain.get("output_config").is_none());
    }

    fn replayed(events: Vec<serde_json::Value>) -> Replay {
        let mut replay = Replay::default();
        for e in events {
            let e = serde_json::from_value::<SseEvent>(e).unwrap();
            match (e.event_type.as_str(), e.index) {
                ("content_block_start", Some(i)) => replay.start(i, e.content_block.as_ref().unwrap()),
                ("content_block_delta", Some(i)) => replay.delta(i, e.delta.as_ref().unwrap()),
                _ => {}
            }
        }
        replay
    }

    /// A round's reply: a thinking block (the current models think by
    /// default, Think harder or not), then a tool call.
    fn thought_then_called(id: &str) -> Replay {
        replayed(vec![
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Check the calendar."}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "c2ln"}}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": id, "name": "list_events", "input": {}}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"days\":7}"}}),
        ])
    }

    fn turn_requests(deeper: Option<Deeper>) -> TurnRequests {
        TurnRequests::new("m".into(), 1_024, "You are JARVIS.".into(), vec![json!({"name": "list_events", "input_schema": {"type": "object"}})], vec![("user".into(), "What is on this week?".into())], deeper)
    }

    fn last_reply(turn: &TurnRequests) -> serde_json::Value {
        serde_json::to_value(&turn.request(false).messages.last().unwrap().content).unwrap()
    }

    /// Only a Think harder turn sends its thinking blocks back; any other
    /// turn leaves them out, as main does (P06 fix round 4: review A I-3 is
    /// not changed; a request without them is accepted).
    #[test]
    fn only_a_think_harder_turn_sends_its_thinking_blocks_back() {
        let call = json!({"type": "tool_use", "id": "t1", "name": "list_events", "input": {"days": 7}});
        let tool_uses = vec![("t1".to_string(), "list_events".to_string(), json!({"days": 7}))];
        let mut plain = turn_requests(None);
        plain.add_reply(thought_then_called("t1"), &[], &tool_uses);
        assert_eq!(last_reply(&plain), json!([call]));
        for deeper in [Deeper { thinking: true, effort: false }, Deeper { thinking: false, effort: true }, Deeper { thinking: true, effort: true }] {
            let mut harder = turn_requests(Some(deeper));
            harder.add_reply(thought_then_called("t1"), &[], &tool_uses);
            assert_eq!(last_reply(&harder), json!([{"type": "thinking", "thinking": "Check the calendar.", "signature": "c2ln"}, call]), "{deeper:?}");
        }
    }

    /// On a Think harder turn the system text, the tools and every earlier
    /// message go out byte for byte the same in each tool round; a round only
    /// appends. Anthropic refuses a later round whose earlier parts differ
    /// (accounts created on or after 2026-08-31); not checked live.
    #[test]
    fn a_think_harder_turn_keeps_its_earlier_parts_byte_identical_between_rounds() {
        let mut turn = turn_requests(Some(Deeper { thinking: true, effort: true }));
        let mut previous = serde_json::to_value(turn.request(false)).unwrap();
        for (round, id) in ["t1", "t2"].into_iter().enumerate() {
            turn.add_reply(thought_then_called(id), &[], &[(id.to_string(), "list_events".to_string(), json!({"days": 7}))]);
            turn.add_results(vec![ContentBlock::ToolResult { tool_use_id: id.into(), content: format!("No events ({round}).") }]);
            let next = serde_json::to_value(turn.request(round == 1)).unwrap();
            for part in ["system", "tools", "thinking", "output_config"] {
                assert_eq!(serde_json::to_string(&next[part]).unwrap(), serde_json::to_string(&previous[part]).unwrap(), "{part}, round {round}");
            }
            let (before, after) = (previous["messages"].as_array().unwrap(), next["messages"].as_array().unwrap());
            assert_eq!(after.len(), before.len() + 2, "a round appends its reply and its results");
            for (i, message) in before.iter().enumerate() {
                assert_eq!(serde_json::to_string(&after[i]).unwrap(), serde_json::to_string(message).unwrap(), "message {i}, round {round}");
            }
            previous = next;
        }
    }

    /// A turn that asked for thinking sends the reply back as it streamed:
    /// the thinking block with its signature, the text and the tool call in
    /// their order, and a redacted block untouched.
    #[test]
    fn thinking_blocks_go_back_unchanged_before_the_tool_call() {
        let event = |data: serde_json::Value| serde_json::from_value::<SseEvent>(data).unwrap();
        let mut replay = Replay::default();
        for e in [
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Check the "}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "calendar."}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "c2ln"}}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "redacted_thinking", "data": "b3BhcXVl"}}),
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_start", "index": 4, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "text_delta", "text": "Looking."}}),
            json!({"type": "content_block_start", "index": 3, "content_block": {"type": "tool_use", "id": "t1", "name": "list_events", "input": {}}}),
            json!({"type": "content_block_delta", "index": 3, "delta": {"type": "input_json_delta", "partial_json": "{\"days\":"}}),
            json!({"type": "content_block_delta", "index": 3, "delta": {"type": "input_json_delta", "partial_json": "7}"}}),
        ] {
            let e = event(e);
            match (e.event_type.as_str(), e.index) {
                ("content_block_start", Some(i)) => replay.start(i, e.content_block.as_ref().unwrap()),
                ("content_block_delta", Some(i)) => replay.delta(i, e.delta.as_ref().unwrap()),
                _ => {}
            }
        }
        let sent = serde_json::to_value(replay.into_blocks()).unwrap();
        assert_eq!(sent, json!([
            {"type": "thinking", "thinking": "Check the calendar.", "signature": "c2ln"},
            {"type": "redacted_thinking", "data": "b3BhcXVl"},
            {"type": "text", "text": "Looking."},
            {"type": "tool_use", "id": "t1", "name": "list_events", "input": {"days": 7}},
        ]));
    }
}
