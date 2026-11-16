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
use crate::llm::{dispatch_stream, StreamEvent, TokenCountRouter, TokenUsage};
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
                if total_chars + capped.len() > max_total_chars {
                    break;
                }
                total_chars += capped.len();
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
            let allowed = ["command", "description", "cwd", "timeout_secs", "background"];
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

        let timeout_secs = match args.get("timeout_secs") {
            Some(v) => match v.as_u64() {
                Some(n) if (5..=600).contains(&n) => n,
                _ => {
                    let err_msg = "Invalid argument: timeout_secs must be an integer between 5 and 600".to_string();
                    return (false, serde_json::json!({ "error": err_msg }).to_string(), Some(err_msg));
                }
            },
            None => 120,
        };

        let cwd_raw = match args.get("cwd") {
            Some(v) => match v.as_str() {
                Some(s) if !s.trim().is_empty() => s,
                _ => {
                    let err_msg = "Invalid argument: cwd must be a non-empty string".to_string();
                    return (false, serde_json::json!({ "error": err_msg }).to_string(), Some(err_msg));
                }
            },
            None => workspace_root,
        };
        if !std::path::Path::new(cwd_raw).is_dir() {
            let err_msg = format!("Invalid cwd: directory '{}' does not exist", cwd_raw);
            return (false, serde_json::json!({ "error": err_msg }).to_string(), Some(err_msg));
        }
        let cwd_validated = match crate::tools::workspace::validate_path(cwd_raw, &tools.allowed_roots) {
            Ok(p) => p,
            Err(e) => return (false, serde_json::json!({ "error": e }).to_string(), Some(e)),
        };
        let cwd_str = cwd_validated.to_string_lossy().to_string();
        let cwd = cwd_str.as_str();
        let background = args.get("background").and_then(|v| v.as_bool()).unwrap_or(false);
        let started = std::time::Instant::now();

        let encoded_ps = if cfg!(target_os = "windows") {
            use base64::Engine;
            let full_script = format!(
                "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::InputEncoding = [System.Text.Encoding]::UTF8; $OutputEncoding = [System.Text.Encoding]::UTF8;\n{}",
                command
            );
            let utf16_bytes: Vec<u8> = full_script
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes())
                .collect();
            base64::engine::general_purpose::STANDARD.encode(&utf16_bytes)
        } else {
            String::new()
        };

        if !background {
            use tokio::io::AsyncReadExt;
            let mut cmd = if cfg!(target_os = "windows") {
                let mut c = tokio::process::Command::new("powershell.exe");
                c.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded_ps]);
                c
            } else {
                let mut c = tokio::process::Command::new("sh");
                c.args(["-c", command]);
                c
            };
            cmd.current_dir(cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);
            #[cfg(windows)]
            {
                cmd.creation_flags(0x08000000);
            }

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => return (false, serde_json::json!({ "error": e.to_string() }).to_string(), Some(e.to_string())),
            };
            let child_pid = child.id();

            #[cfg(windows)]
            struct WinJobGuard(Option<windows::Win32::Foundation::HANDLE>);
            #[cfg(windows)]
            impl WinJobGuard {
                fn attach(raw_handle: Option<std::os::windows::io::RawHandle>) -> Self {
                    use windows::Win32::Foundation::HANDLE;
                    use windows::Win32::System::JobObjects::{
                        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    };
                    unsafe {
                        let Ok(job) = CreateJobObjectW(None, None) else {
                            return Self(None);
                        };
                        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                        let _ = SetInformationJobObject(
                            job,
                            JobObjectExtendedLimitInformation,
                            &info as *const _ as *const std::ffi::c_void,
                            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                        );
                        if let Some(rh) = raw_handle {
                            let _ = AssignProcessToJobObject(job, HANDLE(rh));
                        }
                        Self(Some(job))
                    }
                }
                fn terminate(&mut self) {
                    use windows::Win32::Foundation::CloseHandle;
                    use windows::Win32::System::JobObjects::TerminateJobObject;
                    if let Some(job) = self.0.take() {
                        unsafe {
                            let _ = TerminateJobObject(job, 1);
                            let _ = CloseHandle(job);
                        }
                    }
                }
            }
            #[cfg(windows)]
            impl Drop for WinJobGuard {
                fn drop(&mut self) {
                    use windows::Win32::Foundation::CloseHandle;
                    if let Some(job) = self.0.take() {
                        unsafe {
                            let _ = CloseHandle(job);
                        }
                    }
                }
            }
            #[cfg(windows)]
            unsafe impl Send for WinJobGuard {}
            #[cfg(windows)]
            unsafe impl Sync for WinJobGuard {}
            #[cfg(windows)]
            let mut job_guard = WinJobGuard::attach(child.raw_handle());

            let mut stdout_pipe = child.stdout.take();
            let mut stderr_pipe = child.stderr.take();
            let tx_out = event_sender.clone();
            let tx_err = event_sender.clone();
            let cid_out = call_id.to_string();
            let cid_err = call_id.to_string();

            let out_task = tokio::spawn(async move {
                let mut acc = Vec::new();
                if let Some(ref mut r) = stdout_pipe {
                    let mut buf = [0u8; 8192];
                    while let Ok(n) = r.read(&mut buf).await {
                        if n == 0 { break; }
                        acc.extend_from_slice(&buf[..n]);
                        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                        if !chunk.is_empty() {
                            let _ = tx_out.send(StreamEvent::ToolOutputDelta { id: cid_out.clone(), chunk }).await;
                        }
                    }
                }
                acc
            });

            let err_task = tokio::spawn(async move {
                let mut acc = Vec::new();
                if let Some(ref mut r) = stderr_pipe {
                    let mut buf = [0u8; 8192];
                    while let Ok(n) = r.read(&mut buf).await {
                        if n == 0 { break; }
                        acc.extend_from_slice(&buf[..n]);
                        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                        if !chunk.is_empty() {
                            let _ = tx_err.send(StreamEvent::ToolOutputDelta { id: cid_err.clone(), chunk }).await;
                        }
                    }
                }
                acc
            });

            let kill_tree = |pid_opt: Option<u32>| {
                #[cfg(windows)]
                if let Some(pid) = pid_opt {
                    use std::os::windows::process::CommandExt;
                    let mut tk = std::process::Command::new("taskkill");
                    tk.args(["/F", "/T", "/PID", &pid.to_string()]).creation_flags(0x08000000);
                    let _ = tk.output();
                }
                let _ = pid_opt;
            };

            let mut timed_out = false;
            let mut canceled = false;
            let mut status_opt = None;

            tokio::select! {
                _ = session.cancel_notify.notified() => {
                    canceled = true;
                    #[cfg(windows)]
                    job_guard.terminate();
                    kill_tree(child_pid);
                    let _ = child.kill().await;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_secs)) => {
                    timed_out = true;
                    #[cfg(windows)]
                    job_guard.terminate();
                    kill_tree(child_pid);
                    let _ = child.kill().await;
                }
                res = child.wait() => {
                    if let Ok(st) = res {
                        status_opt = Some(st);
                    }
                }
            }

            if canceled {
                let _ = out_task.await;
                let _ = err_task.await;
                return (false, serde_json::json!({ "error": "User canceled execution", "duration_ms": started.elapsed().as_millis() as u64 }).to_string(), Some("User canceled execution".to_string()));
            }

            let out_bytes = out_task.await.unwrap_or_default();
            let err_bytes = err_task.await.unwrap_or_default();
            let stdout_raw = crate::tools::process_pty::strip_ansi(&String::from_utf8_lossy(&out_bytes).replace("\r\n", "\n"));
            let stderr_raw = crate::tools::process_pty::strip_ansi(&String::from_utf8_lossy(&err_bytes).replace("\r\n", "\n"));
            let duration_ms = started.elapsed().as_millis() as u64;

            let mut combined = String::new();
            if !stdout_raw.is_empty() {
                combined.push_str(&stdout_raw);
            }
            if !stderr_raw.is_empty() {
                if !combined.is_empty() && !combined.ends_with('\n') {
                    combined.push('\n');
                }
                combined.push_str(&stderr_raw);
            }

            let (bounded_out, spill_file) = spill_tool_output_with_path(workspace_root, call_id, &combined);
            let (bounded_stdout, _) = spill_tool_output_with_path(workspace_root, &format!("{}_out", call_id), &stdout_raw);
            let (bounded_stderr, _) = spill_tool_output_with_path(workspace_root, &format!("{}_err", call_id), &stderr_raw);

            if timed_out {
                let err_msg = format!("Command timed out after {}s and was terminated", timeout_secs);
                let out_json = serde_json::json!({
                    "stdout": bounded_stdout,
                    "stderr": bounded_stderr,
                    "output": bounded_out,
                    "exit_code": serde_json::Value::Null,
                    "duration_ms": duration_ms,
                    "running": false,
                    "timed_out": true,
                    "spill_file": spill_file,
                    "error": err_msg,
                });
                return (false, out_json.to_string(), Some(err_msg));
            }

            let exit_code = status_opt.and_then(|s| s.code()).unwrap_or(-1);
            if exit_code != 0 {
                let err_msg = format!("Command exited with non-zero code {}", exit_code);
                let out_json = serde_json::json!({
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
                "stdout": bounded_stdout,
                "stderr": bounded_stderr,
                "output": bounded_out,
                "exit_code": 0,
                "duration_ms": duration_ms,
                "running": false,
                "timed_out": false,
                "spill_file": spill_file,
            });
            return (true, out_json.to_string(), None);
        }

        let (shell_bin, shell_args): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
            ("powershell.exe", vec!["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded_ps])
        } else {
            ("sh", vec!["-c", command])
        };

        let sid = match tools.pty_manager.spawn_pty_for_chat(120, 32, shell_bin, &shell_args, Some(cwd), None, Some(chat_id)) {
            Ok(id) => id,
            Err(e) => return (false, serde_json::json!({ "error": e }).to_string(), Some(e)),
        };

        let mut collected = String::new();
        let mut pending_utf8: Vec<u8> = Vec::new();
        let max_dur = std::time::Duration::from_millis(1200);

        let decode_utf8_stream = |buf: &mut Vec<u8>, incoming: &[u8], flush_all: bool| -> String {
            buf.extend_from_slice(incoming);
            if flush_all {
                let out = String::from_utf8_lossy(buf).to_string();
                buf.clear();
                return out;
            }
            match std::str::from_utf8(buf) {
                Ok(s) => {
                    let out = s.to_string();
                    buf.clear();
                    out
                }
                Err(e) => {
                    let valid_up_to = e.valid_up_to();
                    let out = String::from_utf8_lossy(&buf[..valid_up_to]).to_string();
                    if e.error_len().is_none() {
                        *buf = buf[valid_up_to..].to_vec();
                    } else {
                        let skip = valid_up_to + e.error_len().unwrap_or(1);
                        *buf = buf[skip..].to_vec();
                    }
                    out
                }
            }
        };

        loop {
            if !session.is_active() {
                let _ = tools.pty_manager.kill_pty(&sid);
                return (false, serde_json::json!({ "error": "User canceled execution" }).to_string(), Some("User canceled execution".to_string()));
            }
            if let Ok(chunk_bytes) = tools.pty_manager.read_pty(&sid, 65536) {
                if !chunk_bytes.is_empty() {
                    let text = decode_utf8_stream(&mut pending_utf8, &chunk_bytes, false).replace("\r\n", "\n");
                    if !text.is_empty() {
                        collected.push_str(&text);
                        let _ = event_sender
                            .send(StreamEvent::ToolOutputDelta {
                                id: call_id.to_string(),
                                chunk: text,
                            })
                            .await;
                    }
                }
            }
            let alive = tools.pty_manager.is_session_alive(&sid);
            if !alive || started.elapsed() >= max_dur {
                if !alive {
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                }
                break;
            }
            tokio::select! {
                _ = session.cancel_notify.notified() => {
                    let _ = tools.pty_manager.kill_pty(&sid);
                    return (false, serde_json::json!({ "error": "User canceled execution" }).to_string(), Some("User canceled execution".to_string()));
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => {}
            }
        }

        let tail_bytes = tools.pty_manager.read_pty(&sid, 65536).unwrap_or_default();
        if !tail_bytes.is_empty() || !pending_utf8.is_empty() {
            let text = decode_utf8_stream(&mut pending_utf8, &tail_bytes, true).replace("\r\n", "\n");
            if !text.is_empty() {
                collected.push_str(&text);
            }
        }

        let still_running = tools.pty_manager.is_session_alive(&sid);
        let exit_code = if still_running { None } else { tools.pty_manager.get_exit_code(&sid) };
        let duration_ms = started.elapsed().as_millis() as u64;
        let (bounded, spill_file) = spill_tool_output_with_path(workspace_root, call_id, &collected);

        if !still_running && exit_code.unwrap_or(0) != 0 {
            let code = exit_code.unwrap_or(-1);
            let err_msg = format!("Command exited with non-zero code {}", code);
            let out_json = serde_json::json!({
                "session_id": sid,
                "running": false,
                "exit_code": code,
                "duration_ms": duration_ms,
                "output": bounded,
                "spill_file": spill_file,
                "error": err_msg,
            });
            return (false, out_json.to_string(), Some(err_msg));
        }

        let out_json = serde_json::json!({
            "session_id": sid,
            "running": still_running,
            "exit_code": exit_code,
            "duration_ms": duration_ms,
            "output": bounded,
            "spill_file": spill_file,
        });
        (true, out_json.to_string(), None)
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
        key_pool: &KeyPool,
        caps_registry: &CapabilityRegistry,
        model_name: &str,
        cache_key: &str,
        tools: &[ToolDefinition],
        projection: &[ChatMessage],
        custom_instruction: Option<&str>,
    ) -> Result<String, String> {
        let mut fork_msgs = projection.to_vec();
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
        let kp = key_pool.clone();
        let cr = caps_registry.clone();
        let c = client.clone();

        let join = tokio::spawn(async move {
            dispatch_stream(&c, &p, &b, &kp, &cr, req, tx).await
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
        let mut i = 0;
        while i < history.len() {
            if history[i].role == "assistant" {
                if let Some(tcs) = history[i].tool_calls.clone() {
                    if !tcs.is_empty() {
                        let mut existing_ids = std::collections::HashSet::new();
                        let mut j = i + 1;
                        while j < history.len() && history[j].role == "tool" {
                            if let Some(ref tid) = history[j].tool_call_id {
                                existing_ids.insert(tid.clone());
                            }
                            j += 1;
                        }
                        let mut insert_pos = j;
                        for tc in tcs {
                            if !existing_ids.contains(&tc.id) {
                                history.insert(
                                    insert_pos,
                                    ChatMessage {
                                        role: "tool".to_string(),
                                        content: MessageContent::Text("[Interrupted before tool finished]".to_string()),
                                        name: Some(tc.function.name.clone()),
                                        tool_calls: None,
                                        tool_call_id: Some(tc.id.clone()),
                                        reasoning_content: None,
                                    },
                                );
                                insert_pos += 1;
                            }
                        }
                        i = insert_pos;
                        continue;
                    }
                }
            }
            i += 1;
        }
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
            let sys_prompt = PromptBuilder::build_system_prompt(
                &AgentRole::Main,
                workspace_root,
                None,
                None,
            );
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
            let client = reqwest::Client::new();
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
                &client,
                provider_name,
                base_url,
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

        if resume && !history.is_empty() {
            Self::sanitize_resumed_history(history);
        } else {
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
            history.push(ChatMessage::user(final_prompt));
        }
        Self::save_checkpoint(db, chat_id, history);


        let mut last_request_ts: Option<i64> = None;
        let mut last_call_sig = String::new();
        let mut same_call_streak: u32 = 0;
        let mut last_err_sig = String::new();
        let mut same_err_streak: u32 = 0;
        let mut consecutive_compact_failures: u32 = 0;
        let mut reactive_compact_count: u32 = 0;
        let mut turn_seq: i64 = 0;

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

            loop {
                attempt += 1;
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

                loop {
                    let evt = tokio::select! {
                        biased;
                        _ = session.cancel_notify.notified() => {
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
                    if let StreamEvent::Finish { ref usage, .. } = evt {
                        if usage.is_some() {
                            turn_usage = usage.clone();
                        }
                    } else {
                        let _ = event_sender.send(evt.clone()).await;
                    }

                    match &evt {
                        StreamEvent::ContentDelta { delta } => {
                            assistant_text.push_str(delta);
                        }
                        StreamEvent::ReasoningDelta { delta } => {
                            assistant_reasoning.push_str(delta);
                        }
                        StreamEvent::ToolCallComplete {
                            id,
                            name,
                            arguments,
                            ..
                        } => {
                            active_tool_calls.push(ToolCall {
                                id: id.clone(),
                                call_type: "function".to_string(),
                                function: crate::llm::request_compat::FunctionCall {
                                    name: name.clone(),
                                    arguments: arguments.clone(),
                                },
                            });
                        }
                        _ => {}
                    }
                }

                let stream_res = match stream_join.await {
                    Ok(r) => r.map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };

                match stream_res {
                    Ok(()) => break,
                    Err(err_msg) => {
                        let lower = err_msg.to_lowercase();
                        let is_ctx_overflow = lower.contains("context_length_exceeded")
                            || lower.contains("prompt_too_long")
                            || lower.contains("maximum context length")
                            || lower.contains("too many tokens")
                            || lower.contains("413");
                        if is_ctx_overflow && reactive_compact_count < 2 {
                            reactive_compact_count += 1;
                            let before_tokens = count_context_tokens(model_name, caps.family, history, &available_tools, db);
                            ContextBuilder::micro_prune(history, 12_000, 4);
                            ContextBuilder::tier2_dedup_and_clean(history);
                            let summary = Self::run_cache_safe_compaction(
                                http_client,
                                provider_name,
                                base_url,
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
                        let is_fatal = lower.contains("400")
                            || lower.contains("401")
                            || lower.contains("403")
                            || lower.contains("404")
                            || lower.contains("invalid_request")
                            || lower.contains("no available api keys");

                        let has_partial = !assistant_text.is_empty()
                            || !assistant_reasoning.is_empty()
                            || !active_tool_calls.is_empty();

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
                    Self::save_checkpoint(db, chat_id, history);
                    let msg = "Model finished turn with empty response".to_string();
                    let _ = event_sender.send(StreamEvent::Error { error: msg.clone() }).await;
                    return Err(msg);
                }
                let _ = state.transition(TaskStateEnum::Reviewing);
                break;
            }

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

            let all_concurrent_safe = active_tool_calls.len() > 1
                && active_tool_calls
                    .iter()
                    .all(|tc| Self::is_concurrency_safe_tool(&tc.function.name) || tc.function.name == "terminal_exec");

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
                            let exec_result = td.dispatch_tool(&tc_name, &parsed_args, None, Some(&cid)).await;
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
                        batch_res.push(item);
                    }
                }
                batch_res.sort_by_key(|r| r.0);

                for (_idx, tc_id, tc_name, elapsed_ms, ok, bounded_output, err_opt, _ws) in batch_res {
                    let _ = event_sender
                        .send(StreamEvent::ToolExecEnd {
                            id: tc_id.clone(),
                            success: ok,
                            output: bounded_output.clone(),
                            error: err_opt,
                            elapsed_ms,
                        })
                        .await;
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
                    let pre_exec_manifest = crate::tools::blob_store::capture_workspace_manifest(workspace_root, &exec_cp_id, "pre_exec", false).ok();
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

                if let Some(pre_m) = pre_exec_manifest {
                    if let Ok(post_m) = crate::tools::blob_store::capture_workspace_manifest(workspace_root, &pre_m.checkpoint_id, "post_exec", false) {
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
                let streak_limit = if tool_name == "terminal_io" { 20 } else { 3 };
                if same_call_streak >= streak_limit {
                    Self::save_checkpoint(db, chat_id, history);
                    let msg = format!("Anti-Loop Circuit Breaker: '{}' called {} times identically. Pausing.", tool_name, streak_limit);
                    let _ = event_sender.send(StreamEvent::Error { error: msg.clone() }).await;
                    return Err(msg);
                }

                if matches!(tool_name.as_str(), "write_file" | "edit_file" | "patch_file" | "fs_ops") && !turn_checkpoint_created {
                    turn_checkpoint_created = true;
                    let cp_id = uuid::Uuid::new_v4().to_string();
                    if let Ok(manifest) = crate::tools::blob_store::capture_workspace_manifest(workspace_root, &cp_id, "pre_write", true) {
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
                let exec_result = tokio::select! {
                    biased;
                    _ = session.cancel_notify.notified() => {
                        let _ = tools.pty_manager.kill_all_for_chat(chat_id);
                        Self::save_checkpoint(db, chat_id, history);
                        let _ = event_sender
                            .send(StreamEvent::Error { error: "User canceled execution".to_string() })
                            .await;
                        return Err("User canceled execution".to_string());
                    }
                    timed = tokio::time::timeout(
                        std::time::Duration::from_secs(if tool_name == "code_intel" || tool_name == "web_research" { 60 } else { 45 }),
                        tools.dispatch_tool(tool_name, &parsed_args, conn, Some(chat_id)),
                    ) => match timed {
                    Ok(res) => res,
                    Err(_) => crate::tools::ToolExecutionResult {
                        tool_name: tool_name.clone(),
                        success: false,
                        output: serde_json::json!({ "timed_out": true, "error": format!("Tool '{}' timed out", tool_name) }),
                        error: Some(format!("Tool '{}' timed out", tool_name)),
                        elapsed_ms: 45_000,
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

                let tool_msg = ChatMessage {
                    role: "tool".to_string(),
                    content: MessageContent::Text(bounded_output),
                    name: Some(tool_name.clone()),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    reasoning_content: None,
                };
                history.push(tool_msg);
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
                        history.push(ChatMessage {
                            role: "user".to_string(),
                            content: MessageContent::Parts(vec![
                                ContentPart::Text {
                                    text: "Here is the captured screen from computer_use (normalized coordinates 0..1000).".to_string(),
                                },
                                ContentPart::ImageUrl {
                                    image_url: ImageUrlDetail {
                                        url: du,
                                        detail: Some("high".to_string()),
                                    },
                                },
                            ]),
                            name: None,
                            tool_calls: None,
                            tool_call_id: None,
                            reasoning_content: None,
                        });
                    }
                }
            }

            let _ = state.transition(TaskStateEnum::Executing);
        }

        if let Some(c) = contract.as_mut() {
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
        } else if !session.is_active() {
            let _ = tools.pty_manager.kill_all_for_chat(chat_id);
            Self::save_checkpoint(db, chat_id, history);
            let _ = event_sender
                .send(StreamEvent::Error {
                    error: "User canceled execution".to_string(),
                })
                .await;
            return Err("User canceled execution".to_string());
        } else {
            let _ = state.transition(TaskStateEnum::Completed);
        }

        if let Some(d) = db {
            let _ = d.delete_checkpoint(chat_id);
        }
        Ok(state)
    }
}
