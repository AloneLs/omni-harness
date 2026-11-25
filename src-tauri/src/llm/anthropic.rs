use crate::llm::capabilities::CapabilityRegistry;
use crate::llm::key_pool::KeyPool;
use crate::llm::request_compat::{
    apply_retry_action, handle_provider_error, normalize_request, ChatRequest,
    ContentPart, MessageContent,
};
use crate::llm::{LlmError, StreamEvent, TokenUsage};
use futures_util::StreamExt;
use std::collections::HashMap;

struct AnthropicToolBlock {
    id: String,
    name: String,
    arguments: String,
}

pub async fn stream_anthropic_oauth(
    client: &reqwest::Client,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let empty_pool = KeyPool::new();
    stream_anthropic_inner(
        client,
        "https://api.anthropic.com/v1/messages",
        &empty_pool,
        "claude_oauth",
        caps_registry,
        request,
        sender,
        true,
    )
    .await
}

pub async fn stream_kimi_oauth(
    client: &reqwest::Client,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let empty_pool = KeyPool::new();
    stream_anthropic_inner(
        client,
        "https://api.kimi.com/coding/v1/messages",
        &empty_pool,
        "kimi_oauth",
        caps_registry,
        request,
        sender,
        true,
    )
    .await
}

pub async fn stream_zcode_oauth(
    client: &reqwest::Client,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let empty_pool = KeyPool::new();
    stream_anthropic_inner(
        client,
        "https://api.z.ai/api/anthropic/v1/messages",
        &empty_pool,
        "zcode_oauth",
        caps_registry,
        request,
        sender,
        true,
    )
    .await
}

