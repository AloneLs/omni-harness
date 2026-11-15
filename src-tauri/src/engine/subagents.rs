use crate::engine::context_engine::{compute_prompt_cache_key, spill_tool_output_if_needed};
use crate::engine::stream_manager::SessionHandle;
use crate::llm::request_compat::{ChatMessage, ChatRequest, FunctionCall, FunctionSpec, MessageContent, ToolCall, ToolDefinition};
use crate::llm::{dispatch_stream_with_protocol, CapabilityRegistry, KeyPool, StreamEvent};
use crate::tools::ToolDispatcher;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomSubagentDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default, rename = "disallowedTools")]
    pub disallowed_tools: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, rename = "maxTurns")]
    pub max_turns: Option<u32>,
    #[serde(skip)]
    pub system_prompt: String,
}

#[derive(Debug, Clone)]
struct SubagentTranscript {
    history: Vec<ChatMessage>,
}

static SUBAGENT_STORE: OnceLock<Mutex<HashMap<String, SubagentTranscript>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, SubagentTranscript>> {
    SUBAGENT_STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_custom_agents(workspace_root: &str) -> Vec<CustomSubagentDef> {
    let mut roots = Vec::new();
    let ws = Path::new(workspace_root);
    roots.push(ws.join(".omni").join("agents"));
    roots.push(ws.join(".agents").join("agents"));
    roots.push(ws.join(".claude").join("agents"));
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let h = Path::new(&home);
        roots.push(h.join(".omni").join("agents"));
        roots.push(h.join(".agents").join("agents"));
        roots.push(h.join(".claude").join("agents"));
    }

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for dir in roots {
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&p) else {
                continue;
            };
            let trimmed = raw.trim_start();
            if !trimmed.starts_with("---") {
                continue;
            }
            let rest = &trimmed[3..];
            let Some(end_idx) = rest.find("---") else {
                continue;
            };
            let yaml_str = &rest[..end_idx];
            let body = rest[end_idx + 3..].trim().to_string();
            if let Ok(mut def) = serde_yaml::from_str::<CustomSubagentDef>(yaml_str) {
                if def.name.trim().is_empty() {
                    continue;
                }
                let key = def.name.to_ascii_lowercase();
                if seen.insert(key) {
                    def.system_prompt = body;
                    out.push(def);
                }
            }
        }
    }
    out
}

fn map_tool_alias(t: &str) -> &str {
    match t.trim().to_ascii_lowercase().as_str() {
        "read" | "read_file" => "read_file",
        "write" | "write_file" => "write_file",
        "edit" | "edit_file" => "edit_file",
        "delete" | "delete_file" => "delete_file",
        "glob" => "glob",
        "grep" => "grep",
        "bash" | "terminal_exec" => "terminal_exec",
        "terminal_io" => "terminal_io",
        "webfetch" | "websearch" | "web_research" => "web_research",
        "code_intel" | "lsp" => "code_intel",
        "skill" | "tool_hub" => "tool_hub",
        "memory" | "memory_tool" => "memory_tool",
        other => {
            let _ = other;
            ""
        }
    }
}

pub fn allowed_tools_for_subagent(
    subagent_type: &str,
    custom_def: Option<&CustomSubagentDef>,
) -> Vec<&'static str> {
    if let Some(def) = custom_def {
        if !def.tools.is_empty() {
            let mut list: Vec<&'static str> = Vec::new();
            for raw in &def.tools {
                let mapped = map_tool_alias(raw);
                if !mapped.is_empty() {
                    let static_name: Option<&'static str> = match mapped {
                        "read_file" => Some("read_file"),
                        "write_file" => Some("write_file"),
                        "edit_file" => Some("edit_file"),
                        "delete_file" => Some("delete_file"),
                        "glob" => Some("glob"),
                        "grep" => Some("grep"),
                        "terminal_exec" => Some("terminal_exec"),
                        "terminal_io" => Some("terminal_io"),
                        "web_research" => Some("web_research"),
                        "code_intel" => Some("code_intel"),
                        "tool_hub" => Some("tool_hub"),
                        "memory_tool" => Some("memory_tool"),
                        _ => None,
                    };
                    if let Some(s) = static_name {
                        if !list.contains(&s) {
                            list.push(s);
                        }
                    }
                }
            }
            if !list.is_empty() {
                return list;
            }
        }
    }

    match subagent_type.trim().to_ascii_lowercase().as_str() {
        "explore" => vec!["read_file", "glob", "grep", "code_intel", "web_research"],
        "plan" => vec!["read_file", "glob", "grep", "code_intel", "web_research"],
        "verification" | "verifier" => vec![
            "read_file",
            "glob",
            "grep",
            "code_intel",
            "terminal_exec",
            "terminal_io",
        ],
        _ => vec![
            "read_file",
            "write_file",
            "edit_file",
            "delete_file",
            "glob",
            "grep",
            "code_intel",
            "terminal_exec",
            "terminal_io",
            "web_research",
            "tool_hub",
            "memory_tool",
        ],
    }
}

