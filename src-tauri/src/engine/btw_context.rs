use crate::db::Database;
use crate::engine::context_engine::estimate_text_tokens;
use crate::engine::stream_manager::{LiveContextSnapshot, StreamManager};
use crate::llm::capabilities::CapabilityRegistry;
use crate::llm::request_compat::ChatMessage;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct BtwFallbackMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    User,
    Assistant,
    ToolResult,
    LiveInProgress,
}

#[derive(Debug, Clone)]
struct TranscriptEntry {
    role: &'static str,
    kind: EntryKind,
    header: String,
    args_text: String,
    body_text: String,
    reasoning_text: String,
}

fn smart_truncate_head_tail(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars || max_chars < 64 {
        return text.to_string();
    }
    let head_len = (max_chars * 60) / 100;
    let tail_len = max_chars.saturating_sub(head_len);
    let head: String = text.chars().take(head_len).collect();
    let skip = total.saturating_sub(tail_len);
    let tail: String = text.chars().skip(skip).collect();
    let omitted = total.saturating_sub(head_len + tail_len);
    format!("{}\n... [truncated {} chars] ...\n{}", head, omitted, tail)
}

fn normalize_sig(role: &str, text: &str) -> String {
    let trimmed = text.trim();
    let prefix: String = trimmed.chars().take(240).collect();
    format!("{}:{}", role, prefix)
}

fn format_tool_record(rec: &Value) -> String {
    let name = rec
        .get("toolName")
        .or_else(|| rec.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("tool");
    let status = rec
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let elapsed_ms = rec.get("elapsedMs").and_then(|v| v.as_u64());
    let raw_args = rec
        .get("rawArgs")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            rec.get("args")
                .filter(|v| !v.is_null())
                .and_then(|v| serde_json::to_string_pretty(v).ok())
                .filter(|s| s != "{}" && !s.trim().is_empty())
        })
        .unwrap_or_default();
    let output = rec
        .get("output")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let error = rec
        .get("error")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut out = String::new();
    match elapsed_ms {
        Some(ms) => out.push_str(&format!("[TOOL CALL: {} | status={} | {}ms]", name, status, ms)),
        None => out.push_str(&format!("[TOOL CALL: {} | status={}]", name, status)),
    }
    if !raw_args.is_empty() {
        out.push_str("\nArguments:\n");
        out.push_str(&smart_truncate_head_tail(&raw_args, 8000));
    }
    if !output.is_empty() {
        out.push_str("\nTool Output:\n");
        out.push_str(&smart_truncate_head_tail(&output, 12000));
    }
    if let Some(err) = error {
        out.push_str("\nTool Error:\n");
        out.push_str(&smart_truncate_head_tail(&err, 3000));
    }
    out
}

