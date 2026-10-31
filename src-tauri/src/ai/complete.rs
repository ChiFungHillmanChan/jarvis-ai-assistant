//! One plain request (no tools) to the AI the user chose, for features that
//! are not a chat turn: mail analysis, and triage next. It walks the same
//! provider chain as chat, so whichever single provider the user set up in
//! step 1 answers, and nothing else is tried behind their back.
//!
//! Error bodies can echo the request (mail text), so they are never
//! surfaced: only the provider and the HTTP status are.
use super::{compat, gemini, AiRouter, ProviderChainEntry, Workload};
use crate::db::Database;
use serde_json::{json, Value};
use std::time::Instant;

/// Context window asked of Ollama: a triaged email (6,000 characters), its
/// instructions and the sender's known follow-ups fit with room to answer.
const OLLAMA_CONTEXT: u32 = 8_192;

pub struct Request<'a> {
    pub system: &'a str,
    pub user: &'a str,
    /// Ask for a JSON object where the provider supports it; the prompt
    /// must still say so, because not every provider has a JSON mode.
    pub json: bool,
    pub max_tokens: u32,
}

#[derive(Debug)]
pub struct Completion {
    pub text: String,
    /// Shown to the user, e.g. "Kimi (kimi-k2)".
    pub provider: String,
    pub model: String,
    /// Includes `cached_tokens`.
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Read from the provider's cache, billed at its cached rate.
    pub cached_tokens: i64,
    /// From models::usd_cost_micro; None for a local model, which is free.
    pub cost_micro: Option<i64>,
}

/// The chosen AI, tried in chain order. An empty chain means the user has not
/// chosen one yet.
pub async fn complete(router: &AiRouter, db: &Database, request: Request<'_>) -> Result<Completion, String> {
    let entries: Vec<&ProviderChainEntry> = router.provider_chain.iter().filter(|e| e.enabled).collect();
    if entries.is_empty() {
        return Err(super::NO_AI_CHOSEN.into());
    }
    let mut errors = Vec::new();
    for entry in entries {
        let started = Instant::now();
        match complete_one(router, db, entry, &request).await {
            Ok(mut done) => {
                let provider = if entry.provider_type == "local" { "local".to_string() } else { done.provider.to_lowercase() };
                record(db, &provider, &mut done, started.elapsed().as_millis() as i64);
                return Ok(done);
            }
            Err(error) => errors.push(error),
        }
    }
    Err(errors.join("；"))
}

/// Price the completion and store it as one Volume row.
fn record(db: &Database, provider: &str, done: &mut Completion, latency_ms: i64) {
    done.cost_micro = super::models::usd_cost_micro(provider, &done.model, done.input_tokens, done.output_tokens, done.cached_tokens, 0);
    super::cost::record_inference(db, provider, Workload::Volume, &done.model, done.input_tokens, done.output_tokens, done.cached_tokens, 0, latency_ms);
}

