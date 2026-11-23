use crate::telegram::formatting::{
    escape_html, split_telegram_html, telegram_html_to_plain_text, TELEGRAM_SAFE_LIMIT,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

#[derive(Debug, Clone)]
pub struct TelegramApiError {
    pub method: String,
    pub status_code: u16,
    pub message: String,
    pub retryable: bool,
}

impl std::fmt::Display for TelegramApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Telegram {}: {}", self.method, self.message)
    }
}

#[derive(Clone)]
pub struct TelegramClient {
    token: String,
    base_url: String,
    file_base_url: String,
    http: reqwest::Client,
    slots: Arc<Semaphore>,
}

impl TelegramClient {
    pub fn new(token: impl Into<String>) -> Result<Self, String> {
        Self::new_with_proxy(token, None)
    }

    pub fn new_with_proxy(token: impl Into<String>, proxy_url: Option<&str>) -> Result<Self, String> {
        let clean = token.into().trim().to_string();
        if clean.is_empty() {
            return Err("Telegram bot token is required".to_string());
        }
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .tcp_keepalive(Duration::from_secs(15))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(8);
        let eff_proxy = proxy_url
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(crate::resolve_windows_system_proxy);
        if let Some(pu) = eff_proxy {
            let proxy = reqwest::Proxy::all(&pu)
                .map_err(|e| format!("Invalid proxy URL: {}", e))?
                .no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1,::1,*.local,*.internal"));
            builder = builder.proxy(proxy);
        } else {
            builder = builder.local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
        }
        let http = builder.build().map_err(|e| e.to_string())?;
        Ok(Self {
            base_url: format!("https://api.telegram.org/bot{}", clean),
            file_base_url: format!("https://api.telegram.org/file/bot{}", clean),
            token: clean,
            http,
            slots: Arc::new(Semaphore::new(12)),
        })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub async fn request(
        &self,
        method: &str,
        payload: Value,
        max_attempts: usize,
    ) -> Result<Value, TelegramApiError> {
        let replay_safe = matches!(
            method,
            "getUpdates"
                | "getMe"
                | "getFile"
                | "deleteWebhook"
                | "setWebhook"
                | "setMyCommands"
                | "editMessageText"
                | "editMessageReplyMarkup"
                | "sendChatAction"
                | "answerCallbackQuery"
                | "deleteMessage"
        );
        let limit = max_attempts.clamp(1, 4);
        let req_timeout = if method == "getUpdates" {
            Duration::from_secs(40)
        } else {
            Duration::from_secs(30)
        };
        let mut last_err: Option<TelegramApiError> = None;

        for attempt in 0..limit {
            let permit = self.slots.acquire().await.map_err(|_| TelegramApiError {
                method: method.to_string(),
                status_code: 0,
                message: "Client closed".to_string(),
                retryable: false,
            })?;
            let resp = self
                .http
                .post(format!("{}/{}", self.base_url, method))
                .json(&payload)
                .timeout(req_timeout)
                .send()
                .await;
            drop(permit);

            match resp {
                Ok(r) => {
                    let status = r.status().as_u16();
                    let body: Value = r.json().await.unwrap_or(Value::Null);
                    if status == 200 && body["ok"].as_bool() == Some(true) {
                        return Ok(body["result"].clone());
                    }
                    let retry_after = body["parameters"]["retry_after"].as_u64().unwrap_or(0);
                    let desc = body["description"]
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("HTTP {}", status));
                    let err = TelegramApiError {
                        method: method.to_string(),
                        status_code: status,
                        message: desc,
                        retryable: status == 429 || status >= 500,
                    };
                    if status == 429 && retry_after > 0 && attempt + 1 < limit {
                        tokio::time::sleep(Duration::from_secs(retry_after.min(15))).await;
                        last_err = Some(err);
                        continue;
                    }
                    if status >= 500 && replay_safe && attempt + 1 < limit {
                        tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
                        last_err = Some(err);
                        continue;
                    }
                    return Err(err);
                }
                Err(e) => {
                    let mut detail = e.to_string();
                    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(&e);
                    while let Some(s) = src {
                        detail.push_str(&format!(" -> {}", s));
                        src = s.source();
                    }
                    let err = TelegramApiError {
                        method: method.to_string(),
                        status_code: 0,
                        message: detail,
                        retryable: true,
                    };
                    if !replay_safe || attempt + 1 >= limit {
                        return Err(err);
                    }
                    last_err = Some(err);
                    tokio::time::sleep(Duration::from_millis(400 * (1 << attempt))).await;
                }
            }
        }
        Err(last_err.unwrap_or_else(|| TelegramApiError {
            method: method.to_string(),
            status_code: 0,
            message: "Request failed".to_string(),
            retryable: false,
        }))
    }

