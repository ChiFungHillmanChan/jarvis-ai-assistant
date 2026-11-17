use super::tools;
use crate::voice::tts::TtsCommand;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use crate::chat_session::ScopedEmitter;

pub const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

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
    /// Thinking, billed at the output rate; candidatesTokenCount leaves it out.
    #[serde(rename = "thoughtsTokenCount")]
    thoughts_token_count: Option<i64>,
}

impl UsageMetadata {
    /// Fold one request into the turn. promptTokenCount already includes
    /// the cached tokens.
    fn add_to(&self, turn: &mut crate::ai::cost::TurnUsage) {
        turn.add_request(
            self.prompt_token_count.unwrap_or(0),
            self.candidates_token_count.unwrap_or(0) + self.thoughts_token_count.unwrap_or(0),
            self.cached_content_token_count.unwrap_or(0),
            0,
        );
    }
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

/// The request's generationConfig. A "Think harder" turn on a model the
/// registry marks as thinking asks for the high thinking level (P06):
/// `thinkingLevel` is Google's setting for Gemini 3 and later, whose default
/// level differs per model. Google refuses it on earlier models, which ends
/// on the error card; nothing is switched.
fn generation_config(max_output_tokens: u32, deeper: Option<crate::ai::capability::Deeper>) -> serde_json::Value {
    let mut config = json!({"maxOutputTokens": max_output_tokens});
    if deeper.is_some_and(|d| d.thinking) {
        config["thinkingConfig"] = json!({"thinkingLevel": "HIGH"});
    }
    config
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
    deeper: Option<crate::ai::capability::Deeper>,
    ledger: &crate::ai::SideEffectLedger,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let started_at = std::time::Instant::now();
    let mut turn = crate::ai::cost::TurnUsage::new("gemini", model);
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

    let max_iterations = crate::ai::models::max_tool_rounds("gemini");
    for round in 0..max_iterations {
        let final_round = round + 1 == max_iterations;
        // Per-round usage: Gemini reports usageMetadata cumulatively within one
        // request, so overwrite within a round and fold into the turn when it ends.
        let mut usage = UsageMetadata::default();
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            API_BASE, model
        );

        let mut request_body = json!({
            "contents": contents,
            "tools": gemini_tools,
            "systemInstruction": system_instruction,
            "generationConfig": generation_config(output_cap(model), deeper)
        });
        if final_round {
            // The last allowed round answers from what earlier rounds gathered.
            request_body["toolConfig"] = json!({"functionCallingConfig": {"mode": "NONE"}});
        }

        let response = client
            .post(&url)
            .header("x-goog-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&request_body)
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
                    usage.add_to(&mut turn);
                    turn.record_if_billed(db, workload, started_at.elapsed().as_millis() as i64);
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

        usage.add_to(&mut turn);

        if !function_call_parts.is_empty() && !final_round {
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
                let mut args = fc.args.clone();
                if let Some(tool) = tool_defs.iter().find(|t| t.name == fc.name) {
                    restore_json_text(&tool.parameters, &mut args);
                }
                let args_str = serde_json::to_string(&args).unwrap_or_default();
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
                log::info!("JARVIS tool call: {} ({} bytes of arguments)", fc.name, args_str.len());
                let label = tools::tool_status_label(&fc.name);
                let _ = app_handle.emit(
                    "chat-status",
                    json!({
                        "status": format!("Running: {}", label),
                        "detail": tools::tool_status_detail(&fc.name, &args_str),
                        "phase": "acting"
                    }),
                );
                if !narrated {
                    if let Some(ref tx) = tts_tx {
                        let narration = tools::tool_voice_narration(&fc.name, &tools::read_language_pref(db));
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
            if turn.is_empty() {
                log::warn!(
                    "[gemini] no usageMetadata received for non-empty response; recording zero tokens (model={})",
                    model
                );
            }
            turn.record(db, workload, started_at.elapsed().as_millis() as i64);
            return Ok(text_parts.join(""));
        }
        if final_round && !function_call_parts.is_empty() {
            turn.record(db, workload, started_at.elapsed().as_millis() as i64);
            return Ok(tools::tool_limit_fallback(&tools::read_language_pref(db)).into());
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
        turn.record(db, workload, started_at.elapsed().as_millis() as i64);
        return Err(empty_reply_error(model, finish_reason.as_deref()).into());
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    turn.record(db, workload, started_at.elapsed().as_millis() as i64);
    // The last round always returns above.
    Ok(tools::tool_limit_fallback(&tools::read_language_pref(db)).into())
}

/// Map persisted `(role, content)` rows to Gemini contents. Rows whose content
/// is empty or whitespace are dropped: Gemini rejects an empty text part with
/// 400, so one such row would fail every later turn until "New chat".
/// Dropping one leaves the turns on either side with the same role, so each
/// run of one role becomes one content with a text part per row, as
/// `bedrock::merge_history` does for Converse.
fn history_contents(messages: Vec<(String, String)>) -> Vec<GeminiContent> {
    let mut contents: Vec<GeminiContent> = Vec::new();
    for (role, content) in messages {
        if content.trim().is_empty() {
            continue;
        }
        let role = if role == "assistant" { "model".to_string() } else { role };
        let part = GeminiPart { text: Some(content), ..Default::default() };
        match contents.last_mut() {
            Some(last) if last.role == role => last.parts.push(part),
            _ => contents.push(GeminiContent { role, parts: vec![part] }),
        }
    }
    contents
}

/// The output cap for a chat request: the model's own limit from the
/// registry. Thinking counts against it, so a fixed small cap could be spent
/// before any answer. A model the registry does not know keeps Gemini's
/// `default_max_output_tokens`.
fn output_cap(model: &str) -> u32 {
    crate::ai::models::max_output_tokens(model).unwrap_or_else(|| crate::ai::models::limits("gemini").default_max_output_tokens.unwrap_or_default())
}

/// A reply that ended without text or a tool call. The router sends the
/// request once more to the same model on "returned no text"
/// (`failure::code_of`), then shows the error card naming the model.
fn empty_reply_error(model: &str, finish_reason: Option<&str>) -> String {
    format!("Gemini ({model}) returned no text (finish_reason={})", finish_reason.unwrap_or("none"))
}

/// Function declarations take an OpenAPI subset, not JSON Schema: these are
/// the Schema fields the Gemini API documents. Anything else is dropped.
const SCHEMA_FIELDS: &[&str] = &[
    "type", "format", "title", "description", "nullable", "enum", "maxItems", "minItems", "properties", "required",
    "minProperties", "maxProperties", "minLength", "maxLength", "pattern", "example", "anyOf", "propertyOrdering",
    "default", "items", "minimum", "maximum",
];

/// Added to the description of a field Gemini is asked to send as JSON text.
const JSON_TEXT_NOTE: &str = "Give this value as one JSON object written as a string.";

/// An object with no listed properties: JSON Schema's "any object".
fn is_open_object(schema: &serde_json::Map<String, serde_json::Value>) -> bool {
    schema.get("type").and_then(serde_json::Value::as_str) == Some("object")
        && schema.get("properties").and_then(serde_json::Value::as_object).is_none_or(|p| p.is_empty())
}

/// A tool's parameters in a shape Gemini accepts. Gemini refuses an object
/// field with no properties ("should be non-empty for OBJECT type") and has
/// no way to say "any object", so such a field is asked for as JSON text and
/// `restore_json_text` turns it back into the object before the tool runs.
/// The top level is left an object: system_info and list_memories take no
/// arguments and have shipped with empty properties.
pub(crate) fn sanitize_schema(schema: &serde_json::Value) -> serde_json::Value {
    sanitize_node(schema, true)
}

fn sanitize_node(schema: &serde_json::Value, top_level: bool) -> serde_json::Value {
    use serde_json::Value;
    let Some(fields) = schema.as_object() else { return schema.clone() };
    if !top_level && is_open_object(fields) {
        let description = fields.get("description").and_then(Value::as_str).unwrap_or_default();
        return json!({"type": "string", "description": format!("{description} {JSON_TEXT_NOTE}").trim()});
    }
    let mut out = serde_json::Map::new();
    for (key, value) in fields.iter().filter(|(key, _)| SCHEMA_FIELDS.contains(&key.as_str())) {
        let value = match key.as_str() {
            "properties" => Value::Object(value.as_object().map(|p| p.iter().map(|(name, field)| (name.clone(), sanitize_node(field, false))).collect()).unwrap_or_default()),
            "items" => sanitize_node(value, false),
            "anyOf" => Value::Array(value.as_array().map(|options| options.iter().map(|option| sanitize_node(option, false)).collect()).unwrap_or_default()),
            _ => value.clone(),
        };
        out.insert(key.clone(), value);
    }
    // A required name that is not a property is another 400.
    if let Some(Value::Array(required)) = out.get("required") {
        let listed = out.get("properties").and_then(Value::as_object);
        let kept: Vec<Value> = required.iter().filter(|name| name.as_str().is_some_and(|n| listed.is_some_and(|p| p.contains_key(n)))).cloned().collect();
        out.insert("required".into(), Value::Array(kept));
    }
    Value::Object(out)
}

/// Undo `sanitize_schema` on the arguments Gemini sent: a field asked for as
/// JSON text goes back to the object the tool expects. A field Gemini sent
/// as an object anyway, or text that is not a JSON object, is left as it is.
pub(crate) fn restore_json_text(schema: &serde_json::Value, value: &mut serde_json::Value) {
    use serde_json::Value;
    let Some(fields) = schema.as_object() else { return };
    if is_open_object(fields) {
        if let Some(Ok(parsed @ Value::Object(_))) = value.as_str().map(serde_json::from_str::<Value>) {
            *value = parsed;
        }
        return;
    }
    match value {
        Value::Object(args) => {
            if let Some(properties) = fields.get("properties").and_then(Value::as_object) {
                for (name, arg) in args.iter_mut() {
                    if let Some(field) = properties.get(name) {
                        restore_json_text(field, arg);
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(item) = fields.get("items") {
                items.iter_mut().for_each(|value| restore_json_text(item, value));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Think harder" on a thinking model asks for the high thinking level;
    /// other turns, and a model the registry does not mark as thinking,
    /// keep Google's default.
    #[test]
    fn think_harder_asks_for_the_high_thinking_level() {
        use crate::ai::capability::Deeper;
        let deeper = generation_config(100, Some(Deeper { thinking: true, effort: false }));
        assert_eq!(deeper, json!({"maxOutputTokens": 100, "thinkingConfig": {"thinkingLevel": "HIGH"}}));
        assert_eq!(generation_config(100, None), json!({"maxOutputTokens": 100}));
        assert_eq!(generation_config(100, Some(Deeper { thinking: false, effort: true })), json!({"maxOutputTokens": 100}));
    }

    #[test]
    fn stream_chunk_parses_usage_metadata_field_names() {
        let json = r#"{"usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":34,"cachedContentTokenCount":5}}"#;
        let sse: StreamChunk = serde_json::from_str(json).unwrap();
        let u = sse.usage_metadata.unwrap();
        assert_eq!(u.prompt_token_count, Some(12));
        assert_eq!(u.candidates_token_count, Some(34));
        assert_eq!(u.cached_content_token_count, Some(5));
    }

    /// gemini-3.7-flash, 800 thinking + 200 visible tokens: all 1,000 are
    /// billed as output ($3.75/M), not just the 200.
    #[test]
    fn thinking_tokens_count_as_output() {
        let json = r#"{"usageMetadata":{"promptTokenCount":1000,"candidatesTokenCount":200,"thoughtsTokenCount":800}}"#;
        let sse: StreamChunk = serde_json::from_str(json).unwrap();
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let mut turn = crate::ai::cost::TurnUsage::new("gemini", crate::ai::models::GEMINI_FLASH);
        sse.usage_metadata.unwrap().add_to(&mut turn);
        turn.record(&db, crate::ai::Workload::Tools, 1);
        let (output, micro): (i64, i64) = db.conn.lock().unwrap().query_row("SELECT output_tokens, usd_cost_micro FROM inference_metrics", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        // 1,000 x $0.75 + 1,000 x $3.75 per 1M.
        assert_eq!((output, micro), (1_000, 4_500));
    }

    #[test]
    fn stream_chunk_tolerates_missing_usage_metadata() {
        let sse: StreamChunk = serde_json::from_str(r#"{"candidates":[]}"#).unwrap();
        assert!(sse.usage_metadata.is_none());
    }

    /// Dropping an empty assistant row leaves two user turns side by side;
    /// each run of one role is sent as one content, as Bedrock does.
    #[test]
    fn consecutive_same_role_turns_are_merged() {
        let contents = history_contents(vec![
            ("user".to_string(), "what is on today".to_string()),
            ("assistant".to_string(), String::new()),
            ("user".to_string(), "and tomorrow".to_string()),
            ("assistant".to_string(), "checking".to_string()),
            ("assistant".to_string(), "two meetings".to_string()),
            ("user".to_string(), "thanks".to_string()),
        ]);
        let shape: Vec<(&str, Vec<&str>)> = contents
            .iter()
            .map(|content| (content.role.as_str(), content.parts.iter().map(|part| part.text.as_deref().unwrap()).collect()))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("user", vec!["what is on today", "and tomorrow"]),
                ("model", vec!["checking", "two meetings"]),
                ("user", vec!["thanks"]),
            ]
        );
    }

    fn tool_parameters(name: &str) -> serde_json::Value {
        crate::ai::tools::get_tool_definitions().into_iter().find(|tool| tool.name == name).unwrap().parameters
    }

    /// render_chart and render_status take `data` as any object, which
    /// Gemini's schema cannot say ("should be non-empty for OBJECT type"); it
    /// is asked for as JSON text instead.
    #[test]
    fn the_two_open_data_fields_are_asked_for_as_json_text() {
        assert_eq!(
            sanitize_schema(&tool_parameters("render_chart")),
            json!({
                "type": "object",
                "properties": {
                    "chart_type": {"type": "string", "enum": ["line", "pie", "bar"], "description": "Type of chart to render"},
                    "data": {"type": "string", "description": format!("Chart data. For line/bar: {{labels: string[], series: [{{name: string, data: number[]}}]}}. For pie: {{segments: [{{label: string, value: number}}]}} {JSON_TEXT_NOTE}")}
                },
                "required": ["chart_type", "data"]
            })
        );
        assert_eq!(
            sanitize_schema(&tool_parameters("render_status")),
            json!({
                "type": "object",
                "properties": {
                    "status_type": {
                        "type": "string",
                        "enum": ["task_created", "task_completed", "email_synced", "calendar_synced", "cron_created", "action_completed"],
                        "description": "Type of status to display"
                    },
                    "data": {"type": "string", "description": format!("Status data. Fields vary by type: task_created needs name/priority/due, email_synced needs count/folder, etc. {JSON_TEXT_NOTE}")}
                },
                "required": ["status_type", "data"]
            })
        );
    }

    #[test]
    fn keywords_gemini_does_not_take_are_dropped() {
        let schema = json!({
            "type": "object",
            "$schema": "http://json-schema.org/draft-07/schema#",
            "additionalProperties": false,
            "properties": {
                "ids": {"type": "array", "items": {"type": "integer", "const": 3, "minimum": 1}},
                "when": {"type": "string", "format": "date-time", "examples": ["2026-11-12"]}
            },
            "required": ["ids", "missing"]
        });
        assert_eq!(
            sanitize_schema(&schema),
            json!({
                "type": "object",
                "properties": {
                    "ids": {"type": "array", "items": {"type": "integer", "minimum": 1}},
                    "when": {"type": "string", "format": "date-time"}
                },
                "required": ["ids"]
            })
        );
    }

    /// Every declaration sent to Gemini is free of open objects and of
    /// keywords outside its subset.
    #[test]
    fn every_tool_declaration_fits_the_gemini_subset() {
        fn check(schema: &serde_json::Value, top_level: bool, path: &str) {
            let fields = schema.as_object().unwrap();
            for key in fields.keys() {
                assert!(SCHEMA_FIELDS.contains(&key.as_str()), "{path}: {key}");
            }
            if !top_level && fields.get("type").and_then(serde_json::Value::as_str) == Some("object") {
                assert!(fields.get("properties").and_then(serde_json::Value::as_object).is_some_and(|p| !p.is_empty()), "{path} is an open object");
            }
            for (name, field) in fields.get("properties").and_then(serde_json::Value::as_object).into_iter().flatten() {
                check(field, false, &format!("{path}.{name}"));
            }
            if let Some(items) = fields.get("items") {
                check(items, false, &format!("{path}[]"));
            }
        }
        let tools = crate::ai::tools::to_gemini_format(&crate::ai::tools::get_tool_definitions());
        for declaration in tools[0]["functionDeclarations"].as_array().unwrap() {
            check(&declaration["parameters"], true, declaration["name"].as_str().unwrap());
        }
    }

    /// What Gemini sends back as JSON text reaches the tool as the object it
    /// expects; anything else is left for the tool to reject.
    #[test]
    fn json_text_arguments_are_turned_back_into_objects() {
        let schema = tool_parameters("render_chart");
        let mut args = json!({"chart_type": "pie", "data": "{\"segments\": [{\"label\": \"a\", \"value\": 1}]}"});
        restore_json_text(&schema, &mut args);
        assert_eq!(args, json!({"chart_type": "pie", "data": {"segments": [{"label": "a", "value": 1}]}}));

        let mut already = json!({"chart_type": "pie", "data": {"segments": []}});
        restore_json_text(&schema, &mut already);
        assert_eq!(already, json!({"chart_type": "pie", "data": {"segments": []}}));

        let mut broken = json!({"chart_type": "pie", "data": "not json"});
        restore_json_text(&schema, &mut broken);
        assert_eq!(broken["data"], "not json");
    }

    #[test]
    fn the_output_cap_is_the_models_own_limit() {
        assert_eq!(output_cap(crate::ai::models::GEMINI_FLASH), 65_536);
        assert_eq!(output_cap(crate::ai::models::GEMINI_PRO), 65_536);
        assert_eq!(output_cap("gemini-unlisted"), 4_096);
    }

    /// The router sends the request once more to the same model on
    /// "returned no text", and the card names Gemini and the model.
    #[test]
    fn an_empty_reply_names_gemini_and_the_model() {
        let error = empty_reply_error("gemini-3.6-flash", Some("STOP"));
        assert_eq!(error, "Gemini (gemini-3.6-flash) returned no text (finish_reason=STOP)");
        assert_eq!(crate::ai::failure::code_of(&error), crate::ai::failure::Code::EmptyReply);
        assert!(crate::ai::failure::Code::EmptyReply.transient());
        assert_eq!(empty_reply_error("m", None), "Gemini (m) returned no text (finish_reason=none)");
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
}
