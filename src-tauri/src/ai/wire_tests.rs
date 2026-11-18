//! Each chat wire pinned through `AiRouter::send_one` (P07): the request it
//! sends, how it reads a recorded reply, what the chat window and the voice
//! get, the tool round and its approval, the usage row and the error text.
//! The replies are fixtures written from each provider's documented stream
//! format and served from a loopback address: nothing leaves this Mac.

use super::*;
use crate::chat_session::ScopedEmitter;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

thread_local! {
    /// Where [`api_base`] sends this thread's requests instead of the
    /// provider's own address.
    pub(super) static API_BASE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Points this thread's provider requests at `base` until dropped.
pub(crate) struct PointedAt;

impl Drop for PointedAt {
    fn drop(&mut self) {
        API_BASE.with(|base| *base.borrow_mut() = None);
        crate::ai::bedrock::tests::LOOPBACK.with(|endpoint| *endpoint.borrow_mut() = None);
    }
}

pub(crate) fn point_at(base: &str) -> PointedAt {
    API_BASE.with(|slot| *slot.borrow_mut() = Some(base.to_string()));
    PointedAt
}

/// Points this thread's Bedrock clients at `base` until dropped.
pub(crate) fn point_bedrock_at(base: &str) -> PointedAt {
    crate::ai::bedrock::tests::LOOPBACK.with(|endpoint| *endpoint.borrow_mut() = Some(base.to_string()));
    PointedAt
}

/// One reply the loopback server sends.
pub(crate) struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    cut: bool,
}

impl Reply {
    pub(crate) fn json(status: u16, body: Value) -> Self {
        Reply { status, headers: vec![("Content-Type".into(), "application/json".into())], body: body.to_string().into_bytes(), cut: false }
    }

    pub(crate) fn raw(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Reply { status, headers: vec![("Content-Type".into(), content_type.into())], body, cut: false }
    }

    /// A server-sent event stream, one `data:` line per event.
    pub(crate) fn sse(events: &[Value]) -> Self {
        let body: String = events.iter().map(|event| format!("data: {event}\n\n")).collect();
        Reply::raw(200, "text/event-stream", body.into_bytes())
    }

    /// OpenAI's format: the stream ends with `data: [DONE]`.
    pub(crate) fn openai_sse(events: &[Value]) -> Self {
        let mut body: String = events.iter().map(|event| format!("data: {event}\n\n")).collect();
        body.push_str("data: [DONE]\n\n");
        Reply::raw(200, "text/event-stream", body.into_bytes())
    }

    /// Anthropic's format: every event is named.
    pub(crate) fn named_sse(events: &[(&str, Value)]) -> Self {
        let body: String = events.iter().map(|(name, event)| format!("event: {name}\ndata: {event}\n\n")).collect();
        Reply::raw(200, "text/event-stream", body.into_bytes())
    }

    pub(crate) fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The connection drops before the body announced is complete.
    pub(crate) fn cut_short(mut self) -> Self {
        self.cut = true;
        self
    }
}

/// One request the loopback server received.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub method: String,
    /// Percent-decoded where the path carries a model id (":").
    pub path: String,
    headers: Vec<(String, String)>,
    pub body: Value,
}

impl Seen {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }
}

/// A loopback stand-in for a provider's API: it answers one request per
/// reply, in order, and keeps what each request carried.
pub(crate) struct Loopback {
    pub base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Loopback {
    /// The requests received so far, in order.
    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) async fn serve(replies: Vec<Reply>) -> Loopback {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    let task = tokio::spawn(async move {
        for reply in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 16 * 1024];
            let (head_end, length) = loop {
                let count = stream.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&request[..end]).to_string();
                    let length = head.lines().find_map(|line| line.to_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break (end, length);
                    }
                }
                assert!(count > 0, "the request ended early");
            };
            let head = String::from_utf8_lossy(&request[..head_end]).to_string();
            let mut lines = head.lines();
            let first = lines.next().unwrap_or_default().to_string();
            let mut parts = first.split(' ');
            let method = parts.next().unwrap_or_default().to_string();
            let path = parts.next().unwrap_or_default().replace("%3A", ":").replace("%3a", ":");
            let headers = lines.filter_map(|line| line.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))).collect();
            let body_bytes = &request[head_end + 4..head_end + 4 + length];
            let body = serde_json::from_slice(body_bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body_bytes).to_string()));
            kept.lock().unwrap().push(Seen { method, path, headers, body });

            let reason = if reply.status < 400 { "OK" } else { "Error" };
            let announced = if reply.cut { reply.body.len() + 64 } else { reply.body.len() };
            let mut out = format!("HTTP/1.1 {} {reason}\r\n", reply.status);
            for (name, value) in &reply.headers {
                out.push_str(&format!("{name}: {value}\r\n"));
            }
            out.push_str(&format!("Content-Length: {announced}\r\nConnection: close\r\n\r\n"));
            let _ = stream.write_all(out.as_bytes()).await;
            let _ = stream.write_all(&reply.body).await;
            let _ = stream.flush().await;
            let _ = stream.shutdown().await;
        }
    });
    Loopback { base, seen, task }
}

/// The chat window: every event a turn emits, in order.
#[derive(Default)]
pub(crate) struct Window(Mutex<Vec<(String, Value)>>);

impl ScopedEmitter for Window {
    fn emit<S: serde::Serialize + Clone>(&self, event: &str, payload: S) -> tauri::Result<()> {
        self.0.lock().unwrap().push((event.to_string(), serde_json::to_value(payload).unwrap()));
        Ok(())
    }
}

/// What one turn did.
pub(crate) struct Turn {
    pub reply: Result<String, String>,
    pub window: Vec<(String, Value)>,
    /// What went to the voice: "text: ..." chunks and "narrate: ..." lines.
    pub voice: Vec<String>,
    pub ledger: SideEffectLedger,
}

impl Turn {
    /// The events, with each payload, as `name payload` lines.
    pub(crate) fn window_lines(&self) -> Vec<String> {
        self.window.iter().map(|(name, payload)| format!("{name} {payload}")).collect()
    }
}

/// One chat turn through `send_one`, as chat sends it: under the chat
/// feature, with a voice channel, and the approval card answered `approve`.
pub(crate) async fn run_turn(router: &AiRouter, entry: ProviderChainEntry, messages: &[(&str, &str)], db: &crate::db::Database, workload: Workload, approve: bool) -> Turn {
    run_turn_as(router, entry, messages, db, workload, approve, cost::Feature::Chat, false).await
}

/// [`run_turn`] under another feature, with tools turned off when
/// `tools_off` (as the briefing's classification sends it).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_turn_as(router: &AiRouter, entry: ProviderChainEntry, messages: &[(&str, &str)], db: &crate::db::Database, workload: Workload, approve: bool, feature: cost::Feature, tools_off: bool) -> Turn {
    let window = Window::default();
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let ledger = SideEffectLedger::new();
    let auth = Arc::new(crate::auth::google::GoogleAuth::new_empty());
    let messages = messages.iter().map(|(role, text)| (role.to_string(), text.to_string())).collect();
    let reply = crate::chat_session::TOOL_EXECUTION_DISABLED
        .scope(tools_off, crate::tool_approval::TEST_ANSWER.scope(approve, cost::with_feature(feature, router.send_one(entry, messages, db, &auth, &window, Some(tx), workload, &ledger))))
        .await;
    let mut voice = Vec::new();
    while let Ok(command) = rx.try_recv() {
        voice.push(match command {
            TtsCommand::TextChunk(text) => format!("text: {text}"),
            TtsCommand::Narrate(text) => format!("narrate: {text}"),
            TtsCommand::Done => "done".to_string(),
        });
    }
    let window = window.0.into_inner().unwrap();
    Turn { reply, window, voice, ledger }
}

/// A stored usage row.
#[derive(Debug, PartialEq)]
pub(crate) struct Row {
    pub provider: String,
    pub feature: String,
    pub model: String,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub cost: Option<i64>,
    pub price_unknown: bool,
    pub registry_version: i64,
}

// The whole module is test only; the attribute repeats it for the usage
// guard (ai/usage.rs), which reads each file's text for code outside tests.
#[cfg(test)]
pub(crate) fn rows(db: &crate::db::Database) -> Vec<Row> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT provider, feature, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, usd_cost_micro, price_unknown, registry_version FROM inference_metrics ORDER BY rowid")
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(Row {
                provider: r.get(0)?,
                feature: r.get(1)?,
                model: r.get(2)?,
                input: r.get(3)?,
                output: r.get(4)?,
                cache_read: r.get(5)?,
                cache_write: r.get(6)?,
                cost: r.get(7)?,
                price_unknown: r.get::<_, i64>(8)? != 0,
                registry_version: r.get(9)?,
            })
        })
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

/// The row a priced cloud request of these figures is stored as.
#[allow(clippy::too_many_arguments)]
pub(crate) fn priced_row(provider: &str, feature: &str, model: &str, requests: &[(i64, i64, i64, i64)]) -> Row {
    let registry = crate::ai::registry::current();
    let mut row = Row { provider: provider.into(), feature: feature.into(), model: model.into(), input: 0, output: 0, cache_read: 0, cache_write: 0, cost: Some(0), price_unknown: false, registry_version: registry.version as i64 };
    for &(input, output, cache_read, cache_write) in requests {
        row.input += input;
        row.output += output;
        row.cache_read += cache_read;
        row.cache_write += cache_write;
        match registry.usd_cost_micro(provider, model, input, output, cache_read, cache_write) {
            Some(micro) => row.cost = row.cost.map(|sum| sum + micro),
            None => {
                row.cost = None;
                row.price_unknown = true;
            }
        }
    }
    row
}

/// The system prompt a chat request carries: the app's prompt, with the
/// per-request part (the time) after the cacheable part.
pub(crate) fn assert_app_system_prompt(text: &str) {
    let cacheable = tools::build_cacheable_system_prompt();
    assert!(text.starts_with(&format!("{cacheable}\n\n")), "the system prompt starts with the cacheable prompt");
    assert!(text.contains("Current local time: "), "the per-request part follows");
}

