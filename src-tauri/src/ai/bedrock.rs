use super::tools;
use crate::aws::credentials::AwsCredentials;
use crate::voice::tts::TtsCommand;
use aws_config::BehaviorVersion;
use aws_sdk_bedrockruntime::config::Region;
use aws_sdk_bedrockruntime::types::{
    CachePointBlock, CachePointType, ContentBlock, ConversationRole, InferenceConfiguration,
    Message, StopReason, SystemContentBlock, TokenUsage, Tool, ToolConfiguration, ToolInputSchema,
    ToolResultBlock, ToolResultContentBlock, ToolResultStatus, ToolSpecification,
};
use aws_smithy_types::{Document, Number};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use crate::chat_session::ScopedEmitter;

/// The Bedrock model: AWS_BEDROCK_MODEL_ID, else models::BEDROCK_DEFAULT.
pub fn default_model() -> String {
    std::env::var("AWS_BEDROCK_MODEL_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| crate::ai::models::BEDROCK_DEFAULT.to_string())
}

/// Build a Bedrock client from explicit credentials.  We intentionally do not
/// rely on the ambient AWS environment because credentials live in our SQLite +
/// keychain pair, not in the process environment.
async fn client_for(creds: &AwsCredentials) -> aws_sdk_bedrockruntime::Client {
    let region = Region::new(creds.region.clone());
    let provider = aws_credential_types::Credentials::new(
        creds.access_key_id.clone(),
        creds.secret_access_key.clone(),
        None,
        None,
        "jarvis-aws-creds",
    );
    let cfg = aws_config::defaults(BehaviorVersion::latest())
        .region(region)
        .credentials_provider(provider)
        .load()
        .await;
    aws_sdk_bedrockruntime::Client::new(&cfg)
}

/// One-shot helper used by mail analysis and the integration test below.
/// Sends a single user message and returns the assistant's text reply and the
/// usage Bedrock reported -- no tool loop, no caching. This is intentionally
/// minimal so it can serve as a "round trip works" smoke test for a
/// freshly-saved AWS credential set.
pub async fn send_simple(
    creds: &AwsCredentials,
    model_id: &str,
    user_text: &str,
) -> Result<(String, Option<TokenUsage>), String> {
    let client = client_for(creds).await;
    let msg = Message::builder()
        .role(ConversationRole::User)
        .content(ContentBlock::Text(user_text.to_string()))
        .build()
        .map_err(|e| format!("msg build: {e}"))?;
    let out = client
        .converse()
        .model_id(model_id)
        .messages(msg)
        .send()
        .await
        .map_err(|e| format!("converse: {e}"))?;
    let output = out.output.ok_or("no output")?;
    let message = output
        .as_message()
        .map_err(|_| "output is not a message".to_string())?;
    let text = message
        .content()
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    Ok((text, out.usage))
}

/// Appended to the system prompt when a plain completion asks for JSON.
/// Converse has no JSON switch that every Bedrock model accepts (a forced
/// tool choice is refused by models without tool choice), so `complete`
/// asks in words and checks the reply.
pub(crate) const JSON_INSTRUCTION: &str = "Reply with only one JSON object: no code fences and no text before or after it.";

/// One plain Converse request, as `complete` sends it: no tools, no cache
/// points, not streamed.
pub(crate) struct PlainRequest {
    pub system: Vec<SystemContentBlock>,
    pub messages: Vec<Message>,
    pub inference: InferenceConfiguration,
}

pub(crate) fn plain_request(system: &str, user: &str, json: bool, max_tokens: u32) -> Result<PlainRequest, String> {
    let system = match (system.trim().is_empty(), json) {
        (true, false) => None,
        (true, true) => Some(JSON_INSTRUCTION.to_string()),
        (false, false) => Some(system.to_string()),
        (false, true) => Some(format!("{system}\n\n{JSON_INSTRUCTION}")),
    };
    let message = Message::builder()
        .role(ConversationRole::User)
        .content(ContentBlock::Text(user.to_string()))
        .build()
        .map_err(|_| "未能建立 Bedrock 請求。".to_string())?;
    Ok(PlainRequest {
        system: system.map(SystemContentBlock::Text).into_iter().collect(),
        messages: vec![message],
        inference: InferenceConfiguration::builder().max_tokens(max_tokens.min(i32::MAX as u32) as i32).build(),
    })
}

pub(crate) async fn converse_plain(
    creds: &AwsCredentials,
    model_id: &str,
    request: PlainRequest,
) -> Result<aws_sdk_bedrockruntime::operation::converse::ConverseOutput, String> {
    let client = client_for(creds).await;
    client
        .converse()
        .model_id(model_id)
        .set_system((!request.system.is_empty()).then_some(request.system))
        .set_messages(Some(request.messages))
        .inference_config(request.inference)
        .send()
        .await
        .map_err(|err| {
            log::warn!("{}", converse_error(&err));
            plain_error(err.as_service_error().map(|e| aws_sdk_bedrockruntime::error::ProvideErrorMetadata::code(e).unwrap_or_default()))
        })
}

/// What a failed plain request means, in the user's terms. The SDK error is
/// logged, never shown: a validation message can quote the request. `code` is
/// the service's error code, or None when the request never reached AWS.
fn plain_error(code: Option<&str>) -> String {
    match code {
        Some("AccessDeniedException" | "UnrecognizedClientException" | "InvalidSignatureException" | "ExpiredTokenException") => {
            "Bedrock 拒絕了 AWS 憑證或模型存取權限，請在設定中檢查後重試。".into()
        }
        Some("ResourceNotFoundException") => "Bedrock 找不到所選模型，請檢查模型 ID 及區域。".into(),
        Some("ThrottlingException" | "ServiceQuotaExceededException") => "Bedrock 暫時限速或額度不足，請稍後重試。".into(),
        Some("ValidationException") => "Bedrock 不接受這個請求，請檢查所選模型及區域。".into(),
        Some(_) => "Bedrock 暫時未能回覆，請稍後重試。".into(),
        None => "未能連接 AWS Bedrock，請檢查網絡及 AWS 憑證。".into(),
    }
}

/// The reply of a plain request and its usage: (text, input tokens
/// including cache, output tokens, cache-read tokens).
pub(crate) fn plain_result(output: &aws_sdk_bedrockruntime::operation::converse::ConverseOutput) -> (String, i64, i64, i64) {
    let text = match output.output() {
        Some(aws_sdk_bedrockruntime::types::ConverseOutput::Message(message)) => collect_text(message).join(""),
        _ => String::new(),
    };
    match output.usage() {
        Some(usage) => (text, inclusive_input(usage), usage.output_tokens() as i64, usage.cache_read_input_tokens().unwrap_or(0) as i64),
        None => (text, 0, 0, 0),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn send(
    creds: &AwsCredentials,
    model_id: &str,
    workload: crate::ai::Workload,
    messages: Vec<(String, String)>,
    db: &crate::db::Database,
    google_auth: &std::sync::Arc<crate::auth::google::GoogleAuth>,
    app_handle: &tauri::AppHandle,
    tts_tx: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
    ledger: &crate::ai::SideEffectLedger,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = client_for(creds).await;
    let tool_config = build_tool_config()?;

    let mut bedrock_messages = build_messages(messages)?;
    // A7: split system prompt into a byte-stable cacheable prefix and a
    // per-request dynamic suffix.  The CachePoint sits between them so the
    // static prefix is served from cache while the dynamic suffix (user name,
    // date, memory) is sent fresh each call without invalidating the cache.
    let static_prompt = tools::build_cacheable_system_prompt();
    let user_name = crate::util::display_name(db);
    let language = tools::read_language_pref(db);
    let dynamic = tools::build_dynamic_suffix(&user_name, chrono::Local::now(), &language, db);
    let system_blocks = build_system_blocks(static_prompt, dynamic)?;
    let inference_config = InferenceConfiguration::builder()
        .max_tokens(4096)
        .temperature(0.2)
        .build();

    let max_iterations = 5;
    for round in 0..max_iterations {
        let final_round = round + 1 == max_iterations;
        let start = std::time::Instant::now();
        let response = client
            .converse()
            .model_id(model_id)
            .set_messages(Some(bedrock_messages.clone()))
            .set_system(Some(system_blocks.clone()))
            .tool_config(tool_config.clone())
            .inference_config(inference_config.clone())
            .send()
            .await
            .map_err(converse_error)?;
        let latency_ms = start.elapsed().as_millis() as i64;

        if let Some(usage) = response.usage() {
            record_metrics(db, workload, model_id, usage, latency_ms).await;
        }

        let output_message = match response.output() {
            Some(aws_sdk_bedrockruntime::types::ConverseOutput::Message(message)) => {
                message.clone()
            }
            _ => return Err("Bedrock returned no message output".into()),
        };

        let text_parts = collect_text(&output_message);
        let tool_uses = collect_tool_uses(&output_message);

        // The last round runs no tools.
        if (!tool_uses.is_empty() || response.stop_reason() == &StopReason::ToolUse) && !final_round {
            bedrock_messages.push(output_message);

            let mut executed_tools: HashSet<String> = HashSet::new();
            let mut narrated = false;
            let mut result_blocks: Vec<ContentBlock> = Vec::new();

            for tool_use in &tool_uses {
                let name = tool_use.name();
                let args_value = document_to_json(tool_use.input());
                let args_str = serde_json::to_string(&args_value).unwrap_or_default();
                let call_key = crate::ai::tool_call_key(name, &args_str);
                let content = if executed_tools.contains(&call_key) {
                    log::info!("JARVIS skipping duplicate tool call: {}", name);
                    "Skipped: already executed this tool with these arguments in this batch."
                        .to_string()
                } else {
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
                    let _ = app_handle.emit("chat-tool-call", json!({"tool_name": name}));

                    if !narrated {
                        if let Some(ref tx) = tts_tx {
                            let narration = tools::tool_voice_narration(name, &tools::read_language_pref(db));
                            let _ = tx.try_send(TtsCommand::Narrate(narration.to_string()));
                            narrated = true;
                        }
                    }

                    ledger.note(name);
                    let result = tools::execute_tool(name, &args_str, db, google_auth, ledger).await;
                    log::info!("JARVIS tool result: {}", crate::util::truncate_bytes(&result, 200));
                    result
                };

                let tool_result = ToolResultBlock::builder()
                    .tool_use_id(tool_use.tool_use_id())
                    .content(ToolResultContentBlock::Text(content))
                    .status(ToolResultStatus::Success)
                    .build()?;
                result_blocks.push(ContentBlock::ToolResult(tool_result));
            }
            // Converse has no "no tools" choice while the history holds tool
            // calls, so the next, last round is told in words.
            if round + 2 == max_iterations {
                result_blocks.push(ContentBlock::Text(FINAL_ROUND_NOTE.to_string()));
            }

            bedrock_messages.push(
                Message::builder()
                    .role(ConversationRole::User)
                    .set_content(Some(result_blocks))
                    .build()?,
            );

            let _ = app_handle.emit(
                "chat-status",
                json!({
                    "status": "Reviewing tool results...",
                    "phase": "thinking"
                }),
            );
            continue;
        }

        let mut final_text = text_parts.join("");
        if final_round && final_text.trim().is_empty() {
            final_text = tools::tool_limit_fallback(&tools::read_language_pref(db)).to_string();
        }
        let _ = app_handle.emit(
            "chat-status",
            json!({
                "status": "Composing response...",
                "phase": "responding"
            }),
        );
        if !final_text.is_empty() {
            let _ = app_handle.emit("chat-token", json!({"token": final_text, "done": false}));
            if let Some(ref tx) = tts_tx {
                let _ = tx.try_send(TtsCommand::TextChunk(final_text.clone()));
            }
        }
        let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
        return Ok(final_text);
    }

    // The last round always returns above.
    let _ = app_handle.emit("chat-token", json!({"token": "", "done": true}));
    Ok(tools::tool_limit_fallback(&tools::read_language_pref(db)).into())
}

/// Sent with the tool results before the last allowed round.
const FINAL_ROUND_NOTE: &str = "Tool limit reached. Do not call any more tools: answer the user now from the results above, and say briefly what you could not check.";

/// Render a Converse SDK error with the cause chain attached.
///
/// `SdkError`'s own `Display` is the literal "service error" -- the
/// ValidationException text that says which field Bedrock rejected only lives
/// in the source chain, so every model, region or message-shape mistake reached
/// the user as three useless words. `DisplayErrorContext` walks that chain.
fn converse_error<E>(err: E) -> String
where
    E: std::error::Error,
{
    format!(
        "Bedrock converse: {}",
        aws_smithy_types::error::display::DisplayErrorContext(&err)
    )
}

fn build_system_blocks(
    static_prompt: String,
    dynamic_suffix: String,
) -> Result<Vec<SystemContentBlock>, Box<dyn std::error::Error + Send + Sync>> {
    let cache_point = CachePointBlock::builder()
        .r#type(CachePointType::Default)
        .build()
        .map_err(|e| format!("cachepoint build: {e}"))?;
    // Order: [static_text, CachePoint, dynamic_text]
    // The CachePoint marks the end of the cached region -- everything before it
    // is served from cache on subsequent calls.  The dynamic suffix (user name,
    // date, persisted memory) is sent fresh each request without breaking the
    // cache key for the static prefix.
    Ok(vec![
        SystemContentBlock::Text(static_prompt),
        SystemContentBlock::CachePoint(cache_point),
        SystemContentBlock::Text(dynamic_suffix),
    ])
}

/// Collapse a stored `(role, content)` history into the shape Converse accepts.
///
/// Converse rejects two consecutive messages with the same role, and rejects a
/// conversation that opens on the assistant. Both shapes come out of the
/// database routinely: dropping an empty assistant row leaves the user turns on
/// either side adjacent, and a history trimmed mid-exchange starts on the
/// assistant. Sending either produced a ValidationException the user saw as a
/// failed message, so the survivors are merged into one message per run of the
/// same role -- several text blocks in one message is legal -- and a leading
/// assistant run is dropped.
fn merge_history(messages: Vec<(String, String)>) -> Vec<(ConversationRole, Vec<String>)> {
    let mut merged: Vec<(ConversationRole, Vec<String>)> = Vec::new();
    for (role, content) in messages {
        if content.trim().is_empty() {
            continue;
        }
        let role = if role == "assistant" {
            ConversationRole::Assistant
        } else {
            ConversationRole::User
        };
        if merged.is_empty() && role == ConversationRole::Assistant {
            continue;
        }
        match merged.last_mut() {
            Some((last_role, texts)) if *last_role == role => texts.push(content),
            _ => merged.push((role, vec![content])),
        }
    }
    merged
}

fn build_messages(
    messages: Vec<(String, String)>,
) -> Result<Vec<Message>, Box<dyn std::error::Error + Send + Sync>> {
    merge_history(messages)
        .into_iter()
        .map(|(role, texts)| {
            Ok(Message::builder()
                .role(role)
                .set_content(Some(texts.into_iter().map(ContentBlock::Text).collect()))
                .build()?)
        })
        .collect()
}

fn build_tool_config() -> Result<ToolConfiguration, Box<dyn std::error::Error + Send + Sync>> {
    let mut tool_specs = tools::get_tool_definitions()
        .into_iter()
        .map(|tool| {
            let spec = ToolSpecification::builder()
                .name(tool.name)
                .description(tool.description)
                .input_schema(ToolInputSchema::Json(json_to_document(&tool.parameters)))
                .build()?;
            Ok(Tool::ToolSpec(spec))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;

    // Cache the tool specs as well -- they are stable for the lifetime of the
    // process and reusing the cache prefix keeps per-call cost low for tool-
    // heavy workloads (volume + tools tiers).
    let cache_point = CachePointBlock::builder()
        .r#type(CachePointType::Default)
        .build()
        .map_err(|e| format!("tool cachepoint build: {e}"))?;
    tool_specs.push(Tool::CachePoint(cache_point));

    Ok(ToolConfiguration::builder()
        .set_tools(Some(tool_specs))
        .build()?)
}

fn collect_text(message: &Message) -> Vec<String> {
    message
        .content()
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn collect_tool_uses(message: &Message) -> Vec<aws_sdk_bedrockruntime::types::ToolUseBlock> {
    message
        .content()
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse(tool_use) => Some(tool_use.clone()),
            _ => None,
        })
        .collect()
}

async fn record_metrics(
    db: &crate::db::Database,
    workload: crate::ai::Workload,
    model_id: &str,
    usage: &TokenUsage,
    latency_ms: i64,
) {
    let cache_read = usage.cache_read_input_tokens().unwrap_or(0) as i64;
    let cache_write = usage.cache_write_input_tokens().unwrap_or(0) as i64;
    crate::ai::cost::record_inference(
        db,
        "bedrock",
        crate::ai::cost::current_feature(),
        workload,
        model_id,
        inclusive_input(usage),
        usage.output_tokens() as i64,
        cache_read,
        cache_write,
        latency_ms,
    );
}

/// Converse's inputTokens leaves out the tokens read from or written to the
/// cache; the cost table counts input inclusive, so they are added back.
pub fn inclusive_input(usage: &TokenUsage) -> i64 {
    (usage.input_tokens() + usage.cache_read_input_tokens().unwrap_or(0) + usage.cache_write_input_tokens().unwrap_or(0)) as i64
}

fn json_to_document(value: &serde_json::Value) -> Document {
    match value {
        serde_json::Value::Null => Document::Null,
        serde_json::Value::Bool(value) => Document::Bool(*value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_u64() {
                Document::from(value)
            } else if let Some(value) = value.as_i64() {
                Document::from(value)
            } else if let Some(value) = value.as_f64() {
                Document::from(value)
            } else {
                Document::Null
            }
        }
        serde_json::Value::String(value) => Document::String(value.clone()),
        serde_json::Value::Array(values) => {
            Document::Array(values.iter().map(json_to_document).collect())
        }
        serde_json::Value::Object(values) => Document::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), json_to_document(value)))
                .collect::<HashMap<_, _>>(),
        ),
    }
}

