use crate::engine::context_engine::ContextBuilder;
use crate::llm::request_compat::{ChatMessage, MessageContent};

pub struct ContextCompactor;

impl ContextCompactor {
    pub fn estimate_tokens(messages: &[ChatMessage]) -> u32 {
        let mut total_chars: usize = 0;
        for msg in messages {
            total_chars += msg.role.len() + 4;
            total_chars += msg.content.as_text_lossy().len();
            if let Some(tool_calls) = &msg.tool_calls {
                for tc in tool_calls {
                    total_chars += tc.function.name.len() + tc.function.arguments.len() + 10;
                }
            }
        }
        (total_chars as f64 / 3.5).ceil() as u32
    }

    pub fn needs_compaction(messages: &[ChatMessage], context_window: u32) -> bool {
        let current_tokens = Self::estimate_tokens(messages);
        let threshold = (context_window as u64 * 80 / 100) as u32;
        current_tokens >= threshold
    }

    pub fn compact_history(
        messages: Vec<ChatMessage>,
        keep_recent: usize,
    ) -> (Vec<ChatMessage>, bool) {
        if messages.len() <= 2 {
            return (messages, false);
        }

        let mut system_message: Option<ChatMessage> = None;
        let mut work_messages = messages;
        ContextBuilder::strip_all_media_for_compaction(&mut work_messages);

        if let Some(first) = work_messages.first() {
            if first.role == "system" {
                system_message = Some(work_messages.remove(0));
            }
        }

        if work_messages.len() <= 1 {
            let mut result = Vec::new();
            if let Some(sys) = system_message {
                result.push(sys);
            }
            result.extend(work_messages);
            return (result, false);
        }

        let mut split_idx = work_messages.len().saturating_sub(keep_recent.min(work_messages.len().saturating_sub(1)));
        while split_idx > 0 && work_messages[split_idx].role == "tool" {
            split_idx -= 1;
        }
        if split_idx == 0 {
            split_idx = work_messages.len().saturating_sub(1);
            while split_idx < work_messages.len() && work_messages[split_idx].role == "tool" {
                split_idx += 1;
            }
        }
        if split_idx == 0 {
            let mut result = Vec::new();
            if let Some(sys) = system_message {
                result.push(sys);
            }
            result.extend(work_messages);
            return (result, false);
        }
        let older_slice = &work_messages[..split_idx];
        let recent_slice = &work_messages[split_idx..];

        let mut summary_body = String::with_capacity(2048);
        summary_body.push_str("[CONTEXT COMPACTION SUMMARY - Key Events & Prior State]\n");

        for (i, msg) in older_slice.iter().enumerate() {
            let role_prefix = match msg.role.as_str() {
                "user" => "User Query",
                "assistant" => "Assistant Action",
                "tool" => "Tool Output",
                other => other,
            };
            let text = msg.content.as_text_lossy();
            let truncated = if text.len() > 300 {
                let end = text.char_indices().map(|(idx, _)| idx).nth(300).unwrap_or(text.len());
                format!("{}...", &text[..end])
            } else {
                text
            };

            summary_body.push_str(&format!("{}. [{}] {}\n", i + 1, role_prefix, truncated.trim()));
            if let Some(tool_calls) = &msg.tool_calls {
                for tc in tool_calls {
                    summary_body.push_str(&format!("   Executed Tool: {}({})\n", tc.function.name, tc.function.arguments));
                }
            }
        }
        summary_body.push_str("[End of Prior State Summary. Retaining subsequent conversation context verbatim.]\n");

        let mut compacted = Vec::with_capacity(keep_recent + 2);
        if let Some(sys) = system_message {
            compacted.push(sys);
        }
        compacted.push(ChatMessage {
            role: "system".to_string(),
            content: MessageContent::Text(summary_body),
            name: Some("context_compact_summary".to_string()),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        });
        compacted.extend_from_slice(recent_slice);

        (compacted, true)
    }
}
