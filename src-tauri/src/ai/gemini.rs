use super::tools;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use super::provider::{Call, ChatRequest, Event, Events, Metering, Outcome, Round, RoundError, SchemaRules, Wire, SKIPPED};
use super::complete::{Completion, Request as PlainRequest};
use super::bedrock::UiLanguage;

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
    /// One request's figures. promptTokenCount already includes the cached
    /// tokens; thinking is billed as output.
    fn figures(&self) -> super::provider::Usage {
        super::provider::Usage {
            input: self.prompt_token_count.unwrap_or(0),
            output: self.candidates_token_count.unwrap_or(0) + self.thoughts_token_count.unwrap_or(0),
            cache_read: self.cached_content_token_count.unwrap_or(0),
            cache_write: 0,
        }
    }

    /// Fold this request into a turn, as the usage path does.
    #[cfg(test)]
    fn add_to(&self, turn: &mut crate::ai::cost::TurnUsage) {
        let figures = self.figures();
        turn.add_request(figures.input, figures.output, figures.cache_read, figures.cache_write);
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

/// The request's generationConfig.
fn generation_config(max_output_tokens: u32, deeper: Option<crate::ai::capability::Deeper>) -> serde_json::Value {
    let mut config = json!({"maxOutputTokens": max_output_tokens});
    if let Some(thinking) = thinking_config(deeper) {
        config["thinkingConfig"] = thinking;
    }
    config
}

/// The thinking parameter a request carries (P06), the one place that
/// decides it. A "Think harder" turn on a model the registry marks as
/// thinking asks for the high thinking level, and only that: never minimal
/// or xhigh, and no model id or version is read here. Any other turn keeps
/// Google's default for the model. Whether gemini-2.5 models accept
/// `thinkingLevel` is not settled (DECISIONS.md, P06 rows of 2026-11-17); if
/// Google refuses it, the request ends on the error card with Google's
/// reason, and nothing is switched.
fn thinking_config(deeper: Option<crate::ai::capability::Deeper>) -> Option<serde_json::Value> {
    deeper.is_some_and(|d| d.thinking).then(|| json!({"thinkingLevel": "HIGH"}))
}

/// What the model sent in one round, kept so the next request can send it
/// back as Google requires ("resend all thought blocks exactly as they were
/// received"): every function-call part unchanged, with the thoughtSignature
/// it carries, and the streamed text as one part with the signature a text
/// part arrived with. Gemini 3 models think on every turn, so this holds for
/// any turn with tools, not only "Think harder".
#[derive(Default)]
struct ModelRound {
    texts: Vec<String>,
    text_signature: Option<String>,
    calls: Vec<GeminiPart>,
}

impl ModelRound {
    fn take(&mut self, part: &GeminiPart) {
        if let Some(text) = &part.text {
            self.texts.push(text.clone());
        }
        if part.function_call.is_some() {
            self.calls.push(part.clone());
        } else if let Some(signature) = &part.thought_signature {
            self.text_signature = Some(signature.clone());
        }
    }

    /// The model turn sent back before the function responses.
    fn parts(&self) -> Vec<GeminiPart> {
        let mut parts = Vec::new();
        let text = self.texts.join("");
        if !text.is_empty() || self.text_signature.is_some() {
            parts.push(GeminiPart { text: Some(text), thought_signature: self.text_signature.clone(), ..Default::default() });
        }
        parts.extend(self.calls.iter().cloned());
        parts
    }
}

/// Google's Gemini API on the provider interface.
pub(crate) struct GeminiWire {
    key: String,
    model: String,
    deeper: Option<crate::ai::capability::Deeper>,
}

impl GeminiWire {
    pub(crate) fn new(key: &str, model: &str, deeper: Option<crate::ai::capability::Deeper>) -> Self {
        GeminiWire { key: key.to_string(), model: model.to_string(), deeper }
    }
}

/// One chat turn's requests to Gemini.
pub(crate) struct Conversation {
    client: Client,
    contents: Vec<GeminiContent>,
    tools: Vec<serde_json::Value>,
    system_instruction: serde_json::Value,
    /// The round in progress, sent back as it came before the results of
    /// its calls.
    round: ModelRound,
    /// What the last reply held, for the log of an empty one.
    events: usize,
    bytes: usize,
}

#[async_trait::async_trait]
impl Wire for GeminiWire {
    type Conversation = Conversation;

    fn max_rounds(&self) -> usize {
        crate::ai::models::max_tool_rounds("gemini")
    }

    fn metering(&self) -> Metering {
        Metering::Turn { provider: "gemini".into(), model: self.model.clone() }
    }

    fn stopped_at_limit(&self, round: &Round) -> bool {
        round.stop.as_deref() == Some("MAX_TOKENS")
    }

    fn schema_rules(&self) -> &'static SchemaRules {
        &GEMINI_SCHEMA
    }

    async fn start(&self, request: ChatRequest) -> Result<Conversation, String> {
        let client = Client::builder()
            .read_timeout(std::time::Duration::from_secs(90))
            // A redirect would carry the key (x-goog-api-key) and the conversation elsewhere.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Conversation {
            client,
            contents: history_contents(request.messages),
            tools: declarations(&request.tools, self.schema_rules()),
            system_instruction: json!({"parts": [{"text": request.system.joined()}]}),
            round: ModelRound::default(),
            events: 0,
            bytes: 0,
        })
    }

    async fn round(&self, conversation: &mut Conversation, final_round: bool, events: &mut Events<'_>) -> Result<(), RoundError> {
        let url = format!("{}/models/{}:streamGenerateContent?alt=sse", super::api_base(API_BASE), self.model);
        let mut request_body = json!({
            "contents": conversation.contents,
            "tools": conversation.tools,
            "systemInstruction": conversation.system_instruction,
            "generationConfig": generation_config(output_cap(&self.model), self.deeper)
        });
        if final_round {
            // The last allowed round answers from what earlier rounds gathered.
            request_body["toolConfig"] = json!({"functionCallingConfig": {"mode": "NONE"}});
        }
        let response = conversation
            .client
            .post(&url)
            .header("x-goog-api-key", &self.key)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| RoundError::refused(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RoundError::refused(format!("Gemini API error {}: {}", status, body)));
        }

        // usageMetadata is cumulative within one request: the last one stands.
        let mut usage = UsageMetadata::default();
        let mut stream = response.bytes_stream();
        let mut buffer = super::SseByteBuffer::new();
        let mut round = ModelRound::default();
        let mut finish_reason: Option<String> = None;
        let mut event_count: usize = 0;
        let mut total_bytes: usize = 0;

        while let Some(chunk_result) = stream.next().await {
            let chunk = match chunk_result {
                Ok(bytes) => bytes,
                Err(e) => {
                    events.send(Event::Usage(usage.figures()));
                    return Err(RoundError::broken(format!("Stream error: {}", e)));
                }
            };
            total_bytes += chunk.len();
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
                        log::warn!("{}", super::failure::unparsed_event_line("Gemini", &e, data_str.len()));
                        continue;
                    }
                };
                if let Some(u) = sse.usage_metadata {
                    usage = u;
                }
                for candidate in sse.candidates.iter().flatten() {
                    if let Some(ref fr) = candidate.finish_reason {
                        finish_reason = Some(fr.clone());
                    }
                    for part in candidate.content.iter().flat_map(|c| c.parts.iter().flatten()) {
                        if let Some(ref text) = part.text {
                            events.send(Event::Text(text.clone()));
                        }
                        if let Some(ref fc) = part.function_call {
                            // Each tool is announced once, by name.
                            events.send(Event::ToolStarted { key: fc.name.clone(), name: fc.name.clone() });
                        }
                        round.take(part);
                    }
                }
            }
        }
        events.send(Event::Usage(usage.figures()));
        for fc in round.calls.iter().filter_map(|part| part.function_call.as_ref()) {
            events.send(Event::ToolCall(Call { id: fc.id.clone().unwrap_or_default(), name: fc.name.clone(), arguments: serde_json::to_string(&fc.args).unwrap_or_default() }));
        }
        conversation.round = round;
        conversation.events = event_count;
        conversation.bytes = total_bytes;
        events.send(Event::Done { stop: finish_reason });
        Ok(())
    }

    fn add_results(&self, conversation: &mut Conversation, _round: Round, results: Vec<(Call, Outcome)>, _next_is_final: bool) -> Result<(), String> {
        let round = std::mem::take(&mut conversation.round);
        conversation.contents.push(GeminiContent { role: "model".into(), parts: round.parts() });
        let parts = results
            .into_iter()
            .map(|(call, outcome)| {
                let result = match outcome {
                    Outcome::Ran(result) => result,
                    Outcome::Duplicate => SKIPPED.to_string(),
                };
                GeminiPart { function_response: Some(FunctionResponseData { name: call.name, response: json!({"result": result}) }), ..Default::default() }
            })
            .collect();
        conversation.contents.push(GeminiContent { role: "user".into(), parts });
        Ok(())
    }

    /// A reply with neither text nor a call is an error the router sends
    /// once more on. The last round's calls end with the fallback; calls
    /// the loop refused earlier, with its sentence after nothing said.
    fn reply(&self, conversation: &Conversation, round: Round, final_round: bool, language: &str) -> Result<String, String> {
        if !round.texts.is_empty() {
            return Ok(round.text());
        }
        if !round.calls.is_empty() {
            return Ok(if final_round { tools::tool_limit_fallback(language).into() } else { String::new() });
        }
        log::warn!(
            "[gemini] empty response: model={} finish_reason={:?} function_calls={} events={} bytes={}",
            self.model,
            round.stop,
            round.calls.len(),
            conversation.events,
            conversation.bytes
        );
        Err(empty_reply_error(&self.model, round.stop.as_deref()))
    }

    async fn complete(&self, request: &PlainRequest<'_>, _language: UiLanguage) -> Result<Completion, String> {
        let client = super::compat::client()?;
        let mut body = json!({
            "contents": [{"role": "user", "parts": [{"text": request.user}]}],
            "generationConfig": {"maxOutputTokens": request.max_tokens},
        });
        if !request.system.is_empty() {
            body["systemInstruction"] = json!({"parts": [{"text": request.system}]});
        }
        if request.json {
            body["generationConfig"]["responseMimeType"] = json!("application/json");
        }
        let url = format!("{}/models/{}:generateContent", super::api_base(API_BASE), self.model);
        let value = super::complete::post(&client, &url, "Gemini", |r| r.header("x-goog-api-key", &self.key), &body).await?;
        gemini_result(&value, self.model.clone())
    }
}

