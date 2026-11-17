use crate::llm::capabilities::CapabilityRegistry;
use crate::llm::key_pool::KeyPool;
use crate::llm::request_compat::{apply_retry_action, handle_provider_error, normalize_request, ChatRequest};
use crate::llm::{LlmError, StreamEvent, TokenUsage};
use futures_util::StreamExt;
use std::collections::HashMap;

struct ThinkExtractor {
    in_think: bool,
    emitted_initial_content: String,
    pending_prefix: String,
}

impl ThinkExtractor {
    fn new() -> Self {
        Self {
            in_think: false,
            emitted_initial_content: String::new(),
            pending_prefix: String::new(),
        }
    }

    async fn feed(
        &mut self,
        chunk: &str,
        sender: &tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), LlmError> {
        let mut text = format!("{}{}", self.pending_prefix, chunk);
        self.pending_prefix.clear();

        while !text.is_empty() {
            if !self.in_think {
                if let Some(pos) = text.find("<think>") {
                    let before = &text[..pos];
                    if !before.is_empty() {
                        self.emitted_initial_content.push_str(before);
                        sender
                            .send(StreamEvent::ContentDelta {
                                delta: before.to_string(),
                            })
                            .await
                            .map_err(|e| LlmError::StreamError(e.to_string()))?;
                    }

                    if !self.emitted_initial_content.is_empty() {
                        sender
                            .send(StreamEvent::RetroactiveReasoningMove {
                                text: self.emitted_initial_content.clone(),
                            })
                            .await
                            .map_err(|e| LlmError::StreamError(e.to_string()))?;
                        self.emitted_initial_content.clear();
                    }

                    self.in_think = true;
                    text = text[pos + 7..].to_string();
                } else {
                    let mut ends_with_prefix = false;
                    for len in (1..7).rev() {
                        if text.len() >= len {
                            let idx = text.len() - len;
                            if text.is_char_boundary(idx) && "<think>".starts_with(&text[idx..]) {
                                self.pending_prefix = text[idx..].to_string();
                                text.truncate(idx);
                                ends_with_prefix = true;
                                break;
                            }
                        }
                    }

                    if !text.is_empty() {
                        self.emitted_initial_content.push_str(&text);
                        sender
                            .send(StreamEvent::ContentDelta {
                                delta: text.clone(),
                            })
                            .await
                            .map_err(|e| LlmError::StreamError(e.to_string()))?;
                        text.clear();
                    }

                    if ends_with_prefix {
                        break;
                    }
                }
            } else if let Some(pos) = text.find("</think>") {
                let think_content = &text[..pos];
                if !think_content.is_empty() {
                    sender
                        .send(StreamEvent::ReasoningDelta {
                            delta: think_content.to_string(),
                        })
                        .await
                        .map_err(|e| LlmError::StreamError(e.to_string()))?;
                }

                self.in_think = false;
                text = text[pos + 8..].to_string();
            } else {
                let mut ends_with_prefix = false;
                for len in (1..8).rev() {
                    if text.len() >= len {
                        let idx = text.len() - len;
                        if text.is_char_boundary(idx) && "</think>".starts_with(&text[idx..]) {
                            self.pending_prefix = text[idx..].to_string();
                            text.truncate(idx);
                            ends_with_prefix = true;
                            break;
                        }
                    }
                }

                if !text.is_empty() {
                    sender
                        .send(StreamEvent::ReasoningDelta {
                            delta: text.clone(),
                        })
                        .await
                        .map_err(|e| LlmError::StreamError(e.to_string()))?;
                    text.clear();
                }

                if ends_with_prefix {
                    break;
                }
            }
        }

        Ok(())
    }

