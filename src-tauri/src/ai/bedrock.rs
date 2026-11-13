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
    let loader = aws_config::defaults(BehaviorVersion::latest())
        .region(region)
        .credentials_provider(provider);
    #[cfg(test)]
    let loader = match tests::LOOPBACK.with(|endpoint| endpoint.borrow().clone()) {
        Some(endpoint) => loader.endpoint_url(endpoint),
        None => loader,
    };
    let cfg = loader.load().await;
    aws_sdk_bedrockruntime::Client::new(&cfg)
}

/// The interface language, which the frontend saves as
/// `user_preferences.ui_locale` (the scheduler's task alerts read the same
/// key). Anything but "zh-HK" is English, as there. The Bedrock errors a user
/// can see are written in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum UiLanguage {
    En,
    ZhHk,
}

impl UiLanguage {
    /// Locks the database briefly: never call it while holding the lock.
    pub(crate) fn read(db: &crate::db::Database) -> Self {
        let saved = db.conn.lock().ok().and_then(|conn| {
            conn.query_row("SELECT value FROM user_preferences WHERE key = 'ui_locale'", [], |r| r.get::<_, String>(0)).ok()
        });
        if saved.as_deref() == Some("zh-HK") { UiLanguage::ZhHk } else { UiLanguage::En }
    }

    pub(crate) fn pick(self, en: impl Into<String>, zh: impl Into<String>) -> String {
        match self {
            UiLanguage::En => en.into(),
            UiLanguage::ZhHk => zh.into(),
        }
    }
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
    // AWS bills a reply whether or not it holds a message (the SDK reads a
    // missing or unfamiliar output as Unknown). Such a reply comes back as
    // empty text with its usage, so the caller stores the usage and then
    // rejects the empty answer.
    let text = match out.output.as_ref().and_then(|output| output.as_message().ok()) {
        Some(message) => message
            .content()
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        None => String::new(),
    };
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

pub(crate) fn plain_request(system: &str, user: &str, json: bool, max_tokens: u32, language: UiLanguage) -> Result<PlainRequest, String> {
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
        .map_err(|_| language.pick("Could not build the Bedrock request.", "未能建立 Bedrock 請求。"))?;
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
    language: UiLanguage,
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
            let code = err.as_service_error().map(|e| aws_sdk_bedrockruntime::error::ProvideErrorMetadata::code(e).unwrap_or_default());
            log::warn!("Bedrock completion failed: {}", failure_kind(&err, code));
            plain_error(code, language)
        })
}

/// What is logged when a plain request fails: the service's error code, or
/// the kind of failure when there is none. Never the SDK's text, which can
/// quote the request, and the request is mail.
fn failure_kind<E, R>(err: &aws_sdk_bedrockruntime::error::SdkError<E, R>, code: Option<&str>) -> String {
    use aws_sdk_bedrockruntime::error::SdkError;
    match (err, code) {
        (_, Some(code)) if !code.is_empty() => format!("code {code}"),
        (SdkError::ServiceError(_), _) => "service error without a code".into(),
        (SdkError::TimeoutError(_), _) => "timed out".into(),
        (SdkError::DispatchFailure(_), _) => "could not connect".into(),
        (SdkError::ResponseError(_), _) => "unreadable response".into(),
        (SdkError::ConstructionFailure(_), _) => "request not built".into(),
        _ => "other failure".into(),
    }
}

/// What a failed plain request means, in the user's terms. Only the error
/// code is used, never the SDK's text: a validation message can quote the
/// request. `code` is the service's error code, or None when the request
/// never reached AWS.
fn plain_error(code: Option<&str>, language: UiLanguage) -> String {
    match code {
        Some("AccessDeniedException" | "UnrecognizedClientException" | "InvalidSignatureException" | "ExpiredTokenException") => language.pick(
            "Bedrock refused the AWS credentials or access to the model. Check them in Settings and try again.",
            "Bedrock 拒絕了 AWS 憑證或模型存取權限，請在設定中檢查後重試。",
        ),
        Some("ResourceNotFoundException") => language.pick(
            "Bedrock could not find the chosen model. Check the model ID and the region.",
            "Bedrock 找不到所選模型，請檢查模型 ID 及地區。",
        ),
        Some("ThrottlingException" | "ServiceQuotaExceededException") => language.pick(
            "Bedrock is limiting requests or your quota is used up. Try again later.",
            "Bedrock 暫時限速或額度不足，請稍後重試。",
        ),
        Some("ValidationException") => language.pick(
            "Bedrock did not accept this request. Check the chosen model and the region.",
            "Bedrock 不接受這個請求，請檢查所選模型及地區。",
        ),
        Some(_) => language.pick("Bedrock could not answer just now. Try again later.", "Bedrock 暫時未能回覆，請稍後重試。"),
        None => language.pick(
            "Could not reach AWS Bedrock. Check your network and your AWS credentials.",
            "未能連接 AWS Bedrock，請檢查網絡及 AWS 憑證。",
        ),
    }
}

