use crate::llm::request_compat::{ChatMessage, ChatRequest};
use crate::llm::{dispatch_stream_with_protocol, CapabilityRegistry, KeyPool, StreamEvent};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub const MAX_GOAL_CONDITION_CHARS: usize = 4000;
pub const DEFAULT_GOAL_MAX_TURNS: u32 = 100;
pub const CEILING_GOAL_MAX_TURNS: u32 = 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Achieved,
    Paused,
    Failed,
    Idle,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Active => "active",
            GoalStatus::Achieved => "achieved",
            GoalStatus::Paused => "paused",
            GoalStatus::Failed => "failed",
            GoalStatus::Idle => "idle",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalState {
    pub chat_id: String,
    pub condition: String,
    pub status: GoalStatus,
    pub turn: u32,
    pub max_turns: u32,
    pub zero_tool_streak: u32,
    pub started_at: i64,
    pub last_reason: Option<String>,
}

impl GoalState {
    pub fn new(chat_id: &str, raw_condition: &str) -> Self {
        let (condition, max_turns) = parse_condition_and_budget(raw_condition);
        Self {
            chat_id: chat_id.to_string(),
            condition,
            status: GoalStatus::Active,
            turn: 1,
            max_turns,
            zero_tool_streak: 0,
            started_at: chrono::Utc::now().timestamp(),
            last_reason: None,
        }
    }

    pub fn elapsed_secs(&self) -> u64 {
        let now = chrono::Utc::now().timestamp();
        (now - self.started_at).max(0) as u64
    }