    pub async fn get_me(&self) -> Result<Value, TelegramApiError> {
        let res = self.request("getMe", json!({}), 1).await?;
        if res["id"].as_i64().is_none() {
            return Err(TelegramApiError {
                method: "getMe".to_string(),
                status_code: 400,
                message: "Invalid bot identity".to_string(),
                retryable: false,
            });
        }
        Ok(res)
    }

    pub async fn delete_webhook(&self) -> Result<(), TelegramApiError> {
        let _ = self
            .request("deleteWebhook", json!({ "drop_pending_updates": false }), 1)
            .await?;
        Ok(())
    }

    pub async fn set_my_commands(&self, commands: Value) {
        let _ = self
            .request("setMyCommands", json!({ "commands": commands }), 1)
            .await;
    }

    pub async fn send_html(
        &self,
        chat_id: i64,
        html_text: &str,
        reply_markup: Option<&Value>,
    ) -> Result<Vec<i64>, TelegramApiError> {
        let chunks = split_telegram_html(html_text, TELEGRAM_SAFE_LIMIT);
        let total = chunks.len();
        let mut ids = Vec::new();
        for (idx, chunk) in chunks.into_iter().enumerate() {
            let mut data = json!({
                "chat_id": chat_id,
                "text": chunk,
                "parse_mode": "HTML",
                "disable_web_page_preview": true,
            });
            if let Some(rm) = reply_markup {
                if idx + 1 == total {
                    data["reply_markup"] = rm.clone();
                }
            }
            let res = match self.request("sendMessage", data.clone(), 4).await {
                Ok(v) => v,
                Err(err) if is_html_parse_error(&err) => {
                    data["text"] = json!(escape_html(&telegram_html_to_plain_text(&chunk)));
                    self.request("sendMessage", data, 1).await?
                }
                Err(err) => return Err(err),
            };
            if let Some(mid) = res["message_id"].as_i64() {
                ids.push(mid);
            }
        }
        Ok(ids)
    }

