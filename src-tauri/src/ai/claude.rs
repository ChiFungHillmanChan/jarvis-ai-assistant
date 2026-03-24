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

const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant running on macOS for user Hillman Chan (GitHub: ChiFungHillmanChan). Respond concisely. Be direct, not chatty.\n\nYou can create tasks and control the computer. Put each action tag on its OWN LINE.\n\nAction tags:\n[TASK:title|description|deadline(YYYY-MM-DD)|priority(0-3)]\n[OPEN_APP:ExactAppName]\n[OPEN_URL:https://full-url-here]\n[RUN_CMD:shell command]\n[FIND_FILE:filename]\n[OPEN_FILE:/path/to/file]\n[NOTE:~/path/to/file.md|content]\n[SYSTEM_INFO]\n\nIMPORTANT RULES:\n1. Always construct FULL URLs. Never just open a homepage when the user wants something specific.\n   - 'go to github and find evoke-square' -> [OPEN_URL:https://github.com/ChiFungHillmanChan/evoke-square]\n   - 'search google for rust tutorials' -> [OPEN_URL:https://www.google.com/search?q=rust+tutorials]\n   - 'find my repo jarvis' -> [OPEN_URL:https://github.com/ChiFungHillmanChan/jarvis-ai-assistant]\n   - 'go to youtube and search lofi' -> [OPEN_URL:https://www.youtube.com/results?search_query=lofi]\n2. Use exact macOS app names: 'Google Chrome' not 'Chrome', 'Visual Studio Code' not 'VS Code'\n3. For file operations use full paths starting with ~ or /\n4. You can chain multiple actions. Each on its own line.\n5. For GitHub repos, the user's username is ChiFungHillmanChan\n6. For web searches, construct the search URL directly\n\nCommon macOS apps: Google Chrome, Safari, Firefox, Visual Studio Code, Finder, Terminal, Spotify, Slack, Discord, Notion, Obsidian\n\nExample:\n[OPEN_APP:Google Chrome]\n[OPEN_URL:https://github.com/ChiFungHillmanChan/evoke-square]\nOpened Chrome and navigated to the evoke-square repository.";

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