pub(crate) fn memories(db: &crate::db::Database) -> Vec<String> {
    let conn = db.conn.lock().unwrap();
    let mut stmt = conn.prepare("SELECT content FROM user_memory ORDER BY id").unwrap();
    let rows = stmt.query_map([], |r| r.get(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn fallback(db: &crate::db::Database) -> String {
    tools::tool_limit_fallback(&tools::read_language_pref(db)).to_string()
}

const SKIPPED: &str = "Skipped: already executed this tool with these arguments in this batch.";
const REMEMBER_ARGS: &str = r#"{"content":"Kenny is my accountant"}"#;

fn status(text: &str, phase: &str) -> String {
    format!("chat-status {}", json!({"status": text, "phase": phase}))
}

fn running(label: &str, detail: Value) -> String {
    format!("chat-status {}", json!({"status": format!("Running: {label}"), "detail": detail, "phase": "acting"}))
}

fn token(text: &str) -> String {
    format!("chat-token {}", json!({"token": text, "done": false}))
}

fn token_done() -> String {
    format!("chat-token {}", json!({"token": "", "done": true}))
}

fn tool_call(name: &str) -> String {
    format!("chat-tool-call {}", json!({"tool_name": name}))
}

fn planning(name: &str) -> String {
    status(&format!("Planning: {}", tools::tool_status_label(name)), "planning")
}

/// The result a remember call returns once the user allowed it.
async fn remembered(content: &str) -> String {
    let db = crate::db::test_database();
    let auth = Arc::new(crate::auth::google::GoogleAuth::new_empty());
    let args = json!({"content": content}).to_string();
    crate::tool_approval::TEST_ANSWER.scope(true, tools::execute_tool("remember", &args, &db, &auth, &SideEffectLedger::new())).await
}

// ---------------------------------------------------------------------------
// OpenAI (api.openai.com) and OpenAI-compatible services
// ---------------------------------------------------------------------------

mod openai_wire {
    use super::*;

    fn official() -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "openai".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None }
    }

    fn router() -> AiRouter {
        let mut router = AiRouter::new("gemini_primary");
        router.set_openai_key("sk-fixture-openai-not-real");
        router
    }

    /// The body with the system prompt and the tool list checked and
    /// replaced by markers.
    fn marked(body: &Value, tools_sent: bool) -> Value {
        let mut body = body.clone();
        let system = &body["messages"][0];
        assert_eq!(system["role"], "system");
        assert_app_system_prompt(system["content"].as_str().unwrap());
        body["messages"][0]["content"] = json!("<system>");
        if tools_sent {
            assert_eq!(body["tools"], json!(openai_tools()), "every tool, in OpenAI's function format");
            body["tools"] = json!("<tools>");
        }
        body
    }

    pub(crate) fn openai_tools() -> Vec<Value> {
        tools::get_tool_definitions().iter().map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}})).collect()
    }

    fn text_reply() -> Reply {
        Reply::openai_sse(&[
            json!({"choices": [{"delta": {"role": "assistant", "content": "Hel"}}]}),
            json!({"choices": [{"delta": {"content": "lo."}, "finish_reason": "stop"}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 1200, "completion_tokens": 30, "prompt_tokens_details": {"cached_tokens": 1000}}}),
        ])
    }

    /// A plain answer: the request, the streamed tokens, the voice, one
    /// priced row with the cached tokens.
    #[tokio::test]
    async fn a_text_turn() {
        let server = serve(vec![text_reply()]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Hi"), ("assistant", "  "), ("user", "What is on today?")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Hello."));

        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", "/v1/chat/completions"));
        assert_eq!(seen[0].header("authorization"), Some("Bearer sk-fixture-openai-not-real"));
        assert_eq!(seen[0].header("content-type"), Some("application/json"));
        let model = crate::ai::models::openai_default();
        assert_eq!(
            marked(&seen[0].body, true),
            json!({
                "model": model,
                "messages": [{"role": "system", "content": "<system>"}, {"role": "user", "content": "Hi"}, {"role": "user", "content": "What is on today?"}],
                "tools": "<tools>",
                "max_completion_tokens": 4096,
                "reasoning_effort": "none",
                "stream": true,
                "stream_options": {"include_usage": true}
            })
        );
        assert_eq!(turn.window_lines(), vec![status("Composing response...", "responding"), token("Hel"), token("lo."), token_done()]);
        assert_eq!(turn.voice, vec!["text: Hel", "text: lo."]);
        assert_eq!(rows(&db), vec![priced_row("openai", "chat", &model, &[(1200, 30, 1000, 0)])]);
        assert_eq!(turn.ledger.count(), 0);
    }

    /// A tool round: the call streams in pieces, the same call twice runs
    /// once, the approval card is answered, the results go back with their
    /// ids, and the turn is one row under tools.
    #[tokio::test]
    async fn a_tool_turn() {
        let server = serve(vec![
            Reply::openai_sse(&[
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "remember", "arguments": ""}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "{\"content\":"}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "\"Kenny is my accountant\"}"}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 1, "id": "call_2", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "tool_calls"}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10}}),
            ]),
            Reply::openai_sse(&[
                json!({"choices": [{"delta": {"content": "Saved."}, "finish_reason": "stop"}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 150, "completion_tokens": 5}}),
            ]),
        ])
        .await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        assert_eq!(turn.ledger.count(), 1, "the call that ran is in the ledger");

        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        let result = remembered("Kenny is my accountant").await;
        let second = marked(&seen[1].body, true);
        assert_eq!(second["tool_choice"], Value::Null, "not the last round");
        assert_eq!(
            second["messages"],
            json!([
                {"role": "system", "content": "<system>"},
                {"role": "user", "content": "Remember Kenny is my accountant"},
                {"role": "assistant", "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}},
                    {"id": "call_2", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}
                ]},
                {"role": "tool", "content": result, "tool_call_id": "call_1"},
                {"role": "tool", "content": SKIPPED, "tool_call_id": "call_2"}
            ])
        );
        assert_eq!(
            turn.window_lines(),
            vec![
                planning("remember"),
                tool_call("remember"),
                planning("remember"),
                tool_call("remember"),
                running(tools::tool_status_label("remember"), json!(tools::tool_status_detail("remember", REMEMBER_ARGS))),
                status("Reviewing tool results...", "thinking"),
                status("Composing response...", "responding"),
                token("Saved."),
                token_done(),
            ]
        );
        assert_eq!(turn.voice, vec![format!("narrate: {}", tools::tool_voice_narration("remember", &tools::read_language_pref(&db))), "text: Saved.".into()]);
        let model = crate::ai::models::openai_default();
        assert_eq!(rows(&db), vec![priced_row("openai", "tools", &model, &[(100, 10, 0, 0), (150, 5, 0, 0)])]);
    }

    /// A declined approval: the tool does not run, and the model is told.
    #[tokio::test]
    async fn a_declined_tool() {
        let server = serve(vec![
            Reply::openai_sse(&[json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "tool_calls"}]})]),
            Reply::openai_sse(&[json!({"choices": [{"delta": {"content": "Not saved."}, "finish_reason": "stop"}]})]),
        ])
        .await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Not saved."));
        assert!(memories(&db).is_empty());
        let seen = server.seen();
        let refusal = seen[1].body["messages"][3]["content"].as_str().unwrap().to_string();
        assert!(refusal.starts_with("The user did not allow remember"), "{refusal}");
        // A reply without usage figures is stored as a row of zeros.
        let model = crate::ai::models::openai_default();
        assert_eq!(rows(&db), vec![priced_row("openai", "tools", &model, &[(0, 0, 0, 0), (0, 0, 0, 0)])]);
    }

    /// The last allowed round sends tool_choice none, and a call the model
    /// still asks for there is not run: the turn ends with the fallback.
    #[tokio::test]
    async fn the_last_round() {
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "openai").unwrap().limits.max_tool_rounds = Some(2));
        let call = |id: &str| Reply::openai_sse(&[json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": id, "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "tool_calls"}]})]);
        let server = serve(vec![call("call_1"), call("call_2")]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply, Ok(fallback(&db)));
        assert_eq!(memories(&db).len(), 1, "the last round runs no tool");
        // `remember` refreshes a fact it already has, so only the ledger
        // tells whether the last round's call ran.
        assert_eq!(turn.ledger.count(), 1, "the last round runs no tool");
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].body.get("tool_choice"), None);
        assert_eq!(seen[1].body["tool_choice"], "none");
        assert_eq!(turn.window_lines().last().unwrap(), &token_done());
    }

    /// A refused request: the status and the body in the error, which the
    /// router turns into the error card; nothing stored.
    #[tokio::test]
    async fn a_refused_request() {
        let server = serve(vec![Reply::json(401, json!({"error": {"message": "Incorrect API key provided", "code": "invalid_api_key"}}))]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(
            turn.reply,
            Err(format!("OpenAI API error 401 Unauthorized: {}", json!({"error": {"message": "Incorrect API key provided", "code": "invalid_api_key"}})))
        );
        assert_eq!(server.seen().len(), 1);
        assert!(rows(&db).is_empty());
        assert!(turn.window.is_empty());
    }

    /// A 503 is sent once more after the wait the service asks for, on the
    /// same model; a second 503 ends the turn.
    #[tokio::test]
    async fn a_busy_service_is_asked_once_more() {
        let busy = || Reply::json(503, json!({"error": {"message": "overloaded"}})).header("Retry-After", "0");
        let server = serve(vec![busy(), busy()]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err(format!("OpenAI API error 503 Service Unavailable: {}", json!({"error": {"message": "overloaded"}}))));
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].body, seen[1].body, "the same request");

        let server = serve(vec![busy(), text_reply()]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let turn = run_turn(&router(), official(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Hello."));
    }

    /// A stream that breaks off: the window goes idle, what was billed is
    /// stored, and the error names a stream error.
    #[tokio::test]
    async fn a_broken_stream() {
        let body = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":40,\"completion_tokens\":2}}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n";
        let server = serve(vec![Reply::raw(200, "text/event-stream", body.as_bytes().to_vec()).cut_short()]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        let error = turn.reply.clone().unwrap_err();
        assert!(error.starts_with("Stream error: "), "{error}");
        assert_eq!(turn.window_lines().last().unwrap(), &format!("chat-state {}", json!({"state": "idle"})));
        let model = crate::ai::models::openai_default();
        assert_eq!(rows(&db), vec![priced_row("openai", "chat", &model, &[(40, 2, 0, 0)])]);
    }

    fn compat(base: &str, model: Option<&str>) -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "openai".into(), endpoint_id: None, model_id: model.map(str::to_string), enabled: true, base_url: Some(base.to_string()) }
    }

    /// A compatible service: its own address, `max_tokens` and no reasoning
    /// field, its provider id in the row, calls without an index slotted by
    /// id (Gemini's compatible endpoint), and their extra content sent back.
    #[tokio::test]
    async fn a_compatible_service() {
        let server = serve(vec![
            Reply::openai_sse(&[
                json!({"choices": [{"delta": {"tool_calls": [{"id": "c1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}, "extra_content": {"google": {"thought_signature": "sig-1"}}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"id": "c2", "type": "function", "function": {"name": "list_memories", "arguments": "{}"}}]}, "finish_reason": "stop"}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 70, "completion_tokens": 7}}),
            ]),
            Reply::openai_sse(&[json!({"choices": [{"delta": {"content": "Done."}, "finish_reason": "stop"}]})]),
        ])
        .await;
        let base = format!("{}/v1", server.base);
        let mut router = AiRouter::new("gemini_primary");
        router.set_compat_key(&base, "sk-fixture-compat-not-real");
        let db = crate::db::test_database();
        let turn = run_turn(&router, compat(&base, Some("unlisted-chat-1")), &[("user", "Remember Kenny")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Done."));
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].path, "/v1/chat/completions");
        assert_eq!(seen[0].header("authorization"), Some("Bearer sk-fixture-compat-not-real"));
        let first = marked(&seen[0].body, true);
        assert_eq!(
            first,
            json!({
                "model": "unlisted-chat-1",
                "messages": [{"role": "system", "content": "<system>"}, {"role": "user", "content": "Remember Kenny"}],
                "tools": "<tools>",
                "max_tokens": 4096,
                "stream": true,
                "stream_options": {"include_usage": true}
            })
        );
        let calls = &seen[1].body["messages"][2]["tool_calls"];
        assert_eq!(calls[0]["id"], "c1");
        assert_eq!(calls[0]["extra_content"], json!({"google": {"thought_signature": "sig-1"}}));
        assert_eq!(calls[1]["id"], "c2");
        assert_eq!(calls[1]["function"]["name"], "list_memories");
        assert_eq!(seen[1].body["messages"][4]["tool_call_id"], "c2");
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        let provider = cost::provider_id("openai", Some(&base));
        assert_eq!(rows(&db), vec![priced_row(&provider, "tools", "unlisted-chat-1", &[(70, 7, 0, 0), (0, 0, 0, 0)])]);
    }

    /// A compatible service needs a chosen model; its errors carry its name.
    #[tokio::test]
    async fn a_compatible_service_without_a_model_or_refusing() {
        let server = serve(vec![Reply::json(400, json!({"error": {"message": "bad model"}}))]).await;
        let base = format!("{}/v1", server.base);
        let mut router = AiRouter::new("gemini_primary");
        router.set_compat_key(&base, "sk-fixture-compat-not-real");
        let db = crate::db::test_database();
        let turn = run_turn(&router, compat(&base, None), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err(format!("尚未為 {} 選擇模型，請在設定中選擇。", compat::label(&base))));
        assert!(server.seen().is_empty());
        let turn = run_turn(&router, compat(&base, Some("unlisted-chat-1")), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err(format!("{} API error 400 Bad Request: {}", compat::label(&base), json!({"error": {"message": "bad model"}}))));
        assert!(rows(&db).is_empty());
    }

    /// Calls in a reply cut off by the length limit may be half written:
    /// they do not run, and the text so far is the answer.
    #[tokio::test]
    async fn calls_cut_off_by_the_length_limit_do_not_run() {
        let server = serve(vec![Reply::openai_sse(&[
            json!({"choices": [{"delta": {"content": "Let me save that."}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "length"}]}),
        ])])
        .await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Let me save that."));
        assert_eq!(turn.ledger.count(), 0);
        assert!(memories(&db).is_empty());
        assert_eq!(server.seen().len(), 1);
    }

    /// The text a tool round streamed goes back with its calls.
    #[tokio::test]
    async fn the_text_of_a_tool_round_goes_back_with_its_calls() {
        let server = serve(vec![
            Reply::openai_sse(&[
                json!({"choices": [{"delta": {"content": "Let me save that."}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "tool_calls"}]}),
            ]),
            Reply::openai_sse(&[json!({"choices": [{"delta": {"content": "Saved."}, "finish_reason": "stop"}]})]),
        ])
        .await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[1].body["messages"][2],
            json!({"role": "assistant", "content": "Let me save that.", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]})
        );
    }

    /// A turn that fails after a tool round stores what the earlier round
    /// was billed, as one row under tools.
    #[tokio::test]
    async fn a_failure_after_a_tool_round_stores_the_earlier_round() {
        let server = serve(vec![
            Reply::openai_sse(&[
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "tool_calls"}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10}}),
            ]),
            Reply::json(400, json!({"error": {"message": "Invalid request", "code": "invalid_request_error"}})),
        ])
        .await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert!(turn.reply.is_err(), "{:?}", turn.reply);
        assert_eq!(turn.ledger.count(), 1);
        let model = crate::ai::models::openai_default();
        assert_eq!(rows(&db), vec![priced_row("openai", "tools", &model, &[(100, 10, 0, 0)])]);
    }

    /// Two different calls in one round both run, and the voice says what
    /// it is doing once.
    #[tokio::test]
    async fn one_narration_per_round() {
        let other = r#"{"content":"Mary is my dentist"}"#;
        let server = serve(vec![
            Reply::openai_sse(&[
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 1, "id": "call_2", "type": "function", "function": {"name": "remember", "arguments": other}}]}, "finish_reason": "tool_calls"}]}),
            ]),
            Reply::openai_sse(&[json!({"choices": [{"delta": {"content": "Saved."}, "finish_reason": "stop"}]})]),
        ])
        .await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Remember Kenny and Mary")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(memories(&db).len(), 2);
        assert_eq!(turn.ledger.count(), 2);
        let narration = tools::tool_voice_narration("remember", &tools::read_language_pref(&db));
        assert_eq!(turn.voice, vec![format!("narrate: {narration}"), "text: Saved.".into()]);
    }

    /// The fallback on an empty last round is shown and spoken in one piece,
    /// as Bedrock did before P07 (P07: on every wire).
    #[tokio::test]
    async fn the_fallback_is_shown_and_spoken() {
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "openai").unwrap().limits.max_tool_rounds = Some(2));
        let server = serve(vec![
            Reply::openai_sse(&[json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "tool_calls"}]})]),
            Reply::openai_sse(&[json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})]),
        ])
        .await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), official(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        let fallback = fallback(&db);
        assert_eq!(turn.reply.as_ref(), Ok(&fallback));
        let lines = turn.window_lines();
        assert_eq!(&lines[lines.len() - 3..], &[status("Composing response...", "responding"), token(&fallback), token_done()]);
        assert_eq!(turn.voice.last(), Some(&format!("text: {fallback}")));
    }

    /// A reply without usage figures is stored as zeros, and the log says
    /// so, with none of the reply.
    #[tokio::test]
    async fn a_reply_without_usage_figures_is_logged() {
        let server = serve(vec![Reply::openai_sse(&[json!({"choices": [{"delta": {"content": "Your dentist is Mary."}, "finish_reason": "stop"}]})])]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let db = crate::db::test_database();
        let logged = crate::test_log::capture(|| async {
            run_turn(&router(), official(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        })
        .await;
        let model = crate::ai::models::openai_default();
        assert!(logged.contains(&format!("[openai] no usage figures received for a non-empty response; recording zero tokens (model={model})")), "{logged:?}");
        assert!(!logged.iter().any(|line| line.contains("dentist")), "{logged:?}");
        assert_eq!(rows(&db), vec![priced_row("openai", "chat", &model, &[(0, 0, 0, 0)])]);
    }
}

// ---------------------------------------------------------------------------
// Claude (api.anthropic.com)
// ---------------------------------------------------------------------------

mod claude_wire {
    use super::*;

    fn row() -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "claude".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None }
    }

    fn router() -> AiRouter {
        let mut router = AiRouter::new("gemini_primary");
        router.set_claude_key("sk-ant-fixture-not-real");
        router
    }

    fn marked(body: &Value) -> Value {
        let mut body = body.clone();
        assert_app_system_prompt(body["system"].as_str().unwrap());
        body["system"] = json!("<system>");
        let expected: Vec<Value> = tools::get_tool_definitions().iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.parameters})).collect();
        assert_eq!(body["tools"], json!(expected), "every tool, in Anthropic's format");
        body["tools"] = json!("<tools>");
        body
    }

    fn start(input: i64, cache_read: i64, cache_write: i64) -> (&'static str, Value) {
        ("message_start", json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [], "usage": {"input_tokens": input, "cache_read_input_tokens": cache_read, "cache_creation_input_tokens": cache_write, "output_tokens": 1}}}))
    }

    fn text_block(index: usize) -> (&'static str, Value) {
        ("content_block_start", json!({"type": "content_block_start", "index": index, "content_block": {"type": "text", "text": ""}}))
    }

    fn text(index: usize, text: &str) -> (&'static str, Value) {
        ("content_block_delta", json!({"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": text}}))
    }

    fn stop(index: usize) -> (&'static str, Value) {
        ("content_block_stop", json!({"type": "content_block_stop", "index": index}))
    }

    fn end(reason: &str, output: i64) -> Vec<(&'static str, Value)> {
        vec![
            ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": reason}, "usage": {"output_tokens": output}})),
            ("message_stop", json!({"type": "message_stop"})),
        ]
    }

    fn tool_use(index: usize, id: &str, pieces: &[&str]) -> Vec<(&'static str, Value)> {
        let mut events = vec![("content_block_start", json!({"type": "content_block_start", "index": index, "content_block": {"type": "tool_use", "id": id, "name": "remember", "input": {}}}))];
        events.extend(pieces.iter().map(|piece| ("content_block_delta", json!({"type": "content_block_delta", "index": index, "delta": {"type": "input_json_delta", "partial_json": piece}}))));
        events.push(stop(index));
        events
    }

    fn thinking_done() -> String {
        format!("chat-thinking {}", json!({"text": "", "done": true}))
    }

    #[tokio::test]
    async fn a_text_turn() {
        let mut events = vec![start(50, 900, 100), text_block(0), text(0, "Hi"), text(0, " there."), stop(0)];
        events.extend(end("end_turn", 12));
        let server = serve(vec![Reply::named_sse(&events)]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Hi"), ("assistant", " "), ("user", "What is on today?")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Hi there."));
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", "/v1/messages"));
        assert_eq!(seen[0].header("x-api-key"), Some("sk-ant-fixture-not-real"));
        assert_eq!(seen[0].header("anthropic-version"), Some(crate::ai::models::anthropic_version().as_str()));
        assert_eq!(seen[0].header("content-type"), Some("application/json"));
        let model = crate::ai::models::claude_default();
        assert_eq!(
            marked(&seen[0].body),
            json!({
                "model": model,
                "max_tokens": 4096,
                "system": "<system>",
                "tools": "<tools>",
                "messages": [{"role": "user", "content": "Hi"}, {"role": "user", "content": "What is on today?"}],
                "stream": true
            })
        );
        assert_eq!(turn.window_lines(), vec![status("Composing response...", "responding"), token("Hi"), token(" there."), token_done(), thinking_done()]);
        assert_eq!(turn.voice, vec!["text: Hi", "text:  there."]);
        // Anthropic's input_tokens leaves the cache out; the row counts it in.
        assert_eq!(rows(&db), vec![priced_row("claude", "chat", &model, &[(1050, 12, 900, 100)])]);
    }

    #[tokio::test]
    async fn a_tool_turn() {
        let mut first = vec![start(50, 900, 100), text_block(0), text(0, "Let me "), text(0, "save that."), stop(0)];
        first.extend(tool_use(1, "toolu_1", &["{\"content\":", "\"Kenny is my accountant\"}"]));
        first.extend(end("tool_use", 20));
        let mut second = vec![start(60, 900, 0), text_block(0), text(0, "Saved."), stop(0)];
        second.extend(end("end_turn", 4));
        let server = serve(vec![Reply::named_sse(&first), Reply::named_sse(&second)]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        assert_eq!(turn.ledger.count(), 1);
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        let result = remembered("Kenny is my accountant").await;
        let second = marked(&seen[1].body);
        assert_eq!(second.get("tool_choice"), None);
        // A turn without Think harder sends back the streamed text pieces
        // and the call; the results go in one user message.
        assert_eq!(
            second["messages"],
            json!([
                {"role": "user", "content": "Remember Kenny is my accountant"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Let me "},
                    {"type": "text", "text": "save that."},
                    {"type": "tool_use", "id": "toolu_1", "name": "remember", "input": {"content": "Kenny is my accountant"}}
                ]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": result}]}
            ])
        );
        assert_eq!(
            turn.window_lines(),
            vec![
                status("Composing response...", "responding"),
                token("Let me "),
                token("save that."),
                planning("remember"),
                tool_call("remember"),
                running(tools::tool_status_label("remember"), json!(tools::tool_status_detail("remember", REMEMBER_ARGS))),
                status("Reviewing tool results...", "thinking"),
                status("Composing response...", "responding"),
                token("Saved."),
                token_done(),
                thinking_done(),
            ]
        );
        let narration = tools::tool_voice_narration("remember", &tools::read_language_pref(&db));
        assert_eq!(turn.voice, vec!["text: Let me ".to_string(), "text: save that.".into(), format!("narrate: {narration}"), "text: Saved.".into()]);
        let model = crate::ai::models::claude_default();
        assert_eq!(rows(&db), vec![priced_row("claude", "tools", &model, &[(1050, 20, 900, 100), (960, 4, 900, 0)])]);
    }

    /// Calls run in the order the reply gave them, and go back in that
    /// order (P07: they ran in hash order before).
    #[tokio::test]
    async fn calls_run_in_the_order_they_came() {
        let mut first = vec![start(10, 0, 0)];
        first.extend(tool_use(0, "toolu_a", &[r#"{"content":"First fact"}"#]));
        first.extend(tool_use(1, "toolu_b", &[r#"{"content":"Second fact"}"#]));
        first.extend(tool_use(2, "toolu_c", &[r#"{"content":"Third fact"}"#]));
        first.extend(end("tool_use", 5));
        let mut second = vec![start(10, 0, 0), text_block(0), text(0, "Saved."), stop(0)];
        second.extend(end("end_turn", 4));
        let server = serve(vec![Reply::named_sse(&first), Reply::named_sse(&second)]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Remember three facts")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(memories(&db), vec!["First fact", "Second fact", "Third fact"]);
        let seen = server.seen();
        let ids = |message: &Value, field: &str| message["content"].as_array().unwrap().iter().map(|b| b[field].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(ids(&seen[1].body["messages"][1], "id"), ["toolu_a", "toolu_b", "toolu_c"]);
        assert_eq!(ids(&seen[1].body["messages"][2], "tool_use_id"), ["toolu_a", "toolu_b", "toolu_c"]);
    }

    /// Think harder: the thinking fields the registry says the model takes,
    /// the thinking shown as it streams, and the thinking block sent back
    /// with its signature before the call it led to.
    #[tokio::test]
    async fn a_think_harder_turn() {
        // Registry version 1 marks Claude as not thinking; a later version
        // can mark it so.
        let _registry = crate::ai::registry::test_override::with(|r| {
            for m in r.models.iter_mut().filter(|m| m.provider == "claude") {
                m.capabilities.thinking = true;
                m.capabilities.reasoning_effort = true;
            }
        });
        let deeper = capability::deeper(&row()).expect("Think harder is offered on Claude's default model");
        assert!(deeper.thinking && deeper.effort);
        let mut first = vec![
            start(10, 0, 0),
            ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}})),
            ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Considering."}})),
            ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig-1"}})),
            stop(0),
        ];
        first.extend(tool_use(1, "toolu_1", &[REMEMBER_ARGS]));
        first.extend(end("tool_use", 20));
        let mut second = vec![start(10, 0, 0), text_block(0), text(0, "Saved."), stop(0)];
        second.extend(end("end_turn", 4));
        let server = serve(vec![Reply::named_sse(&first), Reply::named_sse(&second)]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Premium, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        let seen = server.seen();
        for request in &seen {
            assert_eq!(request.body.get("thinking"), deeper.thinking.then(|| json!({"type": "adaptive"})).as_ref());
            assert_eq!(request.body.get("output_config"), deeper.effort.then(|| json!({"effort": "high"})).as_ref());
        }
        assert_eq!(
            seen[1].body["messages"][1],
            json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": "Considering.", "signature": "sig-1"},
                {"type": "tool_use", "id": "toolu_1", "name": "remember", "input": {"content": "Kenny is my accountant"}}
            ]})
        );
        assert_eq!(turn.window_lines()[0], format!("chat-thinking {}", json!({"text": "Considering.", "done": false})));
    }

    /// The last allowed round sends tool_choice none; a call there is not
    /// run and the turn ends with the fallback.
    #[tokio::test]
    async fn the_last_round() {
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "claude").unwrap().limits.max_tool_rounds = Some(2));
        let call = |id: &str| {
            let mut events = vec![start(10, 0, 0)];
            events.extend(tool_use(0, id, &[REMEMBER_ARGS]));
            events.extend(end("tool_use", 5));
            Reply::named_sse(&events)
        };
        let server = serve(vec![call("toolu_1"), call("toolu_2")]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply, Ok(fallback(&db)));
        assert_eq!(memories(&db).len(), 1);
        assert_eq!(turn.ledger.count(), 1, "the last round runs no tool");
        let seen = server.seen();
        assert_eq!(seen[0].body.get("tool_choice"), None);
        assert_eq!(seen[1].body["tool_choice"], json!({"type": "none"}));
    }

    #[tokio::test]
    async fn a_refused_request_and_a_stream_error() {
        let refusal = json!({"type": "error", "error": {"type": "invalid_request_error", "message": "max_tokens too large"}});
        let overloaded = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
        let server = serve(vec![Reply::json(400, refusal.clone()), Reply::named_sse(&[start(30, 0, 0), ("error", overloaded.clone())])]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err(format!("Claude API error 400 Bad Request: {refusal}")));
        assert!(rows(&db).is_empty());
        let turn = run_turn(&router(), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err(format!("Claude stream error: {overloaded}")));
        assert_eq!(turn.window_lines(), vec![format!("chat-state {}", json!({"state": "idle"}))]);
        let model = crate::ai::models::claude_default();
        assert_eq!(rows(&db), vec![priced_row("claude", "chat", &model, &[(30, 0, 0, 0)])], "what was billed before the error is stored");
    }

    /// A reply that has calls and text and says it is done (end_turn) is the
    /// answer once its calls ran: no further request.
    #[tokio::test]
    async fn an_end_turn_reply_with_calls_is_the_answer() {
        let mut events = vec![start(50, 0, 0), text_block(0), text(0, "Saved it."), stop(0)];
        events.extend(tool_use(1, "toolu_1", &[REMEMBER_ARGS]));
        events.extend(end("end_turn", 20));
        let server = serve(vec![Reply::named_sse(&events)]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved it."));
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        assert_eq!(turn.ledger.count(), 1);
        assert_eq!(server.seen().len(), 1, "no request after the calls");
    }

    /// A redirect is not followed: it would carry the key and the
    /// conversation to another address. The 307 is the error.
    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        let mut events = vec![start(10, 0, 0), text_block(0), text(0, "Hi."), stop(0)];
        events.extend(end("end_turn", 2));
        // Followed, the redirect would reach the same server and its answer.
        let server = serve(vec![Reply::json(307, json!({})).header("Location", "/v1/messages"), Reply::named_sse(&events)]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        let error = turn.reply.unwrap_err();
        assert!(error.contains("307"), "{error}");
        assert_eq!(server.seen().len(), 1, "no second request");
    }

    /// A call with no arguments streams no input: the tool gets `{}`, and
    /// the call goes back with `"input": {}` (it was null before P07).
    #[tokio::test]
    async fn a_call_without_arguments_goes_back_as_an_empty_object() {
        let mut first = vec![start(50, 0, 0)];
        first.push(("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "list_memories", "input": {}}})));
        first.push(stop(0));
        first.extend(end("tool_use", 5));
        let mut second = vec![start(60, 0, 0), text_block(0), text(0, "Nothing saved yet."), stop(0)];
        second.extend(end("end_turn", 4));
        let server = serve(vec![Reply::named_sse(&first), Reply::named_sse(&second)]).await;
        let _base = point_at(&server.base);
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "What do you remember?")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Nothing saved yet."));
        assert_eq!(turn.window_lines().iter().filter(|line| line.contains("\"detail\"")).count(), 1, "the call ran");
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].body["messages"][1], json!({"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_1", "name": "list_memories", "input": {}}]}));
        assert_eq!(seen[1].body["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
    }
}

// ---------------------------------------------------------------------------
// Gemini (generativelanguage.googleapis.com)
// ---------------------------------------------------------------------------

mod gemini_wire {
    use super::*;

    fn row() -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "gemini".into(), endpoint_id: None, model_id: Some(crate::ai::models::GEMINI_FLASH.into()), enabled: true, base_url: None }
    }

    fn router() -> AiRouter {
        let mut router = AiRouter::new("gemini_primary");
        router.set_gemini_key("AIza-fixture-not-real");
        router
    }

    fn marked(body: &Value) -> Value {
        let mut body = body.clone();
        assert_app_system_prompt(body["systemInstruction"]["parts"][0]["text"].as_str().unwrap());
        body["systemInstruction"]["parts"][0]["text"] = json!("<system>");
        let declarations: Vec<Value> = tools::get_tool_definitions().iter().map(|t| json!({"name": t.name, "description": t.description, "parameters": crate::ai::gemini::sanitize_schema(&t.parameters)})).collect();
        assert_eq!(body["tools"], json!([{"functionDeclarations": declarations}]), "every tool, with the schema Gemini takes");
        body["tools"] = json!("<tools>");
        body
    }

    fn chunk(parts: Value, finish: Option<&str>, usage: Option<Value>) -> Value {
        let mut candidate = json!({"content": {"role": "model", "parts": parts}});
        if let Some(finish) = finish {
            candidate["finishReason"] = json!(finish);
        }
        let mut chunk = json!({"candidates": [candidate]});
        if let Some(usage) = usage {
            chunk["usageMetadata"] = usage;
        }
        chunk
    }

    fn path(model: &str) -> String {
        format!("/v1beta/models/{model}:streamGenerateContent?alt=sse")
    }

    #[tokio::test]
    async fn a_text_turn() {
        let usage = json!({"promptTokenCount": 500, "candidatesTokenCount": 20, "thoughtsTokenCount": 30, "cachedContentTokenCount": 100});
        let server = serve(vec![Reply::sse(&[chunk(json!([{"text": "Hel"}]), None, None), chunk(json!([{"text": "lo."}]), Some("STOP"), Some(usage))])]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Hi"), ("assistant", " "), ("user", "What is on today?"), ("assistant", "Nothing.")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Hello."));
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        let model = crate::ai::models::GEMINI_FLASH;
        assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", path(model).as_str()));
        assert_eq!(seen[0].header("x-goog-api-key"), Some("AIza-fixture-not-real"));
        let cap = crate::ai::models::max_output_tokens(model).unwrap_or_else(|| crate::ai::models::limits("gemini").default_max_output_tokens.unwrap());
        // An empty row is dropped and the turns of one role either side of
        // it merge; the assistant is "model".
        assert_eq!(
            marked(&seen[0].body),
            json!({
                "contents": [
                    {"role": "user", "parts": [{"text": "Hi"}, {"text": "What is on today?"}]},
                    {"role": "model", "parts": [{"text": "Nothing."}]}
                ],
                "tools": "<tools>",
                "systemInstruction": {"parts": [{"text": "<system>"}]},
                "generationConfig": {"maxOutputTokens": cap}
            })
        );
        assert_eq!(turn.window_lines(), vec![status("Composing response...", "responding"), token("Hel"), token("lo."), token_done()]);
        assert_eq!(turn.voice, vec!["text: Hel", "text: lo."]);
        // Thinking is billed as output.
        assert_eq!(rows(&db), vec![priced_row("gemini", "chat", model, &[(500, 50, 100, 0)])]);
    }

    #[tokio::test]
    async fn a_tool_turn() {
        let call = json!({"functionCall": {"name": "remember", "args": {"content": "Kenny is my accountant"}}, "thoughtSignature": "sig-1"});
        let again = json!({"functionCall": {"name": "remember", "args": {"content": "Kenny is my accountant"}}});
        let server = serve(vec![
            Reply::sse(&[chunk(json!([call.clone(), again.clone()]), Some("STOP"), Some(json!({"promptTokenCount": 100, "candidatesTokenCount": 10})))]),
            Reply::sse(&[chunk(json!([{"text": "Saved."}]), Some("STOP"), Some(json!({"promptTokenCount": 150, "candidatesTokenCount": 5})))]),
        ])
        .await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        let result = remembered("Kenny is my accountant").await;
        let second = marked(&seen[1].body);
        assert_eq!(second.get("toolConfig"), None);
        // The calls go back as they came, signature included; each result
        // is wrapped as {"result": ...}.
        assert_eq!(
            second["contents"],
            json!([
                {"role": "user", "parts": [{"text": "Remember Kenny is my accountant"}]},
                {"role": "model", "parts": [call, again]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "remember", "response": {"result": result}}},
                    {"functionResponse": {"name": "remember", "response": {"result": SKIPPED}}}
                ]}
            ])
        );
        // Announced once per tool name.
        assert_eq!(
            turn.window_lines(),
            vec![
                planning("remember"),
                tool_call("remember"),
                planning("remember"),
                running(tools::tool_status_label("remember"), json!(tools::tool_status_detail("remember", REMEMBER_ARGS))),
                status("Reviewing tool results...", "thinking"),
                status("Composing response...", "responding"),
                token("Saved."),
                token_done(),
            ]
        );
        assert_eq!(rows(&db), vec![priced_row("gemini", "tools", crate::ai::models::GEMINI_FLASH, &[(100, 10, 0, 0), (150, 5, 0, 0)])]);
    }

    /// A field Gemini is asked to send as JSON text (render_chart's open
    /// `data` object) is an object again when the tool runs; the call goes
    /// back to Gemini as it came.
    #[tokio::test]
    async fn json_text_arguments_are_restored_before_the_tool_runs() {
        let data = r#"{"segments":[{"label":"Mail","value":3}]}"#;
        let call = json!({"functionCall": {"name": "render_chart", "args": {"chart_type": "pie", "data": data}}});
        let server = serve(vec![Reply::sse(&[chunk(json!([call.clone()]), Some("STOP"), None)]), Reply::sse(&[chunk(json!([{"text": "Here."}]), Some("STOP"), None)])]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Chart my mail")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Here."));
        let seen = server.seen();
        assert_eq!(seen[1].body["contents"][1]["parts"][0], call, "sent back unchanged");
        let result = &seen[1].body["contents"][2]["parts"][0]["functionResponse"]["response"]["result"];
        assert_eq!(result.as_str(), Some(format!("[CHART:pie|{data}]").as_str()), "the tool got the object, not the text");
    }

    /// A reply without text or a call is an error naming the model and why
    /// it stopped (the router sends it once more); its usage is stored.
    #[tokio::test]
    async fn an_empty_reply() {
        let server = serve(vec![Reply::sse(&[chunk(json!([]), Some("SAFETY"), Some(json!({"promptTokenCount": 80, "candidatesTokenCount": 0})))])]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err(format!("Gemini ({}) returned no text (finish_reason=SAFETY)", crate::ai::models::GEMINI_FLASH)));
        assert_eq!(turn.window_lines(), vec![token_done()]);
        assert_eq!(rows(&db), vec![priced_row("gemini", "chat", crate::ai::models::GEMINI_FLASH, &[(80, 0, 0, 0)])]);
    }

    #[tokio::test]
    async fn the_last_round() {
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "gemini").unwrap().limits.max_tool_rounds = Some(2));
        let call = || Reply::sse(&[chunk(json!([{"functionCall": {"name": "remember", "args": {"content": "Kenny is my accountant"}}}]), Some("STOP"), None)]);
        let server = serve(vec![call(), call()]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply, Ok(fallback(&db)));
        assert_eq!(memories(&db).len(), 1);
        assert_eq!(turn.ledger.count(), 1, "the last round runs no tool");
        let seen = server.seen();
        assert_eq!(seen[0].body.get("toolConfig"), None);
        assert_eq!(seen[1].body["toolConfig"], json!({"functionCallingConfig": {"mode": "NONE"}}));
    }

    /// Think harder asks for the high thinking level, and the request goes
    /// to the model the row names.
    #[tokio::test]
    async fn a_think_harder_turn() {
        let server = serve(vec![Reply::sse(&[chunk(json!([{"text": "Deep."}]), Some("STOP"), None)])]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let pro = ProviderChainEntry { model_id: Some(crate::ai::models::GEMINI_PRO.into()), ..row() };
        let deeper = capability::deeper(&pro).expect("the test registry offers Think harder on the pro model");
        let turn = run_turn(&router(), pro, &[("user", "Hi")], &db, Workload::Premium, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Deep."));
        let seen = server.seen();
        assert_eq!(seen[0].path, path(crate::ai::models::GEMINI_PRO));
        assert_eq!(seen[0].body["generationConfig"].get("thinkingConfig"), deeper.thinking.then(|| json!({"thinkingLevel": "HIGH"})).as_ref());
    }

    #[tokio::test]
    async fn a_refused_request() {
        let refusal = json!({"error": {"code": 429, "message": "Resource has been exhausted", "status": "RESOURCE_EXHAUSTED"}});
        let server = serve(vec![Reply::json(429, refusal.clone())]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err(format!("Gemini API error 429 Too Many Requests: {refusal}")));
        assert_eq!(server.seen().len(), 1, "no resend inside the wire");
        assert!(rows(&db).is_empty());
    }

    /// A reply without usageMetadata is stored as zeros, and the log says
    /// so, with none of the reply.
    #[tokio::test]
    async fn a_reply_without_usage_figures_is_logged() {
        let server = serve(vec![Reply::sse(&[chunk(json!([{"text": "Your dentist is Mary."}]), Some("STOP"), None)])]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let logged = crate::test_log::capture(|| async {
            run_turn(&router(), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        })
        .await;
        let model = crate::ai::models::GEMINI_FLASH;
        assert!(logged.contains(&format!("[gemini] no usage figures received for a non-empty response; recording zero tokens (model={model})")), "{logged:?}");
        assert!(!logged.iter().any(|line| line.contains("dentist")), "{logged:?}");
    }

    /// A redirect is not followed: it would carry the key and the
    /// conversation to another address. The 307 is the error.
    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        let answer = || Reply::sse(&[chunk(json!([{"text": "Hi."}]), Some("STOP"), None)]);
        // Followed, the redirect would reach the same server and its answer.
        let server = serve(vec![Reply::json(307, json!({})).header("Location", "/elsewhere"), answer()]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let db = crate::db::test_database();
        let turn = run_turn(&router(), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        let error = turn.reply.unwrap_err();
        assert!(error.contains("307"), "{error}");
        assert_eq!(server.seen().len(), 1, "no second request");
    }
}

// ---------------------------------------------------------------------------
// Bedrock (Converse)
// ---------------------------------------------------------------------------

pub(crate) mod bedrock_wire {
    use super::*;

    pub(crate) const MODEL: &str = "us.anthropic.claude-haiku-4-5-20251001-v1:0";

    fn row() -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "bedrock".into(), endpoint_id: None, model_id: Some(MODEL.into()), enabled: true, base_url: None }
    }

    /// Runs `body` with AWS credentials saved for us-east-1, holding the
    /// lock on the process-wide Keychain stand-in. Bedrock's SDK client is
    /// built on this thread, so the loopback endpoint is read here.
    pub(crate) fn with_credentials<F: std::future::Future<Output = ()>>(body: impl FnOnce(Arc<crate::db::Database>) -> F) {
        let _keychain = crate::aws::credentials::tests::KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let db = Arc::new(crate::db::test_database());
            crate::aws::credentials::AwsCredsRepo::new(db.clone()).save(&crate::ai::bedrock::tests::fixture_creds("us-east-1")).await.unwrap();
            body(db).await;
        });
        let _ = crate::security::keychain::KeychainStore::delete("com.jarvis.aws", "secret_access_key");
    }

    /// The Converse body with the system blocks and the tool list checked
    /// and replaced by markers.
    fn marked(body: &Value) -> Value {
        let mut body = body.clone();
        let system = body["system"].as_array().unwrap().clone();
        assert_eq!(system.len(), 3, "static prompt, cache point, per-request part");
        assert_eq!(system[0], json!({"text": tools::build_cacheable_system_prompt()}));
        assert_eq!(system[1], json!({"cachePoint": {"type": "default"}}));
        assert!(system[2]["text"].as_str().unwrap().contains("Current local time: "));
        body["system"] = json!("<system>");
        let mut specs: Vec<Value> = tools::get_tool_definitions().iter().map(|t| json!({"toolSpec": {"name": t.name, "description": t.description, "inputSchema": {"json": t.parameters}}})).collect();
        specs.push(json!({"cachePoint": {"type": "default"}}));
        assert_eq!(body["toolConfig"], json!({"tools": specs}), "every tool, then a cache point");
        body["toolConfig"] = json!("<tools>");
        let temperature = body["inferenceConfig"]["temperature"].as_f64().unwrap();
        assert!((temperature - 0.2).abs() < 1e-6, "{temperature}");
        body["inferenceConfig"]["temperature"] = json!("<temperature>");
        body
    }

    /// A ConverseStream reply holding the Converse message `content`: each
    /// block streamed whole, then the stop reason and the usage.
    fn converse(content: Value, stop: &str, usage: Value) -> Reply {
        let mut events = vec![("messageStart", json!({"role": "assistant"}))];
        for (index, block) in content.as_array().unwrap().iter().enumerate() {
            match block["text"].as_str() {
                Some(text) => events.push(("contentBlockDelta", json!({"contentBlockIndex": index, "delta": {"text": text}}))),
                None => {
                    let tool = &block["toolUse"];
                    events.push(("contentBlockStart", json!({"contentBlockIndex": index, "start": {"toolUse": {"toolUseId": tool["toolUseId"], "name": tool["name"]}}})));
                    events.push(("contentBlockDelta", json!({"contentBlockIndex": index, "delta": {"toolUse": {"input": tool["input"].to_string()}}})));
                }
            }
            events.push(("contentBlockStop", json!({"contentBlockIndex": index})));
        }
        events.push(("messageStop", json!({"stopReason": stop})));
        events.push(("metadata", json!({"usage": usage, "metrics": {"latencyMs": 5}})));
        stream(&events)
    }

    fn stream(events: &[(&str, Value)]) -> Reply {
        Reply::raw(200, crate::ai::bedrock::tests::EVENT_STREAM, crate::ai::bedrock::tests::converse_stream(events))
    }

    fn usage(input: i64, output: i64) -> Value {
        json!({"inputTokens": input, "outputTokens": output, "totalTokens": input + output})
    }

    fn one(provider: &str, feature: &str, request: (i64, i64, i64, i64)) -> Row {
        priced_row(provider, feature, MODEL, &[request])
    }

    #[test]
    fn a_text_turn() {
        with_credentials(|db| async move {
            let server = serve(vec![Reply::raw(200, crate::ai::bedrock::tests::EVENT_STREAM, crate::ai::bedrock::tests::converse_stream_text("Hello."))]).await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("assistant", "Earlier"), ("user", "Hi"), ("assistant", " "), ("user", "What is on today?")], &db, Workload::Tools, false).await;
            assert_eq!(turn.reply.as_deref(), Ok("Hello."));
            let seen = server.seen();
            assert_eq!(seen.len(), 1);
            assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", format!("/model/{MODEL}/converse-stream").as_str()));
            assert!(seen[0].header("authorization").unwrap().starts_with("AWS4-HMAC-SHA256 Credential=AKIAFIXTURENOTREAL/"));
            // A leading assistant run is dropped, an empty row too, and the
            // user rows either side of it merge.
            assert_eq!(
                marked(&seen[0].body),
                json!({
                    "messages": [{"role": "user", "content": [{"text": "Hi"}, {"text": "What is on today?"}]}],
                    "system": "<system>",
                    "toolConfig": "<tools>",
                    "inferenceConfig": {"maxTokens": 4096, "temperature": "<temperature>"}
                })
            );
            // P07: ConverseStream, so the answer is shown and spoken as it
            // comes (before: Converse, the whole answer at once).
            assert_eq!(turn.window_lines(), vec![status("Composing response...", "responding"), token("Hel"), token("lo."), token_done()]);
            assert_eq!(turn.voice, vec!["text: Hel", "text: lo."]);
            assert_eq!(rows(&db), vec![one("bedrock", "chat", (1320, 45, 800, 400))]);
        });
    }

    #[test]
    fn a_tool_turn() {
        with_credentials(|db| async move {
            let call = json!({"toolUse": {"toolUseId": "tu_1", "name": "remember", "input": {"content": "Kenny is my accountant"}}});
            let again = json!({"toolUse": {"toolUseId": "tu_2", "name": "remember", "input": {"content": "Kenny is my accountant"}}});
            let server = serve(vec![
                converse(json!([{"text": "Let me save that."}, call.clone(), again.clone()]), "tool_use", usage(100, 10)),
                converse(json!([{"text": "Saved."}]), "end_turn", usage(150, 5)),
            ])
            .await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
            assert_eq!(turn.reply.as_deref(), Ok("Saved."));
            assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
            assert_eq!(turn.ledger.count(), 1);
            let seen = server.seen();
            assert_eq!(seen.len(), 2);
            let result = remembered("Kenny is my accountant").await;
            assert_eq!(
                marked(&seen[1].body)["messages"],
                json!([
                    {"role": "user", "content": [{"text": "Remember Kenny is my accountant"}]},
                    {"role": "assistant", "content": [{"text": "Let me save that."}, call, again]},
                    {"role": "user", "content": [
                        {"toolResult": {"toolUseId": "tu_1", "content": [{"text": result}], "status": "success"}},
                        {"toolResult": {"toolUseId": "tu_2", "content": [{"text": SKIPPED}], "status": "success"}}
                    ]}
                ])
            );
            // P07, as on every other wire: the text of a round that calls a
            // tool is shown, and each call is announced when the reply
            // arrives (before: on running, and the text was not shown).
            assert_eq!(
                turn.window_lines(),
                vec![
                    status("Composing response...", "responding"),
                    token("Let me save that."),
                    planning("remember"),
                    tool_call("remember"),
                    planning("remember"),
                    tool_call("remember"),
                    running(tools::tool_status_label("remember"), json!(tools::tool_status_detail("remember", REMEMBER_ARGS))),
                    status("Reviewing tool results...", "thinking"),
                    status("Composing response...", "responding"),
                    token("Saved."),
                    token_done(),
                ]
            );
            // One row per request, both under tools.
            assert_eq!(rows(&db), vec![one("bedrock", "tools", (100, 10, 0, 0)), one("bedrock", "tools", (150, 5, 0, 0))]);
        });
    }

    /// Before the last allowed round the results carry a note in words
    /// (Converse has no "no tools" choice while the history holds calls);
    /// a call the last round still asks for is not run.
    #[test]
    fn the_last_round() {
        with_credentials(|db| async move {
            let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "bedrock").unwrap().limits.max_tool_rounds = Some(2));
            let call = |id: &str| converse(json!([{"toolUse": {"toolUseId": id, "name": "remember", "input": {"content": "Kenny is my accountant"}}}]), "tool_use", usage(10, 1));
            let server = serve(vec![call("tu_1"), call("tu_2")]).await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
            assert_eq!(turn.reply, Ok(fallback(&db)));
            assert_eq!(memories(&db).len(), 1);
            assert_eq!(turn.ledger.count(), 1, "the last round runs no tool");
            let seen = server.seen();
            let results = &seen[1].body["messages"][2]["content"];
            assert_eq!(results[1], json!({"text": "Tool limit reached. Do not call any more tools: answer the user now from the results above, and say briefly what you could not check."}));
            assert_eq!(seen[1].body.get("toolChoice"), None);
            assert_eq!(rows(&db).len(), 2);
        });
    }

    #[test]
    fn a_refused_request() {
        with_credentials(|db| async move {
            let server = serve(vec![Reply::json(400, json!({"message": "The provided model identifier is invalid."})).header("x-amzn-ErrorType", "ValidationException")]).await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
            let error = turn.reply.clone().unwrap_err();
            assert!(error.starts_with("Bedrock converse: "), "{error}");
            assert!(error.contains("ValidationException"), "{error}");
            assert!(rows(&db).is_empty());
            assert!(turn.window.is_empty());
        });
    }

    /// A call's input can come in several pieces; it runs once whole.
    #[test]
    fn a_call_streamed_in_pieces() {
        with_credentials(|db| async move {
            let server = serve(vec![
                stream(&[
                    ("messageStart", json!({"role": "assistant"})),
                    ("contentBlockStart", json!({"contentBlockIndex": 0, "start": {"toolUse": {"toolUseId": "tu_1", "name": "remember"}}})),
                    ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"toolUse": {"input": "{\"content\":"}}})),
                    ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"toolUse": {"input": "\"Kenny is my accountant\"}"}}})),
                    ("contentBlockStop", json!({"contentBlockIndex": 0})),
                    ("messageStop", json!({"stopReason": "tool_use"})),
                    ("metadata", json!({"usage": usage(100, 10)})),
                ]),
                converse(json!([{"text": "Saved."}]), "end_turn", usage(150, 5)),
            ])
            .await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
            assert_eq!(turn.reply.as_deref(), Ok("Saved."));
            assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
            let seen = server.seen();
            assert_eq!(seen[1].body["messages"][1], json!({"role": "assistant", "content": [{"toolUse": {"toolUseId": "tu_1", "name": "remember", "input": {"content": "Kenny is my accountant"}}}]}));
        });
    }

    /// An error part-way through the stream ends the turn: the window goes
    /// back to idle, and with no usage figures received nothing is stored.
    #[test]
    fn a_broken_stream() {
        with_credentials(|db| async move {
            let mut body = crate::ai::bedrock::tests::converse_stream(&[
                ("messageStart", json!({"role": "assistant"})),
                ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"text": "Partly"}})),
            ]);
            body.extend(crate::ai::bedrock::tests::stream_exception("modelStreamErrorException", "The model stopped."));
            let server = serve(vec![Reply::raw(200, crate::ai::bedrock::tests::EVENT_STREAM, body)]).await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
            let error = turn.reply.clone().unwrap_err();
            assert!(error.starts_with("Bedrock converse: "), "{error}");
            assert!(error.contains("ModelStreamErrorException"), "{error}");
            assert_eq!(server.seen().len(), 1);
            assert_eq!(turn.window_lines(), vec![status("Composing response...", "responding"), token("Partly"), "chat-state {\"state\":\"idle\"}".to_string()]);
            assert!(rows(&db).is_empty());
        });
    }

    /// A stream that ends without its stop is broken (P07): the window goes
    /// back to idle, and the usage it did report is stored, since AWS bills
    /// it.
    #[test]
    fn a_stream_without_its_stop() {
        with_credentials(|db| async move {
            let server = serve(vec![stream(&[
                ("messageStart", json!({"role": "assistant"})),
                ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"text": "Partly"}})),
                ("metadata", json!({"usage": usage(40, 2)})),
            ])])
            .await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
            let error = turn.reply.clone().unwrap_err();
            assert!(error.contains("Bedrock stream ended before the reply was complete"), "{error}");
            assert_eq!(turn.window_lines().last().unwrap(), "chat-state {\"state\":\"idle\"}");
            assert_eq!(rows(&db), vec![one("bedrock", "chat", (40, 2, 0, 0))]);
        });
    }

    /// No saved credentials: nothing is sent.
    #[tokio::test]
    async fn no_credentials() {
        let db = crate::db::test_database();
        let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err("bedrock: AWS credentials not configured in Settings".into()));
    }

    /// The fallback on an empty last round did not stream: it reaches the
    /// window as one token and the voice in one piece.
    #[test]
    fn the_fallback_is_shown_and_spoken() {
        with_credentials(|db| async move {
            let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "bedrock").unwrap().limits.max_tool_rounds = Some(2));
            let call = converse(json!([{"toolUse": {"toolUseId": "tu_1", "name": "remember", "input": {"content": "Kenny is my accountant"}}}]), "tool_use", usage(10, 1));
            let server = serve(vec![call, converse(json!([]), "end_turn", usage(10, 0))]).await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
            let fallback = fallback(&db);
            assert_eq!(turn.reply.as_ref(), Ok(&fallback));
            let lines = turn.window_lines();
            assert_eq!(&lines[lines.len() - 3..], &[status("Composing response...", "responding"), token(&fallback), token_done()]);
            assert_eq!(turn.voice.last(), Some(&format!("text: {fallback}")));
        });
    }
}

