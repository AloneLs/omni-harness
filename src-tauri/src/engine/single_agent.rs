use crate::engine::compaction::ContextCompactor;
use crate::engine::context_engine::{
    cleanup_old_tmp_logs, compute_prompt_cache_key, count_context_tokens,
    safe_slice_head, spill_tool_output_if_needed, spill_tool_output_with_path, CompactionCursor, ContextBuilder,
};
use crate::engine::prompt_builder::PromptBuilder;
use crate::engine::role_permissions::{AgentRole, RolePermissionEngine};
use crate::engine::stream_manager::SessionHandle;
use crate::engine::task_contract::{
    AcceptanceCriterion, ContractStatus, CriterionStatus, StructuredVerdict, TaskContract, TaskStateEnum, VerdictStatus,
};
use crate::llm::capabilities::{CapabilityRegistry, ModelFamily};
use crate::llm::key_pool::KeyPool;
use crate::llm::request_compat::{ChatMessage, ChatRequest, ContentPart, FunctionSpec, ImageUrlDetail, MessageContent, ToolCall, ToolDefinition};
use crate::llm::{dispatch_stream_with_protocol, StreamEvent, TokenCountRouter, TokenUsage};
use crate::tools::ToolDispatcher;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct SingleAgentRunner;

impl SingleAgentRunner {
    fn load_plan_tuple(db: Option<&crate::db::Database>, chat_id: &str) -> Option<(bool, String)> {
        db.and_then(|d| d.load_plan_state(chat_id).ok().flatten())
    }

