use reqwest::Client;
use serde::{Deserialize, Serialize};
use super::tools;

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    tools: Vec<serde_json::Value>,
    max_tokens: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: FunctionCall,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let tool_defs = tools::get_tool_definitions();
    let openai_tools = tools::to_openai_format(&tool_defs);

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".into(), content: Some(tools::SYSTEM_PROMPT.into()),
        tool_calls: None, tool_call_id: None,
    }];
    openai_messages.extend(messages.into_iter().map(|(role, content)| OpenAIMessage {
        role, content: Some(content), tool_calls: None, tool_call_id: None,
    }));

    let max_iterations = 5;
    for _ in 0..max_iterations {
        let request = OpenAIRequest {
            model: "gpt-4o-mini".into(),
            messages: openai_messages.clone(),
            tools: openai_tools.clone(),
            max_tokens: 1024,
        };

        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(30))
            .send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("OpenAI API error {}: {}", status, body).into());
        }

        let body: OpenAIResponse = response.json().await?;
        let choice = body.choices.first().ok_or("No response from OpenAI")?;

        if let Some(tool_calls) = &choice.message.tool_calls {
            if !tool_calls.is_empty() {
                openai_messages.push(choice.message.clone());
                for tc in tool_calls {
                    log::info!("JARVIS tool call: {}({})", tc.function.name, tc.function.arguments);
                    let result = tools::execute_tool(&tc.function.name, &tc.function.arguments).await;
                    log::info!("JARVIS tool result: {}", &result[..result.len().min(200)]);
                    openai_messages.push(OpenAIMessage {
                        role: "tool".into(), content: Some(result),
                        tool_calls: None, tool_call_id: Some(tc.id.clone()),
                    });
                }
                continue;
            }
        }

        return Ok(choice.message.content.clone().unwrap_or_default());
    }

    Ok("I've completed the actions.".into())
}