// ---------------------------------------------------------------------------
// Local servers (Ollama, vLLM, any OpenAI-compatible server on this Mac)
// ---------------------------------------------------------------------------

mod local_wire {
    use super::*;
    use crate::ai::local::backend::{BackendType, LocalEndpoint};

    fn endpoint(url: &str, backend_type: BackendType) -> LocalEndpoint {
        LocalEndpoint { id: "lm".into(), name: "Local".into(), url: url.into(), backend_type, api_key: None, use_tls: false, connection_timeout_ms: 5000, keep_alive_minutes: 30, is_active: true, last_health_check: None, last_health_status: None }
    }

    fn router(url: &str, backend_type: BackendType) -> AiRouter {
        let mut router = AiRouter::new("gemini_primary");
        router.set_local_endpoints(vec![endpoint(url, backend_type)]);
        router
    }

    fn row(model: &str) -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "local".into(), endpoint_id: Some("lm".into()), model_id: Some(model.into()), enabled: true, base_url: None }
    }

    /// Saves the model's settings, as Settings does.
    fn overrides(db: &crate::db::Database, url: &str, model: &str, context_length: Option<u32>, tool_capability: &str) {
        let conn = db.conn.lock().unwrap();
        conn.execute("INSERT OR IGNORE INTO local_endpoints (id, name, url, backend_type) VALUES ('lm', 'Local', ?1, 'vllm')", [url]).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO local_model_overrides (endpoint_id, model_id, context_length, tool_capability) VALUES ('lm', ?1, ?2, ?3)",
            rusqlite::params![model, context_length, tool_capability],
        )
        .unwrap();
    }

    fn local_row(feature: &str, model: &str, input: i64, output: i64) -> Row {
        Row { provider: "local".into(), feature: feature.into(), model: model.into(), input, output, cache_read: 0, cache_write: 0, cost: Some(0), price_unknown: false, registry_version: crate::ai::registry::current().version as i64 }
    }

    fn system_of(body: &Value) -> String {
        assert_eq!(body["messages"][0]["role"], "system");
        let system = body["messages"][0]["content"].as_str().unwrap().to_string();
        assert_app_system_prompt(&system);
        system
    }

    fn marked(body: &Value) -> Value {
        let mut body = body.clone();
        system_of(&body);
        body["messages"][0]["content"] = json!("<system>");
        if body.get("tools").is_some() {
            assert_eq!(body["tools"], json!(openai_wire::openai_tools()));
            body["tools"] = json!("<tools>");
        }
        body
    }

    fn text_reply(text: &str, usage: Option<(i64, i64)>) -> Reply {
        let mut events = vec![json!({"choices": [{"delta": {"content": text}, "finish_reason": "stop"}]})];
        if let Some((prompt, completion)) = usage {
            events.push(json!({"choices": [], "usage": {"prompt_tokens": prompt, "completion_tokens": completion, "prompt_tokens_details": {"cached_tokens": 50}}}));
        }
        Reply::openai_sse(&events)
    }

    /// A model that calls tools itself (vLLM): every tool, the local output
    /// cap, the usage asked for, and the row stored at no cost without the
    /// cache figures.
    #[tokio::test]
    async fn a_native_text_turn() {
        let server = serve(vec![text_reply("Hi.", Some((300, 20)))]).await;
        let db = crate::db::test_database();
        let turn = run_turn(&router(&server.base, BackendType::Vllm), row("my-model"), &[("user", "Hi"), ("assistant", " "), ("user", "What is on today?")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Hi."));
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].path, "/v1/chat/completions");
        assert_eq!(seen[0].header("authorization"), None);
        let context = crate::ai::models::limits("ollama").default_context_tokens.unwrap();
        assert_eq!(
            marked(&seen[0].body),
            json!({
                "model": "my-model",
                "messages": [{"role": "system", "content": "<system>"}, {"role": "user", "content": "Hi"}, {"role": "user", "content": "What is on today?"}],
                "tools": "<tools>",
                "max_tokens": crate::ai::models::local_chat_max_output_tokens(context),
                "stream": true,
                "stream_options": {"include_usage": true}
            })
        );
        assert_eq!(turn.window_lines(), vec![status("Composing response...", "responding"), token("Hi."), token_done()]);
        assert_eq!(rows(&db), vec![local_row("chat", "my-model", 300, 20)]);
    }

    #[tokio::test]
    async fn a_native_tool_turn() {
        let server = serve(vec![
            Reply::openai_sse(&[
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "remember", "arguments": "{\"content\":"}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "\"Kenny is my accountant\"}"}}]}, "finish_reason": "tool_calls"}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10}}),
            ]),
            text_reply("Saved.", Some((150, 5))),
        ])
        .await;
        let db = crate::db::test_database();
        let mut router = router(&server.base, BackendType::Ollama);
        router.local_endpoints[0].api_key = Some("local-fixture-key".into());
        // A model of a family that calls tools itself (the registry's list).
        let turn = run_turn(&router, row("qwen3:8b"), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        assert_eq!(turn.ledger.count(), 1);
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].header("authorization"), Some("Bearer local-fixture-key"));
        let result = remembered("Kenny is my accountant").await;
        assert_eq!(
            marked(&seen[1].body)["messages"],
            json!([
                {"role": "system", "content": "<system>"},
                {"role": "user", "content": "Remember Kenny is my accountant"},
                {"role": "assistant", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]},
                {"role": "tool", "content": result, "tool_call_id": "call_1"}
            ])
        );
        assert_eq!(
            turn.window_lines(),
            vec![
                planning("remember"),
                tool_call("remember"),
                running(tools::tool_status_label("remember"), json!(tools::tool_status_detail("remember", REMEMBER_ARGS))),
                status("Reviewing tool results...", "thinking"),
                status("Composing response...", "responding"),
                token("Saved."),
                token_done(),
            ]
        );
        assert_eq!(rows(&db), vec![local_row("tools", "qwen3:8b", 250, 15)]);
    }

    /// The last allowed round sends tool_choice none, and a call the model
    /// still asks for there is not run.
    #[tokio::test]
    async fn the_last_round() {
        let _registry = crate::ai::registry::test_override::with(|r| r.providers.iter_mut().find(|p| p.id == "ollama").unwrap().limits.max_tool_rounds = Some(2));
        let call = |id: &str| Reply::openai_sse(&[json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": id, "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "tool_calls"}]})]);
        let server = serve(vec![call("call_1"), call("call_2")]).await;
        let db = crate::db::test_database();
        let turn = run_turn(&router(&server.base, BackendType::Vllm), row("my-model"), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply, Ok(fallback(&db)));
        assert_eq!(turn.ledger.count(), 1, "the last round runs no tool");
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].body.get("tool_choice"), None);
        assert_eq!(seen[1].body["tool_choice"], "none");
    }

    /// A model that calls tools itself runs its calls only when it stops to
    /// call them (`tool_calls`), as before P07: calls in a reply that ended
    /// for another reason are not run.
    #[tokio::test]
    async fn calls_run_only_on_a_tool_calls_stop() {
        let server = serve(vec![Reply::openai_sse(&[
            json!({"choices": [{"delta": {"content": "Done."}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]}, "finish_reason": "stop"}]}),
        ])])
        .await;
        let db = crate::db::test_database();
        let turn = run_turn(&router(&server.base, BackendType::Vllm), row("my-model"), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Done."));
        assert!(memories(&db).is_empty());
        assert_eq!(server.seen().len(), 1);
    }

    /// Calls streamed whole without an index are told apart by id, as on
    /// OpenAI's own parser (P07: they all went to one slot before), and a
    /// usage chunk whose choices are null still counts.
    #[tokio::test]
    async fn calls_without_an_index_are_told_apart_by_id() {
        let call = |id: &str, content: &str| json!({"choices": [{"delta": {"tool_calls": [{"id": id, "type": "function", "function": {"name": "remember", "arguments": json!({"content": content}).to_string()}}]}}]});
        let server = serve(vec![
            Reply::openai_sse(&[
                call("call_a", "First fact"),
                call("call_b", "Second fact"),
                json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}),
                json!({"choices": null, "usage": {"prompt_tokens": 100, "completion_tokens": 10}}),
            ]),
            text_reply("Saved.", Some((150, 5))),
        ])
        .await;
        let db = crate::db::test_database();
        let turn = run_turn(&router(&server.base, BackendType::Ollama), row("qwen3:8b"), &[("user", "Remember two facts")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(memories(&db), vec!["First fact", "Second fact"]);
        let seen = server.seen();
        let ids: Vec<&str> = seen[1].body["messages"][2]["tool_calls"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["call_a", "call_b"]);
        assert_eq!(rows(&db), vec![local_row("tools", "qwen3:8b", 250, 15)]);
    }

    /// A model that cannot call tools itself (any server JARVIS cannot ask):
    /// the tools are described in the system prompt, the reply is asked for
    /// as JSON, a call in it runs, and its result goes back in words. A
    /// generic server is not asked for usage.
    #[tokio::test]
    async fn a_prompt_injected_tool_turn() {
        let call = r#"{"tool_calls":[{"name":"remember","arguments":{"content":"Kenny is my accountant"}}]}"#;
        let (head, tail) = call.split_at(20);
        let server = serve(vec![
            Reply::openai_sse(&[json!({"choices": [{"delta": {"content": head}}]}), json!({"choices": [{"delta": {"content": tail}, "finish_reason": "stop"}]})]),
            Reply::openai_sse(&[json!({"choices": [{"delta": {"content": "{\"response\":\"Saved.\"}"}, "finish_reason": "stop"}]})]),
        ])
        .await;
        let db = crate::db::test_database();
        let question = "Remember Kenny is my accountant";
        let turn = run_turn(&router(&server.base, BackendType::Generic), row("my-model"), &[("user", question)], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        let context = crate::ai::models::limits("ollama").default_context_tokens.unwrap();
        let relevant = crate::ai::local::prompt_inject::select_relevant_tools(&tools::get_tool_definitions(), question, crate::ai::local::context::ContextManager::max_tools_for_context(context));
        assert!(system_of(&seen[0].body).ends_with(&crate::ai::local::prompt_inject::build_tool_injection_prompt(&relevant)));
        assert_eq!(
            marked(&seen[0].body),
            json!({
                "model": "my-model",
                "messages": [{"role": "system", "content": "<system>"}, {"role": "user", "content": question}],
                "max_tokens": crate::ai::models::local_chat_max_output_tokens(context),
                "stream": true,
                "response_format": {"type": "json_object"}
            })
        );
        let result = remembered("Kenny is my accountant").await;
        assert_eq!(
            marked(&seen[1].body)["messages"],
            json!([
                {"role": "system", "content": "<system>"},
                {"role": "user", "content": question},
                {"role": "assistant", "content": call},
                {"role": "user", "content": format!("Tool 'remember' returned: {result}\n\nNow respond to the user based on this result. Do NOT call any more tools. Respond in plain text.")}
            ])
        );
        assert_eq!(
            turn.window_lines(),
            vec![
                status("Composing response...", "responding"),
                token(head),
                token(tail),
                running(tools::tool_status_label("remember"), json!(tools::tool_status_detail("remember", REMEMBER_ARGS))),
                status("Reviewing tool results...", "thinking"),
                status("Composing response...", "responding"),
                token("{\"response\":\"Saved.\"}"),
                token_done(),
            ]
        );
        assert_eq!(rows(&db), vec![local_row("tools", "my-model", 0, 0)]);
    }

    /// A model set to chat only, or with a context under 4,096 tokens, is
    /// sent no tools and no JSON request.
    #[tokio::test]
    async fn a_chat_only_turn() {
        let server = serve(vec![text_reply("Hi.", None), text_reply("Hi again.", None)]).await;
        let db = crate::db::test_database();
        overrides(&db, &server.base, "my-model", None, "chat_only");
        overrides(&db, &server.base, "small-model", Some(2048), "auto");
        let router = router(&server.base, BackendType::Vllm);
        let turn = run_turn(&router, row("my-model"), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Hi."));
        let turn = run_turn(&router, row("small-model"), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply.as_deref(), Ok("Hi again."));
        let seen = server.seen();
        let context = crate::ai::models::limits("ollama").default_context_tokens.unwrap();
        for (request, model, context) in [(&seen[0], "my-model", context), (&seen[1], "small-model", 2048)] {
            assert_eq!(
                marked(&request.body),
                json!({
                    "model": model,
                    "messages": [{"role": "system", "content": "<system>"}, {"role": "user", "content": "Hi"}],
                    "max_tokens": crate::ai::models::local_chat_max_output_tokens(context),
                    "stream": true,
                    "stream_options": {"include_usage": true}
                })
            );
        }
        assert_eq!(rows(&db), vec![local_row("chat", "my-model", 0, 0), local_row("chat", "small-model", 0, 0)]);
    }

    /// A model that takes no tools, by the user's setting or a small
    /// context, is told so at the end of its system prompt, after the
    /// prompt that tells models to use tools (P07, the lead's ruling: not
    /// in the reply). A model that takes tools is not.
    #[tokio::test]
    async fn a_model_without_tools_is_told_it_has_none() {
        let server = serve((0..4).map(|_| text_reply("Hi.", None)).collect()).await;
        let db = crate::db::test_database();
        overrides(&db, &server.base, "my-model", None, "chat_only");
        overrides(&db, &server.base, "small-model", Some(2048), "auto");
        overrides(&db, &server.base, "json-model", None, "prompt_injected");
        let router = router(&server.base, BackendType::Vllm);
        for model in ["my-model", "small-model", "json-model", "qwen3:8b"] {
            let turn = run_turn(&router, row(model), &[("user", "What is on my calendar?")], &db, Workload::Tools, false).await;
            assert_eq!(turn.reply.as_deref(), Ok("Hi."));
        }
        let seen = server.seen();
        let systems: Vec<String> = seen.iter().map(|request| system_of(&request.body)).collect();
        assert!(systems[0].ends_with(crate::ai::local::NO_TOOLS_NOTE), "chat only: {}", systems[0]);
        assert!(systems[1].ends_with(crate::ai::local::NO_TOOLS_NOTE), "small context");
        assert_eq!(systems.iter().filter(|system| system.contains(crate::ai::local::NO_TOOLS_NOTE)).count(), 2, "described or native tools: not told");
        assert!(crate::ai::local::NO_TOOLS_NOTE.contains("this model cannot use tools"));
    }

    /// A refusal is not sent again and stores nothing.
    #[tokio::test]
    async fn a_refused_request() {
        let server = serve(vec![Reply::json(500, json!({"error": "model not loaded"}))]).await;
        let db = crate::db::test_database();
        let turn = run_turn(&router(&server.base, BackendType::Vllm), row("my-model"), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err(format!("Local LLM API error 500 Internal Server Error: {}", json!({"error": "model not loaded"}))));
        assert_eq!(server.seen().len(), 1);
        assert!(rows(&db).is_empty());
    }

    /// Pieces with neither an index nor an id: a piece goes to the call at
    /// its position in the chunk. A call streamed in pieces, with the id and
    /// name on the first, is one call (as before P07); two whole calls in
    /// one chunk are two calls (before P07 a local server's pieces without
    /// an index all went to the first call).
    #[tokio::test]
    async fn pieces_without_an_index_or_an_id_go_by_position() {
        let server = serve(vec![
            Reply::openai_sse(&[
                json!({"choices": [{"delta": {"tool_calls": [{"id": "call_a", "type": "function", "function": {"name": "remember", "arguments": "{\"content\":"}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"function": {"arguments": "\"Kenny is my accountant\"}"}}]}, "finish_reason": "tool_calls"}]}),
            ]),
            text_reply("Saved.", None),
            Reply::openai_sse(&[json!({"choices": [{"delta": {"tool_calls": [
                {"type": "function", "function": {"name": "remember", "arguments": "{\"content\":\"First fact\"}"}},
                {"type": "function", "function": {"name": "remember", "arguments": "{\"content\":\"Second fact\"}"}}
            ]}, "finish_reason": "tool_calls"}]})]),
            text_reply("Saved both.", None),
        ])
        .await;
        let db = crate::db::test_database();
        let router = router(&server.base, BackendType::Ollama);
        let turn = run_turn(&router, row("qwen3:8b"), &[("user", "Remember Kenny is my accountant")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved."));
        assert_eq!(turn.ledger.count(), 1);
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        let turn = run_turn(&router, row("qwen3:8b"), &[("user", "Remember two facts")], &db, Workload::Tools, true).await;
        assert_eq!(turn.reply.as_deref(), Ok("Saved both."));
        assert_eq!(turn.ledger.count(), 2);
        assert_eq!(memories(&db).len(), 3);
        let seen = server.seen();
        assert_eq!(seen[1].body["messages"][2]["tool_calls"], json!([{"id": "call_a", "type": "function", "function": {"name": "remember", "arguments": REMEMBER_ARGS}}]));
    }
}

// ---------------------------------------------------------------------------
// The parity matrix (P07): each feature that reaches a model, run the same
// way on every wire. The wire sections above pin each request in full; these
// tests hold the features to one behaviour across them.
// ---------------------------------------------------------------------------

mod parity {
    use super::*;
    use crate::ai::complete::{complete, Request};
    use crate::ai::local::backend::{BackendType, LocalEndpoint};

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(super) enum W {
        OpenAi,
        Compatible,
        Claude,
        Gemini,
        Bedrock,
        /// Best effort: a model on a local server that calls tools itself.
        Local,
    }

    const ALL: [W; 6] = [W::OpenAi, W::Compatible, W::Claude, W::Gemini, W::Bedrock, W::Local];

    const KEY: &str = "sk-fixture-parity-not-real";
    const COMPAT_MODEL: &str = "unlisted-chat-1";
    const LOCAL_MODEL: &str = "my-model";

    /// One wire set up against a loopback server, as Settings saves it.
    struct On {
        wire: W,
        server: Loopback,
        router: AiRouter,
        entry: ProviderChainEntry,
        provider: String,
        _pointed: Option<PointedAt>,
    }

    impl On {
        /// The rows requests of these figures are stored as: one per turn,
        /// or one per request on Bedrock.
        fn rows(&self, feature: &str, model: &str, requests: &[(i64, i64)]) -> Vec<Row> {
            let figures: Vec<(i64, i64, i64, i64)> = requests.iter().map(|&(input, output)| (input, output, 0, 0)).collect();
            match self.wire {
                W::Bedrock => figures.iter().map(|request| priced_row(&self.provider, feature, model, &[*request])).collect(),
                W::Local => vec![Row {
                    provider: "local".into(),
                    feature: feature.into(),
                    model: model.into(),
                    input: requests.iter().map(|r| r.0).sum(),
                    output: requests.iter().map(|r| r.1).sum(),
                    cache_read: 0,
                    cache_write: 0,
                    cost: Some(0),
                    price_unknown: false,
                    registry_version: crate::ai::registry::current().version as i64,
                }],
                _ => vec![priced_row(&self.provider, feature, model, &figures)],
            }
        }

        async fn turn(&self, text: &str, db: &crate::db::Database, approve: bool) -> Turn {
            run_turn(&self.router, self.entry.clone(), &[("user", text)], db, Workload::Tools, approve).await
        }
    }

    impl W {
        async fn serve(self, replies: Vec<Reply>, db: &crate::db::Database) -> On {
            let server = serve(replies).await;
            let base = server.base.clone();
            let mut router = AiRouter::new("gemini_primary");
            let row = |provider_type: &str, model: Option<&str>, base_url: Option<String>, endpoint_id: Option<&str>| ProviderChainEntry {
                position: 0,
                provider_type: provider_type.into(),
                endpoint_id: endpoint_id.map(Into::into),
                model_id: model.map(Into::into),
                enabled: true,
                base_url,
            };
            let (entry, pointed) = match self {
                W::OpenAi => {
                    router.set_openai_key(KEY);
                    (row("openai", None, None, None), Some(point_at(&format!("{base}/v1"))))
                }
                W::Compatible => {
                    let url = format!("{base}/v1");
                    router.set_compat_key(&url, KEY);
                    (row("openai", Some(COMPAT_MODEL), Some(url), None), None)
                }
                W::Claude => {
                    router.set_claude_key(KEY);
                    (row("claude", None, None, None), Some(point_at(&base)))
                }
                W::Gemini => {
                    router.set_gemini_key(KEY);
                    (row("gemini", Some(crate::ai::models::GEMINI_FLASH), None, None), Some(point_at(&format!("{base}/v1beta"))))
                }
                W::Bedrock => (row("bedrock", Some(bedrock_wire::MODEL), None, None), Some(point_bedrock_at(&base))),
                W::Local => {
                    router.set_local_endpoints(vec![LocalEndpoint {
                        id: "lm".into(),
                        name: "Local".into(),
                        url: base.clone(),
                        backend_type: BackendType::Vllm,
                        api_key: None,
                        use_tls: false,
                        connection_timeout_ms: 5000,
                        keep_alive_minutes: 30,
                        is_active: true,
                        last_health_check: None,
                        last_health_status: None,
                    }]);
                    // The mail gate reads where the saved endpoint is.
                    db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO local_endpoints (id, name, url, backend_type) VALUES ('lm', 'Local', ?1, 'vllm')", [&base]).unwrap();
                    (row("local", Some(LOCAL_MODEL), None, Some("lm")), None)
                }
            };
            router.set_provider_chain(vec![entry.clone()]);
            let provider = cost::provider_id(&entry.provider_type, entry.base_url.as_deref());
            On { wire: self, server, router, entry, provider, _pointed: pointed }
        }

        /// A streamed answer, in two pieces, of 100 tokens in and 10 out.
        fn says(self, text: &str) -> Reply {
            let (head, tail) = text.split_at(text.len() / 2);
            match self {
                W::OpenAi | W::Compatible | W::Local => Reply::openai_sse(&[
                    json!({"choices": [{"delta": {"role": "assistant", "content": head}}]}),
                    json!({"choices": [{"delta": {"content": tail}, "finish_reason": "stop"}]}),
                    json!({"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10}}),
                ]),
                W::Claude => Reply::named_sse(&[
                    ("message_start", json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [], "usage": {"input_tokens": 100, "output_tokens": 1}}})),
                    ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})),
                    ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": head}})),
                    ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": tail}})),
                    ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
                    ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 10}})),
                    ("message_stop", json!({"type": "message_stop"})),
                ]),
                W::Gemini => Reply::sse(&[
                    json!({"candidates": [{"content": {"role": "model", "parts": [{"text": head}]}}]}),
                    json!({"candidates": [{"content": {"role": "model", "parts": [{"text": tail}]}, "finishReason": "STOP"}], "usageMetadata": {"promptTokenCount": 100, "candidatesTokenCount": 10}}),
                ]),
                W::Bedrock => bedrock_stream(&[
                    ("messageStart", json!({"role": "assistant"})),
                    ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"text": head}})),
                    ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"text": tail}})),
                    ("contentBlockStop", json!({"contentBlockIndex": 0})),
                    ("messageStop", json!({"stopReason": "end_turn"})),
                    ("metadata", json!({"usage": {"inputTokens": 100, "outputTokens": 10, "totalTokens": 110}})),
                ]),
            }
        }

        /// A streamed reply that calls one tool and says nothing, of 100
        /// tokens in and 10 out.
        fn calls(self, name: &str, args: Value) -> Reply {
            let text = args.to_string();
            match self {
                W::OpenAi | W::Compatible | W::Local => Reply::openai_sse(&[
                    json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": name, "arguments": ""}}]}}]}),
                    json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": text}}]}, "finish_reason": "tool_calls"}]}),
                    json!({"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10}}),
                ]),
                W::Claude => Reply::named_sse(&[
                    ("message_start", json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": [], "usage": {"input_tokens": 100, "output_tokens": 1}}})),
                    ("content_block_start", json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "call_1", "name": name, "input": {}}})),
                    ("content_block_delta", json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": text}})),
                    ("content_block_stop", json!({"type": "content_block_stop", "index": 0})),
                    ("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 10}})),
                    ("message_stop", json!({"type": "message_stop"})),
                ]),
                W::Gemini => Reply::sse(&[json!({
                    "candidates": [{"content": {"role": "model", "parts": [{"functionCall": {"name": name, "args": args}}]}, "finishReason": "STOP"}],
                    "usageMetadata": {"promptTokenCount": 100, "candidatesTokenCount": 10}
                })]),
                W::Bedrock => bedrock_stream(&[
                    ("messageStart", json!({"role": "assistant"})),
                    ("contentBlockStart", json!({"contentBlockIndex": 0, "start": {"toolUse": {"toolUseId": "call_1", "name": name}}})),
                    ("contentBlockDelta", json!({"contentBlockIndex": 0, "delta": {"toolUse": {"input": text}}})),
                    ("contentBlockStop", json!({"contentBlockIndex": 0})),
                    ("messageStop", json!({"stopReason": "tool_use"})),
                    ("metadata", json!({"usage": {"inputTokens": 100, "outputTokens": 10, "totalTokens": 110}})),
                ]),
            }
        }

        /// A plain completion's reply (not streamed), of 10 tokens in and 2
        /// out.
        fn completes(self, text: &str) -> Reply {
            match self {
                W::OpenAi | W::Compatible | W::Local => Reply::json(200, json!({"choices": [{"message": {"content": text}}], "usage": {"prompt_tokens": 10, "completion_tokens": 2}})),
                W::Claude => Reply::json(200, json!({"content": [{"type": "text", "text": text}], "usage": {"input_tokens": 10, "output_tokens": 2}})),
                W::Gemini => Reply::json(200, json!({"candidates": [{"content": {"parts": [{"text": text}]}}], "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 2}})),
                W::Bedrock => Reply::json(
                    200,
                    json!({"output": {"message": {"role": "assistant", "content": [{"text": text}]}}, "stopReason": "end_turn", "usage": {"inputTokens": 10, "outputTokens": 2, "totalTokens": 12}, "metrics": {"latencyMs": 5}}),
                ),
            }
        }

        /// The tool results a request carried back to the model, in order.
        fn results(self, body: &Value) -> Vec<String> {
            let items = |list: &Value| list.as_array().cloned().unwrap_or_default();
            match self {
                W::OpenAi | W::Compatible | W::Local => items(&body["messages"]).iter().filter(|m| m["role"] == "tool").map(|m| m["content"].as_str().unwrap().to_string()).collect(),
                W::Claude => items(&body["messages"])
                    .iter()
                    .flat_map(|m| items(&m["content"]))
                    .filter(|block| block["type"] == "tool_result")
                    .map(|block| block["content"].as_str().unwrap().to_string())
                    .collect(),
                W::Gemini => items(&body["contents"])
                    .iter()
                    .flat_map(|c| items(&c["parts"]))
                    .filter_map(|part| part["functionResponse"]["response"]["result"].as_str().map(str::to_string))
                    .collect(),
                W::Bedrock => items(&body["messages"])
                    .iter()
                    .flat_map(|m| items(&m["content"]))
                    .filter_map(|block| block["toolResult"]["content"][0]["text"].as_str().map(str::to_string))
                    .collect(),
            }
        }

        /// The model a request named.
        fn model_sent(self, seen: &Seen) -> String {
            match self {
                W::Gemini => seen.path.split("/models/").nth(1).unwrap().split(':').next().unwrap().to_string(),
                W::Bedrock => seen.path.trim_start_matches("/model/").split('/').next().unwrap().to_string(),
                _ => seen.body["model"].as_str().unwrap().to_string(),
            }
        }

        /// Where a plain completion of `model` is sent.
        fn plain_path(self, model: &str) -> String {
            match self {
                W::OpenAi | W::Compatible | W::Local => "/v1/chat/completions".into(),
                W::Claude => "/v1/messages".into(),
                W::Gemini => format!("/v1beta/models/{model}:generateContent"),
                W::Bedrock => format!("/model/{model}/converse"),
            }
        }

        /// The plain request asks for one JSON object, by the provider's own
        /// switch where it has one, else in words.
        fn asks_for_json(self, body: &Value) -> bool {
            let in_words = |text: Option<&str>| text.is_some_and(|text| text.contains(crate::ai::bedrock::JSON_INSTRUCTION));
            match self {
                W::OpenAi | W::Compatible | W::Local => body["response_format"] == json!({"type": "json_object"}),
                W::Claude => in_words(body["system"].as_str()),
                W::Gemini => body["generationConfig"]["responseMimeType"] == "application/json",
                W::Bedrock => body["system"].as_array().is_some_and(|blocks| blocks.iter().any(|block| in_words(block["text"].as_str()))),
            }
        }
    }

    fn bedrock_stream(events: &[(&str, Value)]) -> Reply {
        Reply::raw(200, crate::ai::bedrock::tests::EVENT_STREAM, crate::ai::bedrock::tests::converse_stream(events))
    }

    /// Runs `test` once per wire, each on its own database with AWS
    /// credentials saved (Bedrock's; the others ignore them).
    fn on_each<F, Fut>(wires: &[W], test: F)
    where
        F: Fn(W, Arc<crate::db::Database>) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        for &wire in wires {
            bedrock_wire::with_credentials(|db| test(wire, db));
        }
    }

    fn running_line(turn: &Turn, name: &str, args: &Value) -> bool {
        turn.window_lines().contains(&running(tools::tool_status_label(name), json!(tools::tool_status_detail(name, &args.to_string()))))
    }

    fn tasks(db: &crate::db::Database) -> Vec<(String, Option<String>)> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT title, deadline FROM tasks ORDER BY id").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    /// Chat and streaming: the answer is shown and spoken as it comes, and
    /// the turn is stored under chat.
    #[test]
    fn chat_streams_the_answer() {
        on_each(&ALL, |wire, db| async move {
            let on = wire.serve(vec![wire.says("Hello there.")], &db).await;
            let turn = on.turn("Hi", &db, false).await;
            assert_eq!(turn.reply.as_deref(), Ok("Hello there."), "{wire:?}");
            let seen = on.server.seen();
            assert_eq!(seen.len(), 1, "{wire:?}");
            let tokens: Vec<String> = turn.window_lines().into_iter().filter(|line| line.starts_with("chat-token")).collect();
            assert_eq!(tokens, vec![token("Hello "), token("there."), token_done()], "{wire:?}");
            assert_eq!(turn.voice, vec!["text: Hello ", "text: there."], "{wire:?}");
            assert_eq!(turn.ledger.count(), 0, "{wire:?}");
            assert_eq!(rows(&db), on.rows("chat", &wire.model_sent(&seen[0]), &[(100, 10)]), "{wire:?}");
        });
    }

    /// A call that changes the user's data waits for the approval card:
    /// allowed, it runs once and its result goes back to the model; declined,
    /// it does not run and the model is told.
    #[test]
    fn a_tool_call_waits_for_approval() {
        on_each(&ALL, |wire, db| async move {
            let args = json!({"content": "Kenny is my accountant"});
            let on = wire.serve(vec![wire.calls("remember", args.clone()), wire.says("Saved it.")], &db).await;
            let turn = on.turn("Remember Kenny is my accountant", &db, true).await;
            assert_eq!(turn.reply.as_deref(), Ok("Saved it."), "{wire:?}");
            assert_eq!(memories(&db), vec!["Kenny is my accountant"], "{wire:?}");
            assert_eq!(turn.ledger.count(), 1, "{wire:?}");
            let seen = on.server.seen();
            assert_eq!(seen.len(), 2, "{wire:?}");
            assert_eq!(wire.results(&seen[1].body), vec![remembered("Kenny is my accountant").await], "{wire:?}");
            assert!(running_line(&turn, "remember", &args), "{wire:?}");
            assert_eq!(rows(&db), on.rows("tools", &wire.model_sent(&seen[0]), &[(100, 10), (100, 10)]), "{wire:?}");
        });
        on_each(&ALL, |wire, db| async move {
            let on = wire.serve(vec![wire.calls("remember", json!({"content": "Kenny is my accountant"})), wire.says("Not saved.")], &db).await;
            let turn = on.turn("Remember Kenny is my accountant", &db, false).await;
            assert_eq!(turn.reply.as_deref(), Ok("Not saved."), "{wire:?}");
            assert!(memories(&db).is_empty(), "{wire:?}");
            // Noted when it was asked for, as before P07: a turn that may
            // have acted is never sent again.
            assert_eq!(turn.ledger.count(), 1, "{wire:?}");
            let results = wire.results(&on.server.seen()[1].body);
            assert_eq!(results.len(), 1, "{wire:?}");
            assert!(results[0].starts_with("The user did not allow remember, so nothing was changed."), "{wire:?}: {}", results[0]);
        });
    }

    /// Calendar tools: the call reaches the calendar with its arguments
    /// whole (the status line shows the title it carried). With no Google
    /// account in a test the calendar answers without a request to Google,
    /// and that answer goes back to the model. Deleting an event asks first.
    #[test]
    fn calendar_tools() {
        on_each(&ALL, |wire, db| async move {
            let event = json!({"title": "Dentist", "start": "2026-12-01T10:00:00", "end": "2026-12-01T11:00:00"});
            let on = wire.serve(vec![wire.calls("create_event", event.clone()), wire.says("I could not add it.")], &db).await;
            let turn = on.turn("Add the dentist on 1 December at 10", &db, true).await;
            assert_eq!(turn.reply.as_deref(), Ok("I could not add it."), "{wire:?}");
            assert!(running_line(&turn, "create_event", &event), "{wire:?}: {:?}", turn.window_lines());
            assert_eq!(wire.results(&on.server.seen()[1].body), vec!["Google account not connected. Please connect in Settings to use Calendar tools."], "{wire:?}");
            drop(on);

            let on = wire.serve(vec![wire.calls("delete_event", json!({"event_id": "evt_1"})), wire.says("Kept it.")], &db).await;
            let turn = on.turn("Delete that event", &db, false).await;
            assert_eq!(turn.reply.as_deref(), Ok("Kept it."), "{wire:?}");
            let results = wire.results(&on.server.seen()[1].body);
            assert!(results[0].starts_with("The user did not allow delete_event, so nothing was changed."), "{wire:?}: {results:?}");
        });
    }

    /// Reminders: a task with a date, saved without asking, flagged on that
    /// day and never promised at a time.
    #[test]
    fn a_reminder_is_a_dated_task() {
        on_each(&ALL, |wire, db| async move {
            let args = json!({"title": "Pay the invoice", "deadline": "2027-01-15"});
            let on = wire.serve(vec![wire.calls("create_task", args.clone()), wire.says("I will flag it that day.")], &db).await;
            // Declining would stop it if a card were shown: none is.
            let turn = on.turn("Remind me to pay the invoice on 15 January", &db, false).await;
            assert_eq!(turn.reply.as_deref(), Ok("I will flag it that day."), "{wire:?}");
            assert_eq!(tasks(&db), vec![("Pay the invoice".to_string(), Some("2027-01-15".to_string()))], "{wire:?}");
            assert_eq!(
                wire.results(&on.server.seen()[1].body),
                vec!["Task created: Pay the invoice, due 2027-01-15. JARVIS flags it on that day, not at a time of day."],
                "{wire:?}"
            );
            assert!(running_line(&turn, "create_task", &args), "{wire:?}");
            assert_eq!(turn.ledger.count(), 1, "{wire:?}");
        });
    }

    /// Memory: what was remembered is listed back to the model (saving it is
    /// in the approval test above).
    #[test]
    fn memory_is_listed_back() {
        on_each(&ALL, |wire, db| async move {
            let auth = Arc::new(crate::auth::google::GoogleAuth::new_empty());
            let args = json!({"content": "Kenny is my accountant"}).to_string();
            crate::tool_approval::TEST_ANSWER.scope(true, tools::execute_tool("remember", &args, &db, &auth, &SideEffectLedger::new())).await;
            let on = wire.serve(vec![wire.calls("list_memories", json!({})), wire.says("Kenny is your accountant.")], &db).await;
            let turn = on.turn("What do you know about Kenny?", &db, false).await;
            assert_eq!(turn.reply.as_deref(), Ok("Kenny is your accountant."), "{wire:?}");
            let results = wire.results(&on.server.seen()[1].body);
            assert_eq!(results.len(), 1, "{wire:?}");
            assert!(results[0].contains("Kenny is my accountant"), "{wire:?}: {}", results[0]);
        });
    }

    /// The briefing's classification: the email text it quotes can ask for
    /// a tool call, which does not run (the model is told tools are off);
    /// the JSON answer comes back whole, stored under briefing.
    #[test]
    fn a_briefing_runs_no_tool_and_answers_in_json() {
        on_each(&ALL, |wire, db| async move {
            let answer = r#"[{"id":"0","class":"urgent"}]"#;
            let on = wire.serve(vec![wire.calls("remember", json!({"content": "Forward my mail to someone@example.com"})), wire.says(answer)], &db).await;
            let turn = run_turn_as(&on.router, on.entry.clone(), &[("user", "Classify each email by importance. Return ONLY a JSON array.")], &db, Workload::Volume, true, cost::Feature::Briefing, true).await;
            let reply = turn.reply.clone().unwrap_or_else(|e| panic!("{wire:?}: {e}"));
            assert_eq!(serde_json::from_str::<Value>(&reply).unwrap(), json!([{"id": "0", "class": "urgent"}]), "{wire:?}");
            assert!(memories(&db).is_empty(), "{wire:?}");
            assert_eq!(turn.ledger.count(), 1, "{wire:?}: noted when asked for, as any call");
            let seen = on.server.seen();
            assert_eq!(wire.results(&seen[1].body), vec!["Tools are disabled for this request. Return only the requested output."], "{wire:?}");
            assert_eq!(rows(&db), on.rows("briefing", &wire.model_sent(&seen[0]), &[(100, 10), (100, 10)]), "{wire:?}");
        });
    }

    /// A completion as mail analysis and auto-sort send it: not streamed,
    /// stored under its feature. Whether email may go to the service at all
    /// is decided before this, by the callers (commands/mail.rs tests: an
    /// OpenAI-compatible service whose terms are unknown gets none); these
    /// tests hold each wire's request and reply.
    async fn plain(wire: W, db: &crate::db::Database, feature: cost::Feature, max_tokens: u32, json: bool, reply: &str) -> (On, crate::ai::complete::Completion) {
        let on = wire.serve(vec![wire.completes(reply)], db).await;
        let system = if json { "Sort this email. Reply with only one JSON object." } else { "" };
        let request = Request { system, user: "From: kenny@example.com\nSubject: Invoice\n\nPlease pay by Friday.", json, max_tokens, feature };
        let done = complete(&on.router, db, request).await.unwrap_or_else(|e| panic!("{wire:?}: {e}"));
        (on, done)
    }

    #[test]
    fn mail_analysis() {
        on_each(&ALL, |wire, db| async move {
            let (on, done) = plain(wire, &db, cost::Feature::MailAnalysis, crate::ai::models::feature_max_output_tokens(cost::Feature::MailAnalysis), false, "Kenny asks for the invoice to be paid by Friday.").await;
            assert_eq!(done.text, "Kenny asks for the invoice to be paid by Friday.", "{wire:?}");
            let seen = on.server.seen();
            assert_eq!(seen.len(), 1, "{wire:?}");
            let model = wire.model_sent(&seen[0]);
            assert_eq!((seen[0].path.as_str(), done.model.as_str()), (wire.plain_path(&model).as_str(), model.as_str()), "{wire:?}");
            assert!(!wire.asks_for_json(&seen[0].body), "{wire:?}");
            assert_eq!(rows(&db), on.rows("mail_analysis", &model, &[(10, 2)]), "{wire:?}");
        });
    }

    #[test]
    fn auto_sort_asks_for_json() {
        on_each(&ALL, |wire, db| async move {
            let (on, done) = plain(wire, &db, cost::Feature::MailSort, crate::ai::models::feature_max_output_tokens(cost::Feature::MailSort), true, r#"{"category": "action", "needs_reply": true}"#).await;
            assert_eq!(serde_json::from_str::<Value>(&done.text).unwrap(), json!({"category": "action", "needs_reply": true}), "{wire:?}");
            let seen = on.server.seen();
            assert_eq!(seen.len(), 1, "{wire:?}");
            let model = wire.model_sent(&seen[0]);
            assert_eq!(seen[0].path, wire.plain_path(&model), "{wire:?}");
            assert!(wire.asks_for_json(&seen[0].body), "{wire:?}: {}", seen[0].body);
            assert_eq!(rows(&db), on.rows("mail_sort", &model, &[(10, 2)]), "{wire:?}");
        });
    }
}

// ---------------------------------------------------------------------------
// Live smoke tests (P07): the real services, never run by default. Each is
// ignored, and runs only with its JARVIS_LIVE_<PROVIDER>_KEY set, e.g.
//
//   JARVIS_LIVE_OPENAI_KEY=... cargo test --manifest-path src-tauri/Cargo.toml live_openai -- --ignored --nocapture
//
// Each sends one chat turn that should call `remember` (approved), then one
// JSON completion, and prints only counts and lengths. The key is used for
// the requests only: it is not printed, logged or saved.
// ---------------------------------------------------------------------------

mod live {
    use super::*;
    use crate::ai::complete::{complete, Request};

    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
    }

    fn entry(provider_type: &str, model: Option<String>, base_url: Option<String>, endpoint_id: Option<&str>) -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id: endpoint_id.map(Into::into), model_id: model, enabled: true, base_url }
    }

    /// `tools_expected`: a cloud model is held to calling the tool; a local
    /// one is best effort.
    async fn smoke(name: &str, mut router: AiRouter, entry: ProviderChainEntry, db: &crate::db::Database, tools_expected: bool) {
        let turn = run_turn(&router, entry.clone(), &[("user", "Use the remember tool to save this fact: my live test colour is blue. Then answer in one short sentence.")], db, Workload::Tools, true).await;
        let reply = turn.reply.unwrap_or_else(|e| panic!("{name} chat: {e}"));
        assert!(!reply.trim().is_empty(), "{name}: an answer");
        let saved = memories(db).len();
        if tools_expected {
            assert_eq!(saved, 1, "{name}: the remember call ran once");
        }
        assert!(!rows(db).is_empty(), "{name}: the turn is stored");
        router.set_provider_chain(vec![entry]);
        let done = complete(&router, db, Request { system: "", user: "Return a JSON object with one field \"ok\" set to true.", json: true, max_tokens: 1_024, feature: cost::Feature::Other })
            .await
            .unwrap_or_else(|e| panic!("{name} completion: {e}"));
        let text = done.text.trim().trim_start_matches("```json").trim_end_matches("```").trim();
        let parsed: Value = serde_json::from_str(text).unwrap_or_else(|_| panic!("{name}: the completion is not JSON ({} characters)", done.text.len()));
        assert_eq!(parsed["ok"], true, "{name}");
        println!("{name}: answer of {} characters, {saved} memory saved, JSON completion ok, {} usage rows", reply.chars().count(), rows(db).len());
    }

    fn skipped(names: &str) {
        println!("skipped: set {names} to run this live test");
    }

    #[tokio::test]
    #[ignore]
    async fn live_openai() {
        let Some(key) = var("JARVIS_LIVE_OPENAI_KEY") else { return skipped("JARVIS_LIVE_OPENAI_KEY") };
        let mut router = AiRouter::new("gemini_primary");
        router.set_openai_key(&key);
        smoke("openai", router, entry("openai", None, None, None), &crate::db::test_database(), true).await;
    }

    #[tokio::test]
    #[ignore]
    async fn live_claude() {
        let Some(key) = var("JARVIS_LIVE_CLAUDE_KEY") else { return skipped("JARVIS_LIVE_CLAUDE_KEY") };
        let mut router = AiRouter::new("gemini_primary");
        router.set_claude_key(&key);
        smoke("claude", router, entry("claude", None, None, None), &crate::db::test_database(), true).await;
    }

    #[tokio::test]
    #[ignore]
    async fn live_gemini() {
        let Some(key) = var("JARVIS_LIVE_GEMINI_KEY") else { return skipped("JARVIS_LIVE_GEMINI_KEY") };
        let mut router = AiRouter::new("gemini_primary");
        router.set_gemini_key(&key);
        smoke("gemini", router, entry("gemini", None, None, None), &crate::db::test_database(), true).await;
    }

    /// Any OpenAI-compatible service: its address and a chat model on it.
    #[tokio::test]
    #[ignore]
    async fn live_compatible() {
        let (Some(key), Some(url), Some(model)) = (var("JARVIS_LIVE_COMPAT_KEY"), var("JARVIS_LIVE_COMPAT_URL"), var("JARVIS_LIVE_COMPAT_MODEL")) else {
            return skipped("JARVIS_LIVE_COMPAT_KEY, JARVIS_LIVE_COMPAT_URL and JARVIS_LIVE_COMPAT_MODEL");
        };
        let mut router = AiRouter::new("gemini_primary");
        router.set_compat_key(&url, &key);
        smoke("compatible", router, entry("openai", Some(model), Some(url), None), &crate::db::test_database(), true).await;
    }

    /// The access key id in JARVIS_LIVE_BEDROCK_KEY, its secret in
    /// JARVIS_LIVE_BEDROCK_SECRET, and the region (us-east-1 when unset).
    /// The credentials go to the test build's in-memory Keychain stand-in.
    #[test]
    #[ignore]
    fn live_bedrock() {
        let (Some(key), Some(secret)) = (var("JARVIS_LIVE_BEDROCK_KEY"), var("JARVIS_LIVE_BEDROCK_SECRET")) else {
            return skipped("JARVIS_LIVE_BEDROCK_KEY and JARVIS_LIVE_BEDROCK_SECRET");
        };
        let region = var("JARVIS_LIVE_BEDROCK_REGION").unwrap_or_else(|| "us-east-1".into());
        let _keychain = crate::aws::credentials::tests::KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let db = Arc::new(crate::db::test_database());
            let creds = crate::aws::credentials::AwsCredentials {
                access_key_id: key,
                secret_access_key: secret,
                inference_profile: crate::aws::credentials::inference_profile_for_region(&region).into(),
                region,
            };
            crate::aws::credentials::AwsCredsRepo::new(db.clone()).save(&creds).await.unwrap();
            smoke("bedrock", AiRouter::new("gemini_primary"), entry("bedrock", None, None, None), &db, true).await;
        });
        let _ = crate::security::keychain::KeychainStore::delete("com.jarvis.aws", "secret_access_key");
    }

    /// Best effort: a model on a local server (JARVIS_LIVE_LOCAL_URL, e.g.
    /// http://127.0.0.1:11434, and JARVIS_LIVE_LOCAL_MODEL). It need not
    /// call the tool; the turn and the JSON completion must work.
    #[tokio::test]
    #[ignore]
    async fn live_local() {
        let (Some(url), Some(model)) = (var("JARVIS_LIVE_LOCAL_URL"), var("JARVIS_LIVE_LOCAL_MODEL")) else {
            return skipped("JARVIS_LIVE_LOCAL_URL and JARVIS_LIVE_LOCAL_MODEL");
        };
        let backend_type = crate::ai::local::detect_backend(&url).await;
        let mut router = AiRouter::new("gemini_primary");
        router.set_local_endpoints(vec![crate::ai::local::backend::LocalEndpoint {
            id: "live".into(),
            name: "Live".into(),
            url,
            backend_type,
            api_key: var("JARVIS_LIVE_LOCAL_KEY"),
            use_tls: false,
            connection_timeout_ms: 5000,
            keep_alive_minutes: 30,
            is_active: true,
            last_health_check: None,
            last_health_status: None,
        }]);
        smoke("local", router, entry("local", Some(model), None, Some("live")), &crate::db::test_database(), false).await;
    }
}