async fn complete_one(router: &AiRouter, db: &Database, entry: &ProviderChainEntry, request: &Request<'_>) -> Result<Completion, String> {
    let client = compat::client()?;
    match entry.provider_type.as_str() {
        "openai" => {
            let (url, key, model, name) = match entry.base_url.as_deref() {
                Some(base) => {
                    let key = router.compat_key(base).await.ok_or_else(|| format!("尚未設定 {} API 密鑰。", compat::label(base)))?;
                    let model = entry.model_id.clone().filter(|m| !m.trim().is_empty()).ok_or_else(|| format!("尚未為 {} 選擇模型。", compat::label(base)))?;
                    (format!("{base}/chat/completions"), key, model, compat::label(base))
                }
                None => {
                    let key = router.openai_key.clone().ok_or("尚未設定 OpenAI API 密鑰。")?;
                    ("https://api.openai.com/v1/chat/completions".to_string(), key, entry.model_id.clone().unwrap_or_else(|| crate::ai::models::OPENAI_DEFAULT.to_string()), "OpenAI".to_string())
                }
            };
            let body = chat_completions_body(&model, request, entry.base_url.is_none());
            let value = post(&client, &url, &name, |r| r.bearer_auth(&key), &body).await?;
            chat_completions_result(&value, name, model)
        }
        "local" => {
            let endpoint_id = entry.endpoint_id.as_deref().unwrap_or_default();
            let endpoint = router.find_endpoint(endpoint_id).ok_or("本機 AI 端點不存在或已停用。")?;
            let model = entry.model_id.clone().ok_or("尚未選擇本機模型。")?;
            let base = endpoint.url.trim_end_matches('/');
            if matches!(endpoint.backend_type, super::local::backend::BackendType::Ollama) {
                // Ollama's OpenAI endpoint keeps its small default context and
                // drops the start of a long prompt (the instructions) silently.
                let mut messages = Vec::new();
                if !request.system.is_empty() {
                    messages.push(json!({"role": "system", "content": request.system}));
                }
                messages.push(json!({"role": "user", "content": request.user}));
                let mut body = json!({"model": model, "messages": messages, "stream": false, "options": {"num_ctx": OLLAMA_CONTEXT, "num_predict": request.max_tokens}});
                if request.json {
                    body["format"] = json!("json");
                }
                let value = post(&client, &format!("{base}/api/chat"), "Ollama", |r| r, &body).await?;
                let text = value.pointer("/message/content").and_then(Value::as_str).unwrap_or_default().to_string();
                return done(text, "Ollama".into(), model, value.get("prompt_eval_count"), value.get("eval_count"));
            }
            let body = chat_completions_body(&model, request, false);
            let key = endpoint.api_key.clone();
            let value = post(&client, &format!("{base}/v1/chat/completions"), "本機 AI", |r| match &key {
                Some(key) if !key.is_empty() => r.bearer_auth(key),
                _ => r,
            }, &body).await?;
            chat_completions_result(&value, "本機 AI".into(), model)
        }
        "gemini" => {
            let key = router.gemini_key.clone().ok_or("尚未設定 Gemini API 密鑰。")?;
            let model = super::gemini_model_for(entry.model_id.as_deref(), Workload::Volume).to_string();
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
            let url = format!("{}/models/{model}:generateContent", gemini::API_BASE);
            let value = post(&client, &url, "Gemini", |r| r.header("x-goog-api-key", &key), &body).await?;
            gemini_result(&value, model)
        }
        "claude" => {
            let key = router.claude_key.clone().ok_or("尚未設定 Claude API 密鑰。")?;
            let model = entry.model_id.clone().unwrap_or_else(|| crate::ai::models::CLAUDE_DEFAULT.to_string());
            let mut body = json!({
                "model": model,
                "max_tokens": request.max_tokens,
                "messages": [{"role": "user", "content": request.user}],
            });
            if !request.system.is_empty() {
                body["system"] = json!(request.system);
            }
            let value = post(&client, "https://api.anthropic.com/v1/messages", "Claude", |r| r.header("x-api-key", &key).header("anthropic-version", "2023-06-01"), &body).await?;
            let text = value
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<String>())
                .unwrap_or_default();
            done(text, "Claude".into(), model, value.pointer("/usage/input_tokens"), value.pointer("/usage/output_tokens"))
        }
        other => {
            let _ = db;
            Err(format!("{other} 暫不支援這項功能，請在設定中改用其他 AI。"))
        }
    }
}

/// OpenAI Chat Completions body. api.openai.com takes
/// `max_completion_tokens`; compatible services and Ollama take `max_tokens`.
fn chat_completions_body(model: &str, request: &Request<'_>, official: bool) -> Value {
    let mut messages = Vec::new();
    if !request.system.is_empty() {
        messages.push(json!({"role": "system", "content": request.system}));
    }
    messages.push(json!({"role": "user", "content": request.user}));
    let mut body = json!({"model": model, "messages": messages});
    body[if official { "max_completion_tokens" } else { "max_tokens" }] = json!(request.max_tokens);
    if request.json {
        body["response_format"] = json!({"type": "json_object"});
    }
    body
}

fn chat_completions_result(value: &Value, provider: String, model: String) -> Result<Completion, String> {
    let text = value.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut done = done(text, provider, model, value.pointer("/usage/prompt_tokens"), value.pointer("/usage/completion_tokens"))?;
    done.cached_tokens = value.pointer("/usage/prompt_tokens_details/cached_tokens").and_then(Value::as_i64).unwrap_or(0);
    Ok(done)
}

fn gemini_result(value: &Value, model: String) -> Result<Completion, String> {
    let text = value
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|parts| parts.iter().filter(|p| p.get("thought").and_then(Value::as_bool) != Some(true)).filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<String>())
        .unwrap_or_default();
    let usage = |field: &str| value.pointer(&format!("/usageMetadata/{field}")).and_then(Value::as_i64).unwrap_or(0);
    let mut done = done(text, "Gemini".into(), model, value.pointer("/usageMetadata/promptTokenCount"), value.pointer("/usageMetadata/candidatesTokenCount"))?;
    // Thinking is billed as output; candidatesTokenCount leaves it out.
    done.output_tokens += usage("thoughtsTokenCount");
    done.cached_tokens = usage("cachedContentTokenCount");
    Ok(done)
}

