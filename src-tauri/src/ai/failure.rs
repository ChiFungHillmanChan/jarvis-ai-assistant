//! What the user is told when a request cannot run on the chosen model, or
//! the model failed (P06, DECISIONS.md section 5 "Failure handling"): which
//! model, why, and a one-click choice of models that support the feature.
//! JARVIS never moves the request to another model by itself.
//!
//! A request the user is waiting on fails with [`Failure::to_error`], a JSON
//! envelope the frontend turns into the error card (src/lib/ai-error.ts).
//! A background job (mail sorting, the briefing) does not prompt: its run
//! records a plain sentence and one notice is raised per launch
//! ([`Failure::surface`]).
//!
//! The reason is always JARVIS's own wording for the provider's error code.
//! A provider's error body is never shown or stored: it can quote the
//! request, and the request can be mail.

use super::bedrock::UiLanguage;
use super::capability::{Alternative, Reason, Target};
use serde::Serialize;

/// Why a request did not get an answer.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    /// No key or credentials saved for the model's service.
    NoKey,
    BadKey,
    RateLimited,
    /// The request was refused as malformed or too large.
    BadRequest,
    /// The model or the address does not exist.
    NotFound,
    Unreachable,
    EmptyReply,
    ServerError,
    /// Anything else.
    Failed,
    /// The registry turned the saved model off.
    Disabled,
    /// The model lacks a capability the feature needs.
    Unsupported,
    /// The service has no model for the feature.
    NoModel,
    /// The request needed Gmail or Calendar content, which is not sent to
    /// this service (P06, Google Limited Use).
    GoogleContent,
}

impl Code {
    /// Worth sending once more to the same model, before the card is shown.
    pub fn transient(self) -> bool {
        matches!(self, Code::RateLimited | Code::ServerError | Code::Unreachable | Code::EmptyReply)
    }
}

/// The HTTP status in the router's "<provider> API error <status>: <body>"
/// strings, or in the "（<status>）" of complete.rs's own messages.
fn status_in(err: &str) -> Option<u16> {
    let digits = |rest: &str| rest.split(|c: char| !c.is_ascii_digit()).next().and_then(|d| d.parse().ok());
    // "<provider> API error <status>: ..." from the chat wires, and
    // "<provider> STT error <status>: ..." from voice/transcribe.rs.
    if let Some(rest) = err.split(" API error ").nth(1).or_else(|| err.split(" STT error ").nth(1)) {
        return digits(rest);
    }
    err.split('（').skip(1).find_map(|rest| digits(rest).filter(|s: &u16| (100..600).contains(s)))
}

/// The `error.message` of a provider's JSON error reply
/// ("<provider> API error <status>: {...}"), on one line and capped. The
/// rest of the reply is never read out.
fn provider_reason(err: &str) -> Option<String> {
    let rest = err.split(" API error ").nth(1)?;
    let body = &rest[rest.find('{')?..];
    let value: serde_json::Value = serde_json::Deserializer::from_str(body).into_iter().next()?.ok()?;
    let message = value.pointer("/error/message")?.as_str()?.split_whitespace().collect::<Vec<_>>().join(" ");
    if message.is_empty() {
        return None;
    }
    const MAX: usize = 300;
    Some(if message.chars().count() > MAX { format!("{}…", message.chars().take(MAX).collect::<String>()) } else { message })
}

/// A sentence for an error shown where no card can be: a voice turn's toast,
/// the voice indicator and Settings (review A, I-1). A failure card gives its
/// message and where to pick another model; a consent card says where to
/// agree. Any other error is returned as it is.
pub(crate) fn plain(error: &str, language: UiLanguage) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(error) else { return error.to_string() };
    let text = |pointer: &str| value.pointer(pointer).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    match value.pointer("/jarvis_ai/kind").and_then(|kind| kind.as_str()) {
        Some("failure") => {
            let acted = value.pointer("/jarvis_ai/failure/actions_ran").and_then(|n| n.as_u64()).unwrap_or(0) > 0;
            let next = if acted {
                language.pick("Part of the request was already carried out, such as an event or a draft: check it before you ask again.", "部分操作已完成，例如日曆事件或草稿：再次提出前請先檢查。")
            } else {
                language.pick("You can pick another model in Settings, Models.", "可在「設定」的「模型」中改用其他模型。")
            };
            format!("{} {next}", text("/jarvis_ai/failure/message"))
        }
        Some("consent") => {
            let label = text("/jarvis_ai/consent/label");
            language.pick(
                format!("{label} receives Gmail and Calendar content only after you agree to its data terms. Agree in Settings, Models, then ask again."),
                format!("你同意 {label} 的資料條款後，{label} 才會收到 Gmail 及日曆內容。請在「設定」的「模型」中同意，然後再問一次。"),
            )
        }
        _ => error.to_string(),
    }
}