// ---------------------------------------------------------------------------
// How many requests a temporarily unavailable service (503) is sent, on each
// wire, through the router's one resend (run_chosen): a chat turn as chat
// sends it, and a completion as mail analysis sends it.
// ---------------------------------------------------------------------------

mod resend_counts {
    use super::*;
    use crate::ai::complete::{complete, Request};
    use crate::ai::local::backend::{BackendType, LocalEndpoint};

    const KEY: &str = "sk-fixture-resend-not-real";

    #[derive(Clone, Copy, Debug)]
    enum W {
        OpenAi,
        Compatible,
        Claude,
        Gemini,
        Bedrock,
        Local,
    }

    /// A 503 as each service sends it: AWS names the error in a header.
    fn busy(wire: W) -> Reply {
        match wire {
            W::Bedrock => Reply::json(503, json!({"message": "The service is busy."})).header("x-amzn-ErrorType", "ServiceUnavailableException"),
            _ => Reply::json(503, json!({"error": {"message": "The service is busy."}})),
        }
    }

    /// The router and its one saved row for `wire`, pointed at `base`.
    fn set_up(wire: W, base: &str) -> (AiRouter, ProviderChainEntry, Option<PointedAt>) {
        let mut router = AiRouter::new("gemini_primary");
        let row = |provider_type: &str, model: Option<&str>, base_url: Option<String>, endpoint_id: Option<&str>| ProviderChainEntry {
            position: 0,
            provider_type: provider_type.into(),
            endpoint_id: endpoint_id.map(Into::into),
            model_id: model.map(Into::into),
            enabled: true,
            base_url,
        };
        let (entry, pointed) = match wire {
            W::OpenAi => {
                router.set_openai_key(KEY);
                (row("openai", None, None, None), Some(point_at(&format!("{base}/v1"))))
            }
            W::Compatible => {
                let url = format!("{base}/v1");
                router.set_compat_key(&url, KEY);
                (row("openai", Some("unlisted-chat-1"), Some(url), None), None)
            }
            W::Claude => {
                router.set_claude_key(KEY);
                (row("claude", None, None, None), Some(point_at(base)))
            }
            W::Gemini => {
                router.set_gemini_key(KEY);
                (row("gemini", Some(crate::ai::models::GEMINI_FLASH), None, None), Some(point_at(&format!("{base}/v1beta"))))
            }
            W::Bedrock => (row("bedrock", Some(bedrock_wire::MODEL), None, None), Some(point_bedrock_at(base))),
            W::Local => {
                router.set_local_endpoints(vec![LocalEndpoint {
                    id: "lm".into(),
                    name: "Local".into(),
                    url: base.into(),
                    backend_type: BackendType::Vllm,
                    api_key: None,
                    use_tls: false,
                    connection_timeout_ms: 5000,
                    keep_alive_minutes: 30,
                    is_active: true,
                    last_health_check: None,
                    last_health_status: None,
                }]);
                (row("local", Some("my-model"), None, Some("lm")), None)
            }
        };
        router.set_provider_chain(vec![entry.clone()]);
        (router, entry, pointed)
    }

