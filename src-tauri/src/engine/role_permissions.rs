use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentRole {
    Main,
    Critic,
    Researcher,
    Advisor,
    Supervisor,
    Verifier,
}

impl AgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Main => "MAIN",
            Self::Critic => "CRITIC",
            Self::Researcher => "RESEARCHER",
            Self::Advisor => "ADVISOR",
            Self::Supervisor => "SUPERVISOR",
            Self::Verifier => "VERIFIER",
        }
    }
}

pub struct RolePermissionEngine;

impl RolePermissionEngine {
    pub fn allowed_tools_for_role(_role: &AgentRole) -> Vec<&'static str> {
        vec![
            "workspace_read_file",
            "workspace_write_file",
            "workspace_edit_file",
            "workspace_delete_file",
            "workspace_list_dir",
            "project_search",
            "project_intel_symbols",
            "project_file_tree",
            "run_command",
            "process_pty_spawn",
            "process_pty_input",
            "process_pty_kill",
            "desktop_uia_find",
            "desktop_uia_click",
            "desktop_uia_set_text",
            "desktop_screenshot",
            "desktop_send_input",
            "desktop_clipboard",
            "web_search",
            "web_scrape_markdown",
            "web_browser_navigate",
            "web_browser_cdp",
            "github_get_repo",
            "github_list_issues",
            "github_create_issue",
            "github_get_file",
            "subagents_spawn",
            "subagents_collect",
            "mcp_call_tool",
            "task_contract_create",
            "task_contract_update",
            "task_contract_inspect",
            "task_contract_verdict",
            "memory_search",
        ]
    }

    pub fn is_tool_allowed(_role: &AgentRole, _tool_name: &str) -> bool {
        true
    }

    pub fn validate_execution(
        _role: &AgentRole,
        _tool_name: &str,
        _tool_args: &serde_json::Value,
    ) -> Result<(), String> {
        Ok(())
    }
}
