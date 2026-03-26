pub mod backend;
pub mod context;
pub mod generic;
pub mod ollama;
pub mod prompt_inject;
pub mod vllm;

use backend::{BackendType, LocalBackend, LocalEndpoint, ToolCapability};
use context::ContextManager;
use prompt_inject::{
    build_tool_injection_prompt, effective_tool_capability, parse_tool_calls_from_response,
    select_relevant_tools,
};

use crate::ai::tools;
use crate::voice::tts::TtsCommand;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tauri::Emitter;

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<LocalMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct LocalMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<LocalToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct LocalToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: LocalFunction,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct LocalFunction {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct SseChunk {
    choices: Option<Vec<SseChoice>>,
}

#[derive(Deserialize)]
struct SseChoice {
    delta: Option<SseDelta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct SseDelta {
    content: Option<String>,
    tool_calls: Option<Vec<SseToolCallDelta>>,
}

#[derive(Deserialize)]
struct SseToolCallDelta {
    index: Option<usize>,
    id: Option<String>,
    function: Option<SseFunctionDelta>,
}

#[derive(Deserialize)]
struct SseFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
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

/// Unified inference path for all local models using OpenAI-compatible API
pub async fn send_local(
    endpoint: &LocalEndpoint,
    model: &str,
    messages: Vec<(String, String)>,
    tool_capability: ToolCapability,
    context_length: u32,
    tool_capability_override: Option<&str>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    app_handle: &tauri::AppHandle,
    tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let effective_cap =
        effective_tool_capability(&tool_capability, tool_capability_override, context_length);

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_millis(
            endpoint.connection_timeout_ms as u64,
        ))
        .build()?;

    let all_tool_defs = tools::get_tool_definitions();
    let openai_tools = tools::to_openai_format(&all_tool_defs);

    // Build system prompt
    let mut system_prompt = tools::SYSTEM_PROMPT.to_string();

    // Determine tools to use based on capability
    let (request_tools, use_response_format) = match effective_cap {
        ToolCapability::Native => (Some(openai_tools), false),
        ToolCapability::PromptInjected => {
            let max_tools = ContextManager::max_tools_for_context(context_length);
            let user_msg = messages.last().map(|(_, c)| c.as_str()).unwrap_or("");
            let relevant = select_relevant_tools(&all_tool_defs, user_msg, max_tools);
            let injection = build_tool_injection_prompt(&relevant);
            system_prompt.push_str(&injection);
            (None, true)
        }
        ToolCapability::ChatOnly => (None, false),
    };

    // Context management: truncate messages to fit
    let ctx_manager = ContextManager::new(
        context_length,
        matches!(effective_cap, ToolCapability::PromptInjected),
    );
    let system_tokens = ContextManager::count_tokens(&system_prompt);
    let truncated = ctx_manager.truncate_messages(&messages, system_tokens);

    // Build message list
    let mut local_messages: Vec<LocalMessage> = vec![LocalMessage {
        role: "system".into(),
        content: Some(system_prompt),
        tool_calls: None,
        tool_call_id: None,
    }];
    local_messages.extend(truncated.into_iter().map(|(role, content)| LocalMessage {
        role,
        content: Some(content),
        tool_calls: None,
        tool_call_id: None,
    }));

    // Build base URL for OpenAI-compatible endpoint
    let base_url = endpoint.url.trim_end_matches('/');
    let chat_url = if endpoint.backend_type == BackendType::Ollama {
        format!("{}/v1/chat/completions", base_url)
    } else {
        format!("{}/v1/chat/completions", base_url)
    };

    let max_iterations = 5;
    for _ in 0..max_iterations {
        let request = ChatCompletionRequest {
            model: model.to_string(),
            messages: local_messages.clone(),
            tools: request_tools.clone(),
            max_tokens: Some(4096.min(context_length / 2)),
            stream: true,
            response_format: if use_response_format {
                Some(json!({"type": "json_object"}))
            } else {
                None
            },
        };

        let mut req = client
            .post(&chat_url)
            .header("Content-Type", "application/json");

        if let Some(ref key) = endpoint.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        // Ollama keep_alive support
        if endpoint.backend_type == BackendType::Ollama {
            // Ollama accepts keep_alive as part of the request via options
            // but for /v1/chat/completions, we pass it differently -- skip for now
            // The keep_alive is set at the Ollama server level
        }

        let response = req.json(&request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Local LLM API error {}: {}", status, body).into());
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_map: HashMap<usize, (String, String, String)> = HashMap::new();
        let mut announced_tools: HashSet<usize> = HashSet::new();
        let mut finish_reason: Option<String> = None;
        let mut response_started = false;

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => bytes,
                Err(e) => {
                    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    return Err(format!("Stream error: {}", e).into());
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find("\n\n") {
                let event_block = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

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

                if let Some(choices) = &chunk.choices {
                    for choice in choices {
                        if let Some(ref delta) = choice.delta {
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

                            if let Some(ref tool_calls) = delta.tool_calls {
                                for tc in tool_calls {
                                    let idx = tc.index.unwrap_or(0);
                                    let entry = tool_map
                                        .entry(idx)
                                        .or_insert_with(|| (String::new(), String::new(), String::new()));
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
                                            if announced_tools.insert(idx) {
                                                let _ = app_handle.emit(
                                                    "chat-tool-call",
                                                    json!({"tool_name": name}),
                                                );
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
        }

        // Handle prompt-injected tool calls (parse from text response)
        if matches!(effective_cap, ToolCapability::PromptInjected) && tool_map.is_empty() {
            let full_text = text_parts.join("");
            let (parsed_calls, response_text) = parse_tool_calls_from_response(&full_text);

            if let Some(calls) = parsed_calls {
                // Execute parsed tool calls
                let mut executed_tools: HashSet<String> = HashSet::new();
                let mut narrated = false;

                for call in &calls {
                    if executed_tools.contains(&call.name) {
                        continue;
                    }
                    executed_tools.insert(call.name.clone());

                    let label = tools::tool_status_label(&call.name);
                    let _ = app_handle.emit(
                        "chat-status",
                        json!({
                            "status": format!("Running: {}", label),
                            "phase": "acting"
                        }),
                    );
                    if !narrated {
                        if let Some(ref tx) = tts_tx {
                            let narration = tools::tool_voice_narration(&call.name);
                            let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                            narrated = true;
                        }
                    }

                    let args_str =
                        serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".into());
                    let result =
                        tools::execute_tool(&call.name, &args_str, db, google_auth).await;
                    log::info!("Local LLM tool result: {}", &result[..result.len().min(200)]);

                    // Add tool results to messages and continue the loop
                    local_messages.push(LocalMessage {
                        role: "assistant".into(),
                        content: Some(full_text.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                    local_messages.push(LocalMessage {
                        role: "user".into(),
                        content: Some(format!(
                            "Tool '{}' returned: {}\n\nNow respond to the user based on this result. Do NOT call any more tools. Respond in plain text.",
                            call.name, result
                        )),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }

                if !calls.is_empty() {
                    let _ = app_handle.emit(
                        "chat-status",
                        json!({
                            "status": "Reviewing tool results...",
                            "phase": "thinking"
                        }),
                    );
                    // Clear text_parts since we're continuing the loop
                    continue;
                }

                if !response_text.is_empty() {
                    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
                    return Ok(response_text);
                }
            } else if !response_text.is_empty() {
                // No tool calls, just text response from JSON parsing
                let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
                return Ok(response_text);
            }
        }

        // Handle native tool calls (OpenAI format)
        if finish_reason.as_deref() == Some("tool_calls") && !tool_map.is_empty() {
            let text_content = text_parts.join("");
            let mut tool_calls_vec: Vec<LocalToolCall> = Vec::new();
            let mut sorted_indices: Vec<usize> = tool_map.keys().copied().collect();
            sorted_indices.sort();
            for idx in &sorted_indices {
                let (ref call_id, ref fn_name, ref args) = tool_map[idx];
                tool_calls_vec.push(LocalToolCall {
                    id: call_id.clone(),
                    call_type: "function".into(),
                    function: LocalFunction {
                        name: fn_name.clone(),
                        arguments: args.clone(),
                    },
                });
            }

            local_messages.push(LocalMessage {
                role: "assistant".into(),
                content: if text_content.is_empty() {
                    None
                } else {
                    Some(text_content)
                },
                tool_calls: Some(tool_calls_vec.clone()),
                tool_call_id: None,
            });

            let mut executed_tools: HashSet<String> = HashSet::new();
            let mut narrated = false;
            for tc in &tool_calls_vec {
                let is_duplicate = executed_tools.contains(&tc.function.name);
                if is_duplicate {
                    log::info!("Local LLM skipping duplicate tool call: {}", tc.function.name);
                    local_messages.push(LocalMessage {
                        role: "tool".into(),
                        content: Some(
                            "Skipped: already executed this tool in this batch.".to_string(),
                        ),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                    });
                    continue;
                }
                executed_tools.insert(tc.function.name.clone());
                log::info!(
                    "Local LLM tool call: {}({})",
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
                        let narration = tools::tool_voice_narration(&tc.function.name);
                        let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                        narrated = true;
                    }
                }
                let result =
                    tools::execute_tool(&tc.function.name, &tc.function.arguments, db, google_auth)
                        .await;
                log::info!("Local LLM tool result: {}", &result[..result.len().min(200)]);
                local_messages.push(LocalMessage {
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

        // No tool calls -- done
        let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
        return Ok(text_parts.join(""));
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    Ok("I've completed the actions.".into())
}
