use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReasoningType {
    None,
    OpenAiEffort,
    AnthropicBudget,
    DeepSeekThinking,
    GeminiThinking,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelFamily {
    OpenAi,
    Anthropic,
    DeepSeek,
    GoogleGemini,
    OpenRouter,
    ZAi,
    OpenCode,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub model_id: String,
    pub family: ModelFamily,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub supports_reasoning: bool,
    pub reasoning_type: ReasoningType,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_response_format: bool,
    pub supports_temperature: bool,
    pub use_max_completion_tokens: bool,
}

impl ModelCapabilities {
    pub fn default_for_model(model_name: &str) -> Self {
        let name_lower = model_name.to_lowercase();
        let short = name_lower.rsplit('/').next().unwrap_or(&name_lower);

        if short.contains("imagine-image")
            || short.contains("imagine-video")
            || short.contains("flux")
            || short.contains("tts")
            || short.contains("whisper")
            || short.contains("embedding")
            || short.contains("dall-e")
            || short.contains("sora")
        {
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::Custom,
                context_window: 8_192,
                max_output_tokens: 4_096,
                supports_reasoning: false,
                reasoning_type: ReasoningType::None,
                supports_tools: false,
                supports_vision: false,
                supports_response_format: false,
                supports_temperature: false,
                use_max_completion_tokens: false,
            };
        }

        if short.starts_with("o1")
            || short.starts_with("o3")
            || short.starts_with("o4")
            || short.contains("gpt-5")
            || short.contains("gpt-6")
        {
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::OpenAi,
                context_window: if short.contains("gpt-6.1-sol") || short.contains("gpt-6-astra") || short.contains("gpt-6-sol") {
                    524_288
                } else if short.contains("gpt-6-luna") || short.contains("gpt-6-chat") {
                    272_000
                } else if short.contains("gpt-6") || short.contains("gpt-5.6") {
                    400_000
                } else {
                    200_000
                },
                max_output_tokens: 100_000,
                supports_reasoning: true,
                reasoning_type: ReasoningType::OpenAiEffort,
                supports_tools: true,
                supports_vision: true,
                supports_response_format: true,
                supports_temperature: false,
                use_max_completion_tokens: true,
            };
        }

        if short.contains("gpt-4o") || short.contains("gpt-4.5") || short.contains("gpt-4.1") {
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::OpenAi,
                context_window: if short.contains("gpt-4.1") { 1_047_576 } else { 128_000 },
                max_output_tokens: 32_768,
                supports_reasoning: false,
                reasoning_type: ReasoningType::None,
                supports_tools: true,
                supports_vision: true,
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("gpt-4") {
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::OpenAi,
                context_window: 128_000,
                max_output_tokens: 16_384,
                supports_reasoning: false,
                reasoning_type: ReasoningType::None,
                supports_tools: true,
                supports_vision: short.contains("vision") || short.contains("turbo"),
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("claude") {
            let is_modern = short.contains("3-7")
                || short.contains("3.7")
                || short.contains("claude-4")
                || short.contains("claude-sonnet-4")
                || short.contains("claude-opus-4")
                || short.contains("claude-haiku-4")
                || short.contains("claude-5")
                || short.contains("claude-opus-5")
                || short.contains("claude-sonnet-5")
                || short.contains("claude-haiku-5")
                || short.contains("fable-5")
                || short.contains("4.5")
                || short.contains("4.8");
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::Anthropic,
                context_window: if short.contains("sonnet-4") || short.contains("sonnet-5") || short.contains("fable-5") {
                    1_000_000
                } else {
                    200_000
                },
                max_output_tokens: if is_modern { 64_000 } else { 8_192 },
                supports_reasoning: is_modern,
                reasoning_type: if is_modern {
                    ReasoningType::AnthropicBudget
                } else {
                    ReasoningType::None
                },
                supports_tools: true,
                supports_vision: true,
                supports_response_format: false,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("deepseek") {
            let is_v4_1 = short.contains("v4.1") || short.contains("v4-1");
            let is_reasoning = short.contains("reasoner")
                || short.contains("r1")
                || short.contains("r2")
                || short.contains("v4")
                || short.contains("v3.2")
                || short.contains("thinking");
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::DeepSeek,
                context_window: if is_v4_1 { 1_000_000 } else { 128_000 },
                max_output_tokens: if is_v4_1 { 384_000 } else if is_reasoning { 64_000 } else { 8_192 },
                supports_reasoning: is_reasoning,
                reasoning_type: if is_reasoning {
                    ReasoningType::DeepSeekThinking
                } else {
                    ReasoningType::None
                },
                supports_tools: true,
                supports_vision: short.contains("vl") || is_v4_1,
                supports_response_format: true,
                supports_temperature: !is_reasoning || is_v4_1,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("gemini") {
            let is_image = short.contains("-image") || short.ends_with("image");
            let is_thinking = !is_image && (short.contains("2.5")
                || short.contains("gemini-3")
                || short.contains("gemini-4")
                || short.contains("thinking")
                || short.contains("argon"));
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::GoogleGemini,
                context_window: 1_048_576,
                max_output_tokens: if is_thinking { 65_536 } else { 8_192 },
                supports_reasoning: is_thinking,
                reasoning_type: if is_thinking {
                    ReasoningType::GeminiThinking
                } else {
                    ReasoningType::None
                },
                supports_tools: true,
                supports_vision: true,
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("glm") || short.contains("zhipu") || short.contains("chatglm") {
            let is_reasoning = short.contains("glm-5")
                || short.contains("glm-4.7")
                || short.contains("glm-4.5")
                || short.contains("thinking");
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::ZAi,
                context_window: if short.contains("glm-5") { 200_000 } else { 128_000 },
                max_output_tokens: 16_384,
                supports_reasoning: is_reasoning,
                reasoning_type: if is_reasoning {
                    ReasoningType::DeepSeekThinking
                } else {
                    ReasoningType::None
                },
                supports_tools: true,
                supports_vision: short.contains("v") && !short.contains("v3"),
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("grok") {
            let is_reasoning = short.contains("grok-4") || short.contains("mini") || short.contains("reason");
            let is_grok_45_plus = short.contains("grok-4.5") || short.contains("grok-4.6") || short.contains("grok-4.7");
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::Custom,
                context_window: if is_grok_45_plus { 500_000 } else { 256_000 },
                max_output_tokens: if short.contains("grok-4.7") { 128_000 } else if is_grok_45_plus { 65_536 } else { 32_768 },
                supports_reasoning: is_reasoning,
                reasoning_type: if is_reasoning {
                    ReasoningType::OpenAiEffort
                } else {
                    ReasoningType::None
                },
                supports_tools: true,
                supports_vision: true,
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("kimi") || short.contains("moonshot") {
            let is_reasoning = short.contains("k2") || short.contains("k3") || short.contains("thinking");
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::Custom,
                context_window: if short.contains("k3") && !short.contains("256k") { 1_000_000 } else { 262_144 },
                max_output_tokens: if short.contains("k2.6") || short.contains("k2.7") || short.contains("k2.8") || short.contains("k3") { 65_536 } else { 32_768 },
                supports_reasoning: is_reasoning,
                reasoning_type: if is_reasoning {
                    ReasoningType::DeepSeekThinking
                } else {
                    ReasoningType::None
                },
                supports_tools: true,
                supports_vision: short.contains("k2.5") || short.contains("k3") || short.contains("vl"),
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("qwen") || short.contains("qwq") {
            let is_reasoning = short.contains("qwq")
                || short.contains("thinking")
                || short.contains("qwen3.6")
                || short.contains("qwen3.8")
                || short.contains("qwen3.5")
                || short.contains("qwen3-max");
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::Custom,
                context_window: if short.contains("3.6") || short.contains("3.8") || short.contains("3.5") {
                    262_144
                } else {
                    131_072
                },
                max_output_tokens: 32_768,
                supports_reasoning: is_reasoning,
                reasoning_type: if is_reasoning {
                    ReasoningType::DeepSeekThinking
                } else {
                    ReasoningType::None
                },
                supports_tools: true,
                supports_vision: short.contains("vl") || short.contains("omni"),
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("minimax") || short.contains("mimo") {
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::Custom,
                context_window: 200_000,
                max_output_tokens: 32_768,
                supports_reasoning: true,
                reasoning_type: ReasoningType::DeepSeekThinking,
                supports_tools: true,
                supports_vision: short.contains("vl") || short.contains("omni"),
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("llama-4") || short.contains("scout") || short.contains("maverick") {
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::Custom,
                context_window: if short.contains("scout") { 10_000_000 } else { 1_048_576 },
                max_output_tokens: 32_768,
                supports_reasoning: short.contains("reason") || short.contains("think"),
                reasoning_type: ReasoningType::DeepSeekThinking,
                supports_tools: true,
                supports_vision: true,
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("sonar") {
            let is_reasoning = short.contains("reason") || short.contains("deep-research");
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::Custom,
                context_window: if short.contains("pro") || short.contains("deep-research") { 200_000 } else { 128_000 },
                max_output_tokens: 16_384,
                supports_reasoning: is_reasoning,
                reasoning_type: if is_reasoning {
                    ReasoningType::OpenAiEffort
                } else {
                    ReasoningType::None
                },
                supports_tools: false,
                supports_vision: true,
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        if short.contains("opencode") {
            return Self {
                model_id: model_name.to_string(),
                family: ModelFamily::OpenCode,
                context_window: 128_000,
                max_output_tokens: 8_192,
                supports_reasoning: false,
                reasoning_type: ReasoningType::None,
                supports_tools: true,
                supports_vision: false,
                supports_response_format: true,
                supports_temperature: true,
                use_max_completion_tokens: false,
            };
        }

        Self {
            model_id: model_name.to_string(),
            family: ModelFamily::Custom,
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_reasoning: short.contains("reason") || short.contains("think") || short.contains("r1"),
            reasoning_type: if short.contains("reason") || short.contains("think") || short.contains("r1") {
                ReasoningType::DeepSeekThinking
            } else {
                ReasoningType::None
            },
            supports_tools: true,
            supports_vision: short.contains("vision") || short.contains("vl") || short.contains("omni"),
            supports_response_format: false,
            supports_temperature: true,
            use_max_completion_tokens: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CapabilityRegistry {
    entries: Arc<RwLock<HashMap<String, ModelCapabilities>>>,
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get(&self, model: &str) -> ModelCapabilities {
        if let Ok(guard) = self.entries.read() {
            if let Some(caps) = guard.get(model) {
                return caps.clone();
            }
        }
        let mut caps = ModelCapabilities::default_for_model(model);
        if let Some(md) = crate::llm::models_dev::lookup(model) {
            if md.context_window > 0 {
                caps.context_window = md.context_window;
            }
            if md.max_output_tokens > 0 {
                caps.max_output_tokens = md.max_output_tokens;
            }
            if let Some(r) = md.reasoning {
                caps.supports_reasoning = r;
            }
            if let Some(v) = md.vision {
                caps.supports_vision = v;
            }
            if let Some(t) = md.tool_call {
                caps.supports_tools = t;
            }
        }
        if let Ok(mut guard) = self.entries.write() {
            guard.insert(model.to_string(), caps.clone());
        }
        caps
    }

    pub fn update(&self, caps: ModelCapabilities) {
        if let Ok(mut guard) = self.entries.write() {
            guard.insert(caps.model_id.clone(), caps);
        }
    }

    pub fn disable_tools(&self, model: &str) {
        if let Ok(mut guard) = self.entries.write() {
            let entry = guard
                .entry(model.to_string())
                .or_insert_with(|| ModelCapabilities::default_for_model(model));
            entry.supports_tools = false;
        }
    }

    pub fn disable_reasoning(&self, model: &str) {
        if let Ok(mut guard) = self.entries.write() {
            let entry = guard
                .entry(model.to_string())
                .or_insert_with(|| ModelCapabilities::default_for_model(model));
            entry.supports_reasoning = false;
            entry.reasoning_type = ReasoningType::None;
        }
    }

    pub fn disable_vision(&self, model: &str) {
        if let Ok(mut guard) = self.entries.write() {
            let entry = guard
                .entry(model.to_string())
                .or_insert_with(|| ModelCapabilities::default_for_model(model));
            entry.supports_vision = false;
        }
    }

    pub fn disable_temperature(&self, model: &str) {
        if let Ok(mut guard) = self.entries.write() {
            let entry = guard
                .entry(model.to_string())
                .or_insert_with(|| ModelCapabilities::default_for_model(model));
            entry.supports_temperature = false;
        }
    }

    pub fn set_use_max_completion_tokens(&self, model: &str, use_it: bool) {
        if let Ok(mut guard) = self.entries.write() {
            let entry = guard
                .entry(model.to_string())
                .or_insert_with(|| ModelCapabilities::default_for_model(model));
            entry.use_max_completion_tokens = use_it;
        }
    }

    pub fn list(&self) -> Vec<ModelCapabilities> {
        if let Ok(guard) = self.entries.read() {
            guard.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    pub fn update_model_caps(
        &self,
        model_id: &str,
        context_window: Option<u32>,
        max_output_tokens: Option<u32>,
        supports_reasoning: Option<bool>,
    ) {
        let mut caps = self.get(model_id);
        if let Some(cw) = context_window {
            if cw > 0 {
                caps.context_window = cw;
            }
        }
        if let Some(mo) = max_output_tokens {
            if mo > 0 {
                caps.max_output_tokens = mo;
            }
        }
        if let Some(sr) = supports_reasoning {
            caps.supports_reasoning = sr;
        }
        if let Ok(mut map) = self.entries.write() {
            map.insert(model_id.to_string(), caps);
        }
    }

    pub fn update_full_model_caps(
        &self,
        model_id: &str,
        context_window: Option<u32>,
        max_output_tokens: Option<u32>,
        supports_reasoning: Option<bool>,
        supports_vision: Option<bool>,
        supports_tools: Option<bool>,
    ) {
        let mut caps = self.get(model_id);
        let default_caps = ModelCapabilities::default_for_model(model_id);
        if let Some(cw) = context_window {
            if cw > 0 {
                if cw == 200_000 && default_caps.context_window > 200_000 {
                    caps.context_window = default_caps.context_window;
                } else {
                    caps.context_window = cw;
                }
            }
        }
        if let Some(mo) = max_output_tokens {
            if mo > 0 {
                caps.max_output_tokens = mo;
            }
        }
        if let Some(sr) = supports_reasoning {
            caps.supports_reasoning = sr;
            if !sr {
                caps.reasoning_type = ReasoningType::None;
            }
        }
        if let Some(sv) = supports_vision {
            caps.supports_vision = sv;
        }
        if let Some(st) = supports_tools {
            caps.supports_tools = st;
        }
        if let Ok(mut map) = self.entries.write() {
            map.insert(model_id.to_string(), caps);
        }
    }
}