pub fn build_subagent_system_prompt(
    subagent_type: &str,
    workspace_root: &str,
    custom_def: Option<&CustomSubagentDef>,
) -> String {
    if let Some(def) = custom_def {
        return format!(
            "{}\n\nWorking directory: {}\nExecute within your allowed tools and return a concise, factual report with exact file paths and symbol names.",
            def.system_prompt, workspace_root
        );
    }

    match subagent_type.trim().to_ascii_lowercase().as_str() {
        "explore" => format!(
            "You are a fast, read-only codebase exploration subagent (Explore).\n\
Working directory: {}\n\
Rules:\n\
- Strictly READ-ONLY. Never attempt to create, modify, or delete files or run shell commands.\n\
- Maximize parallel read-only tool calls (glob, grep, read_file, code_intel) per turn to locate files and trace symbols rapidly.\n\
- Never guess file contents or line numbers; verify every cited symbol with read_file/grep.\n\
- Return a dense, factual report citing exact file paths, line numbers, and symbol names. No filler.",
            workspace_root
        ),
        "plan" => format!(
            "You are a software architecture planning subagent (Plan).\n\
Working directory: {}\n\
Rules:\n\
- Strictly READ-ONLY. Explore the codebase thoroughly before proposing changes; never modify files.\n\
- Identify affected files, module boundaries, edge cases, build/test commands, and architectural trade-offs.\n\
- Return a concrete, step-by-step implementation plan referencing exact file paths, types, and functions.",
            workspace_root
        ),
        "verification" | "verifier" => format!(
            "You are an adversarial read-only verification subagent (Verification).\n\
Working directory: {}\n\
Rules:\n\
- You MUST NOT modify source files (no write/edit/delete).\n\
- Inspect the modified code via read_file/grep and actually execute builds, linters, and tests via terminal_exec.\n\
- Try to break the implementation and check edge cases.\n\
- End with a clear verdict: PASS, FAIL, or PARTIAL, backed by exact command outputs and exit codes.",
            workspace_root
        ),
        _ => format!(
            "You are an autonomous general-purpose subagent operating in an isolated context window.\n\
Working directory: {}\n\
Rules:\n\
- Complete the delegated task end-to-end using your available tools (read, write, edit, glob, grep, terminal_exec).\n\
- Verify all code changes with actual build/test commands before reporting completion.\n\
- You cannot spawn nested subagents.\n\
- Return a concise, factual summary of changes made, files touched, and verification results.",
            workspace_root
        ),
    }
}

pub struct SubagentRunner;

impl SubagentRunner {
    pub async fn run(
        http_client: &reqwest::Client,
        chat_id: &str,
        workspace_root: &str,
        tools: Arc<ToolDispatcher>,
        key_pool: &KeyPool,
        caps_registry: &CapabilityRegistry,
        provider_name: &str,
        base_url: &str,
        api_protocol: Option<&str>,
        model_name: &str,
        session: &SessionHandle,
        subagent_type: &str,
        prompt: &str,
        resume_id: Option<&str>,
        max_turns_override: Option<u32>,
    ) -> Result<(String, String), String> {
        let custom_agents = load_custom_agents(workspace_root);
        let custom_def = custom_agents
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(subagent_type))
            .cloned();

        let effective_type = if subagent_type.trim().is_empty() {
            "general-purpose"
        } else {
            subagent_type.trim()
        };

        let mut allowed = allowed_tools_for_subagent(effective_type, custom_def.as_ref());
        if let Some(ref def) = custom_def {
            if !def.disallowed_tools.is_empty() {
                let dis: Vec<&str> = def.disallowed_tools.iter().map(|s| map_tool_alias(s)).collect();
                allowed.retain(|t| !dis.contains(t));
            }
        }