    pub fn to_stream_event(&self) -> StreamEvent {
        StreamEvent::GoalStatusUpdated {
            status: self.status.as_str().to_string(),
            condition: self.condition.clone(),
            turn: self.turn,
            max_turns: self.max_turns,
            reason: self.last_reason.clone(),
            elapsed_secs: self.elapsed_secs(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalCommandAction {
    Start { condition: String },
    Status,
    Clear,
}

pub fn parse_goal_command(prompt: &str) -> Option<GoalCommandAction> {
    let trimmed = prompt.trim();
    if !trimmed.starts_with("/goal") {
        return None;
    }
    let after = trimmed.strip_prefix("/goal").unwrap_or("").trim();
    if !after.is_empty() && !trimmed[5..].starts_with(char::is_whitespace) {
        return None;
    }
    if after.is_empty() || after.eq_ignore_ascii_case("status") {
        return Some(GoalCommandAction::Status);
    }
    if after.eq_ignore_ascii_case("clear") || after.eq_ignore_ascii_case("stop") {
        return Some(GoalCommandAction::Clear);
    }
    Some(GoalCommandAction::Start {
        condition: after.to_string(),
    })
}

pub fn parse_condition_and_budget(raw: &str) -> (String, u32) {
    let mut max_turns = DEFAULT_GOAL_MAX_TURNS;
    let mut cleaned = raw.trim().to_string();

    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if let Some(pos) = tokens.iter().position(|t| *t == "--max-turns") {
        if let Some(val_str) = tokens.get(pos + 1) {
            if let Ok(n) = val_str.parse::<u32>() {
                max_turns = n.clamp(1, CEILING_GOAL_MAX_TURNS);
                let mut rebuilt = Vec::new();
                for (i, t) in tokens.iter().enumerate() {
                    if i == pos || i == pos + 1 {
                        continue;
                    }
                    rebuilt.push(*t);
                }
                cleaned = rebuilt.join(" ");
            }
        }
    }

    let lower = cleaned.to_lowercase();
    if let Some(idx) = lower.find("stop after ") {
        let rest = &lower[idx + 11..];
        if let Some(num_str) = rest.split_whitespace().next() {
            if let Ok(n) = num_str.parse::<u32>() {
                max_turns = n.clamp(1, CEILING_GOAL_MAX_TURNS);
            }
        }
    }

    if cleaned.chars().count() > MAX_GOAL_CONDITION_CHARS {
        cleaned = cleaned.chars().take(MAX_GOAL_CONDITION_CHARS).collect();
    }
    (cleaned, max_turns)
}

#[derive(Debug, Clone)]
pub enum EvaluatorVerdict {
    Met { reason: String },
    NotYetMet { reason: String },
    Impossible { reason: String },
}

pub fn build_transcript_excerpt(history: &[ChatMessage], max_chars: usize) -> String {
    let mut parts = Vec::new();
    for m in history {
        if m.role == "system" {
            continue;
        }
        let text = m.content.as_text_lossy();
        let trimmed = text.trim();
        if m.role == "tool" {
            let name = m.name.as_deref().unwrap_or("tool");
            let short: String = trimmed.chars().take(2000).collect();
            parts.push(format!("[TOOL: {}]\n{}", name, short));
        } else if m.role == "assistant" {
            let mut line = String::new();
            if let Some(ref tcs) = m.tool_calls {
                if !tcs.is_empty() {
                    let names: Vec<&str> = tcs.iter().map(|t| t.function.name.as_str()).collect();
                    line.push_str(&format!("[ASSISTANT TOOL CALLS: {}]\n", names.join(", ")));
                }
            }
            if !trimmed.is_empty() {
                let short: String = trimmed.chars().take(2500).collect();
                line.push_str(&format!("[ASSISTANT]\n{}", short));
            }
            if !line.is_empty() {
                parts.push(line);
            }
        } else if m.role == "user" && !trimmed.is_empty() {
            let short: String = trimmed.chars().take(1500).collect();
            parts.push(format!("[USER]\n{}", short));
        }
    }
    let joined = parts.join("\n\n");
    let total_chars = joined.chars().count();
    if total_chars <= max_chars {
        joined
    } else {
        let skip = total_chars - max_chars;
        format!("...[earlier transcript truncated]...\n{}", joined.chars().skip(skip).collect::<String>())
    }
}

pub fn parse_evaluator_verdict(raw: &str) -> EvaluatorVerdict {
    let trimmed = raw.trim();
    let json_candidate = if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if start <= end { &trimmed[start..=end] } else { trimmed }
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_candidate) {
        let v = val
            .get("verdict")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_uppercase();
        let reason = val
            .get("reason")
            .and_then(|x| x.as_str())
            .unwrap_or("No reason provided.")
            .trim()
            .to_string();

        if v == "MET" || v == "PASS" || v == "PASSED" {
            return EvaluatorVerdict::Met { reason };
        }
        if v == "IMPOSSIBLE" || v == "ABORT" || v == "UNACHIEVABLE" {
            return EvaluatorVerdict::Impossible { reason };
        }
        return EvaluatorVerdict::NotYetMet { reason };
    }

    let upper = trimmed.to_uppercase();
    if upper.contains("VERDICT: MET") {
        EvaluatorVerdict::Met {
            reason: trimmed.chars().take(400).collect(),
        }
    } else if upper.contains("VERDICT: IMPOSSIBLE") || upper.contains("\"IMPOSSIBLE\"") {
        EvaluatorVerdict::Impossible {
            reason: trimmed.chars().take(400).collect(),
        }
    } else {
        EvaluatorVerdict::NotYetMet {
            reason: if trimmed.is_empty() {
                "Evaluator returned empty response; continue verification.".to_string()
            } else {
                trimmed.chars().take(500).collect()
            },
        }
    }
}

pub async fn evaluate_transcript(
    http_client: &reqwest::Client,
    provider_name: &str,
    base_url: &str,
    api_protocol: Option<&str>,
    key_pool: &KeyPool,
    caps_registry: &CapabilityRegistry,
    model_name: &str,
    condition: &str,
    history: &[ChatMessage],
) -> Result<EvaluatorVerdict, String> {
    let transcript = build_transcript_excerpt(history, 36_000);
    let system_prompt = "You are a Blind Goal Evaluator (Stop-Hook Judge). You have NO tools and NO filesystem access.\nEvaluate ONLY based on concrete evidence in the session transcript below (actual tool outputs, exit codes, test logs, file write confirmations).\nDo NOT trust unverified claims by the assistant like 'I fixed everything' or 'tests pass' unless the transcript shows the actual command execution and passing output.\nReturn STRICTLY valid JSON and nothing else:\n{\"verdict\":\"MET\"|\"NOT_YET_MET\"|\"IMPOSSIBLE\",\"reason\":\"concise technical explanation\"}";

    let user_msg = format!(
        "GOAL CONDITION:\n{}\n\nSESSION TRANSCRIPT:\n{}",
        condition, transcript
    );

    let req = ChatRequest {
        model: model_name.to_string(),
        messages: vec![
            ChatMessage::system(system_prompt.to_string()),
            ChatMessage::user(user_msg),
        ],
        temperature: Some(0.1),
        top_p: None,
        max_tokens: None,
        max_completion_tokens: Some(1024),
        tools: None,
        tool_choice: None,
        reasoning_effort: None,
        thinking_budget: None,
        response_format: None,
        prompt_cache_key: None,
        stream: true,
    };

    let (tx, mut rx) = mpsc::channel(64);
    let client = http_client.clone();
    let prov = provider_name.to_string();
    let base = base_url.to_string();
    let proto = api_protocol.map(|s| s.to_string());
    let kp = key_pool.clone();
    let cr = caps_registry.clone();

    let join = tokio::spawn(async move {
        dispatch_stream_with_protocol(&client, &prov, &base, proto.as_deref(), &kp, &cr, req, tx).await
    });

    let mut collected = String::new();
    let mut stream_err: Option<String> = None;
    while let Some(evt) = rx.recv().await {
        match evt {
            StreamEvent::ContentDelta { delta } => collected.push_str(&delta),
            StreamEvent::Error { error } => stream_err = Some(error),
            _ => {}
        }
    }

    match join.await {
        Ok(Ok(())) => {
            if let Some(e) = stream_err {
                if collected.trim().is_empty() {
                    return Err(e);
                }
            }
            Ok(parse_evaluator_verdict(&collected))
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn is_unrecoverable_goal_error(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("401")
        || lower.contains("403")
        || lower.contains("no available api keys")
        || lower.contains("credit balance")
        || lower.contains("insufficient_quota")
        || lower.contains("disk full")
        || lower.contains("no space left")
        || lower.contains("context_length_exceeded")
        || lower.contains("context overflow")
}
