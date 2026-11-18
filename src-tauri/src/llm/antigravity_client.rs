use crate::llm::capabilities::CapabilityRegistry;
use crate::llm::oauth_accounts;
use crate::llm::request_compat::{ChatRequest, ContentPart, MessageContent};
use crate::llm::{LlmError, StreamEvent, TokenUsage};
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

static AG_THOUGHT_SIGNATURES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn sig_store() -> &'static Mutex<HashMap<String, String>> {
    AG_THOUGHT_SIGNATURES.get_or_init(|| Mutex::new(HashMap::new()))
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

fn clean_schema_for_antigravity(val: &serde_json::Value) -> serde_json::Value {
    const UNSUPPORTED: &[&str] = &[
        "patternProperties",
        "additionalProperties",
        "$schema",
        "$id",
        "$ref",
        "$defs",
        "definitions",
        "examples",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "multipleOf",
        "pattern",
        "format",
        "minItems",
        "maxItems",
        "uniqueItems",
        "minProperties",
        "maxProperties",
    ];
    match val {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if k == "properties" {
                    if let serde_json::Value::Object(props_map) = v {
                        let mut cleaned_props = serde_json::Map::new();
                        for (pk, pv) in props_map {
                            cleaned_props.insert(pk.clone(), clean_schema_for_antigravity(pv));
                        }
                        out.insert(k.clone(), serde_json::Value::Object(cleaned_props));
                    }
                    continue;
                }
                if UNSUPPORTED.contains(&k.as_str()) {
                    continue;
                }
                if (k == "anyOf" || k == "oneOf") && v.is_array() {
                    if let Some(first) = v.as_array().and_then(|a| a.first()) {
                        if let serde_json::Value::Object(sub) = clean_schema_for_antigravity(first) {
                            for (sk, sv) in sub {
                                out.entry(sk).or_insert(sv);
                            }
                            continue;
                        }
                    }
                }
                out.insert(k.clone(), clean_schema_for_antigravity(v));
            }
            if let Some(serde_json::Value::Array(req_arr)) = out.get("required").cloned() {
                let prop_keys: std::collections::HashSet<String> = out
                    .get("properties")
                    .and_then(|p| p.as_object())
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
                let filtered: Vec<serde_json::Value> = req_arr
                    .into_iter()
                    .filter(|item| item.as_str().map(|s| prop_keys.contains(s)).unwrap_or(false))
                    .collect();
                if filtered.is_empty() {
                    out.remove("required");
                } else {
                    out.insert("required".to_string(), serde_json::Value::Array(filtered));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(clean_schema_for_antigravity).collect())
        }
        other => other.clone(),
    }
}