fn format_cached_ui_message(item: &Value) -> Option<(String, String, String, String)> {
    let role = item
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if role != "user" && role != "assistant" {
        return None;
    }
    let raw_content = item
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let reasoning = item
        .get("reasoningContent")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if role == "user" {
        let mut text = raw_content.clone();
        if let Some(atts) = item.get("attachments").and_then(|v| v.as_array()) {
            if !atts.is_empty() {
                let names: Vec<String> = atts
                    .iter()
                    .filter_map(|a| {
                        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let path = a.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        if !name.is_empty() || !path.is_empty() {
                            Some(if path.is_empty() {
                                name.to_string()
                            } else {
                                format!("{} ({})", name, path)
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                if !names.is_empty() {
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(&format!("[Attachments: {}]", names.join(", ")));
                }
            }
        }
        if text.is_empty() {
            return None;
        }
        return Some((role, raw_content, text, String::new()));
    }

    let mut parts: Vec<String> = Vec::new();
    let mut seen_call_ids: HashSet<String> = HashSet::new();
    let mut has_message_step = false;

    if let Some(steps) = item.get("steps").and_then(|v| v.as_array()) {
        for s in steps {
            let kind = s.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "message" {
                let txt = s.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
                if !txt.is_empty() {
                    has_message_step = true;
                    parts.push(txt.to_string());
                }
            } else if kind == "tool" {
                if let Some(rec) = s.get("record") {
                    if let Some(cid) = rec.get("toolCallId").and_then(|v| v.as_str()) {
                        seen_call_ids.insert(cid.to_string());
                    }
                    parts.push(format_tool_record(rec));
                }
            } else if kind == "compact" {
                let sum = s.get("summary").and_then(|v| v.as_str()).unwrap_or("").trim();
                if !sum.is_empty() {
                    parts.push(format!("[Compacted Earlier Context]\n{}", smart_truncate_head_tail(sum, 4000)));
                }
            }
        }
    }

    if let Some(tcs) = item.get("toolCalls").and_then(|v| v.as_array()) {
        for rec in tcs {
            let cid = rec
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !cid.is_empty() && seen_call_ids.contains(cid) {
                continue;
            }
            parts.push(format_tool_record(rec));
        }
    }

    if !raw_content.is_empty() && !has_message_step {
        parts.push(raw_content.clone());
    }

    if let Some(todos) = item.get("todos").and_then(|v| v.as_array()) {
        if !todos.is_empty() {
            let todo_lines: Vec<String> = todos
                .iter()
                .filter_map(|t| {
                    let st = t.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
                    let txt = t.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    if txt.is_empty() {
                        None
                    } else {
                        Some(format!("- [{}] {}", st, txt))
                    }
                })
                .collect();
            if !todo_lines.is_empty() {
                parts.push(format!("[Task Board]\n{}", todo_lines.join("\n")));
            }
        }
    }

    if let Some(err) = item.get("error").and_then(|v| v.as_str()).map(|s| s.trim()).filter(|s| !s.is_empty()) {
        parts.push(format!("[Assistant Error: {}]", err));
    } else if item.get("paused").and_then(|v| v.as_bool()).unwrap_or(false) {
        parts.push("[Turn Paused/Interrupted]".to_string());
    }

    let combined = parts.join("\n\n");
    if combined.trim().is_empty() && reasoning.is_empty() {
        return None;
    }
    Some((role, raw_content, combined, reasoning))
}

fn load_cached_ui_entries(db: &Database, chat_id: &str) -> Vec<(String, String, String, String)> {
    let raw = match db.get_cached_chat_messages(chat_id).ok().flatten() {
        Some((_, json)) => json,
        None => return Vec::new(),
    };
    let parsed: Vec<Value> = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    parsed
        .iter()
        .filter_map(format_cached_ui_message)
        .collect()
}

fn build_session_metadata_block(db: &Database, chat_id: &str) -> String {
    let mut sections: Vec<String> = Vec::new();

    if let Ok(chat) = db.get_chat(chat_id) {
        let mut meta = format!("Session: {} (id={})", chat.title, chat.id);
        if let Some(ref proj) = chat.project_id {
            if !proj.trim().is_empty() {
                meta.push_str(&format!("\nWorkspace: {}", proj));
            }
        }
        sections.push(meta);
    }

    if let Some(raw_goal) = db.load_goal_state(chat_id).ok().flatten() {
        if let Ok(g) = serde_json::from_str::<crate::engine::goal_loop::GoalState>(&raw_goal) {
            let mut gs = format!(
                "Goal (/goal): status={} | turn={}/{}\nCondition: {}",
                g.status.as_str(),
                g.turn,
                g.max_turns,
                g.condition
            );
            if let Some(ref r) = g.last_reason {
                if !r.trim().is_empty() {
                    gs.push_str(&format!("\nLast Evaluator Reason: {}", r));
                }
            }
            sections.push(gs);
        }
    }

    if let Some((plan_mode, plan_md)) = db.load_plan_state(chat_id).ok().flatten() {
        if plan_mode || !plan_md.trim().is_empty() {
            let mut ps = format!("Plan Mode: {}", if plan_mode { "ACTIVE" } else { "inactive" });
            if !plan_md.trim().is_empty() {
                ps.push_str(&format!("\nSaved Plan:\n{}", smart_truncate_head_tail(plan_md.trim(), 4000)));
            }
            sections.push(ps);
        }
    }

    if let Some((mem_md, _)) = db.load_session_memory(chat_id).ok().flatten() {
        if !mem_md.trim().is_empty() {
            sections.push(format!(
                "Session Memory:\n{}",
                smart_truncate_head_tail(mem_md.trim(), 5000)
            ));
        }
    }

    let files = db
        .list_recent_artifacts(chat_id, "file", 30)
        .unwrap_or_default();
    let cmds = db
        .list_recent_artifacts(chat_id, "command", 20)
        .unwrap_or_default();
    if !files.is_empty() || !cmds.is_empty() {
        let mut art = String::from("Session Artifacts:");
        if !files.is_empty() {
            art.push_str(&format!("\nFiles touched/read: {}", files.join(", ")));
        }
        if !cmds.is_empty() {
            art.push_str(&format!("\nCommands run: {}", cmds.join(" | ")));
        }
        sections.push(art);
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!("[SESSION METADATA & ARTIFACTS]\n{}", sections.join("\n\n"))
    }
}

fn render_entry_text(
    entry: &TranscriptEntry,
    max_args_chars: usize,
    max_body_chars: usize,
    max_reasoning_chars: usize,
    include_reasoning: bool,
) -> String {
    let mut out = String::new();
    if !entry.header.is_empty() {
        out.push_str(&entry.header);
    }
    if include_reasoning && max_reasoning_chars > 0 && !entry.reasoning_text.trim().is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("[Reasoning]\n");
        out.push_str(&smart_truncate_head_tail(
            entry.reasoning_text.trim(),
            max_reasoning_chars,
        ));
    }
    if !entry.args_text.trim().is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        if max_args_chars > 0 {
            out.push_str("Arguments:\n");
            out.push_str(&smart_truncate_head_tail(entry.args_text.trim(), max_args_chars));
        }
    }
    if !entry.body_text.trim().is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&smart_truncate_head_tail(
            entry.body_text.trim(),
            max_body_chars,
        ));
    }
    out
}

fn render_and_budget_entries(
    metadata_block: &str,
    entries: &[TranscriptEntry],
    token_budget: usize,
) -> Vec<( &'static str, String )> {
    let n = entries.len();
    let recent_tail_start = n.saturating_sub(16);

    let build_rendered = |old_args: usize, old_body: usize, new_args: usize, new_body: usize, reasoning_tail: usize| -> (Vec<(&'static str, String)>, usize) {
        let mut items: Vec<(&'static str, String)> = Vec::new();
        let mut total_tokens: usize = 0;

        if !metadata_block.is_empty() {
            total_tokens += estimate_text_tokens(metadata_block) as usize + 8;
            items.push(("user", metadata_block.to_string()));
        }

        let reasoning_start = n.saturating_sub(reasoning_tail);
        for (idx, e) in entries.iter().enumerate() {
            let is_recent = idx >= recent_tail_start;
            let (ma, mb) = match e.kind {
                EntryKind::ToolResult | EntryKind::LiveInProgress => {
                    if is_recent { (new_args, new_body) } else { (old_args, old_body) }
                }
                EntryKind::User | EntryKind::Assistant => {
                    if is_recent { (new_args, 24_000) } else { (old_args, old_body.max(6_000)) }
                }
            };
            let inc_reasoning = idx >= reasoning_start;
            let text = render_entry_text(e, ma, mb, if inc_reasoning { 3_000 } else { 0 }, inc_reasoning);
            if text.trim().is_empty() {
                continue;
            }
            total_tokens += estimate_text_tokens(&text) as usize + 6;
            items.push((e.role, text));
        }
        (items, total_tokens)
    };

    // Pass 1: Full fidelity
    let (items1, tok1) = build_rendered(8_000, 12_000, 8_000, 12_000, 12);
    if tok1 <= token_budget {
        return items1;
    }

    // Pass 2: Compact older tool outputs
    let (items2, tok2) = build_rendered(4_000, 1_800, 8_000, 12_000, 8);
    if tok2 <= token_budget {
        return items2;
    }

    // Pass 3: Compact older tool arguments & outputs more tightly
    let (items3, tok3) = build_rendered(800, 800, 6_000, 10_000, 4);
    if tok3 <= token_budget {
        return items3;
    }

    // Pass 4: Keep metadata + first user prompt (if any) + most recent entries within budget
    let mut kept_tail: Vec<(&'static str, String)> = Vec::new();
    let mut used = if !metadata_block.is_empty() {
        estimate_text_tokens(metadata_block) as usize + 8
    } else {
        0
    };

    let first_user_idx = entries.iter().position(|e| e.kind == EntryKind::User);
    let first_user_rendered = first_user_idx.map(|idx| {
        let t = render_entry_text(&entries[idx], 800, 4_000, 0, false);
        let tok = estimate_text_tokens(&t) as usize + 6;
        (idx, t, tok)
    });
    if let Some((_, _, tok)) = first_user_rendered {
        used += tok + 32;
    }

    let mut dropped_count = 0usize;
    let start_after_first = first_user_idx.map(|i| i + 1).unwrap_or(0);
    for idx in (start_after_first..n).rev() {
        let e = &entries[idx];
        let is_very_recent = idx >= n.saturating_sub(8);
        let text = if is_very_recent {
            render_entry_text(e, 4_000, 6_000, 1_500, true)
        } else {
            render_entry_text(e, 600, 700, 0, false)
        };
        if text.trim().is_empty() {
            continue;
        }
        let tok = estimate_text_tokens(&text) as usize + 6;
        if used + tok <= token_budget || kept_tail.len() < 4 {
            used += tok;
            kept_tail.push((e.role, text));
        } else {
            dropped_count += 1;
        }
    }
    kept_tail.reverse();

    let mut result: Vec<(&'static str, String)> = Vec::new();
    if !metadata_block.is_empty() {
        result.push(("user", metadata_block.to_string()));
    }
    if let Some((_, t, _)) = first_user_rendered {
        if !t.trim().is_empty() {
            result.push(("user", t));
        }
    }
    if dropped_count > 0 {
        result.push((
            "assistant",
            format!("[... {} earlier conversation/tool entries omitted to fit model context window ...]", dropped_count),
        ));
    }
    result.extend(kept_tail);
    result
}

pub async fn build_btw_messages(
    db: &Database,
    stream_mgr: &StreamManager,
    chat_id: &str,
    model_id: &str,
    caps_reg: &CapabilityRegistry,
    fallback_history: Option<&[BtwFallbackMessage]>,
    question: &str,
) -> Vec<ChatMessage> {
    let live_opt: Option<LiveContextSnapshot> = stream_mgr
        .get_session(chat_id)
        .await
        .map(|s| s.snapshot_live_context());

    let live_has_history = live_opt
        .as_ref()
        .map(|l| !l.history.is_empty())
        .unwrap_or(false);

    let mut ckpt_msgs: Vec<ChatMessage> = if live_has_history {
        live_opt.as_ref().map(|l| l.history.clone()).unwrap_or_default()
    } else {
        let full_key = format!("full_ctx:{}", chat_id);
        let tg_key = format!("tg_ctx:{}", chat_id);
        db.load_checkpoint(&full_key)
            .ok()
            .flatten()
            .or_else(|| db.load_checkpoint(chat_id).ok().flatten())
            .or_else(|| db.load_checkpoint(&tg_key).ok().flatten())
            .and_then(|j| serde_json::from_str::<Vec<ChatMessage>>(&j).ok())
            .unwrap_or_default()
    };

    let cached_ui = load_cached_ui_entries(db, chat_id);
    let db_msgs = db.list_messages(chat_id).unwrap_or_default();

    let mut entries: Vec<TranscriptEntry> = Vec::new();

    if !ckpt_msgs.is_empty() {
        // Build lookup for cached UI messages that carry tool steps/calls for earlier Desktop turns
        // (e.g. when Turn 2 started from initial_history without Turn 1's tool_calls).
        let mut cached_rich_by_sig: HashMap<String, (String, String)> = HashMap::new();
        for (role, raw_c, full_c, reason) in &cached_ui {
            if role == "assistant" && full_c != raw_c && !raw_c.trim().is_empty() {
                cached_rich_by_sig.insert(
                    normalize_sig("assistant", raw_c),
                    (full_c.clone(), reason.clone()),
                );
            }
        }

        let mut call_meta: HashMap<String, (String, String)> = HashMap::new();
        let mut known_sigs: HashSet<String> = HashSet::new();

        let total_ckpt = ckpt_msgs.len();
        for idx in 0..total_ckpt {
            let m = &ckpt_msgs[idx];
            if m.role == "system" {
                continue;
            }
            let text = m.content.as_text_lossy().trim().to_string();
            if m.role == "user" {
                if text.is_empty() {
                    continue;
                }
                known_sigs.insert(normalize_sig("user", &text));
                entries.push(TranscriptEntry {
                    role: "user",
                    kind: EntryKind::User,
                    header: String::new(),
                    args_text: String::new(),
                    body_text: text,
                    reasoning_text: String::new(),
                });
            } else if m.role == "assistant" {
                if !text.is_empty() {
                    known_sigs.insert(normalize_sig("assistant", &text));
                }
                let tcs = m.tool_calls.as_deref().unwrap_or(&[]);
                let next_is_tool = ckpt_msgs
                    .get(idx + 1)
                    .map(|nm| nm.role == "tool")
                    .unwrap_or(false);

                for tc in tcs {
                    call_meta.insert(
                        tc.id.clone(),
                        (tc.function.name.clone(), tc.function.arguments.clone()),
                    );
                }

                // If this assistant entry has no tool_calls & no following tool result
                // (common for earlier Desktop turns when Turn 2+ only seeded summary text),
                // enrich it from chat_context_cache if available.
                let mut effective_text = text.clone();
                let mut effective_reasoning = m.reasoning_content.clone().unwrap_or_default();
                if tcs.is_empty() && !next_is_tool && !text.is_empty() {
                    if let Some((rich_text, rich_reason)) =
                        cached_rich_by_sig.get(&normalize_sig("assistant", &text))
                    {
                        effective_text = rich_text.clone();
                        if effective_reasoning.is_empty() && !rich_reason.is_empty() {
                            effective_reasoning = rich_reason.clone();
                        }
                    }
                }

                let mut tc_lines: Vec<String> = Vec::new();
                for tc in tcs {
                    let mut line = format!("[TOOL CALL: {} | id={}]", tc.function.name, tc.id);
                    let args_t = tc.function.arguments.trim();
                    if !args_t.is_empty() && args_t != "{}" {
                        line.push_str(&format!("\nArguments:\n{}", smart_truncate_head_tail(args_t, 8000)));
                    }
                    tc_lines.push(line);
                }

                let mut body = effective_text;
                if !tc_lines.is_empty() {
                    if !body.is_empty() {
                        body.push_str("\n\n");
                    }
                    body.push_str(&tc_lines.join("\n\n"));
                }

                if !body.trim().is_empty() || !effective_reasoning.trim().is_empty() {
                    entries.push(TranscriptEntry {
                        role: "assistant",
                        kind: EntryKind::Assistant,
                        header: String::new(),
                        args_text: String::new(),
                        body_text: body,
                        reasoning_text: effective_reasoning,
                    });
                }
            } else if m.role == "tool" {
                let cid = m.tool_call_id.as_deref().unwrap_or("");
                let (tname, targs) = if let Some((cn, ca)) = call_meta.get(cid) {
                    (cn.as_str(), ca.as_str())
                } else {
                    (m.name.as_deref().unwrap_or("tool"), "")
                };
                let header = if cid.is_empty() {
                    format!("[TOOL RESULT: {}]", tname)
                } else {
                    format!("[TOOL RESULT: {} | call_id={}]", tname, cid)
                };
                entries.push(TranscriptEntry {
                    role: "assistant",
                    kind: EntryKind::ToolResult,
                    header,
                    args_text: if targs.trim() == "{}" { String::new() } else { targs.to_string() },
                    body_text: if text.is_empty() { "(empty tool output)".to_string() } else { text },
                    reasoning_text: String::new(),
                });
            }
        }

        // Append any newer messages present in cached_ui / fallback_history / db_msgs
        for (role, raw_c, full_c, reason) in &cached_ui {
            let sig_full = normalize_sig(role, full_c);
            let sig_raw = normalize_sig(role, raw_c);
            if !known_sigs.contains(&sig_raw) && !known_sigs.contains(&sig_full) {
                known_sigs.insert(sig_raw);
                known_sigs.insert(sig_full);
                entries.push(TranscriptEntry {
                    role: if role == "assistant" { "assistant" } else { "user" },
                    kind: if role == "assistant" { EntryKind::Assistant } else { EntryKind::User },
                    header: String::new(),
                    args_text: String::new(),
                    body_text: full_c.clone(),
                    reasoning_text: reason.clone(),
                });
            }
        }
        if let Some(fb) = fallback_history {
            for h in fb {
                let c = h.content.trim();
                if c.is_empty() || (h.role != "user" && h.role != "assistant") {
                    continue;
                }
                let sig = normalize_sig(&h.role, c);
                if !known_sigs.contains(&sig) {
                    known_sigs.insert(sig);
                    entries.push(TranscriptEntry {
                        role: if h.role == "assistant" { "assistant" } else { "user" },
                        kind: if h.role == "assistant" { EntryKind::Assistant } else { EntryKind::User },
                        header: String::new(),
                        args_text: String::new(),
                        body_text: c.to_string(),
                        reasoning_text: String::new(),
                    });
                }
            }
        }
        for m in &db_msgs {
            let c = m.content.trim();
            if c.is_empty() || (m.role != "user" && m.role != "assistant") {
                continue;
            }
            let sig = normalize_sig(&m.role, c);
            if !known_sigs.contains(&sig) {
                known_sigs.insert(sig);
                entries.push(TranscriptEntry {
                    role: if m.role == "assistant" { "assistant" } else { "user" },
                    kind: if m.role == "assistant" { EntryKind::Assistant } else { EntryKind::User },
                    header: String::new(),
                    args_text: String::new(),
                    body_text: c.to_string(),
                    reasoning_text: m.reasoning_content.clone().unwrap_or_default(),
                });
            }
        }
        // Clear unused warning if ckpt_msgs was modified nowhere else
        ckpt_msgs.clear();
    } else if !cached_ui.is_empty() {
        let mut known_sigs: HashSet<String> = HashSet::new();
        for (role, raw_c, full_c, reason) in &cached_ui {
            known_sigs.insert(normalize_sig(role, raw_c));
            known_sigs.insert(normalize_sig(role, full_c));
            entries.push(TranscriptEntry {
                role: if role == "assistant" { "assistant" } else { "user" },
                kind: if role == "assistant" { EntryKind::Assistant } else { EntryKind::User },
                header: String::new(),
                args_text: String::new(),
                body_text: full_c.clone(),
                reasoning_text: reason.clone(),
            });
        }
        if let Some(fb) = fallback_history {
            for h in fb {
                let c = h.content.trim();
                if c.is_empty() || (h.role != "user" && h.role != "assistant") {
                    continue;
                }
                let sig = normalize_sig(&h.role, c);
                if !known_sigs.contains(&sig) {
                    known_sigs.insert(sig);
                    entries.push(TranscriptEntry {
                        role: if h.role == "assistant" { "assistant" } else { "user" },
                        kind: if h.role == "assistant" { EntryKind::Assistant } else { EntryKind::User },
                        header: String::new(),
                        args_text: String::new(),
                        body_text: c.to_string(),
                        reasoning_text: String::new(),
                    });
                }
            }
        }
    } else if let Some(fb) = fallback_history.filter(|v| !v.is_empty()) {
        for h in fb {
            let c = h.content.trim();
            if c.is_empty() || (h.role != "user" && h.role != "assistant") {
                continue;
            }
            entries.push(TranscriptEntry {
                role: if h.role == "assistant" { "assistant" } else { "user" },
                kind: if h.role == "assistant" { EntryKind::Assistant } else { EntryKind::User },
                header: String::new(),
                args_text: String::new(),
                body_text: c.to_string(),
                reasoning_text: String::new(),
            });
        }
    } else {
        for m in &db_msgs {
            let c = m.content.trim();
            if c.is_empty() || (m.role != "user" && m.role != "assistant") {
                continue;
            }
            entries.push(TranscriptEntry {
                role: if m.role == "assistant" { "assistant" } else { "user" },
                kind: if m.role == "assistant" { EntryKind::Assistant } else { EntryKind::User },
                header: String::new(),
                args_text: String::new(),
                body_text: c.to_string(),
                reasoning_text: m.reasoning_content.clone().unwrap_or_default(),
            });
        }
    }

    // Append any currently streaming text/reasoning or in-flight running tool calls!
    if let Some(live) = live_opt {
        let mut live_parts: Vec<String> = Vec::new();
        if !live.streaming_text.trim().is_empty() {
            live_parts.push(format!(
                "[Streaming Partial Assistant Text]\n{}",
                smart_truncate_head_tail(live.streaming_text.trim(), 8000)
            ));
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        for rt in &live.running_tools {
            let elapsed_ms = (now_ms - rt.started_at_ms).max(0);
            let mut block = format!(
                "[TOOL CURRENTLY RUNNING NOW: {} | id={} | elapsed={}ms]",
                rt.name, rt.id, elapsed_ms
            );
            let args_t = rt.arguments.trim();
            if !args_t.is_empty() && args_t != "{}" {
                block.push_str(&format!("\nArguments:\n{}", smart_truncate_head_tail(args_t, 6000)));
            }
            live_parts.push(block);
        }
        if !live_parts.is_empty() || !live.streaming_reasoning.trim().is_empty() {
            entries.push(TranscriptEntry {
                role: "assistant",
                kind: EntryKind::LiveInProgress,
                header: "[LIVE IN-PROGRESS EXECUTION]".to_string(),
                args_text: String::new(),
                body_text: live_parts.join("\n\n"),
                reasoning_text: live.streaming_reasoning,
            });
        }
    }

    let caps = caps_reg.get(model_id);
    let reserved_out = caps.max_output_tokens.min(8192).max(2048);
    let token_budget = ((caps.context_window.saturating_sub(reserved_out) as usize) * 70 / 100)
        .clamp(24_000, 140_000);

    let metadata_block = build_session_metadata_block(db, chat_id);
    let budgeted = render_and_budget_entries(&metadata_block, &entries, token_budget);

    let mut msgs: Vec<ChatMessage> = Vec::new();
    msgs.push(ChatMessage::system(
        "You are answering a quick ephemeral side question (/btw) about the current session. \
         You have NO callable tools yourself in /btw mode (cannot read new files or run new commands), \
         HOWEVER the full session transcript below includes user messages, assistant responses, reasoning, \
         ALL tool calls with their exact arguments, ALL tool outputs/logs/errors, session artifacts, \
         and any currently running in-flight tools or partial streaming output. \
         Answer concisely, directly, and accurately using ALL the provided conversation and tool context. \
         Cite specific file paths, commands, diffs, and tool outputs from the context when relevant. \
         Only say information is absent if it genuinely does not appear anywhere in the provided transcript or tool outputs."
            .to_string(),
    ));

    // Coalesce consecutive same-role messages so every LLM provider receives valid alternating turns.
    for (role, text) in budgeted {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(last) = msgs.last_mut() {
            if last.role == role {
                let mut prev = last.content.as_text_lossy();
                if !prev.is_empty() {
                    prev.push_str("\n\n");
                }
                prev.push_str(trimmed);
                last.content = crate::llm::request_compat::MessageContent::Text(prev);
                continue;
            }
        }
        if role == "assistant" {
            msgs.push(ChatMessage::assistant(trimmed.to_string()));
        } else {
            msgs.push(ChatMessage::user(trimmed.to_string()));
        }
    }

    // Ensure the final /btw user question is cleanly separated as a user message.
    if let Some(last) = msgs.last_mut() {
        if last.role == "user" {
            let mut prev = last.content.as_text_lossy();
            prev.push_str("\n\n---\n[/btw Side Question]\n");
            prev.push_str(question);
            last.content = crate::llm::request_compat::MessageContent::Text(prev);
            return msgs;
        }
    }
    msgs.push(ChatMessage::user(question.to_string()));
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::request_compat::{FunctionCall, ToolCall};

    #[tokio::test]
    async fn btw_includes_tool_calls_outputs_and_live_running_tools() {
        let dir = std::env::temp_dir().join(format!("omni_btw_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::new(&dir).unwrap();
        let stream_mgr = StreamManager::new();
        let caps_reg = CapabilityRegistry::new();

        let chat_id = "chat-btw-live-test";
        let now = chrono::Utc::now().to_rfc3339();
        db.create_chat(&crate::db::chats::Chat {
            id: chat_id.to_string(),
            project_id: Some("C:/proj".to_string()),
            title: "Test Session".to_string(),
            model_id: "gpt-5.4".to_string(),
            system_prompt: String::new(),
            is_pinned: false,
            created_at: now.clone(),
            updated_at: now,
        })
        .unwrap();

        let session = stream_mgr.create_session(chat_id).await;
        session.set_live_history(vec![
            ChatMessage::system("sys".to_string()),
            ChatMessage::user("Run acceptance tests and check SyntaxInjector".to_string()),
            ChatMessage {
                role: "assistant".to_string(),
                content: crate::llm::request_compat::MessageContent::Text(String::new()),
                name: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "terminal_exec".to_string(),
                        arguments: r#"{"command":"python run_acceptance_test.py --phase 1"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
                reasoning_content: Some("Need to inspect phase 1 exit status".to_string()),
            },
            ChatMessage {
                role: "tool".to_string(),
                content: crate::llm::request_compat::MessageContent::Text(
                    "PHASE 1 PASSED: 14/14 checks OK, SyntaxInjector hooked PID 7080".to_string(),
                ),
                name: Some("terminal_exec".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
                reasoning_content: None,
            },
        ]);
        session.set_streaming_buffers("Analyzing phase 2 results...", "Checking WaitingJob dump");
        session.add_running_tool(
            "call_2".to_string(),
            "terminal_exec".to_string(),
            r#"{"command":"python run_acceptance_test.py --scan-only"}"#.to_string(),
        );

        let msgs = build_btw_messages(
            &db,
            &stream_mgr,
            chat_id,
            "gpt-5.4",
            &caps_reg,
            None,
            "What did phase 1 output and what is running right now?",
        )
        .await;

        let combined: String = msgs
            .iter()
            .map(|m| format!("[{}]\n{}", m.role, m.content.as_text_lossy()))
            .collect::<Vec<_>>()
            .join("\n---\n");

        assert!(combined.contains("terminal_exec"), "missing tool name: {}", combined);
        assert!(combined.contains("run_acceptance_test.py --phase 1"), "missing tool args: {}", combined);
        assert!(combined.contains("PHASE 1 PASSED: 14/14 checks OK"), "missing tool output: {}", combined);
        assert!(combined.contains("TOOL CURRENTLY RUNNING NOW: terminal_exec"), "missing live tool: {}", combined);
        assert!(combined.contains("--scan-only"), "missing live tool args: {}", combined);
        assert!(combined.contains("Analyzing phase 2 results..."), "missing streaming text: {}", combined);
        assert!(msgs.iter().all(|m| m.role != "tool" && m.tool_calls.is_none()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn btw_reads_full_ctx_after_resume_checkpoint_deleted_and_merges_cached_steps() {
        let dir = std::env::temp_dir().join(format!("omni_btw_cache_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::new(&dir).unwrap();
        let stream_mgr = StreamManager::new();
        let caps_reg = CapabilityRegistry::new();

        let chat_id = "chat-btw-cache";
        let cached_ui = serde_json::json!([
            { "id": "u1", "role": "user", "content": "Check file foo.rs" },
            {
                "id": "a1",
                "role": "assistant",
                "content": "Done checking foo.rs.",
                "steps": [
                    {
                        "kind": "tool",
                        "record": {
                            "toolCallId": "tc_read_1",
                            "toolName": "read_file",
                            "rawArgs": "{\"path\":\"src/foo.rs\"}",
                            "status": "success",
                            "elapsedMs": 12,
                            "output": "fn magic_constant() -> i32 { 4242 }"
                        }
                    },
                    { "kind": "message", "text": "Done checking foo.rs." }
                ]
            }
        ])
        .to_string();
        db.sync_chat_context(chat_id, "Cache Test", &cached_ui).unwrap();

        // Simulate Turn 2 checkpoint on Desktop where Turn 1 was seeded as summary text only
        let turn2_history = vec![
            ChatMessage::user("Check file foo.rs".to_string()),
            ChatMessage::assistant("Done checking foo.rs.".to_string()),
            ChatMessage::user("Now check bar.rs".to_string()),
        ];
        db.save_checkpoint(&format!("full_ctx:{}", chat_id), &serde_json::to_string(&turn2_history).unwrap())
            .unwrap();

        let msgs = build_btw_messages(
            &db,
            &stream_mgr,
            chat_id,
            "gpt-5.4",
            &caps_reg,
            None,
            "What constant was in foo.rs?",
        )
        .await;

        let combined: String = msgs
            .iter()
            .map(|m| m.content.as_text_lossy())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(combined.contains("read_file"), "missing enriched Turn 1 tool: {}", combined);
        assert!(combined.contains("4242"), "missing enriched Turn 1 tool output: {}", combined);
        assert!(combined.contains("Now check bar.rs"), "missing Turn 2 prompt: {}", combined);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