pub async fn stream_anthropic(
    client: &reqwest::Client,
    base_url: &str,
    key_pool: &KeyPool,
    provider_name: &str,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    stream_anthropic_inner(
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

async fn stream_anthropic_inner(
    client: &reqwest::Client,
    base_url: &str,
    key_pool: &KeyPool,
    provider_name: &str,
    caps_registry: &CapabilityRegistry,
    mut request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
    use_claude_oauth: bool,
) -> Result<(), LlmError> {
    let clean_base = base_url.trim_end_matches('/');
    let target_url = if clean_base.is_empty() {
        "https:\u{2F}\u{2F}api.anthropic.com/v1/messages".to_string()
    } else if clean_base.ends_with("/messages") {
        clean_base.to_string()
    } else {
        format!("{}/messages", clean_base)
    };

    let max_attempts = if use_claude_oauth { 6 } else { (key_pool.pool_size(provider_name) * 2).max(5) as u32 };
    let mut unavailable_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut excluded_oauth: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_oauth_err: Option<LlmError> = None;
    let is_native_anthropic = !(use_claude_oauth && (provider_name == "kimi_oauth" || provider_name == "zcode_oauth"))
        && (use_claude_oauth || target_url.contains("anthropic.com"));

    for attempt in 1..=max_attempts {
        let (api_key, oauth_acc) = if use_claude_oauth {
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

        let mut system_text = String::new();
        let mut messages_payload: Vec<serde_json::Value> = Vec::new();
        let mut has_tool_results = false;

        for msg in &request.messages {
            if msg.name.as_deref() == Some("ephemeral_runtime_state") {
                let tail_text = msg.content.as_text_lossy();
                if let Some(last_m) = messages_payload.last_mut() {
                    if last_m.get("role").and_then(|r| r.as_str()) == Some("user") {
                        if let Some(arr) = last_m.get_mut("content").and_then(|c| c.as_array_mut()) {
                            if let Some(last_blk) = arr.last_mut() {
                                last_blk["cache_control"] = serde_json::json!({ "type": "ephemeral" });
                            }
                            arr.push(serde_json::json!({
                                "type": "text",
                                "text": tail_text,
                            }));
                            continue;
                        } else if let Some(txt) = last_m.get("content").and_then(|c| c.as_str()).map(|s| s.to_string()) {
                            last_m["content"] = serde_json::json!([
                                {
                                    "type": "text",
                                    "text": txt,
                                    "cache_control": { "type": "ephemeral" }
                                },
                                {
                                    "type": "text",
                                    "text": tail_text,
                                }
                            ]);
                            continue;
                        }
                    }
                }
                messages_payload.push(serde_json::json!({
                    "role": "user",
                    "content": tail_text,
                }));
            } else if msg.role == "system" {
                if !system_text.is_empty() {
                    system_text.push('\n');
                }
                system_text.push_str(&msg.content.as_text_lossy());
            } else if msg.role == "tool" {
                has_tool_results = true;
                let call_id = msg.tool_call_id.clone().unwrap_or_default();
                let tool_content = msg.content.as_text_lossy();
                let block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": tool_content,
                });
                if let Some(last) = messages_payload.last_mut() {
                    if last.get("role").and_then(|v| v.as_str()) == Some("user") {
                        if let Some(arr) = last.get_mut("content").and_then(|v| v.as_array_mut()) {
                            arr.push(block);
                            continue;
                        }
                    }
                }
                messages_payload.push(serde_json::json!({
                    "role": "user",
                    "content": [block]
                }));
            } else if msg.role == "assistant" {
                if let Some(ref tcs) = msg.tool_calls {
                    let mut content_arr: Vec<serde_json::Value> = Vec::new();
                    let text = msg.content.as_text_lossy();
                    if !text.is_empty() {
                        content_arr.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                    for tc in tcs {
                        let parsed_input: serde_json::Value =
                            serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::json!({}));
                        content_arr.push(serde_json::json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.function.name,
                            "input": parsed_input,
                        }));
                    }
                    messages_payload.push(serde_json::json!({
                        "role": "assistant",
                        "content": content_arr,
                    }));
                } else {
                    messages_payload.push(serde_json::json!({
                        "role": "assistant",
                        "content": msg.content.as_text_lossy(),
                    }));
                }
            } else {
                match &msg.content {
                    MessageContent::Text(t) => {
                        messages_payload.push(serde_json::json!({
                            "role": "user",
                            "content": t,
                        }));
                    }
                    MessageContent::Parts(parts) => {
                        let mut arr: Vec<serde_json::Value> = Vec::new();
                        for part in parts {
                            match part {
                                ContentPart::Text { text } => {
                                    arr.push(serde_json::json!({
                                        "type": "text",
                                        "text": text,
                                    }));
                                }
                                ContentPart::ImageUrl { image_url } => {
                                    if image_url.url.starts_with("data:video/") {
                                        arr.push(serde_json::json!({
                                            "type": "text",
                                            "text": "[video omitted: Anthropic API does not support inline video]",
                                        }));
                                    } else if let Some(stripped_b64) =
                                        image_url.url.strip_prefix("data:")
                                    {
                                        if let Some(semi_pos) = stripped_b64.find(';') {
                                            let media_type = &stripped_b64[..semi_pos];
                                            if let Some(b64_start) =
                                                stripped_b64.find("base64,")
                                            {
                                                let data = &stripped_b64[b64_start + 7..];
                                                arr.push(serde_json::json!({
                                                    "type": "image",
                                                    "source": {
                                                        "type": "base64",
                                                        "media_type": media_type,
                                                        "data": data,
                                                    }
                                                }));
                                            }
                                        }
                                    }
                                }
                                ContentPart::InputAudio { .. } => {}
                            }
                        }
                        messages_payload.push(serde_json::json!({
                            "role": "user",
                            "content": arr,
                        }));
                    }
                }
            }
        }

        let len_msgs = messages_payload.len();
        let mut bp_indices = Vec::new();
        if len_msgs >= 16 {
            bp_indices.push(len_msgs - 15);
        }
        if len_msgs >= 2 {
            bp_indices.push(len_msgs - 2);
        }
        for idx in bp_indices {
            if let Some(target_msg) = messages_payload.get_mut(idx) {
                if let Some(content_val) = target_msg.get_mut("content") {
                    if let Some(arr) = content_val.as_array_mut() {
                        if let Some(last_part) = arr.last_mut() {
                            if last_part.get("cache_control").is_none() {
                                last_part["cache_control"] = serde_json::json!({ "type": "ephemeral" });
                            }
                        }
                    } else if let Some(txt) = content_val.as_str() {
                        let owned = txt.to_string();
                        *content_val = serde_json::json!([{
                            "type": "text",
                            "text": owned,
                            "cache_control": { "type": "ephemeral" }
                        }]);
                    }
                }
            }
        }

        let max_tokens = request
            .max_tokens
            .or(request.max_completion_tokens)
            .unwrap_or(caps.max_output_tokens.max(16384));

        let mut payload = serde_json::json!({
            "model": request.model,
            "messages": messages_payload,
            "max_tokens": max_tokens,
            "stream": true,
        });

        if !system_text.is_empty() {
            payload["system"] = serde_json::json!([{
                "type": "text",
                "text": system_text,
                "cache_control": { "type": "ephemeral" }
            }]);
        }

        if let Some(budget) = request.thinking_budget {
            payload["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
            payload["temperature"] = serde_json::json!(1.0);
        } else if let Some(t) = request.temperature {
            payload["temperature"] = serde_json::json!(t);
        }

        if let Some(ref tools) = request.tools {
            let mut mapped_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters,
                    })
                })
                .collect();
            if let Some(last_t) = mapped_tools.last_mut() {
                last_t["cache_control"] = serde_json::json!({ "type": "ephemeral" });
            }
            payload["tools"] = serde_json::json!(mapped_tools);
        }

        let use_context_editing = is_native_anthropic && has_tool_results;
        if use_context_editing {
            payload["context_management"] = serde_json::json!({
                "edits": [{
                    "type": "clear_tool_uses_20250919",
                    "trigger": { "type": "input_tokens", "value": 80000 },
                    "keep": { "type": "tool_uses", "value": 4 },
                    "clear_at_least": { "type": "input_tokens", "value": 15000 }
                }]
            });
        }

        let req_builder = if use_claude_oauth && provider_name == "kimi_oauth" {
            let k_url = if let Some(ref acc) = oauth_acc {
                if acc.account_id.as_deref() == Some("kimi.ai") {
                    "https://api.kimi.ai/coding/v1/messages"
                } else {
                    target_url.as_str()
                }
            } else {
                target_url.as_str()
            };
            active_client
                .post(k_url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("anthropic-version", "2023-06-01")
                .header("user-agent", "claude-code/1.0.0")
                .header("X-Msh-Platform", "kimi_cli")
                .header("Content-Type", "application/json")
                .json(&payload)
        } else if use_claude_oauth && provider_name == "zcode_oauth" {
            active_client
                .post(&target_url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("user-agent", "ZCode/3.17.1")
                .header("HTTP-Referer", "https://zcode.z.ai")
                .header("X-Title", "Z Code@electron")
                .header("Content-Type", "application/json")
                .json(&payload)
        } else if use_claude_oauth {
            let beta = if use_context_editing {
                "oauth-2025-04-20,claude-code-20250219,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14,context-management-2025-06-27"
            } else {
                "oauth-2025-04-20,claude-code-20250219,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14"
            };
            active_client
                .post(&target_url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("anthropic-version", "2023-06-01")
                .header("anthropic-beta", beta)
                .header("user-agent", "claude-cli/1.0.0 (external, cli)")
                .header("x-app", "cli")
                .header("Content-Type", "application/json")
                .json(&payload)
        } else {
            let beta = if use_context_editing {
                "prompt-caching-2024-07-31,context-management-2025-06-27"
            } else {
                "prompt-caching-2024-07-31"
            };
            active_client
                .post(&target_url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("anthropic-beta", beta)
                .header("Content-Type", "application/json")
                .json(&payload)
        };

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
                        reason: format!("Anthropic request error: {}", err),
                        attempt,
                        model: request.model.clone(),
                    })
                    .await;
                continue;
            }
        };

        let status = response.status();
        if status.is_success() {
            if let Some(ref acc) = oauth_acc {
                let u5h = response
                    .headers()
                    .get("anthropic-ratelimit-unified-5h-utilization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|u| ((1.0 - u) * 100.0).clamp(0.0, 100.0));
                let u7d = response
                    .headers()
                    .get("anthropic-ratelimit-unified-7d-utilization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|u| ((1.0 - u) * 100.0).clamp(0.0, 100.0));
                if u5h.is_some() || u7d.is_some() {
                    crate::llm::oauth_accounts::update_account_quota_partial(&acc.id, u5h, None, u7d, None).await;
                }
            }
            key_pool.mark_success(provider_name, base_url, &api_key).await;

            let mut byte_stream = response.bytes_stream();
            let mut raw_buf: Vec<u8> = Vec::new();
            let mut line_buffer = String::new();
            let mut current_event_type = String::new();
            let mut tool_blocks: HashMap<usize, AnthropicToolBlock> = HashMap::new();
            let mut input_tokens = 0_u32;
            let mut output_tokens = 0_u32;
            let mut cache_read_input_tokens = 0_u32;
            let mut cache_creation_input_tokens = 0_u32;
            let mut finish_reason: Option<String> = None;

            while let Some(chunk_res) = byte_stream.next().await {
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
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data_str) {
                            let event_name = val
                                .get("type")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&current_event_type);

                            match event_name {
                                "message_start" => {
                                    if let Some(msg) = val.get("message") {
                                        if let Some(u) = msg.get("usage") {
                                            if let Some(inp) = u.get("input_tokens").and_then(|v| v.as_u64()) {
                                                input_tokens = inp as u32;
                                            }
                                            if let Some(cr) = u.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
                                                cache_read_input_tokens = cr as u32;
                                            }
                                            if let Some(cc) = u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()) {
                                                cache_creation_input_tokens = cc as u32;
                                            }
                                        }
                                    }
                                }
                                "content_block_start" => {
                                    let idx = val
                                        .get("index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as usize;
                                    if let Some(cb) = val.get("content_block") {
                                        let btype = cb.get("type").and_then(|v| v.as_str());
                                        if btype == Some("tool_use") {
                                            let call_id = cb
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            let fn_name = cb
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            tool_blocks.insert(
                                                idx,
                                                AnthropicToolBlock {
                                                    id: call_id,
                                                    name: fn_name,
                                                    arguments: String::new(),
                                                },
                                            );
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    let idx = val
                                        .get("index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as usize;
                                    if let Some(delta) = val.get("delta") {
                                        let dtype = delta.get("type").and_then(|v| v.as_str());
                                        match dtype {
                                            Some("text_delta") => {
                                                if let Some(txt) =
                                                    delta.get("text").and_then(|v| v.as_str())
                                                {
                                                    if !txt.is_empty() {
                                                        let _ = sender
                                                            .send(StreamEvent::ContentDelta {
                                                                delta: txt.to_string(),
                                                            })
                                                            .await;
                                                    }
                                                }
                                            }
                                            Some("thinking_delta") => {
                                                if let Some(thk) = delta
                                                    .get("thinking")
                                                    .and_then(|v| v.as_str())
                                                {
                                                    if !thk.is_empty() {
                                                        let _ = sender
                                                            .send(StreamEvent::ReasoningDelta {
                                                                delta: thk.to_string(),
                                                            })
                                                            .await;
                                                    }
                                                }
                                            }
                                            Some("input_json_delta") => {
                                                if let Some(part) = delta
                                                    .get("partial_json")
                                                    .and_then(|v| v.as_str())
                                                {
                                                    if let Some(block) =
                                                        tool_blocks.get_mut(&idx)
                                                    {
                                                        block.arguments.push_str(part);
                                                        let _ = sender
                                                            .send(StreamEvent::ToolCallDelta {
                                                                index: idx,
                                                                id: Some(block.id.clone()),
                                                                name: Some(block.name.clone()),
                                                                arguments_delta: part.to_string(),
                                                            })
                                                            .await;
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_stop" => {
                                    let idx = val
                                        .get("index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as usize;
                                    if let Some(block) = tool_blocks.remove(&idx) {
                                        let _ = sender
                                            .send(StreamEvent::ToolCallComplete {
                                                index: idx,
                                                id: block.id,
                                                name: block.name,
                                                arguments: block.arguments,
                                            })
                                            .await;
                                    }
                                }
                                "message_delta" => {
                                    if let Some(d) = val.get("delta") {
                                        if let Some(sr) =
                                            d.get("stop_reason").and_then(|v| v.as_str())
                                        {
                                            finish_reason = Some(sr.to_string());
                                        }
                                    }
                                    if let Some(u) = val.get("usage") {
                                        if let Some(out) = u.get("output_tokens").and_then(|v| v.as_u64()) {
                                            output_tokens = out as u32;
                                        }
                                        if let Some(cr) = u.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
                                            cache_read_input_tokens = cr as u32;
                                        }
                                        if let Some(cc) = u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()) {
                                            cache_creation_input_tokens = cc as u32;
                                        }
                                    }
                                }
                                "message_stop" => {
                                    let total = input_tokens.saturating_add(output_tokens);
                                    let _ = sender
                                        .send(StreamEvent::Finish {
                                            finish_reason: finish_reason.clone(),
                                            usage: Some(TokenUsage {
                                                prompt_tokens: input_tokens,
                                                completion_tokens: output_tokens,
                                                total_tokens: total,
                                                cached_tokens: cache_read_input_tokens,
                                                cache_read_input_tokens,
                                                cache_creation_input_tokens,
                                            }),
                                        })
                                        .await;
                                }
                                "error" => {
                                    let err_obj = val.get("error");
                                    let err_type = err_obj
                                        .and_then(|e| e.get("type"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let msg = err_obj
                                        .and_then(|e| e.get("message"))
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| data_str.to_string());
                                    let code = if err_type.contains("rate_limit") {
                                        429
                                    } else if err_type.contains("overloaded") {
                                        529
                                    } else {
                                        500
                                    };
                                    return Err(LlmError::ProviderError {
                                        status: code,
                                        message: msg,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            let mut remaining_keys: Vec<usize> = tool_blocks.keys().cloned().collect();
            remaining_keys.sort_unstable();
            for idx in remaining_keys {
                if let Some(block) = tool_blocks.remove(&idx) {
                    let _ = sender
                        .send(StreamEvent::ToolCallComplete {
                            index: idx,
                            id: block.id,
                            name: block.name,
                            arguments: block.arguments,
                        })
                        .await;
                }
            }

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
                        reason: format!("Corrected Anthropic 400 via {:?}", action),
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
                let acc_cnt = crate::llm::oauth_accounts::list_accounts(Some("claude_oauth")).await.into_iter().filter(|a| a.enabled).count();
                let reason = if acc_cnt > 1 {
                    format!("Claude OAuth HTTP {} ({}) - switching account", code, acc.email)
                } else if code == 429 {
                    format!("Claude OAuth rate limited (429, {}), retrying...", acc.email)
                } else {
                    format!("Claude OAuth HTTP {} ({}), retrying...", code, acc.email)
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
                        reason: format!("HTTP {} - rotating Anthropic key", status),
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
                format!("HTTP {} - rotating Anthropic key", status)
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
