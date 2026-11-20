use crate::llm::capabilities::CapabilityRegistry;
use crate::llm::key_pool::KeyPool;
use crate::llm::request_compat::{ChatRequest, ContentPart, MessageContent};
use crate::llm::{LlmError, StreamEvent, TokenUsage};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static THOUGHT_SIGNATURES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn sig_store() -> &'static Mutex<HashMap<String, String>> {
    THOUGHT_SIGNATURES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn save_thought_sig(call_id: &str, sig: String) {
    if let Ok(mut map) = sig_store().lock() {
        if map.len() > 2000 {
            map.clear();
        }
        map.insert(call_id.to_string(), sig);
    }
}

fn get_thought_sig(call_id: &str) -> String {
    if let Ok(map) = sig_store().lock() {
        if let Some(sig) = map.get(call_id) {
            return sig.clone();
        }
    }
    "skip_thought_signature_validator".to_string()
}

pub async fn stream_google_gemini(
    client: &reqwest::Client,
    base_url: &str,
    key_pool: &KeyPool,
    provider_name: &str,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let clean_base = base_url.trim_end_matches('/');
    let root_base = if clean_base.is_empty() {
        "https://generativelanguage.googleapis.com/v1beta".to_string()
    } else if clean_base.ends_with("/openai") {
        clean_base.trim_end_matches("/openai").to_string()
    } else {
        clean_base.to_string()
    };

    let model_clean = request.model.strip_prefix("models/").unwrap_or(&request.model);
    let target_url = format!("{}/models/{}:streamGenerateContent?alt=sse", root_base, model_clean);

    let max_attempts = (key_pool.pool_size(provider_name) * 2).max(5) as u32;
    let mut unavailable_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for attempt in 1..=max_attempts {
        let api_key = key_pool.next_key_async(provider_name, base_url).await?;

        let caps = caps_registry.get(&request.model);
        let mut system_parts: Vec<serde_json::Value> = Vec::new();
        let mut contents: Vec<serde_json::Value> = Vec::new();
        let mut call_id_to_name: HashMap<String, String> = HashMap::new();

        for msg in &request.messages {
            if msg.role == "system" {
                let txt = msg.content.as_text_lossy();
                if !txt.trim().is_empty() {
                    system_parts.push(serde_json::json!({ "text": txt }));
                }
            } else if msg.role == "assistant" {
                let mut parts: Vec<serde_json::Value> = Vec::new();
                let txt = msg.content.as_text_lossy();
                if !txt.is_empty() {
                    parts.push(serde_json::json!({ "text": txt }));
                }
                if let Some(ref tcs) = msg.tool_calls {
                    for tc in tcs {
                        call_id_to_name.insert(tc.id.clone(), tc.function.name.clone());
                        let parsed_args: serde_json::Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::json!({}));
                        let sig = get_thought_sig(&tc.id);
                        parts.push(serde_json::json!({
                            "functionCall": {
                                "name": tc.function.name,
                                "args": parsed_args,
                            },
                            "thoughtSignature": sig,
                        }));
                    }
                }
                if !parts.is_empty() {
                    contents.push(serde_json::json!({
                        "role": "model",
                        "parts": parts,
                    }));
                }
            } else if msg.role == "tool" {
                let fn_name = msg
                    .name
                    .clone()
                    .or_else(|| msg.tool_call_id.as_ref().and_then(|id| call_id_to_name.get(id).cloned()))
                    .unwrap_or_else(|| "tool".to_string());
                let raw_out = msg.content.as_text_lossy();
                let resp_val = serde_json::from_str::<serde_json::Value>(&raw_out)
                    .unwrap_or_else(|_| serde_json::json!({ "result": raw_out }));
                contents.push(serde_json::json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": fn_name,
                            "response": resp_val,
                        }
                    }]
                }));
            } else {
                let mut parts: Vec<serde_json::Value> = Vec::new();
                match &msg.content {
                    MessageContent::Text(t) => {
                        if !t.is_empty() {
                            parts.push(serde_json::json!({ "text": t }));
                        }
                    }
                    MessageContent::Parts(ps) => {
                        for p in ps {
                            match p {
                                ContentPart::Text { text } => {
                                    parts.push(serde_json::json!({ "text": text }));
                                }
                                ContentPart::ImageUrl { image_url } => {
                                    if let Some(stripped) = image_url.url.strip_prefix("data:") {
                                        if let Some(semi) = stripped.find(';') {
                                            let mime = &stripped[..semi];
                                            if let Some(b64_pos) = stripped.find("base64,") {
                                                let data = &stripped[b64_pos + 7..];
                                                parts.push(serde_json::json!({
                                                    "inlineData": {
                                                        "mimeType": mime,
                                                        "data": data,
                                                    }
                                                }));
                                            }
                                        }
                                    }
                                }
                                ContentPart::InputAudio { input_audio } => {
                                    if !input_audio.data.trim().is_empty() {
                                        parts.push(serde_json::json!({
                                            "inlineData": {
                                                "mimeType": input_audio.mime_type(),
                                                "data": input_audio.data,
                                            }
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
                if !parts.is_empty() {
                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": parts,
                    }));
                }
            }
        }

        let mut gen_cfg = serde_json::Map::new();
        if let Some(t) = request.temperature {
            gen_cfg.insert("temperature".to_string(), serde_json::json!(t));
        }
        if let Some(p) = request.top_p {
            gen_cfg.insert("topP".to_string(), serde_json::json!(p));
        }
        if let Some(mt) = request.max_completion_tokens.or(request.max_tokens) {
            gen_cfg.insert("maxOutputTokens".to_string(), serde_json::json!(mt));
        }
        let m_low = model_clean.to_lowercase();
        if let Some(budget) = request.thinking_budget {
            if m_low.contains("gemini-3") || m_low.contains("gemini-4") {
                let lvl = match request.reasoning_effort.as_deref() {
                    Some("low") => "LOW",
                    Some("medium") => "MEDIUM",
                    _ => "HIGH",
                };
                gen_cfg.insert(
                    "thinkingConfig".to_string(),
                    serde_json::json!({
                        "includeThoughts": true,
                        "thinkingLevel": lvl,
                    }),
                );
            } else if caps.supports_reasoning || m_low.contains("2.5") || m_low.contains("thinking") {
                gen_cfg.insert(
                    "thinkingConfig".to_string(),
                    serde_json::json!({
                        "includeThoughts": true,
                        "thinkingBudget": budget,
                    }),
                );
            }
        }

        let mut payload = serde_json::json!({
            "contents": contents,
            "generationConfig": serde_json::Value::Object(gen_cfg),
        });

        if !system_parts.is_empty() {
            payload["systemInstruction"] = serde_json::json!({ "parts": system_parts });
        }

        if let Some(ref tools) = request.tools {
            if !tools.is_empty() {
                let decls: Vec<serde_json::Value> = tools
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "name": t.function.name,
                            "description": t.function.description,
                            "parameters": t.function.parameters,
                        })
                    })
                    .collect();
                payload["tools"] = serde_json::json!([{ "functionDeclarations": decls }]);
            }
        }

        let response = match client
            .post(&target_url)
            .header("x-goog-api-key", &api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
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
            let mut tool_idx: usize = 0;
            let mut token_usage: Option<TokenUsage> = None;
            let mut finish_reason: Option<String> = None;
            let mut last_thought_sig: Option<String> = None;
            let mut emitted_anything = false;
            let mut block_reason: Option<String> = None;

            let mut process_line = async |line: &str| {
                let data = match line.strip_prefix("data:") {
                    Some(d) => d.trim(),
                    None => return,
                };
                if data.is_empty() || data == "[DONE]" {
                    return;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
                    return;
                };

                if let Some(br) = v.get("promptFeedback").and_then(|pf| pf.get("blockReason")).and_then(|x| x.as_str()) {
                    block_reason = Some(br.to_string());
                }

                if let Some(um) = v.get("usageMetadata") {
                    let pt = um.get("promptTokenCount").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                    let ct = um.get("candidatesTokenCount").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                    let tt = um.get("totalTokenCount").and_then(|x| x.as_u64()).unwrap_or((pt + ct) as u64) as u32;
                    let cached = um.get("cachedContentTokenCount").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                    token_usage = Some(TokenUsage {
                        prompt_tokens: pt,
                        completion_tokens: ct,
                        total_tokens: tt,
                        cached_tokens: cached,
                        cache_read_input_tokens: cached,
                        cache_creation_input_tokens: 0,
                    });
                }

                if let Some(cands) = v.get("candidates").and_then(|c| c.as_array()) {
                    for cand in cands {
                        if let Some(fr) = cand.get("finishReason").and_then(|x| x.as_str()) {
                            finish_reason = Some(fr.to_string());
                        }
                        if let Some(parts) = cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                            for part in parts {
                                if let Some(sig) = part
                                    .get("thoughtSignature")
                                    .or_else(|| part.get("thought_signature"))
                                    .and_then(|s| s.as_str())
                                {
                                    last_thought_sig = Some(sig.to_string());
                                }
                                let is_thought = part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false);
                                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        emitted_anything = true;
                                        let evt = if is_thought {
                                            StreamEvent::ReasoningDelta { delta: text.to_string() }
                                        } else {
                                            StreamEvent::ContentDelta { delta: text.to_string() }
                                        };
                                        let _ = sender.send(evt).await;
                                    }
                                }
                                if let Some(fc) = part.get("functionCall") {
                                    emitted_anything = true;
                                    let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("tool").to_string();
                                    let args = fc.get("args").map(|a| a.to_string()).unwrap_or_else(|| "{}".to_string());
                                    let call_id = format!("gemini-call-{}-{}", tool_idx, uuid::Uuid::new_v4().simple());
                                    if let Some(ref sig) = last_thought_sig {
                                        save_thought_sig(&call_id, sig.clone());
                                    }
                                    let _ = sender
                                        .send(StreamEvent::ToolCallComplete {
                                            index: tool_idx,
                                            id: call_id,
                                            name,
                                            arguments: args,
                                        })
                                        .await;
                                    tool_idx += 1;
                                }
                            }
                        }
                    }
                }
            };

            while let Some(chunk_res) = byte_stream.next().await {
                let chunk = match chunk_res {
                    Ok(c) => c,
                    Err(e) => return Err(LlmError::Network(e)),
                };
                line_buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = line_buffer.find('\n') {
                    let line = line_buffer[..pos].trim_end_matches('\r').to_string();
                    line_buffer = line_buffer[pos + 1..].to_string();
                    process_line(&line).await;
                }
            }
            if !line_buffer.trim().is_empty() {
                let tail = std::mem::take(&mut line_buffer);
                process_line(tail.trim_end_matches('\r')).await;
            }

            if !emitted_anything {
                if let Some(br) = block_reason {
                    return Err(LlmError::StreamError(format!("Google Gemini blocked request: {}", br)));
                }
                if let Some(ref fr) = finish_reason {
                    if fr != "STOP" {
                        return Err(LlmError::StreamError(format!("Google Gemini stopped with reason: {}", fr)));
                    }
                }
            }

            let _ = sender
                .send(StreamEvent::Finish {
                    finish_reason,
                    usage: token_usage,
                })
                .await;
            return Ok(());
        } else {
            let status_code = status.as_u16();
            let retry_after_str = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let err_text = response.text().await.unwrap_or_default();
            if matches!(status_code, 401 | 402 | 403) {
                key_pool.mark_unavailable(provider_name, base_url, &api_key, None).await;
                unavailable_keys.insert(api_key.clone());
                let pool_sz = key_pool.pool_size(provider_name);
                if pool_sz > 1 && unavailable_keys.len() < pool_sz && attempt < max_attempts {
                    let _ = sender
                        .send(StreamEvent::RetryNotice {
                            reason: format!("Google Gemini HTTP {} - rotating key", status_code),
                            attempt,
                            model: request.model.clone(),
                        })
                        .await;
                    continue;
                }
            } else if status_code == 429 || matches!(status_code, 408 | 425 | 500..=504 | 520..=526) {
                let is_rl = status_code == 429;
                let delay = KeyPool::response_retry_delay(retry_after_str.as_deref(), attempt.saturating_sub(1), is_rl);
                if is_rl {
                    key_pool.mark_rate_limited(provider_name, base_url, &api_key, delay).await;
                } else {
                    key_pool.mark_transient(provider_name, base_url, &api_key, Some(delay)).await;
                }
                if attempt < max_attempts {
                    let pool_sz = key_pool.pool_size(provider_name);
                    let reason = if pool_sz > 1 {
                        format!("Google Gemini HTTP {} - rotating key", status_code)
                    } else if is_rl {
                        "Google Gemini rate limited (429), retrying...".to_string()
                    } else {
                        format!("Google Gemini HTTP {}, retrying...", status_code)
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
            } else {
                key_pool.record_failure(provider_name, &api_key, status_code, None);
            }
            return Err(LlmError::ProviderError {
                status: status_code,
                message: err_text,
            });
        }
    }

    Err(LlmError::StreamError("Exceeded max retry attempts for Google Gemini".to_string()))
}
