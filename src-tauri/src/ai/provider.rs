//! One interface for every AI provider's wire format (P07).
//!
//! A [`Wire`] knows one provider's request and reply format: it builds the
//! request from a normalised [`ChatRequest`], sends it, and reports what the
//! reply carried as [`Event`]s (text, thinking, tool calls, usage, done). An
//! error ends the round with its message.
//!
//! Everything else is here, once for every provider: the tool loop with its
//! rounds and its last round, the duplicate-call check, the status lines and
//! the voice, the approval path (`tools::execute_tool`, unchanged), the
//! side-effect ledger the router's "never resend after an action ran" rule
//! reads, and the usage rows. One wire serves several services: OpenAI's own
//! API, every OpenAI-compatible service and the servers on this Mac all speak
//! the OpenAI format.

use super::{tools, SideEffectLedger, Workload};
use crate::chat_session::ScopedEmitter;
use crate::voice::tts::TtsCommand;
use serde_json::{json, Value};
use std::collections::HashSet;

/// What a chat turn asks of a wire, whichever provider answers it.
pub(crate) struct ChatRequest {
    pub system: SystemPrompt,
    /// The stored conversation, oldest first, as `(role, text)`. Each wire
    /// drops the rows its provider refuses (empty text) and merges what it
    /// must.
    pub messages: Vec<(String, String)>,
    pub tools: Vec<tools::Tool>,
    /// "Think harder" (P06), as far as the model takes it.
    pub deeper: Option<super::capability::Deeper>,
}

/// The system prompt in its two parts: the cacheable rules, the same for
/// every request, and the part written for this request (the time, the
/// user's name, what JARVIS remembers).
pub(crate) struct SystemPrompt {
    pub cacheable: String,
    pub dynamic: String,
}

impl SystemPrompt {
    pub(crate) fn for_request(db: &crate::db::Database) -> Self {
        let user_name = crate::util::display_name(db);
        let language = tools::read_language_pref(db);
        SystemPrompt { cacheable: tools::build_cacheable_system_prompt(), dynamic: tools::build_dynamic_suffix(&user_name, chrono::Local::now(), &language, db) }
    }

    /// Both parts as one text, as `tools::build_system_prompt` writes it.
    pub(crate) fn joined(&self) -> String {
        format!("{}\n\n{}", self.cacheable, self.dynamic)
    }
}

/// One tool call the model asked for: arguments as JSON text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Call {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// One request's usage. `input` counts every input token, cached ones
/// included, as the usage table does for every provider.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Usage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

/// What a wire reports while one request streams.
pub(crate) enum Event {
    /// A piece of the answer, shown and spoken as it arrives.
    Text(String),
    /// A piece of the model's thinking (Claude, on Think harder turns).
    Thinking(String),
    /// The model began a tool call. `key` tells the calls of one round
    /// apart for the announcement: the call's position (OpenAI, Claude), or
    /// its name (Gemini announces each tool once).
    ToolStarted { key: String, name: String },
    /// A complete call.
    ToolCall(Call),
    /// The request's usage, once, at the end of the request or just before
    /// an error ends it. A wire that reports nothing for a request without
    /// figures (Bedrock) stores nothing for it.
    Usage(Usage),
    /// The request finished; why, in the provider's words.
    Done { stop: Option<String> },
}

/// Why a round ended early.
pub(crate) struct RoundError {
    pub message: String,
    /// The stream broke after the reply began: the chat window is told the
    /// turn went idle.
    pub idle: bool,
}

impl RoundError {
    /// The request was refused or never reached the provider.
    pub(crate) fn refused(message: impl Into<String>) -> Self {
        RoundError { message: message.into(), idle: false }
    }

    /// The reply broke off part-way.
    pub(crate) fn broken(message: impl Into<String>) -> Self {
        RoundError { message: message.into(), idle: true }
    }
}

impl From<String> for RoundError {
    fn from(message: String) -> Self {
        RoundError::refused(message)
    }
}

/// How a wire's usage is stored (the one usage path).
pub(crate) enum Metering {
    /// One row per turn, each request priced on its own; on failure, what
    /// earlier requests were billed (OpenAI, compatible services, Claude,
    /// Gemini).
    Turn { provider: String, model: String },
    /// One row per turn, only when the turn succeeds (a model on this Mac:
    /// free, but each turn counts).
    AnsweredTurn { provider: String, model: String },
    /// One row per request, as soon as it is answered, against the model id
    /// it was sent with (Bedrock, the founder's P02 rule).
    Request { provider: String, model: String },
}

