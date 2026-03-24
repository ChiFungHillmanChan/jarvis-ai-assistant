use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<ClaudeMessage>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    text: String,
}

const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant. Respond concisely in a technical assistant tone. Be direct, not chatty. Example good response: 'Standup rescheduled to 10:00. Calendar updated.' Example bad response: 'Sure! I have gone ahead and moved your standup meeting for you!' If the user asks to create a task, reminder, or todo, include on a new line: [TASK:title|description|deadline(YYYY-MM-DD)|priority(0-3)]. Then confirm the creation. You can control the user's computer. When the user asks you to:\n- Open an app: respond with [OPEN_APP:AppName]\n- Open a URL: respond with [OPEN_URL:https://...]\n- Run a command: respond with [RUN_CMD:command here]\n- Find a file: respond with [FIND_FILE:filename]\n- Open a file: respond with [OPEN_FILE:/path/to/file]\n- Write a note: respond with [NOTE:/path/to/file.md|content here]\n- Get system info: respond with [SYSTEM_INFO]\nAlways confirm what you're about to do before executing. Include the action tag AND a human-readable confirmation.";

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let claude_messages: Vec<ClaudeMessage> = messages
        .into_iter()
        .map(|(role, content)| ClaudeMessage { role, content })
        .collect();
    let request = ClaudeRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        max_tokens: 1024,
        system: Some(SYSTEM_PROMPT.to_string()),
        messages: claude_messages,
    };
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Claude API error {}: {}", status, body).into());
    }
    let body: ClaudeResponse = response.json().await?;
    Ok(body.content.first().map(|c| c.text.clone()).unwrap_or_default())
}
