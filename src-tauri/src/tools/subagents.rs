use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubagentRole {
    Research,
    CodeReview,
    BugHunter,
    ArchReview,
    TestPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: FindingSeverity,
    pub file_reference: Option<String>,
    pub line_number: Option<usize>,
    pub proposed_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentResult {
    pub role: SubagentRole,
    pub summary: String,
    pub findings: Vec<Finding>,
    pub recommendations: Vec<String>,
    pub confidence_score: f32,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentTask {
    pub task_id: String,
    pub role: SubagentRole,
    pub prompt: String,
    pub context_files: Vec<String>,
    pub code_diff: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinedSubagentReport {
    pub session_id: String,
    pub results: Vec<SubagentResult>,
    pub total_critical_findings: usize,
    pub total_high_findings: usize,
    pub total_medium_findings: usize,
    pub overall_summary: String,
    pub consensus_reached: bool,
}

pub struct SubagentWorker;

impl SubagentWorker {
    pub fn execute_research(task: &SubagentTask) -> SubagentResult {
        let start = std::time::Instant::now();
        let mut findings = Vec::new();
        let mut recommendations = Vec::new();

        let prompt_lower = task.prompt.to_lowercase();
        if prompt_lower.contains("architecture") || prompt_lower.contains("pattern") {
            recommendations.push("Inspect module dependency boundaries before refactoring".to_string());
        }
        if prompt_lower.contains("security") || prompt_lower.contains("auth") {
            recommendations.push("Review token storage and transmission endpoints for cryptographic guarantees".to_string());
        }

        findings.push(Finding {
            id: Uuid::new_v4().to_string(),
            title: "Context Scope Analysis".to_string(),
            description: format!("Analyzed {} target files and prompt requirements", task.context_files.len()),
            severity: FindingSeverity::Info,
            file_reference: task.context_files.first().cloned(),
            line_number: None,
            proposed_fix: None,
        });

        SubagentResult {
            role: SubagentRole::Research,
            summary: format!("Completed research phase on {} files", task.context_files.len()),
            findings,
            recommendations,
            confidence_score: 0.92,
            elapsed_ms: start.elapsed().as_millis(),
        }
    }

    pub fn execute_code_review(task: &SubagentTask) -> SubagentResult {
        let start = std::time::Instant::now();
        let mut findings = Vec::new();
        let mut recommendations = Vec::new();

        if let Some(ref diff) = task.code_diff {
            for (idx, line) in diff.lines().enumerate() {
                if line.contains("unwrap()") {
                    findings.push(Finding {
                        id: Uuid::new_v4().to_string(),
                        title: "Potential Panic On Unwrap".to_string(),
                        description: "Use of unwrap() without explicit error handling or fallback".to_string(),
                        severity: FindingSeverity::High,
                        file_reference: None,
                        line_number: Some(idx + 1),
                        proposed_fix: Some("Replace unwrap() with match, if-let, or ok_or_else(?)".to_string()),
                    });
                }
                if line.contains("unsafe ") {
                    findings.push(Finding {
                        id: Uuid::new_v4().to_string(),
                        title: "Unsafe Block Detected".to_string(),
                        description: "Unsafe block must have strict invariant validation and boundary checks".to_string(),
                        severity: FindingSeverity::Critical,
                        file_reference: None,
                        line_number: Some(idx + 1),
                        proposed_fix: Some("Wrap in safe encapsulation with preconditions".to_string()),
                    });
                }
            }
        }

        if findings.is_empty() {
            recommendations.push("Code patterns adhere to standard hygiene rules".to_string());
        } else {
            recommendations.push("Resolve critical and high severity safety findings prior to merging".to_string());
        }

        SubagentResult {
            role: SubagentRole::CodeReview,
            summary: format!("Code review completed with {} findings", findings.len()),
            findings,
            recommendations,
            confidence_score: 0.95,
            elapsed_ms: start.elapsed().as_millis(),
        }
    }

    pub fn execute_bug_hunter(task: &SubagentTask) -> SubagentResult {
        let start = std::time::Instant::now();
        let mut findings = Vec::new();
        let mut recommendations = Vec::new();

        if let Some(ref diff) = task.code_diff {
            if diff.contains("lock().unwrap()") && diff.contains("recv()") {
                findings.push(Finding {
                    id: Uuid::new_v4().to_string(),
                    title: "Potential Lock Contention / Deadlock".to_string(),
                    description: "Holding a lock while awaiting channel reception can cause deadlocks".to_string(),
                    severity: FindingSeverity::Critical,
                    file_reference: None,
                    line_number: None,
                    proposed_fix: Some("Release mutex lock before blocking channel operations".to_string()),
                });
            }
            if diff.contains("clone()") && diff.lines().count() > 500 {
                findings.push(Finding {
                    id: Uuid::new_v4().to_string(),
                    title: "Frequent Memory Cloning".to_string(),
                    description: "Excessive clone calls in large loops or structures".to_string(),
                    severity: FindingSeverity::Low,
                    file_reference: None,
                    line_number: None,
                    proposed_fix: Some("Use Arc, Cow, or borrowing references where possible".to_string()),
                });
            }
        }

        recommendations.push("Ensure full edge case test coverage for boundary inputs".to_string());

        SubagentResult {
            role: SubagentRole::BugHunter,
            summary: format!("Bug hunter pass finished with {} potential issues", findings.len()),
            findings,
            recommendations,
            confidence_score: 0.88,
            elapsed_ms: start.elapsed().as_millis(),
        }
    }

    pub fn execute_arch_review(task: &SubagentTask) -> SubagentResult {
        let start = std::time::Instant::now();
        let mut findings = Vec::new();
        let mut recommendations = Vec::new();

        if task.context_files.len() > 10 {
            findings.push(Finding {
                id: Uuid::new_v4().to_string(),
                title: "Broad File Scope".to_string(),
                description: "Change spans over 10 files; ensure cohesive separation of concerns".to_string(),
                severity: FindingSeverity::Medium,
                file_reference: None,
                line_number: None,
                proposed_fix: Some("Split changes into granular modular commits".to_string()),
            });
        }

        recommendations.push("Verify unidirectional dependency flow between layers".to_string());
        recommendations.push("Keep business logic decoupled from presentation layer".to_string());

        SubagentResult {
            role: SubagentRole::ArchReview,
            summary: "Architecture and design review completed".to_string(),
            findings,
            recommendations,
            confidence_score: 0.90,
            elapsed_ms: start.elapsed().as_millis(),
        }
    }

    pub fn execute_test_plan(_task: &SubagentTask) -> SubagentResult {
        let start = std::time::Instant::now();
        let mut findings = Vec::new();
        let mut recommendations = Vec::new();

        recommendations.push("Add happy path unit test for every public method".to_string());
        recommendations.push("Add boundary / off-by-one tests for string slicing and offsets".to_string());
        recommendations.push("Add negative testing for invalid input formatting and unexpected EOF".to_string());
        recommendations.push("Test concurrent access with multiple reader threads".to_string());

        findings.push(Finding {
            id: Uuid::new_v4().to_string(),
            title: "Test Coverage Strategy".to_string(),
            description: "Coverage should encompass snapshot persistence, rollback semantics, and timeout limits".to_string(),
            severity: FindingSeverity::Info,
            file_reference: None,
            line_number: None,
            proposed_fix: None,
        });

        SubagentResult {
            role: SubagentRole::TestPlan,
            summary: "Test plan generated with 4 standard verification suites".to_string(),
            findings,
            recommendations,
            confidence_score: 0.94,
            elapsed_ms: start.elapsed().as_millis(),
        }
    }
}

pub struct SubagentCoordinator;

impl SubagentCoordinator {
    pub async fn run_subagent(task: SubagentTask) -> SubagentResult {
        tokio::task::spawn_blocking(move || match task.role {
            SubagentRole::Research => SubagentWorker::execute_research(&task),
            SubagentRole::CodeReview => SubagentWorker::execute_code_review(&task),
            SubagentRole::BugHunter => SubagentWorker::execute_bug_hunter(&task),
            SubagentRole::ArchReview => SubagentWorker::execute_arch_review(&task),
            SubagentRole::TestPlan => SubagentWorker::execute_test_plan(&task),
        })
        .await
        .unwrap_or_else(|_| SubagentResult {
            role: SubagentRole::Research,
            summary: "Task execution panicked".to_string(),
            findings: Vec::new(),
            recommendations: Vec::new(),
            confidence_score: 0.0,
            elapsed_ms: 0,
        })
    }

    pub async fn run_parallel_subagents(
        tasks: Vec<SubagentTask>,
    ) -> CombinedSubagentReport {
        let session_id = Uuid::new_v4().to_string();
        let mut handles = Vec::new();

        for task in tasks {
            let handle = tokio::spawn(async move {
                Self::run_subagent(task).await
            });
            handles.push(handle);
        }

        let mut results = Vec::new();
        for h in handles {
            if let Ok(res) = h.await {
                results.push(res);
            }
        }

        let mut critical_count = 0;
        let mut high_count = 0;
        let mut medium_count = 0;

        for r in &results {
            for f in &r.findings {
                match f.severity {
                    FindingSeverity::Critical => critical_count += 1,
                    FindingSeverity::High => high_count += 1,
                    FindingSeverity::Medium => medium_count += 1,
                    _ => {}
                }
            }
        }

        let consensus_reached = critical_count == 0;
        let overall_summary = format!(
            "Combined subagent review completed. {} critical, {} high, {} medium issues detected across {} agent passes.",
            critical_count,
            high_count,
            medium_count,
            results.len()
        );

        CombinedSubagentReport {
            session_id,
            results,
            total_critical_findings: critical_count,
            total_high_findings: high_count,
            total_medium_findings: medium_count,
            overall_summary,
            consensus_reached,
        }
    }
}