/// The outcome of one call the model asked for.
pub(crate) enum Outcome {
    /// It ran; what the tool returned.
    Ran(String),
    /// The same tool with the same arguments ran earlier in this round.
    Duplicate,
}

/// What the model sent back for the duplicate call of a round.
pub(crate) const SKIPPED: &str = "Skipped: already executed this tool with these arguments in this batch.";

/// What one request's reply carried, once it is complete.
pub(crate) struct Round {
    /// The answer's pieces, as they streamed.
    pub texts: Vec<String>,
    pub calls: Vec<Call>,
    pub stop: Option<String>,
}

impl Round {
    pub(crate) fn text(&self) -> String {
        self.texts.join("")
    }
}

/// One provider's request and reply format.
#[async_trait::async_trait]
pub(crate) trait Wire: Send + Sync {
    /// One turn's requests so far, in the provider's own format.
    type Conversation: Send;

    /// Tool rounds before the last one, whose answer is final (registry).
    fn max_rounds(&self) -> usize;

    fn metering(&self) -> Metering;

    /// The turn's first request, not yet sent.
    async fn start(&self, request: ChatRequest) -> Result<Self::Conversation, String>;

    /// Send the next request and report its reply.
    async fn round(&self, conversation: &mut Self::Conversation, final_round: bool, events: &mut Events<'_>) -> Result<(), RoundError>;

    /// Whether the wire itself sends a chat request once more after a rate
    /// limit or a server error (429, 5xx). The router then does not send it
    /// again ([`super::resends_inside`]). A completion is sent once by every
    /// wire, and once more by the router.
    fn resends_transient(&self) -> bool {
        false
    }

    /// Whether the round's calls run, before the last round: the loop runs
    /// none on the last round, whatever the wire says.
    fn runs_calls(&self, _conversation: &Self::Conversation, round: &Round) -> bool {
        !round.calls.is_empty()
    }

    /// Add the round's reply and the outcome of each of its calls, for the
    /// next request. `next_is_final` is true when the next request is the
    /// last one allowed. Most wires replay the native copy of the reply
    /// they kept in the conversation; `round` serves a reply with no native
    /// form (the OpenAI wire's described-tools mode sends its text back).
    fn add_results(&self, conversation: &mut Self::Conversation, round: Round, results: Vec<(Call, Outcome)>, next_is_final: bool) -> Result<(), String>;

    /// Whether the turn ends with the round's text right after its calls ran
    /// (Claude, when the reply also said it was done).
    fn answers_after_tools(&self, _round: &Round) -> bool {
        false
    }

    /// The turn's answer from its last round.
    fn reply(&self, conversation: &Self::Conversation, round: Round, final_round: bool, language: &str) -> Result<String, String> {
        let _ = conversation;
        let text = round.text();
        if final_round && text.trim().is_empty() {
            return Ok(tools::tool_limit_fallback(language).into());
        }
        Ok(text)
    }

    /// Whether the chat window shows this wire's thinking (and is told when
    /// it ends).
    fn shows_thinking(&self) -> bool {
        false
    }

    /// What this provider accepts in a tool's parameters.
    fn schema_rules(&self) -> &'static SchemaRules {
        &SchemaRules::JSON_SCHEMA
    }

    /// One plain request for a feature that is not a chat turn (mail
    /// analysis, mail sorting, the one-word check): no tools, not streamed,
    /// with this provider's body as it was before the interface. With
    /// `request.json` the reply is asked to be one JSON object, by the
    /// provider's own switch where it has one; the prompt says so too.
    /// `language` is that of the errors a user can see.
    async fn complete(&self, request: &super::complete::Request<'_>, language: super::bedrock::UiLanguage) -> Result<super::complete::Completion, String>;
}

/// What one provider accepts in a tool's parameters: P02's Gemini
/// sanitizer, made a rule of each wire. The tools are written in JSON
/// Schema, which OpenAI, Anthropic and Bedrock take as written. Gemini takes
/// an OpenAPI subset: only its fields, and no object without listed
/// properties, so such a field is asked for as JSON text and turned back
/// into the object before the tool runs.
pub(crate) struct SchemaRules {
    /// The schema fields the provider takes; None takes every field.
    pub fields: Option<&'static [&'static str]>,
    /// Added to the description of a field asked for as JSON text; None
    /// declares an open object as it is.
    pub json_text_note: Option<&'static str>,
}

