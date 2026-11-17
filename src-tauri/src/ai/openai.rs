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
    reasoning_effort: Option<String>,
    /// "none" on the last allowed round, so the model answers from what the
    /// earlier rounds gathered instead of asking for another tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    stream: bool,
    stream_options: StreamOptions,
}

/// Where a chat turn goes: api.openai.com, or an OpenAI-compatible service
/// such as Kimi or Qwen (a chain row with a `base_url`).
#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    /// None is api.openai.com.
    pub base_url: Option<&'a str>,
    /// The chain row's model. On api.openai.com, None follows the registry's default.
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
            None => Ok(self.model.map(str::trim).filter(|m| !m.is_empty()).map(str::to_string).unwrap_or_else(crate::ai::models::openai_default)),
            Some(base) => self
                .model
                .map(str::to_string)
                .filter(|m| !m.trim().is_empty())
                .ok_or_else(|| format!("尚未為 {} 選擇模型，請在設定中選擇。", super::compat::label(base))),
        }
    }

    /// Provider id in the cost table.
    fn provider(&self) -> String {
        super::cost::provider_id("openai", self.base_url)
    }

    fn request(&self, model: String, messages: Vec<OpenAIMessage>, tools: Vec<serde_json::Value>, final_round: bool) -> OpenAIRequest {
        let official = self.base_url.is_none();
        // The registry's limits: OpenAI's own, or the compatible service's.
        let limits = crate::ai::models::limits(crate::ai::models::openai_format_provider(self.base_url));
        let cap = limits.chat_max_output_tokens.unwrap_or_default();
        OpenAIRequest {
            model,
            messages,
            tools,
            max_completion_tokens: official.then_some(cap),
            max_tokens: (!official).then_some(cap),
            reasoning_effort: if official { limits.chat_reasoning_effort } else { None },
            tool_choice: final_round.then_some("none"),
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
    /// Includes the cached tokens below.
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize, Debug, Default, Clone)]
struct PromptTokensDetails {
    cached_tokens: Option<i64>,
}

impl OpenAIUsage {
    /// Fold one request into the turn. OpenAI caches long prompt prefixes on
    /// its own and bills them at the cached rate.
    fn add_to(&self, turn: &mut crate::ai::cost::TurnUsage) {
        let cached = self.prompt_tokens_details.as_ref().and_then(|d| d.cached_tokens).unwrap_or(0);
        turn.add_request(self.prompt_tokens.unwrap_or(0), self.completion_tokens.unwrap_or(0), cached, 0);
    }
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
    let model_id = target.model()?;
    let provider = target.provider();
    let mut turn = crate::ai::cost::TurnUsage::new(&provider, &model_id);
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