/// The log line for a failed request (P06, review A M-6): the service, the
/// failure class, the HTTP status and the provider's own error code or type.
/// Never the error text: a provider's error text can include the model id,
/// the request parameters it rejected and fragments of the request.
pub fn log_line(label: &str, err: &str) -> String {
    let mut line = format!("{label} failed ({:?})", code_of(err));
    if let Some(status) = status_in(err) {
        line.push_str(&format!(": HTTP {status}"));
        for code in provider_codes(err) {
            line.push_str(&format!(", {code}"));
        }
    }
    line
}

/// The log line for an error event in a reply stream (Claude's SSE
/// `error`): the service and the event's own error type or code, never its
/// message (review B, r4-m5).
pub fn stream_error_line(label: &str, event: &str) -> String {
    match plain_codes(event).as_slice() {
        [] => format!("{label} stream error"),
        codes => format!("{label} stream error: {}", codes.join(", ")),
    }
}

/// The log line for a stream event that does not parse: where and how it
/// failed and its length. Never its text, which can be reply text, nor
/// serde_json's message, which quotes it (review B, r4-m5).
pub fn unparsed_event_line(label: &str, error: &serde_json::Error, bytes: usize) -> String {
    format!("{label} stream event did not parse ({:?} at line {}, column {}; {bytes} bytes)", error.classify(), error.line(), error.column())
}

/// `error.type`, `error.code` and `error.status` of a provider's JSON error
/// reply, each only when it is a short plain code.
fn provider_codes(err: &str) -> Vec<String> {
    let Some(rest) = err.split(" API error ").nth(1) else { return Vec::new() };
    plain_codes(rest)
}

/// [`provider_codes`] of the first JSON object in `text`.
fn plain_codes(text: &str) -> Vec<String> {
    let Some(start) = text.find('{') else { return Vec::new() };
    let Some(Ok(value)) = serde_json::Deserializer::from_str(&text[start..]).into_iter::<serde_json::Value>().next() else { return Vec::new() };
    let plain = |code: &&str| !code.is_empty() && code.len() <= 64 && code.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    ["/error/type", "/error/code", "/error/status"].iter().filter_map(|pointer| value.pointer(pointer)?.as_str()).filter(plain).map(str::to_string).collect()
}

/// The code for an HTTP status and the lower-cased body, read but never
/// shown. Gemini reports a bad key, and a retired model id, as 400.
pub fn status_code(status: u16, body_lower: &str) -> Code {
    let bad_key = ["api_key_invalid", "api key not valid", "invalid_api_key", "incorrect api key", "invalid x-api-key", "invalid api key", "authentication"]
        .iter()
        .any(|m| body_lower.contains(m));
    match status {
        401 | 403 => Code::BadKey,
        400 if bad_key => Code::BadKey,
        400 if body_lower.contains("not found") || body_lower.contains("is not supported") => Code::NotFound,
        400 | 413 | 422 => Code::BadRequest,
        404 => Code::NotFound,
        429 => Code::RateLimited,
        _ => Code::ServerError,
    }
}

/// The code for an error a provider returned, as the router and
/// complete.rs carry it: JARVIS's own sentences (complete.rs, bedrock.rs),
/// Bedrock's error codes, or "<provider> API error <status>: <body>".
pub fn code_of(err: &str) -> Code {
    let lower = err.to_lowercase();
    let any = |marks: &[&str]| marks.iter().any(|m| lower.contains(m));
    if any(&["api key configured", "credentials are not set up", "credentials not configured", "could not read your saved aws credentials", "未能讀取已儲存的 aws 憑證"]) || (lower.contains("尚未設定") && (lower.contains("密鑰") || lower.contains("憑證"))) {
        return Code::NoKey;
    }
    if any(&["returned no text", "returned no answer", "沒有回覆內容"]) {
        return Code::EmptyReply;
    }
    if any(&["拒絕了 api 密鑰", "拒絕了 aws 憑證", "refused the aws credentials", "accessdeniedexception", "unrecognizedclientexception", "invalidsignatureexception", "expiredtokenexception"]) {
        return Code::BadKey;
    }
    if any(&["throttlingexception", "servicequotaexceededexception", "limiting requests", "暫時限速"]) {
        return Code::RateLimited;
    }
    if any(&["resourcenotfoundexception", "could not find the chosen model", "找不到所選模型"]) {
        return Code::NotFound;
    }
    if any(&["validationexception", "did not accept this request", "不接受這個請求"]) {
        return Code::BadRequest;
    }
    if let Some(status) = status_in(err) {
        return status_code(status, &lower);
    }
    if any(&["could not answer just now", "暫時未能回覆", "serviceunavailableexception", "internalserverexception", "modelnotreadyexception"]) {
        return Code::ServerError;
    }
    if any(&["error sending request", "timed out", "timeout", "connection refused", "dns error", "未能連接", "could not reach"]) {
        return Code::Unreachable;
    }
    Code::Failed
}