impl SchemaRules {
    /// JSON Schema, as the tools are written.
    pub(crate) const JSON_SCHEMA: SchemaRules = SchemaRules { fields: None, json_text_note: None };

    /// A tool's parameters as the provider accepts them. The top level is
    /// left an object: system_info and list_memories take no arguments and
    /// have shipped with empty properties.
    pub(crate) fn sanitize(&self, schema: &Value) -> Value {
        if self.fields.is_none() && self.json_text_note.is_none() {
            return schema.clone();
        }
        self.sanitize_node(schema, true)
    }

    fn sanitize_node(&self, schema: &Value, top_level: bool) -> Value {
        let Some(fields) = schema.as_object() else { return schema.clone() };
        if let Some(note) = self.json_text_note {
            if !top_level && is_open_object(fields) {
                let description = fields.get("description").and_then(Value::as_str).unwrap_or_default();
                return json!({"type": "string", "description": format!("{description} {note}").trim()});
            }
        }
        let mut out = serde_json::Map::new();
        for (key, value) in fields.iter().filter(|(key, _)| self.fields.is_none_or(|kept| kept.contains(&key.as_str()))) {
            let value = match key.as_str() {
                "properties" => Value::Object(value.as_object().map(|p| p.iter().map(|(name, field)| (name.clone(), self.sanitize_node(field, false))).collect()).unwrap_or_default()),
                "items" => self.sanitize_node(value, false),
                "anyOf" => Value::Array(value.as_array().map(|options| options.iter().map(|option| self.sanitize_node(option, false)).collect()).unwrap_or_default()),
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

    /// Undo [`SchemaRules::sanitize`] on a call's arguments: a field asked
    /// for as JSON text goes back to the object the tool expects. A field
    /// sent as an object anyway, or text that is not a JSON object, is left
    /// as it is.
    pub(crate) fn restore(&self, schema: &Value, value: &mut Value) {
        if self.json_text_note.is_some() {
            restore_json_text(schema, value);
        }
    }

    /// A call's arguments as the tool takes them. Unchanged, byte for byte,
    /// under rules that ask for no JSON text.
    fn arguments(&self, tools: &[tools::Tool], call: Call) -> Call {
        if self.json_text_note.is_none() {
            return call;
        }
        let Some(tool) = tools.iter().find(|t| t.name == call.name) else { return call };
        let Ok(mut args) = serde_json::from_str::<Value>(&call.arguments) else { return call };
        self.restore(&tool.parameters, &mut args);
        Call { arguments: serde_json::to_string(&args).unwrap_or_default(), ..call }
    }
}

/// An object with no listed properties: JSON Schema's "any object".
fn is_open_object(schema: &serde_json::Map<String, Value>) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("object") && schema.get("properties").and_then(Value::as_object).is_none_or(|p| p.is_empty())
}

fn restore_json_text(schema: &Value, value: &mut Value) {
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

/// The chat window, behind one object-safe call.
pub(crate) trait Window {
    fn send(&self, event: &str, payload: Value);
}

impl<T: ScopedEmitter> Window for T {
    fn send(&self, event: &str, payload: Value) {
        let _ = self.emit(event, payload);
    }
}

/// Where a round's events go: shown and spoken as they arrive, and kept for
/// the loop once the request is complete.
pub(crate) struct Events<'a> {
    window: &'a (dyn Window + Sync),
    voice: Option<&'a tokio::sync::mpsc::Sender<TtsCommand>>,
    responding: bool,
    announced: HashSet<String>,
    texts: Vec<String>,
    calls: Vec<Call>,
    usage: Option<Usage>,
    stop: Option<String>,
}

impl<'a> Events<'a> {
    fn new(window: &'a (dyn Window + Sync), voice: Option<&'a tokio::sync::mpsc::Sender<TtsCommand>>) -> Self {
        Events { window, voice, responding: false, announced: HashSet::new(), texts: Vec::new(), calls: Vec::new(), usage: None, stop: None }
    }

    pub(crate) fn send(&mut self, event: Event) {
        match event {
            Event::Text(text) => {
                if !self.responding {
                    self.responding = true;
                    self.window.send("chat-status", json!({"status": "Composing response...", "phase": "responding"}));
                }
                self.window.send("chat-token", json!({"token": text, "done": false}));
                if let Some(voice) = self.voice {
                    let _ = voice.try_send(TtsCommand::TextChunk(text.clone()));
                }
                self.texts.push(text);
            }
            Event::Thinking(text) => self.window.send("chat-thinking", json!({"text": text, "done": false})),
            Event::ToolStarted { key, name } => {
                self.window.send("chat-status", json!({"status": format!("Planning: {}", tools::tool_status_label(&name)), "phase": "planning"}));
                if self.announced.insert(key) {
                    self.window.send("chat-tool-call", json!({"tool_name": name}));
                }
            }
            Event::ToolCall(call) => self.calls.push(call),
            Event::Usage(usage) => self.usage = Some(usage),
            Event::Done { stop } => self.stop = stop,
        }
    }

    fn finish(self) -> (Round, Option<Usage>) {
        (Round { texts: self.texts, calls: self.calls, stop: self.stop }, self.usage)
    }
}

/// What a chat turn runs with, besides the request.
pub(crate) struct Turn<'a> {
    pub db: &'a crate::db::Database,
    pub google_auth: &'a std::sync::Arc<crate::auth::google::GoogleAuth>,
    pub window: &'a (dyn Window + Sync),
    pub voice: Option<tokio::sync::mpsc::Sender<TtsCommand>>,
    pub workload: Workload,
    pub ledger: &'a SideEffectLedger,
}

/// The usage of the turn in progress, stored as the wire's metering says.
struct Meter {
    metering: Metering,
    turn: Option<super::cost::TurnUsage>,
    started_at: std::time::Instant,
}

impl Meter {
    fn new(metering: Metering) -> Self {
        let turn = match &metering {
            Metering::Turn { provider, model } | Metering::AnsweredTurn { provider, model } => Some(super::cost::TurnUsage::new(provider, model)),
            Metering::Request { .. } => None,
        };
        Meter { metering, turn, started_at: std::time::Instant::now() }
    }

