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

const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant. Respond concisely in a technical assistant tone. Be direct, not chatty. Example good response: 'Standup rescheduled to 10:00. Calendar updated.' Example bad response: 'Sure! I have gone ahead and moved your standup meeting for you!' If the user asks to create a task, reminder, or todo, include on a new line: [TASK:title|description|deadline(YYYY-MM-DD)|priority(0-3)]. Then confirm the creation.";

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
        model: "gpt-4o".to_string(),
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
