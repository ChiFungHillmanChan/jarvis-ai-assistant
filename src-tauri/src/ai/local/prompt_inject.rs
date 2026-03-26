use super::backend::ToolCapability;
use crate::ai::tools::Tool;
use serde::Deserialize;

/// Tool categories for intelligent filtering in prompt-injected mode
const TOOL_CATEGORIES: &[(&str, &[&str])] = &[
    (
        "system",
        &[
            "open_app",
            "open_url",
            "run_command",
            "find_files",
            "open_file",
            "write_note",
            "system_info",
            "clipboard_read",
            "clipboard_write",
            "screenshot",
            "manage_window",
            "system_controls",
            "send_notification",
            "list_processes",
            "kill_process",
            "read_file",
        ],
    ),
    (
        "email",
        &["search_emails", "read_email", "send_email", "archive_email"],
    ),
    (
        "calendar",
        &["list_events", "create_event", "update_event", "delete_event"],
    ),
    (
        "github",
        &["list_github_items", "create_github_issue"],
    ),
    (
        "notion",
        &["search_notion", "read_notion_page", "create_notion_page"],
    ),
    (
        "obsidian",
        &["search_notes", "read_note"],
    ),
    (
        "tasks",
        &["create_task"],
    ),
    (
        "render",
        &["render_chart", "render_status"],
    ),
];

/// Category keywords for matching user messages to relevant tool categories
const CATEGORY_KEYWORDS: &[(&str, &[&str])] = &[
    ("email", &["email", "mail", "inbox", "send", "message", "gmail"]),
    ("calendar", &["calendar", "meeting", "event", "schedule", "appointment", "busy", "free"]),
    ("github", &["github", "pr", "pull request", "issue", "repo", "commit", "branch"]),
    ("notion", &["notion", "page", "database", "wiki", "doc"]),
    ("obsidian", &["obsidian", "notes", "vault", "note"]),
    ("tasks", &["task", "todo", "remind", "deadline", "priority"]),
    ("render", &["chart", "graph", "status", "visualize", "data", "stats"]),
    ("system", &["open", "launch", "run", "file", "find", "clipboard", "screenshot", "window", "volume", "notification", "process", "kill"]),
];

/// Select relevant tools based on user message content (for prompt-injected mode)
pub fn select_relevant_tools(
    all_tools: &[Tool],
    user_message: &str,
    max_tools: usize,
) -> Vec<Tool> {
    if max_tools == 0 {
        return vec![];
    }
    if max_tools >= all_tools.len() {
        return all_tools.to_vec();
    }

    let msg_lower = user_message.to_lowercase();
    let mut relevant_tool_names: Vec<&str> = Vec::new();

    // Find matching categories
    for (category, keywords) in CATEGORY_KEYWORDS {
        if keywords.iter().any(|kw| msg_lower.contains(kw)) {
            // Add all tools from this category
            for (cat, tools) in TOOL_CATEGORIES {
                if cat == category {
                    relevant_tool_names.extend_from_slice(tools);
                }
            }
        }
    }

    // Always include create_task as a generally useful tool
    if !relevant_tool_names.contains(&"create_task") {
        relevant_tool_names.push("create_task");
    }

    // Deduplicate
    relevant_tool_names.sort();
    relevant_tool_names.dedup();

    // If we found too few relevant tools, add system tools as a baseline
    if relevant_tool_names.len() < 3 {
        for (cat, tools) in TOOL_CATEGORIES {
            if *cat == "system" {
                for t in tools.iter().take(5) {
                    if !relevant_tool_names.contains(t) {
                        relevant_tool_names.push(t);
                    }
                }
            }
        }
    }

    // Truncate to max
    relevant_tool_names.truncate(max_tools);

    all_tools
        .iter()
        .filter(|t| relevant_tool_names.contains(&t.name.as_str()))
        .cloned()
        .collect()
}

/// Build the tool injection prompt for models without native tool calling
pub fn build_tool_injection_prompt(tools: &[Tool]) -> String {
    if tools.is_empty() {
        return String::new();
    }

    let mut prompt = String::from(
        "\n\nYou have access to tools. To call a tool, respond with valid JSON:\n\
         {\"tool_calls\": [{\"name\": \"tool_name\", \"arguments\": {...}}]}\n\n\
         To respond without tools, use:\n\
         {\"response\": \"your message here\"}\n\n\
         You may also respond with plain text if you have nothing to call.\n\n\
         Available tools:\n",
    );

    for tool in tools {
        prompt.push_str(&format!(
            "- {}: {} | params: {}\n",
            tool.name,
            tool.description,
            serde_json::to_string(&tool.parameters).unwrap_or_default()
        ));
    }

    prompt
}

#[derive(Deserialize)]
struct ToolCallResponse {
    tool_calls: Option<Vec<ParsedToolCall>>,
    response: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ParsedToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Parse tool calls from a prompt-injected model response
pub fn parse_tool_calls_from_response(response: &str) -> (Option<Vec<ParsedToolCall>>, String) {
    let trimmed = response.trim();

    // Try parsing the entire response as JSON first
    if let Ok(parsed) = serde_json::from_str::<ToolCallResponse>(trimmed) {
        if let Some(calls) = parsed.tool_calls {
            if !calls.is_empty() {
                return (Some(calls), String::new());
            }
        }
        if let Some(text) = parsed.response {
            return (None, text);
        }
    }

    // Try to find JSON block within markdown code fences
    if let Some(start) = trimmed.find("```json") {
        if let Some(end) = trimmed[start + 7..].find("```") {
            let json_str = &trimmed[start + 7..start + 7 + end].trim();
            if let Ok(parsed) = serde_json::from_str::<ToolCallResponse>(json_str) {
                if let Some(calls) = parsed.tool_calls {
                    if !calls.is_empty() {
                        return (Some(calls), String::new());
                    }
                }
                if let Some(text) = parsed.response {
                    return (None, text);
                }
            }
        }
    }

    // Try to find a JSON object with tool_calls in the response
    if let Some(start) = trimmed.find("{\"tool_calls\"") {
        // Find matching closing brace
        let mut depth = 0;
        let mut end_pos = start;
        for (i, ch) in trimmed[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end_pos > start {
            let json_str = &trimmed[start..end_pos];
            if let Ok(parsed) = serde_json::from_str::<ToolCallResponse>(json_str) {
                if let Some(calls) = parsed.tool_calls {
                    if !calls.is_empty() {
                        // Return any text before the JSON as the response text
                        let prefix = trimmed[..start].trim().to_string();
                        return (Some(calls), prefix);
                    }
                }
            }
        }
    }

    // No tool calls found -- return as plain text
    (None, trimmed.to_string())
}

/// Determine effective tool capability based on context length and override
pub fn effective_tool_capability(
    detected: &ToolCapability,
    override_setting: Option<&str>,
    context_length: u32,
) -> ToolCapability {
    // User override takes precedence
    if let Some(ov) = override_setting {
        if ov != "auto" {
            return ToolCapability::from_str(ov);
        }
    }

    // Context too small for any tools
    if context_length < 4096 {
        return ToolCapability::ChatOnly;
    }

    detected.clone()
}