    fn elapsed(&self) -> i64 {
        self.started_at.elapsed().as_millis() as i64
    }

    /// One request answered. A per-request wire stores it now, as a tool
    /// request when its calls will run (`tools::execute_tool` marks the turn
    /// only once a tool runs, after this row).
    fn request(&mut self, db: &crate::db::Database, workload: Workload, usage: Option<Usage>, calls: usize, final_round: bool, latency_ms: i64) {
        if let Some(turn) = self.turn.as_mut() {
            let usage = usage.unwrap_or_default();
            turn.add_request(usage.input, usage.output, usage.cache_read, usage.cache_write);
            return;
        }
        let Metering::Request { provider, model } = &self.metering else { return };
        note_tools_to_run(calls, final_round);
        match usage {
            Some(usage) => record_request(db, provider, workload, model, usage, latency_ms),
            None => log::warn!("{provider} reply carried no usage figures; no usage row stored"),
        }
    }

    /// A request failed part-way: its usage so far, then what the turn was
    /// billed. A turn on this Mac is stored only when it succeeds. A request
    /// stored on its own is stored when its reply carried usage (AWS bills a
    /// reply that holds no message); a refused one carries none.
    fn failed(&mut self, db: &crate::db::Database, workload: Workload, usage: Option<Usage>, latency_ms: i64) {
        let turn_latency = self.elapsed();
        match (&self.metering, self.turn.take()) {
            (Metering::Turn { .. }, Some(mut turn)) => {
                if let Some(usage) = usage {
                    turn.add_request(usage.input, usage.output, usage.cache_read, usage.cache_write);
                }
                turn.record_if_billed(db, workload, turn_latency);
            }
            (Metering::Request { provider, model }, _) => {
                if let Some(usage) = usage {
                    record_request(db, provider, workload, model, usage, latency_ms);
                }
            }
            _ => {}
        }
    }

