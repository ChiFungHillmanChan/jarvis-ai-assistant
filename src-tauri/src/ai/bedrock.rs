use super::tools;
use crate::aws::credentials::AwsCredentials;
use aws_config::BehaviorVersion;
use aws_sdk_bedrockruntime::config::Region;
use aws_sdk_bedrockruntime::types::{
    CachePointBlock, CachePointType, ContentBlock, ConversationRole, InferenceConfiguration,
    Message, SystemContentBlock, TokenUsage, Tool, ToolConfiguration, ToolInputSchema,
    ToolResultBlock, ToolResultContentBlock, ToolResultStatus, ToolSpecification,
};
use aws_smithy_types::Document;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use super::provider::{Call, ChatRequest, Event, Events, Metering, Outcome, Round, RoundError, Wire, SKIPPED};
use super::complete::Completion;

/// The Bedrock model: the registry's default. A debug build (development
/// and the native QA app) may override it with AWS_BEDROCK_MODEL_ID; a
/// release build reads only the registry.
pub fn default_model() -> String {
    let dev_override = if cfg!(debug_assertions) { std::env::var("AWS_BEDROCK_MODEL_ID").ok() } else { None };
    dev_override.filter(|value| !value.trim().is_empty()).unwrap_or_else(crate::ai::models::bedrock_default)
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

    fn figures(self) -> super::provider::Usage {
        super::provider::Usage { input: self.input, output: self.output, cache_read: self.cache_read, cache_write: self.cache_write }
    }
}

/// Bedrock's ConverseStream API on the provider interface, with the user's
/// own AWS credentials.
pub(crate) struct BedrockWire {
    creds: AwsCredentials,
    /// The id the requests are sent with, and the one their rows are stored
    /// against.
    model: String,
}

impl BedrockWire {
    pub(crate) fn new(creds: AwsCredentials, model: &str) -> Self {
        BedrockWire { creds, model: model.to_string() }
    }
}

/// One block of a streamed reply.
enum StreamedBlock {
    Text(String),
    Call { id: String, name: String, input: String },
}

/// One chat turn's requests to Bedrock.
pub(crate) struct Conversation {
    client: aws_sdk_bedrockruntime::Client,
    messages: Vec<Message>,
    system: Vec<SystemContentBlock>,
    tools: ToolConfiguration,
    inference: InferenceConfiguration,
    /// The reply of the round in progress, sent back with the results of
    /// its calls.
    reply: Option<Message>,
}

#[async_trait::async_trait]
impl Wire for BedrockWire {
    type Conversation = Conversation;

    fn max_rounds(&self) -> usize {
        crate::ai::models::limits("bedrock").max_tool_rounds.unwrap_or_default() as usize
    }

    /// A row per request: the figures AWS returned for that request, stored
    /// against the model id it was sent with. A reply asking for a tool that
    /// will run is stored as tools.
    fn metering(&self) -> Metering {
        Metering::PerRequest { provider: "bedrock".into(), model: self.model.clone() }
    }

    async fn start(&self, request: ChatRequest) -> Result<Conversation, String> {
        let client = client_for(&self.creds).await;
        let tools = build_tool_config(&request.tools).map_err(|e| e.to_string())?;
        let messages = build_messages(request.messages).map_err(|e| e.to_string())?;
        // A7: the system prompt is split into a byte-stable cacheable prefix
        // and a per-request suffix, with the cache point between them.
        let system = build_system_blocks(request.system.cacheable, request.system.dynamic).map_err(|e| e.to_string())?;
        let limits = crate::ai::models::limits("bedrock");
        let inference = InferenceConfiguration::builder()
            .max_tokens(limits.chat_max_output_tokens.unwrap_or_default() as i32)
            .temperature(limits.chat_temperature.unwrap_or_default())
            .build();
        Ok(Conversation { client, messages, system, tools, inference, reply: None })
    }

