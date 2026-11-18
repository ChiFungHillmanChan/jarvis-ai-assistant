use super::tools;
use crate::voice::tts::TtsCommand;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use crate::chat_session::ScopedEmitter;
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
    fn add_to(&self, turn: &mut crate::ai::cost::TurnUsage) {
        turn.add_request(self.input + self.cache_read + self.cache_write, self.output, self.cache_read, self.cache_write);
    }
}


pub async fn send(
    api_key: &str,
    model: &str,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    app_handle: &tauri::AppHandle,
    tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
    workload: crate::ai::Workload,
    deeper: Option<Deeper>,
    ledger: &crate::ai::SideEffectLedger,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::builder()
        .read_timeout(std::time::Duration::from_secs(60))
        .build()?;
    let tool_defs = tools::get_tool_definitions();
    let claude_tools = tools::to_claude_format(&tool_defs);
    // Pre-compute static fields once to avoid re-allocating each iteration
    let model: String = model.into();
    let system: String = tools::build_system_prompt(db);

    let mut turn = crate::ai::cost::TurnUsage::new("claude", &model);
    let started_at = std::time::Instant::now();

    let limits = crate::ai::models::limits("claude");
    let max_iterations = limits.max_tool_rounds.unwrap_or_default() as usize;
    let api_version = limits.api_version.unwrap_or_default();
    let mut requests = TurnRequests::new(model.clone(), limits.chat_max_output_tokens.unwrap_or_default(), system, claude_tools, messages, deeper);
    for round in 0..max_iterations {
        let final_round = round + 1 == max_iterations;
        let request = requests.request(final_round);

        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", &api_version)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(e) => {
                turn.record_if_billed(db, workload, started_at.elapsed().as_millis() as i64);
                return Err(e.into());
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            turn.record_if_billed(db, workload, started_at.elapsed().as_millis() as i64);
            return Err(format!("Claude API error {}: {}", status, body).into());
        }

        let mut stream = response.bytes_stream();
        let mut buffer = super::SseByteBuffer::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_map: HashMap<usize, (String, String, String)> = HashMap::new();
        let mut announced_tools: HashSet<usize> = HashSet::new();
        let mut stop_reason: Option<String> = None;
        let mut response_started = false;
        let mut round_usage = RoundUsage::default();
        let mut replay = Replay::default();

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => bytes,
                Err(e) => {
                    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    round_usage.add_to(&mut turn);
                    turn.record_if_billed(db, workload, started_at.elapsed().as_millis() as i64);
                    return Err(format!("Stream error: {}", e).into());
                }
            };

            buffer.extend(&chunk);

            // Process complete SSE events (separated by \n\n)
            while let Some(event_block) = buffer.next_event() {
                // Parse SSE lines
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

                if data_str.is_empty() {
                    continue;
                }
                if data_str == "[DONE]" {
                    continue;
                }

                // Handle error events
                if event_type == "error" {
                    log::error!("{}", super::failure::stream_error_line("Claude", &data_str));
                    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    round_usage.add_to(&mut turn);
                    turn.record_if_billed(db, workload, started_at.elapsed().as_millis() as i64);
                    return Err(format!("Claude stream error: {}", data_str).into());
                }

                // Parse the data JSON
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
                        if let Some(ref msg) = sse.message {
                            if let Some(ref u) = msg.usage {
                                round_usage.input = u.input_tokens.unwrap_or(0);
                                round_usage.cache_read = u.cache_read_input_tokens.unwrap_or(0);
                                round_usage.cache_write = u.cache_creation_input_tokens.unwrap_or(0);
                            }
                        }
                    }
                    "content_block_start" => {
                        if let Some(ref cb) = sse.content_block {
                            if let Some(idx) = sse.index {
                                replay.start(idx, cb);
                            }
                            if cb.block_type == "tool_use" {
                                if let (Some(ref id), Some(ref name), Some(idx)) =
                                    (&cb.id, &cb.name, sse.index)
                                {
                                    tool_map.insert(idx, (id.clone(), name.clone(), String::new()));
                                    let label = tools::tool_status_label(name);
                                    let _ = app_handle.emit(
                                        "chat-status",
                                        json!({
                                            "status": format!("Planning: {}", label),
                                            "phase": "planning"
                                        }),
                                    );
                                    if announced_tools.insert(idx) {
                                        let _ = app_handle
                                            .emit("chat-tool-call", json!({"tool_name": name}));
                                    }
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
                                        if !response_started {
                                            response_started = true;
                                            let _ = app_handle.emit(
                                                "chat-status",
                                                json!({
                                                    "status": "Composing response...",
                                                    "phase": "responding"
                                                }),
                                            );
                                        }
                                        text_parts.push(text.clone());
                                        let _ = app_handle.emit(
                                            "chat-token",
                                            json!({"token": text, "done": false}),
                                        );
                                        if let Some(ref tx) = tts_tx {
                                            let _ =
                                                tx.try_send(TtsCommand::TextChunk(text.clone()));
                                        }
                                    }
                                }
                                Some("thinking_delta") => {
                                    if let Some(ref text) = delta.thinking {
                                        let _ = app_handle.emit(
                                            "chat-thinking",
                                            json!({
                                                "text": text,
                                                "done": false
                                            }),
                                        );
                                    }
                                }
                                Some("input_json_delta") => {
                                    if let (Some(ref pj), Some(idx)) =
                                        (&delta.partial_json, sse.index)
                                    {
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
                        if let Some(ref delta) = sse.delta {
                            if let Some(ref sr) = delta.stop_reason {
                                stop_reason = Some(sr.clone());
                            }
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

        round_usage.add_to(&mut turn);

        // Collect tool calls from the map
        let tool_uses: Vec<(String, String, serde_json::Value)> = tool_map
            .into_values()
            .map(|(id, name, json_str)| {
                let input = serde_json::from_str(&json_str).unwrap_or_default();
                (id, name, input)
            })
            .collect();

        // If there are tool calls, execute them and continue. The last round
        // runs no tools.
        if !tool_uses.is_empty() && !final_round {
            requests.add_reply(replay, &text_parts, &tool_uses);

            // Execute tools -- deduplicate identical calls (e.g. 4x the same
            // open_url), but let a fan-out with different arguments through.
            let mut result_blocks = Vec::new();
            let mut executed_tools: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut narrated = false;
            for (id, name, input) in &tool_uses {
                let args_str = serde_json::to_string(input).unwrap_or_default();
                let call_key = crate::ai::tool_call_key(name, &args_str);
                let is_duplicate = executed_tools.contains(&call_key);
                if is_duplicate {
                    log::info!("JARVIS skipping duplicate tool call: {}", name);
                    result_blocks.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: "Skipped: already executed this tool with these arguments in this batch."
                            .to_string(),
                    });
                    continue;
                }
                executed_tools.insert(call_key);
                log::info!("JARVIS tool call: {} ({} bytes of arguments)", name, args_str.len());
                let label = tools::tool_status_label(name);
                let _ = app_handle.emit(
                    "chat-status",
                    json!({
                        "status": format!("Running: {}", label),
                        "detail": tools::tool_status_detail(name, &args_str),
                        "phase": "acting"
                    }),
                );
                if !narrated {
                    if let Some(ref tx) = tts_tx {
                        // Always narrate -- this also flushes any buffered AI text
                        let narration = tools::tool_voice_narration(name, &tools::read_language_pref(db));
                        let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                        narrated = true;
                    }
                }
                ledger.note(name);
                let result = tools::execute_tool(name, &args_str, db, google_auth, ledger).await;
                log::info!("JARVIS tool result: {}", crate::util::truncate_bytes(&result, 200));
                result_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: result,
                });
            }
            requests.add_results(result_blocks);

            // If stop_reason is "end_turn", we're done even with tool calls
            if stop_reason.as_deref() == Some("end_turn") && !text_parts.is_empty() {
                let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
                let _ = app_handle.emit("chat-thinking", json!({"text": "", "done": true}));
                turn.record(db, workload, started_at.elapsed().as_millis() as i64);
                return Ok(text_parts.join(""));
            }
            let _ = app_handle.emit(
                "chat-status",
                json!({
                    "status": "Reviewing tool results...",
                    "phase": "thinking"
                }),
            );
            continue;
        }

        // No tool calls -- emit done and return text
        let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
        let _ = app_handle.emit("chat-thinking", json!({"text": "", "done": true}));
        turn.record(db, workload, started_at.elapsed().as_millis() as i64);
        if !text_parts.is_empty() {
            return Ok(text_parts.join(""));
        }
        if final_round {
            return Ok(tools::tool_limit_fallback(&tools::read_language_pref(db)).into());
        }
        return Ok(String::new());
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    let _ = app_handle.emit("chat-thinking", json!({"text": "", "done": true}));
    turn.record(db, workload, started_at.elapsed().as_millis() as i64);
    // The last round always returns above.
    Ok(tools::tool_limit_fallback(&tools::read_language_pref(db)).into())
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
