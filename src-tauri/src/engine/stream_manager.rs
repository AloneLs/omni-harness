use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Notify, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionStatus {
    Running,
    Paused,
    Interrupted,
    Completed,
    Failed,
}

#[derive(Clone)]
pub struct SessionHandle {
    pub chat_id: String,
    pub is_cancelled: Arc<AtomicBool>,
    pub cancel_notify: Arc<Notify>,
    pub status: Arc<RwLock<SessionStatus>>,
    pub intervention_tx: mpsc::UnboundedSender<String>,
    pub intervention_rx: Arc<Mutex<mpsc::UnboundedReceiver<String>>>,
}

impl SessionHandle {
    pub fn new(chat_id: impl Into<String>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            chat_id: chat_id.into(),
            is_cancelled: Arc::new(AtomicBool::new(false)),
            cancel_notify: Arc::new(Notify::new()),
            status: Arc::new(RwLock::new(SessionStatus::Running)),
            intervention_tx: tx,
            intervention_rx: Arc::new(Mutex::new(rx)),
        }
    }

    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
        self.cancel_notify.notify_waiters();
    }

    pub fn is_active(&self) -> bool {
        !self.is_cancelled.load(Ordering::SeqCst)
    }

    pub async fn check_intervention(&self) -> Option<String> {
        let mut rx = self.intervention_rx.lock().await;
        match rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(_) => None,
        }
    }

    pub async fn set_status(&self, new_status: SessionStatus) {
        let mut s = self.status.write().await;
        *s = new_status;
    }

    pub async fn get_status(&self) -> SessionStatus {
        *self.status.read().await
    }
}

#[derive(Clone, Default)]
pub struct StreamManager {
    sessions: Arc<RwLock<HashMap<String, SessionHandle>>>,
}

impl StreamManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_session(&self, chat_id: &str) -> SessionHandle {
        let handle = SessionHandle::new(chat_id);
        let mut map = self.sessions.write().await;
        if let Some(prev) = map.insert(chat_id.to_string(), handle.clone()) {
            if prev.is_active() {
                prev.cancel();
            }
        }
        handle
    }

    pub async fn get_session(&self, chat_id: &str) -> Option<SessionHandle> {
        let map = self.sessions.read().await;
        map.get(chat_id).cloned()
    }

    pub async fn cancel_session(&self, chat_id: &str) -> bool {
        let map = self.sessions.read().await;
        if let Some(session) = map.get(chat_id) {
            session.cancel();
            session.set_status(SessionStatus::Interrupted).await;
            true
        } else {
            false
        }
    }

    pub async fn inject_intervention(&self, chat_id: &str, message: String) -> Result<(), String> {
        let map = self.sessions.read().await;
        if let Some(session) = map.get(chat_id) {
            session
                .intervention_tx
                .send(message)
                .map_err(|e| format!("Failed to deliver intervention: {}", e))?;
            session.set_status(SessionStatus::Interrupted).await;
            Ok(())
        } else {
            Err(format!("Session '{}' not found", chat_id))
        }
    }

    pub async fn remove_session(&self, chat_id: &str) {
        let mut map = self.sessions.write().await;
        map.remove(chat_id);
    }

    pub async fn remove_session_if_current(&self, chat_id: &str, handle: &SessionHandle) {
        let mut map = self.sessions.write().await;
        let is_same = map
            .get(chat_id)
            .map(|cur| Arc::ptr_eq(&cur.is_cancelled, &handle.is_cancelled))
            .unwrap_or(false);
        if is_same {
            map.remove(chat_id);
        }
    }

    pub async fn list_active_sessions(&self) -> Vec<String> {
        let map = self.sessions.read().await;
        map.keys().cloned().collect()
    }

    pub async fn has_running_sessions(&self) -> bool {
        let map = self.sessions.read().await;
        for s in map.values() {
            if s.is_active() && *s.status.read().await == SessionStatus::Running {
                return true;
            }
        }
        false
    }
}