    /// One ConverseStream request: the text is shown and spoken as it
    /// comes, each call announced when it starts.
    async fn round(&self, conversation: &mut Conversation, _final_round: bool, events: &mut Events<'_>) -> Result<(), RoundError> {
        use aws_sdk_bedrockruntime::types::{ContentBlockDelta, ContentBlockStart, ConverseStreamOutput as StreamEvent};
        let mut output = conversation
            .client
            .converse_stream()
            .model_id(&self.model)
            .set_messages(Some(conversation.messages.clone()))
            .set_system(Some(conversation.system.clone()))
            .tool_config(conversation.tools.clone())
            .inference_config(conversation.inference.clone())
            .send()
            .await
            .map_err(|e| RoundError::refused(converse_error(e)))?;
        // The reply's blocks by index: text, or a call and its input so far.
        let mut blocks: BTreeMap<i32, StreamedBlock> = BTreeMap::new();
        let mut stop: Option<String> = None;
        let mut usage: Option<StoredUsage> = None;
        loop {
            let event = match output.stream.recv().await {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(e) => return Err(RoundError::broken(converse_error(e))),
            };
            match event {
                StreamEvent::ContentBlockStart(start) => {
                    if let Some(ContentBlockStart::ToolUse(tool)) = start.start() {
                        let index = start.content_block_index();
                        blocks.insert(index, StreamedBlock::Call { id: tool.tool_use_id().to_string(), name: tool.name().to_string(), input: String::new() });
                        events.send(Event::ToolStarted { key: index.to_string(), name: tool.name().to_string() });
                    }
                }
                StreamEvent::ContentBlockDelta(delta) => match delta.delta() {
                    Some(ContentBlockDelta::Text(text)) => {
                        if let StreamedBlock::Text(sofar) = blocks.entry(delta.content_block_index()).or_insert_with(|| StreamedBlock::Text(String::new())) {
                            sofar.push_str(text);
                        }
                        if !text.is_empty() {
                            events.send(Event::Text(text.clone()));
                        }
                    }
                    Some(ContentBlockDelta::ToolUse(tool)) => {
                        if let Some(StreamedBlock::Call { input, .. }) = blocks.get_mut(&delta.content_block_index()) {
                            input.push_str(tool.input());
                        }
                    }
                    _ => {}
                },
                StreamEvent::MessageStop(message_stop) => stop = Some(message_stop.stop_reason().as_str().to_string()),
                StreamEvent::Metadata(metadata) => usage = metadata.usage().and_then(StoredUsage::from_aws),
                _ => {}
            }
        }
        if let Some(usage) = usage {
            events.send(Event::Usage(usage.figures()));
        }
        if stop.is_none() {
            return Err(RoundError::broken("Bedrock stream ended before the reply was complete"));
        }
        let mut content = Vec::new();
        for block in blocks.into_values() {
            match block {
                // Converse refuses an empty text block in the history.
                StreamedBlock::Text(text) if text.is_empty() => {}
                StreamedBlock::Text(text) => content.push(ContentBlock::Text(text)),
                StreamedBlock::Call { id, name, input } => {
                    // A call with no input streams none; input cut short
                    // runs as no arguments.
                    let input: serde_json::Value = serde_json::from_str(&input).ok().filter(serde_json::Value::is_object).unwrap_or_else(|| json!({}));
                    events.send(Event::ToolCall(Call { id: id.clone(), name: name.clone(), arguments: input.to_string() }));
                    let tool_use = aws_sdk_bedrockruntime::types::ToolUseBlock::builder().tool_use_id(id).name(name).input(json_to_document(&input)).build().map_err(|e| RoundError::broken(e.to_string()))?;
                    content.push(ContentBlock::ToolUse(tool_use));
                }
            }
        }
        conversation.reply = Some(Message::builder().role(ConversationRole::Assistant).set_content(Some(content)).build().map_err(|e| RoundError::broken(e.to_string()))?);
        events.send(Event::Done { stop });
        Ok(())
    }

