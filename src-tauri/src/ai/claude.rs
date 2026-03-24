use reqwest::Client;
use serde::{Deserialize, Serialize};
use super::tools;

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    system: String,
    tools: Vec<serde_json::Value>,
    messages: Vec<ClaudeMessage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ClaudeMessage {
    role: String,
    content: ClaudeContent,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
enum ClaudeContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: serde_json::Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String },
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ResponseBlock>,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ResponseBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: serde_json::Value },
}

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let tool_defs = tools::get_tool_definitions();
    let claude_tools = tools::to_claude_format(&tool_defs);

    let mut claude_messages: Vec<ClaudeMessage> = messages.into_iter().map(|(role, content)| {
        ClaudeMessage { role, content: ClaudeContent::Text(content) }
    }).collect();

    let max_iterations = 5;
    for _ in 0..max_iterations {
        let request = ClaudeRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 1024,
            system: tools::SYSTEM_PROMPT.into(),
            tools: claude_tools.clone(),
            messages: claude_messages.clone(),
        };

        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(30))
            .send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Claude API error {}: {}", status, body).into());
        }

        let body: ClaudeResponse = response.json().await?;

        // Collect text and tool calls from response
        let mut text_parts = Vec::new();
        let mut tool_uses = Vec::new();

        for block in &body.content {
            match block {
                ResponseBlock::Text { text } => text_parts.push(text.clone()),
                ResponseBlock::ToolUse { id, name, input } => {
                    tool_uses.push((id.clone(), name.clone(), input.clone()));
                }
            }
        }

        // If there are tool calls, execute them and continue
        if !tool_uses.is_empty() {
            // Add the assistant response (with tool uses) to messages
            let assistant_blocks: Vec<ContentBlock> = body.content.iter().map(|b| match b {
                ResponseBlock::Text { text } => ContentBlock::Text { text: text.clone() },
                ResponseBlock::ToolUse { id, name, input } => ContentBlock::ToolUse {
                    id: id.clone(), name: name.clone(), input: input.clone(),
                },
            }).collect();
            claude_messages.push(ClaudeMessage {
                role: "assistant".into(),
                content: ClaudeContent::Blocks(assistant_blocks),
            });

            // Execute tools and add results
            let mut result_blocks = Vec::new();
            for (id, name, input) in &tool_uses {
                let args_str = serde_json::to_string(input).unwrap_or_default();
                log::info!("JARVIS tool call: {}({})", name, args_str);
                let result = tools::execute_tool(name, &args_str).await;
                log::info!("JARVIS tool result: {}", &result[..result.len().min(200)]);
                result_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: result,
                });
            }
            claude_messages.push(ClaudeMessage {
                role: "user".into(),
                content: ClaudeContent::Blocks(result_blocks),
            });

            // If stop_reason is "end_turn", we're done even with tool calls
            if body.stop_reason.as_deref() == Some("end_turn") && !text_parts.is_empty() {
                return Ok(text_parts.join("\n"));
            }
            continue;
        }

        // No tool calls -- return text
        if !text_parts.is_empty() {
            return Ok(text_parts.join("\n"));
        }
        return Ok(String::new());
    }

    Ok("I've completed the actions.".into())
}