    /// Requests a chat turn sends to a busy service, through run_chosen as
    /// the router calls it.
    async fn chat_requests(wire: W, db: &crate::db::Database) -> usize {
        let server = serve((0..8).map(|_| busy(wire)).collect()).await;
        let (router, entry, _pointed) = set_up(wire, &server.base);
        let window = Window::default();
        let ledger = SideEffectLedger::new();
        let auth = Arc::new(crate::auth::google::GoogleAuth::new_empty());
        let configured = vec![entry.clone()];
        let messages: Vec<(String, String)> = vec![("user".into(), "Hi".into())];
        let (messages, window_ref, auth_ref, ledger_ref, router_ref) = (&messages, &window, &auth, &ledger, &router);
        let reply = cost::with_feature(
            cost::Feature::Chat,
            crate::ai::run_chosen(
                Some(entry),
                None,
                &configured,
                cost::Feature::Chat,
                Workload::Tools,
                &ledger,
                crate::ai::bedrock::UiLanguage::En,
                &crate::ai::resends_inside,
                &|_| true,
                |_, _| {},
                move |row| router_ref.send_one(row, messages.clone(), db, auth_ref, window_ref, None, Workload::Tools, ledger_ref),
            ),
        )
        .await;
        assert!(reply.is_err(), "{wire:?}: {reply:?}");
        server.seen().len()
    }