/// The text of a plain request's reply, and the usage AWS returned with it
/// (None when the reply carried none).
pub(crate) fn plain_result(output: &aws_sdk_bedrockruntime::operation::converse::ConverseOutput) -> (String, Option<StoredUsage>) {
    let text = match output.output() {
        Some(aws_sdk_bedrockruntime::types::ConverseOutput::Message(message)) => collect_text(message).join(""),
        _ => String::new(),
    };
    (text, output.usage().and_then(StoredUsage::from_aws))
}

/// What is stored for one Bedrock request, all taken from the usage AWS
/// returned for that request and nothing estimated. `output`, `cache_read`
/// and `cache_write` are AWS's outputTokens, cacheReadInputTokens and
/// cacheWriteInputTokens as they are. `input` counts every input token of
/// the request, as the usage table does for every provider: AWS's
/// inputTokens leaves the cache out, so `input` is inputTokens + cacheRead +
/// cacheWrite, and AWS's own inputTokens is `input - cache_read -
/// cache_write`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StoredUsage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

impl StoredUsage {
    /// None when the reply carried no usage. The SDK fills a usage block
    /// missing from a reply with zeros, and a request AWS bills always has
    /// input tokens, so all zeros means AWS sent no figures: nothing is
    /// stored rather than an invented row.
    pub(crate) fn from_aws(usage: &TokenUsage) -> Option<Self> {
        let stored = StoredUsage {
            input: inclusive_input(usage),
            output: usage.output_tokens() as i64,
            cache_read: usage.cache_read_input_tokens().unwrap_or(0) as i64,
            cache_write: usage.cache_write_input_tokens().unwrap_or(0) as i64,
        };
        (stored.input != 0 || stored.output != 0 || usage.total_tokens() != 0).then_some(stored)
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

    let turn = ChatTurn {
        client: &client,
        model_id,
        system: &system_blocks,
        tools: &tool_config,
        inference: &inference_config,
        db,
        workload,
    };

    let max_iterations = 5;
    for round in 0..max_iterations {
        let final_round = round + 1 == max_iterations;
        let (response, output_message, tool_uses) = converse_round(&turn, &bedrock_messages, final_round).await?;

        let Some(output_message) = output_message else {
            return Err("Bedrock returned no message output".into());
        };

        let text_parts = collect_text(&output_message);

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

/// What every request of one chat turn shares.
struct ChatTurn<'a> {
    client: &'a aws_sdk_bedrockruntime::Client,
    /// The id the requests are sent with, and the one their rows are stored
    /// against.
    model_id: &'a str,
    system: &'a [SystemContentBlock],
    tools: &'a ToolConfiguration,
    inference: &'a InferenceConfiguration,
    db: &'a crate::db::Database,
    workload: crate::ai::Workload,
}

/// One request of a chat turn, and its usage row: the figures AWS returned
/// for this request, stored against the model id it was sent with. A request
/// that fails stores nothing, and neither does a reply without usage. The
/// tool calls are noted first, so a request asking for a tool that will run
/// is stored as tools.
async fn converse_round(
    turn: &ChatTurn<'_>,
    messages: &[Message],
    final_round: bool,
) -> Result<
    (
        aws_sdk_bedrockruntime::operation::converse::ConverseOutput,
        Option<Message>,
        Vec<aws_sdk_bedrockruntime::types::ToolUseBlock>,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let start = std::time::Instant::now();
    let response = turn
        .client
        .converse()
        .model_id(turn.model_id)
        .set_messages(Some(messages.to_vec()))
        .set_system(Some(turn.system.to_vec()))
        .tool_config(turn.tools.clone())
        .inference_config(turn.inference.clone())
        .send()
        .await
        .map_err(converse_error)?;
    let latency_ms = start.elapsed().as_millis() as i64;

    let output_message = match response.output() {
        Some(aws_sdk_bedrockruntime::types::ConverseOutput::Message(message)) => Some(message.clone()),
        _ => None,
    };
    let tool_uses = output_message.as_ref().map(collect_tool_uses).unwrap_or_default();
    note_tools_to_run(tool_uses.len(), final_round);

    match response.usage() {
        Some(usage) => record_metrics(turn.db, turn.workload, turn.model_id, usage, latency_ms).await,
        None => log::warn!("Bedrock reply carried no usage figures; no usage row stored"),
    }
    Ok((response, output_message, tool_uses))
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

/// Bedrock stores a row per request, and `tools::execute_tool` marks a chat
/// turn as a tool turn only when the tool runs, after this request's row. So
/// a response that asks for a tool which will run marks the turn first: the
/// last round runs no tools, and some requests turn tools off.
fn note_tools_to_run(tool_uses: usize, final_round: bool) {
    let disabled = crate::chat_session::TOOL_EXECUTION_DISABLED.try_with(|v| *v).unwrap_or(false);
    if tool_uses > 0 && !final_round && !disabled {
        crate::ai::cost::note_tool_call();
    }
}

async fn record_metrics(
    db: &crate::db::Database,
    workload: crate::ai::Workload,
    model_id: &str,
    usage: &TokenUsage,
    latency_ms: i64,
) {
    let Some(stored) = StoredUsage::from_aws(usage) else {
        log::warn!("Bedrock reply carried no usage figures; no usage row stored");
        return;
    };
    crate::ai::cost::record_inference(
        db,
        "bedrock",
        crate::ai::cost::current_feature(),
        workload,
        model_id,
        stored.input,
        stored.output,
        stored.cache_read,
        stored.cache_write,
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
pub(crate) mod tests {
    use super::*;

    thread_local! {
        /// Where a test's Bedrock client sends its requests instead of AWS.
        /// Tokio tests run on one thread, so the client built inside the
        /// test reads the value set by `aws_loopback`.
        pub(crate) static LOOPBACK: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    }

    /// The request a loopback server received: its request line and body.
    pub(crate) struct Received {
        pub path: String,
        pub body: String,
    }

    /// A one-request stand-in for the Bedrock endpoint, which this test's
    /// clients are pointed at. `error_type` makes it fail the way AWS does,
    /// with that error code. The model id travels in the path, percent
    /// encoded; `path` is decoded.
    pub(crate) async fn aws_loopback(status: u16, error_type: Option<&str>, body: serde_json::Value) -> tokio::task::JoinHandle<Received> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        LOOPBACK.with(|slot| *slot.borrow_mut() = Some(endpoint));
        let error_header = error_type.map(|code| format!("x-amzn-ErrorType: {code}\r\n")).unwrap_or_default();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            LOOPBACK.with(|slot| *slot.borrow_mut() = None);
            let mut request = Vec::new();
            let mut buffer = [0; 8192];
            let (head_end, length) = loop {
                let count = stream.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end].lines().find_map(|line| line.to_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    if request.len() >= end + 4 + length || count == 0 {
                        break (end, length);
                    }
                }
                assert!(count > 0, "the request ended early");
            };
            let body = body.to_string();
            let reason = if status == 200 { "OK" } else { "Error" };
            let reply = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n{error_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(reply.as_bytes()).await.unwrap();
            let text = String::from_utf8_lossy(&request).to_string();
            let line = text.lines().next().unwrap_or_default().to_string();
            Received {
                path: line.split(' ').nth(1).unwrap_or_default().replace("%3A", ":").replace("%3a", ":"),
                body: text[head_end + 4..(head_end + 4 + length).min(text.len())].to_string(),
            }
        })
    }

    /// A Converse reply as AWS sends it over the wire, with every usage
    /// figure set and different from the others.
    pub(crate) fn converse_json(text: &str) -> serde_json::Value {
        json!({
            "output": {"message": {"role": "assistant", "content": [{"text": text}]}},
            "stopReason": "end_turn",
            "usage": {"inputTokens": 120, "outputTokens": 45, "totalTokens": 1_365, "cacheReadInputTokens": 800, "cacheWriteInputTokens": 400},
            "metrics": {"latencyMs": 7}
        })
    }

    pub(crate) fn fixture_creds(region: &str) -> AwsCredentials {
        AwsCredentials {
            access_key_id: "AKIAFIXTURENOTREAL".into(),
            secret_access_key: "fixture-secret-not-real".into(),
            region: region.into(),
            inference_profile: crate::aws::credentials::inference_profile_for_region(region).into(),
        }
    }

    /// One chat request, sent to a stand-in for AWS: the row holds the four
    /// figures AWS returned, against the model id that went out in the
    /// request.
    #[tokio::test]
    async fn a_chat_request_stores_the_usage_aws_returned_for_the_model_it_sent() {
        let server = aws_loopback(200, None, converse_json("Done.")).await;
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let client = client_for(&fixture_creds("ap-southeast-1")).await;
        let tools = build_tool_config().unwrap();
        let system = build_system_blocks("rules".into(), "today".into()).unwrap();
        let inference = InferenceConfiguration::builder().max_tokens(64).build();
        let turn = ChatTurn { client: &client, model_id: "apac.anthropic.claude-haiku-4-5-20251001-v1:0", system: &system, tools: &tools, inference: &inference, db: &db, workload: crate::ai::Workload::Tools };
        let messages = build_messages(vec![("user".into(), "hi".into())]).unwrap();
        let (_, message, tool_uses) = crate::ai::cost::with_feature(crate::ai::cost::Feature::Chat, converse_round(&turn, &messages, false)).await.unwrap();
        assert!(message.is_some() && tool_uses.is_empty());

        let received = server.await.unwrap();
        assert_eq!(received.path, "/model/apac.anthropic.claude-haiku-4-5-20251001-v1:0/converse");
        let rows = crate::ai::cost::tests::usage_rows(&db);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model_id, "apac.anthropic.claude-haiku-4-5-20251001-v1:0", "stored against the id that was sent");
        assert_eq!((rows[0].provider.as_str(), rows[0].feature.as_str()), ("bedrock", "chat"));
        // inputTokens 120 + cacheRead 800 + cacheWrite 400; the rest as AWS sent them.
        assert_eq!((rows[0].input_tokens, rows[0].output_tokens), (1_320, 45));
        assert_eq!(crate::ai::cost::tests::cache_tokens(&db), vec![(800, 400)]);
    }

    /// The order inside `converse_round`: a reply asking for a tool that will
    /// run marks the turn before its row is stored, so that row is stored as
    /// tools. Storing first would leave it as chat.
    #[tokio::test]
    async fn a_chat_request_whose_reply_asks_for_a_tool_is_stored_as_tools() {
        let mut reply = converse_json("");
        reply["output"]["message"]["content"] = json!([{"toolUse": {"toolUseId": "tooluse_fixture_1", "name": "list_calendar_events", "input": {}}}]);
        reply["stopReason"] = json!("tool_use");
        let server = aws_loopback(200, None, reply).await;
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let client = client_for(&fixture_creds("ap-southeast-1")).await;
        let tools = build_tool_config().unwrap();
        let system = build_system_blocks("rules".into(), "today".into()).unwrap();
        let inference = InferenceConfiguration::builder().max_tokens(64).build();
        let turn = ChatTurn { client: &client, model_id: "apac.anthropic.claude-haiku-4-5-20251001-v1:0", system: &system, tools: &tools, inference: &inference, db: &db, workload: crate::ai::Workload::Tools };
        let messages = build_messages(vec![("user".into(), "what is on today?".into())]).unwrap();
        let (_, _, tool_uses) = crate::ai::cost::with_feature(crate::ai::cost::Feature::Chat, converse_round(&turn, &messages, false)).await.unwrap();
        server.await.unwrap();
        assert_eq!(tool_uses.len(), 1, "the stand-in's reply asks for one tool");
        assert_eq!(features(&db), vec!["tools"]);
        assert_eq!(crate::ai::cost::tests::usage_rows(&db)[0].input_tokens, 1_320, "the row holds this reply's usage");
    }

    /// A chat request AWS refuses stores nothing: there is no usage to store.
    #[tokio::test]
    async fn a_failed_chat_request_stores_nothing() {
        let server = aws_loopback(400, Some("ValidationException"), json!({"message": "PRIVATE-MAIL-CANARY"})).await;
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let client = client_for(&fixture_creds("ap-southeast-1")).await;
        let tools = build_tool_config().unwrap();
        let system = build_system_blocks("rules".into(), "today".into()).unwrap();
        let inference = InferenceConfiguration::builder().max_tokens(64).build();
        let turn = ChatTurn { client: &client, model_id: "global.anthropic.claude-sonnet-4-6", system: &system, tools: &tools, inference: &inference, db: &db, workload: crate::ai::Workload::Tools };
        let messages = build_messages(vec![("user".into(), "hi".into())]).unwrap();
        assert!(converse_round(&turn, &messages, false).await.is_err());
        server.await.unwrap();
        assert!(crate::ai::cost::tests::usage_rows(&db).is_empty());
    }

    /// A reply without usage figures stores nothing rather than a row of
    /// zeros.
    #[tokio::test]
    async fn a_chat_reply_without_usage_stores_nothing() {
        let mut reply = converse_json("Done.");
        reply.as_object_mut().unwrap().remove("usage");
        let server = aws_loopback(200, None, reply).await;
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let client = client_for(&fixture_creds("us-east-1")).await;
        let tools = build_tool_config().unwrap();
        let system = build_system_blocks("rules".into(), "today".into()).unwrap();
        let inference = InferenceConfiguration::builder().max_tokens(64).build();
        let turn = ChatTurn { client: &client, model_id: "us.amazon.nova-pro-v1:0", system: &system, tools: &tools, inference: &inference, db: &db, workload: crate::ai::Workload::Tools };
        let messages = build_messages(vec![("user".into(), "hi".into())]).unwrap();
        // Whether the SDK accepts a reply without usage or calls it
        // malformed, no row is stored for it.
        let _ = converse_round(&turn, &messages, false).await;
        server.await.unwrap();
        assert!(crate::ai::cost::tests::usage_rows(&db).is_empty());
    }

    /// The four stored figures are AWS's, and AWS's own inputTokens can be
    /// read back from them.
    #[test]
    fn stored_usage_is_the_figures_aws_returned() {
        let usage = TokenUsage::builder().input_tokens(120).output_tokens(45).total_tokens(1_365).cache_read_input_tokens(800).cache_write_input_tokens(400).build().unwrap();
        let stored = StoredUsage::from_aws(&usage).unwrap();
        assert_eq!(stored, StoredUsage { input: 1_320, output: 45, cache_read: 800, cache_write: 400 });
        assert_eq!(stored.input - stored.cache_read - stored.cache_write, 120);
        let plain = TokenUsage::builder().input_tokens(9).output_tokens(2).total_tokens(11).build().unwrap();
        assert_eq!(StoredUsage::from_aws(&plain), Some(StoredUsage { input: 9, output: 2, cache_read: 0, cache_write: 0 }));
        // What the SDK hands back when the reply had no usage block.
        let filled = TokenUsage::builder().input_tokens(0).output_tokens(0).total_tokens(0).build().unwrap();
        assert_eq!(StoredUsage::from_aws(&filled), None);
    }

    /// Every Bedrock error a user can see is in the interface language.
    #[test]
    fn bedrock_errors_follow_the_interface_language() {
        let codes = [Some("AccessDeniedException"), Some("ResourceNotFoundException"), Some("ThrottlingException"), Some("ValidationException"), Some("InternalServerException"), None];
        for code in codes {
            let english = plain_error(code, UiLanguage::En);
            let chinese = plain_error(code, UiLanguage::ZhHk);
            assert!(english.is_ascii(), "{code:?}: {english}");
            assert!(!chinese.is_ascii(), "{code:?}: {chinese}");
        }
        assert_eq!(plain_error(Some("ValidationException"), UiLanguage::En), "Bedrock did not accept this request. Check the chosen model and the region.");
        assert_eq!(plain_error(Some("ValidationException"), UiLanguage::ZhHk), "Bedrock 不接受這個請求，請檢查所選模型及地區。");
        // Settings calls the AWS Region 地區; the messages use the same word.
        for code in codes {
            assert!(!plain_error(code, UiLanguage::ZhHk).contains("區域"), "{code:?}");
        }
    }

    /// The language is the one the frontend saved; anything else is English.
    #[test]
    fn the_interface_language_is_read_from_ui_locale() {
        let db = crate::db::test_database();
        assert_eq!(UiLanguage::read(&db), UiLanguage::En, "nothing saved");
        let save = |value: &str| db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('ui_locale', ?1)", [value]).unwrap();
        save("zh-HK");
        assert_eq!(UiLanguage::read(&db), UiLanguage::ZhHk);
        save("en");
        assert_eq!(UiLanguage::read(&db), UiLanguage::En);
        // The assistant's reply language is a different preference.
        db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('language', 'yue')", []).unwrap();
        assert_eq!(UiLanguage::read(&db), UiLanguage::En);
    }

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

    /// The log line for a failed completion names the code or the kind of
    /// failure, never the SDK's text, which can quote the mail.
    #[test]
    fn a_failed_completion_logs_the_code_not_the_message() {
        use aws_sdk_bedrockruntime::error::SdkError;
        let timeout = SdkError::<(), ()>::timeout_error("PRIVATE-MAIL-CANARY");
        assert_eq!(failure_kind(&timeout, None), "timed out");
        assert_eq!(failure_kind(&SdkError::<(), ()>::construction_failure("PRIVATE-MAIL-CANARY"), None), "request not built");
        assert_eq!(failure_kind(&timeout, Some("ValidationException")), "code ValidationException");
        assert!(!failure_kind(&timeout, Some("")).contains("PRIVATE"));
    }

    fn usage() -> TokenUsage {
        TokenUsage::builder().input_tokens(2_400).output_tokens(250).total_tokens(2_650).build().unwrap()
    }

    fn features(db: &crate::db::Database) -> Vec<String> {
        crate::ai::cost::tests::usage_rows(db).into_iter().map(|row| row.feature).collect()
    }

    /// Each Bedrock request is stored under the feature of the request that
    /// started the chain.
    #[tokio::test]
    async fn bedrock_rows_carry_the_feature_of_their_request() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        crate::ai::cost::with_feature(crate::ai::cost::Feature::Briefing, record_metrics(&db, crate::ai::Workload::Tools, "us.amazon.nova-pro-v1:0", &usage(), 1)).await;
        crate::ai::cost::with_feature(crate::ai::cost::Feature::Chat, record_metrics(&db, crate::ai::Workload::Tools, "us.amazon.nova-pro-v1:0", &usage(), 1)).await;
        assert_eq!(features(&db), vec!["briefing", "chat"]);
        let rows = crate::ai::cost::tests::usage_rows(&db);
        assert_eq!((rows[0].provider.as_str(), rows[0].input_tokens), ("bedrock", 2_400));
    }