    pub async fn edit_html(
        &self,
        chat_id: i64,
        message_id: i64,
        html_text: &str,
        reply_markup: Option<&Value>,
    ) -> Result<(), TelegramApiError> {
        let chunks = split_telegram_html(html_text, TELEGRAM_SAFE_LIMIT);
        if chunks.len() != 1 {
            return Err(TelegramApiError {
                method: "editMessageText".to_string(),
                status_code: 400,
                message: "message exceeds the single-message edit limit".to_string(),
                retryable: false,
            });
        }
        let mut data = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": chunks[0],
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        });
        if let Some(rm) = reply_markup {
            data["reply_markup"] = rm.clone();
        }
        match self.request("editMessageText", data.clone(), 2).await {
            Ok(_) => Ok(()),
            Err(err) if err.message.to_ascii_lowercase().contains("message is not modified") => Ok(()),
            Err(err) if is_html_parse_error(&err) => {
                data["text"] = json!(escape_html(&telegram_html_to_plain_text(&chunks[0])));
                self.request("editMessageText", data, 1).await.map(|_| ())
            }
            Err(err) => Err(err),
        }
    }

    pub async fn delete_message(&self, chat_id: i64, message_id: i64) -> bool {
        if message_id <= 0 {
            return false;
        }
        self.request(
            "deleteMessage",
            json!({ "chat_id": chat_id, "message_id": message_id }),
            1,
        )
        .await
        .is_ok()
    }

    pub async fn send_chat_action(&self, chat_id: i64, action: &str) {
        let _ = self
            .request(
                "sendChatAction",
                json!({ "chat_id": chat_id, "action": action }),
                1,
            )
            .await;
    }

    pub async fn answer_callback(&self, callback_query_id: &str, text: Option<&str>) {
        let mut data = json!({ "callback_query_id": callback_query_id });
        if let Some(t) = text {
            let short: String = t.chars().take(180).collect();
            data["text"] = json!(short);
        }
        let _ = self.request("answerCallbackQuery", data, 1).await;
    }

    pub async fn download_file(
        &self,
        file_id: &str,
        destination_dir: &Path,
        fallback_name: &str,
        max_bytes: usize,
    ) -> Result<PathBuf, String> {
        let info = self
            .request("getFile", json!({ "file_id": file_id }), 2)
            .await
            .map_err(|e| e.to_string())?;
        let remote_path = info["file_path"]
            .as_str()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Telegram getFile returned no file_path".to_string())?;
        if let Some(declared) = info["file_size"].as_u64() {
            if declared as usize > max_bytes {
                return Err(format!(
                    "Файл слишком большой. Максимальный размер — {} МБ.",
                    max_bytes / (1024 * 1024)
                ));
            }
        }

        std::fs::create_dir_all(destination_dir).map_err(|e| e.to_string())?;
        let ext = Path::new(remote_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let clean_name: String = fallback_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let final_name = if Path::new(&clean_name).extension().is_none() && !ext.is_empty() {
            format!("{}.{}", clean_name, ext)
        } else {
            clean_name
        };
        let dest = destination_dir.join(final_name);

        let url = format!("{}/{}", self.file_base_url, remote_path);
        let resp = self
            .http
            .get(&url)
            .timeout(Duration::from_secs(300))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("Failed to download file: HTTP {}", resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() > max_bytes {
            return Err(format!(
                "Файл слишком большой. Максимальный размер — {} МБ.",
                max_bytes / (1024 * 1024)
            ));
        }
        std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
        Ok(dest)
    }

    pub async fn send_media_with_caption(
        &self,
        chat_id: i64,
        method: &str,
        field: &str,
        source: &str,
        caption: Option<&str>,
    ) -> Result<(), String> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            let mut payload = json!({ "chat_id": chat_id });
            payload[field] = json!(trimmed);
            if let Some(cap) = caption.filter(|c| !c.trim().is_empty()) {
                payload["caption"] = json!(cap);
            }
            return self.request(method, payload, 2).await.map(|_| ()).map_err(|e| e.to_string());
        }
        let local_str = if let Some(rest) = trimmed.strip_prefix("file:///") {
            rest.replace('/', "\\")
        } else if let Some(rest) = trimmed.strip_prefix("file://") {
            rest.to_string()
        } else {
            trimmed.to_string()
        };
        let path = PathBuf::from(&local_str);
        if !path.is_file() {
            return Err(format!("Media file not found: {}", local_str));
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();

        let max_allowed = if field == "photo" { 20 * 1024 * 1024 } else { 50 * 1024 * 1024 };
        if bytes.len() > max_allowed {
            return Err(format!(
                "Размер файла '{}' ({:.1} МБ) превыш��ет лимит Telegram Bot API ({} МБ).",
                fname,
                bytes.len() as f64 / (1024.0 * 1024.0),
                max_allowed / (1024 * 1024)
            ));
        }

        let mut last_err = String::new();
        for attempt in 0..3 {
            let part = reqwest::multipart::Part::bytes(bytes.clone()).file_name(fname.clone());
            let mut form = reqwest::multipart::Form::new()
                .text("chat_id", chat_id.to_string())
                .part(field.to_string(), part);
            if let Some(cap) = caption.filter(|c| !c.trim().is_empty()) {
                form = form.text("caption", cap.to_string());
            }
            let url = format!("{}/{}", self.base_url, method);
            let resp = self
                .http
                .post(&url)
                .multipart(form)
                .timeout(Duration::from_secs(600))
                .send()
                .await;

            match resp {
                Ok(r) => {
                    if r.status().is_success() {
                        return Ok(());
                    }
                    let txt = r.text().await.unwrap_or_default();
                    last_err = format!("Telegram {} error: {}", method, txt);
                    if txt.contains("can't parse") || txt.contains("file is too big") || txt.contains("Bad Request") {
                        return Err(last_err);
                    }
                }
                Err(e) => {
                    last_err = format!("Telegram {} network error (attempt {}): {}", method, attempt + 1, e);
                }
            }
            tokio::time::sleep(Duration::from_millis(1500 * (attempt as u64 + 1))).await;
        }

        Err(last_err)
    }

    pub async fn send_media(
        &self,
        chat_id: i64,
        method: &str,
        field: &str,
        source: &str,
    ) -> Result<(), String> {
        self.send_media_with_caption(chat_id, method, field, source, None).await
    }

    pub async fn send_document(&self, chat_id: i64, source: &str, caption: Option<&str>) -> Result<(), String> {
        self.send_media_with_caption(chat_id, "sendDocument", "document", source, caption).await
    }

    pub async fn send_photo(&self, chat_id: i64, source: &str) -> Result<(), String> {
        self.send_media_with_caption(chat_id, "sendPhoto", "photo", source, None).await
    }

    pub async fn send_audio(&self, chat_id: i64, source: &str) -> Result<(), String> {
        let lower = source.to_ascii_lowercase();
        if lower.ends_with(".ogg") || lower.ends_with(".opus") {
            self.send_media_with_caption(chat_id, "sendVoice", "voice", source, None).await
        } else {
            self.send_media_with_caption(chat_id, "sendAudio", "audio", source, None).await
        }
    }

    pub async fn send_video(&self, chat_id: i64, source: &str) -> Result<(), String> {
        self.send_media_with_caption(chat_id, "sendVideo", "video", source, None).await
    }
}

fn is_html_parse_error(err: &TelegramApiError) -> bool {
    if err.status_code != 400 {
        return false;
    }
    let lower = err.message.to_ascii_lowercase();
    lower.contains("can't parse entities")
        || lower.contains("cant parse entities")
        || lower.contains("unsupported start tag")
        || lower.contains("wrong html")
}