    async fn finish(
        &mut self,
        sender: &tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<(), LlmError> {
        if !self.pending_prefix.is_empty() {
            let left = std::mem::take(&mut self.pending_prefix);
            if self.in_think {
                sender
                    .send(StreamEvent::ReasoningDelta { delta: left })
                    .await
                    .map_err(|e| LlmError::StreamError(e.to_string()))?;
            } else {
                sender
                    .send(StreamEvent::ContentDelta { delta: left })
                    .await
                    .map_err(|e| LlmError::StreamError(e.to_string()))?;
            }
        }
        Ok(())
    }
}

pub async fn stream_openai_chat(
    client: &reqwest::Client,
    base_url: &str,
    key_pool: &KeyPool,
    provider_name: &str,
    caps_registry: &CapabilityRegistry,
    mut request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let clean_base = base_url.trim_end_matches('/');
    let target_url = if clean_base.ends_with("/chat/completions") {
        clean_base.to_string()
    } else {
        format!("{}/chat/completions", clean_base)
    };

    let is_openrouter = provider_name.to_lowercase().contains("openrouter")
        || target_url.contains("openrouter.ai");

    let max_attempts = (key_pool.pool_size(provider_name) * 2).max(5) as u32;
    let mut unavailable_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for attempt in 1..=max_attempts {
        let api_key = key_pool.next_key_async(provider_name, base_url).await?;
        let caps = caps_registry.get(&request.model);
        normalize_request(&mut request, &caps);

        let mut payload = serde_json::json!({
            "model": request.model,
            "messages": request.messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        if let Some(t) = request.temperature {
            payload["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = request.top_p {
            payload["top_p"] = serde_json::json!(p);
        }
        if let Some(mt) = request.max_completion_tokens {
            payload["max_completion_tokens"] = serde_json::json!(mt);
        } else if let Some(mt) = request.max_tokens {
            payload["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(ref re) = request.reasoning_effort {
            payload["reasoning_effort"] = serde_json::json!(re);
        }
        if let Some(ref tools) = request.tools {
            payload["tools"] = serde_json::json!(tools);
        }
        if let Some(ref tc) = request.tool_choice {
            payload["tool_choice"] = serde_json::json!(tc);
        }
        if let Some(ref rf) = request.response_format {
            payload["response_format"] = serde_json::json!(rf);
        }
        if let Some(ref pck) = request.prompt_cache_key {
            payload["prompt_cache_key"] = serde_json::json!(pck);
        }

        let mut req_builder = client
            .post(&target_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload);

        if is_openrouter {
            req_builder = req_builder
                .header("HTTP-Referer", "https:\u{2F}\u{2F}omni-harness.local")
                .header("X-Title", "OmniHarness");
        }

        let response = match req_builder.send().await {
            Ok(resp) => resp,
            Err(err) => {
                let delay = KeyPool::response_retry_delay(None, attempt.saturating_sub(1), false);
                key_pool.mark_transient(provider_name, base_url, &api_key, Some(delay)).await;
                if attempt == max_attempts {
                    return Err(LlmError::Network(err));
                }
                let _ = sender
                    .send(StreamEvent::RetryNotice {
                        reason: format!("Network request failed: {}", err),
                        attempt,
                        model: request.model.clone(),
                    })
                    .await;
                continue;
            }
        };

        let status = response.status();
        if status.is_success() {
            key_pool.mark_success(provider_name, base_url, &api_key).await;

            let mut byte_stream = response.bytes_stream();
            let mut line_buffer = String::new();
            let mut think_extractor = ThinkExtractor::new();
            let mut tool_calls_map: HashMap<usize, (String, String, String)> = HashMap::new();
            let mut finish_reason: Option<String> = None;
            let mut token_usage: Option<TokenUsage> = None;

            while let Some(chunk_res) = byte_stream.next().await {
                let chunk = match chunk_res {
                    Ok(c) => c,
                    Err(e) => return Err(LlmError::Network(e)),
                };

                let text = String::from_utf8_lossy(&chunk);
                line_buffer.push_str(&text);

                while let Some(pos) = line_buffer.find('\n') {
                    let line = line_buffer[..pos].trim_end_matches('\r').to_string();
                    line_buffer = line_buffer[pos + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    if let Some(stripped) = line.strip_prefix("data: ") {
                        let data_str = stripped.trim();
                        if data_str == "[DONE]" {
                            break;
                        }

                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data_str) {
                            if let Some(choices) = val.get("choices").and_then(|v| v.as_array()) {
                                if let Some(first) = choices.first() {
                                    if let Some(delta) = first.get("delta") {
                                        let reasoning_text = delta
                                            .get("reasoning_content")
                                            .and_then(|v| v.as_str())
                                            .or_else(|| {
                                                delta.get("reasoning").and_then(|v| v.as_str())
                                            });

                                        if let Some(r) = reasoning_text {
                                            if !r.is_empty() {
                                                let _ = sender
                                                    .send(StreamEvent::ReasoningDelta {
                                                        delta: r.to_string(),
                                                    })
                                                    .await;
                                            }
                                        }

                                        if let Some(c) =
                                            delta.get("content").and_then(|v| v.as_str())
                                        {
                                            if !c.is_empty() {
                                                think_extractor.feed(c, &sender).await?;
                                            }
                                        }

                                        if let Some(tcs) =
                                            delta.get("tool_calls").and_then(|v| v.as_array())
                                        {
                                            for tc in tcs {
                                                let idx = tc
                                                    .get("index")
                                                    .and_then(|v| v.as_u64())
                                                    .unwrap_or(0)
                                                    as usize;
                                                let call_id = tc
                                                    .get("id")
                                                    .and_then(|v| v.as_str())
                                                    .map(|s| s.to_string());
                                                let fn_obj = tc.get("function");
                                                let fn_name = fn_obj
                                                    .and_then(|f| f.get("name"))
                                                    .and_then(|v| v.as_str())
                                                    .map(|s| s.to_string());
                                                let args_delta = fn_obj
                                                    .and_then(|f| f.get("arguments"))
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();

                                                let entry = tool_calls_map
                                                    .entry(idx)
                                                    .or_insert_with(|| {
                                                        (
                                                            String::new(),
                                                            String::new(),
                                                            String::new(),
                                                        )
                                                    });
                                                if let Some(ref cid) = call_id {
                                                    entry.0 = cid.clone();
                                                }
                                                if let Some(ref fname) = fn_name {
                                                    entry.1 = fname.clone();
                                                }
                                                entry.2.push_str(&args_delta);

                                                let _ = sender
                                                    .send(StreamEvent::ToolCallDelta {
                                                        index: idx,
                                                        id: call_id,
                                                        name: fn_name,
                                                        arguments_delta: args_delta,
                                                    })
                                                    .await;
                                            }
                                        }
                                    }

                                    if let Some(fr) =
                                        first.get("finish_reason").and_then(|v| v.as_str())
                                    {
                                        finish_reason = Some(fr.to_string());
                                    }
                                }
                            }

                            if let Some(u) = val.get("usage") {
                                let prompt_tokens = u
                                    .get("prompt_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                let completion_tokens = u
                                    .get("completion_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                let total_tokens = u
                                    .get("total_tokens")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                let cached_tokens = u
                                    .get("prompt_tokens_details")
                                    .and_then(|d| d.get("cached_tokens"))
                                    .or_else(|| u.get("prompt_cache_hit_tokens"))
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                token_usage = Some(TokenUsage {
                                    prompt_tokens,
                                    completion_tokens,
                                    total_tokens,
                                    cached_tokens,
                                    cache_read_input_tokens: cached_tokens,
                                    cache_creation_input_tokens: 0,
                                });
                            }
                        }
                    }
                }
            }

            think_extractor.finish(&sender).await?;

            let mut sorted_indices: Vec<usize> = tool_calls_map.keys().cloned().collect();
            sorted_indices.sort_unstable();
            for idx in sorted_indices {
                if let Some((mut cid, name, args)) = tool_calls_map.remove(&idx) {
                    if cid.trim().is_empty() {
                        cid = format!("call-{}-{}", idx, uuid::Uuid::new_v4().simple());
                    }
                    let _ = sender
                        .send(StreamEvent::ToolCallComplete {
                            index: idx,
                            id: cid,
                            name,
                            arguments: args,
                        })
                        .await;
                }
            }

            let _ = sender
                .send(StreamEvent::Finish {
                    finish_reason,
                    usage: token_usage,
                })
                .await;

            return Ok(());
        }

        let retry_after_str = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let error_body = response.text().await.unwrap_or_default();

        if status.as_u16() == 400 {
            if let Some(action) =
                handle_provider_error(&error_body, 400, caps_registry, &request.model)
            {
                apply_retry_action(&mut request, &action);
                let _ = sender
                    .send(StreamEvent::RetryNotice {
                        reason: format!("Corrected 400 error via {:?}", action),
                        attempt,
                        model: request.model.clone(),
                    })
                    .await;
                continue;
            }
            key_pool.record_failure(provider_name, &api_key, 400, None);
            return Err(LlmError::ProviderError {
                status: 400,
                message: error_body,
            });
        }

        let code = status.as_u16();
        if matches!(code, 401 | 402 | 403) {
            key_pool.mark_unavailable(provider_name, base_url, &api_key, None).await;
            unavailable_keys.insert(api_key.clone());
            let pool_sz = key_pool.pool_size(provider_name);
            if pool_sz > 1 && unavailable_keys.len() < pool_sz && attempt < max_attempts {
                let _ = sender
                    .send(StreamEvent::RetryNotice {
                        reason: format!("HTTP {} - rotating key", status),
                        attempt,
                        model: request.model.clone(),
                    })
                    .await;
                continue;
            }
            return Err(LlmError::ProviderError {
                status: code,
                message: error_body,
            });
        }

        if code == 429 || matches!(code, 408 | 425 | 500..=504 | 520..=526) {
            let is_rl = code == 429;
            let delay = KeyPool::response_retry_delay(retry_after_str.as_deref(), attempt.saturating_sub(1), is_rl);
            if is_rl {
                key_pool.mark_rate_limited(provider_name, base_url, &api_key, delay).await;
            } else {
                key_pool.mark_transient(provider_name, base_url, &api_key, Some(delay)).await;
            }
            if attempt == max_attempts {
                return Err(LlmError::ProviderError {
                    status: code,
                    message: error_body,
                });
            }
            let _ = sender
                .send(StreamEvent::RetryNotice {
                    reason: format!("HTTP {} - rotating key", status),
                    attempt,
                    model: request.model.clone(),
                })
                .await;
            continue;
        }

        key_pool.record_failure(provider_name, &api_key, code, None);
        return Err(LlmError::ProviderError {
            status: status.as_u16(),
            message: error_body,
        });
    }

    Err(LlmError::ProviderError {
        status: 500,
        message: "Maximum retries exceeded".to_string(),
    })
}


pub async fn stream_perplexity_oauth(
    client: &reqwest::Client,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let excluded = std::collections::HashSet::new();
    let acc = crate::llm::oauth_accounts::acquire_valid_account(client, "perplexity_oauth", &excluded)
        .await
        .map_err(|e| LlmError::ProviderError { status: 401, message: e })?;
    let pool = KeyPool::new();
    pool.set_keys("perplexity_oauth", vec![acc.access_token]);
    stream_openai_chat(
        client,
        "https://api.perplexity.ai",
        &pool,
        "perplexity_oauth",
        caps_registry,
        request,
        sender,
    )
    .await
}
