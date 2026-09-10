use super::tools;
use crate::voice::tts::TtsCommand;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use crate::chat_session::ScopedEmitter;

pub const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
pub const MODEL_PRO: &str = "gemini-3.1-pro-preview";
pub const MODEL_FLASH: &str = "gemini-3.7-flash";

#[derive(Serialize, Deserialize, Clone, Debug)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct GeminiPart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(
        rename = "functionCall",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    function_call: Option<FunctionCallData>,
    #[serde(
        rename = "functionResponse",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    function_response: Option<FunctionResponseData>,
    #[serde(
        rename = "thoughtSignature",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    thought_signature: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    thought: bool,
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
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize, Debug, Default, Clone)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<i64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<i64>,
    #[serde(rename = "cachedContentTokenCount")]
    cached_content_token_count: Option<i64>,
}

#[derive(Deserialize, Debug)]
struct Candidate {
    content: Option<CandidateContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CandidateContent {
    parts: Option<Vec<GeminiPart>>,
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
    let started_at = std::time::Instant::now();
    let mut total_prompt_tokens: i64 = 0;
    let mut total_output_tokens: i64 = 0;
    let mut total_cached_tokens: i64 = 0;
    let client = Client::builder()
        .read_timeout(std::time::Duration::from_secs(90))
        .build()?;
    let tool_defs = tools::get_tool_definitions();
    let gemini_tools = tools::to_gemini_format(&tool_defs);

    let mut contents = history_contents(messages);

    let system_prompt = tools::build_system_prompt(db);
    let system_instruction = json!({
        "parts": [{"text": system_prompt}]
    });

    let max_iterations = 5;
    for _ in 0..max_iterations {
        // Per-round usage: Gemini reports usageMetadata cumulatively within one
        // request, so overwrite within a round and fold into totals when it ends.
        let mut usage = UsageMetadata::default();
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
            record_partial_usage(
                db, workload, model,
                total_prompt_tokens, total_output_tokens, total_cached_tokens,
                &started_at,
            );
            return Err(format!("Gemini API error {}: {}", status, body).into());
        }

        let mut stream = response.bytes_stream();
        let mut buffer = super::SseByteBuffer::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut function_call_parts: Vec<GeminiPart> = Vec::new();
        let mut announced_tools: HashSet<String> = HashSet::new();
        let mut response_started = false;
        let mut finish_reason: Option<String> = None;
        let mut event_count: usize = 0;
        let mut total_bytes: usize = 0;
        let mut first_chunk_sample: Option<String> = None;

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => bytes,
                Err(e) => {
                    let _ = app_handle.emit("chat-state", json!({"state": "idle"}));
                    total_prompt_tokens += usage.prompt_token_count.unwrap_or(0);
                    total_output_tokens += usage.candidates_token_count.unwrap_or(0);
                    total_cached_tokens += usage.cached_content_token_count.unwrap_or(0);
                    record_partial_usage(
                        db, workload, model,
                        total_prompt_tokens, total_output_tokens, total_cached_tokens,
                        &started_at,
                    );
                    return Err(format!("Stream error: {}", e).into());
                }
            };

            total_bytes += chunk.len();
            if first_chunk_sample.is_none() {
                first_chunk_sample =
                    Some(String::from_utf8_lossy(&chunk).chars().take(400).collect());
            }
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

                if data_str.is_empty() {
                    continue;
                }
                event_count += 1;

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

