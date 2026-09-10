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
) -> Result<String, String> {
    let url = match endpoint {
        Endpoint::Production => RESPONSES_URL.to_string(),
        #[cfg(test)]
        Endpoint::Loopback(port) => format!("http://127.0.0.1:{port}/v1/responses"),
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "未能建立 OpenAI 連線。".to_string())?;
    let response = client
        .post(url)
        .bearer_auth(key)
        .json(&json!({
            "model": model,
            "input": prompt,
            "store": false,
            "stream": false,
            "max_output_tokens": 4096,
            "reasoning": {"effort": "low"},
            "truncation": "disabled"
        }))
        .send()
        .await
        .map_err(|_| "未能連接 OpenAI，請檢查網絡後再試。".to_string())?;
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
        return Err("OpenAI 未有文字回覆，請換一個模型或者重試。".into());
    }
    Ok(text)
}
