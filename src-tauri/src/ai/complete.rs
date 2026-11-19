//! One plain request (no tools) to the AI the user chose, for features that
//! are not a chat turn: mail analysis and mail sorting. It runs on the same
//! chosen model as chat (`ai::run_chosen`): checked for the feature before
//! anything is sent, and nothing else is tried behind the user's back.
//!
//! Error bodies can echo the request (mail text), so they are never
//! surfaced: only the provider and the HTTP status are, and for a refused
//! request the reply's `error.message` on the card, as chat shows it.
use super::cost::Feature;
use super::openai::{OpenAiWire, Target};
use super::provider::Wire;
use super::{compat, AiRouter, ProviderChainEntry, Workload};
use crate::db::Database;
use serde_json::Value;
use std::time::Instant;
// Each wire's own plain request format, which the tests below check.
#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use super::{bedrock::bedrock_result, claude::{claude_body, claude_result}, gemini::gemini_result, openai::{chat_completions_body, chat_completions_result}};

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
    /// Written to the provider's cache; only Bedrock reports it. Included in
    /// `input_tokens`.
    pub cache_write_tokens: i64,
    /// The reply carried no usage figures (only Bedrock can tell), so no row
    /// is stored for it rather than one of zeros.
    pub usage_missing: bool,
    /// From models::usd_cost_micro; None for a local model, which is free,
    /// and for a cloud model without a price.
    pub cost_micro: Option<i64>,
    /// A cloud model the price list does not know: `cost_micro` is None but
    /// the request was not free.
    pub price_unknown: bool,
}

/// The request on the chosen model, or on the model picked in the error
/// card. A request the user waits on (mail analysis) fails with the card;
/// a background job (mail sorting) records why and raises one notice.
pub async fn complete(router: &AiRouter, db: &Database, request: Request<'_>) -> Result<Completion, String> {
    let configured = router.configured(db).await;
    let chosen = router.chosen(&configured);
    let picked = super::capability::picked();
    // Gmail and Calendar content goes only where mail_policy allows, judged
    // on the row this request goes to (a model picked in the error card,
    // else the chosen one), as mail_policy::may_send_on judges it. Refused
    // before any request; the caller's own gate shows the card or notice.
    if super::mail_policy::carries_google_content(request.feature) {
        if let Some(receiver) = picked.as_ref().map(super::capability::Pick::entry).or_else(|| chosen.clone()) {
            super::mail_policy::allowed(db, &receiver).map_err(|blocked| blocked.user_message())?;
        }
    }
    let ledger = super::SideEffectLedger::new();
    let request = &request;
    super::run_chosen(
        chosen,
        picked,
        &configured,
        request.feature,
        Workload::Volume,
        &ledger,
        super::bedrock::UiLanguage::read(db),
        // Every wire sends a completion once (Wire::complete).
        &|_| false,
        &|row| super::mail_policy::may_receive(db, row),
        |_, _| {},
        move |row| async move {
            let started = Instant::now();
            let done = complete_one(router, db, &row, request).await?;
            store_then_check(db, &row, request, done, started.elapsed().as_millis() as i64)
        },
    )
    .await
}

/// Store the completion, then check it. The row comes first because the
/// provider bills a reply whatever it says, as mail analysis does: an empty
/// Bedrock reply, or one that is not the JSON asked for, is stored and then
/// rejected.
fn store_then_check(db: &Database, entry: &ProviderChainEntry, request: &Request<'_>, mut done: Completion, latency_ms: i64) -> Result<Completion, String> {
    let provider = super::cost::provider_id(&entry.provider_type, entry.base_url.as_deref());
    if done.usage_missing {
        // Nothing to store: AWS sent no figures, and none are made up. What
        // the request cost is not known.
        log::warn!("{provider} reply carried no usage figures; no usage row stored");
        done.price_unknown = true;
    } else {
        record(db, &provider, request.feature, &mut done, latency_ms);
    }
    if entry.provider_type == "bedrock" {
        let language = super::bedrock::UiLanguage::read(db);
        if done.text.trim().is_empty() {
            return Err(language.pick(
                format!("Bedrock ({}) returned no answer. Try again, or choose another model.", done.model),
                format!("Bedrock（{}）沒有回覆內容，請重試或改用其他模型。", done.model),
            ));
        }
    }
    // Bedrock and Claude have no JSON mode this build uses: the object was
    // asked for in words (bedrock::JSON_INSTRUCTION), so the reply must be
    // checked for one, without code fences or prose around it. A reply that
    // is not one fails once; it is not a transient failure, so it is not
    // sent again.
    if request.json && matches!(entry.provider_type.as_str(), "bedrock" | "claude") {
        let language = super::bedrock::UiLanguage::read(db);
        let name = if entry.provider_type == "bedrock" { "Bedrock" } else { "Claude" };
        done.text = json_object_text(&done.text).ok_or_else(|| {
            language.pick(
                format!("{name} ({}) did not reply with valid JSON. Try again, or choose another model.", done.model),
                format!("{name}（{}）回覆的不是有效 JSON，請重試或改用其他模型。", done.model),
            )
        })?;
    }
    Ok(done)
}

/// Price the completion and store it as one Volume row.
fn record(db: &Database, provider: &str, feature: Feature, done: &mut Completion, latency_ms: i64) {
    done.cost_micro = super::models::usd_cost_micro(provider, &done.model, done.input_tokens, done.output_tokens, done.cached_tokens, done.cache_write_tokens);
    done.price_unknown = !super::models::price_known(provider, &done.model);
    super::cost::record_inference(db, provider, feature, Workload::Volume, &done.model, done.input_tokens, done.output_tokens, done.cached_tokens, done.cache_write_tokens, latency_ms);
}

