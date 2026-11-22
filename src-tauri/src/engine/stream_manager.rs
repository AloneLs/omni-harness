use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex, Notify, RwLock};

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
    cancel_tx: Arc<watch::Sender<bool>>,
    cancel_rx: watch::Receiver<bool>,
    pub status: Arc<RwLock<SessionStatus>>,
    pub intervention_tx: mpsc::Sender<String>,
    pub intervention_rx: Arc<Mutex<mpsc::Receiver<String>>>,
}

impl SessionHandle {
    pub fn new(chat_id: impl Into<String>) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let (cancel_tx, cancel_rx) = watch::channel(false);
        Self {
            chat_id: chat_id.into(),
            is_cancelled: Arc::new(AtomicBool::new(false)),
            cancel_notify: Arc::new(Notify::new()),
            cancel_tx: Arc::new(cancel_tx),
            cancel_rx,
            status: Arc::new(RwLock::new(SessionStatus::Running)),
            intervention_tx: tx,
            intervention_rx: Arc::new(Mutex::new(rx)),
        }
    }

    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::SeqCst);
        let _ = self.cancel_tx.send(true);
        self.cancel_notify.notify_waiters();
    }

    pub fn cancel_receiver(&self) -> watch::Receiver<bool> {
        self.cancel_rx.clone()
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled.load(Ordering::SeqCst) {
            return;
        }
        let mut rx = self.cancel_rx.clone();
        loop {
            if *rx.borrow() {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
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
    active_count: Arc<AtomicUsize>,
}

impl StreamManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            active_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn recount_locked(map: &HashMap<String, SessionHandle>, counter: &AtomicUsize) {
        let cnt = map.values().filter(|s| s.is_active()).count();
        counter.store(cnt, Ordering::SeqCst);
    }

    pub async fn create_session(&self, chat_id: &str) -> SessionHandle {
        let handle = SessionHandle::new(chat_id);
        let prev = {
            let mut map = self.sessions.write().await;
            let old = map.insert(chat_id.to_string(), handle.clone());
            if let Some(ref p) = old {
                if p.is_active() {
                    p.cancel();
                }
            }
            Self::recount_locked(&map, &self.active_count);
            old
        };
        if let Some(p) = prev {
            p.set_status(SessionStatus::Interrupted).await;
        }
        handle
    }

    pub async fn get_session(&self, chat_id: &str) -> Option<SessionHandle> {
        let map = self.sessions.read().await;
        map.get(chat_id).cloned()
    }

    pub async fn cancel_session(&self, chat_id: &str) -> bool {
        let session = {
            let map = self.sessions.read().await;
            let found = map.get(chat_id).cloned();
            if let Some(ref s) = found {
                s.cancel();
                Self::recount_locked(&map, &self.active_count);
            }
            found
        };
        if let Some(s) = session {
            s.set_status(SessionStatus::Interrupted).await;
            true
        } else {
            false
        }
    }

    pub async fn inject_intervention(&self, chat_id: &str, message: String) -> Result<(), String> {
        let session = {
            let map = self.sessions.read().await;
            map.get(chat_id).cloned()
        };
        if let Some(session) = session {
            session
                .intervention_tx
                .try_send(message)
                .map_err(|e| format!("Failed to deliver intervention: {}", e))?;
            Ok(())
        } else {
            Err(format!("Session '{}' not found", chat_id))
        }
    }

    pub async fn remove_session(&self, chat_id: &str) {
        let mut map = self.sessions.write().await;
        map.remove(chat_id);
        Self::recount_locked(&map, &self.active_count);
    }

    pub async fn remove_session_if_current(&self, chat_id: &str, handle: &SessionHandle) {
        let mut map = self.sessions.write().await;
        let is_same = map
            .get(chat_id)
            .map(|cur| Arc::ptr_eq(&cur.is_cancelled, &handle.is_cancelled))
            .unwrap_or(false);
        if is_same {
            map.remove(chat_id);
            Self::recount_locked(&map, &self.active_count);
        }
    }

    pub async fn list_active_sessions(&self) -> Vec<String> {
        let map = self.sessions.read().await;
        map.keys().cloned().collect()
    }

    pub fn has_running_sessions_sync(&self) -> bool {
        self.active_count.load(Ordering::SeqCst) > 0
    }

    pub async fn has_running_sessions(&self) -> bool {
        let sessions: Vec<SessionHandle> = {
            let map = self.sessions.read().await;
            map.values().cloned().collect()
        };
        for s in sessions {
            if s.is_active() && *s.status.read().await == SessionStatus::Running {
                return true;
            }
        }
        false
    }
}
