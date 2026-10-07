use super::tools;
use crate::voice::tts::TtsCommand;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use crate::chat_session::ScopedEmitter;

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    tools: Vec<serde_json::Value>,
    /// api.openai.com only; compatible services take `max_tokens`.
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// gpt-6-luna rejects function tools on Chat Completions unless reasoning
    /// is off ("Function tools with reasoning_effort are not supported"),
    /// verified live 2026-09-25. Not sent to compatible services.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'static str>,
    stream: bool,
    stream_options: StreamOptions,
}

/// Where a chat turn goes: api.openai.com, or an OpenAI-compatible service
/// such as Kimi or Qwen (a chain row with a `base_url`).
#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    /// None is api.openai.com.
    pub base_url: Option<&'a str>,
    /// The chain row's model. On api.openai.com, None follows models::OPENAI_DEFAULT.
    pub model: Option<&'a str>,
}

impl Target<'_> {
    fn url(&self) -> String {
        match self.base_url {
            Some(base) => format!("{base}/chat/completions"),
            None => "https://api.openai.com/v1/chat/completions".into(),
        }
    }

    fn model(&self) -> Result<String, String> {
        match self.base_url {
            None => Ok(self.model.map(str::trim).filter(|m| !m.is_empty()).unwrap_or(crate::ai::models::OPENAI_DEFAULT).to_string()),
            Some(base) => self
                .model
                .map(str::to_string)
                .filter(|m| !m.trim().is_empty())
                .ok_or_else(|| format!("尚未為 {} 選擇模型，請在設定中選擇。", super::compat::label(base))),
        }
    }

    /// Provider name in the cost table.
    fn provider(&self) -> String {
        self.base_url.map(|b| super::compat::label(b).to_lowercase()).unwrap_or_else(|| "openai".into())
    }

    fn request(&self, model: String, messages: Vec<OpenAIMessage>, tools: Vec<serde_json::Value>) -> OpenAIRequest {
        let official = self.base_url.is_none();
        OpenAIRequest {
            model,
            messages,
            tools,
            max_completion_tokens: official.then_some(4096),
            max_tokens: (!official).then_some(4096),
            reasoning_effort: official.then_some("none"),
            stream: true,
            stream_options: StreamOptions { include_usage: true },
        }
    }
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
    /// Provider data that must go back with the call, e.g. Gemini's
    /// `thought_signature` on its OpenAI-compatible endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra_content: Option<serde_json::Value>,
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
    /// Some compatible services (Gemini's) leave it out; the position in
    /// the list stands in.
    #[serde(default)]
    index: Option<usize>,
    id: Option<String>,
    function: Option<SseFunctionDelta>,
    #[serde(default)]
    extra_content: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct SseFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

/// The OpenAI model used by chat and the mail workspace.

