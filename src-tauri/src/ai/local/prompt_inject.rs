use super::backend::ToolCapability;
use crate::ai::tools::Tool;
use serde::Deserialize;

/// Tool categories for intelligent filtering in prompt-injected mode.
///
/// This is the second copy of the tool surface. It must name exactly what
/// `crate::ai::tools::get_tool_definitions()` returns -- no more (or the local
/// path keeps advertising a tool that was removed for security reasons) and no
/// fewer (or a live tool is unreachable here). Enforced by
/// `tests::tool_categories_match_the_live_registry`.
const TOOL_CATEGORIES: &[(&str, &[&str])] = &[
    (
        "system",
        &[
            "open_app",
            "run_command",
            "find_files",
            "open_file",
            "system_info",
            "send_notification",
        ],
    ),
    (
        "email",
        &["search_emails", "read_email", "draft_email", "archive_email", "list_inbox_triage"],
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
        &["create_task", "list_tasks", "list_follow_ups", "update_follow_up"],
    ),
    (
        "render",
        &["render_chart", "render_status"],
    ),
    (
        "memory",
        &["remember", "forget", "list_memories"],
    ),
];

/// Category keywords for matching user messages to relevant tool categories
const CATEGORY_KEYWORDS: &[(&str, &[&str])] = &[
    ("email", &["email", "mail", "inbox", "send", "message", "gmail"]),
    ("calendar", &["calendar", "meeting", "event", "schedule", "appointment", "busy", "free"]),
    ("github", &["github", "pr", "pull request", "issue", "repo", "commit", "branch"]),
    ("notion", &["notion", "page", "database", "wiki", "doc"]),
    ("obsidian", &["obsidian", "notes", "vault", "note"]),
    ("tasks", &["task", "todo", "remind", "deadline", "priority", "follow", "due"]),
    ("render", &["chart", "graph", "status", "visualize", "data", "stats"]),
    ("system", &["open", "launch", "run", "file", "find", "clipboard", "screenshot", "window", "volume", "notification", "process", "kill"]),
    ("memory", &["remember", "forget", "memory", "記住", "唔記得"]),
];

/// Tools that keep a slot when the budget truncates, in the order they are
/// rescued. Pure relevance ranking is not enough on its own: these are only ever
/// candidates when the message asked for them, and losing one there is a
/// capability disappearing while the system prompt still tells the model to call
/// it -- `remember` drops a fact the user asked to keep, `forget` leaves data
/// the user asked to delete. `create_task` is promised unconditionally by
/// `select_relevant_tools`, so it cannot be truncated away either. The cost is
/// up to four of the fifteen slots, paid only on messages that mention memory.
const RESERVED_TOOLS: &[&str] = &["remember", "forget", "list_memories", "create_task"];

/// Append `name` unless it is already present. The list is in relevance order
/// and truncation cuts from the end, so it must never be re-sorted.
fn push_unique<'a>(names: &mut Vec<&'a str>, name: &'a str) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// Move every `RESERVED_TOOLS` entry that fell outside the budget into one of
/// the last surviving slots, so truncation drops the least relevant ordinary
/// tools instead. Slots are handed out from the end of the surviving window
/// backwards and a slot already holding a reserved tool is skipped, so rescuing
/// one reserved tool never evicts another.
fn rescue_reserved_tools(names: &mut [&str], max_tools: usize) {
    let mut next_slot = max_tools.min(names.len());
    for reserved in RESERVED_TOOLS {
        let Some(pos) = names.iter().position(|n| n == reserved) else {
            continue;
        };
        if pos < max_tools {
            continue; // already survives
        }
        let target = (0..next_slot)
            .rev()
            .find(|i| !RESERVED_TOOLS.iter().any(|r| *r == names[*i]));
        let Some(target) = target else {
            break; // every surviving slot is already reserved
        };
        names.swap(pos, target);
        next_slot = target;
    }
}

