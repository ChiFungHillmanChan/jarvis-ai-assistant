use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

// Production callers cannot supply a host or URL. Only unit tests can replace
// the fixed HTTPS endpoint with an isolated loopback fixture.
pub(super) enum Endpoint {
    Production,
    #[cfg(test)]
    Loopback(u16),
}

#[derive(Deserialize)]
struct Response {
    status: String,
    error: Option<Value>,
    incomplete_details: Option<Value>,
    output: Vec<OutputItem>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize, Default)]
struct WireUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    input_tokens_details: Option<InputDetails>,
}

#[derive(Deserialize, Default)]
struct InputDetails {
    #[serde(default)]
    cached_tokens: i64,
}

/// Token counts OpenAI reported for one request, for cost telemetry.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum OutputItem {
    #[serde(rename = "message")]
    Message {
        role: String,
        status: String,
        content: Vec<OutputContent>,
    },
    #[serde(rename = "reasoning")]
    Reasoning,
    #[serde(other)]
    Unsupported,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum OutputContent {
    #[serde(rename = "output_text")]
    Text { text: String },
    #[serde(rename = "refusal")]
    Refusal,
    #[serde(other)]
    Unsupported,
}

pub(super) async fn send(
    key: &str,
    model: &str,
    prompt: &str,
    endpoint: Endpoint,
) -> Result<(String, Usage), String> {
    request(
        key,
        json!({
            "model": model,
            "input": prompt,
            "store": false,
            "stream": false,
            "max_output_tokens": 4096,
            "reasoning": {"effort": "low"},
            "truncation": "disabled"
        }),
        endpoint,
        120,
        0,
    )
    .await
}

/// One Responses API call whose output must match `schema` (strict JSON
/// schema). Trusted rules go in `instructions`; untrusted mail text goes in
/// `input` only, so a message cannot forge the rules around it. No tools.
pub(super) async fn send_json(
    key: &str,
    model: &str,
    instructions: &str,
    input: &str,
    schema_name: &str,
    schema: Value,
    endpoint: Endpoint,
) -> Result<(Value, Usage), String> {
    let (text, usage) = request(
        key,
        json!({
            "model": model,
            "instructions": instructions,
            "input": input,
            "store": false,
            "stream": false,
            "max_output_tokens": 2000,
            "reasoning": {"effort": "low"},
            "truncation": "disabled",
            "text": {"format": {"type": "json_schema", "name": schema_name, "strict": true, "schema": schema}}
        }),
        endpoint,
        60,
        // Triage runs several requests at once; ride out a brief rate limit.
        // Group analysis above never retries (a test holds it to one call).
        2,
    )
    .await?;
    let value = serde_json::from_str(&text).map_err(|_| "OpenAI 回覆格式無效，請重試。".to_string())?;
    Ok((value, usage))
}

async fn request(
    key: &str,
    body: Value,
    endpoint: Endpoint,
    timeout_secs: u64,
    retries: u32,
) -> Result<(String, Usage), String> {
    let url = match endpoint {
        Endpoint::Production => RESPONSES_URL.to_string(),
        #[cfg(test)]
        Endpoint::Loopback(port) => format!("http://127.0.0.1:{port}/v1/responses"),
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|_| "未能建立 OpenAI 連線。".to_string())?;
    let mut attempt = 0;
    let response = loop {
        let sent = client.post(&url).bearer_auth(key).json(&body).send().await;
        let retryable = match &sent {
            Ok(r) => matches!(r.status().as_u16(), 429 | 500 | 502 | 503),
            Err(_) => true,
        };
        if !retryable || attempt >= retries {
            break sent.map_err(|_| "未能連接 OpenAI，請檢查網絡後再試。".to_string())?;
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_millis(1_000 * 3u64.pow(attempt - 1))).await;
    };
    if !response.status().is_success() {
        // Error bodies can echo input or credential details. Never surface them.
        return Err("OpenAI 分析失敗，請檢查 API Key、模型權限及帳戶額度，或者稍後重試。".into());
    }
    let response: Response = response
        .json()
        .await
        .map_err(|_| "OpenAI 回覆格式無效，請重試。".to_string())?;
    if response.status != "completed"
        || response.error.is_some()
        || response.incomplete_details.is_some()
    {
        return Err("OpenAI 未能完成分析，請減少資料、換一個模型或者重試。".into());
    }

    // Responses may put reasoning before text and split text across several
    // messages/content blocks. Concatenate output_text in wire order only.
    let mut text = String::new();
    for item in response.output {
        match item {
            OutputItem::Message {
                role,
                status,
                content,
            } => {
                if role != "assistant" || status != "completed" {
                    return Err("OpenAI 回覆未完成或格式無效，請重試。".into());
                }
                for content in content {
                    match content {
                        OutputContent::Text { text: part } => text.push_str(&part),
                        OutputContent::Refusal => {
                            return Err("OpenAI 未能提供呢次分析，請調整資料或者重試。".into());
                        }
                        OutputContent::Unsupported => {
                            return Err("OpenAI 回覆格式無效，請重試。".into());
                        }
                    }
                }
            }
            OutputItem::Reasoning => {}
            OutputItem::Unsupported => {
                return Err("OpenAI 回覆包含未支援內容，請重試。".into());
            }
        }
    }
    if text.trim().is_empty() {
        return Err("OpenAI 沒有文字回覆，請更換模型或重試。".into());
    }
    let usage = response.usage.unwrap_or_default();
    Ok((
        text,
        Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_tokens: usage.input_tokens_details.map(|d| d.cached_tokens).unwrap_or(0),
        },
    ))
}
