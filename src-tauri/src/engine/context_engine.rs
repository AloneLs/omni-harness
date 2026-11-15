use crate::engine::task_contract::TaskContract;
use crate::llm::request_compat::{ChatMessage, MessageContent};
use std::path::Path;
use std::time::{Duration, SystemTime};

pub fn estimate_text_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let mut ascii_chars = 0usize;
    let mut non_ascii_chars = 0usize;
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii_chars += 1;
        } else {
            non_ascii_chars += 1;
        }
    }
    let est = (ascii_chars as f64 / 3.8) + (non_ascii_chars as f64 / 2.8);
    est.ceil() as u32
}

pub fn estimate_messages_tokens(messages: &[ChatMessage]) -> u32 {
    let mut total: u32 = 0;
    for msg in messages {
        total += 4;
        total += estimate_text_tokens(&msg.role);
        total += estimate_text_tokens(&msg.content.as_text_lossy());
        if let Some(tcs) = &msg.tool_calls {
            for tc in tcs {
                total += 8;
                total += estimate_text_tokens(&tc.function.name);
                total += estimate_text_tokens(&tc.function.arguments);
            }
        }
    }
    total
}

pub fn safe_slice_head(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => &text[..idx],
        None => text,
    }
}

pub fn safe_slice_tail(text: &str, max_chars: usize) -> &str {
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text;
    }
    let skip = total_chars - max_chars;
    match text.char_indices().nth(skip) {
        Some((idx, _)) => &text[idx..],
        None => text,
    }
}

