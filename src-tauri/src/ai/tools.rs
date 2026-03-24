use serde_json::json;

/// Shared tool definitions used by both OpenAI and Claude APIs
pub fn get_tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "open_app".into(),
            description: "Open a macOS application. Use exact app names: 'Google Chrome', 'Visual Studio Code', 'Finder', 'Terminal', 'Safari', 'Spotify', 'Slack', 'Discord', 'Notion', 'Obsidian'".into(),
            parameters: json!({
                "type": "object",
                "properties": { "name": { "type": "string", "description": "Exact macOS application name" } },
                "required": ["name"]
            }),
        },
        Tool {
            name: "open_url".into(),
            description: "Open a URL in the default browser. Construct full URLs with search queries when needed.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "url": { "type": "string", "description": "Full URL to open" } },
                "required": ["url"]
            }),
        },
        Tool {
            name: "run_command".into(),
            description: "Run a shell command on macOS and return the output. Use for system info, file operations, git commands, etc. Do NOT run destructive commands.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "command": { "type": "string", "description": "Shell command to execute" } },
                "required": ["command"]
            }),
        },
        Tool {
            name: "find_files".into(),
            description: "Search for files on the computer using macOS Spotlight".into(),
            parameters: json!({
                "type": "object",
                "properties": { "query": { "type": "string", "description": "Filename or content to search for" } },
                "required": ["query"]
            }),
        },
        Tool {
            name: "open_file".into(),
            description: "Open a file with its default application".into(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Full file path" } },
                "required": ["path"]
            }),
        },
        Tool {
            name: "create_task".into(),
            description: "Create a task/reminder in JARVIS task list".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "deadline": { "type": "string", "description": "YYYY-MM-DD format" },
                    "priority": { "type": "integer", "description": "0=low, 1=normal, 2=high, 3=urgent" }
                },
                "required": ["title"]
            }),
        },
        Tool {
            name: "write_note".into(),
            description: "Write or append to a text file (for notes, logs, etc)".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path (use ~ for home)" },
                    "content": { "type": "string" },
                    "append": { "type": "boolean", "description": "true to append, false to overwrite" }
                },
                "required": ["path", "content"]
            }),
        },
        Tool {
            name: "system_info".into(),
            description: "Get system information: hostname, uptime, disk usage, memory".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
    ]
}

pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Format tools for OpenAI API format
pub fn to_openai_format(tools: &[Tool]) -> Vec<serde_json::Value> {
    tools.iter().map(|t| json!({
        "type": "function",
        "function": {
            "name": t.name,
            "description": t.description,
            "parameters": t.parameters,
        }
    })).collect()
}

/// Format tools for Claude API format
pub fn to_claude_format(tools: &[Tool]) -> Vec<serde_json::Value> {
    tools.iter().map(|t| json!({
        "name": t.name,
        "description": t.description,
        "input_schema": t.parameters,
    })).collect()
}

/// Execute a tool by name with JSON arguments. Shared by both APIs.
pub async fn execute_tool(name: &str, args_str: &str) -> String {
    let args: serde_json::Value = serde_json::from_str(args_str).unwrap_or_default();

    match name {
        "open_app" => {
            let app = args["name"].as_str().unwrap_or("");
            crate::system::control::open_app(app).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "open_url" => {
            let url = args["url"].as_str().unwrap_or("");
            crate::system::control::open_url(url).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "run_command" => {
            let cmd = args["command"].as_str().unwrap_or("");
            crate::system::control::run_command(cmd).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "find_files" => {
            let query = args["query"].as_str().unwrap_or("");
            match crate::system::control::find_files(query, None).await {
                Ok(files) => if files.is_empty() { "No files found.".into() } else { files.join("\n") },
                Err(e) => format!("Error: {}", e),
            }
        }
        "open_file" => {
            let path = args["path"].as_str().unwrap_or("");
            crate::system::control::open_file(path).await.unwrap_or_else(|e| format!("Error: {}", e))
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
                deadline.map(|d| format!("'{}'", d)).unwrap_or_else(|| "NULL".to_string()),
                priority
            );
            match crate::system::control::run_command(&format!(
                "sqlite3 ~/Library/Application\\ Support/jarvis/jarvis.db \"{}\"", sql
            )).await {
                Ok(_) => format!("Task created: {}", title),
                Err(e) => format!("Failed to create task: {}", e),
            }
        }
        "write_note" => {
            let path = args["path"].as_str().unwrap_or("~/jarvis-notes.md");
            let content = args["content"].as_str().unwrap_or("");
            let append = args["append"].as_bool().unwrap_or(true);
            crate::system::control::write_note(path, content, append).await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        "system_info" => {
            crate::system::control::system_info().await.unwrap_or_else(|e| format!("Error: {}", e))
        }
        _ => format!("Unknown tool: {}", name),
    }
}

pub const SYSTEM_PROMPT: &str = "You are JARVIS, a personal AI assistant on macOS for Hillman Chan (GitHub: ChiFungHillmanChan). Be concise and direct like the JARVIS from Iron Man. You have tools to control the computer. Use them when the user asks you to do something. You can call multiple tools in sequence to accomplish complex tasks. Think step by step -- if you need to find something first before opening it, do the search first, then use the result.";