    /// A reply that stopped for tools runs them even when it lists none.
    /// The last round runs no tools.
    fn runs_calls(&self, _conversation: &Conversation, round: &Round, final_round: bool) -> bool {
        (!round.calls.is_empty() || round.stop.as_deref() == Some("tool_use")) && !final_round
    }

    fn add_results(&self, conversation: &mut Conversation, _round: Round, results: Vec<(Call, Outcome)>, next_is_final: bool) -> Result<(), String> {
        conversation.messages.extend(conversation.reply.take());
        let mut blocks = results
            .into_iter()
            .map(|(call, outcome)| {
                let content = match outcome {
                    Outcome::Ran(result) => result,
                    Outcome::Duplicate => SKIPPED.to_string(),
                };
                ToolResultBlock::builder()
                    .tool_use_id(call.id)
                    .content(ToolResultContentBlock::Text(content))
                    .status(ToolResultStatus::Success)
                    .build()
                    .map(ContentBlock::ToolResult)
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        // Converse has no "no tools" choice while the history holds tool
        // calls, so the next, last round is told in words.
        if next_is_final {
            blocks.push(ContentBlock::Text(FINAL_ROUND_NOTE.to_string()));
        }
        conversation.messages.push(Message::builder().role(ConversationRole::User).set_content(Some(blocks)).build().map_err(|e| e.to_string())?);
        Ok(())
    }

    /// A plain Converse request: not streamed, with no tools and no cache
    /// points.
    async fn complete(&self, request: &super::complete::Request<'_>, language: UiLanguage) -> Result<Completion, String> {
        let plain = plain_request(request.system, request.user, request.json, request.max_tokens, language)?;
        let output = converse_plain(&self.creds, &self.model, plain, language).await?;
        Ok(bedrock_result(&output, self.model.clone()))
    }
}

/// The reply as Bedrock wrote it, with the usage AWS returned for it. An
/// empty reply and a JSON request are checked after the row is stored
/// (`store_then_check`).
pub(crate) fn bedrock_result(output: &aws_sdk_bedrockruntime::operation::converse::ConverseOutput, model: String) -> Completion {
    let (text, usage) = super::bedrock::plain_result(output);
    let usage_missing = usage.is_none();
    let usage = usage.unwrap_or_default();
    Completion {
        text,
        provider: "Bedrock".into(),
        model,
        input_tokens: usage.input,
        output_tokens: usage.output,
        cached_tokens: usage.cache_read,
        cache_write_tokens: usage.cache_write,
        usage_missing,
        cost_micro: None,
        price_unknown: false,
    }
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

fn build_tool_config(tool_definitions: &[tools::Tool]) -> Result<ToolConfiguration, Box<dyn std::error::Error + Send + Sync>> {
    let mut tool_specs = tool_definitions
        .iter()
        .map(|tool| {
            let spec = ToolSpecification::builder()
                .name(tool.name.clone())
                .description(tool.description.clone())
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

#[cfg(test)]
use super::provider::note_tools_to_run;

/// One request's row through the usage path, from the figures AWS returned.
#[cfg(test)]
async fn record_metrics(db: &crate::db::Database, workload: crate::ai::Workload, model_id: &str, usage: &TokenUsage, latency_ms: i64) {
    match StoredUsage::from_aws(usage) {
        Some(stored) => super::provider::record_request(db, "bedrock", workload, model_id, stored.figures(), latency_ms),
        None => log::warn!("Bedrock reply carried no usage figures; no usage row stored"),
    }
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
        aws_loopback_with(status, error_type, "application/json", body.to_string().into_bytes()).await
    }

    /// `aws_loopback` with a body of any type, such as a ConverseStream
    /// reply.
    pub(crate) async fn aws_loopback_with(status: u16, error_type: Option<&str>, content_type: &'static str, body: Vec<u8>) -> tokio::task::JoinHandle<Received> {
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
            let reason = if status == 200 { "OK" } else { "Error" };
            let head = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\n{error_header}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
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

    /// One message of AWS's event stream: the prelude (total and header
    /// lengths, their CRC32), string headers, the payload and the CRC32 of
    /// everything before it.
    pub(crate) fn event_message(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for &byte in bytes {
                crc ^= byte as u32;
                for _ in 0..8 {
                    crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
                }
            }
            !crc
        }
        let mut encoded = Vec::new();
        for (name, value) in headers {
            encoded.push(name.len() as u8);
            encoded.extend_from_slice(name.as_bytes());
            encoded.push(7); // a string value
            encoded.extend_from_slice(&(value.len() as u16).to_be_bytes());
            encoded.extend_from_slice(value.as_bytes());
        }
        let total = 12 + encoded.len() + payload.len() + 4;
        let mut message = Vec::with_capacity(total);
        message.extend_from_slice(&(total as u32).to_be_bytes());
        message.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
        let prelude = crc32(&message);
        message.extend_from_slice(&prelude.to_be_bytes());
        message.extend_from_slice(&encoded);
        message.extend_from_slice(payload);
        let checksum = crc32(&message);
        message.extend_from_slice(&checksum.to_be_bytes());
        message
    }

    /// A ConverseStream reply: each (event type, payload) as an event.
    pub(crate) fn converse_stream(events: &[(&str, serde_json::Value)]) -> Vec<u8> {
        events.iter().flat_map(|(kind, payload)| event_message(&[(":event-type", kind), (":content-type", "application/json"), (":message-type", "event")], payload.to_string().as_bytes())).collect()
    }

    /// An error part-way through a ConverseStream reply, as AWS sends it.
    pub(crate) fn stream_exception(kind: &str, message: &str) -> Vec<u8> {
        event_message(&[(":exception-type", kind), (":content-type", "application/json"), (":message-type", "exception")], json!({"message": message}).to_string().as_bytes())
    }

    /// `converse_json` as a ConverseStream reply: the text in two pieces,
    /// then the stop reason and the same usage figures.
    pub(crate) fn converse_stream_text(text: &str) -> Vec<u8> {
        let (first, rest) = text.split_at(text.len() / 2);
        let usage = converse_json(text)["usage"].clone();
        converse_stream(&[
            ("messageStart", json!({"role": "assistant"})),
            ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"text": first}})),
            ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"text": rest}})),
            ("contentBlockStop", json!({"contentBlockIndex": 0})),
            ("messageStop", json!({"stopReason": "end_turn"})),
            ("metadata", json!({"usage": usage, "metrics": {"latencyMs": 7}})),
        ])
    }

    pub(crate) const EVENT_STREAM: &str = "application/vnd.amazon.eventstream";

    pub(crate) fn fixture_creds(region: &str) -> AwsCredentials {
        AwsCredentials {
            access_key_id: "AKIAFIXTURENOTREAL".into(),
            secret_access_key: "fixture-secret-not-real".into(),
            region: region.into(),
            inference_profile: crate::aws::credentials::inference_profile_for_region(region).into(),
        }
    }

    /// The first request of a chat turn as the loop sends it, with the
    /// static prompt "rules", the per-request part "today" and every tool.
    async fn first_request(wire: &BedrockWire, text: &str, db: &crate::db::Database) -> Result<crate::ai::provider::Round, String> {
        let request = crate::ai::provider::ChatRequest {
            system: crate::ai::provider::SystemPrompt { cacheable: "rules".into(), dynamic: "today".into() },
            messages: vec![("user".into(), text.into())],
            tools: tools::get_tool_definitions(),
            deeper: None,
        };
        crate::ai::provider::first_request(wire, request, db, &crate::ai::wire_tests::Window::default()).await
    }

    /// One chat request, sent to a stand-in for AWS: the row holds the four
    /// figures AWS returned, against the model id that went out in the
    /// request.
    #[tokio::test]
    async fn a_chat_request_stores_the_usage_aws_returned_for_the_model_it_sent() {
        let server = aws_loopback_with(200, None, EVENT_STREAM, converse_stream_text("Done.")).await;
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let wire = BedrockWire::new(fixture_creds("ap-southeast-1"), "apac.anthropic.claude-haiku-4-5-20251001-v1:0");
        let round = crate::ai::cost::with_feature(crate::ai::cost::Feature::Chat, first_request(&wire, "hi", &db)).await.unwrap();
        assert!(!round.texts.is_empty() && round.calls.is_empty());

        let received = server.await.unwrap();
        assert_eq!(received.path, "/model/apac.anthropic.claude-haiku-4-5-20251001-v1:0/converse-stream");
        let rows = crate::ai::cost::tests::usage_rows(&db);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model_id, "apac.anthropic.claude-haiku-4-5-20251001-v1:0", "stored against the id that was sent");
        assert_eq!((rows[0].provider.as_str(), rows[0].feature.as_str()), ("bedrock", "chat"));
        // inputTokens 120 + cacheRead 800 + cacheWrite 400; the rest as AWS sent them.
        assert_eq!((rows[0].input_tokens, rows[0].output_tokens), (1_320, 45));
        assert_eq!(crate::ai::cost::tests::cache_tokens(&db), vec![(800, 400)]);
    }

    /// The order in the usage path: a reply asking for a tool that will run
    /// marks the turn before its row is stored, so that row is stored as
    /// tools. Storing first would leave it as chat.
    #[tokio::test]
    async fn a_chat_request_whose_reply_asks_for_a_tool_is_stored_as_tools() {
        let reply = converse_stream(&[
            ("messageStart", json!({"role": "assistant"})),
            ("contentBlockStart", json!({"contentBlockIndex": 0, "start": {"toolUse": {"toolUseId": "tooluse_fixture_1", "name": "list_calendar_events"}}})),
            ("contentBlockStop", json!({"contentBlockIndex": 0})),
            ("messageStop", json!({"stopReason": "tool_use"})),
            ("metadata", json!({"usage": converse_json("")["usage"], "metrics": {"latencyMs": 7}})),
        ]);
        let server = aws_loopback_with(200, None, EVENT_STREAM, reply).await;
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let wire = BedrockWire::new(fixture_creds("ap-southeast-1"), "apac.anthropic.claude-haiku-4-5-20251001-v1:0");
        let round = crate::ai::cost::with_feature(crate::ai::cost::Feature::Chat, first_request(&wire, "what is on today?", &db)).await.unwrap();
        server.await.unwrap();
        assert_eq!(round.calls.len(), 1, "the stand-in's reply asks for one tool");
        assert_eq!(features(&db), vec!["tools"]);
        assert_eq!(crate::ai::cost::tests::usage_rows(&db)[0].input_tokens, 1_320, "the row holds this reply's usage");
    }

    /// A chat request AWS refuses stores nothing: there is no usage to store.
    #[tokio::test]
    async fn a_failed_chat_request_stores_nothing() {
        let server = aws_loopback(400, Some("ValidationException"), json!({"message": "PRIVATE-MAIL-CANARY"})).await;
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let wire = BedrockWire::new(fixture_creds("ap-southeast-1"), "global.anthropic.claude-sonnet-4-6");
        assert!(first_request(&wire, "hi", &db).await.is_err());
        server.await.unwrap();
        assert!(crate::ai::cost::tests::usage_rows(&db).is_empty());
    }

    /// A reply without usage figures stores nothing rather than a row of
    /// zeros.
    #[tokio::test]
    async fn a_chat_reply_without_usage_stores_nothing() {
        let reply = converse_stream(&[
            ("messageStart", json!({"role": "assistant"})),
            ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"text": "Done."}})),
            ("messageStop", json!({"stopReason": "end_turn"})),
            ("metadata", json!({"metrics": {"latencyMs": 7}})),
        ]);
        let server = aws_loopback_with(200, None, EVENT_STREAM, reply).await;
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let wire = BedrockWire::new(fixture_creds("us-east-1"), "us.amazon.nova-pro-v1:0");
        // Whether the SDK accepts a reply without usage or calls it
        // malformed, no row is stored for it.
        let _ = first_request(&wire, "hi", &db).await;
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
