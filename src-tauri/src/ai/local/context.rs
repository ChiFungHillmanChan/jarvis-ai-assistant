use tiktoken_rs::cl100k_base;

pub struct ContextManager {
    max_tokens: u32,
    reserve_for_response: u32,
    reserve_for_tools: u32,
}

impl ContextManager {
    pub fn new(max_tokens: u32, needs_tool_injection: bool) -> Self {
        ContextManager {
            max_tokens,
            reserve_for_response: 1024,
            reserve_for_tools: if needs_tool_injection { 2000 } else { 0 },
        }
    }

    /// Count tokens in a string using tiktoken cl100k_base tokenizer
    pub fn count_tokens(text: &str) -> u32 {
        match cl100k_base() {
            Ok(bpe) => bpe.encode_with_special_tokens(text).len() as u32,
            Err(_) => {
                // Fallback: chars / 3.5 (conservative estimate)
                (text.len() as f64 / 3.5).ceil() as u32
            }
        }
    }

    /// Available tokens for message content after reserves
    pub fn available_tokens(&self) -> u32 {
        self.max_tokens
            .saturating_sub(self.reserve_for_response)
            .saturating_sub(self.reserve_for_tools)
    }

    /// Truncate messages to fit within context window.
    /// Always preserves the system prompt (first message) and the latest user message.
    /// Returns (truncated_messages, rolling_summary_if_any)
    pub fn truncate_messages(
        &self,
        messages: &[(String, String)],
        system_prompt_tokens: u32,
    ) -> Vec<(String, String)> {
        if messages.is_empty() {
            return vec![];
        }

        let available = self.available_tokens().saturating_sub(system_prompt_tokens);
        let mut result: Vec<(String, String)> = Vec::new();
        let mut used_tokens: u32 = 0;

        // Always include the last message (the user's current input)
        let last_msg = &messages[messages.len() - 1];
        let last_tokens = Self::count_tokens(&last_msg.1);
        used_tokens += last_tokens;

        if used_tokens >= available {
            // Even the last message exceeds context -- just return it truncated
            return vec![last_msg.clone()];
        }

        // Walk backwards from second-to-last, adding messages while they fit
        let mut earlier: Vec<(String, String)> = Vec::new();
        for msg in messages[..messages.len() - 1].iter().rev() {
            let msg_tokens = Self::count_tokens(&msg.1);
            if used_tokens + msg_tokens > available {
                break;
            }
            used_tokens += msg_tokens;
            earlier.push(msg.clone());
        }
        earlier.reverse();

        result.extend(earlier);
        result.push(last_msg.clone());
        result
    }

    /// Determine behavior tier based on context length
    pub fn max_history_messages(context_length: u32) -> usize {
        match context_length {
            0..=4095 => 3,
            4096..=8191 => 5,
            8192..=32767 => 10,
            _ => 20,
        }
    }

    /// Determine max tools to inject for prompt-injected mode
    pub fn max_tools_for_context(context_length: u32) -> usize {
        match context_length {
            0..=4095 => 0,   // Chat only
            4096..=8191 => 5,
            8192..=32767 => 15,
            _ => 34, // All tools
        }
    }

    /// Whether day context (tasks, calendar, emails) should be included
    pub fn should_include_day_context(context_length: u32) -> bool {
        context_length >= 8192
    }
}
