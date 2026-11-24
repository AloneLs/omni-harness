use serde::Serialize;
use thiserror::Error;

pub fn format_reqwest_error(err: &reqwest::Error) -> String {
    use std::error::Error as _;
    let mut kinds: Vec<&'static str> = Vec::new();
    if err.is_timeout() {
        kinds.push("timeout");
    }
    if err.is_connect() {
        kinds.push("connect");
    }
    if err.is_body() {
        kinds.push("body");
    }
    if err.is_decode() {
        kinds.push("decode");
    }
    if err.is_redirect() {
        kinds.push("redirect");
    }
    if err.is_request() && kinds.is_empty() {
        kinds.push("request");
    }
    let mut causes: Vec<String> = Vec::new();
    let mut cur: Option<&(dyn std::error::Error + 'static)> = err.source();
    while let Some(src) = cur {
        let s = src.to_string();
        if !s.is_empty() && !causes.iter().any(|c| c == &s) {
            causes.push(s);
        }
        cur = src.source();
    }
    let kind_str = if kinds.is_empty() {
        "network".to_string()
    } else {
        kinds.join("/")
    };
    let url_part = err.url().map(|u| format!(" ({})", u)).unwrap_or_default();
    if causes.is_empty() {
        format!("[{}]{}: {}", kind_str, url_part, err)
    } else {
        format!("[{}]{}: {}", kind_str, url_part, causes.join(" -> "))
    }
}

pub fn extract_api_error_message(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "empty response body".to_string();
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(err_obj) = val.get("error") {
            if let Some(s) = err_obj.as_str() {
                let desc = val.get("error_description").and_then(|v| v.as_str()).unwrap_or("");
                return if desc.is_empty() {
                    s.to_string()
                } else {
                    format!("{}: {}", s, desc)
                };
            }
            let msg = err_obj.get("message").and_then(|v| v.as_str()).unwrap_or("").trim();
            let reason = err_obj
                .get("details")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.iter().find_map(|item| item.get("reason").and_then(|r| r.as_str())))
                .or_else(|| err_obj.get("status").and_then(|v| v.as_str()));
            if !msg.is_empty() {
                return match reason {
                    Some(r) if !r.is_empty() => format!("{} ({})", msg, r),
                    _ => msg.to_string(),
                };
            }
        }
        if let Some(msg) = val.get("message").and_then(|v| v.as_str()) {
            if !msg.trim().is_empty() {
                return msg.trim().to_string();
            }
        }
    }
    trimmed.chars().take(300).collect()
}

#[derive(Error, Debug, Serialize, Clone)]
#[serde(tag = "type", content = "message")]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("IO error: {0}")]
    Io(String),
    #[error("Keyring error: {0}")]
    Keyring(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Snapshot error: {0}")]
    Snapshot(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Validation error: {0}")]
    Validation(String),
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err.to_string())
    }
}

impl From<keyring::Error> for AppError {
    fn from(err: keyring::Error) -> Self {
        AppError::Keyring(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Serialization(err.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Network(format_reqwest_error(&err))
    }
}