fn document_to_json(document: &Document) -> serde_json::Value {
    match document {
        Document::Null => serde_json::Value::Null,
        Document::Bool(value) => serde_json::Value::Bool(*value),
        Document::String(value) => serde_json::Value::String(value.clone()),
        Document::Array(values) => {
            serde_json::Value::Array(values.iter().map(document_to_json).collect())
        }
        Document::Object(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), document_to_json(value)))
                .collect(),
        ),
        Document::Number(Number::PosInt(value)) => serde_json::Value::Number((*value).into()),
        Document::Number(Number::NegInt(value)) => serde_json::Value::Number((*value).into()),
        Document::Number(Number::Float(value)) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sonnet 4.6, a cache hit: inputTokens 2,400, cacheRead 6,400, 250 out
    /// = $0.0072 + $0.00192 + $0.00375. Subtracting the cache from inputTokens
    /// a second time recorded $0.00567.
    #[tokio::test]
    async fn cached_input_is_not_subtracted_twice() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let usage = TokenUsage::builder().input_tokens(2_400).output_tokens(250).total_tokens(9_050).cache_read_input_tokens(6_400).build().unwrap();
        record_metrics(&db, crate::ai::Workload::Tools, "global.anthropic.claude-sonnet-4-6", &usage, 1).await;
        let (input, micro): (i64, i64) = db.conn.lock().unwrap().query_row("SELECT input_tokens, usd_cost_micro FROM inference_metrics", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((input, micro), (8_800, 12_870));
    }

    /// A persisted assistant row with no text must not reach the model: the
    /// Converse API rejects an empty text block, and one such row would fail
    /// every later turn of the conversation.
    #[test]
    fn empty_history_rows_are_dropped() {
        let built = build_messages(vec![
            ("user".to_string(), "hello".to_string()),
            ("assistant".to_string(), String::new()),
            ("user".to_string(), "  \n\t".to_string()),
            ("assistant".to_string(), "hi".to_string()),
        ])
        .unwrap();
        assert_eq!(built.len(), 2);
        assert_eq!(*built[0].role(), ConversationRole::User);
        assert_eq!(built[0].content()[0].as_text().unwrap(), "hello");
        assert_eq!(*built[1].role(), ConversationRole::Assistant);
        assert_eq!(built[1].content()[0].as_text().unwrap(), "hi");
    }

    /// Converse rejects two consecutive turns with the same role. Dropping an
    /// empty assistant row is exactly what leaves two user turns adjacent, so
    /// the survivors have to be merged into one message with two text blocks
    /// rather than sent as-is.
    #[test]
    fn consecutive_same_role_turns_are_merged() {
        let built = build_messages(vec![
            ("user".to_string(), "what is on today".to_string()),
            ("assistant".to_string(), String::new()),
            ("user".to_string(), "and tomorrow".to_string()),
            ("assistant".to_string(), "checking".to_string()),
            ("assistant".to_string(), "two meetings".to_string()),
            ("user".to_string(), "thanks".to_string()),
        ])
        .unwrap();

        assert_eq!(built.len(), 3);
        assert_eq!(*built[0].role(), ConversationRole::User);
        assert_eq!(built[0].content().len(), 2);
        assert_eq!(built[0].content()[0].as_text().unwrap(), "what is on today");
        assert_eq!(built[0].content()[1].as_text().unwrap(), "and tomorrow");
        assert_eq!(*built[1].role(), ConversationRole::Assistant);
        assert_eq!(built[1].content().len(), 2);
        assert_eq!(built[1].content()[0].as_text().unwrap(), "checking");
        assert_eq!(built[1].content()[1].as_text().unwrap(), "two meetings");
        assert_eq!(*built[2].role(), ConversationRole::User);
        assert_eq!(built[2].content().len(), 1);
        assert_eq!(built[2].content()[0].as_text().unwrap(), "thanks");
    }

    /// Converse also rejects a conversation that opens on the assistant, which
    /// is what a history trimmed mid-exchange looks like.
    #[test]
    fn a_leading_assistant_turn_is_dropped() {
        let built = build_messages(vec![
            ("assistant".to_string(), "as I was saying".to_string()),
            ("user".to_string(), "go on".to_string()),
            ("assistant".to_string(), "here it is".to_string()),
        ])
        .unwrap();

        assert_eq!(built.len(), 2);
        assert_eq!(*built[0].role(), ConversationRole::User);
        assert_eq!(built[0].content()[0].as_text().unwrap(), "go on");
        assert_eq!(*built[1].role(), ConversationRole::Assistant);
    }

    /// A history whose only surviving rows are assistant turns leaves nothing
    /// to send, not a message the API will reject.
    #[test]
    fn an_assistant_only_history_merges_to_nothing() {
        let built = build_messages(vec![
            ("assistant".to_string(), "one".to_string()),
            ("assistant".to_string(), "two".to_string()),
        ])
        .unwrap();

        assert!(built.is_empty());
    }

    // Requires AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY in env, region ap-southeast-1
    #[tokio::test]
    #[ignore] // run manually: cargo test --lib ai::bedrock::tests::haiku_singapore_round_trip -- --ignored --nocapture
    async fn haiku_singapore_round_trip() {
        let creds = crate::aws::credentials::AwsCredentials {
            access_key_id: std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default(),
            secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
            region: "ap-southeast-1".to_string(),
            inference_profile: "apac".to_string(),
        };
        if creds.access_key_id.is_empty() {
            eprintln!("Skipping: no AWS_ACCESS_KEY_ID in env");
            return;
        }
        let resp = send_simple(
            &creds,
            "global.anthropic.claude-haiku-4-5-20251001-v1:0",
            "Say only the word: pong",
        )
        .await
        .unwrap()
        .0;
        assert!(resp.to_lowercase().contains("pong"), "got: {resp}");
    }

    /// Verifies cachePoint actually produces cache_read_input_tokens > 0 on a
    /// repeat call within the 5-minute TTL window. This is the runtime proof
    /// that A6 + A7's cache wiring is correct end-to-end.
    #[tokio::test]
    #[ignore] // run manually: cargo test --lib ai::bedrock::tests::cache_hit_smoke -- --ignored --nocapture
    async fn cache_hit_smoke() {
        let creds = crate::aws::credentials::AwsCredentials {
            access_key_id: std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default(),
            secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
            region: "ap-southeast-1".to_string(),
            inference_profile: "apac".to_string(),
        };
        if creds.access_key_id.is_empty() {
            eprintln!("Skipping: no AWS_ACCESS_KEY_ID in env");
            return;
        }
        let client = client_for(&creds).await;
        let model_id = "global.anthropic.claude-haiku-4-5-20251001-v1:0";

        // Anthropic prompt caching needs >= 1024 tokens in the cached prefix.
        // Repeat a 10-word sentence ~150 times -> ~1500 tokens, comfortably above.
        let cacheable = "You are a helpful assistant that answers concisely. ".repeat(150);
        let system = vec![
            SystemContentBlock::Text(cacheable),
            SystemContentBlock::CachePoint(
                CachePointBlock::builder()
                    .r#type(CachePointType::Default)
                    .build()
                    .unwrap(),
            ),
        ];
        let make_msg = || {
            Message::builder()
                .role(ConversationRole::User)
                .content(ContentBlock::Text("Say only: ok".to_string()))
                .build()
                .unwrap()
        };

        // Call 1 - should write cache.
        let r1 = match client
            .converse()
            .model_id(model_id)
            .set_system(Some(system.clone()))
            .messages(make_msg())
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("{e:?}");
                if msg.contains("Model use case details have not been submitted") {
                    eprintln!(
                        "Skipping: account requires Anthropic use case form. \
                         Submit at AWS Console > Bedrock > Model access, wait 15 min."
                    );
                    return;
                }
                panic!("call 1 failed: {msg}");
            }
        };
        let u1 = r1.usage.expect("usage 1");
        let cw1 = u1.cache_write_input_tokens().unwrap_or(0);
        let cr1 = u1.cache_read_input_tokens().unwrap_or(0);
        eprintln!(
            "call 1: input={}, output={}, cache_write={}, cache_read={}",
            u1.input_tokens(),
            u1.output_tokens(),
            cw1,
            cr1
        );

        // Brief pause so the cache settles before call 2.
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;

        // Call 2 - should hit cache.
        let r2 = client
            .converse()
            .model_id(model_id)
            .set_system(Some(system.clone()))
            .messages(make_msg())
            .send()
            .await
            .expect("call 2");
        let u2 = r2.usage.expect("usage 2");
        let cw2 = u2.cache_write_input_tokens().unwrap_or(0);
        let cr2 = u2.cache_read_input_tokens().unwrap_or(0);
        eprintln!(
            "call 2: input={}, output={}, cache_write={}, cache_read={}",
            u2.input_tokens(),
            u2.output_tokens(),
            cw2,
            cr2
        );

        assert!(
            cw1 > 0,
            "expected call 1 to write cache (cache_write_input_tokens > 0), got {cw1}"
        );
        assert!(
            cr2 > 0,
            "expected call 2 to read from cache (cache_read_input_tokens > 0), got {cr2}"
        );
    }
}
