use super::tools;
use crate::voice::tts::TtsCommand;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use tauri::Emitter;

pub const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
pub const MODEL_PRO: &str = "gemini-3.1-pro-preview";
pub const MODEL_FLASH: &str = "gemini-3-flash-preview";

#[derive(Serialize, Deserialize, Clone, Debug)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCallData,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: FunctionResponseData,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FunctionCallData {
    name: String,
    args: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FunctionResponseData {
    name: String,
    response: serde_json::Value,
}

#[derive(Deserialize, Debug)]
struct StreamChunk {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize, Debug)]
struct Candidate {
    content: Option<CandidateContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CandidateContent {
    parts: Option<Vec<ResponsePart>>,
}

#[derive(Deserialize, Debug)]
struct ResponsePart {
    text: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<FunctionCallData>,
}

pub async fn send(
    api_key: &str,
    model: &str,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    app_handle: &tauri::AppHandle,
    tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::builder()
        .read_timeout(std::time::Duration::from_secs(90))
        .build()?;
    let tool_defs = tools::get_tool_definitions();
    let gemini_tools = tools::to_gemini_format(&tool_defs);

    let mut contents: Vec<GeminiContent> = messages
        .into_iter()
        .map(|(role, content)| {
            let gemini_role = if role == "assistant" { "model" } else { &role };
            GeminiContent {
                role: gemini_role.to_string(),
                parts: vec![GeminiPart::Text { text: content }],
            }
        })
        .collect();

    let system_instruction = json!({
        "parts": [{"text": tools::SYSTEM_PROMPT}]
    });

    let max_iterations = 5;
    for _ in 0..max_iterations {
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            API_BASE, model
        );

        let request_body = json!({
            "contents": contents,
            "tools": gemini_tools,
            "systemInstruction": system_instruction,
            "generationConfig": {
                "maxOutputTokens": 4096
            }
        });

        let response = client
            .post(&url)
            .header("x-goog-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Gemini API error {}: {}", status, body).into());
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut function_calls: Vec<FunctionCallData> = Vec::new();
        let mut announced_tools: HashSet<String> = HashSet::new();
        let mut response_started = false;
        let mut finish_reason: Option<String> = None;

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

                if data_str.is_empty() {
                    continue;
                }

                let sse: StreamChunk = match serde_json::from_str(&data_str) {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!(
                            "[gemini] SSE parse failed: {} | data: {}",
                            e,
                            &data_str[..data_str.len().min(200)]
                        );
                        continue;
                    }
                };

                if let Some(candidates) = sse.candidates {
                    for candidate in &candidates {
                        if let Some(ref fr) = candidate.finish_reason {
                            finish_reason = Some(fr.clone());
                        }

                        if let Some(ref content) = candidate.content {
                            if let Some(ref parts) = content.parts {
                                for part in parts {
                                    if let Some(ref text) = part.text {
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

                                    if let Some(ref fc) = part.function_call {
                                        let label = tools::tool_status_label(&fc.name);
                                        let _ = app_handle.emit(
                                            "chat-status",
                                            json!({
                                                "status": format!("Planning: {}", label),
                                                "phase": "planning"
                                            }),
                                        );
                                        if announced_tools.insert(fc.name.clone()) {
                                            let _ = app_handle.emit(
                                                "chat-tool-call",
                                                json!({"tool_name": &fc.name}),
                                            );
                                        }
                                        function_calls.push(fc.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !function_calls.is_empty() {
            let mut assistant_parts: Vec<GeminiPart> = Vec::new();
            let full_text = text_parts.join("");
            if !full_text.is_empty() {
                assistant_parts.push(GeminiPart::Text { text: full_text });
            }
            for fc in &function_calls {
                assistant_parts.push(GeminiPart::FunctionCall {
                    function_call: fc.clone(),
                });
            }
            contents.push(GeminiContent {
                role: "model".into(),
                parts: assistant_parts,
            });

            let mut response_parts: Vec<GeminiPart> = Vec::new();
            let mut executed_tools: HashSet<String> = HashSet::new();
            let mut narrated = false;
            for fc in &function_calls {
                let is_duplicate = executed_tools.contains(&fc.name);
                if is_duplicate {
                    log::info!("JARVIS skipping duplicate tool call: {}", fc.name);
                    response_parts.push(GeminiPart::FunctionResponse {
                        function_response: FunctionResponseData {
                            name: fc.name.clone(),
                            response: json!({"result": "Skipped: already executed this tool in this batch."}),
                        },
                    });
                    continue;
                }
                executed_tools.insert(fc.name.clone());
                let args_str = serde_json::to_string(&fc.args).unwrap_or_default();
                log::info!("JARVIS tool call: {}({})", fc.name, args_str);
                let label = tools::tool_status_label(&fc.name);
                let _ = app_handle.emit(
                    "chat-status",
                    json!({
                        "status": format!("Running: {}", label),
                        "phase": "acting"
                    }),
                );
                if !narrated {
                    if let Some(ref tx) = tts_tx {
                        let narration = tools::tool_voice_narration(&fc.name);
                        let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                        narrated = true;
                    }
                }
                let result = tools::execute_tool(&fc.name, &args_str, db, google_auth).await;
                log::info!("JARVIS tool result: {}", &result[..result.len().min(200)]);
                response_parts.push(GeminiPart::FunctionResponse {
                    function_response: FunctionResponseData {
                        name: fc.name.clone(),
                        response: json!({"result": result}),
                    },
                });
            }
            contents.push(GeminiContent {
                role: "user".into(),
                parts: response_parts,
            });

            if finish_reason.as_deref() == Some("STOP") && !text_parts.is_empty() {
                let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
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

        let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
        if !text_parts.is_empty() {
            return Ok(text_parts.join(""));
        }
        return Ok(String::new());
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    Ok("I've completed the actions.".into())
}