pub async fn send(
    api_key: &str,
    target: Target<'_>,
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
    let model_id = target.model()?;
    let provider = target.provider();
    let client = Client::builder()
        .read_timeout(std::time::Duration::from_secs(60))
        // A redirect would carry the key and the conversation elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let tool_defs = tools::get_tool_definitions();
    let openai_tools = tools::to_openai_format(&tool_defs);

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".into(),
        content: Some(tools::build_system_prompt(db)),
        tool_calls: None,
        tool_call_id: None,
    }];
    openai_messages.extend(
        messages
            .into_iter()
            .filter(|(_, content)| !content.trim().is_empty())
            .map(|(role, content)| OpenAIMessage {
                role,
                content: Some(content),
                tool_calls: None,
                tool_call_id: None,
            }),
    );

    let max_iterations = 5;
    for _ in 0..max_iterations {
        // Per-round usage: OpenAI sends one usage chunk per request, so
        // overwrite within a round and fold into totals when it ends.
        let mut usage = OpenAIUsage::default();
        let request = target.request(model_id.clone(), openai_messages.clone(), openai_tools.clone());

        let response = client
            .post(target.url())
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            record_partial_usage(
                db, &provider, workload, &model_id,
                total_prompt_tokens, total_completion_tokens,
                &started_at,
            );
            let name = target.base_url.map(super::compat::label).unwrap_or_else(|| "OpenAI".into());
            return Err(format!("{name} API error {}: {}", status, body).into());
        }

        let mut stream = response.bytes_stream();
        let mut buffer = super::SseByteBuffer::new();
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_map: ToolSlots = HashMap::new(); // index -> (call_id, fn_name, arguments, extra_content)
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
                        db, &provider, workload, &model_id,
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
                            for (position, tc) in tool_calls.iter().enumerate() {
                                let index = tool_slot(&tool_map, tc.index, tc.id.as_deref(), position);
                                let entry = tool_map.entry(index).or_insert_with(|| {
                                    (String::new(), String::new(), String::new(), None)
                                });
                                if let Some(ref id) = tc.id {
                                    entry.0 = id.clone();
                                }
                                if tc.extra_content.is_some() {
                                    entry.3 = tc.extra_content.clone();
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
                                        if announced_tools.insert(index) {
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

        // Handle tool calls. Some compatible services finish a tool-call
        // turn with "stop", so any complete call counts; a reply cut off by
        // the length limit may carry half-written arguments.
        if !tool_map.is_empty() && finish_reason.as_deref() != Some("length") {
            let text_content = text_parts.join("");
            let mut tool_calls_vec: Vec<ToolCall> = Vec::new();
            let mut sorted_indices: Vec<usize> = tool_map.keys().copied().collect();
            sorted_indices.sort();
            for idx in &sorted_indices {
                let (ref call_id, ref fn_name, ref args, ref extra) = tool_map[idx];
                tool_calls_vec.push(ToolCall {
                    id: call_id.clone(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: fn_name.clone(),
                        arguments: args.clone(),
                    },
                    extra_content: extra.clone(),
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
                    "JARVIS tool call: {} ({} bytes of arguments)",
                    tc.function.name,
                    tc.function.arguments.len()
                );
                let label = tools::tool_status_label(&tc.function.name);
                let _ = app_handle.emit(
                    "chat-status",
                    json!({
                        "status": format!("Running: {}", label),
                        "detail": tools::tool_status_detail(&tc.function.name, &tc.function.arguments),
                        "phase": "acting"
                    }),
                );
                if !narrated {
                    if let Some(ref tx) = tts_tx {
                        // Always narrate -- this also flushes any buffered AI text
                        let narration = tools::tool_voice_narration(&tc.function.name, &tools::read_language_pref(db));
                        let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                        narrated = true;
                    }
                }
                ledger.note(&tc.function.name);
                let result =
                    tools::execute_tool(&tc.function.name, &tc.function.arguments, db, google_auth, ledger)
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
            db, &provider, workload, &model_id,
            total_prompt_tokens,
            total_completion_tokens,
            0, 0,
            started_at.elapsed().as_millis() as i64,
        );
        return Ok(text_parts.join(""));
    }

    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    crate::ai::cost::record_inference(
        db, &provider, workload, &model_id,
        total_prompt_tokens,
        total_completion_tokens,
        0, 0,
        started_at.elapsed().as_millis() as i64,
    );
    Ok("I've completed the actions.".into())
}

type ToolSlots = HashMap<usize, (String, String, String, Option<serde_json::Value>)>;

/// Which accumulating call a streamed tool-call piece belongs to. OpenAI
/// numbers them with `index`. Gemini's compatible endpoint sends each call
/// whole, without an index, one per chunk: a new `id` is a new call.
fn tool_slot(slots: &ToolSlots, index: Option<usize>, id: Option<&str>, position: usize) -> usize {
    if let Some(index) = index {
        return index;
    }
    match id {
        Some(id) => slots
            .iter()
            .find(|(_, slot)| slot.0 == id)
            .map(|(existing, _)| *existing)
            .unwrap_or_else(|| slots.keys().max().map_or(0, |max| max + 1)),
        None => position,
    }
}

/// Record usage accumulated so far when a turn ends abnormally, so tokens
/// already billed by completed rounds are not lost from telemetry.
fn record_partial_usage(
    db: &crate::db::Database,
    provider: &str,
    workload: crate::ai::Workload,
    model_id: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
    started_at: &std::time::Instant,
) {
    if prompt_tokens > 0 || completion_tokens > 0 {
        crate::ai::cost::record_inference(
            db, provider, workload, model_id,
            prompt_tokens, completion_tokens, 0, 0,
            started_at.elapsed().as_millis() as i64,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_without_an_index_are_kept_apart_by_id() {
        let mut slots: ToolSlots = HashMap::new();
        // OpenAI: the index decides, and argument pieces share a slot.
        assert_eq!(tool_slot(&slots, Some(2), None, 0), 2);
        // Gemini: two whole calls in two chunks, both at position 0.
        let first = tool_slot(&slots, None, Some("call_a"), 0);
        slots.insert(first, ("call_a".into(), "list_tasks".into(), "{}".into(), None));
        let second = tool_slot(&slots, None, Some("call_b"), 0);
        assert_ne!(first, second, "a second call must not append to the first");
        slots.insert(second, ("call_b".into(), "list_follow_ups".into(), "{}".into(), None));
        assert_eq!(tool_slot(&slots, None, Some("call_a"), 0), first, "the same id is the same call");
    }

    #[test]
    fn a_tool_call_without_an_index_still_parses() {
        // Gemini's OpenAI-compatible stream, 2026-09-26: no `index` on the
        // call, a thought signature to send back, finish_reason "stop".
        let data = r#"{"choices":[{"delta":{"role":"assistant","tool_calls":[{"extra_content":{"google":{"thought_signature":"sig"}},"function":{"arguments":"{}","name":"list_tasks"},"id":"call_1","type":"function"}]},"index":0}],"usage":{"completion_tokens":10,"prompt_tokens":31}}"#;
        let chunk: SseChunk = serde_json::from_str(data).unwrap();
        let call = &chunk.choices[0].delta.as_ref().unwrap().tool_calls.as_ref().unwrap()[0];
        assert_eq!((call.index, call.id.as_deref()), (None, Some("call_1")));
        assert_eq!(call.extra_content.as_ref().unwrap()["google"]["thought_signature"], "sig");
        let echoed = serde_json::to_value(ToolCall { id: "call_1".into(), call_type: "function".into(), function: FunctionCall { name: "list_tasks".into(), arguments: "{}".into() }, extra_content: call.extra_content.clone() }).unwrap();
        assert_eq!(echoed["extra_content"]["google"]["thought_signature"], "sig");
        let plain = serde_json::to_value(ToolCall { id: "c".into(), call_type: "function".into(), function: FunctionCall { name: "x".into(), arguments: "{}".into() }, extra_content: None }).unwrap();
        assert!(plain.get("extra_content").is_none(), "OpenAI itself never sees the field");
    }

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

    /// The exact chat request a compatible service (Kimi, Qwen) receives, with
    /// every JARVIS tool, sent to Gemini's OpenAI-compatible endpoint: it
    /// must accept the request and answer with a tool call.
    /// GEMINI_API_KEY=... cargo test live_compatible_service_takes_the_tool_request -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_compatible_service_takes_the_tool_request() {
        let key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY");
        let target = Target { base_url: Some(crate::ai::complete::tests::GEMINI_COMPAT), model: Some(crate::ai::models::GEMINI_FLASH) };
        let messages = vec![
            OpenAIMessage { role: "system".into(), content: Some("You are JARVIS. Use tools to answer.".into()), tool_calls: None, tool_call_id: None },
            OpenAIMessage { role: "user".into(), content: Some("What is on my calendar today? Use the calendar tool.".into()), tool_calls: None, tool_call_id: None },
        ];
        let tools = tools::to_openai_format(&tools::get_tool_definitions());
        let request = target.request(target.model().unwrap(), messages, tools);
        let wire = serde_json::to_value(&request).unwrap();
        assert!(wire.get("reasoning_effort").is_none() && wire.get("max_completion_tokens").is_none() && wire["max_tokens"] == 4096);
        let response = Client::new().post(target.url()).bearer_auth(&key).json(&request).send().await.unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert!(status.is_success(), "{status}: {body}");
        let calls: Vec<String> = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
            .filter_map(|chunk| chunk.pointer("/choices/0/delta/tool_calls/0/function/name").and_then(|n| n.as_str()).map(str::to_string))
            .collect();
        println!("tool calls: {calls:?}; usage chunk: {}", body.contains("\"usage\""));
        assert!(calls.iter().any(|c| c == "list_events"), "{body}");
    }
}