                if let Some(u) = sse.usage_metadata {
                    usage = u;
                }

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
                                        function_call_parts.push(part.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        total_prompt_tokens += usage.prompt_token_count.unwrap_or(0);
        total_output_tokens += usage.candidates_token_count.unwrap_or(0);
        total_cached_tokens += usage.cached_content_token_count.unwrap_or(0);

        if !function_call_parts.is_empty() {
            let mut assistant_parts: Vec<GeminiPart> = Vec::new();
            let full_text = text_parts.join("");
            if !full_text.is_empty() {
                assistant_parts.push(GeminiPart {
                    text: Some(full_text),
                    ..Default::default()
                });
            }
            for fc_part in &function_call_parts {
                assistant_parts.push(fc_part.clone());
            }
            contents.push(GeminiContent {
                role: "model".into(),
                parts: assistant_parts,
            });

            let mut response_parts: Vec<GeminiPart> = Vec::new();
            let mut executed_tools: HashSet<String> = HashSet::new();
            let mut narrated = false;
            for fc_part in &function_call_parts {
                let fc = match fc_part.function_call.as_ref() {
                    Some(fc) => fc,
                    None => continue,
                };
                let args_str = serde_json::to_string(&fc.args).unwrap_or_default();
                let call_key = crate::ai::tool_call_key(&fc.name, &args_str);
                let is_duplicate = executed_tools.contains(&call_key);
                if is_duplicate {
                    log::info!("JARVIS skipping duplicate tool call: {}", fc.name);
                    response_parts.push(GeminiPart {
                        function_response: Some(FunctionResponseData {
                            name: fc.name.clone(),
                            response: json!({"result": "Skipped: already executed this tool with these arguments in this batch."}),
                        }),
                        ..Default::default()
                    });
                    continue;
                }
                executed_tools.insert(call_key);
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
                ledger.note(&fc.name);
                let result = tools::execute_tool(&fc.name, &args_str, db, google_auth, ledger).await;
                log::info!("JARVIS tool result: {}", crate::util::truncate_bytes(&result, 200));
                response_parts.push(GeminiPart {
                    function_response: Some(FunctionResponseData {
                        name: fc.name.clone(),
                        response: json!({"result": result}),
                    }),
                    ..Default::default()
                });
            }
            contents.push(GeminiContent {
                role: "user".into(),
                parts: response_parts,
            });

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
            if total_prompt_tokens == 0 && total_output_tokens == 0 {
                log::warn!(
                    "[gemini] no usageMetadata received for non-empty response; recording zero tokens (model={})",
                    model
                );
            }
            crate::ai::cost::record_inference(
                db, "gemini", workload, model,
                total_prompt_tokens,
                total_output_tokens,
                total_cached_tokens,
                0,
                started_at.elapsed().as_millis() as i64,
            );
            return Ok(text_parts.join(""));
        }
        log::warn!(
            "[gemini] empty response: model={} finish_reason={:?} function_calls={} events={} bytes={} first_chunk={:?}",
            model,
            finish_reason,
            function_call_parts.len(),
            event_count,
            total_bytes,
            first_chunk_sample
        );
        crate::ai::cost::record_inference(
            db, "gemini", workload, model,
            total_prompt_tokens,
            total_output_tokens,
            total_cached_tokens,
            0,
            started_at.elapsed().as_millis() as i64,
        );
        return Err(format!("Gemini returned no text (finish_reason={:?})", finish_reason).into());
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    crate::ai::cost::record_inference(
        db, "gemini", workload, model,
        total_prompt_tokens,
        total_output_tokens,
        total_cached_tokens,
        0,
        started_at.elapsed().as_millis() as i64,
    );
    Ok("I've completed the actions.".into())
}

/// Map persisted `(role, content)` rows to Gemini contents. Rows whose content
/// is empty or whitespace are dropped: Gemini rejects an empty text part with
/// 400, so one such row would fail every later turn until "New chat".
fn history_contents(messages: Vec<(String, String)>) -> Vec<GeminiContent> {
    messages
        .into_iter()
        .filter(|(_, content)| !content.trim().is_empty())
        .map(|(role, content)| {
            let gemini_role = if role == "assistant" { "model" } else { &role };
            GeminiContent {
                role: gemini_role.to_string(),
                parts: vec![GeminiPart {
                    text: Some(content),
                    ..Default::default()
                }],
            }
        })
        .collect()
}

/// Record usage accumulated so far when a turn ends abnormally, so tokens
/// already billed by completed rounds are not lost from telemetry.
fn record_partial_usage(
    db: &crate::db::Database,
    workload: crate::ai::Workload,
    model: &str,
    prompt_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    started_at: &std::time::Instant,
) {
    if prompt_tokens > 0 || output_tokens > 0 || cached_tokens > 0 {
        crate::ai::cost::record_inference(
            db, "gemini", workload, model,
            prompt_tokens, output_tokens, cached_tokens, 0,
            started_at.elapsed().as_millis() as i64,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_chunk_parses_usage_metadata_field_names() {
        let json = r#"{"usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":34,"cachedContentTokenCount":5}}"#;
        let sse: StreamChunk = serde_json::from_str(json).unwrap();
        let u = sse.usage_metadata.unwrap();
        assert_eq!(u.prompt_token_count, Some(12));
        assert_eq!(u.candidates_token_count, Some(34));
        assert_eq!(u.cached_content_token_count, Some(5));
    }

    #[test]
    fn stream_chunk_tolerates_missing_usage_metadata() {
        let sse: StreamChunk = serde_json::from_str(r#"{"candidates":[]}"#).unwrap();
        assert!(sse.usage_metadata.is_none());
    }

    /// A persisted assistant row with no text must not reach the model:
    /// Gemini rejects an empty text part with 400, and one such row would
    /// fail every later turn until "New chat".
    #[test]
    fn empty_history_rows_are_dropped() {
        let contents = history_contents(vec![
            ("user".to_string(), "hello".to_string()),
            ("assistant".to_string(), String::new()),
            ("user".to_string(), "  \n\t".to_string()),
            ("assistant".to_string(), "hi".to_string()),
        ]);
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0].role, "user");
        assert_eq!(contents[0].parts[0].text.as_deref(), Some("hello"));
        assert_eq!(contents[1].role, "model");
        assert_eq!(contents[1].parts[0].text.as_deref(), Some("hi"));
    }

    /// `MODEL_PRO` / `MODEL_FLASH` are also hardcoded as TypeScript literals in
    /// Settings.tsx (a third copy, after this file and `AiRouter::new` in
    /// `ai/mod.rs`) so its
    /// "+ Gemini Pro" / "+ Gemini Flash" buttons send a model id that still
    /// exists. There is no JS test runner in this repo to catch drift on the
    /// TS side, so this reads the file as text instead.
    #[test]
    fn settings_page_offers_the_same_model_ids_as_gemini_rs() {
        let settings_tsx = include_str!("../../../src/pages/Settings.tsx");
        assert!(
            settings_tsx.contains(MODEL_PRO),
            "src/pages/Settings.tsx:57-58 must be updated to match gemini.rs: {MODEL_PRO} not found"
        );
        assert!(
            settings_tsx.contains(MODEL_FLASH),
            "src/pages/Settings.tsx:57-58 must be updated to match gemini.rs: {MODEL_FLASH} not found"
        );
    }
}
