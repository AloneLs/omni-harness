use crate::llm::key_pool::KeyPool;
use crate::llm::LlmError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenRequest {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedImageItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenResponse {
    pub created: u64,
    pub images: Vec<GeneratedImageItem>,
}

pub fn encode_base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < data.len() {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < data.len() {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

pub async fn download_image_as_base64(
    client: &reqwest::Client,
    image_url: &str,
) -> Result<String, LlmError> {
    let resp = client.get(image_url).send().await?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .to_string();
    let bytes = resp.bytes().await?;
    let b64 = encode_base64(&bytes);
    Ok(format!("data:{};base64,{}", content_type, b64))
}

pub async fn generate_image(
    client: &reqwest::Client,
    base_url: &str,
    key_pool: &KeyPool,
    provider_name: &str,
    req: ImageGenRequest,
) -> Result<ImageGenResponse, LlmError> {
    let clean_base = base_url.trim_end_matches('/');
    let target_url = if clean_base.is_empty() {
        "https:\u{2F}\u{2F}api.openai.com/v1/images/generations".to_string()
    } else if clean_base.ends_with("/images/generations") {
        clean_base.to_string()
    } else {
        format!("{}/images/generations", clean_base)
    };

    for attempt in 1..=3 {
        let api_key = key_pool.next_key(provider_name)?;

        let mut payload = serde_json::json!({
            "prompt": req.prompt,
            "model": req.model.clone().unwrap_or_else(|| "dall-e-3".to_string()),
            "response_format": req.response_format.clone().unwrap_or_else(|| "b64_json".to_string()),
        });

        if let Some(n) = req.n {
            payload["n"] = serde_json::json!(n);
        }
        if let Some(ref q) = req.quality {
            payload["quality"] = serde_json::json!(q);
        }
        if let Some(ref s) = req.size {
            payload["size"] = serde_json::json!(s);
        }
        if let Some(ref st) = req.style {
            payload["style"] = serde_json::json!(st);
        }

        let req_builder = client
            .post(&target_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload);

        let response = match req_builder.send().await {
            Ok(resp) => resp,
            Err(err) => {
                key_pool.record_failure(provider_name, &api_key, 500, None);
                if attempt == 3 {
                    return Err(LlmError::Network(err));
                }
                continue;
            }
        };

        let status = response.status();
        if status.is_success() {
            key_pool.record_success(provider_name, &api_key);
            let res_json: serde_json::Value = response.json().await?;
            let created = res_json.get("created").and_then(|v| v.as_u64()).unwrap_or(0);
            let mut images = Vec::new();

            if let Some(arr) = res_json.get("data").and_then(|v| v.as_array()) {
                for item in arr {
                    let b64 = item
                        .get("b64_json")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let url = item
                        .get("url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let revised = item
                        .get("revised_prompt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    images.push(GeneratedImageItem {
                        b64_json: b64,
                        url,
                        revised_prompt: revised,
                    });
                }
            }

            return Ok(ImageGenResponse { created, images });
        }

        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        let error_body = response.text().await.unwrap_or_default();

        if status.as_u16() == 429 || status.is_server_error() {
            key_pool.record_failure(provider_name, &api_key, status.as_u16(), retry_after);
            if attempt == 3 {
                return Err(LlmError::ProviderError {
                    status: status.as_u16(),
                    message: error_body,
                });
            }
            continue;
        }

        key_pool.record_failure(provider_name, &api_key, status.as_u16(), None);
        return Err(LlmError::ProviderError {
            status: status.as_u16(),
            message: error_body,
        });
    }

    Err(LlmError::ProviderError {
        status: 500,
        message: "Image generation retries exhausted".to_string(),
    })
}