    /// Requests a completion sends to a busy service.
    async fn completion_requests(wire: W, db: &crate::db::Database) -> usize {
        let server = serve((0..8).map(|_| busy(wire)).collect()).await;
        let (router, _entry, _pointed) = set_up(wire, &server.base);
        let request = Request { system: "", user: "From: kenny@example.com\nSubject: Invoice\n\nPlease pay by Friday.", json: false, max_tokens: 300, feature: cost::Feature::MailAnalysis };
        let done = complete(&router, db, request).await;
        assert!(done.is_err(), "{wire:?}");
        server.seen().len()
    }

    fn check(wire: W, chat: usize, completion: usize) {
        bedrock_wire::with_credentials(|db| async move {
            assert_eq!(chat_requests(wire, &db).await, chat, "{wire:?}: chat");
            assert_eq!(completion_requests(wire, &db).await, completion, "{wire:?}: completion");
        });
    }

    /// The chat wire sends a 503 once more itself, so the router does not;
    /// a completion is sent once by the wire and once more by the router.
    #[test]
    fn openai() {
        check(W::OpenAi, 2, 2);
    }

    #[test]
    fn compatible() {
        check(W::Compatible, 2, 2);
    }

    #[test]
    fn claude() {
        check(W::Claude, 2, 2);
    }

