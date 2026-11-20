use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueuedAttachment {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedPrompt {
    pub id: String,
    pub chat_id: String,
    pub text: String,
    pub created_at: i64,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub attachments: Vec<QueuedAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tg_chat_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tg_message_id: Option<i64>,
}

impl QueuedPrompt {
    pub fn new(chat_id: impl Into<String>, text: impl Into<String>, source: &str) -> Self {
        Self {
            id: format!("q-{}", uuid::Uuid::new_v4().simple()),
            chat_id: chat_id.into(),
            text: text.into(),
            created_at: chrono::Utc::now().timestamp_millis(),
            source: source.to_string(),
            priority: "normal".to_string(),
            attachments: Vec::new(),
            tg_chat_id: None,
            tg_message_id: None,
        }
    }
}

pub struct PromptQueue {
    chats: Arc<RwLock<HashMap<String, Vec<QueuedPrompt>>>>,
    path: PathBuf,
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl PromptQueue {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join("prompt_queues.json");
        let mut chats: HashMap<String, Vec<QueuedPrompt>> = HashMap::new();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str::<HashMap<String, Vec<QueuedPrompt>>>(&raw) {
                chats = parsed;
            }
        }
        Self {
            chats: Arc::new(RwLock::new(chats)),
            path,
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    async fn persist(&self) {
        let _guard = self.write_lock.lock().await;
        let snapshot = {
            let guard = self.chats.read().await;
            guard.clone()
        };
        if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &self.path);
            }
        }
    }

    pub async fn list(&self, chat_id: &str) -> Vec<QueuedPrompt> {
        self.chats
            .read()
            .await
            .get(chat_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn add(&self, item: QueuedPrompt) -> Vec<QueuedPrompt> {
        let chat_id = item.chat_id.clone();
        let is_priority = item.priority == "intervention";
        let result = {
            let mut guard = self.chats.write().await;
            let bucket = guard.entry(chat_id.clone()).or_default();
            if is_priority {
                bucket.insert(0, item);
            } else {
                bucket.push(item);
            }
            bucket.clone()
        };
        self.persist().await;
        result
    }

    pub async fn remove(&self, chat_id: &str, id: &str) -> Vec<QueuedPrompt> {
        let result = {
            let mut guard = self.chats.write().await;
            let mut out = Vec::new();
            if let Some(bucket) = guard.get_mut(chat_id) {
                bucket.retain(|i| i.id != id);
                out = bucket.clone();
                if bucket.is_empty() {
                    guard.remove(chat_id);
                }
            }
            out
        };
        self.persist().await;
        result
    }

    pub async fn take_next(&self, chat_id: &str) -> Option<QueuedPrompt> {
        let taken = {
            let mut guard = self.chats.write().await;
            match guard.get_mut(chat_id) {
                Some(bucket) if !bucket.is_empty() => {
                    let item = bucket.remove(0);
                    if bucket.is_empty() {
                        guard.remove(chat_id);
                    }
                    Some(item)
                }
                _ => None,
            }
        };
        if taken.is_some() {
            self.persist().await;
        }
        taken
    }

    pub async fn take_specific(&self, chat_id: &str, id: &str) -> Option<QueuedPrompt> {
        let taken = {
            let mut guard = self.chats.write().await;
            match guard.get_mut(chat_id) {
                Some(bucket) => {
                    let pos = bucket.iter().position(|i| i.id == id);
                    let item = pos.map(|p| bucket.remove(p));
                    if bucket.is_empty() {
                        guard.remove(chat_id);
                    }
                    item
                }
                None => None,
            }
        };
        if taken.is_some() {
            self.persist().await;
        }
        taken
    }

    pub async fn clear(&self, chat_id: &str) -> Vec<QueuedPrompt> {
        {
            let mut guard = self.chats.write().await;
            guard.remove(chat_id);
        }
        self.persist().await;
        Vec::new()
    }

    pub async fn peek(&self, chat_id: &str) -> Option<QueuedPrompt> {
        self.chats
            .read()
            .await
            .get(chat_id)
            .and_then(|b| b.first().cloned())
    }

    pub async fn is_empty(&self, chat_id: &str) -> bool {
        self.chats
            .read()
            .await
            .get(chat_id)
            .map(|b| b.is_empty())
            .unwrap_or(true)
    }
}