    /// A chat turn that runs a tool takes three requests: the one asking for
    /// the tool carries the prompt, the tool list and the history, so it is
    /// stored as tools with the rest, not as chat.
    #[tokio::test]
    async fn every_request_of_a_bedrock_tool_turn_is_stored_as_tools() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        crate::ai::cost::with_feature(crate::ai::cost::Feature::Chat, async {
            for tool_uses in [1, 1, 0] {
                note_tools_to_run(tool_uses, false);
                record_metrics(&db, crate::ai::Workload::Tools, "us.amazon.nova-pro-v1:0", &usage(), 1).await;
            }
        })
        .await;
        assert_eq!(features(&db), vec!["tools", "tools", "tools"]);
    }

    /// No tool runs on the last round, or when the request turns tools off,
    /// so those turns stay chat.
    #[tokio::test]
    async fn a_tool_request_that_runs_nothing_stays_chat() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        crate::ai::cost::with_feature(crate::ai::cost::Feature::Chat, async {
            note_tools_to_run(1, true);
            record_metrics(&db, crate::ai::Workload::Tools, "m", &usage(), 1).await;
        })
        .await;
        crate::ai::cost::with_feature(
            crate::ai::cost::Feature::Chat,
            crate::chat_session::TOOL_EXECUTION_DISABLED.scope(true, async {
                note_tools_to_run(2, false);
                record_metrics(&db, crate::ai::Workload::Tools, "m", &usage(), 1).await;
            }),
        )
        .await;
        assert_eq!(features(&db), vec!["chat", "chat"]);
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