pub async fn stream_antigravity(
    client: &reqwest::Client,
    caps_registry: &CapabilityRegistry,
    request: ChatRequest,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let model_clean = request.model.strip_prefix("models/").unwrap_or(&request.model).to_string();
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
                MessageContent::Parts(vec_parts) => {
                    for p in vec_parts {
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

    let mut merged_contents: Vec<serde_json::Value> = Vec::with_capacity(contents.len());
    for item in contents {
        let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("").to_string();
        let can_merge = merged_contents
            .last()
            .and_then(|prev| prev.get("role").and_then(|r| r.as_str()))
            .map(|r| r == role)
            .unwrap_or(false);
        if can_merge {
            if let (Some(prev_parts), Some(cur_parts)) = (
                merged_contents.last_mut().and_then(|p| p.get_mut("parts")).and_then(|a| a.as_array_mut()),
                item.get("parts").and_then(|a| a.as_array()),
            ) {
                prev_parts.extend(cur_parts.iter().cloned());
                continue;
            }
        }
        merged_contents.push(item);
    }
    let contents = merged_contents;

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
    let is_gpt_oss = m_low.contains("gpt-oss");
    let is_claude = m_low.contains("claude");
    if let Some(budget) = request.thinking_budget {
        if is_gpt_oss {
        } else if (m_low.contains("gemini-3") || m_low.contains("gemini-4")) && !is_claude {
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
        } else if caps.supports_reasoning || m_low.contains("2.5") || m_low.contains("thinking") || is_claude {
            let cur_max = gen_cfg
                .get("maxOutputTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let min_required = budget.saturating_add(4096);
            if cur_max <= budget {
                let target_max = budget.saturating_add(8192).max(min_required).min(64000);
                gen_cfg.insert("maxOutputTokens".to_string(), serde_json::json!(target_max));
            }
            if is_claude {
                gen_cfg.remove("temperature");
                gen_cfg.remove("topP");
            }
            gen_cfg.insert(
                "thinkingConfig".to_string(),
                serde_json::json!({
                    "includeThoughts": true,
                    "thinkingBudget": budget,
                }),
            );
        }
    }

    let mut inner_req = serde_json::json!({
        "contents": contents,
        "generationConfig": serde_json::Value::Object(gen_cfg),
    });
    if !system_parts.is_empty() {
        inner_req["systemInstruction"] = serde_json::json!({ "role": "user", "parts": system_parts });
    }
    inner_req["session_id"] = serde_json::json!(oauth_accounts::machine_session_id());
    if let Some(ref tools) = request.tools {
        if !tools.is_empty() {
            let decls: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    let mut cleaned = clean_schema_for_antigravity(&t.function.parameters);
                    if let serde_json::Value::Object(ref mut obj) = cleaned {
                        obj.entry("type".to_string()).or_insert(serde_json::json!("object"));
                    }
                    serde_json::json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "parameters": cleaned,
                    })
                })
                .collect();
            inner_req["tools"] = serde_json::json!([{ "functionDeclarations": decls }]);
        }
    }

    let target_url = "https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse";
    let mut excluded_ids = HashSet::new();
    let mut last_err: Option<LlmError> = None;
    let max_attempts = 6u32;

    for attempt in 1..=max_attempts {
        let mut acc = match oauth_accounts::acquire_valid_account_for_model(client, "antigravity", &excluded_ids, Some(&model_clean)).await {
            Ok(a) => a,
            Err(e) => {
                return Err(last_err.unwrap_or(LlmError::ProviderError { status: 401, message: e }));
            }
        };

        let project_id = match acc.project_id.clone() {
            Some(p) if !p.trim().is_empty() => p,
            _ => oauth_accounts::fetch_antigravity_project_id(client, &acc.access_token).await,
        };

        let envelope = serde_json::json!({
            "project": project_id,
            "model": model_clean,
            "request": inner_req,
            "requestType": "agent",
            "userAgent": "antigravity",
            "requestId": format!("agent-{}-{}", chrono::Utc::now().timestamp_millis(), &uuid::Uuid::new_v4().to_string()[..8]),
        });

        crate::logger::info("Antigravity", &format!("POST {} model={} account={} project={}", target_url, model_clean, acc.email, project_id));
        let trace_id = oauth_accounts::antigravity_trace_id();
        let mut req_builder = client
            .post(target_url)
            .bearer_auth(&acc.access_token)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("User-Agent", oauth_accounts::antigravity_hub_ua())
            .header("X-Goog-Api-Client", oauth_accounts::antigravity_api_client())
            .header("Client-Metadata", oauth_accounts::antigravity_client_metadata())
            .header("X-Machine-Session-Id", oauth_accounts::machine_session_id())
            .header("X-Goog-Request-Reason", "user_initiated")
            .header("x-cloudaicompanion-trace-id", &trace_id);
        if !project_id.is_empty() {
            req_builder = req_builder.header("X-Goog-User-Project", &project_id);
        }
        let response = match req_builder
            .json(&envelope)
            .send()
            .await
        {
            Ok(r) => r,
            Err(err) => {
                oauth_accounts::mark_account_cooldown(&acc.id, 15, &err.to_string()).await;
                excluded_ids.insert(acc.id.clone());
                if attempt == max_attempts {
                    return Err(LlmError::Network(err));
                }
                let _ = sender
                    .send(StreamEvent::RetryNotice {
                        reason: format!("Antigravity network error ({}): {}", acc.email, err),
                        attempt,
                        model: request.model.clone(),
                    })
                    .await;
                continue;
            }
        };

        let status = response.status();
        if status.as_u16() == 401 {
            if let Ok(refreshed) = oauth_accounts::force_refresh_account(client, &acc.id).await {
                acc = refreshed;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                let retry_trace = oauth_accounts::antigravity_trace_id();
                let mut retry_builder = client
                    .post(target_url)
                    .bearer_auth(&acc.access_token)
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .header("User-Agent", oauth_accounts::antigravity_hub_ua())
                    .header("X-Goog-Api-Client", oauth_accounts::antigravity_api_client())
                    .header("Client-Metadata", oauth_accounts::antigravity_client_metadata())
                    .header("X-Machine-Session-Id", oauth_accounts::machine_session_id())
                    .header("X-Goog-Request-Reason", "user_initiated")
                    .header("x-cloudaicompanion-trace-id", &retry_trace);
                if !project_id.is_empty() {
                    retry_builder = retry_builder.header("X-Goog-User-Project", &project_id);
                }
                if let Ok(r2) = retry_builder
                    .json(&envelope)
                    .send()
                    .await
                {
                    if r2.status().is_success() {
                        return parse_antigravity_sse(r2, sender).await;
                    }
                }
            }
            let msg_401 = format!("Antigravity authentication failed (401) for {}", acc.email);
            oauth_accounts::mark_account_cooldown(&acc.id, 300, &msg_401).await;
            excluded_ids.insert(acc.id.clone());
            last_err = Some(LlmError::ProviderError {
                status: 401,
                message: msg_401.clone(),
            });
            if attempt == max_attempts {
                return Err(LlmError::ProviderError {
                    status: 401,
                    message: msg_401,
                });
            }
            continue;
        }

        if status.is_success() {
            match parse_antigravity_sse(response, sender.clone()).await {
                Ok(()) => return Ok(()),
                Err(LlmError::ProviderError { status: scode, message: smsg }) => {
                    let cd = if scode == 429 { 120 } else { 30 };
                    if scode == 429 {
                        oauth_accounts::mark_account_quota_exhausted(&acc.id, Some(0.0), None).await;
                    }
                    oauth_accounts::mark_account_cooldown(&acc.id, cd, &smsg).await;
                    excluded_ids.insert(acc.id.clone());
                    last_err = Some(LlmError::ProviderError {
                        status: scode,
                        message: format!("{} ({})", smsg, acc.email),
                    });
                    if attempt < max_attempts && (scode == 429 || scode >= 500 || scode == 403) {
                        let _ = sender
                            .send(StreamEvent::RetryNotice {
                                reason: format!("Antigravity stream {} ({}): switching account", scode, acc.email),
                                attempt,
                                model: request.model.clone(),
                            })
                            .await;
                        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                        continue;
                    }
                    return Err(LlmError::ProviderError {
                        status: scode,
                        message: smsg,
                    });
                }
                Err(e) => return Err(e),
            }
        }

        let code = status.as_u16();
        let err_body = response.text().await.unwrap_or_default();
        crate::logger::record_http("Antigravity", "POST", target_url, code, &err_body);
        let cooldown = if code == 429 { 120 } else { 30 };
        if code == 429 {
            oauth_accounts::mark_account_quota_exhausted(&acc.id, Some(0.0), None).await;
        }
        oauth_accounts::mark_account_cooldown(&acc.id, cooldown, &err_body).await;
        excluded_ids.insert(acc.id.clone());
        last_err = Some(LlmError::ProviderError {
            status: code,
            message: format!("{} ({})", err_body, acc.email),
        });

        if attempt < max_attempts && (code == 429 || code >= 500 || code == 403) {
            let _ = sender
                .send(StreamEvent::RetryNotice {
                    reason: format!("Antigravity {} ({}): switching account", code, acc.email),
                    attempt,
                    model: request.model.clone(),
                })
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            continue;
        }

        crate::logger::error_with_trace("Antigravity", &format!("HTTP {} for model {}: {}", code, model_clean, err_body));
        return Err(LlmError::ProviderError {
            status: code,
            message: err_body,
        });
    }

    Err(LlmError::ProviderError {
        status: 429,
        message: "All Antigravity accounts exhausted or rate-limited.".to_string(),
    })
}