    fn collect_restored_files(
        db: Option<&crate::db::Database>,
        workspace_root: &str,
        chat_id: &str,
    ) -> Vec<(String, String)> {
        let Some(d) = db else {
            return Vec::new();
        };
        let Ok(paths) = d.list_recent_artifacts(chat_id, "file", 5) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut total_chars: usize = 0;
        let max_total_chars: usize = 175_000;
        for raw_p in paths {
            let candidate = std::path::Path::new(&raw_p);
            let full = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                std::path::Path::new(workspace_root).join(candidate)
            };
            if let Ok(content) = std::fs::read_to_string(&full) {
                let capped = if content.chars().count() > 40_000 {
                    format!("{}... [truncated]", safe_slice_head(&content, 40_000))
                } else {
                    content
                };
                let capped_chars = capped.chars().count();
                if total_chars + capped_chars > max_total_chars {
                    break;
                }
                total_chars += capped_chars;
                out.push((raw_p, capped));
            }
        }
        out
    }



    async fn run_live_terminal_exec(
        args: &Value,
        tools: &ToolDispatcher,
        workspace_root: &str,
        chat_id: &str,
        session: &SessionHandle,
        call_id: &str,
        event_sender: &mpsc::Sender<StreamEvent>,
    ) -> (bool, String, Option<String>) {
        if let Some(obj) = args.as_object() {
            let allowed = [
                "command",
                "description",
                "cwd",
                "wait_ms_before_async",
                "timeout_secs",
                "background",
                "is_daemon",
            ];
            let unknown: Vec<&str> = obj.keys().map(|k| k.as_str()).filter(|k| !allowed.contains(k)).collect();
            if !unknown.is_empty() {
                let err_msg = format!("Invalid argument: unknown parameter(s): {}", unknown.join(", "));
                return (false, serde_json::json!({ "error": err_msg }).to_string(), Some(err_msg));
            }
        }
        let command = match args.get("command").and_then(|v| v.as_str()) {
            Some(c) if !c.trim().is_empty() => c,
            _ => return (false, serde_json::json!({ "error": "Missing command" }).to_string(), Some("Missing command".to_string())),
        };
        if let Err(e) = crate::tools::safety::ShellPolicyGuard::classify(command, tools.is_plan_mode()) {
            return (false, serde_json::json!({ "error": e }).to_string(), Some(e));
        }

        let base_wd = if !workspace_root.trim().is_empty() {
            std::path::Path::new(workspace_root)
        } else {
            tools.working_dir.as_path()
        };

        let cwd_validated = match args.get("cwd") {
            Some(v) => match v.as_str() {
                Some(s) if !s.trim().is_empty() => {
                    match crate::tools::path_norm::validate_path_with_base(s, Some(base_wd), &tools.allowed_roots) {
                        Ok(p) => p,
                        Err(e) => return (false, serde_json::json!({ "error": e }).to_string(), Some(e)),
                    }
                }
                _ => {
                    let err_msg = "Invalid argument: cwd must be a non-empty string".to_string();
                    return (false, serde_json::json!({ "error": err_msg }).to_string(), Some(err_msg));
                }
            },
            None => crate::tools::path_norm::strip_verbatim_prefix(base_wd),
        };
        if !cwd_validated.is_dir() {
            let err_msg = format!("Invalid cwd: directory '{}' does not exist", cwd_validated.display());
            return (false, serde_json::json!({ "error": err_msg }).to_string(), Some(err_msg));
        }
        let cwd_str = cwd_validated.to_string_lossy().to_string();

        let background = args
            .get("background")
            .or_else(|| args.get("is_daemon"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let wait_ms = if background {
            500
        } else if let Some(ms) = args.get("wait_ms_before_async").and_then(|v| v.as_u64()) {
            ms.clamp(200, 30_000)
        } else if let Some(secs) = args.get("timeout_secs").and_then(|v| v.as_u64()) {
            (secs.saturating_mul(1000)).clamp(500, 10_000)
        } else {
            8_000
        };

        let task_id = match tools.pty_manager.spawn_managed_task(
            command,
            &cwd_str,
            Some(chat_id),
            Some((event_sender.clone(), call_id.to_string())),
        ) {
            Ok(id) => id,
            Err(e) => return (false, serde_json::json!({ "error": e }).to_string(), Some(e)),
        };

        let outcome = match tools
            .pty_manager
            .wait_or_background(&task_id, wait_ms, Some(session.cancel_receiver()))
            .await
        {
            Ok(o) => o,
            Err(e) => return (false, serde_json::json!({ "error": e }).to_string(), Some(e)),
        };

        match outcome {
            crate::tools::process_pty::TaskExecOutcome::Canceled { duration_ms } => {
                let err_msg = "User canceled execution".to_string();
                (
                    false,
                    serde_json::json!({ "error": err_msg, "duration_ms": duration_ms }).to_string(),
                    Some(err_msg),
                )
            }
            crate::tools::process_pty::TaskExecOutcome::Backgrounded {
                task_id,
                output_so_far,
                duration_ms,
            } => {
                let (bounded, spill_file) = spill_tool_output_with_path(workspace_root, call_id, &output_so_far);
                let out_json = serde_json::json!({
                    "task_id": task_id,
                    "session_id": task_id,
                    "status": "running",
                    "running": true,
                    "background": true,
                    "duration_ms": duration_ms,
                    "output": bounded,
                    "spill_file": spill_file,
                    "hint": "Command is still running in background. Use manage_task(action='status'|'send_input'|'kill', task_id=...) to inspect output, send input, or stop it.",
                });
                (true, out_json.to_string(), None)
            }
            crate::tools::process_pty::TaskExecOutcome::Completed {
                task_id,
                exit_code,
                stdout,
                stderr,
                output,
                duration_ms,
            } => {
                let (bounded_out, spill_file) = spill_tool_output_with_path(workspace_root, call_id, &output);
                let (bounded_stdout, _) = spill_tool_output_with_path(workspace_root, &format!("{}_out", call_id), &stdout);
                let (bounded_stderr, _) = spill_tool_output_with_path(workspace_root, &format!("{}_err", call_id), &stderr);

                if exit_code != 0 {
                    let err_msg = format!("Command exited with non-zero code {}", exit_code);
                    let out_json = serde_json::json!({
                        "task_id": task_id,
                        "status": "completed",
                        "stdout": bounded_stdout,
                        "stderr": bounded_stderr,
                        "output": bounded_out,
                        "exit_code": exit_code,
                        "duration_ms": duration_ms,
                        "running": false,
                        "timed_out": false,
                        "spill_file": spill_file,
                        "error": err_msg,
                    });
                    return (false, out_json.to_string(), Some(err_msg));
                }

                let out_json = serde_json::json!({
                    "task_id": task_id,
                    "status": "completed",
                    "stdout": bounded_stdout,
                    "stderr": bounded_stderr,
                    "output": bounded_out,
                    "exit_code": 0,
                    "duration_ms": duration_ms,
                    "running": false,
                    "timed_out": false,
                    "spill_file": spill_file,
                });
                (true, out_json.to_string(), None)
            }
        }
    }

    fn handle_task_board(
        args: &Value,
        contract: &mut Option<TaskContract>,
    ) -> (Value, Value) {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("list");
        let parse_status = |s: &str| -> CriterionStatus {
            match s.to_lowercase().as_str() {
                "in_progress" | "active" | "running" => CriterionStatus::InProgress,
                "completed" | "done" | "pass" => CriterionStatus::Pass,
                "failed" | "fail" | "error" => CriterionStatus::Fail,
                "blocked" => CriterionStatus::Blocked,
                _ => CriterionStatus::Pending,
            }
        };
        let status_str = |st: CriterionStatus| -> &'static str {
            match st {
                CriterionStatus::InProgress => "in_progress",
                CriterionStatus::Pass => "completed",
                CriterionStatus::Fail => "failed",
                CriterionStatus::Blocked => "blocked",
                _ => "pending",
            }
        };

        match action {
            "set" | "plan" => {
                let goal = args
                    .get("goal")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Execution Plan")
                    .to_string();
                let mut criteria = Vec::new();
                if let Some(arr) = args.get("items").and_then(|v| v.as_array()) {
                    for (i, item) in arr.iter().enumerate() {
                        let id = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("step-{}", i + 1));
                        let text = item
                            .get("text")
                            .or_else(|| item.get("title"))
                            .or_else(|| item.get("description"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let st = item
                            .get("status")
                            .and_then(|v| v.as_str())
                            .map(parse_status)
                            .unwrap_or(if i == 0 { CriterionStatus::InProgress } else { CriterionStatus::Pending });
                        if !text.is_empty() {
                            criteria.push(AcceptanceCriterion {
                                id,
                                description: text,
                                status: st,
                                evidence: None,
                            });
                        }
                    }
                }
                *contract = Some(TaskContract {
                    goal,
                    status: ContractStatus::Active,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    criteria,
                    definition_of_done: Vec::new(),
                    verdict: None,
                });
            }
            "add" => {
                let text = args
                    .get("title")
                    .or_else(|| args.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !text.is_empty() {
                    let c = contract.get_or_insert_with(|| TaskContract {
                        goal: "Execution Plan".to_string(),
                        status: ContractStatus::Active,
                        created_at: chrono::Utc::now().to_rfc3339(),
                        criteria: Vec::new(),
                        definition_of_done: Vec::new(),
                        verdict: None,
                    });
                    let id = args
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("step-{}", c.criteria.len() + 1));
                    c.criteria.push(AcceptanceCriterion {
                        id,
                        description: text,
                        status: CriterionStatus::Pending,
                        evidence: None,
                    });
                }
            }
            "update" | "toggle" => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let new_st = args.get("status").and_then(|v| v.as_str()).map(parse_status);
                if let Some(c) = contract.as_mut() {
                    if let Some(item) = c.criteria.iter_mut().find(|cr| cr.id == id) {
                        item.status = new_st.unwrap_or(if item.status == CriterionStatus::Pass {
                            CriterionStatus::Pending
                        } else {
                            CriterionStatus::Pass
                        });
                    }
                }
            }
            _ => {}
        }

        let items_json: Vec<Value> = contract
            .as_ref()
            .map(|c| {
                c.criteria
                    .iter()
                    .map(|cr| {
                        serde_json::json!({
                            "id": cr.id,
                            "text": cr.description,
                            "status": status_str(cr.status),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let arr_val = Value::Array(items_json);
        (
            serde_json::json!({ "action": action, "items": arr_val.clone() }),
            arr_val,
        )
    }

    fn is_concurrency_safe_tool(name: &str) -> bool {
        matches!(
            name,
            "read_file" | "glob" | "grep" | "code_intel" | "web_research" | "tool_hub" | "memory_tool"
        )
    }

    async fn run_cache_safe_compaction(
        client: &reqwest::Client,
        provider_name: &str,
        base_url: &str,
        api_protocol: Option<&str>,
        key_pool: &KeyPool,
        caps_registry: &CapabilityRegistry,
        model_name: &str,
        cache_key: &str,
        tools: &[ToolDefinition],
        projection: &[ChatMessage],
        custom_instruction: Option<&str>,
    ) -> Result<String, String> {
        let mut fork_msgs = projection.to_vec();
        ContextBuilder::strip_all_media_for_compaction(&mut fork_msgs);
        fork_msgs.push(ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text(ContextBuilder::build_compaction_prompt(custom_instruction)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        });

        let req = ChatRequest {
            model: model_name.to_string(),
            messages: fork_msgs,
            temperature: Some(0.1),
            top_p: Some(0.95),
            max_tokens: None,
            max_completion_tokens: None,
            tools: if tools.is_empty() { None } else { Some(tools.to_vec()) },
            tool_choice: None,
            reasoning_effort: None,
            thinking_budget: None,
            response_format: None,
            prompt_cache_key: Some(cache_key.to_string()),
            stream: true,
        };

        let (tx, mut rx) = mpsc::channel(64);
        let p = provider_name.to_string();
        let b = base_url.to_string();
        let ap = api_protocol.map(|s| s.to_string());
        let kp = key_pool.clone();
        let cr = caps_registry.clone();
        let c = client.clone();

        let join = tokio::spawn(async move {
            dispatch_stream_with_protocol(&c, &p, &b, ap.as_deref(), &kp, &cr, req, tx).await
        });

        let mut summary = String::new();
        while let Some(evt) = rx.recv().await {
            if let StreamEvent::ContentDelta { delta } = evt {
                summary.push_str(&delta);
            }
        }
        let res = join.await.map_err(|e| e.to_string())?;
        res.map_err(|e| e.to_string())?;
        if summary.trim().is_empty() {
            return Err("Empty compaction summary".to_string());
        }
        Ok(summary)
    }

    fn save_checkpoint(db: Option<&crate::db::Database>, chat_id: &str, history: &[ChatMessage]) {
        if let Some(d) = db {
            if let Ok(json) = serde_json::to_string(history) {
                let _ = d.save_checkpoint(chat_id, &json);
            }
        }
    }

    fn sanitize_resumed_history(history: &mut Vec<ChatMessage>) {
        let mut active_valid_ids = std::collections::HashSet::<String>::new();
        let mut cleaned: Vec<ChatMessage> = Vec::with_capacity(history.len());
        let mut idx = 0usize;

        while idx < history.len() {
            let msg = &history[idx];
            if msg.role == "tool" {
                let valid = msg
                    .tool_call_id
                    .as_ref()
                    .map(|tid| active_valid_ids.remove(tid))
                    .unwrap_or(false);
                if valid {
                    cleaned.push(msg.clone());
                }
                idx += 1;
                continue;
            }

            if !active_valid_ids.is_empty() {
                active_valid_ids.clear();
            }

            if msg.role == "assistant" {
                cleaned.push(msg.clone());
                if let Some(ref tcs) = msg.tool_calls {
                    if !tcs.is_empty() {
                        let mut existing_ids = std::collections::HashSet::new();
                        let mut j = idx + 1;
                        while j < history.len() && history[j].role == "tool" {
                            if let Some(ref tid) = history[j].tool_call_id {
                                existing_ids.insert(tid.clone());
                            }
                            j += 1;
                        }
                        for tc in tcs {
                            active_valid_ids.insert(tc.id.clone());
                        }
                        idx += 1;
                        while idx < j {
                            let tmsg = &history[idx];
                            if tmsg
                                .tool_call_id
                                .as_ref()
                                .map(|tid| active_valid_ids.remove(tid))
                                .unwrap_or(false)
                            {
                                cleaned.push(tmsg.clone());
                            }
                            idx += 1;
                        }
                        for tc in tcs {
                            if !existing_ids.contains(&tc.id) {
                                active_valid_ids.remove(&tc.id);
                                cleaned.push(ChatMessage {
                                    role: "tool".to_string(),
                                    content: MessageContent::Text("[Interrupted before tool finished]".to_string()),
                                    name: Some(tc.function.name.clone()),
                                    tool_calls: None,
                                    tool_call_id: Some(tc.id.clone()),
                                    reasoning_content: None,
                                });
                            }
                        }
                        continue;
                    }
                }
                idx += 1;
                continue;
            }

            cleaned.push(msg.clone());
            idx += 1;
        }

        *history = cleaned;
    }

    pub async fn run_turn(
        http_client: &reqwest::Client,
        chat_id: &str,
        user_prompt: &str,
        workspace_root: &str,
        tools: Arc<ToolDispatcher>,
        key_pool: &KeyPool,
        caps_registry: &CapabilityRegistry,
        provider_name: &str,
        base_url: &str,
        api_protocol: Option<&str>,
        model_name: &str,
        reasoning_effort_level: &str,
        conn: Option<&std::sync::Mutex<rusqlite::Connection>>,
        db: Option<&crate::db::Database>,
        resume: bool,
        session: &SessionHandle,
        contract: &mut Option<TaskContract>,
        history: &mut Vec<ChatMessage>,
        event_sender: mpsc::Sender<StreamEvent>,
    ) -> Result<TaskStateEnum, String> {
        let mut state = TaskStateEnum::Created;
        let _ = state.transition(TaskStateEnum::Planning);

        if history.is_empty() {
            let mut sys_prompt = PromptBuilder::build_system_prompt(
                &AgentRole::Main,
                workspace_root,
                None,
                None,
                Some(model_name),
            );
            if let Some(d) = db {
                if let Ok(entries) = d.list_memory_entries(None) {
                    if !entries.is_empty() {
                        sys_prompt.push_str("\n--- LONG-TERM MEMORY (memory_entries) ---\n");
                        let mut used_chars = 0usize;
                        for e in entries.into_iter().take(25) {
                            let line = format!("- [{} / {}]: {}\n", e.category, e.key, e.content.trim());
                            if used_chars + line.len() > 2500 {
                                break;
                            }
                            used_chars += line.len();
                            sys_prompt.push_str(&line);
                        }
                    }
                }
            }
            history.push(ChatMessage::system(sys_prompt));
        }

        let ws_gc = workspace_root.to_string();
        tokio::spawn(async move {
            cleanup_old_tmp_logs(&ws_gc, 24);
        });

        let caps = caps_registry.get(model_name);
        let cache_key = compute_prompt_cache_key(workspace_root, chat_id);
        let preserve_past_reasoning = caps.family == ModelFamily::Anthropic;
        let mut compaction_cursor = CompactionCursor::default();

        if let Some(d) = db {
            if tools.is_plan_mode() {
                let prev_md = Self::load_plan_tuple(db, chat_id).map(|(_, m)| m).unwrap_or_default();
                let _ = d.save_plan_state(chat_id, true, &prev_md);
            } else if resume {
                if let Some((pm, _)) = Self::load_plan_tuple(db, chat_id) {
                    if pm {
                        tools.set_plan_mode(true);
                    }
                }
            }
        }

        let tool_decls = tools.get_all_tool_declarations();
        let mut available_tools = Vec::new();
        for decl in tool_decls {
            if RolePermissionEngine::is_tool_allowed(&AgentRole::Main, &decl.name) {
                available_tools.push(ToolDefinition {
                    tool_type: "function".to_string(),
                    function: FunctionSpec {
                        name: decl.name,
                        description: decl.description,
                        parameters: serde_json::to_value(&decl.parameters)
                            .unwrap_or(serde_json::json!({})),
                    },
                });
            }
        }

        let trimmed_prompt = user_prompt.trim();
        if trimmed_prompt.starts_with("/compact") {
            let custom_instr = trimmed_prompt.strip_prefix("/compact").map(|s| s.trim());
            let before_tokens = count_context_tokens(model_name, caps.family, history, &available_tools, db);
            let (proj, _) = ContextBuilder::build_projection(
                history,
                workspace_root,
                contract.as_ref(),
                caps.context_window,
                None,
                preserve_past_reasoning,
                &mut compaction_cursor,
                false,
            );
            let summary = Self::run_cache_safe_compaction(
                http_client,
                provider_name,
                base_url,
                api_protocol,
                key_pool,
                caps_registry,
                model_name,
                &cache_key,
                &available_tools,
                &proj,
                custom_instr,
            )
            .await
            .unwrap_or_else(|_| {
                let (compacted, _) = ContextCompactor::compact_history(proj.clone(), 4);
                compacted
                    .iter()
                    .find(|m| m.name.as_deref() == Some("context_compact_summary"))
                    .map(|m| m.content.as_text_lossy())
                    .unwrap_or_else(|| "Context compacted.".to_string())
            });
            if let Some(d) = db {
                let _ = d.save_session_memory(chat_id, &summary, history.len());
            }
            let plan_state = Self::load_plan_tuple(db, chat_id);
            let restored_files = Self::collect_restored_files(db, workspace_root, chat_id);
            let rehydrated = ContextBuilder::rehydrate_after_compaction(
                history,
                &summary,
                workspace_root,
                contract.as_ref(),
                4,
                plan_state,
                restored_files,
            );
            let after_tokens = count_context_tokens(model_name, caps.family, &rehydrated, &available_tools, db);
            if let Some(d) = db {
                let _ = d.record_compaction_event(chat_id, "tier4_manual", before_tokens, after_tokens, "Manual /compact");
            }
            *history = rehydrated;
            let _ = event_sender
                .send(StreamEvent::ContextCompacted {
                    before_tokens,
                    after_tokens,
                    summary,
                })
                .await;
            let _ = event_sender
                .send(StreamEvent::ContextTelemetry {
                    used_tokens: after_tokens,
                    context_window: caps.context_window,
                    cached_tokens: 0,
                    cache_hit_rate: 0.0,
                })
                .await;
            return Ok(TaskStateEnum::Completed);
        }

        if !history.is_empty() {
            Self::sanitize_resumed_history(history);
        }
        if !resume {
            let mut final_prompt = user_prompt.to_string();
            let all_skills = crate::skills::discover_all_skills(workspace_root);
            let mut injected_blocks: Vec<String> = Vec::new();
            let mut injected_names: std::collections::HashSet<String> = std::collections::HashSet::new();

            if let Some(crate::engine::commands::SlashCommand::SkillInvoke { skill_name, args }) =
                crate::engine::commands::SlashCommandParser::parse(trimmed_prompt)
            {
                let target_clean = skill_name.trim_start_matches('/').to_ascii_lowercase();
                if let Some(sk) = all_skills.iter().find(|s| {
                    s.metadata.enabled
                        && (s.metadata.name.to_ascii_lowercase() == target_clean
                            || s.metadata
                                .command
                                .as_deref()
                                .map(|c| c.trim_start_matches('/').to_ascii_lowercase() == target_clean)
                                .unwrap_or(false))
                }) {
                    injected_names.insert(sk.metadata.name.clone());
                    injected_blocks.push(crate::skills::render_skill_activation(sk, Some(&args)));
                }
            }

            let prompt_lower = trimmed_prompt.to_lowercase();
            for sk in &all_skills {
                if !sk.metadata.enabled || injected_names.contains(&sk.metadata.name) {
                    continue;
                }
                let dollar_token = format!("${}", sk.metadata.name.to_ascii_lowercase());
                let matched_dollar = prompt_lower.split_whitespace().any(|w| {
                    let clean = w.trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != ':');
                    clean == dollar_token
                });
                if matched_dollar {
                    injected_names.insert(sk.metadata.name.clone());
                    injected_blocks.push(crate::skills::render_skill_activation(sk, Some(trimmed_prompt)));
                }
            }
            for sk in &all_skills {
                if !sk.metadata.enabled || sk.metadata.disable_model_invocation || injected_names.contains(&sk.metadata.name) {
                    continue;
                }
                let already_in_ctx = history.iter().any(|m| {
                    m.content.as_text_lossy().contains(&format!("<skill_content name=\"{}\">", sk.metadata.name))
                });
                if already_in_ctx {
                    continue;
                }
                let matched_trigger = sk
                    .metadata
                    .triggers
                    .iter()
                    .any(|t| !t.trim().is_empty() && prompt_lower.contains(&t.to_lowercase()));
                if matched_trigger {
                    injected_names.insert(sk.metadata.name.clone());
                    injected_blocks.push(crate::skills::render_skill_activation(sk, None));
                }
            }

            if !injected_blocks.is_empty() {
                final_prompt = format!("{}\n\n{}", injected_blocks.join("\n\n"), final_prompt);
            }
            let has_pending_parts = history
                .last()
                .map(|m| m.role == "user" && matches!(m.content, MessageContent::Parts(_)))
                .unwrap_or(false);
            if has_pending_parts {
                if let Some(last_u) = history.last_mut() {
                    if let MessageContent::Parts(ref mut parts) = last_u.content {
                        if let Some(crate::llm::request_compat::ContentPart::Text { text }) = parts
                            .iter_mut()
                            .find(|pt| matches!(pt, crate::llm::request_compat::ContentPart::Text { .. }))
                        {
                            *text = final_prompt;
                        } else {
                            parts.insert(0, crate::llm::request_compat::ContentPart::Text { text: final_prompt });
                        }
                    }
                }
            } else {
                history.push(ChatMessage::user(final_prompt));
            }
        }
        Self::save_checkpoint(db, chat_id, history);


        let mut last_request_ts: Option<i64> = None;
        let mut last_call_sig = String::new();
        let mut same_call_streak: u32 = 0;
        let mut last_err_sig = String::new();
        let mut same_err_streak: u32 = 0;
        let mut consecutive_compact_failures: u32 = 0;
        let mut reactive_compact_count: u32 = 0;
        let mut empty_retries: u32 = 0;
        let mut incomplete_todo_nudges: u32 = 0;
        let mut plan_mode_entries: u32 = if tools.is_plan_mode() { 1 } else { 0 };
        let mut turn_seq: i64 = 0;
        crate::engine::policy_guard::audit_event(chat_id, "turn_started", "");
        crate::engine::policy_guard::audit_event(chat_id, "user_message_received", "");

        let _ = state.transition(TaskStateEnum::Executing);

        while session.is_active() {
            turn_seq += 1;
            let mut turn_checkpoint_created = false;

            if let Some(intervention) = session.check_intervention().await {
                history.push(ChatMessage::user(format!("<user_steering>{}</user_steering>", intervention)));
            }

            let (mut projection, _pruned) = ContextBuilder::build_projection(
                history,
                workspace_root,
                contract.as_ref(),
                caps.context_window,
                last_request_ts,
                preserve_past_reasoning,
                &mut compaction_cursor,
                true,
            );
            let ext_changed = tools.read_guard.scan_external_changes();
            if !ext_changed.is_empty() {
                if let Some(d) = db {
                    for p in &ext_changed {
                        d.emit_event(chat_id, "dirty_external", serde_json::json!({ "path": p }));
                    }
                }
                projection.push(ChatMessage {
                    role: "user".to_string(),
                    content: MessageContent::Text(format!(
                        "<system-reminder>\nNote: The following previously read file(s) changed on disk (modified externally by user, formatter, or another process):\n{}\nRe-read with read_file if needed and do not revert these changes unless asked.\n</system-reminder>",
                        ext_changed.join("\n")
                    )),
                    name: Some("ephemeral_file_watch".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                });
            }

            let mut est_used = count_context_tokens(model_name, caps.family, &projection, &available_tools, db);
            if ContextBuilder::needs_tier1_masking(est_used, caps.context_window, caps.max_output_tokens) {
                let t1_changed = ContextBuilder::micro_prune(history, 20_000, 6).is_some();
                let t2_changed = ContextBuilder::tier2_dedup_and_clean(history);
                if t1_changed || t2_changed {
                    let (reproj, _) = ContextBuilder::build_projection(
                        history,
                        workspace_root,
                        contract.as_ref(),
                        caps.context_window,
                        last_request_ts,
                        preserve_past_reasoning,
                        &mut compaction_cursor,
                        true,
                    );
                    projection = reproj;
                    let after_t12 = count_context_tokens(model_name, caps.family, &projection, &available_tools, db);
                    if let Some(d) = db {
                        let tier_label = if t2_changed { "tier2_dedup" } else { "tier1_masking" };
                        let _ = d.record_compaction_event(chat_id, tier_label, est_used, after_t12, "Zero-LLM pruning");
                    }
                    est_used = after_t12;
                }
            }

            if let Ok(peek_key) = key_pool.next_key(provider_name) {
                let gz = TokenCountRouter::count_with_gray_zone(
                    http_client,
                    base_url,
                    &peek_key,
                    model_name,
                    caps.family,
                    caps.context_window,
                    &projection,
                    &available_tools,
                    db,
                )
                .await;
                est_used = gz.total;
            }

            if consecutive_compact_failures < 3
                && ContextBuilder::needs_auto_compaction_exact(
                    est_used,
                    caps.context_window,
                    caps.max_output_tokens,
                )
            {
                let before_tokens = est_used;
                let cached_mem = db
                    .and_then(|d| d.load_session_memory(chat_id).ok().flatten())
                    .filter(|(m, cnt)| !m.trim().is_empty() && cnt.saturating_add(8) >= history.len());

                let (tier_name, summary) = if let Some((mem_md, _)) = cached_mem {
                    ("tier3_session_memory", mem_md)
                } else {
                    let res = Self::run_cache_safe_compaction(
                        http_client,
                        provider_name,
                        base_url,
                        api_protocol,
                        key_pool,
                        caps_registry,
                        model_name,
                        &cache_key,
                        &available_tools,
                        &projection,
                        None,
                    )
                    .await;
                    let s = match res {
                        Ok(s) => {
                            consecutive_compact_failures = 0;
                            if let Some(d) = db {
                                let _ = d.save_session_memory(chat_id, &s, history.len());
                            }
                            s
                        }
                        Err(_) => {
                            consecutive_compact_failures += 1;
                            let (compacted, _) = ContextCompactor::compact_history(projection.clone(), 4);
                            compacted
                                .iter()
                                .find(|m| m.name.as_deref() == Some("context_compact_summary"))
                                .map(|m| m.content.as_text_lossy())
                                .unwrap_or_else(|| "Context compacted.".to_string())
                        }
                    };
                    ("tier4_full", s)
                };

                let plan_state = Self::load_plan_tuple(db, chat_id);
                let restored_files = Self::collect_restored_files(db, workspace_root, chat_id);
                let rehydrated = ContextBuilder::rehydrate_after_compaction(
                    history,
                    &summary,
                    workspace_root,
                    contract.as_ref(),
                    4,
                    plan_state,
                    restored_files,
                );
                let after_tokens = count_context_tokens(model_name, caps.family, &rehydrated, &available_tools, db);
                if let Some(d) = db {
                    let _ = d.record_compaction_event(chat_id, tier_name, before_tokens, after_tokens, "Auto escalation compaction");
                }
                *history = rehydrated;
                compaction_cursor = CompactionCursor::default();
                projection = history.clone();
                est_used = after_tokens;

                let _ = event_sender
                    .send(StreamEvent::ContextCompacted {
                        before_tokens,
                        after_tokens,
                        summary,
                    })
                    .await;
            }

            let (eff_opt, budget_opt) = match reasoning_effort_level.trim().to_uppercase().as_str() {
                "LOW" => (Some("low".to_string()), Some(2048)),
                "MEDIUM" | "MED" => (Some("medium".to_string()), Some(8192)),
                "HIGH" => (Some("high".to_string()), Some(16384)),
                "MAX" => (Some("high".to_string()), Some(32768)),
                _ => (None, None),
            };
            Self::sanitize_resumed_history(&mut projection);
            let mut chat_req = ChatRequest {
                model: model_name.to_string(),
                messages: projection,
                temperature: Some(0.2),
                top_p: Some(0.95),
                max_tokens: None,
                max_completion_tokens: None,
                tools: if available_tools.is_empty() {
                    None
                } else {
                    Some(available_tools.clone())
                },
                tool_choice: None,
                reasoning_effort: eff_opt,
                thinking_budget: budget_opt,
                response_format: None,
                prompt_cache_key: Some(cache_key.clone()),
                stream: true,
            };

            let mut assistant_text = String::new();
            let mut assistant_reasoning = String::new();
            let mut active_tool_calls: Vec<ToolCall> = Vec::new();
            let mut turn_usage: Option<TokenUsage> = None;

            let max_attempts: u32 = 5;
            let mut attempt: u32 = 0;
            let mut policy_retries: u32 = 0;
            const MAX_POLICY_RETRIES: u32 = 3;

            loop {
                attempt += 1;
                crate::engine::policy_guard::audit_event(
                    chat_id,
                    "model_requested",
                    &format!("step={} retry={}", turn_seq, policy_retries),
                );
                last_request_ts = Some(chrono::Utc::now().timestamp());
                let (stream_tx, mut stream_rx) = mpsc::channel(64);
                let client = http_client.clone();

                let provider = provider_name.to_string();
                let proto_owned = api_protocol.map(|s| s.to_string());
                let base = base_url.to_string();
                let kp = key_pool.clone();
                let cr = caps_registry.clone();
                let req_clone = chat_req.clone();

                let stream_join = tokio::spawn(async move {
                    crate::llm::dispatch_stream_with_protocol(&client, &provider, &base, proto_owned.as_deref(), &kp, &cr, req_clone, stream_tx).await
                });

                assistant_text.clear();
                assistant_reasoning.clear();
                active_tool_calls.clear();
                let mut in_stream_error: Option<String> = None;
                let mut content_prefix_buffer = String::new();
                let mut content_prefix_flushed = false;
                let mut suppress_content_stream = false;
                let mut delta_tool_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                let mut delta_tool_args: std::collections::HashMap<String, String> = std::collections::HashMap::new();

                loop {
                    let evt = tokio::select! {
                        biased;
                        _ = session.cancelled() => {
                            stream_join.abort();
                            let _ = tools.pty_manager.kill_all_for_chat(chat_id);
                            Self::save_checkpoint(db, chat_id, history);
                            let _ = event_sender
                                .send(StreamEvent::Error { error: "User canceled execution".to_string() })
                                .await;
                            return Err("User canceled execution".to_string());
                        }
                        maybe = stream_rx.recv() => match maybe {
                            Some(e) => e,
                            None => break,
                        }
                    };
                    if let StreamEvent::Error { ref error } = evt {
                        in_stream_error = Some(error.clone());
                        continue;
                    }
                    if let StreamEvent::Finish { ref usage, .. } = evt {
                        if usage.is_some() {
                            turn_usage = usage.clone();
                        }
                    }

                    match &evt {
                        StreamEvent::ContentDelta { delta } => {
                            assistant_text.push_str(delta);
                            if !suppress_content_stream {
                                if !content_prefix_flushed {
                                    content_prefix_buffer.push_str(delta);
                                    if crate::engine::policy_guard::detect_filter_signature(&content_prefix_buffer).is_some() {
                                        suppress_content_stream = true;
                                        content_prefix_buffer.clear();
                                    } else if content_prefix_buffer.len() >= 800 || content_prefix_buffer.contains("```") {
                                        let _ = event_sender.send(StreamEvent::ContentDelta { delta: content_prefix_buffer.clone() }).await;
                                        content_prefix_flushed = true;
                                    }
                                } else {
                                    let _ = event_sender.send(evt.clone()).await;
                                }
                            }
                        }
                        StreamEvent::ReasoningDelta { delta } => {
                            assistant_reasoning.push_str(delta);
                            let _ = event_sender.send(evt.clone()).await;
                        }
                        StreamEvent::ToolCallDelta { id, name, arguments_delta, .. } => {
                            if let Some(i) = id {
                                if let Some(n) = name {
                                    delta_tool_names.insert(i.clone(), n.clone());
                                }
                                delta_tool_args.entry(i.clone()).or_default().push_str(arguments_delta);
                            }
                            let effective_name = id.as_ref().and_then(|i| delta_tool_names.get(i)).cloned().or_else(|| name.clone());
                            let effective_args = id.as_ref().and_then(|i| delta_tool_args.get(i)).map(|s| s.as_str()).unwrap_or(arguments_delta.as_str());
                            let is_art = effective_name.as_deref() == Some("artifact_read")
                                || crate::engine::policy_guard::is_internal_artifact_tool(effective_name.as_deref().unwrap_or(""), effective_args)
                                || effective_args.contains("artifact://") || effective_args.contains("sha256_");
                            if !is_art {
                                let _ = event_sender.send(evt.clone()).await;
                            }
                        }
                        StreamEvent::ToolCallComplete {
                            id,
                            name,
                            arguments,
                            ..
                        } => {
                            let is_art = crate::engine::policy_guard::is_internal_artifact_tool(name, arguments);
                            active_tool_calls.push(ToolCall {
                                id: id.clone(),
                                call_type: "function".to_string(),
                                function: crate::llm::request_compat::FunctionCall {
                                    name: name.clone(),
                                    arguments: arguments.clone(),
                                },
                            });
                            if is_art {
                                content_prefix_buffer.clear();
                                content_prefix_flushed = true;
                            } else if !content_prefix_flushed && !suppress_content_stream && !content_prefix_buffer.is_empty() {
                                let _ = event_sender.send(StreamEvent::ContentDelta { delta: content_prefix_buffer.clone() }).await;
                                content_prefix_flushed = true;
                            }
                            if !is_art {
                                let _ = event_sender.send(evt.clone()).await;
                            }
                        }
                        StreamEvent::Finish { .. } | StreamEvent::Error { .. } => {}
                        _ => {
                            let _ = event_sender.send(evt.clone()).await;
                        }
                    }
                }

                let join_res = match stream_join.await {
                    Ok(r) => r.map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                let stream_res = match (join_res, in_stream_error) {
                    (Err(e), _) => Err(e),
                    (Ok(()), Some(se)) => Err(se),
                    (Ok(()), None) => Ok(()),
                };

                let policy_hit = crate::engine::policy_guard::detect_filter_signature(&assistant_text)
                    .or_else(|| {
                        stream_res
                            .as_ref()
                            .err()
                            .and_then(|e| crate::engine::policy_guard::detect_filter_signature(e))
                    })
                    .or_else(|| {
                        crate::engine::policy_guard::detect_thinking_refusal(
                            &assistant_reasoning,
                            !active_tool_calls.is_empty(),
                        )
                    });

                if policy_hit.is_none() {
                    if !content_prefix_flushed && !suppress_content_stream && !content_prefix_buffer.is_empty() {
                        let _ = event_sender.send(StreamEvent::ContentDelta { delta: content_prefix_buffer.clone() }).await;
                    }
                } else {
                    content_prefix_buffer.clear();
                    suppress_content_stream = true;
                }

                if let Some(pm) = policy_hit {
                    crate::engine::policy_guard::audit_event(
                        chat_id,
                        "policy_violation_detected",
                        &format!("step={} signature={} hash={}", turn_seq, pm.category, pm.sig_hash),
                    );
                    crate::engine::policy_guard::audit_event(
                        chat_id,
                        "step_excluded_from_projection",
                        &format!("step={}", turn_seq),
                    );
                    let _ = event_sender.send(StreamEvent::TurnBufferReset).await;

                    if policy_retries < MAX_POLICY_RETRIES {
                        policy_retries += 1;
                        if let Some(last_user) = history
                            .iter_mut()
                            .rev()
                            .find(|m| m.role == "user" && matches!(m.content, MessageContent::Text(_)))
                        {
                            let raw_txt = last_user.content.as_text_lossy();
                            if !raw_txt.trim_start().starts_with("artifact://sha256:") && !raw_txt.trim().is_empty() {
                                if let Ok(art) = crate::engine::policy_guard::ArtifactStore::save_text_artifact(&raw_txt) {
                                    let short_hash: String = art.hash.chars().take(16).collect();
                                    crate::engine::policy_guard::audit_event(
                                        chat_id,
                                        "artifact_created",
                                        &format!("sha256:{}", short_hash),
                                    );
                                    let ref_prompt = crate::engine::policy_guard::ArtifactStore::format_prompt_ref(&art);
                                    crate::engine::policy_guard::audit_event(
                                        chat_id,
                                        "prompt_assembled",
                                        &format!("artifact://sha256:{}", short_hash),
                                    );
                                    last_user.content = MessageContent::Text(ref_prompt);
                                    let (rebuilt_proj, _) = ContextBuilder::build_projection(
                                        history,
                                        workspace_root,
                                        contract.as_ref(),
                                        caps.context_window,
                                        last_request_ts,
                                        preserve_past_reasoning,
                                        &mut compaction_cursor,
                                        true,
                                    );
                                    chat_req.messages = rebuilt_proj;
                                }
                            }
                        }
                        crate::engine::policy_guard::audit_event(
                            chat_id,
                            "retry_scheduled",
                            &format!("anchor_step={} retry={}", turn_seq.saturating_sub(1), policy_retries),
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(600 * policy_retries as u64)).await;
                        attempt = 0;
                        continue;
                    }

                    crate::engine::policy_guard::audit_event(
                        chat_id,
                        "turn_failed",
                        &format!("step={} reason={} hash={}", turn_seq, pm.category, pm.sig_hash),
                    );
                    Self::save_checkpoint(db, chat_id, history);
                    let err_out = match stream_res {
                        Err(e) if !e.trim().is_empty() => format!("Запрос заблокирован фильтром ({}): {}", pm.category, e),
                        _ => format!("Запрос заблокирован фильтром модели ({}) после {} попыток.", pm.category, MAX_POLICY_RETRIES),
                    };
                    let _ = event_sender.send(StreamEvent::Error { error: err_out.clone() }).await;
                    return Err(err_out);
                }

                match stream_res {
                    Ok(()) => {
                        crate::engine::policy_guard::audit_event(
                            chat_id,
                            "turn_committed",
                            &format!("step={}", turn_seq),
                        );
                        break;
                    }
                    Err(err_msg) => {
                        let lower = err_msg.to_lowercase();
                        let is_ctx_overflow = lower.contains("context_length_exceeded")
                            || lower.contains("prompt_too_long")
                            || lower.contains("maximum context length")
                            || lower.contains("too many tokens")
                            || lower.contains("http 413")
                            || lower.contains("status 413");
                        if is_ctx_overflow && reactive_compact_count < 2 {
                            reactive_compact_count += 1;
                            let before_tokens = count_context_tokens(model_name, caps.family, history, &available_tools, db);
                            ContextBuilder::micro_prune(history, 12_000, 4);
                            ContextBuilder::tier2_dedup_and_clean(history);
                            let summary = Self::run_cache_safe_compaction(
                                http_client,
                                provider_name,
                                base_url,
                                api_protocol,
                                key_pool,
                                caps_registry,
                                model_name,
                                &cache_key,
                                &available_tools,
                                history,
                                None,
                            )
                            .await
                            .unwrap_or_else(|_| {
                                let (compacted, _) = ContextCompactor::compact_history(history.clone(), 4);
                                compacted
                                    .iter()
                                    .find(|m| m.name.as_deref() == Some("context_compact_summary"))
                                    .map(|m| m.content.as_text_lossy())
                                    .unwrap_or_else(|| "Emergency reactive compaction.".to_string())
                            });
                            let plan_state = Self::load_plan_tuple(db, chat_id);
                            let restored_files = Self::collect_restored_files(db, workspace_root, chat_id);
                            let mut rehydrated = ContextBuilder::rehydrate_after_compaction(
                                history,
                                &summary,
                                workspace_root,
                                contract.as_ref(),
                                4,
                                plan_state,
                                restored_files,
                            );
                            Self::sanitize_resumed_history(&mut rehydrated);
                            let after_tokens = count_context_tokens(model_name, caps.family, &rehydrated, &available_tools, db);
                            if let Some(d) = db {
                                let _ = d.record_compaction_event(chat_id, "tier4_reactive", before_tokens, after_tokens, "Reactive overflow compaction");
                            }
                            *history = rehydrated;
                            compaction_cursor = CompactionCursor::default();
                            let _ = event_sender
                                .send(StreamEvent::ContextCompacted {
                                    before_tokens,
                                    after_tokens,
                                    summary,
                                })
                                .await;
                            chat_req.messages = history.clone();
                            attempt = 0;
                            continue;
                        }
                        let is_oauth_proto = matches!(
                            api_protocol.unwrap_or(""),
                            "antigravity"
                                | "openai_codex"
                                | "claude_oauth"
                                | "kimi_oauth"
                                | "xai_oauth"
                                | "zcode_oauth"
                                | "perplexity_oauth"
                        );
                        let is_http_400 = lower.contains("http 400")
                            || lower.contains("http error 400")
                            || lower.contains("status 400")
                            || lower.contains("400 bad request")
                            || lower.contains("(400)");
                        let is_http_404 = lower.contains("http 404")
                            || lower.contains("http error 404")
                            || lower.contains("status 404")
                            || lower.contains("404 not found")
                            || lower.contains("model_not_found");
                        let is_geo_block = lower.contains("location_not_eligible")
                            || lower.contains("geographic restriction")
                            || lower.contains("user location is not supported");
                        let is_auth_err = lower.contains("http 401")
                            || lower.contains("http error 401")
                            || lower.contains("status 401")
                            || lower.contains("http 403")
                            || lower.contains("http error 403")
                            || lower.contains("status 403")
                            || lower.contains("invalid api key");
                        let is_fatal = is_http_400
                            || is_http_404
                            || is_geo_block
                            || lower.contains("tos_violation")
                            || lower.contains("violation of terms of service")
                            || lower.contains("disabled in this account")
                            || lower.contains("all antigravity accounts exhausted")
                            || (api_protocol.unwrap_or("") == "antigravity" && is_auth_err)
                            || (is_auth_err && (!is_oauth_proto || attempt >= 2))
                            || lower.contains("no available api keys")
                            || lower.contains("no enabled ") && lower.contains("oauth account");

                        let has_partial = !assistant_text.is_empty() || !active_tool_calls.is_empty();

                        if is_fatal || has_partial || attempt >= max_attempts || !session.is_active() {
                            Self::save_checkpoint(db, chat_id, history);
                            let _ = event_sender
                                .send(StreamEvent::Error { error: err_msg.clone() })
                                .await;
                            return Err(err_msg);
                        }

                        let _ = event_sender
                            .send(StreamEvent::RetryNotice {
                                reason: err_msg,
                                attempt,
                                model: model_name.to_string(),
                            })
                            .await;
                        let backoff_ms = if lower.contains("rate") || lower.contains("429") || lower.contains("cooldown") {
                            3000 * attempt as u64
                        } else {
                            1200 * attempt as u64
                        };
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    }
                }
            }

            let (used_tokens, cached_tokens, cache_hit_rate) = if let Some(u) = turn_usage {
                let prompt_t = u.prompt_tokens.max(est_used);
                let rate = if prompt_t > 0 {
                    (u.cached_tokens as f32 / prompt_t as f32) * 100.0
                } else {
                    0.0
                };
                (prompt_t + u.completion_tokens, u.cached_tokens, rate)
            } else {
                (est_used, 0, 0.0)
            };

            let _ = event_sender
                .send(StreamEvent::ContextTelemetry {
                    used_tokens,
                    context_window: caps.context_window,
                    cached_tokens,
                    cache_hit_rate,
                })
                .await;

            let mut assistant_msg = ChatMessage::assistant(assistant_text);
            if !assistant_reasoning.is_empty() {
                assistant_msg.reasoning_content = Some(assistant_reasoning);
            }
            if !active_tool_calls.is_empty() {
                assistant_msg.tool_calls = Some(active_tool_calls.clone());
            }
            history.push(assistant_msg);
            Self::save_checkpoint(db, chat_id, history);

            if active_tool_calls.is_empty() {
                if history.last().map(|m| m.content.as_text_lossy().trim().is_empty()).unwrap_or(true) {
                    if empty_retries < 3 {
                        empty_retries += 1;
                        history.pop();
                        crate::logger::warn(
                            "SingleAgent",
                            &format!("Empty response on turn {}, auto-retrying ({}/3) model={}", turn_seq, empty_retries, model_name),
                        );
                        if empty_retries >= 2 {
                            history.push(ChatMessage::user("Continue and provide your final response or next tool call."));
                        }
                        let _ = event_sender
                            .send(StreamEvent::RetryNotice {
                                reason: format!("Model returned empty response, retrying ({}/3)...", empty_retries),
                                attempt: empty_retries,
                                model: model_name.to_string(),
                            })
                            .await;
                        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                        continue;
                    }
                    Self::save_checkpoint(db, chat_id, history);
                    let msg = "Model finished turn with empty response".to_string();
                    crate::logger::error_with_trace("SingleAgent", &format!("{} (model={}, turn={})", msg, model_name, turn_seq));
                    let _ = event_sender.send(StreamEvent::Error { error: msg.clone() }).await;
                    return Err(msg);
                }
                if !tools.is_plan_mode() && session.is_active() {
                    let remaining: Vec<String> = contract
                        .as_ref()
                        .map(|c| {
                            c.criteria
                                .iter()
                                .filter(|cr| matches!(cr.status, CriterionStatus::Pending | CriterionStatus::InProgress))
                                .map(|cr| format!("- [{}] ({}) {}", cr.id, match cr.status { CriterionStatus::InProgress => "in_progress", _ => "pending" }, cr.description))
                                .collect()
                        })
                        .unwrap_or_default();
                    if !remaining.is_empty() && incomplete_todo_nudges < 5 {
                        incomplete_todo_nudges += 1;
                        history.push(ChatMessage::user(format!(
                            "You cannot finish yet because the following todo items remain incomplete:
{}

Complete all remaining items and mark each one as completed via task_board(action=\"update\", id=..., status=\"completed\") before finishing.",
                            remaining.join("
")
                        )));
                        let _ = event_sender
                            .send(StreamEvent::RetryNotice {
                                reason: format!("{} unfinished todo item(s) remaining, continuing...", remaining.len()),
                                attempt: incomplete_todo_nudges,
                                model: model_name.to_string(),
                            })
                            .await;
                        continue;
                    }
                }
                let _ = state.transition(TaskStateEnum::Reviewing);
                break;
            }
            empty_retries = 0;

            let _ = state.transition(TaskStateEnum::WaitingTool);

            let all_agents = active_tool_calls.len() > 1
                && active_tool_calls
                    .iter()
                    .all(|tc| tc.function.name == "agent" || tc.function.name == "explore_subagent");
            if all_agents {
                let mut join_set = tokio::task::JoinSet::new();
                for (idx, tc) in active_tool_calls.iter().enumerate() {
                    let parsed_args: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    let sub_type = if tc.function.name == "explore_subagent" {
                        "Explore".to_string()
                    } else {
                        parsed_args
                            .get("subagent_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("general-purpose")
                            .to_string()
                    };
                    let sub_prompt = parsed_args
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let resume_id = parsed_args
                        .get("resume")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let max_t = parsed_args
                        .get("max_turns")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);
                    let hc = http_client.clone();
                    let cid = chat_id.to_string();
                    let ws = workspace_root.to_string();
                    let td = tools.clone();
                    let kp = key_pool.clone();
                    let cr = caps_registry.clone();
                    let pn = provider_name.to_string();
                    let bu = base_url.to_string();
                    let ap = api_protocol.map(|s| s.to_string());
                    let mn = model_name.to_string();
                    let sh = session.clone();
                    let tc_id = tc.id.clone();
                    let tc_name = tc.function.name.clone();

                    join_set.spawn(async move {
                        let started = std::time::Instant::now();
                        let res = crate::engine::subagents::SubagentRunner::run(
                            &hc,
                            &cid,
                            &ws,
                            td,
                            &kp,
                            &cr,
                            &pn,
                            &bu,
                            ap.as_deref(),
                            &mn,
                            &sh,
                            &sub_type,
                            &sub_prompt,
                            resume_id.as_deref(),
                            max_t,
                        )
                        .await;
                        (idx, tc_id, tc_name, started.elapsed().as_millis() as u64, res)
                    });
                }

                let mut results = Vec::with_capacity(active_tool_calls.len());
                while let Some(joined) = join_set.join_next().await {
                    if let Ok(item) = joined {
                        results.push(item);
                    }
                }
                for (idx, tc) in active_tool_calls.iter().enumerate() {
                    if !results.iter().any(|r| r.0 == idx) {
                        results.push((
                            idx,
                            tc.id.clone(),
                            tc.function.name.clone(),
                            0,
                            Err("Subagent task panicked".to_string()),
                        ));
                    }
                }
                results.sort_by_key(|r| r.0);

                for (_idx, tc_id, tc_name, elapsed_ms, sub_res) in results {
                    let (ok, out_str, err_opt) = match sub_res {
                        Ok((agent_id, summary)) => (
                            true,
                            serde_json::json!({ "agent_id": agent_id, "output": summary }).to_string(),
                            None,
                        ),
                        Err(e) => (false, serde_json::json!({ "error": e }).to_string(), Some(e)),
                    };
                    let _ = event_sender
                        .send(StreamEvent::ToolExecEnd {
                            id: tc_id.clone(),
                            success: ok,
                            output: out_str.clone(),
                            error: err_opt,
                            elapsed_ms,
                        })
                        .await;
                    history.push(ChatMessage {
                        role: "tool".to_string(),
                        content: MessageContent::Text(out_str),
                        name: Some(tc_name),
                        tool_calls: None,
                        tool_call_id: Some(tc_id),
                        reasoning_content: None,
                    });
                }
                Self::save_checkpoint(db, chat_id, history);
                let _ = state.transition(TaskStateEnum::Executing);
                continue;
            }

            if event_sender.is_closed() {
                let _ = tools.pty_manager.kill_all_for_chat(chat_id);
                Self::save_checkpoint(db, chat_id, history);
                return Err("Client disconnected".to_string());
            }

            let all_concurrent_safe = active_tool_calls.len() > 1
                && active_tool_calls
                    .iter()
                    .all(|tc| Self::is_concurrency_safe_tool(&tc.function.name));

            if all_concurrent_safe {
                let mut join_set = tokio::task::JoinSet::new();
                for (idx, tc) in active_tool_calls.iter().enumerate() {
                    let parsed_args: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    let td = tools.clone();
                    let ws = workspace_root.to_string();
                    let cid = chat_id.to_string();
                    let tc_id = tc.id.clone();
                    let tc_name = tc.function.name.clone();
                    let sess = session.clone();
                    let ev_tx = event_sender.clone();
                    join_set.spawn(async move {
                        let started = std::time::Instant::now();
                        if tc_name == "terminal_exec" {
                            let (ok, out_str, err_opt) = Self::run_live_terminal_exec(
                                &parsed_args,
                                &td,
                                &ws,
                                &cid,
                                &sess,
                                &tc_id,
                                &ev_tx,
                            )
                            .await;
                            let elapsed_ms = started.elapsed().as_millis() as u64;
                            (idx, tc_id, tc_name, elapsed_ms, ok, out_str, err_opt, ws)
                        } else {
                            let tool_timeout_secs = match tc_name.as_str() {
                                "wait" => {
                                    let secs = parsed_args.get("seconds").and_then(|v| v.as_u64()).unwrap_or(60);
                                    secs + 30
                                }
                                "agent" => 600,
                                "generate_image" => 240,
                                "tg_send_file" => 300,
                                "code_intel" | "web_research" => 120,
                                "computer_use" | "multi_actions" => 240,
                                _ => 180,
                            };
                            let exec_result = match tokio::time::timeout(
                                std::time::Duration::from_secs(tool_timeout_secs),
                                td.dispatch_tool(&tc_name, &parsed_args, None, Some(&cid)),
                            )
                            .await
                            {
                                Ok(res) => res,
                                Err(_) => crate::tools::ToolExecutionResult {
                                    tool_name: tc_name.clone(),
                                    success: false,
                                    output: serde_json::json!({
                                        "timed_out": true,
                                        "error": format!("Tool '{}' timed out after {}s", tc_name, tool_timeout_secs)
                                    }),
                                    error: Some(format!("Tool '{}' timed out after {}s", tc_name, tool_timeout_secs)),
                                    elapsed_ms: (tool_timeout_secs * 1000) as u128,
                                },
                            };
                            let elapsed_ms = started.elapsed().as_millis() as u64;
                            let raw_output = if exec_result.success {
                                serde_json::to_string(&exec_result.output).unwrap_or_else(|_| "{}".to_string())
                            } else {
                                serde_json::json!({
                                    "error": exec_result.error.clone().unwrap_or_else(|| "Tool failed".to_string())
                                })
                                .to_string()
                            };
                            let bounded_output = spill_tool_output_if_needed(&ws, &tc_id, &raw_output);
                            (idx, tc_id, tc_name, elapsed_ms, exec_result.success, bounded_output, exec_result.error, ws)
                        }
                    });
                }

                let mut batch_res = Vec::with_capacity(active_tool_calls.len());
                while let Some(joined) = join_set.join_next().await {
                    if let Ok(item) = joined {
                        let _ = event_sender
                            .send(StreamEvent::ToolExecEnd {
                                id: item.1.clone(),
                                success: item.4,
                                output: item.5.clone(),
                                error: item.6.clone(),
                                elapsed_ms: item.3,
                            })
                            .await;
                        batch_res.push(item);
                    }
                }
                for (idx, tc) in active_tool_calls.iter().enumerate() {
                    if !batch_res.iter().any(|r| r.0 == idx) {
                        let err_msg = "Tool task panicked".to_string();
                        let out_str = serde_json::json!({ "error": err_msg }).to_string();
                        let _ = event_sender
                            .send(StreamEvent::ToolExecEnd {
                                id: tc.id.clone(),
                                success: false,
                                output: out_str.clone(),
                                error: Some(err_msg.clone()),
                                elapsed_ms: 0,
                            })
                            .await;
                        batch_res.push((
                            idx,
                            tc.id.clone(),
                            tc.function.name.clone(),
                            0,
                            false,
                            out_str,
                            Some(err_msg),
                            workspace_root.to_string(),
                        ));
                    }
                }
                batch_res.sort_by_key(|r| r.0);

                for (_idx, tc_id, tc_name, _elapsed_ms, _ok, bounded_output, _err_opt, _ws) in batch_res {
                    history.push(ChatMessage {
                        role: "tool".to_string(),
                        content: MessageContent::Text(bounded_output),
                        name: Some(tc_name),
                        tool_calls: None,
                        tool_call_id: Some(tc_id),
                        reasoning_content: None,
                    });
                }
                Self::save_checkpoint(db, chat_id, history);
                let _ = state.transition(TaskStateEnum::Executing);
                continue;
            }

            for tc in active_tool_calls {
                if !session.is_active() {
                    let _ = tools.pty_manager.kill_all_for_chat(chat_id);
                    Self::save_checkpoint(db, chat_id, history);
                    let _ = event_sender
                        .send(StreamEvent::Error { error: "User canceled execution".to_string() })
                        .await;
                    return Err("User canceled execution".to_string());
                }
                let tool_name = &tc.function.name;
                let parsed_args: Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                if tool_name == "terminal_exec" {
                    let exec_cp_id = uuid::Uuid::new_v4().to_string();
                    let ws_for_pre = workspace_root.to_string();
                    let cp_for_pre = exec_cp_id.clone();
                    let pre_exec_manifest = tokio::task::spawn_blocking(move || crate::tools::blob_store::capture_workspace_manifest(&ws_for_pre, &cp_for_pre, "pre_exec", false).ok()).await.ok().flatten();
                    let tool_started = std::time::Instant::now();
                    let (ok, out_str, err_opt) = Self::run_live_terminal_exec(
                        &parsed_args,
                        &tools,
                        workspace_root,
                        chat_id,
                        session,
                        &tc.id,
                        &event_sender,
                    )
                    .await;
                    let elapsed_ms = tool_started.elapsed().as_millis() as u64;
                if let Some(d) = db {
                    d.emit_event(
                        chat_id,
                        "tool_span",
                        serde_json::json!({
                            "turnSeq": turn_seq,
                            "toolCallId": tc.id,
                            "toolName": tool_name,
                            "durationMs": elapsed_ms,
                            "success": ok
                        }),
                    );
                }

                if let Some(pre_m) = pre_exec_manifest {
                    let ws_for_post = workspace_root.to_string();
                    let cp_for_post = pre_m.checkpoint_id.clone();
                    if let Ok(Ok(post_m)) = tokio::task::spawn_blocking(move || crate::tools::blob_store::capture_workspace_manifest(&ws_for_post, &cp_for_post, "post_exec", false)).await {
                        let pre_map: std::collections::HashMap<String, String> = pre_m.entries.into_iter().map(|e| (e.path, e.blob_hash)).collect();
                        if let Some(d) = db {
                            for post_e in post_m.entries {
                                let prev_h = pre_map.get(&post_e.path).cloned().unwrap_or_default();
                                if prev_h != post_e.blob_hash {
                                    let _ = d.record_session_artifact(chat_id, "file", &post_e.path);
                                    d.emit_event(
                                        chat_id,
                                        "file_patched",
                                        serde_json::json!({
                                            "path": post_e.path,
                                            "beforeHash": prev_h,
                                            "afterHash": post_e.blob_hash,
                                            "toolCallId": tc.id,
                                            "source": "exec",
                                            "truncatedOrUnsaved": post_e.skipped_large
                                        }),
                                    );
                                }
                            }
                        }
                    }
                }
                    let _ = event_sender
                        .send(StreamEvent::ToolExecEnd {
                            id: tc.id.clone(),
                            success: ok,
                            output: out_str.clone(),
                            error: err_opt,
                            elapsed_ms,
                        })
                        .await;
                    history.push(ChatMessage {
                        role: "tool".to_string(),
                        content: MessageContent::Text(out_str),
                        name: Some(tool_name.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        reasoning_content: None,
                    });

                    Self::save_checkpoint(db, chat_id, history);
                    if !session.is_active() {
                        let _ = tools.pty_manager.kill_all_for_chat(chat_id);
                        let _ = event_sender
                            .send(StreamEvent::Error { error: "User canceled execution".to_string() })
                            .await;
                        return Err("User canceled execution".to_string());
                    }
                    continue;
                }

                if tool_name == "wait" {
                    if let Some(wmsg) = parsed_args.get("message").and_then(|v| v.as_str()).filter(|m| !m.trim().is_empty()) {
                        let _ = event_sender
                            .send(StreamEvent::ContentDelta {
                                delta: format!("\n{}\n", wmsg.trim()),
                            })
                            .await;
                    }
                }
                if tool_name == "enter_plan_mode" && (tools.is_plan_mode_locked() || tools.is_plan_mode() || plan_mode_entries >= 1) {
                    let err_msg = if tools.is_plan_mode_locked() {
                        "Plan Mode entry blocked: plan was already approved for execution. Proceed directly with implementation."
                    } else if tools.is_plan_mode() {
                        "Already in Plan Mode. Do not call enter_plan_mode again; continue exploring and call exit_plan_mode when ready."
                    } else {
                        "Plan Mode entry blocked: enter_plan_mode was already used in this run. Proceed directly."
                    };
                    let out_str = serde_json::json!({ "error": err_msg, "plan_mode": tools.is_plan_mode() }).to_string();
                    let _ = event_sender
                        .send(StreamEvent::ToolExecEnd {
                            id: tc.id.clone(),
                            success: false,
                            output: out_str.clone(),
                            error: Some(err_msg.to_string()),
                            elapsed_ms: 0,
                        })
                        .await;
                    history.push(ChatMessage {
                        role: "tool".to_string(),
                        content: MessageContent::Text(out_str),
                        name: Some(tool_name.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        reasoning_content: None,
                    });
                    continue;
                }
                if tool_name == "enter_plan_mode" {
                    plan_mode_entries += 1;
                }
                if tool_name == "task_board" {
                    let tool_started = std::time::Instant::now();
                    let (out_val, items_arr) = Self::handle_task_board(&parsed_args, contract);
                    let elapsed_ms = tool_started.elapsed().as_millis() as u64;
                    let out_str = serde_json::to_string(&out_val).unwrap_or_else(|_| "{}".to_string());
                    let _ = event_sender
                        .send(StreamEvent::TaskBoardUpdated { items: items_arr })
                        .await;
                    let _ = event_sender
                        .send(StreamEvent::ToolExecEnd {
                            id: tc.id.clone(),
                            success: true,
                            output: out_str.clone(),
                            error: None,
                            elapsed_ms,
                        })
                        .await;
                    history.push(ChatMessage {
                        role: "tool".to_string(),
                        content: MessageContent::Text(out_str),
                        name: Some(tool_name.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        reasoning_content: None,
                    });
                    continue;
                }

                if tool_name == "agent" || tool_name == "explore_subagent" {
                    let sub_type = if tool_name == "explore_subagent" {
                        "Explore"
                    } else {
                        parsed_args
                            .get("subagent_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("general-purpose")
                    };
                    let sub_prompt = parsed_args
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let resume_id = parsed_args.get("resume").and_then(|v| v.as_str());
                    let max_t = parsed_args
                        .get("max_turns")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as u32);
                    let tool_started = std::time::Instant::now();
                    let sub_res = crate::engine::subagents::SubagentRunner::run(
                        http_client,
                        chat_id,
                        workspace_root,
                        tools.clone(),
                        key_pool,
                        caps_registry,
                        provider_name,
                        base_url,
                        api_protocol,
                        model_name,
                        session,
                        sub_type,
                        sub_prompt,
                        resume_id,
                        max_t,
                    )
                    .await;
                    let elapsed_ms = tool_started.elapsed().as_millis() as u64;
                    let (ok, out_str, err_opt) = match sub_res {
                        Ok((agent_id, summary)) => (
                            true,
                            serde_json::json!({ "agent_id": agent_id, "output": summary }).to_string(),
                            None,
                        ),
                        Err(e) => (false, serde_json::json!({ "error": e }).to_string(), Some(e)),
                    };
                    let _ = event_sender
                        .send(StreamEvent::ToolExecEnd {
                            id: tc.id.clone(),
                            success: ok,
                            output: out_str.clone(),
                            error: err_opt,
                            elapsed_ms,
                        })
                        .await;
                    history.push(ChatMessage {
                        role: "tool".to_string(),
                        content: MessageContent::Text(out_str),
                        name: Some(tool_name.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        reasoning_content: None,
                    });
                    continue;
                }

                if let Err(perm_err) = RolePermissionEngine::validate_execution(
                    &AgentRole::Main,
                    tool_name,
                    &parsed_args,
                ) {
                    let err_output = serde_json::json!({ "error": perm_err }).to_string();
                    let _ = event_sender
                        .send(StreamEvent::ToolExecEnd {
                            id: tc.id.clone(),
                            success: false,
                            output: err_output.clone(),
                            error: Some(perm_err),
                            elapsed_ms: 0,
                        })
                        .await;
                    let tool_msg = ChatMessage {
                        role: "tool".to_string(),
                        content: MessageContent::Text(err_output),
                        name: Some(tool_name.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        reasoning_content: None,
                    };
                    history.push(tool_msg);
                Self::save_checkpoint(db, chat_id, history);
                    continue;
                }

                let call_sig = format!("{}:{}", tool_name, tc.function.arguments);
                if call_sig == last_call_sig {
                    same_call_streak += 1;
                } else {
                    last_call_sig = call_sig;
                    same_call_streak = 1;
                }
                let streak_limit = if tool_name == "terminal_io" || tool_name == "manage_task" {
                    20
                } else if tool_name == "computer_use" {
                    12
                } else {
                    3
                };
                if same_call_streak >= streak_limit {
                    Self::save_checkpoint(db, chat_id, history);
                    let msg = format!("Anti-Loop Circuit Breaker: '{}' called {} times identically. Pausing.", tool_name, streak_limit);
                    let _ = event_sender.send(StreamEvent::Error { error: msg.clone() }).await;
                    return Err(msg);
                }

                if matches!(tool_name.as_str(), "write_file" | "edit_file" | "patch_file" | "fs_ops") && !turn_checkpoint_created {
                    turn_checkpoint_created = true;
                    let cp_id = uuid::Uuid::new_v4().to_string();
                    let ws_for_pw = workspace_root.to_string();
                    let cp_for_pw = cp_id.clone();
                    if let Ok(Ok(manifest)) = tokio::task::spawn_blocking(move || crate::tools::blob_store::capture_workspace_manifest(&ws_for_pw, &cp_for_pw, "pre_write", true)).await {
                        if let Some(d) = db {
                            if let Ok(m_json) = serde_json::to_string(&manifest) {
                                let msg_seq = d.max_message_seq(chat_id);
                                let _ = d.save_checkpoint_record(
                                    chat_id,
                                    &cp_id,
                                    turn_seq,
                                    "pre_write",
                                    msg_seq,
                                    &m_json,
                                    crate::tools::blob_store::MAX_CHECKPOINTS_PER_SESSION,
                                );
                                let refs = d.list_all_referenced_blob_hashes();
                                crate::tools::blob_store::enforce_lru_disk_limit(workspace_root, &refs);
                                d.emit_event(
                                    chat_id,
                                    "checkpoint_created",
                                    serde_json::json!({
                                        "checkpointId": cp_id,
                                        "kind": "pre_write",
                                        "turnSeq": turn_seq,
                                        "fileCount": manifest.entries.len(),
                                        "skippedLargeFiles": manifest.skipped_large_files
                                    }),
                                );
                            }
                        }
                    }
                }

                let tool_started = std::time::Instant::now();
                let tool_timeout_secs = match tool_name.as_str() {
                    "wait" => {
                        let secs = parsed_args.get("seconds").and_then(|v| v.as_u64()).unwrap_or(60);
                        secs + 30
                    }
                    "agent" => 600,
                    "generate_image" => 240,
                    "tg_send_file" => 300,
                    "code_intel" | "web_research" => 120,
                    "computer_use" | "multi_actions" => 240,
                    _ => 180,
                };
                let exec_result = tokio::select! {
                    biased;
                    _ = session.cancelled() => {
                        let _ = tools.pty_manager.kill_all_for_chat(chat_id);
                        Self::save_checkpoint(db, chat_id, history);
                        let _ = event_sender
                            .send(StreamEvent::Error { error: "User canceled execution".to_string() })
                            .await;
                        return Err("User canceled execution".to_string());
                    }
                    timed = tokio::time::timeout(
                        std::time::Duration::from_secs(tool_timeout_secs),
                        tools.dispatch_tool(tool_name, &parsed_args, conn, Some(chat_id)),
                    ) => match timed {
                    Ok(res) => res,
                    Err(_) => crate::tools::ToolExecutionResult {
                        tool_name: tool_name.clone(),
                        success: false,
                        output: serde_json::json!({ "timed_out": true, "error": format!("Tool '{}' timed out after {}s", tool_name, tool_timeout_secs) }),
                        error: Some(format!("Tool '{}' timed out after {}s", tool_name, tool_timeout_secs)),
                        elapsed_ms: (tool_timeout_secs * 1000) as u128,
                    },
                    }
                };
                let elapsed_ms = tool_started.elapsed().as_millis() as u64;

                if !exec_result.success {
                    let err_sig = exec_result.error.clone().unwrap_or_default();
                    if err_sig == last_err_sig && !err_sig.is_empty() {
                        same_err_streak += 1;
                    } else {
                        last_err_sig = err_sig;
                        same_err_streak = 1;
                    }
                    if same_err_streak >= 3 {
                        let err_out = serde_json::json!({ "error": last_err_sig }).to_string();
                        history.push(ChatMessage {
                            role: "tool".to_string(),
                            content: MessageContent::Text(err_out),
                            name: Some(tool_name.clone()),
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                            reasoning_content: None,
                        });
                        Self::save_checkpoint(db, chat_id, history);
                        let msg = format!("Anti-Loop Circuit Breaker: 3 identical tool errors in a row ({}). Pausing.", last_err_sig);
                        let _ = event_sender.send(StreamEvent::Error { error: msg.clone() }).await;
                        return Err(msg);
                    }
                } else {
                    same_err_streak = 0;
                }

                let mut vision_data_url: Option<String> = None;
                let mut audio_data_part: Option<(String, String)> = None;
                let mut video_data_part: Option<(String, String)> = None;
                let mut media_file_path: Option<String> = None;
                let mut ui_output_override: Option<String> = None;
                let raw_output = if exec_result.success {
                    if tool_name == "computer_use" {
                        if let Some(obj) = exec_result.output.as_object() {
                            if let Some(du) = obj.get("dataUrl").and_then(|v| v.as_str()) {
                                vision_data_url = Some(du.to_string());
                                ui_output_override = serde_json::to_string(&exec_result.output).ok();
                                let mut compact = obj.clone();
                                compact.remove("dataUrl");
                                serde_json::to_string(&Value::Object(compact)).unwrap_or_else(|_| "{}".to_string())
                            } else {
                                serde_json::to_string(&exec_result.output).unwrap_or_else(|_| "{}".to_string())
                            }
                        } else {
                            serde_json::to_string(&exec_result.output).unwrap_or_else(|_| "{}".to_string())
                        }
                    } else if tool_name == "read_file" {
                        if let Some(obj) = exec_result.output.as_object() {
                            let kind = obj.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                            let p = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
                            if kind == "image" {
                                if let Some(du) = obj.get("dataUrl").and_then(|v| v.as_str()) {
                                    vision_data_url = Some(du.to_string());
                                    media_file_path = Some(p.to_string());
                                    ui_output_override = serde_json::to_string(&exec_result.output).ok();
                                    let mut compact = obj.clone();
                                    compact.remove("dataUrl");
                                    serde_json::to_string(&Value::Object(compact)).unwrap_or_else(|_| "{}".to_string())
                                } else {
                                    serde_json::to_string(&exec_result.output).unwrap_or_else(|_| "{}".to_string())
                                }
                            } else if kind == "audio" {
                                if let Some(du) = obj.get("dataUrl").and_then(|v| v.as_str()) {
                                    let fmt_hint = obj
                                        .get("format")
                                        .and_then(|v| v.as_str())
                                        .or_else(|| obj.get("mime").and_then(|v| v.as_str()))
                                        .unwrap_or("wav");
                                    if let Some(idx) = du.find(";base64,") {
                                        let b64 = &du[idx + 8..];
                                        audio_data_part = Some((b64.to_string(), fmt_hint.to_string()));
                                    }
                                    media_file_path = Some(p.to_string());
                                    ui_output_override = serde_json::to_string(&exec_result.output).ok();
                                    let mut compact = obj.clone();
                                    compact.remove("dataUrl");
                                    serde_json::to_string(&Value::Object(compact)).unwrap_or_else(|_| "{}".to_string())
                                } else {
                                    serde_json::to_string(&exec_result.output).unwrap_or_else(|_| "{}".to_string())
                                }
                            } else if kind == "video" {
                                if let Some(du) = obj.get("dataUrl").and_then(|v| v.as_str()) {
                                    let mime = obj.get("mime").and_then(|v| v.as_str()).unwrap_or("video/mp4");
                                    video_data_part = Some((du.to_string(), mime.to_string()));
                                    media_file_path = Some(p.to_string());
                                    ui_output_override = serde_json::to_string(&exec_result.output).ok();
                                    let mut compact = obj.clone();
                                    compact.remove("dataUrl");
                                    serde_json::to_string(&Value::Object(compact)).unwrap_or_else(|_| "{}".to_string())
                                } else {
                                    serde_json::to_string(&exec_result.output).unwrap_or_else(|_| "{}".to_string())
                                }
                            } else {
                                serde_json::to_string(&exec_result.output).unwrap_or_else(|_| "{}".to_string())
                            }
                        } else {
                            serde_json::to_string(&exec_result.output).unwrap_or_else(|_| "{}".to_string())
                        }
                    } else {
                        serde_json::to_string(&exec_result.output)
                            .unwrap_or_else(|_| "{}".to_string())
                    }
                } else {
                    serde_json::json!({
                        "error": exec_result.error.clone().unwrap_or_else(|| "Tool failed".to_string())
                    })
                    .to_string()
                };

                let bounded_output = spill_tool_output_if_needed(workspace_root, &tc.id, &raw_output);
                let is_art = crate::engine::policy_guard::is_internal_artifact_tool(tool_name, &tc.function.arguments);

                if !is_art {
                    let ui_output = ui_output_override.unwrap_or_else(|| bounded_output.clone());
                    let _ = event_sender
                        .send(StreamEvent::ToolExecEnd {
                            id: tc.id.clone(),
                            success: exec_result.success,
                            output: ui_output,
                            error: exec_result.error.clone(),
                            elapsed_ms,
                        })
                        .await;
                }

                let tool_msg = ChatMessage {
                    role: "tool".to_string(),
                    content: MessageContent::Text(bounded_output),
                    name: Some(tool_name.clone()),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    reasoning_content: None,
                };
                history.push(tool_msg);
                if is_art {
                    let _ = event_sender.send(StreamEvent::TurnBufferReset).await;
                    Self::save_checkpoint(db, chat_id, history);
                    continue;
                }
                if exec_result.success {
                    if let Some(d) = db {
                        match tool_name.as_str() {
                            "read_file" | "write_file" | "edit_file" | "patch_file" => {
                                if let Some(fp) = parsed_args.get("path").and_then(|v| v.as_str()) {
                                    if !fp.trim().is_empty() {
                                        let _ = d.record_session_artifact(chat_id, "file", fp.trim());
                                    }
                                }
                                if tool_name != "read_file" {
                                    let out_obj = exec_result.output.as_object();
                                    let path_val = out_obj.and_then(|o| o.get("path")).and_then(|v| v.as_str()).unwrap_or("");
                                    let before_h = out_obj.and_then(|o| o.get("beforeHash")).and_then(|v| v.as_str()).unwrap_or("");
                                    let after_h = out_obj.and_then(|o| o.get("afterHash")).and_then(|v| v.as_str()).unwrap_or("");
                                    let unsaved = out_obj.and_then(|o| o.get("truncatedOrUnsaved")).and_then(|v| v.as_bool()).unwrap_or(false);
                                    d.emit_event(
                                        chat_id,
                                        "file_patched",
                                        serde_json::json!({
                                            "path": path_val,
                                            "beforeHash": before_h,
                                            "afterHash": after_h,
                                            "toolCallId": tc.id,
                                            "source": tool_name,
                                            "truncatedOrUnsaved": unsaved
                                        }),
                                    );
                                }
                            }
                            "run_command" | "terminal_io" | "execute_command" => {
                                if let Some(cmd) = parsed_args.get("command").and_then(|v| v.as_str()) {
                                    let short_cmd: String = cmd.trim().chars().take(120).collect();
                                    if !short_cmd.is_empty() {
                                        let _ = d.record_session_artifact(chat_id, "command", &short_cmd);
                                    }
                                }
                            }
                            "enter_plan_mode" => {
                                let prev_md = Self::load_plan_tuple(db, chat_id).map(|(_, m)| m).unwrap_or_default();
                                let _ = d.save_plan_state(chat_id, true, &prev_md);
                            }
                            _ => {}
                        }
                    }
                }
                if tool_name == "exit_plan_mode" && exec_result.success {
                    let plan_sum = exec_result
                        .output
                        .get("plan_summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(d) = db {
                        let _ = d.save_plan_state(chat_id, false, &plan_sum);
                    }
                    Self::save_checkpoint(db, chat_id, history);
                    let _ = event_sender
                        .send(StreamEvent::PlanApprovalRequired {
                            plan_summary: plan_sum,
                        })
                        .await;
                    let _ = state.transition(TaskStateEnum::Completed);
                    return Ok(state);
                }
                if let Some(du) = vision_data_url {
                    if caps.supports_vision {
                        if tool_name == "computer_use" {
                            let win_title = exec_result
                                .output
                                .get("title")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .unwrap_or("desktop");
                            let offscreen_flag = exec_result
                                .output
                                .get("offscreen")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            history.push(ChatMessage {
                                role: "user".to_string(),
                                content: MessageContent::Parts(vec![
                                    ContentPart::Text {
                                        text: format!(
                                            "Captured target '{}' (offscreen={}) via computer_use. Coordinates 0..1000 map directly to this image's width and height.",
                                            win_title, offscreen_flag
                                        ),
                                    },
                                    ContentPart::ImageUrl {
                                        image_url: ImageUrlDetail {
                                            url: crate::llm::request_compat::resize_image_data_url_if_needed(&du, 1568),
                                            detail: Some("auto".to_string()),
                                        },
                                    },
                                ]),
                                name: Some("ephemeral_vision".to_string()),
                                tool_calls: None,
                                tool_call_id: None,
                                reasoning_content: None,
                            });
                        } else {
                            let fp = media_file_path.as_deref().unwrap_or("image");
                            history.push(ChatMessage {
                                role: "user".to_string(),
                                content: MessageContent::Parts(vec![
                                    ContentPart::Text {
                                        text: format!("Loaded image '{}' for visual inspection.", fp),
                                    },
                                    ContentPart::ImageUrl {
                                        image_url: ImageUrlDetail {
                                            url: du,
                                            detail: Some("auto".to_string()),
                                        },
                                    },
                                ]),
                                name: Some("ephemeral_vision".to_string()),
                                tool_calls: None,
                                tool_call_id: None,
                                reasoning_content: None,
                            });
                        }
                    }
                } else if let Some((b64, mime_or_fmt)) = audio_data_part {
                    let fp = media_file_path.as_deref().unwrap_or("audio");
                    let low = mime_or_fmt.to_ascii_lowercase();
                    let fmt = if low.contains("mp3") || low.contains("mpeg") {
                        "mp3"
                    } else if low.contains("ogg") || low.contains("opus") {
                        "ogg"
                    } else if low.contains("flac") {
                        "flac"
                    } else if low.contains("m4a") || low.contains("aac") || low.contains("mp4") {
                        "aac"
                    } else if low.contains("webm") {
                        "webm"
                    } else {
                        "wav"
                    };
                    history.push(ChatMessage {
                        role: "user".to_string(),
                        content: MessageContent::Parts(vec![
                            ContentPart::Text {
                                text: format!("Loaded audio '{}' for audio inspection.", fp),
                            },
                            ContentPart::InputAudio {
                                input_audio: crate::llm::request_compat::InputAudioDetail {
                                    data: b64,
                                    format: fmt.to_string(),
                                },
                            },
                        ]),
                        name: Some("ephemeral_vision".to_string()),
                        tool_calls: None,
                        tool_call_id: None,
                        reasoning_content: None,
                    });
                } else if let Some((du, _mime)) = video_data_part {
                    let fp = media_file_path.as_deref().unwrap_or("video");
                    if caps.supports_vision {
                        history.push(ChatMessage {
                            role: "user".to_string(),
                            content: MessageContent::Parts(vec![
                                ContentPart::Text {
                                    text: format!("Loaded video '{}' for visual inspection.", fp),
                                },
                                ContentPart::ImageUrl {
                                    image_url: ImageUrlDetail {
                                        url: du,
                                        detail: Some("auto".to_string()),
                                    },
                                },
                            ]),
                            name: Some("ephemeral_vision".to_string()),
                            tool_calls: None,
                            tool_call_id: None,
                            reasoning_content: None,
                        });
                    }
                }
            }

            let _ = state.transition(TaskStateEnum::Executing);
        }

        if !session.is_active() {
            let _ = tools.pty_manager.kill_all_for_chat(chat_id);
            crate::tools::phantom_session::restore_all_staged_windows(true);
            Self::save_checkpoint(db, chat_id, history);
            let _ = event_sender
                .send(StreamEvent::Error {
                    error: "User canceled execution".to_string(),
                })
                .await;
            return Err("User canceled execution".to_string());
        } else if let Some(c) = contract.as_mut() {
            let _ = state.transition(TaskStateEnum::Verifying);
            let all_pass = c.is_all_criteria_passed();

            if all_pass {
                let verdict = StructuredVerdict {
                    status: VerdictStatus::Passed,
                    reasoning: "All acceptance criteria verified and passed".to_string(),
                    confidence: 1.0,
                    evaluated_at: chrono::Utc::now().to_rfc3339(),
                    blocking_issues: Vec::new(),
                    repair_instructions: Vec::new(),
                };
                c.set_verdict(verdict);
                let _ = state.transition(TaskStateEnum::Completed);
            } else {
                let issues = c.blocking_issues();
                let verdict = StructuredVerdict {
                    status: VerdictStatus::Inconclusive,
                    reasoning: "Some criteria remain unverified or pending".to_string(),
                    confidence: 0.7,
                    evaluated_at: chrono::Utc::now().to_rfc3339(),
                    blocking_issues: issues,
                    repair_instructions: vec![
                        "Continue execution to address incomplete criteria".to_string(),
                    ],
                };
                c.set_verdict(verdict);
                let _ = state.transition(TaskStateEnum::Executing);
            }
        } else {
            let _ = state.transition(TaskStateEnum::Completed);
        }

        crate::tools::phantom_session::restore_all_staged_windows(true);
        if let Some(d) = db {
            let _ = d.delete_checkpoint(chat_id);
        }
        Ok(state)
    }
}