    #[test]
    fn gemini() {
        check(W::Gemini, 2, 2);
    }

    /// The router reads each wire's own statement: every wire, built as
    /// send_one builds it, says what `resends_inside` says for its row.
    #[test]
    fn the_router_reads_what_each_wire_states() {
        use crate::ai::provider::Wire;
        let row = |provider_type: &str, base_url: Option<&str>| ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id: None, model_id: Some("m".into()), enabled: true, base_url: base_url.map(Into::into) };
        let endpoint = LocalEndpoint { id: "lm".into(), name: "Local".into(), url: "http://127.0.0.1:9".into(), backend_type: BackendType::Vllm, api_key: None, use_tls: false, connection_timeout_ms: 5000, keep_alive_minutes: 30, is_active: true, last_health_check: None, last_health_status: None };
        let official = crate::ai::openai::OpenAiWire::new(KEY, crate::ai::openai::Target { base_url: None, model: Some("m"), deeper: None }).unwrap();
        let compatible = crate::ai::openai::OpenAiWire::new(KEY, crate::ai::openai::Target { base_url: Some("http://127.0.0.1:9/v1"), model: Some("m"), deeper: None }).unwrap();
        let local = crate::ai::openai::OpenAiWire::local(crate::ai::local::LocalChat::plain(endpoint), "m");
        let claude = crate::ai::claude::ClaudeWire::new(KEY, "m");
        let gemini = crate::ai::gemini::GeminiWire::new(KEY, "m", None);
        let bedrock = crate::ai::bedrock::BedrockWire::new(crate::ai::bedrock::tests::fixture_creds("us-east-1"), "m");
        let stated = [
            (row("openai", None), official.resends_transient()),
            (row("openai", Some("http://127.0.0.1:9/v1")), compatible.resends_transient()),
            (row("local", None), local.resends_transient()),
            (row("claude", None), claude.resends_transient()),
            (row("gemini", None), gemini.resends_transient()),
            (row("bedrock", None), bedrock.resends_transient()),
        ];
        for (row, resends) in stated {
            assert_eq!(crate::ai::resends_inside(&row), resends, "{} {:?}", row.provider_type, row.base_url);
        }
        assert!(official.resends_transient() && compatible.resends_transient(), "OpenAI and the compatible services resend a 429 or 5xx themselves");
    }

    /// The AWS SDK makes three attempts itself (its standard retry), and
    /// the router sends the request once more: six, as before P07.
    #[test]
    fn bedrock() {
        check(W::Bedrock, 6, 6);
    }

    #[test]
    fn local() {
        check(W::Local, 2, 2);
    }
}