fn done(text: String, provider: String, model: String, input: Option<&Value>, output: Option<&Value>) -> Result<Completion, String> {
    if text.trim().is_empty() {
        return Err(format!("{provider} 沒有回覆內容。"));
    }
    Ok(Completion {
        text,
        provider,
        model,
        input_tokens: input.and_then(Value::as_i64).unwrap_or(0),
        output_tokens: output.and_then(Value::as_i64).unwrap_or(0),
        cached_tokens: 0,
        cost_micro: None,
    })
}

async fn post(
    client: &reqwest::Client,
    url: &str,
    name: &str,
    auth: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    body: &Value,
) -> Result<Value, String> {
    let response = auth(client.post(url)).json(body).send().await.map_err(|_| format!("未能連接 {name}，請檢查網絡。"))?;
    let status = response.status().as_u16();
    if status == 200 {
        return response.json().await.map_err(|_| format!("{name} 回覆格式無效。"));
    }
    let body = response.text().await.unwrap_or_default();
    Err(rejection(name, status, &body))
}

/// What went wrong, in the user's terms. The body is inspected, never
/// shown: it can echo the request. Gemini reports a bad key as 400.
fn rejection(name: &str, status: u16, body: &str) -> String {
    let lower = body.to_lowercase();
    let bad_key = ["api_key_invalid", "api key not valid", "invalid_api_key", "incorrect api key", "invalid x-api-key", "invalid api key", "authentication"]
        .iter()
        .any(|m| lower.contains(m));
    match status {
        401 | 403 => format!("{name} 拒絕了 API 密鑰（{status}），請檢查後重試。"),
        400 if bad_key => format!("{name} 拒絕了 API 密鑰，請檢查是否完整複製。"),
        400 => format!("{name} 不接受這個請求（400），請檢查所選模型。"),
        404 => format!("{name} 找不到所選模型或網址（404）。"),
        429 => format!("{name} 暫時限速或額度不足（429），請稍後重試。"),
        _ => format!("{name} 暫時未能回覆（{status}），請稍後重試。"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn chat_completions_bodies_follow_each_service() {
        let request = Request { system: "rules", user: "hi", json: true, max_tokens: 100 };
        let official = chat_completions_body("gpt-6-luna", &request, true);
        assert_eq!(official["max_completion_tokens"], 100);
        assert!(official.get("max_tokens").is_none());
        assert_eq!(official["messages"][0]["role"], "system");
        assert_eq!(official["response_format"]["type"], "json_object");
        let compatible = chat_completions_body("kimi-k2", &Request { system: "", json: false, ..request }, false);
        assert_eq!(compatible["max_tokens"], 100);
        assert!(compatible.get("max_completion_tokens").is_none());
        assert!(compatible.get("response_format").is_none());
        assert_eq!(compatible["messages"].as_array().unwrap().len(), 1, "no empty system message");
    }

    #[test]
    fn a_bad_key_is_named_even_when_the_status_is_400() {
        let gemini = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"reason":"API_KEY_INVALID"}]}}"#;
        assert!(rejection("Gemini", 400, gemini).contains("拒絕了 API 密鑰"));
        assert!(rejection("Kimi", 401, "").contains("拒絕了 API 密鑰"));
        let other = rejection("Qwen", 400, r#"{"error":{"message":"model not supported"}}"#);
        assert!(other.contains("模型") && !other.contains("model not supported"), "never echo the body: {other}");
    }

    #[test]
    fn an_empty_answer_is_an_error_not_a_result() {
        assert!(chat_completions_result(&json!({"choices": [{"message": {"content": "  "}}]}), "Kimi".into(), "m".into()).is_err());
        let ok = chat_completions_result(&json!({"choices": [{"message": {"content": "OK"}}], "usage": {"prompt_tokens": 5, "completion_tokens": 1}}), "Kimi".into(), "m".into()).unwrap();
        assert_eq!((ok.text.as_str(), ok.input_tokens, ok.output_tokens), ("OK", 5, 1));
    }

    fn luna_cache_hit() -> Value {
        json!({"choices": [{"message": {"content": "OK"}}], "usage": {"prompt_tokens": 20173, "completion_tokens": 195, "prompt_tokens_details": {"cached_tokens": 19824}}})
    }

    /// prompt_tokens includes the cached prefix, which OpenAI bills at a
    /// tenth of the price; dropping it overstated mail analysis cost.
    #[test]
    fn cached_prompt_tokens_are_read_from_a_completion() {
        let done = chat_completions_result(&luna_cache_hit(), "OpenAI".into(), "gpt-6-luna".into()).unwrap();
        assert_eq!((done.input_tokens, done.output_tokens, done.cached_tokens), (20_173, 195, 19_824));
    }

    /// gpt-6-luna: 349 x $0.10 + 19,824 x $0.01 + 195 x $0.50 per 1M = 331
    /// micro-USD, stored with the cached count.
    #[test]
    fn a_completion_is_priced_and_stored_with_its_cached_tokens() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let mut done = chat_completions_result(&luna_cache_hit(), "OpenAI".into(), "gpt-6-luna".into()).unwrap();
        record(&db, "openai", &mut done, 1);
        assert_eq!(done.cost_micro, Some(331));
        let stored: (i64, i64) = db.conn.lock().unwrap().query_row("SELECT cache_read_tokens, usd_cost_micro FROM inference_metrics", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(stored, (19_824, 331));
    }

    /// Thinking is billed as output, but candidatesTokenCount leaves it out.
    #[test]
    fn gemini_thinking_counts_as_output() {
        let value = json!({
            "candidates": [{"content": {"parts": [{"text": "planning", "thought": true}, {"text": "OK"}]}}],
            "usageMetadata": {"promptTokenCount": 1000, "candidatesTokenCount": 200, "thoughtsTokenCount": 800, "cachedContentTokenCount": 300}
        });
        let done = gemini_result(&value, crate::ai::models::GEMINI_FLASH.into()).unwrap();
        assert_eq!(done.text, "OK");
        assert_eq!((done.input_tokens, done.output_tokens, done.cached_tokens), (1_000, 1_000, 300));
    }

    /// Gemini's OpenAI-compatible endpoint stands in for Kimi and Qwen: a
    /// real third-party service speaking the same format.
    pub(crate) const GEMINI_COMPAT: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

    /// One router per provider, each with only that provider, as setup saves
    /// it. Run with the keys set:
    /// cargo test live_complete_on_each_provider -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_complete_on_each_provider() {
        let db = crate::db::test_database();
        let key = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name}"));
        let entry = |provider_type: &str, model: Option<&str>, base_url: Option<&str>| ProviderChainEntry {
            position: 0, provider_type: provider_type.into(), endpoint_id: None, model_id: model.map(Into::into), enabled: true, base_url: base_url.map(Into::into),
        };
        let mut cases: Vec<(&str, AiRouter)> = Vec::new();
        let mut openai = AiRouter::new("gemini_primary");
        openai.set_openai_key(&key("OPENAI_API_KEY"));
        openai.set_provider_chain(vec![entry("openai", None, None)]);
        cases.push(("openai", openai));
        let mut gemini = AiRouter::new("gemini_primary");
        gemini.set_gemini_key(&key("GEMINI_API_KEY"));
        gemini.set_provider_chain(vec![entry("gemini", None, None)]);
        cases.push(("gemini", gemini));
        // Optional: the development .env key may be revoked.
        if let Ok(anthropic) = std::env::var("ANTHROPIC_API_KEY") {
            let mut claude = AiRouter::new("gemini_primary");
            claude.set_claude_key(&anthropic);
            claude.set_provider_chain(vec![entry("claude", None, None)]);
            cases.push(("claude", claude));
        }
        let mut compatible = AiRouter::new("gemini_primary");
        compatible.set_compat_key(GEMINI_COMPAT, &key("GEMINI_API_KEY"));
        compatible.set_provider_chain(vec![entry("openai", Some(crate::ai::models::GEMINI_FLASH), Some(GEMINI_COMPAT))]);
        cases.push(("compatible", compatible));
        for (name, router) in cases {
            let done = complete(&router, &db, Request { system: "Answer in one word.", user: "Reply with the single word OK.", json: false, max_tokens: 1_024 }).await;
            println!("{name}: {done:?}");
            let done = done.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(done.text.to_uppercase().contains("OK"), "{name}: {}", done.text);
            let json = complete(&router, &db, Request { system: "", user: "Return a JSON object with one field \"ok\" set to true.", json: true, max_tokens: 1_024 }).await.unwrap();
            let parsed: Value = serde_json::from_str(json.text.trim().trim_start_matches("```json").trim_end_matches("```").trim()).unwrap_or_else(|_| panic!("{name} JSON: {}", json.text));
            assert_eq!(parsed["ok"], true, "{name}");
        }
        let models = compat::list_models(GEMINI_COMPAT, &key("GEMINI_API_KEY")).await.unwrap();
        println!("compatible /models: {} ids, e.g. {:?}", models.len(), models.iter().take(3).collect::<Vec<_>>());
        assert!(models.iter().any(|m| m == crate::ai::models::GEMINI_FLASH));
    }

    #[tokio::test]
    async fn no_chosen_ai_says_so() {
        let db = crate::db::test_database();
        let mut router = AiRouter::new("gemini_primary");
        router.set_provider_chain(Vec::new());
        let error = complete(&router, &db, Request { system: "", user: "hi", json: false, max_tokens: 10 }).await.unwrap_err();
        assert!(error.contains("尚未選擇 AI"), "{error}");
    }
}
