use crate::llm::capabilities::CapabilityRegistry;
use crate::llm::key_pool::KeyPool;
use crate::llm::request_compat::{apply_retry_action, handle_provider_error, normalize_request, ChatRequest};
use crate::llm::{LlmError, StreamEvent, TokenUsage};
use futures_util::StreamExt;
use std::collections::HashMap;

pub async fn stream_openai_codex(
    client: &reqwest::Client,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let empty_pool = KeyPool::new();
    stream_openai_responses_inner(
        client,
        "https://chatgpt.com/backend-api/codex/responses",
        &empty_pool,
        "openai_codex",
        caps_registry,
        request,
        sender,
        true,
    )
    .await
}

pub async fn stream_xai_oauth(
    client: &reqwest::Client,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let empty_pool = KeyPool::new();
    stream_openai_responses_inner(
        client,
        "https://cli-chat-proxy.grok.com/v1/responses",
        &empty_pool,
        "xai_oauth",
        caps_registry,
        request,
        sender,
        true,
    )
    .await
}

pub async fn stream_openai_responses(
    client: &reqwest::Client,
    base_url: &str,
    key_pool: &KeyPool,
    provider_name: &str,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    stream_openai_responses_inner(
        client,
        base_url,
        key_pool,
        provider_name,
        caps_registry,
        request,
        sender,
        false,
    )
    .await
}