fn feature_name(key: &str, language: UiLanguage) -> String {
    let (en, zh) = match key {
        "chat" => ("chat", "聊天"),
        "premium" => ("Think harder", "深入思考"),
        "mail_analysis" => ("mail analysis", "郵件分析"),
        "mail_sort" => ("mail sorting", "自動整理"),
        "briefing" => ("the daily briefing", "每日簡報"),
        "memory" => ("memory", "記憶"),
        "stt" => ("speech to text", "語音辨識"),
        "tts" => ("reading aloud", "語音朗讀"),
        _ => ("this feature", "這項功能"),
    };
    language.pick(en, zh)
}

/// What went wrong, in the app's language, naming the model.
pub(crate) fn reason_text(code: Code, name: &str, key: &str, status: Option<u16>, language: UiLanguage) -> String {
    let feature = feature_name(key, language);
    let status_en = status.map(|s| format!(" ({s})")).unwrap_or_default();
    let status_zh = status.map(|s| format!("（{s}）")).unwrap_or_default();
    match code {
        Code::NoKey => language.pick(format!("{name} has no usable API key or credentials. Add them in Settings, or pick another model."), format!("{name} 沒有可用的 API 密鑰或憑證，請在設定中加入，或改用其他模型。")),
        Code::BadKey => language.pick(format!("{name} rejected the API key or credentials{status_en}. Check them in Settings, or pick another model."), format!("{name} 拒絕了 API 密鑰或憑證{status_zh}，請在設定中檢查，或改用其他模型。")),
        Code::RateLimited => language.pick(format!("{name} is limiting requests or is out of quota{status_en}. Try again shortly, or pick another model."), format!("{name} 暫時限速或額度不足{status_zh}，請稍後重試，或改用其他模型。")),
        Code::BadRequest => language.pick(format!("{name} did not accept this request{status_en}. Check the chosen model, or pick another one."), format!("{name} 不接受這個請求{status_zh}，請檢查所選模型，或改用其他模型。")),
        Code::NotFound => language.pick(format!("{name} could not find the chosen model or address{status_en}."), format!("{name} 找不到所選模型或網址{status_zh}。")),
        Code::Unreachable => language.pick(format!("Could not reach {name}. Check the network, or pick another model."), format!("未能連接 {name}，請檢查網絡，或改用其他模型。")),
        Code::EmptyReply => language.pick(format!("{name} returned no answer. Try again, or pick another model."), format!("{name} 沒有回覆內容，請重試或改用其他模型。")),
        Code::ServerError => language.pick(format!("{name} could not answer just now{status_en}. Try again shortly, or pick another model."), format!("{name} 暫時未能回覆{status_zh}，請稍後重試，或改用其他模型。")),
        Code::Failed => language.pick(format!("{name} could not complete this request. Try again, or pick another model."), format!("{name} 未能完成這個請求，請重試或改用其他模型。")),
        Code::Disabled => language.pick(format!("{name} has been turned off. Choose a replacement; JARVIS does not switch models until you choose."), format!("{name} 已停用，請選擇替代模型。未選擇前不會改用其他模型。")),
        Code::Unsupported => language.pick(format!("{name} cannot be used for {feature}. Pick a model that supports it."), format!("{name} 不支援{feature}，請改用支援這項功能的模型。")),
        Code::NoModel => language.pick(format!("{name} has no model for {feature}. Pick a model that supports it."), format!("{name} 沒有可用於{feature}的模型，請改用支援這項功能的模型。")),
        Code::GoogleContent => super::mail_policy::not_for_google_content(name, language),
    }
}

