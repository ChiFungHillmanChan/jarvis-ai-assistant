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
    let window = Window::default();
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let ledger = SideEffectLedger::new();
    let auth = Arc::new(crate::auth::google::GoogleAuth::new_empty());
    let messages = messages.iter().map(|(role, text)| (role.to_string(), text.to_string())).collect();
    let reply = crate::tool_approval::TEST_ANSWER
        .scope(approve, cost::with_feature(cost::Feature::Chat, router.send_one(entry, messages, db, &auth, &window, Some(tx), workload, &ledger)))
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

    /// A tool argument the schema asks for as JSON text comes back as the
    /// object before the tool runs.
    #[test]
    fn json_text_arguments_are_restored() {
        let tool = tools::get_tool_definitions().into_iter().find(|t| crate::ai::gemini::sanitize_schema(&t.parameters).to_string().contains("JSON object written as a string"));
        assert!(tool.is_some(), "a tool with an open object field exists for this pin");
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

    fn converse(content: Value, stop: &str, usage: Value) -> Reply {
        Reply::json(200, json!({"output": {"message": {"role": "assistant", "content": content}}, "stopReason": stop, "usage": usage, "metrics": {"latencyMs": 5}}))
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
            let server = serve(vec![Reply::json(200, crate::ai::bedrock::tests::converse_json("Hello."))]).await;
            let _aws = point_bedrock_at(&server.base);
            let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("assistant", "Earlier"), ("user", "Hi"), ("assistant", " "), ("user", "What is on today?")], &db, Workload::Tools, false).await;
            assert_eq!(turn.reply.as_deref(), Ok("Hello."));
            let seen = server.seen();
            assert_eq!(seen.len(), 1);
            assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", format!("/model/{MODEL}/converse").as_str()));
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
            // Converse is not streamed: the whole answer arrives at once.
            assert_eq!(turn.window_lines(), vec![status("Composing response...", "responding"), token("Hello."), token_done()]);
            assert_eq!(turn.voice, vec!["text: Hello."]);
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
            // The call is announced when it runs; the text of a round that
            // called a tool is not shown.
            assert_eq!(
                turn.window_lines(),
                vec![
                    running(tools::tool_status_label("remember"), json!(tools::tool_status_detail("remember", REMEMBER_ARGS))),
                    tool_call("remember"),
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

    /// No saved credentials: nothing is sent.
    #[tokio::test]
    async fn no_credentials() {
        let db = crate::db::test_database();
        let turn = run_turn(&AiRouter::new("gemini_primary"), row(), &[("user", "Hi")], &db, Workload::Tools, false).await;
        assert_eq!(turn.reply, Err("bedrock: AWS credentials not configured in Settings".into()));
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
}
