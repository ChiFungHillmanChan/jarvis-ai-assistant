use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use super::tools;
use tauri::Emitter;
use crate::voice::tts::TtsCommand;

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    system: String,
    tools: Vec<serde_json::Value>,
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
    ToolUse { id: String, name: String, input: serde_json::Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String },
}

// ClaudeResponse and ResponseBlock are no longer needed (SSE replaces JSON parsing)

#[derive(Deserialize)]
struct SseEvent {
    #[serde(rename = "type")]
    event_type: String,
    index: Option<usize>,
    delta: Option<SseDelta>,
    content_block: Option<SseContentBlock>,
}

#[derive(Deserialize)]
struct SseDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
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

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    app_handle: &tauri::AppHandle,
    tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::builder()
        .read_timeout(std::time::Duration::from_secs(60))
        .build()?;
    let tool_defs = tools::get_tool_definitions();
    let claude_tools = tools::to_claude_format(&tool_defs);

    let mut claude_messages: Vec<ClaudeMessage> = messages.into_iter().map(|(role, content)| {
        ClaudeMessage { role, content: ClaudeContent::Text(content) }
    }).collect();

    let max_iterations = 5;
    for _ in 0..max_iterations {
        let request = ClaudeRequest {
            model: "claude-sonnet-4-6-20250610".into(),
            max_tokens: 4096,
            system: tools::SYSTEM_PROMPT.into(),
            tools: claude_tools.clone(),
            messages: claude_messages.clone(),
            stream: true,
        };

        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Claude API error {}: {}", status, body).into());
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_map: HashMap<usize, (String, String, String)> = HashMap::new();
        let mut stop_reason: Option<String> = None;

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => bytes,
                Err(e) => {
                    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    return Err(format!("Stream error: {}", e).into());
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE events (separated by \n\n)
            while let Some(pos) = buffer.find("\n\n") {
                let event_block = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                // Parse SSE lines
                let mut event_type = String::new();
                let mut data_str = String::new();

                for line in event_block.lines() {
                    if line.starts_with(':') { continue; }
                    if let Some(rest) = line.strip_prefix("event: ") {
                        event_type = rest.to_string();
                    } else if let Some(rest) = line.strip_prefix("data: ") {
                        data_str = rest.to_string();
                    }
                }

                if data_str.is_empty() { continue; }
                if data_str == "[DONE]" { continue; }

                // Handle error events
                if event_type == "error" {
                    log::error!("Claude SSE error: {}", data_str);
                    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    return Err(format!("Claude stream error: {}", data_str).into());
                }

                // Parse the data JSON
                let sse: SseEvent = match serde_json::from_str(&data_str) {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!("[STREAM-DEBUG] SSE parse failed: {} | data: {}", e, &data_str[..data_str.len().min(200)]);
                        continue;
                    }
                };

                log::info!("[STREAM-DEBUG] SSE event: {}", sse.event_type);
                match sse.event_type.as_str() {
                    "content_block_start" => {
                        if let Some(ref cb) = sse.content_block {
                            if cb.block_type == "tool_use" {
                                if let (Some(ref id), Some(ref name), Some(idx)) = (&cb.id, &cb.name, sse.index) {
                                    tool_map.insert(idx, (id.clone(), name.clone(), String::new()));
                                    let label = tools::tool_status_label(name);
                                    let _ = app_handle.emit("chat-status", json!({"status": label}));
                                }
                            }
                        }
                    }
                    "content_block_delta" => {
                        if let Some(ref delta) = sse.delta {
                            match delta.delta_type.as_deref() {
                                Some("text_delta") => {
                                    if let Some(ref text) = delta.text {
                                        text_parts.push(text.clone());
                                        let _ = app_handle.emit("chat-token", json!({"token": text, "done": false}));
                                        if let Some(ref tx) = tts_tx {
                                            let _ = tx.try_send(TtsCommand::TextChunk(text.clone()));
                                        }
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
                        if let Some(ref delta) = sse.delta {
                            if let Some(ref sr) = delta.stop_reason {
                                stop_reason = Some(sr.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Collect tool calls from the map
        let tool_uses: Vec<(String, String, serde_json::Value)> = tool_map
            .into_values()
            .map(|(id, name, json_str)| {
                let input = serde_json::from_str(&json_str).unwrap_or_default();
                (id, name, input)
            })
            .collect();

        // If there are tool calls, execute them and continue
        if !tool_uses.is_empty() {
            // Build assistant message with text and tool_use blocks
            let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
            for text in &text_parts {
                assistant_blocks.push(ContentBlock::Text { text: text.clone() });
            }
            for (id, name, input) in &tool_uses {
                assistant_blocks.push(ContentBlock::ToolUse {
                    id: id.clone(), name: name.clone(), input: input.clone(),
                });
            }
            claude_messages.push(ClaudeMessage {
                role: "assistant".into(),
                content: ClaudeContent::Blocks(assistant_blocks),
            });

            // Execute tools -- deduplicate same-name calls (e.g. 4x open_url → run only the first)
            let mut result_blocks = Vec::new();
            let mut executed_tools: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut narrated = false;
            for (id, name, input) in &tool_uses {
                let is_duplicate = executed_tools.contains(name.as_str());
                if is_duplicate {
                    log::info!("JARVIS skipping duplicate tool call: {}", name);
                    result_blocks.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: "Skipped: already executed this tool in this batch.".to_string(),
                    });
                    continue;
                }
                executed_tools.insert(name.clone());
                let args_str = serde_json::to_string(input).unwrap_or_default();
                log::info!("JARVIS tool call: {}({})", name, args_str);
                let _ = app_handle.emit("chat-tool-call", json!({"tool_name": name}));
                let label = tools::tool_status_label(name);
                let _ = app_handle.emit("chat-status", json!({"status": format!("Running: {}", label)}));
                if !narrated {
                    if let Some(ref tx) = tts_tx {
                        // Always narrate -- this also flushes any buffered AI text
                        let narration = tools::tool_voice_narration(name);
                        let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                        narrated = true;
                    }
                }
                let result = tools::execute_tool(name, &args_str, db, google_auth).await;
                log::info!("JARVIS tool result: {}", &result[..result.len().min(200)]);
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
                return Ok(text_parts.join(""));
            }
            let _ = app_handle.emit("chat-status", json!({"status": "Composing response..."}));
            continue;
        }

        // No tool calls -- emit done and return text
        let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
        if !text_parts.is_empty() {
            return Ok(text_parts.join(""));
        }
        return Ok(String::new());
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    Ok("I've completed the actions.".into())
}