/// A request that did not get an answer, and what the user can do.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Failure {
    /// The registry feature ("chat", "mail_analysis", ...).
    pub feature: String,
    pub model: Target,
    pub code: Code,
    /// The capability missing, for `Unsupported`.
    pub capability: Option<String>,
    /// Why, in the app's language, naming the model.
    pub message: String,
    /// For a refused request (bad request, unknown model), the provider's
    /// own reason: its `error.message`, on one line, at most 300 characters.
    /// Nothing else of the provider's reply reaches the card.
    pub provider_reason: Option<String>,
    /// Models the user can pick; empty after an action already ran.
    pub alternatives: Vec<Alternative>,
    /// Actions (a calendar event, a draft) completed before the failure.
    /// When any ran the request is never sent again, to any model.
    pub actions_ran: usize,
}

impl Failure {
    /// A request the capability check refused before anything was sent.
    pub(crate) fn unsupported(key: &str, unsupported: super::capability::Unsupported, language: UiLanguage) -> Failure {
        let (code, capability) = match &unsupported.reason {
            Reason::Disabled { .. } => (Code::Disabled, None),
            Reason::Missing { capability } => (Code::Unsupported, Some(capability.clone())),
            Reason::NoModel | Reason::UnknownProvider => (Code::NoModel, None),
        };
        let message = reason_text(code, &unsupported.target.label, key, None, language);
        Failure { feature: key.to_string(), model: unsupported.target, code, capability, message, provider_reason: None, alternatives: unsupported.alternatives, actions_ran: 0 }
    }

    /// A request the model failed. After an action ran nothing is offered:
    /// sending it again would carry the action out twice.
    pub(crate) fn failed(key: &str, model: Target, error: &str, actions_ran: usize, alternatives: Vec<Alternative>, language: UiLanguage) -> Failure {
        let code = code_of(error);
        let message = reason_text(code, &model.label, key, status_in(error), language);
        let alternatives = if actions_ran > 0 { Vec::new() } else { alternatives.into_iter().filter(|a| a.pick != model.pick).collect() };
        let provider_reason = matches!(code, Code::BadRequest | Code::NotFound).then(|| provider_reason(error)).flatten();
        Failure { feature: key.to_string(), model, code, capability: None, message, provider_reason, alternatives, actions_ran }
    }

    /// A request that needed Gmail or Calendar content `model` may not
    /// receive (P06, Google Limited Use). `message` says why, naming the
    /// service; `alternatives` are the models that may receive it. After an
    /// action ran nothing is offered, as for any failure.
    pub(crate) fn google_content(key: &str, model: Target, message: String, actions_ran: usize, alternatives: Vec<Alternative>) -> Failure {
        let alternatives = if actions_ran > 0 { Vec::new() } else { alternatives.into_iter().filter(|a| a.pick != model.pick).collect() };
        Failure { feature: key.to_string(), model, code: Code::GoogleContent, capability: None, message, provider_reason: None, alternatives, actions_ran }
    }

    /// The error a foreground request fails with: the card's data.
    pub fn to_error(&self) -> String {
        serde_json::json!({ "jarvis_ai": { "kind": "failure", "failure": self } }).to_string()
    }

    /// Foreground: the card ([`to_error`]). Background: nothing is asked;
    /// the run records a sentence (for a model that cannot run the job,
    /// that it needs one that can), and one notice is raised for this
    /// model, feature and reason per launch.
    pub(crate) fn surface(self, foreground: bool, language: UiLanguage) -> String {
        log::warn!("[ai] {} on {} did not run: {:?}{}", self.feature, self.model.model.as_deref().unwrap_or("no model"), self.code, if foreground { "" } else { " (background)" });
        if foreground {
            return self.to_error();
        }
        let feature = feature_name(&self.feature, language);
        let sentence = match self.code {
            Code::Unsupported | Code::NoModel => language.pick(
                format!("Needs a model that supports {feature}: {} does not.", self.model.label),
                format!("需要支援{feature}的模型：{} 不支援。", self.model.label),
            ),
            _ => self.message.clone(),
        };
        let key = format!("{}|{}|{:?}|{:?}", self.feature, self.model.label, self.code, self.capability);
        if super::notify::first_time(&key) {
            super::notify::emit("ai-needs-model", serde_json::json!({ "feature": self.feature, "message": sentence, "model": self.model.label, "code": self.code }));
        }
        sentence
    }
}

