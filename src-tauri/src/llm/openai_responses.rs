use crate::llm::capabilities::CapabilityRegistry;
use crate::llm::key_pool::KeyPool;
use crate::llm::request_compat::{apply_retry_action, handle_provider_error, normalize_request, ChatRequest};
use crate::llm::{LlmError, StreamEvent, TokenUsage};
use futures_util::StreamExt;
use std::collections::HashMap;

pub async fn stream_openai_responses(
    client: &reqwest::Client,
    base_url: &str,
    key_pool: &KeyPool,
    provider_name: &str,
    caps_registry: &CapabilityRegistry,
    mut request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let clean_base = base_url.trim_end_matches('/');
    let target_url = if clean_base.ends_with("/responses") {
        clean_base.to_string()
    } else {
        format!("{}/responses", clean_base)
    };

    let max_attempts = (key_pool.pool_size(provider_name) * 2).max(5) as u32;
    let mut unavailable_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for attempt in 1..=max_attempts {
        let api_key = key_pool.next_key_async(provider_name, base_url).await?;
        let caps = caps_registry.get(&request.model);
        normalize_request(&mut request, &caps);

        let input_items: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content.as_text_lossy(),
                })
            })
            .collect();

        let mut payload = serde_json::json!({
            "model": request.model,
            "input": input_items,
            "stream": true,
        });

        if let Some(ref re) = request.reasoning_effort {
            payload["reasoning"] = serde_json::json!({ "effort": re });
        }
        if let Some(t) = request.temperature {
            payload["temperature"] = serde_json::json!(t);
        }
        if let Some(mt) = request.max_completion_tokens.or(request.max_tokens) {
            payload["max_output_tokens"] = serde_json::json!(mt);
        }
        if let Some(ref tools) = request.tools {
            let mapped_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "name": t.function.name,
                        "description": t.function.description,
                        "parameters": t.function.parameters,
                    })
                })
                .collect();
            payload["tools"] = serde_json::json!(mapped_tools);
        }

        let req_builder = client
            .post(&target_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload);

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
                        reason: format!("Responses API network failure: {}", err),
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
            let mut current_event_type = String::new();
            let mut tool_calls_map: HashMap<usize, (String, String, String)> = HashMap::new();
            let mut token_usage: Option<TokenUsage> = None;
            let mut finish_reason: Option<String> = None;

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

                    if let Some(ev) = line.strip_prefix("event: ") {
                        current_event_type = ev.trim().to_string();
                        continue;
                    }

                    if let Some(stripped) = line.strip_prefix("data: ") {
                        let data_str = stripped.trim();
                        if data_str == "[DONE]" {
                            break;
                        }

                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data_str) {
                            let event_name = val
                                .get("type")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&current_event_type);

                            match event_name {
                                "response.output_text.delta" | "response.text.delta" => {
                                    if let Some(delta) = val.get("delta").and_then(|v| v.as_str()) {
                                        if !delta.is_empty() {
                                            let _ = sender
                                                .send(StreamEvent::ContentDelta {
                                                    delta: delta.to_string(),
                                                })
                                                .await;
                                        }
                                    }
                                }
                                "response.reasoning_text.delta" | "response.reasoning.delta" => {
                                    if let Some(delta) = val.get("delta").and_then(|v| v.as_str()) {
                                        if !delta.is_empty() {
                                            let _ = sender
                                                .send(StreamEvent::ReasoningDelta {
                                                    delta: delta.to_string(),
                                                })
                                                .await;
                                        }
                                    }
                                }
                                "response.output_item.added" => {
                                    let out_idx = val
                                        .get("output_index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as usize;
                                    if let Some(item) = val.get("item") {
                                        let item_type = item.get("type").and_then(|v| v.as_str());
                                        if item_type == Some("function_call") {
                                            let call_id = item
                                                .get("id")
                                                .or_else(|| item.get("call_id"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            let fn_name = item
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            tool_calls_map.insert(
                                                out_idx,
                                                (call_id, fn_name, String::new()),
                                            );
                                        }
                                    }
                                }
                                "response.function_call_arguments.delta" => {
                                    let out_idx = val
                                        .get("output_index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as usize;
                                    let call_id = val
                                        .get("call_id")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    let delta_str = val
                                        .get("delta")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    if let Some(entry) = tool_calls_map.get_mut(&out_idx) {
                                        entry.2.push_str(delta_str);
                                    }

                                    let _ = sender
                                        .send(StreamEvent::ToolCallDelta {
                                            index: out_idx,
                                            id: call_id,
                                            name: None,
                                            arguments_delta: delta_str.to_string(),
                                        })
                                        .await;
                                }
                                "response.output_item.done" => {
                                    let out_idx = val
                                        .get("output_index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as usize;
                                    if let Some(item) = val.get("item") {
                                        let item_type = item.get("type").and_then(|v| v.as_str());
                                        if item_type == Some("function_call") {
                                            let call_id = item
                                                .get("id")
                                                .or_else(|| item.get("call_id"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let fn_name = item
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let args = item
                                                .get("arguments")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");

                                            let _ = sender
                                                .send(StreamEvent::ToolCallComplete {
                                                    index: out_idx,
                                                    id: call_id.to_string(),
                                                    name: fn_name.to_string(),
                                                    arguments: args.to_string(),
                                                })
                                                .await;
                                            tool_calls_map.remove(&out_idx);
                                        }
                                    }
                                }
                                "response.completed" | "response.done" => {
                                    finish_reason = Some("completed".to_string());
                                    let usage_obj = val
                                        .get("response")
                                        .and_then(|r| r.get("usage"))
                                        .or_else(|| val.get("usage"));

                                    if let Some(u) = usage_obj {
                                        let prompt_tokens = u
                                            .get("input_tokens")
                                            .or_else(|| u.get("prompt_tokens"))
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0)
                                            as u32;
                                        let completion_tokens = u
                                            .get("output_tokens")
                                            .or_else(|| u.get("completion_tokens"))
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0)
                                            as u32;
                                        let total_tokens = u
                                            .get("total_tokens")
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0)
                                            as u32;
                                        let cached_tokens = u
                                            .get("input_tokens_details")
                                            .and_then(|d| d.get("cached_tokens"))
                                            .and_then(|v| v.as_u64())
                                            .unwrap_or(0) as u32;
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
                                _ => {}
                            }
                        }
                    }
                }
            }

            let mut remaining_indices: Vec<usize> = tool_calls_map.keys().cloned().collect();
            remaining_indices.sort_unstable();
            for idx in remaining_indices {
                if let Some((cid, name, args)) = tool_calls_map.remove(&idx) {
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
