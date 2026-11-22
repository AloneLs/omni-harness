use crate::db::Database;
use crate::llm::capabilities::ModelFamily;
use crate::llm::request_compat::{ChatMessage, ContentPart, MessageContent, ToolDefinition};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tiktoken_rs::o200k_base_singleton;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCount {
    pub text: u32,
    pub tools: u32,
    pub images: u32,
    pub total: u32,
}

pub trait TokenCounter: Send + Sync {
    fn count(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        db: Option<&Database>,
    ) -> TokenCount;
}

pub struct LocalTiktokenCounter;

impl LocalTiktokenCounter {
    fn count_str_cached(model: &str, kind: &str, text: &str, db: Option<&Database>) -> u32 {
        if text.is_empty() {
            return 0;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(model.as_bytes());
        hasher.update(b"\0");
        hasher.update(kind.as_bytes());
        hasher.update(b"\0");
        hasher.update(text.as_bytes());
        let hash_hex = hasher.finalize().to_hex().to_string();

        if let Some(database) = db {
            if let Ok(Some(cached)) = database.get_cached_token_count(&hash_hex) {
                return cached;
            }
        }

        let bpe = o200k_base_singleton();
        let tokens = bpe.encode_with_special_tokens(text).len() as u32;

        if let Some(database) = db {
            let _ = database.set_cached_token_count(&hash_hex, tokens);
        }
        tokens
    }

    pub fn count_parts(
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        db: Option<&Database>,
    ) -> TokenCount {
        let mut text_tokens: u32 = 3;
        let mut image_tokens: u32 = 0;

        for msg in messages {
            text_tokens += 4;
            let serialized = match &msg.content {
                MessageContent::Text(s) => s.clone(),
                MessageContent::Parts(parts) => {
                    let mut buf = String::new();
                    for p in parts {
                        match p {
                            ContentPart::Text { text } => {
                                buf.push_str(text);
                                buf.push('\n');
                            }
                            ContentPart::ImageUrl { .. } => {
                                image_tokens += 1024;
                            }
                            ContentPart::InputAudio { input_audio } => {
                                image_tokens += (input_audio.data.len() as u32 / 4).min(24_000);
                            }
                        }
                    }
                    buf
                }
            };

            let mut msg_key = String::with_capacity(serialized.len() + 64);
            msg_key.push_str(&msg.role);
            if let Some(ref name) = msg.name {
                msg_key.push(':');
                msg_key.push_str(name);
            }
            msg_key.push('\n');
            msg_key.push_str(&serialized);

            if let Some(ref tcs) = msg.tool_calls {
                for tc in tcs {
                    msg_key.push_str("\ncall:");
                    msg_key.push_str(&tc.function.name);
                    msg_key.push(':');
                    msg_key.push_str(&tc.function.arguments);
                    text_tokens += 8;
                }
            }

            text_tokens += Self::count_str_cached(model, "msg", &msg_key, db);
        }

        let mut tool_tokens: u32 = 0;
        for tool in tools {
            if let Ok(schema_str) = serde_json::to_string(tool) {
                tool_tokens += 8 + Self::count_str_cached(model, "tool", &schema_str, db);
            }
        }

        let total = text_tokens.saturating_add(tool_tokens).saturating_add(image_tokens);
        TokenCount {
            text: text_tokens,
            tools: tool_tokens,
            images: image_tokens,
            total,
        }
    }
}

impl TokenCounter for LocalTiktokenCounter {
    fn count(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        db: Option<&Database>,
    ) -> TokenCount {
        Self::count_parts(model, messages, tools, db)
    }
}

pub struct EstimatorCounter {
    pub factor: f64,
}

impl TokenCounter for EstimatorCounter {
    fn count(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        db: Option<&Database>,
    ) -> TokenCount {
        let base = LocalTiktokenCounter::count_parts(model, messages, tools, db);
        let text = ((base.text as f64) * self.factor).ceil() as u32;
        let tools_c = ((base.tools as f64) * self.factor).ceil() as u32;
        let total = text.saturating_add(tools_c).saturating_add(base.images);
        TokenCount {
            text,
            tools: tools_c,
            images: base.images,
            total,
        }
    }
}

pub struct TokenCountRouter;

impl TokenCountRouter {
    pub fn calibration_factor(family: ModelFamily) -> f64 {
        match family {
            ModelFamily::OpenAi => 1.0,
            ModelFamily::Anthropic => 1.08,
            ModelFamily::GoogleGemini => 1.04,
            ModelFamily::DeepSeek | ModelFamily::ZAi => 1.06,
            ModelFamily::OpenRouter | ModelFamily::OpenCode | ModelFamily::Custom => 1.05,
        }
    }

