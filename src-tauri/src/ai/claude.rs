use super::tools;
use crate::voice::tts::TtsCommand;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use crate::chat_session::ScopedEmitter;

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
    partial_json: Option<String>,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct SseContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    id: Option<String>,
    name: Option<String>,
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

    let mut claude_messages: Vec<ClaudeMessage> = messages
        .into_iter()
        .filter(|(_, content)| !content.trim().is_empty())
        .map(|(role, content)| ClaudeMessage {
            role,
            content: ClaudeContent::Text(content),
        })
        .collect();

    let mut turn = crate::ai::cost::TurnUsage::new("claude", &model);
    let started_at = std::time::Instant::now();

    let limits = crate::ai::models::limits("claude");
    let max_iterations = limits.max_tool_rounds.unwrap_or_default() as usize;
    let api_version = limits.api_version.unwrap_or_default();
    for round in 0..max_iterations {
        let final_round = round + 1 == max_iterations;
        let request = ClaudeRequest {
            model: model.clone(),
            max_tokens: limits.chat_max_output_tokens.unwrap_or_default(),
            system: system.clone(),
            tools: claude_tools.clone(),
            tool_choice: final_round.then(|| json!({"type": "none"})),
            messages: claude_messages.clone(),
            stream: true,
        };

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
                    log::error!("Claude SSE error: {}", data_str);
                    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    round_usage.add_to(&mut turn);
                    turn.record_if_billed(db, workload, started_at.elapsed().as_millis() as i64);
                    return Err(format!("Claude stream error: {}", data_str).into());
                }

                // Parse the data JSON
                let sse: SseEvent = match serde_json::from_str(&data_str) {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!(
                            "[STREAM-DEBUG] SSE parse failed: {} | data: {}",
                            e,
                            crate::util::truncate_bytes(&data_str, 200)
                        );
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
            // Build assistant message with text and tool_use blocks
            let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
            for text in &text_parts {
                assistant_blocks.push(ContentBlock::Text { text: text.clone() });
            }
            for (id, name, input) in &tool_uses {
                assistant_blocks.push(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
            }
            claude_messages.push(ClaudeMessage {
                role: "assistant".into(),
                content: ClaudeContent::Blocks(assistant_blocks),
            });

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
            claude_messages.push(ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Blocks(result_blocks),
            });

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
            serde_json::to_value(ClaudeRequest { model: "m".into(), max_tokens: 1, system: String::new(), tools: Vec::new(), tool_choice, messages: Vec::new(), stream: true }).unwrap()
        };
        assert_eq!(request(Some(json!({"type": "none"})))["tool_choice"]["type"], "none");
        assert!(request(None).get("tool_choice").is_none());
    }
}