    let max_iterations = crate::ai::models::max_tool_rounds(crate::ai::models::openai_format_provider(target.base_url));
    for round in 0..max_iterations {
        let final_round = round + 1 == max_iterations;
        // Per-round usage: OpenAI sends one usage chunk per request, so
        // overwrite within a round and fold into the turn when it ends.
        let mut usage = OpenAIUsage::default();
        let request = target.request(model_id.clone(), openai_messages.clone(), openai_tools.clone(), final_round);

        let sent = send_retrying(|| {
            client
                .post(target.url())
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&request)
        })
        .await;
        let response = match sent {
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
                    usage.add_to(&mut turn);
                    turn.record_if_billed(db, workload, started_at.elapsed().as_millis() as i64);
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

        usage.add_to(&mut turn);

        // Handle tool calls. Some compatible services finish a tool-call
        // turn with "stop", so any complete call counts; a reply cut off by
        // the length limit may carry half-written arguments. The last round
        // runs no tools: a service that ignores tool_choice gets the fallback.
        if !tool_map.is_empty() && finish_reason.as_deref() != Some("length") && !final_round {
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
        if turn.is_empty() && !text_parts.is_empty() {
            log::warn!("[openai] no usage chunk received for non-empty response; recording zero tokens");
        }
        turn.record(db, workload, started_at.elapsed().as_millis() as i64);
        let text = text_parts.join("");
        if final_round && text.trim().is_empty() {
            return Ok(tools::tool_limit_fallback(&tools::read_language_pref(db)).into());
        }
        return Ok(text);
    }

    // The last round always returns above.
    Ok(tools::tool_limit_fallback(&tools::read_language_pref(db)).into())
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

/// Send once more after a transient failure (429 or 5xx): with "Choose one
/// AI" there is no other provider to fall back to, and a resend usually works.
async fn send_retrying(build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, reqwest::Error> {
    let response = build().send().await?;
    let retry_after = response.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok());
    match retry_delay(response.status().as_u16(), retry_after) {
        Some(delay) => {
            log::warn!("[openai] {} -- retrying once in {:?}", response.status(), delay);
            drop(response);
            tokio::time::sleep(delay).await;
            build().send().await
        }
        None => Ok(response),
    }
}

/// The wait before the one retry, or None when the status is not transient or
/// the service asks for a longer wait than the user should sit through.
fn retry_delay(status: u16, retry_after: Option<&str>) -> Option<std::time::Duration> {
    if !matches!(status, 429 | 500 | 502 | 503 | 504) {
        return None;
    }
    match retry_after.map(|v| v.trim().parse::<f64>()) {
        None | Some(Err(_)) => Some(std::time::Duration::from_secs(1)),
        Some(Ok(secs)) if (0.0..=5.0).contains(&secs) => Some(std::time::Duration::from_secs_f64(secs)),
        Some(Ok(_)) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The output cap and reasoning effort are the registry's: OpenAI's own
    /// for api.openai.com, the compatible service's for any other address.
    #[test]
    fn request_limits_follow_the_registry() {
        let _registry = crate::ai::registry::test_override::with(|r| {
            fn limits<'a>(r: &'a mut crate::ai::registry::Registry, id: &str) -> &'a mut crate::ai::registry::doc::Limits {
                &mut r.providers.iter_mut().find(|p| p.id == id).unwrap().limits
            }
            limits(r, "openai").chat_max_output_tokens = Some(1234);
            limits(r, "openai").chat_reasoning_effort = Some("low".into());
            limits(r, "kimi").chat_max_output_tokens = Some(777);
            limits(r, "openai_compat").chat_max_output_tokens = Some(555);
        });
        let wire = |base_url: Option<&str>| {
            let target = Target { base_url, model: Some("m") };
            serde_json::to_value(target.request("m".into(), Vec::new(), Vec::new(), false)).unwrap()
        };
        let official = wire(None);
        assert_eq!((official["max_completion_tokens"].as_u64(), official["reasoning_effort"].as_str()), (Some(1234), Some("low")));
        let kimi = wire(Some("https://api.moonshot.ai/v1"));
        assert_eq!((kimi["max_tokens"].as_u64(), kimi.get("reasoning_effort")), (Some(777), None));
        assert_eq!(wire(Some("https://api.deepseek.com/v1"))["max_tokens"].as_u64(), Some(555));
        let target = Target { base_url: None, model: None };
        assert_eq!(target.model().unwrap(), crate::ai::models::OPENAI_DEFAULT);
    }

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

    /// The last allowed round must get an answer, not another tool call
    /// that nothing will run.
    #[test]
    fn only_the_last_round_turns_tools_off() {
        let official = Target { base_url: None, model: None };
        let compatible = Target { base_url: Some("https://api.moonshot.ai/v1"), model: Some("kimi-k2") };
        for target in [official, compatible] {
            let last = serde_json::to_value(target.request("m".into(), Vec::new(), Vec::new(), true)).unwrap();
            assert_eq!(last["tool_choice"], "none");
            let earlier = serde_json::to_value(target.request("m".into(), Vec::new(), Vec::new(), false)).unwrap();
            assert!(earlier.get("tool_choice").is_none(), "{earlier}");
        }
    }

    #[test]
    fn transient_failures_are_retried_once_after_a_short_wait() {
        use std::time::Duration;
        for status in [429, 500, 502, 503, 504] {
            assert_eq!(retry_delay(status, None), Some(Duration::from_secs(1)), "{status}");
        }
        assert_eq!(retry_delay(429, Some("2")), Some(Duration::from_secs(2)));
        assert_eq!(retry_delay(503, Some("0.5")), Some(Duration::from_millis(500)));
        assert_eq!(retry_delay(429, Some("60")), None, "a long wait fails now instead of hanging the turn");
        assert_eq!(retry_delay(429, Some("Wed, 21 Oct 2026 07:28:00 GMT")), Some(Duration::from_secs(1)));
        for status in [200, 400, 401, 403, 404] {
            assert_eq!(retry_delay(status, None), None, "{status}");
        }
    }

    /// prompt_tokens includes the cached prefix; dropping
    /// prompt_tokens_details billed every cached token at 10x its price.
    #[test]
    fn cached_prompt_tokens_reach_the_turn() {
        let chunk: SseChunk = serde_json::from_str(r#"{"choices":[],"usage":{"prompt_tokens":20173,"completion_tokens":195,"prompt_tokens_details":{"cached_tokens":19824}}}"#).unwrap();
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let mut turn = crate::ai::cost::TurnUsage::new("openai", "gpt-6-luna");
        chunk.usage.unwrap().add_to(&mut turn);
        turn.record(&db, crate::ai::Workload::Tools, 1);
        let (cached, micro): (i64, i64) = db.conn.lock().unwrap().query_row("SELECT cache_read_tokens, usd_cost_micro FROM inference_metrics", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((cached, micro), (19_824, 331));
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
        let request = target.request(target.model().unwrap(), messages, tools, false);
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