/// Select relevant tools based on user message content (for prompt-injected mode)
///
/// A local model with an 8k-32k context gets `max_tools = 15` (see
/// `context.rs`), which a multi-category message overshoots easily. What
/// survives that cut therefore decides what the model can actually do, so the
/// candidate list is ordered by how strongly each category matches the message
/// and truncated from the least relevant end -- never alphabetically, which
/// used to drop whatever sorted late (`remember`, `run_command`, `search_*`)
/// while the system prompt still told the model to call it.
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

    // Relevance of a category = how much of its vocabulary the message uses,
    // weighted by keyword length because a longer keyword is a more specific
    // signal: "github" is evidence, "pr" is a coincidence, and short keywords
    // that are substrings of other categories' words ("mail" inside "email",
    // "note" inside "notion") must not outvote a whole-word match elsewhere.
    let mut matched: Vec<(&str, usize)> = CATEGORY_KEYWORDS
        .iter()
        .map(|(category, keywords)| {
            (
                *category,
                keywords
                    .iter()
                    .filter(|kw| msg_lower.contains(**kw))
                    .map(|kw| kw.chars().count())
                    .sum::<usize>(),
            )
        })
        .filter(|(_, score)| *score > 0)
        .collect();
    // Stable sort, so equally relevant categories keep CATEGORY_KEYWORDS order.
    matched.sort_by(|a, b| b.1.cmp(&a.1));

    let mut relevant_tool_names: Vec<&str> = Vec::new();
    for (category, _) in &matched {
        for (cat, tools) in TOOL_CATEGORIES {
            if cat == category {
                for t in tools.iter() {
                    push_unique(&mut relevant_tool_names, *t);
                }
            }
        }
    }

    // Always include create_task as a generally useful tool
    push_unique(&mut relevant_tool_names, "create_task");

    // If we found too few relevant tools, add system tools as a baseline
    if relevant_tool_names.len() < 3 {
        for (cat, tools) in TOOL_CATEGORIES {
            if *cat == "system" {
                for t in tools.iter().take(5) {
                    push_unique(&mut relevant_tool_names, *t);
                }
            }
        }
    }

    // Truncate to max, dropping the least relevant categories first, after the
    // reserved tools have been pulled back inside the budget: relevance alone
    // lets one strongly-matching category push `remember` off the end of a
    // sentence that plainly asked to remember something. Dropping capability is
    // never silent.
    if relevant_tool_names.len() > max_tools {
        rescue_reserved_tools(&mut relevant_tool_names, max_tools);
        log::debug!(
            "prompt-injected tool list truncated to {} of {}; dropped {:?}",
            max_tools,
            relevant_tool_names.len(),
            &relevant_tool_names[max_tools..]
        );
        relevant_tool_names.truncate(max_tools);
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// `ai/tools.rs` and this file each enumerate the tool surface, and CLAUDE.md
    /// requires them to move together. Drift in one direction leaves a tool that
    /// was removed for security reasons still advertised on the prompt-injected
    /// local path; drift in the other silently makes a live tool unreachable
    /// there. This turns that manual discipline into an enforced one.
    #[test]
    fn tool_categories_match_the_live_registry() {
        let categorised: BTreeSet<&str> = TOOL_CATEGORIES
            .iter()
            .flat_map(|(_, tools)| tools.iter().copied())
            .collect();
        let live: BTreeSet<String> = crate::ai::tools::get_tool_definitions()
            .into_iter()
            .map(|t| t.name)
            .collect();

        let uncategorised: Vec<&str> = live
            .iter()
            .map(|n| n.as_str())
            .filter(|n| !categorised.contains(n))
            .collect();
        assert!(
            uncategorised.is_empty(),
            "in get_tool_definitions() but in no TOOL_CATEGORIES entry, so unreachable on the local path: {uncategorised:?}"
        );

        let stale: Vec<&str> = categorised
            .iter()
            .copied()
            .filter(|n| !live.contains(*n))
            .collect();
        assert!(
            stale.is_empty(),
            "in TOOL_CATEGORIES but not in get_tool_definitions(), so the local path still offers it: {stale:?}"
        );
    }

    /// A category with no keywords can never match a message, so its tools are
    /// never selected. That is how the memory tools went missing.
    #[test]
    fn every_tool_category_has_keywords() {
        for (category, _) in TOOL_CATEGORIES {
            assert!(
                CATEGORY_KEYWORDS.iter().any(|(c, _)| c == category),
                "TOOL_CATEGORIES has '{category}' but CATEGORY_KEYWORDS does not, so its tools are never selected"
            );
        }
    }

    /// The concrete drift failure: an 8k-32k local model gets max_tools = 15, so
    /// the filter runs, and `remember` must survive it.
    #[test]
    fn remember_survives_tool_filtering() {
        let all = crate::ai::tools::get_tool_definitions();
        let selected = select_relevant_tools(
            &all,
            "remember that I never take meetings before 10am",
            15,
        );
        assert!(
            selected.iter().any(|t| t.name == "remember"),
            "the fact would be silently dropped on the local path"
        );
    }

    /// Truncation used to cut an alphabetically sorted list, so on a busy
    /// message the surviving capabilities were decided by spelling: everything
    /// from `search_emails` onwards fell off the end while the system prompt
    /// still told the model to call it. The message's strongest category has to
    /// survive the budget intact.
    #[test]
    fn an_email_message_keeps_the_email_tools_when_the_budget_truncates() {
        let all = crate::ai::tools::get_tool_definitions();
        // Email is the strongest signal here, but github, notion, system and
        // calendar all match too, so the candidates overshoot 15 slots.
        let selected = select_relevant_tools(
            &all,
            "search my email inbox for the invoice, then open the notion page \
             about the meeting and file a github issue",
            15,
        );
        assert!(selected.len() <= 15);
        for name in ["search_emails", "read_email", "draft_email", "archive_email"] {
            assert!(
                selected.iter().any(|t| t.name == name),
                "'{name}' lost its slot on a message that is plainly about email; \
                 selected: {:?}",
                selected.iter().map(|t| &t.name).collect::<Vec<_>>()
            );
        }
        // The generally-useful tool the selector promises is never truncated away.
        assert!(selected.iter().any(|t| t.name == "create_task"));
    }

    /// The F017 residual, verbatim: one busy sentence that touches six
    /// categories used to push `remember` past the alphabetical cut, so the
    /// fact was silently not persisted. Relevance order keeps it.
    #[test]
    fn a_multi_category_message_keeps_the_memory_tools() {
        let all = crate::ai::tools::get_tool_definitions();
        let selected = select_relevant_tools(
            &all,
            "remember to open the github issue and the notion page about the meeting email",
            15,
        );
        assert!(
            selected.iter().any(|t| t.name == "remember"),
            "selected: {:?}",
            selected.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    /// Relevance ranking alone was not enough: on this sentence `calendar`
    /// scores 15 and `system` ties `memory` at 8 but sorts ahead of it, so the
    /// 15-slot cut landed inside the system block and the memory tools never
    /// made the list -- the model is told to call `remember` while the tool is
    /// absent, and the fact is silently not persisted. The reserved slots close
    /// that without disturbing the strongest category.
    #[test]
    fn the_memory_tools_survive_a_stronger_category() {
        let all = crate::ai::tools::get_tool_definitions();
        let message = "remember that the meeting on my calendar about the email invoice \
                       is in the notion page and I need to open the file";
        let selected = select_relevant_tools(&all, message, 15);
        assert!(selected.len() <= 15);
        for name in ["remember", "forget", "list_memories"] {
            assert!(
                selected.iter().any(|t| t.name == name),
                "'{name}' lost its slot on a sentence that asked for it; selected: {:?}",
                selected.iter().map(|t| &t.name).collect::<Vec<_>>()
            );
        }
        // The reserved slots are taken from the least relevant survivors, so the
        // message's strongest category is still there in full.
        for name in ["list_events", "create_event", "update_event", "delete_event"] {
            assert!(
                selected.iter().any(|t| t.name == name),
                "the strongest-matching category lost '{name}'; selected: {:?}",
                selected.iter().map(|t| &t.name).collect::<Vec<_>>()
            );
        }
        assert!(selected.iter().any(|t| t.name == "create_task"));
    }

    /// Rescuing one reserved tool must not evict another, and a budget smaller
    /// than the reserved set must not panic or duplicate.
    #[test]
    fn rescued_reserved_tools_do_not_evict_each_other() {
        let mut names = vec![
            "a",
            "b",
            "c",
            "list_memories",
            "d",
            "remember",
            "forget",
            "create_task",
        ];
        rescue_reserved_tools(&mut names, 5);
        let kept = &names[..5];
        for name in RESERVED_TOOLS {
            assert!(kept.contains(name), "'{name}' was dropped; kept: {kept:?}");
        }
        assert_eq!(kept.len(), 5);

        // Budget smaller than the reserved set: fill what fits, drop the rest.
        let mut tight = vec!["a", "remember", "forget", "list_memories", "create_task"];
        rescue_reserved_tools(&mut tight, 2);
        assert_eq!(tight.len(), 5);
    }
}
