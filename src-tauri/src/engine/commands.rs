use crate::engine::task_contract::{CriterionStatus, TaskContract};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GoalSubcommand {
    Add { description: String },
    Edit { id: String, description: String },
    Remove { id: String },
    List,
    Status,
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlashCommand {
    Goal(GoalSubcommand),
    Research { query: String },
    Swarm { task: String },
    Skills,
    SkillInvoke { skill_name: String, args: String },
}

pub struct SlashCommandParser;

impl SlashCommandParser {
    pub fn parse(input: &str) -> Option<SlashCommand> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        let without_slash = &trimmed[1..];
        let mut parts = without_slash.split_whitespace();
        let cmd = parts.next()?.to_lowercase();
        let rest = without_slash.strip_prefix(&cmd)?.trim();

        match cmd.as_str() {
            "goal" => {
                let mut sub_parts = rest.split_whitespace();
                let sub_action = sub_parts.next().unwrap_or("list").to_lowercase();
                let sub_rest = rest.strip_prefix(&sub_action).unwrap_or("").trim();

                match sub_action.as_str() {
                    "add" => Some(SlashCommand::Goal(GoalSubcommand::Add {
                        description: sub_rest.to_string(),
                    })),
                    "edit" => {
                        let mut edit_parts = sub_rest.split_whitespace();
                        let id = edit_parts.next().unwrap_or("").to_string();
                        let desc = sub_rest.strip_prefix(&id).unwrap_or("").trim().to_string();
                        Some(SlashCommand::Goal(GoalSubcommand::Edit {
                            id,
                            description: desc,
                        }))
                    }
                    "remove" | "rm" | "delete" => {
                        let id = sub_rest.split_whitespace().next().unwrap_or("").to_string();
                        Some(SlashCommand::Goal(GoalSubcommand::Remove { id }))
                    }
                    "status" => Some(SlashCommand::Goal(GoalSubcommand::Status)),
                    "clear" => Some(SlashCommand::Goal(GoalSubcommand::Clear)),
                    "list" | "" => Some(SlashCommand::Goal(GoalSubcommand::List)),
                    _ => Some(SlashCommand::Goal(GoalSubcommand::Add {
                        description: rest.to_string(),
                    })),
                }
            }
            "research" => Some(SlashCommand::Research {
                query: rest.to_string(),
            }),
            "swarm" => Some(SlashCommand::Swarm {
                task: rest.to_string(),
            }),
            "skills" => Some(SlashCommand::Skills),
            dynamic_skill => Some(SlashCommand::SkillInvoke {
                skill_name: dynamic_skill.to_string(),
                args: rest.to_string(),
            }),
        }
    }

    pub fn handle_goal(sub: GoalSubcommand, contract: &mut Option<TaskContract>) -> String {
        match sub {
            GoalSubcommand::Add { description } => {
                if description.is_empty() {
                    return "Error: Goal description cannot be empty. Usage: /goal add <description>".to_string();
                }
                if contract.is_none() {
                    *contract = Some(TaskContract::new(&description));
                    format!("Created new task contract with goal: '{}'", description)
                } else if let Some(c) = contract {
                    let id = c.add_criterion(&description);
                    format!("Added criterion {} to contract: '{}'", id, description)
                } else {
                    "Error creating goal".to_string()
                }
            }
            GoalSubcommand::Edit { id, description } => {
                if id.is_empty() || description.is_empty() {
                    return "Error: Usage: /goal edit <id> <description>".to_string();
                }
                if let Some(c) = contract {
                    if let Some(crit) = c.criteria.iter_mut().find(|item| item.id == id) {
                        crit.description = description.clone();
                        format!("Updated criterion {}: '{}'", id, description)
                    } else {
                        format!("Criterion '{}' not found in contract", id)
                    }
                } else {
                    "No active task contract. Create one with /goal add <description>".to_string()
                }
            }
            GoalSubcommand::Remove { id } => {
                if id.is_empty() {
                    return "Error: Usage: /goal remove <id>".to_string();
                }
                if let Some(c) = contract {
                    let len_before = c.criteria.len();
                    c.criteria.retain(|item| item.id != id);
                    if c.criteria.len() < len_before {
                        format!("Removed criterion '{}'", id)
                    } else {
                        format!("Criterion '{}' not found", id)
                    }
                } else {
                    "No active task contract".to_string()
                }
            }
            GoalSubcommand::List => {
                if let Some(c) = contract {
                    let mut out = format!("Task Contract Goal: {}\nStatus: {:?}\nCriteria:\n", c.goal, c.status);
                    for item in &c.criteria {
                        out.push_str(&format!("  - [{:?}] {}: {}\n", item.status, item.id, item.description));
                    }
                    out
                } else {
                    "No active task contract".to_string()
                }
            }
            GoalSubcommand::Status => {
                if let Some(c) = contract {
                    let passed = c.criteria.iter().filter(|x| x.status == CriterionStatus::Pass).count();
                    let total = c.criteria.len();
                    let gate = if c.is_completion_gate_passed() { "PASSED" } else { "PENDING" };
                    format!(
                        "Contract Goal: {}\nState: {:?}\nGate: {}\nProgress: {}/{} criteria passed\nVerdict: {:?}",
                        c.goal, c.status, gate, passed, total, c.verdict
                    )
                } else {
                    "No active task contract".to_string()
                }
            }
            GoalSubcommand::Clear => {
                *contract = None;
                "Cleared active task contract".to_string()
            }
        }
    }
}