pub fn compute_prompt_cache_key(workspace_root: &str, chat_id: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in workspace_root.as_bytes().iter().chain(b":").chain(chat_id.as_bytes()) {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("omni-{:016x}", hash)
}

pub fn cleanup_old_tmp_logs(workspace_root: &str, max_age_hours: u64) {
    let tmp_dir = crate::state::AppState::resolve_chat_temp_dir(Some(workspace_root)).join("tmp");
    if !tmp_dir.is_dir() {
        return;
    }
    let max_age = Duration::from_secs(max_age_hours.saturating_mul(3600));
    let now = SystemTime::now();
    if let Ok(entries) = std::fs::read_dir(&tmp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("tool_") && name.ends_with(".log") {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if let Ok(elapsed) = now.duration_since(modified) {
                                if elapsed > max_age {
                                    let _ = std::fs::remove_file(&path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn spill_tool_output_if_needed(
    workspace_root: &str,
    call_id: &str,
    raw_output: &str,
) -> String {
    let lines: Vec<&str> = raw_output.lines().collect();
    let total_lines = lines.len();
    if total_lines <= 300 && raw_output.len() <= 24_000 {
        return raw_output.to_string();
    }

    let safe_id: String = call_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let file_name = if safe_id.is_empty() {
        format!("tool_{}.log", chrono::Utc::now().timestamp_millis())
    } else {
        format!("tool_{}.log", safe_id)
    };

    let tmp_dir = crate::state::AppState::resolve_chat_temp_dir(Some(workspace_root)).join("tmp");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let log_path = tmp_dir.join(&file_name);
    let _ = std::fs::write(&log_path, raw_output);

    if total_lines <= 10 && raw_output.len() > 24_000 {
        let head = safe_slice_head(raw_output, 2_000);
        let tail = safe_slice_tail(raw_output, 2_000);
        return format!(
            "{}\n\n... [Single-line output truncated ({} bytes). Full output saved to {} — use grep or read_file to inspect] ...\n\n{}",
            head, raw_output.len(), file_name, tail
        );
    }

    let head = lines
        .iter()
        .take(60)
        .map(|l| safe_slice_head(l, 500))
        .collect::<Vec<&str>>()
        .join("\n");
    let tail_start = total_lines.saturating_sub(80);
    let tail = lines[tail_start..]
        .iter()
        .map(|l| safe_slice_head(l, 500))
        .collect::<Vec<&str>>()
        .join("\n");

    format!(
        "{}\n\n... [Output truncated ({} lines, {} bytes). Full output saved to {} — use grep or read_file with offset/limit to inspect] ...\n\n{}",
        head, total_lines, raw_output.len(), file_name, tail
    )
}

pub fn read_project_memory_prefix(workspace_root: &str) -> String {
    let root = Path::new(workspace_root);
    for candidate in ["AGENTS.md", "CLAUDE.md", ".omniharness/rules.md"] {
        let p = root.join(candidate);
        if p.is_file() {
            if let Ok(content) = std::fs::read_to_string(&p) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    let cap = if trimmed.chars().count() > 6000 {
                        format!("{}...", safe_slice_head(trimmed, 6000))
                    } else {
                        trimmed.to_string()
                    };
                    let age_note = std::fs::metadata(&p)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| SystemTime::now().duration_since(t).ok())
                        .map(|d| d.as_secs() / 86400)
                        .filter(|days| *days >= 1)
                        .map(|days| format!("\n<system-reminder>Memory file {} is {} day(s) old. Verify its claims against live files before acting.</system-reminder>", candidate, days))
                        .unwrap_or_default();
                    return format!("\n--- PROJECT MEMORY ({}) ---\n{}{}\n", candidate, cap, age_note);
                }
            }
        }
    }
    String::new()
}

pub fn read_git_short_status(workspace_root: &str) -> Option<String> {
    let git_dir = Path::new(workspace_root).join(".git");
    if !git_dir.exists() {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.args(["-C", workspace_root, "status", "--short"]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    if let Ok(out) = cmd.output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                let lines: Vec<&str> = s.lines().take(25).collect();
                return Some(lines.join("\n"));
            }
        }
    }
    None
}

pub fn snap_to_turn_boundary(messages: &[ChatMessage], start_idx: usize) -> usize {
    if messages.is_empty() {
        return 0;
    }
    let mut idx = start_idx.min(messages.len().saturating_sub(1));
    while idx > 1 && messages[idx].role == "tool" {
        idx -= 1;
    }
    if idx > 1 && messages[idx].role == "assistant" && messages[idx].tool_calls.is_some() {
        return idx;
    }
    idx
}

#[derive(Debug, Default, Clone)]
pub struct CompactionCursor {
    pub last_pruned_upto: usize,
    pub last_pruned_at_ts: Option<i64>,
}

pub struct ContextBuilder;

impl ContextBuilder {
    pub fn build_projection(
        transcript: &[ChatMessage],
        workspace_root: &str,
        contract: Option<&TaskContract>,
        context_window: u32,
        last_request_ts: Option<i64>,
        preserve_past_reasoning: bool,
        cursor: &mut CompactionCursor,
        include_tail_state: bool,
    ) -> (Vec<ChatMessage>, bool) {
        let _ = (preserve_past_reasoning, cursor);
        let mut proj: Vec<ChatMessage> = transcript.to_vec();
        let now_ts = chrono::Utc::now().timestamp();
        let cache_cold = last_request_ts.map(|ts| now_ts - ts > 300).unwrap_or(false);
        let used = estimate_messages_tokens(&proj);
        let high_pressure = context_window > 0 && (used as u64 * 100 / context_window as u64) >= 80;
        let pruned = if cache_cold || high_pressure {
            Self::micro_prune(&mut proj, 20_000, 6).is_some()
        } else {
            false
        };

        if include_tail_state {
            let mut parts: Vec<String> = Vec::new();
            if let Some(tail) = Self::build_dynamic_tail(workspace_root, contract) {
                parts.push(tail);
            }
            if context_window > 0 {
                let pct = (used as u64 * 100 / context_window as u64) as u32;
                if pct >= 75 {
                    parts.push(format!(
                        "Context budget at {}% ({}/{} tokens). Prefer targeted read_file(offset, limit) and delegate broad searches to agent(subagent_type=\"Explore\") to avoid context rot.",
                        pct, used, context_window
                    ));
                }
            }
            if !parts.is_empty() {
                proj.push(ChatMessage {
                    role: "user".to_string(),
                    content: MessageContent::Text(format!("<system-reminder>\n{}\n</system-reminder>", parts.join("\n"))),
                    name: Some("ephemeral_runtime_state".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                });
            }
        }

        (proj, pruned)
    }

    fn apply_prune_range(messages: &mut [ChatMessage], from: usize, to: usize) -> bool {
        let mut changed = false;
        let end = to.min(messages.len());
        for i in from..end {
            if messages[i].role == "tool" {
                let text = messages[i].content.as_text_lossy();
                if text.chars().count() > 4000 && !text.starts_with("[Pruned output of ") {
                    let tool_name = messages[i].name.as_deref().unwrap_or("tool");
                    let line_count = text.lines().count();
                    let head = safe_slice_head(&text, 400);
                    let tail = safe_slice_tail(&text, 600);
                    messages[i].content = MessageContent::Text(format!(
                        "[Pruned output of {} ({} lines, {} chars) to save context]\nHead:\n{}\n...\nTail:\n{}",
                        tool_name, line_count, text.chars().count(), head, tail
                    ));
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn micro_prune(
        messages: &mut [ChatMessage],
        protect_recent_tokens: u32,
        protect_recent_msgs: usize,
    ) -> Option<usize> {
        let len = messages.len();
        if len <= protect_recent_msgs + 2 {
            return None;
        }

        let mut protected_from = len.saturating_sub(protect_recent_msgs);
        let mut acc_tokens: u32 = 0;
        for i in (0..len).rev() {
            acc_tokens += estimate_text_tokens(&messages[i].content.as_text_lossy());
            if acc_tokens >= protect_recent_tokens {
                protected_from = protected_from.min(i);
                break;
            }
        }

        let safe_boundary = snap_to_turn_boundary(messages, protected_from);
        if safe_boundary <= 1 {
            return None;
        }

        if Self::apply_prune_range(messages, 1, safe_boundary) {
            Some(safe_boundary)
        } else {
            None
        }
    }

    pub fn needs_auto_compaction(
        messages: &[ChatMessage],
        context_window: u32,
        max_output_tokens: u32,
    ) -> bool {
        let current = estimate_messages_tokens(messages);
        let reserve = max_output_tokens.min(32_000) + 16_000;
        let hard_cap = context_window.saturating_sub(reserve);
        let soft_cap = (context_window as u64 * 82 / 100) as u32;
        let trigger = hard_cap.min(soft_cap).max(16_000);
        current >= trigger
    }

    pub fn build_compaction_prompt(custom_instruction: Option<&str>) -> String {
        let extra = custom_instruction
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| format!("\nAdditional user compaction focus: {}\n", s))
            .unwrap_or_default();

        format!(
            "First, perform a chronological scratchpad inside <analysis>...</analysis> reviewing user intent, tool calls, errors, and file states. Then output a dense, lossless engineering handoff inside <context_summary>...</context_summary> with these 9 mandatory sections (write '(none)' if empty, never omit a section):\n\
1. Primary Request and Intent\n\
2. Key Technical Concepts\n\
3. Files and Code Sections (exact paths, symbols, verbatim snippets)\n\
4. Errors and Fixes\n\
5. Problem-Solving Progress\n\
6. All User Messages (preserve exact requirements and constraints verbatim)\n\
7. Pending Tasks and Active Plan\n\
8. Current Work State\n\
9. Exact Next Step{}",
            extra
        )
    }

    pub fn rehydrate_after_compaction(
        transcript: &[ChatMessage],
        summary_text: &str,
        workspace_root: &str,
        contract: Option<&TaskContract>,
        keep_recent_msgs: usize,
    ) -> Vec<ChatMessage> {
        let mut out = Vec::new();
        if let Some(first) = transcript.first() {
            if first.role == "system" {
                out.push(first.clone());
            }
        }

        let extracted_summary = if let Some(s) = summary_text.find("<context_summary>") {
            if let Some(e) = summary_text[s..].find("</context_summary>") {
                summary_text[s..s + e + "</context_summary>".len()].to_string()
            } else {
                summary_text[s..].to_string()
            }
        } else {
            format!("<context_summary>\n{}\n</context_summary>", summary_text.trim())
        };
        let clean_summary = extracted_summary;

        out.push(ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text(clean_summary),
            name: Some("context_compact_summary".to_string()),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        });

        let max_tail_tokens: u32 = 25_000;
        let mut raw_start = transcript.len().saturating_sub(keep_recent_msgs);
        let mut tail_tokens: u32 = 0;
        for i in (raw_start..transcript.len()).rev() {
            let t = estimate_text_tokens(&transcript[i].content.as_text_lossy());
            if tail_tokens + t > max_tail_tokens && i + 1 < transcript.len() {
                raw_start = i + 1;
                break;
            }
            tail_tokens += t;
        }

        let snapped_start = snap_to_turn_boundary(transcript, raw_start);
        let mut preserved_user_msgs: Vec<String> = Vec::new();
        for m in &transcript[..snapped_start] {
            if m.name.as_deref() == Some("preserved_user_messages") {
                let prev = m.content.as_text_lossy();
                let body = prev
                    .trim()
                    .strip_prefix("<preserved_user_messages>")
                    .unwrap_or(&prev)
                    .strip_suffix("</preserved_user_messages>")
                    .unwrap_or(&prev)
                    .trim();
                if !body.is_empty() && !preserved_user_msgs.contains(&body.to_string()) {
                    preserved_user_msgs.push(body.to_string());
                }
                continue;
            }
            if m.role == "user"
                && m.name.as_deref() != Some("ephemeral_runtime_state")
                && m.name.as_deref() != Some("context_compact_summary")
                && m.name.as_deref() != Some("preserved_active_skills")
            {
                let txt = m.content.as_text_lossy();
                let trimmed = txt.trim();
                if !trimmed.is_empty()
                    && !trimmed.starts_with("<system-reminder>")
                    && !trimmed.starts_with("[Goal Stop-Hook Feedback")
                {
                    let entry = format!("- {}", trimmed);
                    if !preserved_user_msgs.contains(&entry) {
                        preserved_user_msgs.push(entry);
                    }
                }
            }
        }
        if !preserved_user_msgs.is_empty() {
            let joined = preserved_user_msgs.join("\n");
            let capped = if joined.chars().count() > 24_000 {
                format!("{}...\n[truncated older middle]", safe_slice_head(&joined, 24_000))
            } else {
                joined
            };
            out.push(ChatMessage {
                role: "user".to_string(),
                content: MessageContent::Text(format!("<preserved_user_messages>\n{}\n</preserved_user_messages>", capped)),
                name: Some("preserved_user_messages".to_string()),
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            });
        }

        let mut preserved_skills: Vec<String> = Vec::new();
        for m in &transcript[..snapped_start] {
            let txt = m.content.as_text_lossy();
            let mut search_from = 0usize;
            while let Some(rel_start) = txt[search_from..].find("<skill_content ") {
                let abs_start = search_from + rel_start;
                if let Some(rel_end) = txt[abs_start..].find("</skill_content>") {
                    let abs_end = abs_start + rel_end + "</skill_content>".len();
                    let block = txt[abs_start..abs_end].to_string();
                    if !preserved_skills.contains(&block) {
                        preserved_skills.push(block);
                    }
                    search_from = abs_end;
                } else {
                    break;
                }
            }
        }
        if !preserved_skills.is_empty() {
            out.push(ChatMessage {
                role: "user".to_string(),
                content: MessageContent::Text(preserved_skills.join("\n\n")),
                name: Some("preserved_active_skills".to_string()),
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            });
        }

        for m in &transcript[snapped_start..] {
            if m.role != "system" && m.name.as_deref() != Some("ephemeral_runtime_state") {
                let mut cloned = m.clone();
                cloned.reasoning_content = None;
                out.push(cloned);
            }
        }

        if let Some(tail) = Self::build_dynamic_tail(workspace_root, contract) {
            out.push(ChatMessage {
                role: "user".to_string(),
                content: MessageContent::Text(format!("<system-reminder>\n{}\n</system-reminder>", tail)),
                name: Some("post_compaction_rehydration".to_string()),
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            });
        }

        out
    }

    fn build_dynamic_tail(workspace_root: &str, contract: Option<&TaskContract>) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(git_st) = read_git_short_status(workspace_root) {
            parts.push(format!("Git status:\n{}", git_st));
        }
        if let Some(c) = contract {
            let mut todo_lines = Vec::new();
            for crit in &c.criteria {
                todo_lines.push(format!("- [{:?}] {}", crit.status, crit.description));
            }
            if !todo_lines.is_empty() {
                parts.push(format!("Active contract ({}):\n{}", c.goal, todo_lines.join("\n")));
            }
        }
        if parts.is_empty() {
            None
        } else {
            Some(format!("[RUNTIME STATE]\n{}", parts.join("\n\n")))
        }
    }
}