    /// The turn answered; `answered` when its last reply streamed text.
    fn succeeded(mut self, db: &crate::db::Database, workload: Workload, answered: bool) {
        let latency = self.elapsed();
        if let Some(turn) = self.turn.take() {
            if let Metering::Turn { provider, model } = &self.metering {
                if answered && turn.is_empty() {
                    log::warn!("[{provider}] no usage figures received for a non-empty response; recording zero tokens (model={model})");
                }
            }
            turn.record(db, workload, latency);
        }
    }
}

/// A request stored on its own is stored as a tool request when the calls
/// it asks for will run: `tools::execute_tool` marks the turn only once a
/// tool runs, after this request's row. The last round runs no tools, and
/// some requests turn tools off.
pub(crate) fn note_tools_to_run(calls: usize, final_round: bool) {
    let disabled = crate::chat_session::TOOL_EXECUTION_DISABLED.try_with(|v| *v).unwrap_or(false);
    if calls > 0 && !final_round && !disabled {
        super::cost::note_tool_call();
    }
}

/// One request's row, under the feature of the request that started the
/// turn.
pub(crate) fn record_request(db: &crate::db::Database, provider: &str, workload: Workload, model: &str, usage: Usage, latency_ms: i64) {
    super::cost::record_inference(db, provider, super::cost::current_feature(), workload, model, usage.input, usage.output, usage.cache_read, usage.cache_write, latency_ms);
}

/// One chat turn on `wire`: rounds of requests and tool calls until the
/// model answers or the last round is reached.
pub(crate) async fn chat<W: Wire>(wire: &W, request: ChatRequest, turn: Turn<'_>) -> Result<String, String> {
    let mut meter = Meter::new(wire.metering());
    let tools = request.tools.clone();
    let mut conversation = wire.start(request).await?;
    let max_rounds = wire.max_rounds();
    for number in 0..max_rounds {
        let final_round = number + 1 == max_rounds;
        let (round, runs) = one_request(wire, &mut conversation, &tools, final_round, &turn, &mut meter).await?;
        if runs {
            let results = run_calls(&round.calls, &turn).await;
            let answers = wire.answers_after_tools(&round);
            let text = round.text();
            // The results not added is a failed request: what the turn was
            // billed is stored, and the router sees the calls that ran.
            if let Err(error) = wire.add_results(&mut conversation, round, results, number + 2 == max_rounds) {
                meter.failed(turn.db, turn.workload, None, 0);
                return Err(error);
            }
            if answers {
                finish(wire, &turn);
                meter.succeeded(turn.db, turn.workload, !text.is_empty());
                return Ok(text);
            }
            turn.window.send("chat-status", json!({"status": "Reviewing tool results...", "phase": "thinking"}));
            continue;
        }
        let streamed = !round.texts.is_empty();
        let reply = wire.reply(&conversation, round, final_round, &tools::read_language_pref(turn.db));
        if let Ok(text) = &reply {
            // An answer that did not stream (the fallback on an empty last
            // round) is shown and spoken in one piece.
            if !streamed && !text.is_empty() {
                Events::new(turn.window, turn.voice.as_ref()).send(Event::Text(text.clone()));
            }
        }
        finish(wire, &turn);
        meter.succeeded(turn.db, turn.workload, streamed);
        return reply;
    }
    // Only when the registry allows no round at all.
    finish(wire, &turn);
    meter.succeeded(turn.db, turn.workload, false);
    Ok(tools::tool_limit_fallback(&tools::read_language_pref(turn.db)).into())
}

/// One request of a turn: its events shown as they come, its calls'
/// arguments as the tools take them, and its usage stored as the wire's
/// metering says. Returns the round and whether its calls run.
async fn one_request<W: Wire>(wire: &W, conversation: &mut W::Conversation, tools: &[tools::Tool], final_round: bool, turn: &Turn<'_>, meter: &mut Meter) -> Result<(Round, bool), String> {
    let sent_at = std::time::Instant::now();
    let mut events = Events::new(turn.window, turn.voice.as_ref());
    let result = wire.round(conversation, final_round, &mut events).await;
    let (mut round, usage) = events.finish();
    let rules = wire.schema_rules();
    round.calls = round.calls.into_iter().map(|call| rules.arguments(tools, call)).collect();
    let latency_ms = sent_at.elapsed().as_millis() as i64;
    if let Err(error) = result {
        if error.idle {
            turn.window.send("chat-state", json!({"state": "idle"}));
        }
        meter.failed(turn.db, turn.workload, usage, latency_ms);
        return Err(error.message);
    }
    // The last round runs no tool, on every wire; nor does a round with a
    // call whose arguments were cut off part-way.
    let unfinished = round.calls.iter().filter(|call| !whole(&call.arguments)).count();
    if unfinished > 0 {
        log::warn!("JARVIS tool calls not run: {unfinished} with arguments cut off part-way");
    }
    let runs = !final_round && unfinished == 0 && wire.runs_calls(conversation, &round);
    meter.request(turn.db, turn.workload, usage, if runs { round.calls.len() } else { 0 }, final_round, latency_ms);
    Ok((round, runs))
}

/// Whether a call's arguments came whole: none at all (a call that takes
/// none), or one JSON object. Arguments cut off part-way are neither.
fn whole(arguments: &str) -> bool {
    arguments.trim().is_empty() || serde_json::from_str::<Value>(arguments).is_ok_and(|arguments| arguments.is_object())
}

/// The first request of a chat turn as `chat` sends it, with its usage
/// stored, and its calls not run: for tests of a wire's usage rows.
#[cfg(test)]
pub(crate) async fn first_request<W: Wire>(wire: &W, chat_request: ChatRequest, db: &crate::db::Database, window: &(dyn Window + Sync)) -> Result<Round, String> {
    let google_auth = std::sync::Arc::new(crate::auth::google::GoogleAuth::new_empty());
    let ledger = SideEffectLedger::new();
    let turn = Turn { db, google_auth: &google_auth, window, voice: None, workload: Workload::Tools, ledger: &ledger };
    let mut meter = Meter::new(wire.metering());
    let tools = chat_request.tools.clone();
    let mut conversation = wire.start(chat_request).await?;
    one_request(wire, &mut conversation, &tools, false, &turn, &mut meter).await.map(|(round, _)| round)
}

/// The chat window is told the answer is complete.
fn finish<W: Wire>(wire: &W, turn: &Turn<'_>) {
    turn.window.send("chat-token", json!({"token": "", "done": true}));
    if wire.shows_thinking() {
        turn.window.send("chat-thinking", json!({"text": "", "done": true}));
    }
}

/// Run a round's calls in order. The same tool with the same arguments runs
/// once (a model can ask for one open_url four times); calls that differ all
/// run. Each call goes through `tools::execute_tool`, which asks the user
/// first where an action needs it.
async fn run_calls(calls: &[Call], turn: &Turn<'_>) -> Vec<(Call, Outcome)> {
    let mut ran: HashSet<String> = HashSet::new();
    let mut narrated = false;
    let mut results = Vec::new();
    for call in calls {
        if !ran.insert(super::tool_call_key(&call.name, &call.arguments)) {
            log::info!("JARVIS skipping duplicate tool call: {}", call.name);
            results.push((call.clone(), Outcome::Duplicate));
            continue;
        }
        log::info!("JARVIS tool call: {} ({} bytes of arguments)", call.name, call.arguments.len());
        turn.window.send(
            "chat-status",
            json!({
                "status": format!("Running: {}", tools::tool_status_label(&call.name)),
                "detail": tools::tool_status_detail(&call.name, &call.arguments),
                "phase": "acting"
            }),
        );
        if !narrated {
            if let Some(voice) = turn.voice.as_ref() {
                // Spoken at once; this also flushes the text spoken so far.
                let narration = tools::tool_voice_narration(&call.name, &tools::read_language_pref(turn.db));
                let _ = voice.try_send(TtsCommand::Narrate(narration.to_string()));
                narrated = true;
            }
        }
        turn.ledger.note(&call.name);
        let result = tools::execute_tool(&call.name, &call.arguments, turn.db, turn.google_auth, turn.ledger).await;
        // The founder's rule: the tool's name and the result's length, none of
        // the result (it can hold mail, events or memories).
        log::info!("JARVIS tool result: {} ({} bytes)", call.name, result.len());
        results.push((call.clone(), Outcome::Ran(result)));
    }
    results
}
