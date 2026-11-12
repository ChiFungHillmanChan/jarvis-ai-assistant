//! One plain request (no tools) to the AI the user chose, for features that
//! are not a chat turn: mail analysis, and triage next. It walks the same
//! provider chain as chat, so whichever single provider the user set up in
//! step 1 answers, and nothing else is tried behind their back.
//!
//! Error bodies can echo the request (mail text), so they are never
//! surfaced: only the provider and the HTTP status are.
use super::cost::Feature;
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
    /// What the request is for, stored with its usage row.
    pub feature: Feature,
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
    /// From models::usd_cost_micro; None for a local model, which is free,
    /// and for a cloud model without a price.
    pub cost_micro: Option<i64>,
    /// A cloud model the price list does not know: `cost_micro` is None but
    /// the request was not free.
    pub price_unknown: bool,
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
        let answered = complete_one(router, db, entry, &request).await;
        match answered.and_then(|done| store_then_check(db, entry, &request, done, started.elapsed().as_millis() as i64)) {
            Ok(done) => return Ok(done),
            Err(error) => errors.push(error),
        }
    }
    Err(errors.join("；"))
}

/// Store the completion, then check it. The row comes first because the
/// provider bills a reply whatever it says, as mail analysis does.
fn store_then_check(db: &Database, entry: &ProviderChainEntry, request: &Request<'_>, mut done: Completion, latency_ms: i64) -> Result<Completion, String> {
    let provider = super::cost::provider_id(&entry.provider_type, entry.base_url.as_deref());
    record(db, &provider, request.feature, &mut done, latency_ms);
    // Bedrock has no JSON mode: the object was asked for in words
    // (bedrock::JSON_INSTRUCTION), so the reply must be checked for one,
    // without code fences or prose around it.
    if entry.provider_type == "bedrock" && request.json {
        done.text = json_object_text(&done.text).ok_or_else(|| format!("Bedrock（{}）回覆的不是有效 JSON，請重試或改用其他模型。", done.model))?;
    }
    Ok(done)
}

/// Price the completion and store it as one Volume row.
fn record(db: &Database, provider: &str, feature: Feature, done: &mut Completion, latency_ms: i64) {
    done.cost_micro = super::models::usd_cost_micro(provider, &done.model, done.input_tokens, done.output_tokens, done.cached_tokens, 0);
    done.price_unknown = !super::models::price_known(provider, &done.model);
    super::cost::record_inference(db, provider, feature, Workload::Volume, &done.model, done.input_tokens, done.output_tokens, done.cached_tokens, 0, latency_ms);
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
            let body = claude_body(&model, request);
            let value = post(&client, "https://api.anthropic.com/v1/messages", "Claude", |r| r.header("x-api-key", &key).header("anthropic-version", "2023-06-01"), &body).await?;
            claude_result(&value, model)
        }
        "bedrock" => {
            let creds = crate::aws::credentials::load_from(db)?.ok_or("尚未設定 AWS 憑證，請在設定中加入後重試。")?;
            // The chain row's model, pointed at the geography the credentials
            // were saved for, as in chat.
            let model = super::apply_inference_profile(&super::bedrock_model(entry), &creds.inference_profile);
            let plain = super::bedrock::plain_request(request.system, request.user, request.json, request.max_tokens)?;
            let output = super::bedrock::converse_plain(&creds, &model, plain).await?;
            bedrock_result(&output, model)
        }
        other => Err(format!("{other} 暫不支援這項功能，請在設定中改用其他 AI。")),
    }
}

/// The one tool a JSON request to Claude is made to call. Claude has no
/// JSON switch on every model; a forced tool call always arrives as a JSON
/// object, so the shape is guaranteed by the API rather than asked for in
/// words. The schema is open because the instructions carry the real one.
const CLAUDE_JSON_TOOL: &str = "json_reply";

fn claude_body(model: &str, request: &Request<'_>) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": request.max_tokens,
        "messages": [{"role": "user", "content": request.user}],
    });
    if !request.system.is_empty() {
        body["system"] = json!(request.system);
    }
    if request.json {
        body["tools"] = json!([{
            "name": CLAUDE_JSON_TOOL,
            "description": "Give your whole answer as this tool's input: one JSON object in the shape the instructions describe.",
            "input_schema": {"type": "object"},
        }]);
        body["tool_choice"] = json!({"type": "tool", "name": CLAUDE_JSON_TOOL});
    }
    body
}