/// The request on one chain row: its key and model as the row says, then
/// the wire's own plain request.
async fn complete_one(router: &AiRouter, db: &Database, entry: &ProviderChainEntry, request: &Request<'_>) -> Result<Completion, String> {
    let language = super::bedrock::UiLanguage::read(db);
    match entry.provider_type.as_str() {
        "openai" => {
            let wire = match entry.base_url.as_deref() {
                Some(base) => {
                    let key = router.compat_key(base).await.ok_or_else(|| format!("尚未設定 {} API 密鑰。", compat::label(base)))?;
                    let model = entry.model_id.clone().filter(|m| !m.trim().is_empty()).ok_or_else(|| format!("尚未為 {} 選擇模型。", compat::label(base)))?;
                    OpenAiWire::new(&key, Target { base_url: Some(base), model: Some(&model), deeper: None })?
                }
                None => {
                    let key = router.openai_key.clone().ok_or("尚未設定 OpenAI API 密鑰。")?;
                    OpenAiWire::new(&key, Target { base_url: None, model: entry.model_id.as_deref(), deeper: None })?
                }
            };
            wire.complete(request, language).await
        }
        "local" => {
            let endpoint_id = entry.endpoint_id.as_deref().unwrap_or_default();
            let endpoint = router.find_endpoint(endpoint_id).ok_or("本機 AI 端點不存在或已停用。")?;
            let model = entry.model_id.clone().ok_or("尚未選擇本機模型。")?;
            OpenAiWire::local(super::local::LocalChat::plain(endpoint.clone()), &model).complete(request, language).await
        }
        "gemini" => {
            let key = router.gemini_key.clone().ok_or("尚未設定 Gemini API 密鑰。")?;
            let model = super::gemini_model_for(entry.model_id.as_deref(), Workload::Volume);
            super::gemini::GeminiWire::new(&key, &model, None).complete(request, language).await
        }
        "claude" => {
            let key = router.claude_key.clone().ok_or("尚未設定 Claude API 密鑰。")?;
            let model = entry.model_id.clone().unwrap_or_else(crate::ai::models::claude_default);
            super::claude::ClaudeWire::new(&key, &model).complete(request, language).await
        }
        "bedrock" => {
            // A saved row whose Keychain item is gone (after a restore, say)
            // fails here. The user is asked to save the credentials again;
            // the Keychain or database text goes to the log only.
            let creds = crate::aws::credentials::load_from(db)
                .map_err(|e| {
                    log::warn!("Bedrock completion: the saved AWS credentials could not be read: {e}");
                    language.pick(
                        "JARVIS could not read your saved AWS credentials. Save them again in Settings and try again.",
                        "JARVIS 未能讀取已儲存的 AWS 憑證，請在設定中重新儲存後重試。",
                    )
                })?
                .ok_or_else(|| {
                    language.pick("AWS credentials are not set up. Add them in Settings and try again.", "尚未設定 AWS 憑證，請在設定中加入後重試。")
                })?;
            // The chain row's model, pointed at the inference profile of the
            // saved region (load_from derives it from the region; chat reads
            // the stored profile, and the two agree). This id is the one sent
            // and the one its row is stored against.
            let model = super::apply_inference_profile(&super::bedrock_model(entry), &creds.inference_profile);
            super::bedrock::BedrockWire::new(creds, &model).complete(request, language).await
        }
        other => Err(format!("{other} 暫不支援這項功能，請在設定中改用其他 AI。")),
    }
}

/// The JSON object in a reply, without code fences or prose around it.
fn json_object_text(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let value: Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    value.is_object().then(|| value.to_string())
}

/// A reply's text and the usage it reported. An empty reply is an error.
pub(crate) fn done(text: String, provider: String, model: String, input: Option<&Value>, output: Option<&Value>) -> Result<Completion, String> {
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
        cache_write_tokens: 0,
        usage_missing: false,
        cost_micro: None,
        price_unknown: false,
    })
}