        let mut sub_tools = Vec::new();
        for decl in tools.get_all_tool_declarations() {
            if allowed.contains(&decl.name.as_str()) {
                sub_tools.push(ToolDefinition {
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

        let agent_id = resume_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("agent-{}", &uuid::Uuid::new_v4().to_string()[..8]));

        let mut sub_history = if let Some(rid) = resume_id.filter(|s| !s.trim().is_empty()) {
            let guard = store().lock().await;
            if let Some(existing) = guard.get(rid) {
                let mut h = existing.history.clone();
                h.push(ChatMessage::user(prompt.to_string()));
                h
            } else {
                vec![
                    ChatMessage::system(build_subagent_system_prompt(
                        effective_type,
                        workspace_root,
                        custom_def.as_ref(),
                    )),
                    ChatMessage::user(prompt.to_string()),
                ]
            }
        } else {
            vec![
                ChatMessage::system(build_subagent_system_prompt(
                    effective_type,
                    workspace_root,
                    custom_def.as_ref(),
                )),
                ChatMessage::user(prompt.to_string()),
            ]
        };

        let max_turns = max_turns_override
            .or_else(|| custom_def.as_ref().and_then(|d| d.max_turns))
            .unwrap_or_else(|| match effective_type.to_ascii_lowercase().as_str() {
                "explore" | "plan" | "verification" | "verifier" => 25,
                _ => 40,
            })
            .clamp(1, 80);

        let target_model = custom_def
            .as_ref()
            .and_then(|d| d.model.as_deref())
            .filter(|m| !m.trim().is_empty() && !m.eq_ignore_ascii_case("inherit"))
            .unwrap_or(model_name)
            .to_string();

        let cache_key = compute_prompt_cache_key(workspace_root, &format!("{}-{}", chat_id, agent_id));

        let mut final_output = String::new();
        for _step in 0..max_turns {
            if !session.is_active() {
                return Err("User canceled execution".to_string());
            }

            let req = ChatRequest {
                model: target_model.clone(),
                messages: sub_history.clone(),
                temperature: Some(0.15),
                top_p: Some(0.95),
                max_tokens: None,
                max_completion_tokens: None,
                tools: if sub_tools.is_empty() {
                    None
                } else {
                    Some(sub_tools.clone())
                },
                tool_choice: None,
                reasoning_effort: None,
                thinking_budget: None,
                response_format: None,
                prompt_cache_key: Some(cache_key.clone()),
                stream: true,
            };

            let (tx, mut rx) = mpsc::channel(64);
            let p = provider_name.to_string();
            let b = base_url.to_string();
            let proto = api_protocol.map(|s| s.to_string());
            let kp = key_pool.clone();
            let cr = caps_registry.clone();
            let c = http_client.clone();

            let join = tokio::spawn(async move {
                dispatch_stream_with_protocol(&c, &p, &b, proto.as_deref(), &kp, &cr, req, tx).await
            });

            let mut text = String::new();
            let mut tcs: Vec<ToolCall> = Vec::new();
            loop {
                let evt = tokio::select! {
                    biased;
                    _ = session.cancel_notify.notified() => {
                        join.abort();
                        return Err("User canceled execution".to_string());
                    }
                    maybe = rx.recv() => match maybe {
                        Some(e) => e,
                        None => break,
                    }
                };
                match evt {
                    StreamEvent::ContentDelta { delta } => text.push_str(&delta),
                    StreamEvent::ToolCallComplete { id, name, arguments, .. } => {
                        tcs.push(ToolCall {
                            id,
                            call_type: "function".to_string(),
                            function: FunctionCall { name, arguments },
                        });
                    }
                    _ => {}
                }
            }
            join.await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;

            let mut asst = ChatMessage::assistant(text.clone());
            if !tcs.is_empty() {
                asst.tool_calls = Some(tcs.clone());
            }
            sub_history.push(asst);

            if tcs.is_empty() {
                final_output = text;
                break;
            }

            for tc in tcs {
                if !allowed.contains(&tc.function.name.as_str()) {
                    sub_history.push(ChatMessage {
                        role: "tool".to_string(),
                        content: MessageContent::Text(
                            serde_json::json!({
                                "error": format!("Tool '{}' is denied for subagent type '{}'", tc.function.name, effective_type)
                            })
                            .to_string(),
                        ),
                        name: Some(tc.function.name),
                        tool_calls: None,
                        tool_call_id: Some(tc.id),
                        reasoning_content: None,
                    });
                    continue;
                }
                let args: Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(serde_json::json!({}));
                let res = tools
                    .dispatch_tool(&tc.function.name, &args, None, Some(chat_id))
                    .await;
                let raw = if res.success {
                    serde_json::to_string(&res.output).unwrap_or_else(|_| "{}".to_string())
                } else {
                    serde_json::json!({ "error": res.error.unwrap_or_default() }).to_string()
                };
                let bounded = spill_tool_output_if_needed(workspace_root, &tc.id, &raw);
                sub_history.push(ChatMessage {
                    role: "tool".to_string(),
                    content: MessageContent::Text(bounded),
                    name: Some(tc.function.name),
                    tool_calls: None,
                    tool_call_id: Some(tc.id),
                    reasoning_content: None,
                });
            }
        }

        if final_output.is_empty() {
            final_output = "Subagent reached turn limit without final text summary.".to_string();
        }

        {
            let mut guard = store().lock().await;
            guard.insert(
                agent_id.clone(),
                SubagentTranscript {
                    history: sub_history,
                },
            );
        }

        Ok((agent_id, final_output))
    }
}
