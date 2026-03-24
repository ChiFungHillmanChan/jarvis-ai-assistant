use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: u32,
}

#[derive(Serialize, Deserialize, Clone)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant running on macOS. Respond concisely in a technical assistant tone. Be direct, not chatty.\n\nYou can create tasks and control the computer. Put each action tag on its OWN LINE with nothing else on that line.\n\nAction tags (one per line, no extra text on same line):\n[TASK:title|description|deadline(YYYY-MM-DD)|priority(0-3)]\n[OPEN_APP:ExactAppName] -- use exact macOS names: 'Google Chrome' not 'Chrome', 'Visual Studio Code' not 'VS Code'\n[OPEN_URL:https://full-url-here]\n[RUN_CMD:shell command]\n[FIND_FILE:filename]\n[OPEN_FILE:/path/to/file]\n[NOTE:~/path/to/file.md|content]\n[SYSTEM_INFO]\n\nCommon macOS app names: Google Chrome, Safari, Firefox, Visual Studio Code, Finder, Terminal, Spotify, Slack, Discord, Notion, Obsidian, Preview, TextEdit, Calculator, System Settings\n\nAfter action tags, add a short confirmation on the next line. Example:\n[OPEN_APP:Google Chrome]\n[OPEN_URL:https://www.youtube.com]\nOpening Chrome and navigating to YouTube.";

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let mut openai_messages: Vec<OpenAIMessage> = vec![
        OpenAIMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
    ];
    openai_messages.extend(
        messages.into_iter().map(|(role, content)| OpenAIMessage { role, content }),
    );
    let request = OpenAIRequest {
        model: "gpt-4o-mini".to_string(),
        messages: openai_messages,
        max_tokens: 1024,
    };
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("OpenAI API error {}: {}", status, body).into());
    }
    let body: OpenAIResponse = response.json().await?;
    Ok(body.choices.first().map(|c| c.message.content.clone()).unwrap_or_default())
}
