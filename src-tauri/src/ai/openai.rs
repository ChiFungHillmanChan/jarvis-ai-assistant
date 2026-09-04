use super::tools;
use crate::voice::tts::TtsCommand;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tauri::Emitter;

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    tools: Vec<serde_json::Value>,
    max_completion_tokens: u32,
    stream: bool,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Deserialize, Debug, Default, Clone)]
struct OpenAIUsage {
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FunctionCall {
    name: String,
    arguments: String,
}

// OpenAIResponse and OpenAIChoice are no longer needed (SSE replaces JSON parsing)

#[derive(Deserialize)]
struct SseChunk {
    #[serde(default)]
    choices: Vec<SseChoice>,
    usage: Option<OpenAIUsage>,
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
    index: usize,
    id: Option<String>,
    function: Option<SseFunctionDelta>,
}

#[derive(Deserialize)]
struct SseFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    app_handle: &tauri::AppHandle,
    tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
    workload: crate::ai::Workload,
    ledger: &crate::ai::SideEffectLedger,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let started_at = std::time::Instant::now();
    let mut total_prompt_tokens: i64 = 0;
    let mut total_completion_tokens: i64 = 0;
    let model_id = "gpt-5.6-luna".to_string();
    let client = Client::builder()
        .read_timeout(std::time::Duration::from_secs(60))
        .build()?;
    let tool_defs = tools::get_tool_definitions();
    let openai_tools = tools::to_openai_format(&tool_defs);

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".into(),
        content: Some(tools::build_system_prompt(db)),
        tool_calls: None,
        tool_call_id: None,
    }];
    openai_messages.extend(messages.into_iter().map(|(role, content)| OpenAIMessage {
        role,
        content: Some(content),
        tool_calls: None,
        tool_call_id: None,
    }));

    let max_iterations = 5;
    for _ in 0..max_iterations {
        // Per-round usage: OpenAI sends one usage chunk per request, so
        // overwrite within a round and fold into totals when it ends.
        let mut usage = OpenAIUsage::default();
        let request = OpenAIRequest {
            model: model_id.clone(),
            messages: openai_messages.clone(),
            tools: openai_tools.clone(),
            max_completion_tokens: 4096,
            stream: true,
            stream_options: StreamOptions { include_usage: true },
        };

        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            record_partial_usage(
                db, workload, &model_id,
                total_prompt_tokens, total_completion_tokens,
                &started_at,
            );
            return Err(format!("OpenAI API error {}: {}", status, body).into());
        }

        let mut stream = response.bytes_stream();
        let mut buffer = super::SseByteBuffer::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_map: HashMap<usize, (String, String, String)> = HashMap::new(); // index -> (call_id, fn_name, arguments)
        let mut announced_tools: HashSet<usize> = HashSet::new();
        let mut finish_reason: Option<String> = None;
        let mut response_started = false;

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => bytes,
                Err(e) => {
                    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    total_prompt_tokens += usage.prompt_tokens.unwrap_or(0);
                    total_completion_tokens += usage.completion_tokens.unwrap_or(0);
                    record_partial_usage(
                        db, workload, &model_id,
                        total_prompt_tokens, total_completion_tokens,
                        &started_at,
                    );
                    return Err(format!("Stream error: {}", e).into());
                }
            };

            buffer.extend(&chunk);

            // Process complete SSE events
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

                if data_str.is_empty() {
                    continue;
                }
                if data_str == "[DONE]" {
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
                        // Text content
                        if let Some(ref content) = delta.content {
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
                            text_parts.push(content.clone());
                            let _ = app_handle
                                .emit("chat-token", json!({"token": content, "done": false}));
                            if let Some(ref tx) = tts_tx {
                                let _ = tx.try_send(TtsCommand::TextChunk(content.clone()));
                            }
                        }

                        // Tool calls
                        if let Some(ref tool_calls) = delta.tool_calls {
                            for tc in tool_calls {
                                let entry = tool_map.entry(tc.index).or_insert_with(|| {
                                    (String::new(), String::new(), String::new())
                                });
                                if let Some(ref id) = tc.id {
                                    entry.0 = id.clone();
                                }
                                if let Some(ref func) = tc.function {
                                    if let Some(ref name) = func.name {
                                        entry.1 = name.clone();
                                        let label = tools::tool_status_label(name);
                                        let _ = app_handle.emit(
                                            "chat-status",
                                            json!({
                                                "status": format!("Planning: {}", label),
                                                "phase": "planning"
                                            }),
                                        );
                                        if announced_tools.insert(tc.index) {
                                            let _ = app_handle
                                                .emit("chat-tool-call", json!({"tool_name": name}));
                                        }
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

        total_prompt_tokens += usage.prompt_tokens.unwrap_or(0);
        total_completion_tokens += usage.completion_tokens.unwrap_or(0);

        // Handle tool calls
        if finish_reason.as_deref() == Some("tool_calls") && !tool_map.is_empty() {
            let text_content = text_parts.join("");
            let mut tool_calls_vec: Vec<ToolCall> = Vec::new();
            let mut sorted_indices: Vec<usize> = tool_map.keys().copied().collect();
            sorted_indices.sort();
            for idx in &sorted_indices {
                let (ref call_id, ref fn_name, ref args) = tool_map[idx];
                tool_calls_vec.push(ToolCall {
                    id: call_id.clone(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: fn_name.clone(),
                        arguments: args.clone(),
                    },
                });
            }

            // Push assistant message with tool calls
            openai_messages.push(OpenAIMessage {
                role: "assistant".into(),
                content: if text_content.is_empty() {
                    None
                } else {
                    Some(text_content)
                },
                tool_calls: Some(tool_calls_vec.clone()),
                tool_call_id: None,
            });

            // Execute each tool -- deduplicate identical calls (e.g. 4x the same
            // open_url), but let a fan-out with different arguments through.
            let mut executed_tools: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut narrated = false;
            for tc in &tool_calls_vec {
                let call_key = crate::ai::tool_call_key(&tc.function.name, &tc.function.arguments);
                let is_duplicate = executed_tools.contains(&call_key);
                if is_duplicate {
                    log::info!("JARVIS skipping duplicate tool call: {}", tc.function.name);
                    openai_messages.push(OpenAIMessage {
                        role: "tool".into(),
                        content: Some(
                            "Skipped: already executed this tool with these arguments in this batch."
                                .to_string(),
                        ),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                    });
                    continue;
                }
                executed_tools.insert(call_key);
                log::info!(
                    "JARVIS tool call: {}({})",
                    tc.function.name,
                    tc.function.arguments
                );
                let label = tools::tool_status_label(&tc.function.name);
                let _ = app_handle.emit(
                    "chat-status",
                    json!({
                        "status": format!("Running: {}", label),
                        "phase": "acting"
                    }),
                );
                if !narrated {
                    if let Some(ref tx) = tts_tx {
                        // Always narrate -- this also flushes any buffered AI text
                        let narration = tools::tool_voice_narration(&tc.function.name);
                        let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                        narrated = true;
                    }
                }
                ledger.note(&tc.function.name);
                let result =
                    tools::execute_tool(&tc.function.name, &tc.function.arguments, db, google_auth)
                        .await;
                log::info!("JARVIS tool result: {}", crate::util::truncate_bytes(&result, 200));
                openai_messages.push(OpenAIMessage {
                    role: "tool".into(),
                    content: Some(result),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                });
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

        // No tool calls -- emit done and return the text
        let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
        if total_prompt_tokens == 0 && total_completion_tokens == 0 && !text_parts.is_empty() {
            log::warn!("[openai] no usage chunk received for non-empty response; recording zero tokens");
        }
        crate::ai::cost::record_inference(
            db, "openai", workload, &model_id,
            total_prompt_tokens,
            total_completion_tokens,
            0, 0,
            started_at.elapsed().as_millis() as i64,
        );
        return Ok(text_parts.join(""));
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    crate::ai::cost::record_inference(
        db, "openai", workload, &model_id,
        total_prompt_tokens,
        total_completion_tokens,
        0, 0,
        started_at.elapsed().as_millis() as i64,
    );
    Ok("I've completed the actions.".into())
}

/// Record usage accumulated so far when a turn ends abnormally, so tokens
/// already billed by completed rounds are not lost from telemetry.
fn record_partial_usage(
    db: &crate::db::Database,
    workload: crate::ai::Workload,
    model_id: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
    started_at: &std::time::Instant,
) {
    if prompt_tokens > 0 || completion_tokens > 0 {
        crate::ai::cost::record_inference(
            db, "openai", workload, model_id,
            prompt_tokens, completion_tokens, 0, 0,
            started_at.elapsed().as_millis() as i64,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