/// Chat runs that carried out an action. A retry of one is refused
/// whatever the frontend sends (commands::chat): it would carry the action
/// out twice.
fn acted_runs() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static ACTED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    ACTED.get_or_init(Default::default)
}

pub fn note_acted(run_id: &str) {
    if let Ok(mut runs) = acted_runs().lock() {
        runs.insert(run_id.to_string());
    }
}

/// Record that the chat run this task belongs to carried out an action.
pub fn note_current_run_acted() {
    if let Some(run) = current_run() {
        note_acted(&run);
    }
}

/// The chat run this task belongs to, if it is one. Tests set it without
/// an app (`TEST_RUN`), since a chat context carries an AppHandle.
fn current_run() -> Option<String> {
    #[cfg(test)]
    if let Ok(run) = TEST_RUN.try_with(Clone::clone) {
        return Some(run);
    }
    crate::chat_session::CONTEXT.try_with(|c| c.run_id.clone()).ok()
}

#[cfg(test)]
tokio::task_local! { pub static TEST_RUN: String; }

/// Whether the run `retry_of` names may be sent again.
pub fn may_retry(retry_of: &str) -> bool {
    acted_runs().lock().map(|runs| !runs.contains(retry_of)).unwrap_or(false)
}

/// The failure a foreground error string carries, if it is one.
#[cfg(test)]
pub fn parse(error: &str) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(error).ok()?;
    (value.pointer("/jarvis_ai/kind")?.as_str()? == "failure").then(|| value["jarvis_ai"]["failure"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Voice turns have no card: the toast, the voice indicator and Settings
    /// show a sentence, in the UI language, saying where to pick a model or
    /// agree to terms (review A, I-1). Any other error is shown as it is.
    #[test]
    fn a_voice_turn_shows_the_cards_sentence_not_its_data() {
        use super::super::consent::{Request, Subject};
        use super::super::registry::DataTerms;
        for language in [UiLanguage::En, UiLanguage::ZhHk] {
            let failed = Failure::failed("chat", target("Gemini 3.7 Flash"), "Google Gemini API error 429: {}", 0, Vec::new(), language);
            let shown = plain(&failed.to_error(), language);
            assert_eq!(shown, format!("{} {}", failed.message, language.pick("You can pick another model in Settings, Models.", "可在「設定」的「模型」中改用其他模型。")));
            let acted = Failure::failed("chat", target("Gemini 3.7 Flash"), "Google Gemini API error 500: {}", 1, Vec::new(), language);
            assert_eq!(
                plain(&acted.to_error(), language),
                format!("{} {}", acted.message, language.pick("Part of the request was already carried out, such as an event or a draft: check it before you ask again.", "部分操作已完成，例如日曆事件或草稿：再次提出前請先檢查。"))
            );
            let consent = Request::new(Subject { provider: "gemini".into(), base_url: String::new() }, "Google Gemini".into(), DataTerms::NeedsBillingConfirmation);
            assert_eq!(
                plain(&consent.to_error(), language),
                language.pick(
                    "Google Gemini receives Gmail and Calendar content only after you agree to its data terms. Agree in Settings, Models, then ask again.",
                    "你同意 Google Gemini 的資料條款後，Google Gemini 才會收到 Gmail 及日曆內容。請在「設定」的「模型」中同意，然後再問一次。",
                )
            );
            for shown in [plain(&failed.to_error(), language), plain(&consent.to_error(), language)] {
                assert!(!shown.contains("jarvis_ai") && !shown.contains('{'), "{shown}");
            }
            assert_eq!(plain("另一個助理任務進行中，請等完成後再試。", language), "另一個助理任務進行中，請等完成後再試。");
            assert_eq!(plain("{\"not\": \"a card\"}", language), "{\"not\": \"a card\"}");
        }
    }

    /// The chat failure log line names the service, the failure class, the
    /// HTTP status and the provider's own error code or type, never the body:
    /// a provider's error text can quote the request (review A, M-6). The
    /// card still shows the provider's message to the user.
    #[test]
    fn a_failed_request_is_logged_without_the_providers_error_text() {
        let gemini = r#"Google Gemini API error 400: {"error":{"code":400,"message":"MAIL-CANARY Kenny owes HK$12,000","status":"INVALID_ARGUMENT"}}"#;
        assert_eq!(log_line("Google Gemini", gemini), "Google Gemini failed (BadRequest): HTTP 400, INVALID_ARGUMENT");
        let openai = r#"OpenAI API error 404: {"error":{"message":"MAIL-CANARY","type":"invalid_request_error","code":"model_not_found"}}"#;
        assert_eq!(log_line("OpenAI", openai), "OpenAI failed (NotFound): HTTP 404, invalid_request_error, model_not_found");
        let claude = r#"Claude API error 429: {"type":"error","error":{"type":"rate_limit_error","message":"MAIL-CANARY"}}"#;
        assert_eq!(log_line("Claude", claude), "Claude failed (RateLimited): HTTP 429, rate_limit_error");
        // A field that is not a plain code is left out, as is a body that is not JSON.
        let odd = r#"Kimi API error 400: {"error":{"type":"MAIL-CANARY Kenny owes","code":{"x":1}}}"#;
        assert_eq!(log_line("Kimi", odd), "Kimi failed (BadRequest): HTTP 400");
        assert_eq!(log_line("Kimi", "Kimi API error 502: <html>MAIL-CANARY</html>"), "Kimi failed (ServerError): HTTP 502");
        assert_eq!(log_line("OpenAI", "error sending request: connection refused"), "OpenAI failed (Unreachable)");
        for error in [gemini, openai, claude, odd] {
            assert!(!log_line("X", error).contains("CANARY"), "{error}");
        }
    }

    /// An error event in a reply stream (Claude's SSE `error`) is logged
    /// with its type only, never its message (review B, r4-m5).
    #[test]
    fn a_stream_error_event_is_logged_without_its_message() {
        let event = r#"{"type":"error","error":{"type":"overloaded_error","message":"MAIL-CANARY Kenny owes HK$12,000"}}"#;
        assert_eq!(stream_error_line("Claude", event), "Claude stream error: overloaded_error");
        assert_eq!(stream_error_line("Claude", "MAIL-CANARY, not JSON"), "Claude stream error");
        assert_eq!(stream_error_line("Claude", r#"{"error":{"type":"MAIL-CANARY Kenny owes"}}"#), "Claude stream error");
    }

    /// A stream event that does not parse is logged by where and how it
    /// failed, never by its text, which can be reply text (review B, r4-m5).
    /// serde_json's own message quotes the text, so it is not used.
    #[test]
    fn an_event_that_does_not_parse_is_logged_without_its_text() {
        let event = r#""MAIL-CANARY Kenny owes HK$12,000""#;
        let error = serde_json::from_str::<u32>(event).unwrap_err();
        assert!(error.to_string().contains("MAIL-CANARY"), "serde_json's message quotes it");
        assert_eq!(unparsed_event_line("Claude", &error, event.len()), "Claude stream event did not parse (Data at line 1, column 34; 34 bytes)");
    }
    use crate::ai::capability::Pick;

    fn target(label: &str) -> Target {
        Target { pick: Pick { provider_type: "gemini".into(), base_url: None, endpoint_id: None, model_id: None }, provider: "gemini".into(), model: Some("gemini-3.7-flash".into()), label: label.into() }
    }

    /// The reasons the old chain walk told apart, now named for the user.
    #[test]
    fn provider_errors_map_to_reasons() {
        let cases = [
            ("Stream error: error decoding response body", Code::Failed),
            ("Gemini API error 429 Too Many Requests: {\"error\":{}}", Code::RateLimited),
            ("Claude API error 503 Service Unavailable: overloaded", Code::ServerError),
            ("Local LLM API error 500 Internal Server Error: ", Code::ServerError),
            ("No Gemini API key configured", Code::NoKey),
            ("bedrock: AWS credentials not configured in Settings", Code::NoKey),
            ("Gemini API error 404 Not Found: models/x is not found for API version v1beta", Code::NotFound),
            ("Gemini API error 400 Bad Request: {\"error\":{\"status\":\"INVALID_ARGUMENT\",\"message\":\"API_KEY_INVALID\"}}", Code::BadKey),
            ("Claude API error 401 Unauthorized: invalid x-api-key", Code::BadKey),
            ("OpenAI API error 403 Forbidden: country not supported", Code::BadKey),
            ("OpenAI API error 400 Bad Request: context_length_exceeded", Code::BadRequest),
            ("Claude API error 413 Payload Too Large: ", Code::BadRequest),
            ("Gemini API error 400 Bad Request: {\"message\":\"models/x is not found for API version v1beta, or is not supported for content generation\"}", Code::NotFound),
            ("Gemini API error 400 Bad Request: {\"message\":\"Request contains an invalid argument\"}", Code::BadRequest),
            ("Gemini (gemini-3.7-flash) returned no text (finish_reason=STOP)", Code::EmptyReply),
            ("error sending request for url (https://api.openai.com/v1/chat/completions)", Code::Unreachable),
            // complete.rs's own messages.
            ("Gemini 拒絕了 API 密鑰（401），請檢查後重試。", Code::BadKey),
            ("Kimi 暫時限速或額度不足（429），請稍後重試。", Code::RateLimited),
            ("Qwen 找不到所選模型或網址（404）。", Code::NotFound),
            ("尚未設定 OpenAI API 密鑰。", Code::NoKey),
            ("未能連接 Ollama，請檢查網絡。", Code::Unreachable),
            ("AWS credentials are not set up. Add them in Settings and try again.", Code::NoKey),
            ("JARVIS could not read your saved AWS credentials. Save them again in Settings and try again.", Code::NoKey),
            ("JARVIS 未能讀取已儲存的 AWS 憑證，請在設定中重新儲存後重試。", Code::NoKey),
            ("Bedrock refused the AWS credentials or access to the model. Check them in Settings and try again.", Code::BadKey),
            ("Bedrock 拒絕了 AWS 憑證或模型存取權限，請在設定中檢查後重試。", Code::BadKey),
            ("Bedrock could not find the chosen model. Check the model ID and the region.", Code::NotFound),
            ("Bedrock is limiting requests or your quota is used up. Try again later.", Code::RateLimited),
            ("Bedrock 不接受這個請求，請檢查所選模型及地區。", Code::BadRequest),
            ("Bedrock 暫時未能回覆，請稍後重試。", Code::ServerError),
            ("Could not reach AWS Bedrock. Check your network and your AWS credentials.", Code::Unreachable),
            ("Bedrock (m) returned no answer. Try again, or choose another model.", Code::EmptyReply),
            ("Bedrock converse: service error: ThrottlingException: Too many requests", Code::RateLimited),
            ("Bedrock converse: service error: AccessDeniedException: no access", Code::BadKey),
            ("Bedrock converse: service error: ValidationException: bad field", Code::BadRequest),
            ("Kimi 拒絕了 API 密鑰，請檢查是否完整複製。", Code::BadKey),
            ("Kimi 不接受這個請求（400），請檢查所選模型。", Code::BadRequest),
            ("Qwen 暫時未能回覆（502），請稍後重試。", Code::ServerError),
        ];
        for (error, code) in cases {
            assert_eq!(code_of(error), code, "{error}");
        }
        assert!(Code::RateLimited.transient() && Code::EmptyReply.transient() && !Code::BadKey.transient() && !Code::BadRequest.transient());
    }

    /// The card names the model and the reason in the app's language, and
    /// never shows the provider's body.
    #[test]
    fn the_reason_names_the_model_and_never_the_body() {
        let error = "Gemini API error 400 Bad Request: {\"error\":\"PRIVATE-MAIL-CANARY\"}";
        let failure = Failure::failed("chat", target("Gemini 3.7 Flash (Google Gemini)"), error, 0, vec![], UiLanguage::En);
        assert_eq!(failure.message, "Gemini 3.7 Flash (Google Gemini) did not accept this request (400). Check the chosen model, or pick another one.");
        assert!(!failure.to_error().contains("CANARY"));
        let zh = Failure::failed("mail_analysis", target("Gemini 3.7 Flash (Google Gemini)"), "Gemini API error 429: x", 0, vec![], UiLanguage::ZhHk);
        assert_eq!(zh.message, "Gemini 3.7 Flash (Google Gemini) 暫時限速或額度不足（429），請稍後重試，或改用其他模型。");
        let parsed = parse(&zh.to_error()).unwrap();
        assert_eq!((parsed["feature"].as_str(), parsed["code"].as_str()), (Some("mail_analysis"), Some("rate_limited")));
    }

    /// A refused request (a Think harder turn Google does not take, a model
    /// id it does not know) shows the provider's own reason: its
    /// `error.message`, on one line and capped, and nothing else of the body.
    #[test]
    fn the_card_shows_the_providers_own_reason_for_a_refused_request() {
        let google = "Gemini API error 400 Bad Request: {\"error\":{\"code\":400,\"message\":\"Thinking level is not supported\\nfor this model.\",\"status\":\"INVALID_ARGUMENT\",\"details\":[{\"echo\":\"PRIVATE-MAIL-CANARY\"}]}}";
        let failure = Failure::failed("premium", target("Gemini 2.5 Flash (Google Gemini)"), google, 0, vec![], UiLanguage::En);
        assert_eq!(failure.provider_reason.as_deref(), Some("Thinking level is not supported for this model."));
        assert!(!failure.to_error().contains("CANARY"));
        let anthropic = "Claude API error 404 Not Found: {\"type\":\"error\",\"error\":{\"type\":\"not_found_error\",\"message\":\"model: claude-x\"}}";
        assert_eq!(Failure::failed("chat", target("Claude"), anthropic, 0, vec![], UiLanguage::En).provider_reason.as_deref(), Some("model: claude-x"));
        let long = format!("OpenAI API error 400: {{\"error\":{{\"message\":\"{}\"}}}}", "x".repeat(500));
        assert_eq!(Failure::failed("chat", target("GPT"), &long, 0, vec![], UiLanguage::En).provider_reason.map(|r| r.chars().count()), Some(301));
        // No message field, a server error, or a rate limit: nothing is quoted.
        assert_eq!(Failure::failed("chat", target("Gemini"), "Gemini API error 400 Bad Request: {\"error\":\"PRIVATE-MAIL-CANARY\"}", 0, vec![], UiLanguage::En).provider_reason, None);
        assert_eq!(Failure::failed("chat", target("Gemini"), "Gemini API error 503: {\"error\":{\"message\":\"overloaded\"}}", 0, vec![], UiLanguage::En).provider_reason, None);
    }

    /// After an action ran, the card offers nothing: no model, not even the
    /// same one, may run the request again.
    #[test]
    fn after_an_action_ran_no_model_is_offered() {
        let other = Alternative { pick: Pick { provider_type: "openai".into(), base_url: None, endpoint_id: None, model_id: None }, label: "GPT-6 Luna (OpenAI (GPT))".into(), saves_at: None };
        let same = Alternative { pick: target("x").pick, label: "x".into(), saves_at: None };
        let before = Failure::failed("chat", target("Gemini"), "Gemini API error 503: x", 0, vec![same.clone(), other.clone()], UiLanguage::En);
        assert_eq!(before.alternatives, vec![other.clone()], "the failed model is not offered");
        let after = Failure::failed("chat", target("Gemini"), "Gemini API error 503: x", 1, vec![other], UiLanguage::En);
        assert!(after.alternatives.is_empty());
        assert_eq!(after.actions_ran, 1);
    }

    /// A run that failed after an action ran cannot be sent again.
    #[test]
    fn a_run_that_acted_may_not_be_retried() {
        let (acted, clean) = (format!("run-{}", uuid::Uuid::new_v4()), format!("run-{}", uuid::Uuid::new_v4()));
        note_acted(&acted);
        assert!(!may_retry(&acted));
        assert!(may_retry(&clean));
    }

    /// A background job does not prompt: it records why, and raises its
    /// notice once per launch. A model that cannot run the job is recorded
    /// as "needs a model that supports" it.
    #[test]
    fn a_background_job_records_what_it_needs_and_does_not_prompt() {
        let unsupported = crate::ai::capability::Unsupported { target: target(&format!("Model {}", uuid::Uuid::new_v4())), reason: Reason::Missing { capability: "json".into() }, alternatives: vec![] };
        let failure = Failure::unsupported("mail_sort", unsupported, UiLanguage::En);
        let key = format!("{}|{}|{:?}|{:?}", failure.feature, failure.model.label, failure.code, failure.capability);
        let recorded = failure.clone().surface(false, UiLanguage::En);
        assert_eq!(recorded, format!("Needs a model that supports mail sorting: {} does not.", failure.model.label));
        assert!(parse(&recorded).is_none(), "no card for a background job");
        assert!(!super::super::notify::first_time(&key), "its notice was raised once");
        let zh = failure.clone().surface(false, UiLanguage::ZhHk);
        assert_eq!(zh, format!("需要支援自動整理的模型：{} 不支援。", failure.model.label));
        assert!(parse(&failure.surface(true, UiLanguage::En)).is_some(), "a foreground request gets the card");

        let limited = Failure::failed("mail_sort", target("Gemini 3.7 Flash (Google Gemini)"), "Gemini API error 429: x", 0, vec![], UiLanguage::En);
        assert_eq!(limited.surface(false, UiLanguage::En), "Gemini 3.7 Flash (Google Gemini) is limiting requests or is out of quota (429). Try again shortly, or pick another model.");
    }
}
