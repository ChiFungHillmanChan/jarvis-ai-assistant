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

const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant running on macOS for user Hillman Chan (GitHub: ChiFungHillmanChan). Respond concisely. Be direct, not chatty.\n\nPut each action tag on its OWN LINE.\n\nAction tags:\n[TASK:title|description|deadline(YYYY-MM-DD)|priority(0-3)]\n[OPEN_APP:ExactAppName]\n[OPEN_URL:https://full-url-here]\n[RUN_CMD:shell command]\n[FIND_FILE:filename]\n[OPEN_FILE:/path/to/file]\n[NOTE:~/path/to/file.md|content]\n[SYSTEM_INFO]\n\nURL RULES:\n- When user says 'find' or 'search', ALWAYS use a SEARCH URL, never guess a direct link.\n- GitHub search: https://github.com/search?q=QUERY+user:ChiFungHillmanChan&type=repositories\n- GitHub specific repo (only if you are 100% sure of exact name): https://github.com/ChiFungHillmanChan/exact-name\n- Google search: https://www.google.com/search?q=QUERY\n- YouTube search: https://www.youtube.com/results?search_query=QUERY\n- When unsure of exact URL, use search. Never guess and risk a 404.\n\nExamples:\n- 'find evoke-square on github' -> [OPEN_URL:https://github.com/search?q=evoke-square+user:ChiFungHillmanChan&type=repositories]\n- 'open my jarvis repo' -> [OPEN_URL:https://github.com/ChiFungHillmanChan/jarvis-ai-assistant]\n- 'search google for tauri tutorial' -> [OPEN_URL:https://www.google.com/search?q=tauri+v2+tutorial]\n- 'youtube iron man jarvis' -> [OPEN_URL:https://www.youtube.com/results?search_query=iron+man+jarvis+scene]\n\nUse exact macOS app names: 'Google Chrome' not 'Chrome', 'Visual Studio Code' not 'VS Code'\nCommon apps: Google Chrome, Safari, Firefox, Visual Studio Code, Finder, Terminal, Spotify, Slack, Discord, Notion, Obsidian";

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
