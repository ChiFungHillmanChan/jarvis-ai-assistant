//! The mail requests that set their own output cap send the registry's
//! (`feature_limits.<feature>.max_output_tokens`), not a number of their own.
//! Each request is caught on a loopback server and the cap read from its
//! body. The registry is the test fixture with the two caps set to values
//! that differ from each other and from `OUTPUT_TOKENS_WHEN_UNSET`, so each
//! request also shows it reads its own feature's cap.

use super::models::{MailContext, MailGroup, MailMessage, MailSettings};
use crate::ai::registry::{doc::FeatureLimits, test_override};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ANALYSIS_CAP: u32 = 1_234;
const SORT_CAP: u32 = 4_321;

fn registry_with_caps() -> test_override::Guard {
    assert!(ANALYSIS_CAP != crate::ai::models::OUTPUT_TOKENS_WHEN_UNSET && SORT_CAP != crate::ai::models::OUTPUT_TOKENS_WHEN_UNSET);
    test_override::with(|r| {
        r.feature_limits.insert("mail_analysis".into(), FeatureLimits { max_output_tokens: Some(ANALYSIS_CAP) });
        r.feature_limits.insert("mail_sort".into(), FeatureLimits { max_output_tokens: Some(SORT_CAP) });
    })
}

type Seen = Arc<Mutex<Vec<(String, Value)>>>;

/// A loopback server that answers every request with `reply` and keeps each
/// request's path and JSON body.
async fn capture(reply: Value) -> (u16, Seen) {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Seen = Arc::default();
    let kept = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            let body_start = loop {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break None;
                }
                request.extend_from_slice(&buf[..n]);
                if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(at + 4);
                }
            };
            let Some(body_start) = body_start else { continue };
            let head = String::from_utf8_lossy(&request[..body_start]).to_string();
            let length = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
            while request.len() < body_start + length {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
            let body = serde_json::from_slice(&request[body_start..]).unwrap_or(Value::Null);
            kept.lock().unwrap().push((path, body));
            let reply = reply.to_string();
            let response = format!("HTTP/1.1 200 Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    (port, seen)
}

/// The AI chosen in setup: a local Ollama endpoint on the loopback server.
fn chosen_local_ai(port: u16) -> crate::ai::AiRouter {
    use crate::ai::local::backend::{BackendType, LocalEndpoint};
    let mut router = crate::ai::AiRouter::new("gemini_primary");
    router.set_local_endpoints(vec![LocalEndpoint {
        id: "loopback".into(),
        name: "Loopback".into(),
        url: format!("http://127.0.0.1:{port}"),
        backend_type: BackendType::Ollama,
        api_key: None,
        use_tls: false,
        connection_timeout_ms: 5_000,
        keep_alive_minutes: 5,
        is_active: true,
        last_health_check: None,
        last_health_status: None,
    }]);
    router.set_provider_chain(vec![crate::ai::ProviderChainEntry { position: 0, provider_type: "local".into(), endpoint_id: Some("loopback".into()), model_id: Some("fixture-model".into()), enabled: true, base_url: None }]);
    router
}

fn ollama_reply(content: &str) -> Value {
    json!({"model": "fixture-model", "message": {"role": "assistant", "content": content}, "done": true, "prompt_eval_count": 10, "eval_count": 5})
}

fn analysis_context(mode: &str) -> MailContext {
    MailContext {
        group: MailGroup { id: 7, name: "Clients".into(), instructions: "List deadlines first.".into(), message_count: 1 },
        messages: vec![MailMessage {
            id: 101,
            account_id: "account-a".into(),
            account_email: "owner@example.com".into(),
            provider_id: "provider-101".into(),
            thread_id: None,
            subject: "Quote".into(),
            sender: "sender@example.com".into(),
            snippet: "Quote".into(),
            body: Some("A made-up test email: please send the quote by Friday.".into()),
            is_read: false,
            is_inbox: true,
            received_at: "2026-09-10T09:00:00Z".into(),
            attachments: None,
            triage: None,
            next_due: None,
        }],
        memories: vec![],
        settings: MailSettings { processing_mode: mode.into(), local_model: "fixture-model".into(), bedrock_model: "fixture-model".into(), openai_model: "fixture-model".into() },
    }
}

/// mail/openai.rs `send`: mail analysis on api.openai.com (its only caller).
#[tokio::test]
async fn mail_analysis_on_openai_asks_for_the_registry_cap() {
    let _registry = registry_with_caps();
    let reply = json!({
        "status": "completed", "error": null, "incomplete_details": null,
        "output": [{"type": "message", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": "Reply by Friday."}]}],
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let (port, seen) = capture(reply).await;
    let sent = tokio::time::timeout(Duration::from_secs(10), super::openai::send("sk-test-not-a-real-key", "fixture-model", "A made-up prompt.", super::openai::Endpoint::Loopback(port))).await.expect("the loopback reply arrives");
    assert!(sent.is_ok(), "{sent:?}");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "/v1/responses");
    assert_eq!(seen[0].1["max_output_tokens"], json!(ANALYSIS_CAP));
}

/// mail/analysis.rs: mail analysis with the AI chosen in setup ("assistant").
#[tokio::test]
async fn mail_analysis_with_the_chosen_ai_asks_for_the_registry_cap() {
    let _registry = registry_with_caps();
    let (port, seen) = capture(ollama_reply("Send the quote by Friday. [郵件 #101]")).await;
    let db = Arc::new(crate::db::test_database());
    let route = super::analysis::AssistantRoute { router: chosen_local_ai(port), db: db.clone() };
    let analysis = tokio::time::timeout(Duration::from_secs(10), super::analysis::analyze(analysis_context("assistant"), None, None, Some(route), Some(&db))).await.expect("the loopback reply arrives");
    assert!(analysis.is_ok(), "{:?}", analysis.err());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "/api/chat");
    assert_eq!(seen[0].1["options"]["num_predict"], json!(ANALYSIS_CAP));
}

/// mail/triage.rs: AI sorting with the AI chosen in setup (no TypeSafe or
/// OpenAI key), through the public run.
#[tokio::test]
async fn ai_sorting_with_the_chosen_ai_asks_for_the_registry_cap() {
    let _registry = registry_with_caps();
    let (port, seen) = capture(ollama_reply("{}")).await;
    let db = crate::db::test_database();
    super::store::upsert_account(&db, "account-a", "owner@example.com", true).unwrap();
    db.conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO mail_messages(account_id,provider_id,thread_id,subject,sender,snippet,body,is_read,is_inbox,received_at,attachments) VALUES ('account-a','provider-7',NULL,'Quarterly figures','Ann <ann@example.com>','','Please see the figures.',0,1,'2026-09-20T10:00:00Z','[]')",
            [],
        )
        .unwrap();
    let keys = super::triage::TriageKeys { typesafe: None, openai: None, openai_model: String::new(), assistant: Some(chosen_local_ai(port)) };
    let run = tokio::time::timeout(Duration::from_secs(10), super::triage::run(&db, keys, "en")).await.expect("the loopback reply arrives");
    assert!(run.is_ok(), "{:?}", run.err());
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty(), "the chosen AI was asked");
    for (path, body) in seen.iter() {
        assert_eq!(path, "/api/chat");
        assert_eq!(body["options"]["num_predict"], json!(SORT_CAP));
    }
}