/// One plain request: the reply's JSON, or what went wrong in the user's
/// terms.
pub(crate) async fn post(
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
/// shown: it can echo the request. Gemini reports a bad key as 400. A
/// refused request (400, 404) also carries the reply's `error.message` for
/// the card, as chat does (failure::refusal); nothing else of the body.
fn rejection(name: &str, status: u16, body: &str) -> String {
    let lower = body.to_lowercase();
    let bad_key = ["api_key_invalid", "api key not valid", "invalid_api_key", "incorrect api key", "invalid x-api-key", "invalid api key", "authentication"]
        .iter()
        .any(|m| lower.contains(m));
    match status {
        401 | 403 => format!("{name} 拒絕了 API 密鑰（{status}），請檢查後重試。"),
        400 if bad_key => format!("{name} 拒絕了 API 密鑰，請檢查是否完整複製。"),
        400 => super::failure::refusal(format!("{name} 不接受這個請求（400），請檢查所選模型。"), body),
        404 => super::failure::refusal(format!("{name} 找不到所選模型或網址（404）。"), body),
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
        // A refused request keeps the reply's error.message for the card
        // (X5) and nothing else of the body; its class and its log line are
        // JARVIS's sentence's, whatever the message says.
        let other = rejection("Qwen", 400, r#"{"error":{"message":"ThrottlingException: limiting requests"},"echo":"PRIVATE-MAIL-CANARY"}"#);
        assert!(other.starts_with("Qwen 不接受這個請求（400），請檢查所選模型。") && !other.contains("CANARY"), "{other}");
        assert_eq!(crate::ai::failure::code_of(&other), crate::ai::failure::Code::BadRequest, "not a rate limit: {other}");
        assert_eq!(crate::ai::failure::log_line("Qwen", &other), "Qwen failed (BadRequest): HTTP 400");
        assert_eq!(rejection("Qwen", 404, "<html>PRIVATE-MAIL-CANARY</html>"), "Qwen 找不到所選模型或網址（404）。", "no message, nothing kept");
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

    /// A background job (mail sorting) whose saved model the registry
    /// disabled does not run: it fails with the message its run records,
    /// nothing is sent or stored, and neither the replacement nor the next
    /// entry of the chain answers in its place. Mail analysis, which the
    /// user waits on, gets the error card instead.
    #[tokio::test]
    async fn a_background_job_on_a_disabled_model_records_it_and_does_not_run() {
        let db = crate::db::test_database();
        let _registry = crate::ai::registry::test_override::with(|r| {
            let entry = r.models.iter_mut().find(|m| m.id == "gpt-5.6-luna").unwrap();
            entry.status = crate::ai::registry::Status::Disabled;
            entry.replacement = Some("gpt-6-luna".into());
        });
        let mut router = AiRouter::new("gemini_primary");
        router.set_openai_key("sk-test-not-sent");
        let row = |position: i32, provider_type: &str, model_id: Option<&str>| ProviderChainEntry { position, provider_type: provider_type.into(), endpoint_id: None, model_id: model_id.map(str::to_string), enabled: true, base_url: None };
        router.set_provider_chain(vec![row(0, "openai", Some("gpt-5.6-luna")), row(1, "claude", None)]);
        let error = complete(&router, &db, sort_request(true)).await.unwrap_err();
        let expected = crate::ai::disabled::Disabled { provider: "openai".into(), model: "gpt-5.6-luna".into(), replacement: Some("gpt-6-luna".into()) }.message(super::super::bedrock::UiLanguage::En);
        assert_eq!(error, expected, "the run records why it did not run");
        let analysis = complete(&router, &db, Request { feature: Feature::MailAnalysis, ..sort_request(false) }).await.unwrap_err();
        let card = crate::ai::failure::parse(&analysis).expect("the error card");
        assert_eq!((card["code"].as_str(), card["alternatives"][0]["pick"]["model_id"].as_str()), (Some("disabled"), Some("gpt-6-luna")));
        assert!(crate::ai::cost::tests::usage_rows(&db).is_empty(), "nothing was sent");
    }

    /// Claude JSON is asked for in words, as for Bedrock: no tool, no forced
    /// `tool_choice` and no prefilled reply. Claude's current models refuse a
    /// forced tool choice and a prefill with a 400 (P02's tool path, changed
    /// in P06 fix round 4, review A I-4). No model id decides this.
    #[test]
    fn a_claude_json_request_asks_in_words_with_no_tool_and_no_prefill() {
        let body = claude_body("claude-sonnet-4-6", &sort_request(true));
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["max_tokens"], 2_048);
        assert_eq!(body["system"], format!("Sort this email.\n\n{}", super::super::bedrock::JSON_INSTRUCTION));
        assert_eq!(body["messages"], json!([{"role": "user", "content": "From: a@example.com"}]), "no assistant prefill");
        assert!(body.get("tools").is_none() && body.get("tool_choice").is_none(), "{body}");
        let no_system = claude_body("claude-sonnet-4-6", &Request { system: "", ..sort_request(true) });
        assert_eq!(no_system["system"], super::super::bedrock::JSON_INSTRUCTION);

        let plain = claude_body("claude-sonnet-4-6", &Request { system: "", ..sort_request(false) });
        assert!(plain.get("tools").is_none() && plain.get("tool_choice").is_none());
        assert!(plain.get("system").is_none(), "no empty system prompt");
    }

    fn claude_entry() -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: "claude".into(), endpoint_id: None, model_id: None, enabled: true, base_url: None }
    }

    fn claude_text_reply(text: &str) -> Value {
        json!({
            "id": "msg_fixture", "type": "message", "role": "assistant", "model": "claude-sonnet-4-6",
            "content": [{"type": "text", "text": text}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 812, "output_tokens": 64}
        })
    }

    /// The reply is checked for one object after its row is stored: a
    /// fenced object is taken out of the fence; prose is a clear failure,
    /// which is not sent again (not a transient failure, so `run_chosen`
    /// does not resend it).
    #[test]
    fn a_claude_json_reply_is_checked_for_one_object() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let fenced = claude_result(&claude_text_reply("```json\n{\"category\": \"action\", \"summary\": \"Reply by Friday\"}\n```"), "claude-sonnet-4-6".into()).unwrap();
        let done = store_then_check(&db, &claude_entry(), &sort_request(true), fenced, 1).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&done.text).unwrap(), json!({"category": "action", "summary": "Reply by Friday"}));
        assert_eq!((done.provider.as_str(), done.input_tokens, done.output_tokens), ("Claude", 812, 64));

        let prose = claude_result(&claude_text_reply("This email asks for a reply by Friday."), "claude-sonnet-4-6".into()).unwrap();
        let error = store_then_check(&db, &claude_entry(), &sort_request(true), prose, 1).unwrap_err();
        assert_eq!(error, "Claude (claude-sonnet-4-6) did not reply with valid JSON. Try again, or choose another model.");
        assert!(!crate::ai::failure::code_of(&error).transient(), "not sent again");
        assert_eq!(crate::ai::cost::tests::usage_rows(&db).len(), 2, "both billed replies are stored");
        let plain = claude_result(&claude_text_reply("The email asks for a reply."), "m".into()).unwrap();
        assert_eq!(store_then_check(&db, &claude_entry(), &sort_request(false), plain, 1).unwrap().text, "The email asks for a reply.");
    }

    #[test]
    fn a_plain_claude_reply_is_its_text() {
        let value = json!({"content": [{"type": "text", "text": "Reply "}, {"type": "text", "text": "by Friday"}], "usage": {"input_tokens": 10, "output_tokens": 4}});
        assert_eq!(claude_result(&value, "m".into()).unwrap().text, "Reply by Friday");
        let no_text = json!({"content": [{"type": "tool_use", "name": "json_reply", "input": {"a": 1}}], "usage": {}});
        assert!(claude_result(&no_text, "m".into()).is_err(), "only text blocks are the answer");
    }

    #[test]
    fn a_bedrock_json_request_asks_for_one_object() {
        use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, SystemContentBlock};
        let request = super::super::bedrock::plain_request("Sort this email.", "From: a@example.com", true, 2_048, super::super::bedrock::UiLanguage::En).unwrap();
        assert_eq!(request.system.len(), 1);
        let SystemContentBlock::Text(system) = &request.system[0] else { panic!("a text system block") };
        assert_eq!(system, &format!("Sort this email.\n\n{}", super::super::bedrock::JSON_INSTRUCTION));
        assert_eq!(request.messages.len(), 1);
        assert_eq!(*request.messages[0].role(), ConversationRole::User);
        assert_eq!(request.messages[0].content(), &[ContentBlock::Text("From: a@example.com".into())]);
        assert_eq!(request.inference.max_tokens(), Some(2_048));

        let plain = super::super::bedrock::plain_request("", "hi", false, 10, super::super::bedrock::UiLanguage::En).unwrap();
        assert!(plain.system.is_empty(), "no empty system block");
        let json_only = super::super::bedrock::plain_request("", "hi", true, 10, super::super::bedrock::UiLanguage::En).unwrap();
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
        let done = bedrock_result(&converse_reply("```json\n{\"category\": \"action\"}\n```"), "us.amazon.nova-pro-v1:0".into());
        assert_eq!(done.text, "```json\n{\"category\": \"action\"}\n```", "checked later, after the row is stored");
        // inputTokens leaves the cache read out; the row counts it in.
        assert_eq!((done.input_tokens, done.output_tokens, done.cached_tokens, done.cache_write_tokens), (1_000, 40, 300, 0));
        assert!(!done.usage_missing);
        assert_eq!(done.provider, "Bedrock");
    }

    #[test]
    fn a_bedrock_json_reply_is_one_object_without_fences() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let done = bedrock_result(&converse_reply("```json\n{\"category\": \"action\"}\n```"), "us.amazon.nova-pro-v1:0".into());
        let done = store_then_check(&db, &bedrock_entry(), &sort_request(true), done, 1).unwrap();
        assert_eq!(done.text, r#"{"category":"action"}"#);
        assert_eq!(crate::ai::cost::tests::usage_rows(&db).len(), 1);
    }

    /// AWS bills a reply that is not JSON, so its row is stored before the
    /// check rejects it.
    #[test]
    fn a_bedrock_reply_that_is_not_json_is_stored_then_rejected() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let prose = bedrock_result(&converse_reply("The email asks for a reply."), "us.amazon.nova-pro-v1:0".into());
        let error = store_then_check(&db, &bedrock_entry(), &sort_request(true), prose, 1).unwrap_err();
        assert_eq!(error, "Bedrock (us.amazon.nova-pro-v1:0) did not reply with valid JSON. Try again, or choose another model.");
        let rows = crate::ai::cost::tests::usage_rows(&db);
        assert_eq!(rows.len(), 1, "the billed reply is stored");
        assert_eq!((rows[0].provider.as_str(), rows[0].feature.as_str(), rows[0].input_tokens, rows[0].output_tokens), ("bedrock", "mail_sort", 1_000, 40));

        let array = bedrock_result(&converse_reply("[1, 2]"), "m".into());
        assert!(store_then_check(&db, &bedrock_entry(), &sort_request(true), array, 1).is_err(), "an array is not the object asked for");
        let plain = bedrock_result(&converse_reply("The email asks for a reply."), "m".into());
        assert_eq!(store_then_check(&db, &bedrock_entry(), &sort_request(false), plain, 1).unwrap().text, "The email asks for a reply.");
    }

    /// The name the error card gives the model a Bedrock row runs.
    fn bedrock_label(entry: &ProviderChainEntry) -> String {
        crate::ai::capability::resolve("mail_sort", entry, &[]).ready().expect("Bedrock runs mail sorting").label
    }

    /// No AWS credentials saved: Bedrock asks for them instead of saying it is
    /// not supported. (No credentials row, so the Keychain is never read.)
    #[tokio::test]
    async fn bedrock_without_credentials_asks_for_them() {
        let db = crate::db::test_database();
        let mut router = AiRouter::new("gemini_primary");
        router.set_provider_chain(vec![bedrock_entry()]);
        let label = bedrock_label(&bedrock_entry());
        let error = complete(&router, &db, sort_request(true)).await.unwrap_err();
        assert_eq!(error, format!("{label} has no usable API key or credentials. Add them in Settings, or pick another model."));
        db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('ui_locale', 'zh-HK')", []).unwrap();
        let error = complete(&router, &db, sort_request(true)).await.unwrap_err();
        assert_eq!(error, format!("{label} 沒有可用的 API 密鑰或憑證，請在設定中加入，或改用其他模型。"));
    }

    /// The new Bedrock messages of a completion are in the interface
    /// language: the not-JSON one here, the empty-reply one below, the rest
    /// in bedrock.rs.
    #[test]
    fn bedrock_completion_errors_follow_the_interface_language() {
        let db = crate::db::test_database();
        db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('ui_locale', 'zh-HK')", []).unwrap();
        let prose = bedrock_result(&converse_reply("The email asks for a reply."), "us.amazon.nova-pro-v1:0".into());
        let error = store_then_check(&db, &bedrock_entry(), &sort_request(true), prose, 1).unwrap_err();
        assert_eq!(error, "Bedrock（us.amazon.nova-pro-v1:0）回覆的不是有效 JSON，請重試或改用其他模型。");
        let empty = bedrock_result(&converse_reply(""), "us.amazon.nova-pro-v1:0".into());
        let error = store_then_check(&db, &bedrock_entry(), &sort_request(false), empty, 1).unwrap_err();
        assert_eq!(error, "Bedrock（us.amazon.nova-pro-v1:0）沒有回覆內容，請重試或改用其他模型。");
        assert_eq!(crate::ai::cost::tests::usage_rows(&db).len(), 2, "both billed replies are stored");
    }

    /// AWS bills an empty reply, so it is stored before it is rejected.
    #[test]
    fn an_empty_bedrock_reply_is_stored_then_rejected() {
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let empty = bedrock_result(&converse_reply("  "), "apac.amazon.nova-micro-v1:0".into());
        let error = store_then_check(&db, &bedrock_entry(), &sort_request(false), empty, 1).unwrap_err();
        assert_eq!(error, "Bedrock (apac.amazon.nova-micro-v1:0) returned no answer. Try again, or choose another model.");
        let rows = crate::ai::cost::tests::usage_rows(&db);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].model_id.as_str(), rows[0].input_tokens, rows[0].output_tokens), ("apac.amazon.nova-micro-v1:0", 1_000, 40));
    }

    /// A reply without usage figures is not stored as a row of zeros, and
    /// its cost is unknown.
    #[test]
    fn a_bedrock_reply_without_usage_stores_nothing() {
        use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, ConverseOutput as Output, Message, StopReason};
        let output = aws_sdk_bedrockruntime::operation::converse::ConverseOutput::builder()
            .output(Output::Message(Message::builder().role(ConversationRole::Assistant).content(ContentBlock::Text("OK".into())).build().unwrap()))
            .stop_reason(StopReason::EndTurn)
            .build()
            .unwrap();
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let done = bedrock_result(&output, "us.amazon.nova-pro-v1:0".into());
        assert!(done.usage_missing);
        let done = store_then_check(&db, &bedrock_entry(), &sort_request(false), done, 1).unwrap();
        assert_eq!((done.text.as_str(), done.cost_micro, done.price_unknown), ("OK", None, true));
        assert!(crate::ai::cost::tests::usage_rows(&db).is_empty());
    }

    /// A Bedrock completion goes out to a stand-in for AWS. The model id the
    /// user typed carries the US geography and the credentials were saved for
    /// Singapore, so the request goes out as the APAC profile, and the row is
    /// stored against that id with the four figures AWS returned, including
    /// the cache write.
    #[test]
    fn a_bedrock_completion_stores_the_usage_aws_returned_for_the_model_it_sent() {
        let _keychain = crate::aws::credentials::tests::KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let db = std::sync::Arc::new(crate::db::test_database());
            let creds = crate::ai::bedrock::tests::fixture_creds("ap-southeast-1");
            crate::aws::credentials::AwsCredsRepo::new(db.clone()).save(&creds).await.unwrap();
            let mut router = AiRouter::new("gemini_primary");
            let saved = ProviderChainEntry { model_id: Some("us.anthropic.claude-haiku-4-5-20251001-v1:0".into()), ..bedrock_entry() };
            router.set_provider_chain(vec![saved.clone()]);

            let server = crate::ai::bedrock::tests::aws_loopback(200, None, crate::ai::bedrock::tests::converse_json("{\"category\": \"action\"}")).await;
            let done = complete(&router, &db, sort_request(true)).await.unwrap();
            let received = server.await.unwrap();
            assert_eq!(received.path, "/model/apac.anthropic.claude-haiku-4-5-20251001-v1:0/converse");
            assert!(received.body.contains(super::super::bedrock::JSON_INSTRUCTION), "a JSON request asks for one object");
            assert_eq!(done.text, r#"{"category":"action"}"#);
            assert_eq!(done.model, "apac.anthropic.claude-haiku-4-5-20251001-v1:0");

            let rows = crate::ai::cost::tests::usage_rows(&db);
            assert_eq!(rows.len(), 1);
            assert_eq!((rows[0].provider.as_str(), rows[0].feature.as_str(), rows[0].model_id.as_str()), ("bedrock", "mail_sort", "apac.anthropic.claude-haiku-4-5-20251001-v1:0"));
            assert_eq!((rows[0].input_tokens, rows[0].output_tokens), (1_320, 45));
            assert_eq!(crate::ai::cost::tests::cache_tokens(&db), vec![(800, 400)]);

            // A request AWS refuses stores nothing, and says why in the
            // interface language.
            let server = crate::ai::bedrock::tests::aws_loopback(403, Some("AccessDeniedException"), json!({"message": "PRIVATE-MAIL-CANARY"})).await;
            let error = complete(&router, &db, sort_request(true)).await.unwrap_err();
            server.await.unwrap();
            assert_eq!(error, format!("{} rejected the API key or credentials. Check them in Settings, or pick another model.", bedrock_label(&saved)));
            assert!(!error.contains("CANARY"));
            assert_eq!(crate::ai::cost::tests::usage_rows(&db).len(), 1, "nothing stored for the refused request");
        });
        let _ = crate::security::keychain::KeychainStore::delete("com.jarvis.aws", "secret_access_key");
    }

    /// Credentials saved on this Mac whose Keychain item is gone: the user is
    /// asked to save them again, in the interface language, and sees no
    /// Keychain text. Nothing is sent, so nothing is stored.
    #[test]
    fn bedrock_credentials_that_cannot_be_read_ask_to_be_saved_again() {
        let _keychain = crate::aws::credentials::tests::KEYCHAIN_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let db = std::sync::Arc::new(crate::db::test_database());
            crate::aws::credentials::AwsCredsRepo::new(db.clone()).save(&crate::ai::bedrock::tests::fixture_creds("ap-southeast-1")).await.unwrap();
            crate::security::keychain::KeychainStore::delete("com.jarvis.aws", "secret_access_key").unwrap();
            let mut router = AiRouter::new("gemini_primary");
            router.set_provider_chain(vec![bedrock_entry()]);
            let label = bedrock_label(&bedrock_entry());

            let error = complete(&router, &db, sort_request(true)).await.unwrap_err();
            assert_eq!(error, format!("{label} has no usable API key or credentials. Add them in Settings, or pick another model."));
            db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('ui_locale', 'zh-HK')", []).unwrap();
            let error = complete(&router, &db, sort_request(true)).await.unwrap_err();
            assert_eq!(error, format!("{label} 沒有可用的 API 密鑰或憑證，請在設定中加入，或改用其他模型。"));
            assert!(!error.to_lowercase().contains("keychain"));
            assert!(crate::ai::cost::tests::usage_rows(&db).is_empty());
        });
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
    /// (Not mail sorting: no mail goes to a service whose terms JARVIS cannot
    /// confirm.)
    #[tokio::test]
    async fn a_completion_is_stored_with_its_feature_and_provider_id() {
        let (port, server) = one_reply(json!({"choices": [{"message": {"content": "{\"category\": \"action\"}"}}], "usage": {"prompt_tokens": 300, "completion_tokens": 20}})).await;
        let base = format!("http://127.0.0.1:{port}/v1");
        let mut router = AiRouter::new("gemini_primary");
        router.set_compat_key(&base, "sk-fixture-not-a-real-key");
        router.set_provider_chain(vec![ProviderChainEntry { position: 0, provider_type: "openai".into(), endpoint_id: None, model_id: Some("unlisted-chat-1".into()), enabled: true, base_url: Some(base) }]);
        let db = crate::ai::cost::tests::temp_metrics_db("");
        let done = complete(&router, &db, Request { feature: Feature::Other, ..sort_request(true) }).await.unwrap();
        assert_eq!((done.cost_micro, done.price_unknown), (None, true));
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /v1/chat/completions"), "{request}");
        assert_eq!(
            crate::ai::cost::tests::usage_rows(&db),
            vec![crate::ai::cost::tests::UsageRow {
                provider: "compat:127.0.0.1".into(),
                feature: "other".into(),
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

    // -----------------------------------------------------------------------
    // Each wire's plain request pinned against a loopback stand-in (P07).
    // -----------------------------------------------------------------------

    use crate::ai::wire_tests::{point_at, serve, Reply};

    fn sort() -> Request<'static> {
        Request { system: "Sort this.", user: "From: a@example.com", json: true, max_tokens: 300, feature: Feature::MailSort }
    }

    fn row(provider_type: &str, model: Option<&str>, base_url: Option<&str>, endpoint_id: Option<&str>) -> ProviderChainEntry {
        ProviderChainEntry { position: 0, provider_type: provider_type.into(), endpoint_id: endpoint_id.map(Into::into), model_id: model.map(Into::into), enabled: true, base_url: base_url.map(Into::into) }
    }

    fn local_router(url: &str, backend_type: crate::ai::local::backend::BackendType, api_key: Option<&str>) -> AiRouter {
        let mut router = AiRouter::new("gemini_primary");
        router.set_local_endpoints(vec![crate::ai::local::backend::LocalEndpoint {
            id: "lm".into(), name: "Local".into(), url: url.into(), backend_type, api_key: api_key.map(Into::into), use_tls: false,
            connection_timeout_ms: 5000, keep_alive_minutes: 30, is_active: true, last_health_check: None, last_health_status: None,
        }]);
        router
    }

    fn figures(done: &Completion) -> (&str, &str, &str, i64, i64, i64, i64) {
        (done.text.as_str(), done.provider.as_str(), done.model.as_str(), done.input_tokens, done.output_tokens, done.cached_tokens, done.cache_write_tokens)
    }

    #[tokio::test]
    async fn an_openai_plain_request() {
        let server = serve(vec![Reply::json(200, json!({"choices": [{"message": {"content": "{\"a\":1}"}}], "usage": {"prompt_tokens": 10, "completion_tokens": 2, "prompt_tokens_details": {"cached_tokens": 4}}}))]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let mut router = AiRouter::new("gemini_primary");
        router.set_openai_key("sk-fixture-openai-not-real");
        let db = crate::db::test_database();
        let done = complete_one(&router, &db, &row("openai", None, None, None), &sort()).await.unwrap();
        let model = crate::ai::models::openai_default();
        assert_eq!(figures(&done), ("{\"a\":1}", "OpenAI", model.as_str(), 10, 2, 4, 0));
        let seen = server.seen();
        assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", "/v1/chat/completions"));
        assert_eq!(seen[0].header("authorization"), Some("Bearer sk-fixture-openai-not-real"));
        assert_eq!(
            seen[0].body,
            json!({"model": model, "messages": [{"role": "system", "content": "Sort this."}, {"role": "user", "content": "From: a@example.com"}], "max_completion_tokens": 300, "response_format": {"type": "json_object"}})
        );
    }

    /// A service that takes OpenAI's format at its own address: its own
    /// key, `max_tokens` (not OpenAI's `max_completion_tokens`), the JSON
    /// switch, and the answer and usage read back. A service the app does
    /// not know is named by its host.
    #[tokio::test]
    async fn a_compatible_plain_request() {
        let server = serve(vec![Reply::json(200, json!({"choices": [{"message": {"content": "{\"a\":1}"}}], "usage": {"prompt_tokens": 10, "completion_tokens": 2, "prompt_tokens_details": {"cached_tokens": 4}}}))]).await;
        let url = format!("{}/v1", server.base);
        let mut router = AiRouter::new("gemini_primary");
        router.set_compat_key(&url, "sk-fixture-compat-not-real");
        let db = crate::db::test_database();
        let done = complete_one(&router, &db, &row("openai", Some("unlisted-chat-1"), Some(&url), None), &sort()).await.unwrap();
        assert_eq!(figures(&done), ("{\"a\":1}", "127.0.0.1", "unlisted-chat-1", 10, 2, 4, 0));
        let seen = server.seen();
        assert_eq!((seen.len(), seen[0].method.as_str(), seen[0].path.as_str()), (1, "POST", "/v1/chat/completions"));
        assert_eq!(seen[0].header("authorization"), Some("Bearer sk-fixture-compat-not-real"));
        assert_eq!(
            seen[0].body,
            json!({"model": "unlisted-chat-1", "messages": [{"role": "system", "content": "Sort this."}, {"role": "user", "content": "From: a@example.com"}], "max_tokens": 300, "response_format": {"type": "json_object"}})
        );
    }

    /// Gmail and Calendar content (decision 115): for each feature that
    /// carries it, a completion refuses before any request when mail_policy
    /// would not let the row receive it: a service whose terms JARVIS
    /// cannot confirm, and Gemini before the user confirmed billing, chosen
    /// or picked in the error card. A row that may receive it still gets its
    /// one request; so does Gemini once billing is confirmed.
    #[tokio::test]
    async fn google_content_goes_only_where_mail_policy_allows() {
        use crate::ai::capability::{with_pick, Pick};
        use crate::ai::mail_policy::{carries_google_content, test_support::agree_to_gemini};
        let reply = || Reply::json(200, json!({"choices": [{"message": {"content": "Kenny asks for the invoice."}}], "candidates": [{"content": {"parts": [{"text": "Kenny asks for the invoice."}]}}], "usage": {"prompt_tokens": 10, "completion_tokens": 2}, "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 2}}));
        for feature in [Feature::MailAnalysis, Feature::MailSort, Feature::Briefing, Feature::Memory] {
            assert!(carries_google_content(feature), "{feature:?}");
            let request = || Request { system: "", user: "From: kenny@example.com\nSubject: Invoice\n\nPlease pay by Friday.", json: false, max_tokens: 300, feature };
            let server = serve((0..4).map(|_| reply()).collect()).await;
            let compat_url = format!("{}/v1", server.base);
            let mut router = AiRouter::new("gemini_primary");
            router.set_compat_key(&compat_url, "sk-fixture-compat-not-real");
            router.set_gemini_key("gemini-fixture-not-real");
            router.set_openai_key("sk-fixture-openai-not-real");
            let unknown = row("openai", Some("unlisted-chat-1"), Some(&compat_url), None);
            let gemini = row("gemini", Some(crate::ai::models::GEMINI_FLASH), None, None);
            let openai = row("openai", None, None, None);
            let db = crate::db::test_database();
            let sent = || server.seen().len();

            router.set_provider_chain(vec![unknown.clone()]);
            let error = complete(&router, &db, request()).await.unwrap_err();
            assert!(error.contains("JARVIS cannot confirm how this service uses data"), "{feature:?}: {error}");
            assert_eq!(sent(), 0, "{feature:?}: unknown terms");

            // OpenAI and Gemini at the stand-in, one at a time.
            let at_gemini = point_at(&format!("{}/v1beta", server.base));
            router.set_provider_chain(vec![gemini.clone()]);
            let error = complete(&router, &db, request()).await.unwrap_err();
            assert!(error.contains(r#""kind":"consent""#), "{feature:?}: {error}");
            assert_eq!(sent(), 0, "{feature:?}: Gemini before billing is confirmed");
            drop(at_gemini);

            let at_openai = point_at(&format!("{}/v1", server.base));
            // Picked in the error card, with an allowed AI chosen.
            router.set_provider_chain(vec![openai.clone(), unknown.clone()]);
            let error = with_pick(Some(Pick::of(&unknown)), complete(&router, &db, request())).await.unwrap_err();
            assert!(error.contains("JARVIS cannot confirm how this service uses data"), "{feature:?}: {error}");
            assert_eq!(sent(), 0, "{feature:?}: a picked service with unknown terms");
            // No notice or card of its own: the caller's gate raises that.
            for name in std::collections::BTreeSet::from([crate::ai::capability::feature_key(feature, Workload::Volume), feature.as_str()]) {
                for raised in [format!("google_content|127.0.0.1|{name}"), format!("consent|gemini||{name}")] {
                    assert!(crate::ai::notify::first_time(&raised), "{feature:?}: {raised} was raised");
                }
            }

            router.set_provider_chain(vec![openai.clone()]);
            complete(&router, &db, request()).await.unwrap_or_else(|e| panic!("{feature:?}: {e}"));
            assert_eq!(sent(), 1, "{feature:?}: OpenAI may receive it");
            drop(at_openai);

            agree_to_gemini(&db);
            let _at_gemini = point_at(&format!("{}/v1beta", server.base));
            router.set_provider_chain(vec![gemini.clone()]);
            complete(&router, &db, request()).await.unwrap_or_else(|e| panic!("{feature:?}: {e}"));
            assert_eq!(sent(), 2, "{feature:?}: Gemini once billing is confirmed");
        }
    }

    /// P07: the official OpenAI completion takes its model as chat does
    /// (`Target::model`): a saved id that is blank goes out as the default
    /// (before: sent blank, and refused), and one with spaces round it goes
    /// out trimmed.
    #[tokio::test]
    async fn a_blank_saved_openai_model_goes_out_as_the_default() {
        let reply = || Reply::json(200, json!({"choices": [{"message": {"content": "{}"}}], "usage": {"prompt_tokens": 1, "completion_tokens": 1}}));
        let server = serve(vec![reply(), reply()]).await;
        let _base = point_at(&format!("{}/v1", server.base));
        let mut router = AiRouter::new("gemini_primary");
        router.set_openai_key("sk-fixture-openai-not-real");
        let db = crate::db::test_database();
        complete_one(&router, &db, &row("openai", Some("  "), None, None), &sort()).await.unwrap();
        complete_one(&router, &db, &row("openai", Some(" gpt-fixture-1 "), None, None), &sort()).await.unwrap();
        let seen = server.seen();
        assert_eq!(seen[0].body["model"], crate::ai::models::openai_default());
        assert_eq!(seen[1].body["model"], "gpt-fixture-1");
    }

    #[tokio::test]
    async fn a_claude_plain_request() {
        let server = serve(vec![Reply::json(200, json!({"content": [{"type": "text", "text": "{\"a\":1}"}], "usage": {"input_tokens": 10, "output_tokens": 2, "cache_read_input_tokens": 0}}))]).await;
        let _base = point_at(&server.base);
        let mut router = AiRouter::new("gemini_primary");
        router.set_claude_key("sk-ant-fixture-not-real");
        let db = crate::db::test_database();
        let done = complete_one(&router, &db, &row("claude", None, None, None), &sort()).await.unwrap();
        let model = crate::ai::models::claude_default();
        assert_eq!(figures(&done), ("{\"a\":1}", "Claude", model.as_str(), 10, 2, 0, 0));
        let seen = server.seen();
        assert_eq!(seen[0].path, "/v1/messages");
        assert_eq!(seen[0].header("x-api-key"), Some("sk-ant-fixture-not-real"));
        assert_eq!(seen[0].header("anthropic-version"), Some(crate::ai::models::anthropic_version().as_str()));
        assert_eq!(
            seen[0].body,
            json!({"model": model, "max_tokens": 300, "messages": [{"role": "user", "content": "From: a@example.com"}], "system": format!("Sort this.\n\n{}", crate::ai::bedrock::JSON_INSTRUCTION)})
        );
    }

    #[tokio::test]
    async fn a_gemini_plain_request() {
        let reply = json!({
            "candidates": [{"content": {"parts": [{"text": "Weighing it.", "thought": true}, {"text": "{\"a\":1}"}]}}],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 2, "thoughtsTokenCount": 3, "cachedContentTokenCount": 4}
        });
        let server = serve(vec![Reply::json(200, reply)]).await;
        let _base = point_at(&format!("{}/v1beta", server.base));
        let mut router = AiRouter::new("gemini_primary");
        router.set_gemini_key("AIza-fixture-not-real");
        let db = crate::db::test_database();
        let done = complete_one(&router, &db, &row("gemini", None, None, None), &sort()).await.unwrap();
        let model = crate::ai::models::gemini_flash();
        // The thought is not the answer, and is billed as output.
        assert_eq!(figures(&done), ("{\"a\":1}", "Gemini", model.as_str(), 10, 5, 4, 0));
        let seen = server.seen();
        assert_eq!(seen[0].path, format!("/v1beta/models/{model}:generateContent"));
        assert_eq!(seen[0].header("x-goog-api-key"), Some("AIza-fixture-not-real"));
        assert_eq!(
            seen[0].body,
            json!({
                "contents": [{"role": "user", "parts": [{"text": "From: a@example.com"}]}],
                "generationConfig": {"maxOutputTokens": 300, "responseMimeType": "application/json"},
                "systemInstruction": {"parts": [{"text": "Sort this."}]}
            })
        );
    }

    /// Ollama's own endpoint, so the context window can be set; any other
    /// server on this Mac gets the OpenAI format, with its key if it has one.
    #[tokio::test]
    async fn a_local_plain_request() {
        use crate::ai::local::backend::BackendType;
        let server = serve(vec![
            Reply::json(200, json!({"message": {"role": "assistant", "content": "{\"a\":1}"}, "prompt_eval_count": 10, "eval_count": 2})),
            Reply::json(200, json!({"choices": [{"message": {"content": "{\"a\":2}"}}], "usage": {"prompt_tokens": 11, "completion_tokens": 3}})),
        ])
        .await;
        let db = crate::db::test_database();
        let entry = row("local", Some("qwen3:8b"), None, Some("lm"));
        let done = complete_one(&local_router(&server.base, BackendType::Ollama, None), &db, &entry, &sort()).await.unwrap();
        assert_eq!(figures(&done), ("{\"a\":1}", "Ollama", "qwen3:8b", 10, 2, 0, 0));
        let done = complete_one(&local_router(&server.base, BackendType::Vllm, Some("local-fixture-key")), &db, &entry, &sort()).await.unwrap();
        assert_eq!(figures(&done), ("{\"a\":2}", "本機 AI", "qwen3:8b", 11, 3, 0, 0));
        let seen = server.seen();
        assert_eq!(seen[0].path, "/api/chat");
        let context = crate::ai::models::limits("ollama").completion_context_tokens.unwrap();
        assert_eq!(
            seen[0].body,
            json!({"model": "qwen3:8b", "messages": [{"role": "system", "content": "Sort this."}, {"role": "user", "content": "From: a@example.com"}], "stream": false, "options": {"num_ctx": context, "num_predict": 300}, "format": "json"})
        );
        assert_eq!(seen[1].path, "/v1/chat/completions");
        assert_eq!(seen[1].header("authorization"), Some("Bearer local-fixture-key"));
        assert_eq!(
            seen[1].body,
            json!({"model": "qwen3:8b", "messages": [{"role": "system", "content": "Sort this."}, {"role": "user", "content": "From: a@example.com"}], "max_tokens": 300, "response_format": {"type": "json_object"}})
        );
    }

    /// A refusal names the service and the status, in the user's terms, and
    /// never the body (it can echo the mail); an empty answer is an error.
    #[tokio::test]
    async fn refused_and_empty_plain_requests() {
        let canary = json!({"error": {"message": "PRIVATE-MAIL-CANARY"}});
        let server = serve(vec![
            Reply::json(401, canary.clone()),
            Reply::json(429, canary.clone()),
            Reply::json(400, json!({"error": {"message": "API key not valid. PRIVATE-MAIL-CANARY"}})),
            Reply::json(500, canary.clone()),
            Reply::json(200, json!({"choices": [{"message": {"content": "  "}}]})),
        ])
        .await;
        let _base = point_at(&server.base);
        let mut router = local_router(&server.base, crate::ai::local::backend::BackendType::Vllm, None);
        router.set_claude_key("sk-ant-fixture-not-real");
        router.set_openai_key("sk-fixture-openai-not-real");
        router.set_gemini_key("AIza-fixture-not-real");
        let db = crate::db::test_database();
        let claude = complete_one(&router, &db, &row("claude", None, None, None), &sort()).await.unwrap_err();
        assert_eq!(claude, "Claude 拒絕了 API 密鑰（401），請檢查後重試。");
        let openai = complete_one(&router, &db, &row("openai", None, None, None), &sort()).await.unwrap_err();
        assert_eq!(openai, "OpenAI 暫時限速或額度不足（429），請稍後重試。");
        let gemini = complete_one(&router, &db, &row("gemini", None, None, None), &sort()).await.unwrap_err();
        assert_eq!(gemini, "Gemini 拒絕了 API 密鑰，請檢查是否完整複製。");
        let local = complete_one(&router, &db, &row("local", Some("my-model"), None, Some("lm")), &sort()).await.unwrap_err();
        assert_eq!(local, "本機 AI 暫時未能回覆（500），請稍後重試。");
        let empty = complete_one(&router, &db, &row("local", Some("my-model"), None, Some("lm")), &sort()).await.unwrap_err();
        assert_eq!(empty, "本機 AI 沒有回覆內容。");
        for error in [claude, openai, gemini, local] {
            assert!(!error.contains("CANARY"), "{error}");
        }
    }

    /// X5: "Test and use this AI" (commands::ai_choice::choose_ai, one
    /// plain request for `Feature::Other`) on a Claude key Anthropic refuses
    /// with a 400, in Anthropic's error shape. The card names the model and
    /// the class in the interface language and quotes the reply's
    /// `error.message`, as chat does; before, `provider_reason` was empty.
    /// The other wires that send a plain request through `post` carry it
    /// the same way. Nothing else of the reply reaches the card, and the log
    /// line has the status and the codes only.
    #[tokio::test]
    async fn a_refused_plain_request_shows_the_services_reason() {
        use crate::ai::bedrock::UiLanguage;
        let scoped = "This API key is not scoped to a workspace, so this request must include the anthropic-workspace-id header";
        let replies = [
            ("claude", json!({"type": "error", "error": {"type": "invalid_request_error", "message": scoped}, "request_id": "req_PRIVATE-MAIL-CANARY"})),
            ("openai", json!({"error": {"message": "Unsupported value: PRIVATE-REASON-OPENAI", "type": "invalid_request_error", "param": "PRIVATE-MAIL-CANARY", "code": "unsupported_value"}})),
            ("gemini", json!({"error": {"code": 400, "message": "Request contains an invalid argument.", "status": "INVALID_ARGUMENT", "details": [{"echo": "PRIVATE-MAIL-CANARY"}]}})),
            ("compatible", json!({"error": {"message": "Model Not Exist", "type": "invalid_request_error", "echo": "PRIVATE-MAIL-CANARY"}})),
        ];
        for language in [UiLanguage::En, UiLanguage::ZhHk] {
            for (wire, reply) in &replies {
                let server = serve(vec![Reply::json(400, reply.clone())]).await;
                let compat_url = format!("{}/v1", server.base);
                let mut router = AiRouter::new("gemini_primary");
                let (_base, entry) = match *wire {
                    "claude" => {
                        router.set_claude_key("sk-ant-fixture-not-real");
                        (point_at(&server.base), row("claude", None, None, None))
                    }
                    "openai" => {
                        router.set_openai_key("sk-fixture-openai-not-real");
                        (point_at(&compat_url), row("openai", None, None, None))
                    }
                    "gemini" => {
                        router.set_gemini_key("AIza-fixture-not-real");
                        (point_at(&format!("{}/v1beta", server.base)), row("gemini", None, None, None))
                    }
                    _ => {
                        router.set_compat_key(&compat_url, "sk-fixture-compat-not-real");
                        (point_at(&server.base), row("openai", Some("unlisted-chat-1"), Some(&compat_url), None))
                    }
                };
                router.set_provider_chain(vec![entry]);
                let db = crate::db::test_database();
                if language == UiLanguage::ZhHk {
                    db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('ui_locale', 'zh-HK')", []).unwrap();
                }
                let mut result = None;
                let logged = crate::test_log::capture(|| async {
                    result = Some(complete(&router, &db, Request { system: "", user: "Reply with the single word OK.", json: false, max_tokens: 1_024, feature: Feature::Other }).await);
                })
                .await;
                let error = result.unwrap().unwrap_err();
                let card = crate::ai::failure::parse(&error).unwrap_or_else(|| panic!("{wire}: a card, not {error}"));
                let label = card["model"]["label"].as_str().unwrap().to_string();
                assert_eq!(server.seen().len(), 1, "{wire}: sent once");
                assert_eq!(card["code"], "bad_request", "{wire}");
                assert_eq!(
                    card["message"],
                    language.pick(format!("{label} did not accept this request (400). Check the chosen model, or pick another one."), format!("{label} 不接受這個請求（400），請檢查所選模型，或改用其他模型。")),
                    "{wire}"
                );
                assert_eq!(card["provider_reason"], reply["error"]["message"], "{wire}: the service's own reason");
                assert!(!error.contains("CANARY"), "{wire}: nothing else of the reply: {error}");
                let reason = reply["error"]["message"].as_str().unwrap();
                assert!(!logged.iter().any(|line| line.contains(reason) || line.contains("CANARY")), "{wire}: {logged:?}");
                assert!(logged.iter().any(|line| line.contains("failed (BadRequest): HTTP 400")), "{wire}: {logged:?}");
            }
        }
    }

    /// X5 review, finding 1: mail sorting is a background job, so a refused
    /// request ends in the sentence its run records and one notice, never a
    /// card. Both are JARVIS's sentence; the provider's reason, which the
    /// failure now carries for a foreground card, is in neither.
    #[test]
    fn a_background_refusal_leaves_the_services_reason_out() {
        use crate::ai::bedrock::UiLanguage;
        const REASON: &str = "Invalid value for messages PRIVATE-REASON";
        async fn refused_in_background(language: UiLanguage) {
            let server = serve(vec![Reply::json(400, json!({"error": {"message": REASON, "type": "invalid_request_error", "code": null}}))]).await;
            let _base = point_at(&format!("{}/v1", server.base));
            let mut router = AiRouter::new("gemini_primary");
            router.set_openai_key("sk-fixture-openai-not-real");
            router.set_provider_chain(vec![row("openai", None, None, None)]);
            let db = crate::db::test_database();
            if language == UiLanguage::ZhHk {
                db.conn.lock().unwrap().execute("INSERT OR REPLACE INTO user_preferences (key, value) VALUES ('ui_locale', 'zh-HK')", []).unwrap();
            }
            let sentence = complete(&router, &db, sort()).await.unwrap_err();
            assert_eq!(server.seen().len(), 1, "{language:?}: sent once");
            assert!(crate::ai::failure::parse(&sentence).is_none(), "{language:?}: no card for a background job: {sentence}");
            let said = language.pick(" did not accept this request (400). Check the chosen model, or pick another one.", " 不接受這個請求（400），請檢查所選模型，或改用其他模型。");
            assert!(sentence.ends_with(&said) && !sentence.contains("PRIVATE-REASON"), "{language:?}: {sentence}");
            let notices: Vec<_> = crate::ai::notify::emitted().into_iter().filter(|(event, _)| event == "ai-needs-model").map(|(_, notice)| notice).collect();
            assert_eq!(notices.len(), 1, "{language:?}: one notice: {notices:?}");
            assert_eq!(notices[0]["message"], json!(sentence), "{language:?}");
            assert!(!notices[0].to_string().contains("PRIVATE-REASON"), "{language:?}: {}", notices[0]);
        }
        for language in [UiLanguage::En, UiLanguage::ZhHk] {
            // Each language on its own thread: a test thread is one launch,
            // and a notice is raised once per launch (notify::first_time).
            std::thread::spawn(move || tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(refused_in_background(language))).join().unwrap();
        }
    }
}
