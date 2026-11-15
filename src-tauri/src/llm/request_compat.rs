use crate::llm::capabilities::{CapabilityRegistry, ModelCapabilities, ModelFamily};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    pub fn as_text_lossy(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Parts(parts) => {
                let mut out = String::new();
                for part in parts {
                    if let ContentPart::Text { text } = part {
                        out.push_str(text);
                    }
                }
                out
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlDetail },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrlDetail {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryAction {
    StripTools,
    StripReasoning,
    StripVision,
    StripTemperature,
    SwitchTokenField,
}

pub fn normalize_request(req: &mut ChatRequest, caps: &ModelCapabilities) {
    let name_lower = req.model.to_lowercase();
    let is_o_series = name_lower.starts_with("o1")
        || name_lower.starts_with("o3")
        || name_lower.starts_with("o4")
        || name_lower.contains("gpt-5")
        || name_lower.contains("deepseek-reasoner");

    if !caps.supports_temperature || is_o_series {
        req.temperature = None;
        req.top_p = None;
    } else if caps.family == ModelFamily::Anthropic && req.thinking_budget.is_some() {
        req.temperature = Some(1.0);
    }

    if is_o_series || caps.use_max_completion_tokens {
        if req.max_completion_tokens.is_none() && req.max_tokens.is_some() {
            req.max_completion_tokens = req.max_tokens;
            req.max_tokens = None;
        }
    } else if caps.family == ModelFamily::Anthropic {
        if let Some(budget) = req.thinking_budget {
            let min_needed = budget.saturating_add(4096);
            match req.max_tokens {
                Some(current) if current < min_needed => {
                    req.max_tokens = Some(min_needed);
                }
                None => {
                    req.max_tokens = Some(min_needed);
                }
                _ => {}
            }
        }
    }

    if !caps.supports_tools {
        req.tools = None;
        req.tool_choice = None;
    }

    if !caps.supports_reasoning && caps.family != ModelFamily::Custom {
        req.reasoning_effort = None;
        req.thinking_budget = None;
    }

    if !caps.supports_vision {
        for msg in req.messages.iter_mut() {
            if let MessageContent::Parts(parts) = &mut msg.content {
                let mut sanitized_parts = Vec::new();
                for part in parts.drain(..) {
                    match part {
                        ContentPart::Text { text } => {
                            sanitized_parts.push(ContentPart::Text { text });
                        }
                        ContentPart::ImageUrl { .. } => {
                            sanitized_parts.push(ContentPart::Text {
                                text: "[Image omitted: model does not support vision]".to_string(),
                            });
                        }
                    }
                }
                msg.content = MessageContent::Parts(sanitized_parts);
            }
        }
    }
}

pub fn handle_provider_error(
    error_body: &str,
    status: u16,
    caps_registry: &CapabilityRegistry,
    model: &str,
) -> Option<RetryAction> {
    if status != 400 {
        return None;
    }

    let body_low = error_body.to_lowercase();
    if body_low.contains("tool")
        || body_low.contains("function")
        || body_low.contains("tools is not supported")
        || body_low.contains("unrecognized parameter: 'tools'")
    {
        caps_registry.disable_tools(model);
        return Some(RetryAction::StripTools);
    }

    if body_low.contains("reasoning_effort")
        || body_low.contains("thinking")
        || body_low.contains("budget_tokens")
        || body_low.contains("reasoning is not supported")
    {
        caps_registry.disable_reasoning(model);
        return Some(RetryAction::StripReasoning);
    }

    if body_low.contains("image")
        || body_low.contains("multimodal")
        || body_low.contains("vision")
        || body_low.contains("image_url")
    {
        caps_registry.disable_vision(model);
        return Some(RetryAction::StripVision);
    }

    if body_low.contains("temperature")
        || body_low.contains("top_p")
        || body_low.contains("unsupported parameter: 'temperature'")
    {
        caps_registry.disable_temperature(model);
        return Some(RetryAction::StripTemperature);
    }

    if body_low.contains("max_completion_tokens") || body_low.contains("max_tokens") {
        caps_registry.set_use_max_completion_tokens(model, true);
        return Some(RetryAction::SwitchTokenField);
    }

    None
}

pub fn apply_retry_action(req: &mut ChatRequest, action: &RetryAction) {
    match action {
        RetryAction::StripTools => {
            req.tools = None;
            req.tool_choice = None;
        }
        RetryAction::StripReasoning => {
            req.reasoning_effort = None;
            req.thinking_budget = None;
        }
        RetryAction::StripVision => {
            for msg in req.messages.iter_mut() {
                if let MessageContent::Parts(parts) = &mut msg.content {
                    let mut sanitized = Vec::new();
                    for part in parts.drain(..) {
                        match part {
                            ContentPart::Text { text } => sanitized.push(ContentPart::Text { text }),
                            ContentPart::ImageUrl { .. } => sanitized.push(ContentPart::Text {
                                text: "[Image omitted: model does not support vision]".to_string(),
                            }),
                        }
                    }
                    msg.content = MessageContent::Parts(sanitized);
                }
            }
        }
        RetryAction::StripTemperature => {
            req.temperature = None;
            req.top_p = None;
        }
        RetryAction::SwitchTokenField => {
            if req.max_tokens.is_some() {
                req.max_completion_tokens = req.max_tokens;
                req.max_tokens = None;
            } else if req.max_completion_tokens.is_some() {
                req.max_tokens = req.max_completion_tokens;
                req.max_completion_tokens = None;
            }
        }
    }
}
