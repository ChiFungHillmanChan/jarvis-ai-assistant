use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

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

const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant on macOS for Hillman Chan (GitHub: ChiFungHillmanChan). Be concise and direct like the JARVIS from Iron Man. You have tools to control the computer. Use them when the user asks you to do something. You can call multiple tools in sequence to accomplish complex tasks. Think step by step -- if you need to find something first before opening it, do the search first, then use the result.";

fn get_tools() -> Vec<serde_json::Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "open_app",
                "description": "Open a macOS application. Use exact app names: 'Google Chrome', 'Visual Studio Code', 'Finder', 'Terminal', 'Safari', 'Spotify', 'Slack', 'Discord', 'Notion', 'Obsidian'",
                "parameters": {
                    "type": "object",
                    "properties": { "name": { "type": "string", "description": "Exact macOS application name" } },
                    "required": ["name"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "open_url",
                "description": "Open a URL in the default browser. Construct full URLs with search queries when needed. Examples: https://github.com/search?q=query, https://www.google.com/search?q=query, https://www.youtube.com/results?search_query=query",
                "parameters": {
                    "type": "object",
                    "properties": { "url": { "type": "string", "description": "Full URL to open" } },
                    "required": ["url"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "Run a shell command on macOS and return the output. Use for system info, file operations, git commands, etc. Do NOT run destructive commands (rm, sudo).",
                "parameters": {
                    "type": "object",
                    "properties": { "command": { "type": "string", "description": "Shell command to execute" } },
                    "required": ["command"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "find_files",
                "description": "Search for files on the computer using macOS Spotlight (mdfind)",
                "parameters": {
                    "type": "object",
                    "properties": { "query": { "type": "string", "description": "Filename or content to search for" } },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "open_file",
                "description": "Open a file with its default application",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "Full file path" } },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_task",
                "description": "Create a task/reminder in JARVIS task list",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "description": { "type": "string" },
                        "deadline": { "type": "string", "description": "YYYY-MM-DD format" },
                        "priority": { "type": "integer", "description": "0=low, 1=normal, 2=high, 3=urgent" }
                    },
                    "required": ["title"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "write_note",
                "description": "Write or append to a text file (for notes, logs, etc)",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path (use ~ for home)" },
                        "content": { "type": "string" },
                        "append": { "type": "boolean", "description": "true to append, false to overwrite" }
                    },
                    "required": ["path", "content"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "system_info",
                "description": "Get system information: hostname, uptime, disk usage, memory",
                "parameters": { "type": "object", "properties": {} }
            }
        }),
    ]
}

/// Execute a tool call and return the result
async fn execute_tool(name: &str, args: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args).unwrap_or_default();

    match name {
        "open_app" => {
            let app = args["name"].as_str().unwrap_or("");
            crate::system::control::open_app(app)
                .await
                .unwrap_or_else(|e| format!("Error: {}", e))
        }
        "open_url" => {
            let url = args["url"].as_str().unwrap_or("");
            crate::system::control::open_url(url)
                .await
                .unwrap_or_else(|e| format!("Error: {}", e))
        }
        "run_command" => {
            let cmd = args["command"].as_str().unwrap_or("");
            crate::system::control::run_command(cmd)
                .await
                .unwrap_or_else(|e| format!("Error: {}", e))
        }
        "find_files" => {
            let query = args["query"].as_str().unwrap_or("");
            match crate::system::control::find_files(query, None).await {
                Ok(files) => {
                    if files.is_empty() {
                        "No files found.".to_string()
                    } else {
                        files.join("\n")
                    }
                }
                Err(e) => format!("Error: {}", e),
            }
        }
        "open_file" => {
            let path = args["path"].as_str().unwrap_or("");
            crate::system::control::open_file(path)
                .await
                .unwrap_or_else(|e| format!("Error: {}", e))
        }
        "create_task" => {
            let title = args["title"].as_str().unwrap_or("Untitled");
            let desc = args["description"].as_str();
            let deadline = args["deadline"].as_str();
            let priority = args["priority"].as_i64().unwrap_or(1) as i32;
            let sql = format!(
                "INSERT INTO tasks (title, description, deadline, priority) VALUES ('{}', '{}', {}, {})",
                title.replace('\'', "''"),
                desc.unwrap_or("").replace('\'', "''"),
                deadline
                    .map(|d| format!("'{}'", d))
                    .unwrap_or("NULL".to_string()),
                priority
            );
            match crate::system::control::run_command(&format!(
                "sqlite3 ~/Library/Application\\ Support/jarvis/jarvis.db \"{}\"",
                sql
            ))
            .await
            {
                Ok(_) => format!("Task created: {}", title),
                Err(e) => format!("Failed to create task: {}", e),
            }
        }
        "write_note" => {
            let path = args["path"].as_str().unwrap_or("~/jarvis-notes.md");
            let content = args["content"].as_str().unwrap_or("");
            let append = args["append"].as_bool().unwrap_or(true);
            crate::system::control::write_note(path, content, append)
                .await
                .unwrap_or_else(|e| format!("Error: {}", e))
        }
        "system_info" => crate::system::control::system_info()
            .await
            .unwrap_or_else(|e| format!("Error: {}", e)),
        _ => format!("Unknown tool: {}", name),
    }
}

pub async fn send(
    api_key: &str,
    messages: Vec<(String, String)>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();

    let mut openai_messages: Vec<OpenAIMessage> = vec![OpenAIMessage {
        role: "system".to_string(),
        content: Some(SYSTEM_PROMPT.to_string()),
        tool_calls: None,
        tool_call_id: None,
    }];
    openai_messages.extend(messages.into_iter().map(|(role, content)| OpenAIMessage {
        role,
        content: Some(content),
        tool_calls: None,
        tool_call_id: None,
    }));

    // Loop: send -> check for tool calls -> execute tools -> send results back -> repeat
    let max_iterations = 5;
    for _ in 0..max_iterations {
        let request = OpenAIRequest {
            model: "gpt-4o-mini".to_string(),
            messages: openai_messages.clone(),
            tools: get_tools(),
            max_tokens: 1024,
        };

        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("OpenAI API error {}: {}", status, body).into());
        }

        let body: OpenAIResponse = response.json().await?;
        let choice = body.choices.first().ok_or("No response from OpenAI")?;

        // If the AI wants to call tools
        if let Some(tool_calls) = &choice.message.tool_calls {
            if !tool_calls.is_empty() {
                // Add the assistant message with tool calls
                openai_messages.push(choice.message.clone());

                // Execute each tool and add results
                for tc in tool_calls {
                    log::info!(
                        "JARVIS tool call: {}({})",
                        tc.function.name,
                        tc.function.arguments
                    );
                    let result =
                        execute_tool(&tc.function.name, &tc.function.arguments).await;
                    log::info!(
                        "JARVIS tool result: {}",
                        &result[..result.len().min(200)]
                    );

                    openai_messages.push(OpenAIMessage {
                        role: "tool".to_string(),
                        content: Some(result),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                    });
                }
                // Continue the loop -- AI will see the tool results and decide next action
                continue;
            }
        }

        // No tool calls -- return the text response
        let text = choice.message.content.clone().unwrap_or_default();
        return Ok(text);
    }

    Ok("I've completed the actions.".to_string())
}
