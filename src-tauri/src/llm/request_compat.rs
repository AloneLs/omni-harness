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
                    match part {
                        ContentPart::Text { text } => out.push_str(text),
                        ContentPart::InputAudio { .. } => out.push_str("[audio input]"),
                        ContentPart::ImageUrl { image_url } => {
                            if image_url.url.starts_with("data:video/") {
                                out.push_str("[video input]");
                            }
                        }
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
    #[serde(rename = "input_audio")]
    InputAudio { input_audio: InputAudioDetail },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrlDetail {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputAudioDetail {
    pub data: String,
    pub format: String,
}

impl InputAudioDetail {
    pub fn mime_type(&self) -> String {
        match self.format.to_ascii_lowercase().as_str() {
            "wav" => "audio/wav".to_string(),
            "mp3" => "audio/mpeg".to_string(),
            "ogg" | "opus" => "audio/ogg".to_string(),
            "flac" => "audio/flac".to_string(),
            "m4a" | "mp4" | "aac" => "audio/aac".to_string(),
            "webm" => "audio/webm".to_string(),
            other => format!("audio/{}", other),
        }
    }

    pub fn from_file(path: &str, format_hint: &str) -> Option<Self> {
        use base64::Engine;
        let bytes = std::fs::read(path).ok()?;
        if bytes.is_empty() || bytes.len() > 20 * 1024 * 1024 {
            return None;
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or(format_hint)
            .to_ascii_lowercase();
        Some(Self {
            data: b64,
            format: if ext.is_empty() { format_hint.to_string() } else { ext },
        })
    }
}

pub fn resize_png_bytes_if_needed(bytes: &[u8], max_dim: u32) -> Option<Vec<u8>> {
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let w = info.width;
    let h = info.height;
    if w <= max_dim && h <= max_dim {
        return None;
    }
    let color_type = info.color_type;
    let bit_depth = info.bit_depth;
    if bit_depth != png::BitDepth::Eight {
        return None;
    }
    let bpp = match color_type {
        png::ColorType::Rgb => 3usize,
        png::ColorType::Rgba => 4usize,
        png::ColorType::Grayscale => 1usize,
        png::ColorType::GrayscaleAlpha => 2usize,
        _ => return None,
    };
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let out_info = reader.next_frame(&mut buf).ok()?;
    let src = &buf[..out_info.buffer_size()];

    let scale = (w.max(h) as f64) / (max_dim as f64);
    let nw = ((w as f64 / scale).round() as u32).max(1);
    let nh = ((h as f64 / scale).round() as u32).max(1);
    let mut dst = vec![0u8; (nw as usize) * (nh as usize) * bpp];

    for y in 0..nh {
        let sy = (((y as u64) * (h as u64)) / (nh as u64)).min((h - 1) as u64) as usize;
        for x in 0..nw {
            let sx = (((x as u64) * (w as u64)) / (nw as u64)).min((w - 1) as u64) as usize;
            let s_off = (sy * (w as usize) + sx) * bpp;
            let d_off = ((y as usize) * (nw as usize) + (x as usize)) * bpp;
            if s_off + bpp <= src.len() && d_off + bpp <= dst.len() {
                dst[d_off..d_off + bpp].copy_from_slice(&src[s_off..s_off + bpp]);
            }
        }
    }

    let mut out_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out_bytes, nw, nh);
        encoder.set_color(color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&dst).ok()?;
    }
    if out_bytes.len() < bytes.len() {
        Some(out_bytes)
    } else {
        None
    }
}

pub fn resize_image_data_url_if_needed(data_url: &str, max_dim: u32) -> String {
    use base64::Engine;
    let Some(rest) = data_url.strip_prefix("data:image/png;base64,") else {
        return data_url.to_string();
    };
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(rest) else {
        return data_url.to_string();
    };
    if let Some(resized) = resize_png_bytes_if_needed(&raw, max_dim) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&resized);
        format!("data:image/png;base64,{}", b64)
    } else {
        data_url.to_string()
    }
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
        || name_lower.contains("gpt-6")
        || name_lower.contains("codex")
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
                        ContentPart::ImageUrl { image_url } => {
                            let kind = if image_url.url.starts_with("data:video/") {
                                "video"
                            } else {
                                "image"
                            };
                            sanitized_parts.push(ContentPart::Text {
                                text: format!("[{} omitted: model does not support vision]", kind),
                            });
                        }
                        ContentPart::InputAudio { input_audio } => {
                            sanitized_parts.push(ContentPart::InputAudio { input_audio });
                        }
                    }
                }
                msg.content = MessageContent::Parts(sanitized_parts);
            }
        }
    }

    if !crate::llm::capabilities::model_supports_native_audio(&req.model) {
        for msg in req.messages.iter_mut() {
            if let MessageContent::Parts(parts) = &mut msg.content {
                let mut sanitized_parts = Vec::new();
                let mut audio_count = 0usize;
                for part in parts.drain(..) {
                    match part {
                        ContentPart::InputAudio { .. } => {
                            audio_count += 1;
                        }
                        other => sanitized_parts.push(other),
                    }
                }
                if audio_count > 0 {
                    sanitized_parts.push(ContentPart::Text {
                        text: format!(
                            "[{} audio attachment(s) omitted: model does not support native audio input]",
                            audio_count
                        ),
                    });
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
    if body_low.contains("tools is not supported")
        || body_low.contains("does not support tools")
        || body_low.contains("does not support function")
        || body_low.contains("tool calling is not supported")
        || body_low.contains("function calling is not supported")
        || body_low.contains("unrecognized parameter: 'tools'")
        || body_low.contains("unknown parameter: 'tools'")
        || body_low.contains("unsupported parameter: 'tools'")
    {
        caps_registry.disable_tools(model);
        return Some(RetryAction::StripTools);
    }

    if body_low.contains("reasoning_effort")
        || body_low.contains("reasoning is not supported")
        || body_low.contains("does not support reasoning")
        || body_low.contains("does not support thinking")
        || body_low.contains("thinking is not supported")
        || body_low.contains("unsupported parameter: 'reasoning'")
        || body_low.contains("unknown parameter: 'thinking'")
    {
        caps_registry.disable_reasoning(model);
        return Some(RetryAction::StripReasoning);
    }

    if body_low.contains("does not support image")
        || body_low.contains("vision is not supported")
        || body_low.contains("does not support vision")
        || body_low.contains("image input is not supported")
        || body_low.contains("multimodal is not supported")
    {
        caps_registry.disable_vision(model);
        return Some(RetryAction::StripVision);
    }

    if body_low.contains("unsupported parameter: 'temperature'")
        || body_low.contains("unsupported value: 'temperature'")
        || body_low.contains("temperature is not supported")
        || body_low.contains("does not support temperature")
        || body_low.contains("unsupported parameter: 'top_p'")
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
                            ContentPart::InputAudio { input_audio } => {
                                sanitized.push(ContentPart::InputAudio { input_audio })
                            }
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
