pub mod anthropic;
pub mod capabilities;
pub mod google_gemini;
pub mod image_gen;
pub mod key_pool;
pub mod models_dev;
pub mod openai_chat;
pub mod antigravity_client;
pub mod oauth_accounts;
pub mod openai_responses;
pub mod request_compat;
pub mod token_counter;

pub use anthropic::*;
pub use capabilities::*;
pub use google_gemini::*;
pub use image_gen::*;
pub use key_pool::*;
pub use models_dev::*;
pub use openai_chat::*;
pub use openai_responses::*;
pub use request_compat::*;
pub use token_counter::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub cached_tokens: u32,
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum StreamEvent {
    ContentDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    RetroactiveReasoningMove {
        text: String,
    },
    TurnBufferReset,
    ToolCallDelta {
        index: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        arguments_delta: String,
    },
    ToolOutputDelta {
        id: String,
        chunk: String,
    },
    ToolCallComplete {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    },
    ToolExecEnd {
        id: String,
        success: bool,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        elapsed_ms: u64,
    },
    Finish {
        #[serde(skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
    },
    RetryNotice {
        reason: String,
        attempt: u32,
        model: String,
    },
    ContextTelemetry {
        used_tokens: u32,
        context_window: u32,
        cached_tokens: u32,
        cache_hit_rate: f32,
    },
    ContextCompacted {
        before_tokens: u32,
        after_tokens: u32,
        summary: String,
    },
    TaskBoardUpdated {
        items: serde_json::Value,
    },
    PlanApprovalRequired {
        plan_summary: String,
    },
    GoalStatusUpdated {
        status: String,
        condition: String,
        turn: u32,
        max_turns: u32,
        reason: Option<String>,
        elapsed_secs: u64,
    },
    Error {
        error: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Provider HTTP error {status}: {message}")]
    ProviderError { status: u16, message: String },
    #[error("No available API keys for provider '{0}'")]
    NoAvailableKeys(String),
    #[error("All keys for provider '{provider}' are rate limited, cooldown remaining {wait_secs}s")]
    AllKeysRateLimited { provider: String, wait_secs: u64 },
    #[error("Unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error("Stream error: {0}")]
    StreamError(String),
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

impl From<LlmError> for crate::error::AppError {
    fn from(err: LlmError) -> Self {
        crate::error::AppError::Execution(err.to_string())
    }
}

pub async fn dispatch_stream(
    client: &reqwest::Client,
    provider_name: &str,
    base_url: &str,
    key_pool: &KeyPool,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    dispatch_stream_with_protocol(
        client,
        provider_name,
        base_url,
        None,
        key_pool,
        caps_registry,
        request,
        sender,
    )
    .await
}

pub async fn dispatch_stream_with_protocol(
    client: &reqwest::Client,
    provider_name: &str,
    base_url: &str,
    api_protocol: Option<&str>,
    key_pool: &KeyPool,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let p_lower = provider_name.to_lowercase();
    let proto = api_protocol.unwrap_or("auto").trim().to_lowercase();
    let caps = caps_registry.get(&request.model);

    if proto == "antigravity" {
        return antigravity_client::stream_antigravity(client, caps_registry, request, sender).await;
    }
    if proto == "openai_codex" {
        return openai_responses::stream_openai_codex(client, caps_registry, request, sender).await;
    }
    if proto == "claude_oauth" {
        return anthropic::stream_anthropic_oauth(client, caps_registry, request, sender).await;
    }
    if proto == "kimi_oauth" {
        return anthropic::stream_kimi_oauth(client, caps_registry, request, sender).await;
    }
    if proto == "xai_oauth" {
        return openai_responses::stream_xai_oauth(client, caps_registry, request, sender).await;
    }
    if proto == "zcode_oauth" {
        return anthropic::stream_zcode_oauth(client, caps_registry, request, sender).await;
    }
    if proto == "perplexity_oauth" {
        return openai_chat::stream_perplexity_oauth(client, caps_registry, request, sender).await;
    }

    let use_google = match proto.as_str() {
        "google_gemini" | "gemini" => true,
        "anthropic_messages" | "anthropic" | "chat_completions" | "responses" => false,
        _ => base_url.to_lowercase().contains("generativelanguage.googleapis.com") && !base_url.to_lowercase().contains("/openai"),
    };
    let use_anthropic = match proto.as_str() {
        "anthropic_messages" | "anthropic" => true,
        "google_gemini" | "gemini" | "chat_completions" | "responses" => false,
        _ => p_lower.contains("anthropic") || caps.family == ModelFamily::Anthropic,
    };
    let use_responses = match proto.as_str() {
        "responses" => true,
        "google_gemini" | "gemini" | "anthropic_messages" | "anthropic" | "chat_completions" => false,
        _ => base_url.ends_with("/responses") || request.model.contains("responses"),
    };

    if use_google {
        google_gemini::stream_google_gemini(
            client,
            base_url,
            key_pool,
            provider_name,
            caps_registry,
            request,
            sender,
        )
        .await
    } else if use_anthropic {
        anthropic::stream_anthropic(
            client,
            base_url,
            key_pool,
            provider_name,
            caps_registry,
            request,
            sender,
        )
        .await
    } else if use_responses {
        openai_responses::stream_openai_responses(
            client,
            base_url,
            key_pool,
            provider_name,
            caps_registry,
            request,
            sender,
        )
        .await
    } else {
        openai_chat::stream_openai_chat(
            client,
            base_url,
            key_pool,
            provider_name,
            caps_registry,
            request,
            sender,
        )
        .await
    }
}
