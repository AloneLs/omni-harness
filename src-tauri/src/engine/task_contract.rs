use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStateEnum {
    Created,
    ContractBuilding,
    Planning,
    Executing,
    WaitingTool,
    Reviewing,
    Verifying,
    Repairing,
    Blocked,
    Failed,
    Completed,
}

impl TaskStateEnum {
    pub fn can_transition_to(&self, target: &TaskStateEnum) -> bool {
        match (self, target) {
            (Self::Created, Self::ContractBuilding) => true,
            (Self::Created, Self::Planning) => true,
            (Self::Created, Self::Blocked) => true,
            (Self::Created, Self::Failed) => true,

            (Self::ContractBuilding, Self::Planning) => true,
            (Self::ContractBuilding, Self::Blocked) => true,
            (Self::ContractBuilding, Self::Failed) => true,

            (Self::Planning, Self::Executing) => true,
            (Self::Planning, Self::Blocked) => true,
            (Self::Planning, Self::Failed) => true,

            (Self::Executing, Self::WaitingTool) => true,
            (Self::Executing, Self::Reviewing) => true,
            (Self::Executing, Self::Repairing) => true,
            (Self::Executing, Self::Blocked) => true,
            (Self::Executing, Self::Failed) => true,

            (Self::WaitingTool, Self::Executing) => true,
            (Self::WaitingTool, Self::Blocked) => true,
            (Self::WaitingTool, Self::Failed) => true,

            (Self::Reviewing, Self::Verifying) => true,
            (Self::Reviewing, Self::Repairing) => true,
            (Self::Reviewing, Self::Blocked) => true,
            (Self::Reviewing, Self::Failed) => true,

            (Self::Verifying, Self::Completed) => true,
            (Self::Verifying, Self::Repairing) => true,
            (Self::Verifying, Self::Blocked) => true,
            (Self::Verifying, Self::Failed) => true,

            (Self::Repairing, Self::Planning) => true,
            (Self::Repairing, Self::Executing) => true,
            (Self::Repairing, Self::Blocked) => true,
            (Self::Repairing, Self::Failed) => true,

            (Self::Blocked, Self::Planning) => true,
            (Self::Blocked, Self::Executing) => true,
            (Self::Blocked, Self::Failed) => true,

            (Self::Failed, Self::Created) => true,
            (Self::Completed, Self::Created) => true,

            _ => false,
        }
    }

    pub fn transition(&mut self, next: TaskStateEnum) -> Result<(), String> {
        if self.can_transition_to(&next) {
            *self = next;
            Ok(())
        } else {
            Err(format!(
                "Invalid task state transition from {:?} to {:?}",
                self, next
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CriterionStatus {
    Pending,
    InProgress,
    Pass,
    Fail,
    Blocked,
    NotVerified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriterionEvidence {
    #[serde(rename = "type")]
    pub evidence_type: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceCriterion {
    pub id: String,
    pub description: String,
    pub status: CriterionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<CriterionEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerdictStatus {
    Passed,
    Failed,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredVerdict {
    pub status: VerdictStatus,
    pub reasoning: String,
    pub confidence: f32,
    pub evaluated_at: String,
    pub blocking_issues: Vec<String>,
    #[serde(default)]
    pub repair_instructions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContractStatus {
    Pending,
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskContract {
    pub goal: String,
    pub status: ContractStatus,
    pub created_at: String,
    pub criteria: Vec<AcceptanceCriterion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<StructuredVerdict>,
    #[serde(default)]
    pub definition_of_done: Vec<String>,
}

impl TaskContract {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            status: ContractStatus::Active,
            created_at: chrono::Utc::now().to_rfc3339(),
            criteria: Vec::new(),
            verdict: None,
            definition_of_done: vec![
                "All acceptance criteria are in PASS status".to_string(),
                "Independent verifier verdict is PASSED with zero blocking issues".to_string(),
                "All code and artifacts compile and validate without errors".to_string(),
            ],
        }
    }

    pub fn add_criterion(&mut self, description: impl Into<String>) -> String {
        let id = format!("crit-{}", self.criteria.len() + 1);
        self.criteria.push(AcceptanceCriterion {
            id: id.clone(),
            description: description.into(),
            status: CriterionStatus::Pending,
            evidence: None,
        });
        id
    }

    pub fn update_criterion_status(
        &mut self,
        id: &str,
        status: CriterionStatus,
        evidence: Option<CriterionEvidence>,
    ) -> bool {
        if let Some(item) = self.criteria.iter_mut().find(|c| c.id == id) {
            item.status = status;
            if evidence.is_some() {
                item.evidence = evidence;
            }
            true
        } else {
            false
        }
    }

    pub fn set_verdict(&mut self, verdict: StructuredVerdict) {
        if verdict.status == VerdictStatus::Passed && self.is_all_criteria_passed() {
            self.status = ContractStatus::Completed;
        } else if verdict.status == VerdictStatus::Failed {
            self.status = ContractStatus::Failed;
        }
        self.verdict = Some(verdict);
    }

    pub fn is_all_criteria_passed(&self) -> bool {
        !self.criteria.is_empty() && self.criteria.iter().all(|c| c.status == CriterionStatus::Pass)
    }

    pub fn is_completion_gate_passed(&self) -> bool {
        if !self.is_all_criteria_passed() {
            return false;
        }
        match &self.verdict {
            Some(v) => v.status == VerdictStatus::Passed && v.blocking_issues.is_empty(),
            None => false,
        }
    }

    pub fn blocking_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for c in &self.criteria {
            if c.status == CriterionStatus::Fail || c.status == CriterionStatus::Blocked {
                issues.push(format!("Criterion '{}' is {:?}", c.description, c.status));
            }
        }
        if let Some(v) = &self.verdict {
            issues.extend(v.blocking_issues.clone());
        }
        issues
    }
}