pub(crate) fn gemini_result(value: &serde_json::Value, model: String) -> Result<Completion, String> {
    let text = value
        .pointer("/candidates/0/content/parts")
        .and_then(serde_json::Value::as_array)
        .map(|parts| parts.iter().filter(|p| p.get("thought").and_then(serde_json::Value::as_bool) != Some(true)).filter_map(|p| p.get("text").and_then(serde_json::Value::as_str)).collect::<String>())
        .unwrap_or_default();
    let usage = |field: &str| value.pointer(&format!("/usageMetadata/{field}")).and_then(serde_json::Value::as_i64).unwrap_or(0);
    let mut done = super::complete::done(text, "Gemini".into(), model, value.pointer("/usageMetadata/promptTokenCount"), value.pointer("/usageMetadata/candidatesTokenCount"))?;
    // Thinking is billed as output; candidatesTokenCount leaves it out.
    done.output_tokens += usage("thoughtsTokenCount");
    done.cached_tokens = usage("cachedContentTokenCount");
    Ok(done)
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

/// Gemini's rules for tool parameters (P02): the Schema fields above, and
/// an object field with no properties asked for as JSON text. Gemini
/// refuses such a field ("should be non-empty for OBJECT type") and has no
/// way to say "any object".
pub(crate) const GEMINI_SCHEMA: SchemaRules = SchemaRules { fields: Some(SCHEMA_FIELDS), json_text_note: Some(JSON_TEXT_NOTE) };

/// The tools as Gemini's function declarations, their parameters under
/// `rules`: the wire's own (`schema_rules`), the same rules that turn the
/// arguments back before a tool runs.
fn declarations(tools: &[tools::Tool], rules: &SchemaRules) -> Vec<serde_json::Value> {
    let declarations: Vec<serde_json::Value> = tools.iter().map(|t| json!({"name": t.name, "description": t.description, "parameters": rules.sanitize(&t.parameters)})).collect();
    vec![json!({ "functionDeclarations": declarations })]
}

/// A tool's parameters in a shape Gemini accepts.
#[cfg(test)]
pub(crate) fn sanitize_schema(schema: &serde_json::Value) -> serde_json::Value {
    GEMINI_SCHEMA.sanitize(schema)
}

/// Undo `sanitize_schema` on the arguments Gemini sent.
#[cfg(test)]
pub(crate) fn restore_json_text(schema: &serde_json::Value, value: &mut serde_json::Value) {
    GEMINI_SCHEMA.restore(schema, value);
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

    /// The one place that decides the thinking parameter: HIGH on a marked
    /// turn for a thinking model, nothing otherwise, and never another level.
    #[test]
    fn the_thinking_parameter_is_high_on_a_marked_turn_and_absent_otherwise() {
        use crate::ai::capability::Deeper;
        assert_eq!(thinking_config(Some(Deeper { thinking: true, effort: false })), Some(json!({"thinkingLevel": "HIGH"})));
        assert_eq!(thinking_config(Some(Deeper { thinking: true, effort: true })), Some(json!({"thinkingLevel": "HIGH"})));
        assert_eq!(thinking_config(Some(Deeper { thinking: false, effort: true })), None);
        assert_eq!(thinking_config(None), None);
    }

    /// A round streamed with thought signatures (canned SSE chunks): the
    /// model turn sent back carries every signature unchanged, the
    /// function-call parts as received and the text with its signature.
    #[test]
    fn thought_signatures_go_back_unchanged_before_the_function_responses() {
        let chunks = [
            r#"{"candidates":[{"content":{"parts":[{"text":"Let me check "}]}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":"your calendar.","thoughtSignature":"dGV4dC1zaWc="}]}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"list_events","args":{"days":7}},"thoughtSignature":"Y2FsbC1zaWc="}]}}]}"#,
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"list_tasks","args":{}}}]}}]}"#,
        ];
        let mut round = ModelRound::default();
        for chunk in chunks {
            let sse: StreamChunk = serde_json::from_str(chunk).unwrap();
            for candidate in sse.candidates.unwrap() {
                for part in candidate.content.unwrap().parts.unwrap() {
                    round.take(&part);
                }
            }
        }
        assert_eq!(serde_json::to_value(round.parts()).unwrap(), json!([
            {"text": "Let me check your calendar.", "thoughtSignature": "dGV4dC1zaWc="},
            {"functionCall": {"name": "list_events", "args": {"days": 7}}, "thoughtSignature": "Y2FsbC1zaWc="},
            {"functionCall": {"name": "list_tasks", "args": {}}},
        ]));
        assert_eq!(round.texts.join(""), "Let me check your calendar.");
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
        let tools = declarations(&crate::ai::tools::get_tool_definitions(), &GEMINI_SCHEMA);
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
