use crate::engine::prompt_builder::PromptBuilder;
use crate::engine::role_permissions::{AgentRole, RolePermissionEngine};
use crate::engine::stream_manager::SessionHandle;
use crate::engine::task_contract::{
    CriterionStatus, StructuredVerdict, TaskContract,
    TaskStateEnum, VerdictStatus,
};
use crate::llm::capabilities::CapabilityRegistry;
use crate::llm::key_pool::KeyPool;
use crate::llm::request_compat::{ChatMessage, ChatRequest, FunctionSpec, ToolCall, ToolDefinition};
use crate::llm::{dispatch_stream, StreamEvent};
use crate::tools::ToolDispatcher;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct DebateSwarmRunner;

impl DebateSwarmRunner {
    pub async fn run_swarm(
        chat_id: &str,
        task_prompt: &str,
        workspace_root: &str,
        tools: Arc<ToolDispatcher>,
        key_pool: &KeyPool,
        caps_registry: &CapabilityRegistry,
        provider_name: &str,
        base_url: &str,
        model_name: &str,
        conn: Option<&std::sync::Mutex<rusqlite::Connection>>,
        session: &SessionHandle,
        contract: &mut Option<TaskContract>,
        event_sender: mpsc::Sender<StreamEvent>,
    ) -> Result<TaskStateEnum, String> {
        let mut state = TaskStateEnum::Created;
        let _ = state.transition(TaskStateEnum::ContractBuilding);

        let _ = event_sender
            .send(StreamEvent::ContentDelta {
                delta: format!("\n[SWARM INITIALIZED]: Orchestrating multi-agent consensus for: {}\n\n", task_prompt),
            })
            .await;

        if contract.is_none() {
            let mut new_contract = TaskContract::new(task_prompt);
            new_contract.add_criterion("Analyze problem space, code architecture, and requirements");
            new_contract.add_criterion("Execute necessary implementation changes and verify integrity");
            new_contract.add_criterion("Independent verification audit passes with zero blocking issues");
            *contract = Some(new_contract);
        }

        let _ = state.transition(TaskStateEnum::Planning);

        let roles_in_order = [
            AgentRole::Supervisor,
            AgentRole::Researcher,
            AgentRole::Critic,
            AgentRole::Main,
            AgentRole::Verifier,
        ];

        let mut round = 0;
        let max_rounds = 4;
        let mut swarm_memory: Vec<ChatMessage> = Vec::new();

        while round < max_rounds && session.is_active() {
            round += 1;

            let _ = event_sender
                .send(StreamEvent::ContentDelta {
                    delta: format!("\n--- [SWARM ROUND {} / {}] ---\n", round, max_rounds),
                })
                .await;

            for role in &roles_in_order {
                if !session.is_active() {
                    break;
                }

                let _ = event_sender
                    .send(StreamEvent::ReasoningDelta {
                        delta: format!("[Role Transition -> {}]\n", role.as_str()),
                    })
                    .await;

                let role_prompt = PromptBuilder::build_system_prompt(
                    role,
                    workspace_root,
                    contract.as_ref(),
                    None,
                );

                let mut messages = vec![ChatMessage::system(role_prompt)];
                for prior in &swarm_memory {
                    messages.push(prior.clone());
                }

                let prompt_for_role = match role {
                    AgentRole::Supervisor => format!(
                        "Supervisor Objective: Review the task '{}' and current swarm state. Direct the swarm with clear next steps.",
                        task_prompt
                    ),
                    AgentRole::Researcher => {
                        "Researcher Objective: Examine project files, symbols, or external technical references needed for this round.".to_string()
                    }
                    AgentRole::Critic => {
                        "Critic Objective: Scrutinize the proposed actions, uncover edge cases, security risks, or missing criteria.".to_string()
                    }
                    AgentRole::Main => {
                        "Main Executor Objective: Implement the solution, apply file modifications, and resolve identified issues.".to_string()
                    }
                    AgentRole::Verifier => {
                        PromptBuilder::build_verifier_prompt(contract.as_ref().unwrap(), workspace_root)
                    }
                    AgentRole::Advisor => {
                        "Advisor Objective: Provide strategic recommendations.".to_string()
                    }
                };

                messages.push(ChatMessage::user(prompt_for_role));

                let tool_decls = tools.get_all_tool_declarations();
                let mut available_tools = Vec::new();
                for decl in tool_decls {
                    if RolePermissionEngine::is_tool_allowed(role, &decl.name) {
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

                let chat_req = ChatRequest {
                    model: model_name.to_string(),
                    messages,
                    temperature: Some(0.3),
                    top_p: Some(0.95),
                    max_tokens: Some(3072),
                    max_completion_tokens: None,
                    tools: if available_tools.is_empty() {
                        None
                    } else {
                        Some(available_tools)
                    },
                    tool_choice: None,
                    reasoning_effort: None,
                    thinking_budget: None,
                    response_format: None,
                    prompt_cache_key: None,
                    stream: true,
                };

                let (tx, mut rx) = mpsc::channel(64);
                let client = reqwest::Client::new();
                let p = provider_name.to_string();
                let b = base_url.to_string();
                let kp = key_pool.clone();
                let cr = caps_registry.clone();

                tokio::spawn(async move {
                    dispatch_stream(&client, &p, &b, &kp, &cr, chat_req, tx).await
                });

                let mut role_output = String::new();
                let mut role_tools: Vec<ToolCall> = Vec::new();

                let _ = event_sender
                    .send(StreamEvent::ContentDelta {
                        delta: format!("\n**[{}]**:\n", role.as_str()),
                    })
                    .await;

                while let Some(evt) = rx.recv().await {
                    match &evt {
                        StreamEvent::ContentDelta { delta } => {
                            role_output.push_str(delta);
                            let _ = event_sender.send(evt).await;
                        }
                        StreamEvent::ReasoningDelta { .. } => {
                            let _ = event_sender.send(evt).await;
                        }
                        StreamEvent::ToolCallComplete {
                            id,
                            name,
                            arguments,
                            ..
                        } => {
                            role_tools.push(ToolCall {
                                id: id.clone(),
                                call_type: "function".to_string(),
                                function: crate::llm::request_compat::FunctionCall {
                                    name: name.clone(),
                                    arguments: arguments.clone(),
                                },
                            });
                            let _ = event_sender.send(evt).await;
                        }
                        _ => {}
                    }
                }

                for tc in role_tools {
                    let tool_name = &tc.function.name;
                    let parsed_args: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                    if let Ok(()) = RolePermissionEngine::validate_execution(
                        role,
                        tool_name,
                        &parsed_args,
                    ) {
                        let res = tools
                            .dispatch_tool(tool_name, &parsed_args, conn, Some(chat_id))
                            .await;
                        let output_text = if res.success {
                            serde_json::to_string(&res.output).unwrap_or_default()
                        } else {
                            res.error.unwrap_or_else(|| "Tool failed".to_string())
                        };

                        let _ = event_sender
                            .send(StreamEvent::ContentDelta {
                                delta: format!("\n*[{} Tool Output: {}]*\n", role.as_str(), tool_name),
                            })
                            .await;

                        swarm_memory.push(ChatMessage {
                            role: "tool".to_string(),
                            content: crate::llm::request_compat::MessageContent::Text(output_text),
                            name: Some(tool_name.clone()),
                            tool_calls: None,
                            tool_call_id: Some(tc.id),
                            reasoning_content: None,
                        });
                    }
                }

                if !role_output.is_empty() {
                    swarm_memory.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: crate::llm::request_compat::MessageContent::Text(format!(
                            "[{}] {}",
                            role.as_str(),
                            role_output
                        )),
                        name: Some(role.as_str().to_lowercase()),
                        tool_calls: None,
                        tool_call_id: None,
                        reasoning_content: None,
                    });
                }

                if *role == AgentRole::Verifier {
                    if let Some(c) = contract.as_mut() {
                        let mut parsed_verdict: Option<StructuredVerdict> = None;
                        if let Some(json_start) = role_output.find('{') {
                            if let Some(json_end) = role_output.rfind('}') {
                                let json_candidate = &role_output[json_start..=json_end];
                                if let Ok(val) = serde_json::from_str::<Value>(json_candidate) {
                                    let status_str = val
                                        .get("status")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("INCONCLUSIVE");
                                    let v_status = match status_str {
                                        "PASSED" => VerdictStatus::Passed,
                                        "FAILED" => VerdictStatus::Failed,
                                        _ => VerdictStatus::Inconclusive,
                                    };
                                    let reasoning = val
                                        .get("reasoning")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Verifier evaluation completed")
                                        .to_string();
                                    let confidence = val
                                        .get("confidence")
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(0.9) as f32;
                                    let blocking_issues = val
                                        .get("blockingIssues")
                                        .and_then(|v| v.as_array())
                                        .map(|arr| {
                                            arr.iter()
                                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    let repair_instructions = val
                                        .get("repairInstructions")
                                        .and_then(|v| v.as_array())
                                        .map(|arr| {
                                            arr.iter()
                                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                                .collect()
                                        })
                                        .unwrap_or_default();

                                    parsed_verdict = Some(StructuredVerdict {
                                        status: v_status,
                                        reasoning,
                                        confidence,
                                        evaluated_at: chrono::Utc::now().to_rfc3339(),
                                        blocking_issues,
                                        repair_instructions,
                                    });
                                }
                            }
                        }

                        if let Some(v) = parsed_verdict {
                            if v.status == VerdictStatus::Passed {
                                for item in &mut c.criteria {
                                    item.status = CriterionStatus::Pass;
                                }
                                c.set_verdict(v);
                                let _ = state.transition(TaskStateEnum::Completed);

                                let _ = event_sender
                                    .send(StreamEvent::ContentDelta {
                                        delta: "\n\n**[SWARM GATE PASSED]**: All acceptance criteria satisfied. Task successfully completed!\n".to_string(),
                                    })
                                    .await;

                                return Ok(state);
                            } else {
                                c.set_verdict(v);
                                let _ = state.transition(TaskStateEnum::Repairing);
                            }
                        }
                    }
                }
            }
        }

        if let Some(c) = contract.as_mut() {
            if c.is_completion_gate_passed() {
                let _ = state.transition(TaskStateEnum::Completed);
            } else {
                let _ = state.transition(TaskStateEnum::Failed);
            }
        } else {
            let _ = state.transition(TaskStateEnum::Completed);
        }

        Ok(state)
    }
}