    pub fn count_sync(
        model: &str,
        family: ModelFamily,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        db: Option<&Database>,
    ) -> TokenCount {
        if family == ModelFamily::OpenAi {
            LocalTiktokenCounter.count(model, messages, tools, db)
        } else {
            EstimatorCounter {
                factor: Self::calibration_factor(family),
            }
            .count(model, messages, tools, db)
        }
    }

    pub async fn count_with_gray_zone(
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
        model: &str,
        family: ModelFamily,
        context_window: u32,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        db: Option<&Database>,
    ) -> TokenCount {
        let local = Self::count_sync(model, family, messages, tools, db);
        if context_window == 0 || api_key.trim().is_empty() || base_url.trim().is_empty() {
            return local;
        }

        let pct = (local.total as u64 * 100) / context_window as u64;
        if !(80..=110).contains(&pct) {
            return local;
        }

        if let Some(remote_total) = Self::try_remote_count(
            client,
            base_url,
            api_key,
            model,
            family,
            messages,
            tools,
            db,
        )
        .await
        {
            return TokenCount {
                text: remote_total.saturating_sub(local.tools).saturating_sub(local.images),
                tools: local.tools,
                images: local.images,
                total: remote_total,
            };
        }

        local
    }

    async fn try_remote_count(
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
        model: &str,
        family: ModelFamily,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        db: Option<&Database>,
    ) -> Option<u32> {
        let req_hash = {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"remote\0");
            hasher.update(model.as_bytes());
            if let Ok(m_json) = serde_json::to_string(messages) {
                hasher.update(m_json.as_bytes());
            }
            if let Ok(t_json) = serde_json::to_string(tools) {
                hasher.update(t_json.as_bytes());
            }
            hasher.finalize().to_hex().to_string()
        };

        if let Some(database) = db {
            if let Ok(Some(cached)) = database.get_cached_token_count(&req_hash) {
                return Some(cached);
            }
        }

        let clean_base = base_url.trim_end_matches('/');
        let count_opt = match family {
            ModelFamily::Anthropic => {
                let url = if clean_base.ends_with("/v1") {
                    format!("{}/messages/count_tokens", clean_base)
                } else {
                    format!("{}/v1/messages/count_tokens", clean_base)
                };
                let mut system_text = String::new();
                let mut anth_msgs = Vec::new();
                for m in messages {
                    if m.role == "system" {
                        if !system_text.is_empty() {
                            system_text.push_str("\n\n");
                        }
                        system_text.push_str(&m.content.as_text_lossy());
                    } else {
                        let role = if m.role == "assistant" { "assistant" } else { "user" };
                        anth_msgs.push(serde_json::json!({
                            "role": role,
                            "content": m.content.as_text_lossy()
                        }));
                    }
                }
                let mut body = serde_json::json!({
                    "model": model,
                    "messages": anth_msgs
                });
                if !system_text.is_empty() {
                    body["system"] = serde_json::json!(system_text);
                }
                if !tools.is_empty() {
                    let anth_tools: Vec<serde_json::Value> = tools
                        .iter()
                        .map(|t| {
                            serde_json::json!({
                                "name": t.function.name,
                                "description": t.function.description,
                                "input_schema": t.function.parameters
                            })
                        })
                        .collect();
                    body["tools"] = serde_json::json!(anth_tools);
                }

                let resp = client
                    .post(&url)
                    .timeout(Duration::from_secs(3))
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&body)
                    .send()
                    .await
                    .ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                let val: serde_json::Value = resp.json().await.ok()?;
                val.get("input_tokens").and_then(|v| v.as_u64()).map(|n| n as u32)
            }
            ModelFamily::GoogleGemini => {
                let url = format!("{}/models/{}:countTokens?key={}", clean_base, model, api_key);
                let contents: Vec<serde_json::Value> = messages
                    .iter()
                    .map(|m| {
                        let role = if m.role == "assistant" { "model" } else { "user" };
                        serde_json::json!({
                            "role": role,
                            "parts": [{ "text": m.content.as_text_lossy() }]
                        })
                    })
                    .collect();
                let body = serde_json::json!({ "contents": contents });
                let resp = client
                    .post(&url)
                    .timeout(Duration::from_secs(3))
                    .json(&body)
                    .send()
                    .await
                    .ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                let val: serde_json::Value = resp.json().await.ok()?;
                val.get("totalTokens").and_then(|v| v.as_u64()).map(|n| n as u32)
            }
            _ => None,
        };

        if let Some(tokens) = count_opt {
            if let Some(database) = db {
                let _ = database.set_cached_token_count(&req_hash, tokens);
            }
        }
        count_opt
    }
}