async fn stream_openai_responses_inner(
    client: &reqwest::Client,
    base_url: &str,
    key_pool: &KeyPool,
    provider_name: &str,
    caps_registry: &CapabilityRegistry,
    mut request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
    use_codex_oauth: bool,
) -> Result<(), LlmError> {
    let clean_base = base_url.trim_end_matches('/');
    let target_url = if clean_base.ends_with("/responses") {
        clean_base.to_string()
    } else {
        format!("{}/responses", clean_base)
    };

    let max_attempts = if use_codex_oauth { 6 } else { (key_pool.pool_size(provider_name) * 2).max(5) as u32 };
    let mut unavailable_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut excluded_oauth: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_oauth_err: Option<LlmError> = None;
    for attempt in 1..=max_attempts {
        let (api_key, oauth_acc) = if use_codex_oauth {
            let acc = match crate::llm::oauth_accounts::acquire_valid_account_for_model(client, provider_name, &excluded_oauth, Some(&request.model)).await {
                Ok(a) => a,
                Err(e) => return Err(last_oauth_err.unwrap_or(LlmError::ProviderError { status: 401, message: e })),
            };
            (acc.access_token.clone(), Some(acc))
        } else {
            (key_pool.next_key_async(provider_name, base_url).await?, None)
        };
        let acc_client_opt = oauth_acc
            .as_ref()
            .and_then(|a| crate::llm::oauth_accounts::client_for_account(a));
        let active_client = acc_client_opt.as_ref().unwrap_or(client);

        let caps = caps_registry.get(&request.model);
        normalize_request(&mut request, &caps);

        let mut system_instructions = Vec::new();
        let mut input_items: Vec<serde_json::Value> = Vec::new();
        for m in &request.messages {
            if m.role == "system" {
                let txt = m.content.as_text_lossy();
                if !txt.trim().is_empty() {
                    system_instructions.push(txt);
                }
            } else if m.role == "tool" {
                let cid = m.tool_call_id.clone().unwrap_or_default();
                input_items.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": cid,
                    "output": m.content.as_text_lossy(),
                }));
            } else if m.role == "assistant" {
                let txt = m.content.as_text_lossy();
                if !txt.is_empty() {
                    input_items.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": txt,
                        }],
                    }));
                }
                if let Some(ref tcs) = m.tool_calls {
                    for tc in tcs {
                        input_items.push(serde_json::json!({
                            "type": "function_call",
                            "call_id": tc.id,
                            "name": tc.function.name,
                            "arguments": tc.function.arguments,
                        }));
                    }
                }
            } else {
                let mut content_parts: Vec<serde_json::Value> = Vec::new();
                match &m.content {
                    crate::llm::request_compat::MessageContent::Text(t) => {
                        if !t.is_empty() {
                            content_parts.push(serde_json::json!({
                                "type": "input_text",
                                "text": t,
                            }));
                        }
                    }
                    crate::llm::request_compat::MessageContent::Parts(parts) => {
                        for part in parts {
                            match part {
                                crate::llm::request_compat::ContentPart::Text { text } => {
                                    content_parts.push(serde_json::json!({
                                        "type": "input_text",
                                        "text": text,
                                    }));
                                }
                                crate::llm::request_compat::ContentPart::ImageUrl { image_url } => {
                                    if image_url.url.starts_with("data:video/") {
                                        content_parts.push(serde_json::json!({
                                            "type": "input_text",
                                            "text": "[video omitted: OpenAI Responses API does not support inline video]",
                                        }));
                                    } else {
                                        let mut img_obj = serde_json::json!({
                                            "type": "input_image",
                                            "image_url": image_url.url,
                                        });
                                        if let Some(ref d) = image_url.detail {
                                            img_obj["detail"] = serde_json::json!(d);
                                        }
                                        content_parts.push(img_obj);
                                    }
                                }
                                crate::llm::request_compat::ContentPart::InputAudio { input_audio } => {
                                    content_parts.push(serde_json::json!({
                                        "type": "input_audio",
                                        "input_audio": {
                                            "data": input_audio.data,
                                            "format": input_audio.format,
                                        },
                                    }));
                                }
                            }
                        }
                    }
                }
                if !content_parts.is_empty() {
                    input_items.push(serde_json::json!({
                        "type": "message",
                        "role": "user",
                        "content": content_parts,
                    }));
                }
            }
        }

        let mut payload = serde_json::json!({
            "model": request.model,
            "input": input_items,
            "stream": true,
        });
        let instr_joined = system_instructions.join("\n\n");
        if !instr_joined.is_empty() {
            payload["instructions"] = serde_json::json!(instr_joined);
        } else if use_codex_oauth && provider_name == "openai_codex" {
            payload["instructions"] = serde_json::json!("You are a helpful assistant.");
        }

        let is_codex_backend = use_codex_oauth && provider_name == "openai_codex";
        if let Some(ref re) = request.reasoning_effort {
            payload["reasoning"] = serde_json::json!({ "effort": re });
        }
        if !is_codex_backend {
            if let Some(t) = request.temperature {
                payload["temperature"] = serde_json::json!(t);
            }
            if let Some(mt) = request.max_completion_tokens.or(request.max_tokens) {
                payload["max_output_tokens"] = serde_json::json!(mt);
            }
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

        if use_codex_oauth && provider_name == "openai_codex" {
            payload["store"] = serde_json::json!(false);
        }
        let mut req_builder = active_client
            .post(&target_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json");
        if let Some(ref acc) = oauth_acc {
            if let Some(ref aid) = acc.account_id {
                if !aid.trim().is_empty() {
                    req_builder = req_builder.header("ChatGPT-Account-Id", aid.trim());
                }
            }
        }
        let req_builder = req_builder.json(&payload);

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
            let mut raw_buf: Vec<u8> = Vec::new();
            let mut line_buffer = String::new();
            let mut current_event_type = String::new();
            let mut tool_calls_map: HashMap<usize, (String, String, String)> = HashMap::new();
            let mut token_usage: Option<TokenUsage> = None;
            let mut finish_reason: Option<String> = None;

            'resp_loop: while let Some(chunk_res) = byte_stream.next().await {
                let chunk = match chunk_res {
                    Ok(c) => c,
                    Err(e) => return Err(LlmError::Network(e)),
                };

                raw_buf.extend_from_slice(&chunk);
                crate::llm::drain_valid_utf8(&mut raw_buf, &mut line_buffer);

                while let Some(pos) = line_buffer.find('\n') {
                    let line = line_buffer[..pos].trim_end_matches('\r').to_string();
                    line_buffer.drain(..=pos);

                    if line.is_empty() {
                        current_event_type.clear();
                        continue;
                    }
                    if line.starts_with(':') {
                        continue;
                    }

                    if let Some(ev) = line.strip_prefix("event:") {
                        current_event_type = ev.trim().to_string();
                        continue;
                    }

                    if let Some(stripped) = line.strip_prefix("data:") {
                        let data_str = stripped.trim();
                        if data_str == "[DONE]" {
                            break 'resp_loop;
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
                                                .get("call_id")
                                                .or_else(|| item.get("id"))
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
                                                .get("call_id")
                                                .or_else(|| item.get("id"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let fn_name = item
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let args = item
                                                .get("arguments")
                                                .and_then(|v| v.as_str())
                                                .filter(|s| !s.is_empty())
                                                .map(|s| s.to_string())
                                                .unwrap_or_else(|| {
                                                    tool_calls_map
                                                        .get(&out_idx)
                                                        .map(|e| e.2.clone())
                                                        .unwrap_or_default()
                                                });

                                            let _ = sender
                                                .send(StreamEvent::ToolCallComplete {
                                                    index: out_idx,
                                                    id: call_id.to_string(),
                                                    name: fn_name.to_string(),
                                                    arguments: args,
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
                                "error" | "response.failed" => {
                                    let err_obj = val
                                        .get("error")
                                        .or_else(|| val.get("response").and_then(|r| r.get("error")));
                                    let msg = err_obj
                                        .and_then(|e| e.get("message"))
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| data_str.to_string());
                                    let code_str = err_obj
                                        .and_then(|e| e.get("code"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let status_code = if code_str.contains("rate_limit") || msg.to_lowercase().contains("rate limit") || msg.to_lowercase().contains("usage limit") {
                                        429
                                    } else {
                                        500
                                    };
                                    if let Some(ref acc) = oauth_acc {
                                        if status_code == 429 {
                                            crate::llm::oauth_accounts::mark_account_quota_exhausted(&acc.id, Some(0.0), None).await;
                                            crate::llm::oauth_accounts::mark_account_cooldown(&acc.id, 120, &msg).await;
                                        } else {
                                            crate::llm::oauth_accounts::mark_account_cooldown(&acc.id, 30, &msg).await;
                                        }
                                    }
                                    return Err(LlmError::ProviderError {
                                        status: status_code,
                                        message: msg,
                                    });
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
        if let Some(ref acc) = oauth_acc {
            let cd = if code == 429 { 120 } else { 30 };
            if code == 429 {
                crate::llm::oauth_accounts::mark_account_quota_exhausted(&acc.id, Some(0.0), None).await;
            }
            crate::llm::oauth_accounts::mark_account_cooldown(&acc.id, cd, &error_body).await;
            excluded_oauth.insert(acc.id.clone());
            last_oauth_err = Some(LlmError::ProviderError { status: code, message: error_body.clone() });
            if attempt < max_attempts && matches!(code, 401 | 403 | 429 | 500..=504) {
                if code == 401 {
                    let _ = crate::llm::oauth_accounts::force_refresh_account(active_client, &acc.id).await;
                }
                let acc_cnt = crate::llm::oauth_accounts::list_accounts(Some("openai_codex")).await.into_iter().filter(|a| a.enabled).count();
                let reason = if acc_cnt > 1 {
                    format!("ChatGPT Codex HTTP {} ({}) - switching account", code, acc.email)
                } else if code == 429 {
                    format!("ChatGPT Codex rate limited (429, {}), retrying...", acc.email)
                } else {
                    format!("ChatGPT Codex HTTP {} ({}), retrying...", code, acc.email)
                };
                let _ = sender
                    .send(StreamEvent::RetryNotice {
                        reason,
                        attempt,
                        model: request.model.clone(),
                    })
                    .await;
                continue;
            }
            return Err(LlmError::ProviderError { status: code, message: error_body });
        }
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
            let pool_sz = key_pool.pool_size(provider_name);
            let reason = if pool_sz > 1 {
                format!("HTTP {} - rotating key", status)
            } else if is_rl {
                "Rate limited (429), retrying...".to_string()
            } else {
                format!("HTTP {}, retrying...", code)
            };
            let _ = sender
                .send(StreamEvent::RetryNotice {
                    reason,
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