async fn parse_antigravity_sse(
    response: reqwest::Response,
    sender: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), LlmError> {
    let mut byte_stream = response.bytes_stream();
    let mut line_buffer = String::new();
    let mut token_usage: Option<TokenUsage> = None;
    let mut finish_reason: Option<String> = None;
    let mut tool_idx: usize = 0;

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
            let trimmed = line.trim();
            if !trimmed.starts_with("data:") {
                continue;
            }
            let payload_str = trimmed.trim_start_matches("data:").trim();
            if payload_str.is_empty() || payload_str == "[DONE]" {
                continue;
            }
            let Ok(outer) = serde_json::from_str::<serde_json::Value>(payload_str) else {
                continue;
            };
            let v = outer.get("response").unwrap_or(&outer);
            if let Some(err_obj) = outer.get("error").or_else(|| v.get("error")).filter(|e| !e.is_null()) {
                let code = err_obj
                    .get("code")
                    .and_then(|c| c.as_u64())
                    .map(|n| n as u16)
                    .unwrap_or(500);
                let msg = err_obj
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| err_obj.to_string());
                if tool_idx == 0 {
                    return Err(LlmError::ProviderError {
                        status: code,
                        message: msg,
                    });
                }
                let _ = sender.send(StreamEvent::Error { error: msg.clone() }).await;
                return Err(LlmError::ProviderError {
                    status: code,
                    message: msg,
                });
            }

            if let Some(usage) = v.get("usageMetadata") {
                let prompt = usage.get("promptTokenCount").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                let cached = usage.get("cachedContentTokenCount").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                let comp = usage.get("candidatesTokenCount").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                let thoughts = usage.get("thoughtsTokenCount").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                token_usage = Some(TokenUsage {
                    prompt_tokens: prompt,
                    completion_tokens: comp + thoughts,
                    total_tokens: prompt + comp + thoughts,
                    cached_tokens: cached,
                    cache_read_input_tokens: cached,
                    cache_creation_input_tokens: 0,
                });
            }

            if let Some(cands) = v.get("candidates").and_then(|c| c.as_array()) {
                for cand in cands {
                    if let Some(fr) = cand.get("finishReason").and_then(|f| f.as_str()) {
                        finish_reason = Some(fr.to_string());
                    }
                    if let Some(parts) = cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                        for part in parts {
                            let is_thought = part.get("thought").and_then(|b| b.as_bool()).unwrap_or(false);
                            if let Some(txt) = part.get("text").and_then(|t| t.as_str()) {
                                if !txt.is_empty() {
                                    if is_thought {
                                        let _ = sender
                                            .send(StreamEvent::ReasoningDelta {
                                                delta: txt.to_string(),
                                            })
                                            .await;
                                    } else {
                                        let _ = sender
                                            .send(StreamEvent::ContentDelta {
                                                delta: txt.to_string(),
                                            })
                                            .await;
                                    }
                                }
                            }
                            if let Some(fc) = part.get("functionCall") {
                                if let Some(fname) = fc.get("name").and_then(|n| n.as_str()) {
                                    let call_id = format!("ag_call_{}", uuid::Uuid::new_v4().simple());
                                    if let Some(sig) = part
                                        .get("thoughtSignature")
                                        .or_else(|| fc.get("thoughtSignature"))
                                        .and_then(|s| s.as_str())
                                    {
                                        save_thought_sig(&call_id, sig.to_string());
                                    }
                                    let args_str = fc
                                        .get("args")
                                        .map(|a| a.to_string())
                                        .unwrap_or_else(|| "{}".to_string());
                                    let _ = sender
                                        .send(StreamEvent::ToolCallComplete {
                                            index: tool_idx,
                                            id: call_id,
                                            name: fname.to_string(),
                                            arguments: args_str,
                                        })
                                        .await;
                                    tool_idx += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let _ = sender
        .send(StreamEvent::Finish {
            finish_reason,
            usage: token_usage,
        })
        .await;
    Ok(())
}


pub async fn generate_antigravity_image(
    client: &reqwest::Client,
    requested_model: Option<&str>,
    prompt: &str,
    aspect_ratio: Option<&str>,
) -> Result<(String, Vec<u8>, String), String> {
    use base64::Engine;
    let mut chosen_model = requested_model
        .map(|m| m.trim().strip_prefix("models/").unwrap_or(m.trim()).to_string())
        .filter(|m| !m.is_empty());

    if chosen_model.is_none() {
        if let Ok(models) = oauth_accounts::fetch_antigravity_models(client).await {
            if let Some(found) = models
                .iter()
                .find(|m| m.get("supportsImageGen").and_then(|v| v.as_bool()).unwrap_or(false))
                .and_then(|m| m.get("id").and_then(|v| v.as_str()))
            {
                chosen_model = Some(found.to_string());
            }
        }
    }
    let model_id = chosen_model.unwrap_or_else(|| "gemini-3-pro-image".to_string());
    let excluded = HashSet::new();
    let acc = oauth_accounts::acquire_valid_account(client, "antigravity", &excluded).await?;
    let project_id = match acc.project_id.clone() {
        Some(p) if !p.trim().is_empty() => p,
        _ => oauth_accounts::fetch_antigravity_project_id(client, &acc.access_token).await,
    };

    let mut gen_cfg = serde_json::json!({
        "responseModalities": ["TEXT", "IMAGE"]
    });
    if let Some(ar) = aspect_ratio.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        gen_cfg["imageConfig"] = serde_json::json!({ "aspectRatio": ar });
    }

    let envelope = serde_json::json!({
        "project": project_id,
        "model": model_id,
        "request": {
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": gen_cfg,
            "session_id": oauth_accounts::machine_session_id()
        },
        "requestType": "agent",
        "userAgent": "antigravity",
        "requestId": format!("img-{}-{}", chrono::Utc::now().timestamp_millis(), &uuid::Uuid::new_v4().to_string()[..8]),
    });

    let url = "https://daily-cloudcode-pa.googleapis.com/v1internal:generateContent";
    crate::logger::info("AntigravityImage", &format!("Generating image with model={} account={}", model_id, acc.email));
    let trace_id = oauth_accounts::antigravity_trace_id();
    let mut req_builder = client
        .post(url)
        .bearer_auth(&acc.access_token)
        .header("Content-Type", "application/json")
        .header("User-Agent", oauth_accounts::antigravity_hub_ua())
        .header("X-Goog-Api-Client", oauth_accounts::antigravity_api_client())
        .header("Client-Metadata", oauth_accounts::antigravity_client_metadata())
        .header("X-Machine-Session-Id", oauth_accounts::machine_session_id())
        .header("X-Goog-Request-Reason", "user_initiated")
        .header("x-cloudaicompanion-trace-id", &trace_id);
    if let Some(ref pid) = acc.project_id.as_deref().filter(|s| !s.is_empty()) {
        req_builder = req_builder.header("X-Goog-User-Project", *pid);
    }
    let resp = req_builder
        .json(&envelope)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status().as_u16();
    let body_text = resp.text().await.map_err(|e| e.to_string())?;
    crate::logger::record_http("AntigravityImage", "POST", url, status, &body_text[..body_text.len().min(300)]);
    if !(200..300).contains(&status) {
        crate::logger::error_with_trace("AntigravityImage", &format!("Image gen failed ({}): {}", status, body_text));
        return Err(format!("Antigravity image generation failed ({}): {}", status, body_text));
    }

    let val: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| e.to_string())?;
    let root = val.get("response").unwrap_or(&val);
    if let Some(cands) = root.get("candidates").and_then(|c| c.as_array()) {
        for cand in cands {
            if let Some(parts) = cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                for part in parts {
                    if let Some(inline) = part.get("inlineData") {
                        let mime = inline.get("mimeType").and_then(|m| m.as_str()).unwrap_or("image/png").to_string();
                        if let Some(b64) = inline.get("data").and_then(|d| d.as_str()) {
                            let bytes = base64::engine::general_purpose::STANDARD
                                .decode(b64)
                                .map_err(|e| format!("Invalid image base64: {}", e))?;
                            return Ok((model_id, bytes, mime));
                        }
                    }
                }
            }
        }
    }
    Err("No image inlineData returned by Antigravity image model".to_string())
}
