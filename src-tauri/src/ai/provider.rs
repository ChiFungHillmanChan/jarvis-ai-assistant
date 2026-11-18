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
    PerTurn { provider: String, model: String },
    /// One row per turn, only when the turn succeeds (a model on this Mac:
    /// free, but each turn counts).
    PerTurnOnSuccess { provider: String, model: String },
    /// One row per request, as soon as it is answered, against the model id
    /// it was sent with (Bedrock, the founder's P02 rule).
    PerRequest { provider: String, model: String },
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

    /// Whether the round's calls run. The last round runs none.
    fn runs_calls(&self, _conversation: &Self::Conversation, round: &Round, final_round: bool) -> bool {
        !round.calls.is_empty() && !final_round
    }

    /// Add the round's reply and the outcome of each of its calls, for the
    /// next request. `next_is_final` is true when the next request is the
    /// last one allowed.
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
            Metering::PerTurn { provider, model } | Metering::PerTurnOnSuccess { provider, model } => Some(super::cost::TurnUsage::new(provider, model)),
            Metering::PerRequest { .. } => None,
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
        let Metering::PerRequest { provider, model } = &self.metering else { return };
        let disabled = crate::chat_session::TOOL_EXECUTION_DISABLED.try_with(|v| *v).unwrap_or(false);
        if calls > 0 && !final_round && !disabled {
            super::cost::note_tool_call();
        }
        match usage {
            Some(usage) => super::cost::record_inference(db, provider, super::cost::current_feature(), workload, model, usage.input, usage.output, usage.cache_read, usage.cache_write, latency_ms),
            None => log::warn!("{provider} reply carried no usage figures; no usage row stored"),
        }
    }

    /// A request failed part-way: its usage so far, then what the turn was
    /// billed.
    fn failed(mut self, db: &crate::db::Database, workload: Workload, usage: Option<Usage>) {
        let latency = self.elapsed();
        // A turn on this Mac is stored only when it succeeds, and a failed
        // Bedrock request was not answered, so nothing is stored for it.
        if let (Metering::PerTurn { .. }, Some(mut turn)) = (&self.metering, self.turn.take()) {
            if let Some(usage) = usage {
                turn.add_request(usage.input, usage.output, usage.cache_read, usage.cache_write);
            }
            turn.record_if_billed(db, workload, latency);
        }
    }

    fn succeeded(mut self, db: &crate::db::Database, workload: Workload) {
        let latency = self.elapsed();
        if let Some(turn) = self.turn.take() {
            turn.record(db, workload, latency);
        }
    }
}

/// One chat turn on `wire`: rounds of requests and tool calls until the
/// model answers or the last round is reached.
pub(crate) async fn chat<W: Wire>(wire: &W, request: ChatRequest, turn: Turn<'_>) -> Result<String, String> {
    let mut meter = Meter::new(wire.metering());
    let mut conversation = wire.start(request).await?;
    let max_rounds = wire.max_rounds();
    for number in 0..max_rounds {
        let final_round = number + 1 == max_rounds;
        let sent_at = std::time::Instant::now();
        let mut events = Events::new(turn.window, turn.voice.as_ref());
        let result = wire.round(&mut conversation, final_round, &mut events).await;
        let (round, usage) = events.finish();
        if let Err(error) = result {
            if error.idle {
                turn.window.send("chat-state", json!({"state": "idle"}));
            }
            meter.failed(turn.db, turn.workload, usage);
            return Err(error.message);
        }
        let runs = wire.runs_calls(&conversation, &round, final_round);
        meter.request(turn.db, turn.workload, usage, round.calls.len(), final_round, sent_at.elapsed().as_millis() as i64);
        if runs {
            let results = run_calls(&round.calls, &turn).await;
            let answers = wire.answers_after_tools(&round);
            let text = round.text();
            wire.add_results(&mut conversation, round, results, number + 2 == max_rounds)?;
            if answers {
                finish(wire, &turn);
                meter.succeeded(turn.db, turn.workload);
                return Ok(text);
            }
            turn.window.send("chat-status", json!({"status": "Reviewing tool results...", "phase": "thinking"}));
            continue;
        }
        finish(wire, &turn);
        meter.succeeded(turn.db, turn.workload);
        return wire.reply(&conversation, round, final_round, &tools::read_language_pref(turn.db));
    }
    // Only when the registry allows no round at all.
    finish(wire, &turn);
    meter.succeeded(turn.db, turn.workload);
    Ok(tools::tool_limit_fallback(&tools::read_language_pref(turn.db)).into())
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
        log::info!("JARVIS tool result: {}", crate::util::truncate_bytes(&result, 200));
        results.push((call.clone(), Outcome::Ran(result)));
    }
    results
}