/// A JSON answer is the forced tool call's input; anything else is the text
/// blocks.
fn claude_result(value: &Value, model: String) -> Result<Completion, String> {
    let blocks = value.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
    let tool_input = blocks
        .iter()
        .find(|b| b.get("type").and_then(Value::as_str) == Some("tool_use") && b.get("name").and_then(Value::as_str) == Some(CLAUDE_JSON_TOOL))
        .and_then(|b| b.get("input"))
        .filter(|input| input.as_object().is_some_and(|fields| !fields.is_empty()));
    let text = match tool_input {
        Some(input) => input.to_string(),
        None => blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<String>(),
    };
    done(text, "Claude".into(), model, value.pointer("/usage/input_tokens"), value.pointer("/usage/output_tokens"))
}

/// The reply as Bedrock wrote it. A JSON request is checked after the row is
/// stored (`store_then_check`).
fn bedrock_result(output: &aws_sdk_bedrockruntime::operation::converse::ConverseOutput, model: String) -> Result<Completion, String> {
    let (text, input_tokens, output_tokens, cached_tokens) = super::bedrock::plain_result(output);
    let mut done = done(text, "Bedrock".into(), model, None, None)?;
    done.input_tokens = input_tokens;
    done.output_tokens = output_tokens;
    done.cached_tokens = cached_tokens;
    Ok(done)
}