// ---------------------------------------------------------------------------
// The shared loop (provider::chat) on a wire written for the test.
// ---------------------------------------------------------------------------

mod shared_loop {
    use super::*;
    use crate::ai::provider::{Call, ChatRequest, Event, Events, Metering, Outcome, Round, RoundError, Usage, Wire};

    /// A wire whose first reply calls `remember`, and whose results cannot
    /// be added to the next request (as a Converse builder that fails).
    struct ResultsFail;

    #[async_trait::async_trait]
    impl Wire for ResultsFail {
        type Conversation = ();

        fn max_rounds(&self) -> usize {
            3
        }

        fn metering(&self) -> Metering {
            Metering::Turn { provider: "openai".into(), model: crate::ai::models::openai_default() }
        }

        async fn start(&self, _request: ChatRequest) -> Result<(), String> {
            Ok(())
        }

        async fn round(&self, _conversation: &mut (), _final_round: bool, events: &mut Events<'_>) -> Result<(), RoundError> {
            events.send(Event::ToolCall(Call { id: "call_1".into(), name: "remember".into(), arguments: REMEMBER_ARGS.into() }));
            events.send(Event::Usage(Usage { input: 100, output: 10, cache_read: 0, cache_write: 0 }));
            events.send(Event::Done { stop: Some("tool_calls".into()) });
            Ok(())
        }

        fn add_results(&self, _conversation: &mut (), _round: Round, _results: Vec<(Call, Outcome)>, _next_is_final: bool) -> Result<(), String> {
            Err("The results could not be added.".into())
        }

        async fn complete(&self, _request: &crate::ai::complete::Request<'_>, _language: crate::ai::bedrock::UiLanguage) -> Result<crate::ai::complete::Completion, String> {
            Err("not used".into())
        }
    }

    /// Results that cannot be added end the turn as a failed request does:
    /// what the turn was billed is stored, and the call that ran is in the
    /// ledger, so the router does not send the request again.
    #[tokio::test]
    async fn results_that_cannot_be_added_fail_the_turn_as_a_request_does() {
        let db = crate::db::test_database();
        let window = Window::default();
        let ledger = SideEffectLedger::new();
        let auth = Arc::new(crate::auth::google::GoogleAuth::new_empty());
        let request = ChatRequest { system: crate::ai::provider::SystemPrompt::for_request(&db), messages: vec![("user".into(), "Remember Kenny is my accountant".into())], tools: tools::get_tool_definitions(), deeper: None };
        let turn = crate::ai::provider::Turn { db: &db, google_auth: &auth, window: &window, voice: None, workload: Workload::Tools, ledger: &ledger };
        let reply = crate::tool_approval::TEST_ANSWER.scope(true, cost::with_feature(cost::Feature::Chat, crate::ai::provider::chat(&ResultsFail, request, turn))).await;
        assert_eq!(reply, Err("The results could not be added.".to_string()));
        assert_eq!(ledger.count(), 1);
        assert_eq!(memories(&db), vec!["Kenny is my accountant"]);
        let model = crate::ai::models::openai_default();
        assert_eq!(rows(&db), vec![priced_row("openai", "tools", &model, &[(100, 10, 0, 0)])]);
    }
}