/// The JSON object in a reply, without code fences or prose around it.
fn json_object_text(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let value: Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    value.is_object().then(|| value.to_string())
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
        price_unknown: false,
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
        let request = Request { system: "rules", user: "hi", json: true, max_tokens: 100, feature: Feature::Other };
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
        record(&db, "openai", Feature::MailAnalysis, &mut done, 1);
        assert_eq!((done.cost_micro, done.price_unknown), (Some(331), false));
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

    fn sort_request(json: bool) -> Request<'static> {
        Request { system: "Sort this email.", user: "From: a@example.com", json, max_tokens: 2_048, feature: Feature::MailSort }
    }

    /// Claude's JSON mode is one forced tool call, so the object arrives as
    /// structured input rather than text that may carry fences or prose.
    #[test]
    fn a_claude_json_request_forces_the_reply_tool() {
        let body = claude_body("claude-sonnet-4-6", &sort_request(true));
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["max_tokens"], 2_048);
        assert_eq!(body["system"], "Sort this email.");
        assert_eq!(body["messages"], json!([{"role": "user", "content": "From: a@example.com"}]));
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        assert_eq!(body["tools"][0]["name"], CLAUDE_JSON_TOOL);
        assert_eq!(body["tools"][0]["input_schema"], json!({"type": "object"}));
        assert_eq!(body["tool_choice"], json!({"type": "tool", "name": CLAUDE_JSON_TOOL}));

        let plain = claude_body("claude-sonnet-4-6", &Request { system: "", ..sort_request(false) });
        assert!(plain.get("tools").is_none() && plain.get("tool_choice").is_none());
        assert!(plain.get("system").is_none(), "no empty system prompt");
    }

    /// A Messages API reply to a forced tool call (stop_reason tool_use).
    fn claude_tool_reply() -> Value {
        json!({
            "id": "msg_fixture", "type": "message", "role": "assistant", "model": "claude-sonnet-4-6",
            "content": [{"type": "tool_use", "id": "toolu_fixture", "name": "json_reply", "input": {"category": "action", "summary": "Reply by Friday"}}],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 812, "output_tokens": 64}
        })
    }

    #[test]
    fn a_claude_json_reply_is_the_tool_input() {
        let done = claude_result(&claude_tool_reply(), "claude-sonnet-4-6".into()).unwrap();
        let parsed: Value = serde_json::from_str(&done.text).unwrap();
        assert_eq!(parsed, json!({"category": "action", "summary": "Reply by Friday"}));
        assert_eq!((done.provider.as_str(), done.input_tokens, done.output_tokens), ("Claude", 812, 64));
    }

    #[test]
    fn a_plain_claude_reply_is_its_text() {
        let value = json!({"content": [{"type": "text", "text": "Reply "}, {"type": "text", "text": "by Friday"}], "usage": {"input_tokens": 10, "output_tokens": 4}});
        assert_eq!(claude_result(&value, "m".into()).unwrap().text, "Reply by Friday");
        let empty_tool = json!({"content": [{"type": "tool_use", "name": "json_reply", "input": {}}], "usage": {}});
        assert!(claude_result(&empty_tool, "m".into()).is_err(), "an empty object is no answer");
    }

    #[test]
    fn a_bedrock_json_request_asks_for_one_object() {
        use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, SystemContentBlock};
        let request = super::super::bedrock::plain_request("Sort this email.", "From: a@example.com", true, 2_048).unwrap();
        assert_eq!(request.system.len(), 1);
        let SystemContentBlock::Text(system) = &request.system[0] else { panic!("a text system block") };
        assert_eq!(system, &format!("Sort this email.\n\n{}", super::super::bedrock::JSON_INSTRUCTION));
        assert_eq!(request.messages.len(), 1);
        assert_eq!(*request.messages[0].role(), ConversationRole::User);
        assert_eq!(request.messages[0].content(), &[ContentBlock::Text("From: a@example.com".into())]);
        assert_eq!(request.inference.max_tokens(), Some(2_048));

        let plain = super::super::bedrock::plain_request("", "hi", false, 10).unwrap();
        assert!(plain.system.is_empty(), "no empty system block");
        let json_only = super::super::bedrock::plain_request("", "hi", true, 10).unwrap();
        assert_eq!(json_only.system, vec![SystemContentBlock::Text(super::super::bedrock::JSON_INSTRUCTION.into())]);
    }

    /// A Converse reply, as the SDK hands it back.
    fn converse_reply(text: &str) -> aws_sdk_bedrockruntime::operation::converse::ConverseOutput {
        use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, ConverseMetrics, ConverseOutput as Output, Message, StopReason, TokenUsage};
        aws_sdk_bedrockruntime::operation::converse::ConverseOutput::builder()
            .output(Output::Message(Message::builder().role(ConversationRole::Assistant).content(ContentBlock::Text(text.into())).build().unwrap()))
            .stop_reason(StopReason::EndTurn)
            .usage(TokenUsage::builder().input_tokens(700).output_tokens(40).total_tokens(1_040).cache_read_input_tokens(300).build().unwrap())
            .metrics(ConverseMetrics::builder().latency_ms(5).build().unwrap())
            .build()
            .unwrap()
    }

    fn bedrock_entry() -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "bedrock".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None }
    }

    #[test]
    fn a_bedrock_reply_carries_its_text_and_usage() {
        let done = bedrock_result(&converse_reply("```json\n{\"category\": \"action\"}\n```"), "us.amazon.nova-pro-v1:0".into()).unwrap();
        assert_eq!(done.text, "```json\n{\"category\": \"action\"}\n```", "checked later, after the row is stored");
        // inputTokens leaves the cache read out; the row counts it in.
        assert_eq!((done.input_tokens, done.output_tokens, done.cached_tokens), (1_000, 40, 300));
        assert_eq!(done.provider, "Bedrock");
    }

    #[test]
    fn a_bedrock_json_reply_is_one_object_without_fences() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let done = bedrock_result(&converse_reply("```json\n{\"category\": \"action\"}\n```"), "us.amazon.nova-pro-v1:0".into()).unwrap();
        let done = store_then_check(&db, &bedrock_entry(), &sort_request(true), done, 1).unwrap();
        assert_eq!(done.text, r#"{"category":"action"}"#);
        assert_eq!(crate::ai::cost::tests::usage_rows(&db).len(), 1);
    }

    /// AWS bills a reply that is not JSON, so its row is stored before the
    /// check rejects it.
    #[test]
    fn a_bedrock_reply_that_is_not_json_is_stored_then_rejected() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let prose = bedrock_result(&converse_reply("The email asks for a reply."), "us.amazon.nova-pro-v1:0".into()).unwrap();
        let error = store_then_check(&db, &bedrock_entry(), &sort_request(true), prose, 1).unwrap_err();
        assert_eq!(error, "Bedrock（us.amazon.nova-pro-v1:0）回覆的不是有效 JSON，請重試或改用其他模型。");
        let rows = crate::ai::cost::tests::usage_rows(&db);
        assert_eq!(rows.len(), 1, "the billed reply is stored");
        assert_eq!((rows[0].provider.as_str(), rows[0].feature.as_str(), rows[0].input_tokens, rows[0].output_tokens), ("bedrock", "mail_sort", 1_000, 40));

        let array = bedrock_result(&converse_reply("[1, 2]"), "m".into()).unwrap();
        assert!(store_then_check(&db, &bedrock_entry(), &sort_request(true), array, 1).is_err(), "an array is not the object asked for");
        let plain = bedrock_result(&converse_reply("The email asks for a reply."), "m".into()).unwrap();
        assert_eq!(store_then_check(&db, &bedrock_entry(), &sort_request(false), plain, 1).unwrap().text, "The email asks for a reply.");
    }

    /// No AWS credentials saved: Bedrock asks for them instead of saying it is
    /// not supported. (No credentials row, so the Keychain is never read.)
    #[tokio::test]
    async fn bedrock_without_credentials_asks_for_them() {
        let db = crate::db::test_database();
        let mut router = AiRouter::new("gemini_primary");
        router.set_provider_chain(vec![ProviderChainEntry { position: 0, provider_type: "bedrock".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None }]);
        let error = complete(&router, &db, sort_request(true)).await.unwrap_err();
        assert!(error.contains("AWS 憑證"), "{error}");
        assert!(!error.contains("暫不支援"), "{error}");
    }

    /// One loopback HTTP reply, for a request that would go to a service.
    async fn one_reply(body: Value) -> (u16, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = stream.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end].lines().find_map(|line| line.to_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    if request.len() >= end + 4 + length || count == 0 {
                        break;
                    }
                }
            }
            let body = body.to_string();
            let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).to_string()
        });
        (port, task)
    }

    /// The row says what the completion was for and names the service by its
    /// provider id; a model without a price is stored with the cost unknown.
    #[tokio::test]
    async fn a_completion_is_stored_with_its_feature_and_provider_id() {
        let (port, server) = one_reply(json!({"choices": [{"message": {"content": "{\"category\": \"action\"}"}}], "usage": {"prompt_tokens": 300, "completion_tokens": 20}})).await;
        let base = format!("http://127.0.0.1:{port}/v1");
        let mut router = AiRouter::new("gemini_primary");
        router.set_compat_key(&base, "sk-fixture-not-a-real-key");
        router.set_provider_chain(vec![ProviderChainEntry { position: 0, provider_type: "openai".into(), endpoint_id: None, model_id: Some("unlisted-chat-1".into()), enabled: true, base_url: Some(base) }]);
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let done = complete(&router, &db, sort_request(true)).await.unwrap();
        assert_eq!((done.cost_micro, done.price_unknown), (None, true));
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /v1/chat/completions"), "{request}");
        assert_eq!(
            crate::ai::cost::tests::usage_rows(&db),
            vec![crate::ai::cost::tests::UsageRow {
                provider: "compat:127.0.0.1".into(),
                feature: "mail_sort".into(),
                model_id: "unlisted-chat-1".into(),
                input_tokens: 300,
                output_tokens: 20,
                usd_cost_micro: None,
                price_unknown: true,
            }]
        );
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
            let done = complete(&router, &db, Request { system: "Answer in one word.", user: "Reply with the single word OK.", json: false, max_tokens: 1_024, feature: Feature::Other }).await;
            println!("{name}: {done:?}");
            let done = done.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(done.text.to_uppercase().contains("OK"), "{name}: {}", done.text);
            let json = complete(&router, &db, Request { system: "", user: "Return a JSON object with one field \"ok\" set to true.", json: true, max_tokens: 1_024, feature: Feature::Other }).await.unwrap();
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
        let error = complete(&router, &db, Request { system: "", user: "hi", json: false, max_tokens: 10, feature: Feature::Other }).await.unwrap_err();
        assert!(error.contains("尚未選擇 AI"), "{error}");
    }
}
