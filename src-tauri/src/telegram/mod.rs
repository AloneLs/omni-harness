pub mod client;
pub mod formatting;
pub mod media_split;
pub mod tool_stream;
pub mod ui_menu;

pub use media_split::*;
pub use ui_menu::*;

use client::TelegramClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadJobInfo {
    pub upload_id: String,
    pub chat_id: String,
    pub path: String,
    pub kind: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct UploadJobRuntime {
    info: UploadJobInfo,
    cancelled: Arc<AtomicBool>,
    cancel_notify: Arc<tokio::sync::Notify>,
}

struct UploadManagerState {
    active_targets: RwLock<HashMap<String, (TelegramClient, i64)>>,
    jobs: RwLock<HashMap<String, Arc<tokio::sync::Mutex<UploadJobRuntime>>>>,
    chat_locks: RwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

static TG_UPLOAD_MGR: std::sync::OnceLock<UploadManagerState> = std::sync::OnceLock::new();

fn upload_mgr() -> &'static UploadManagerState {
    TG_UPLOAD_MGR.get_or_init(|| UploadManagerState {
        active_targets: RwLock::new(HashMap::new()),
        jobs: RwLock::new(HashMap::new()),
        chat_locks: RwLock::new(HashMap::new()),
    })
}

async fn get_chat_lock(chat_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    {
        let r = upload_mgr().chat_locks.read().await;
        if let Some(l) = r.get(chat_id) {
            return l.clone();
        }
    }
    let mut w = upload_mgr().chat_locks.write().await;
    w.entry(chat_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

pub struct TelegramUploadManager;

impl TelegramUploadManager {
    pub async fn register_active_chat(chat_id: &str, client: TelegramClient, tg_chat_id: i64) {
        let mut w = upload_mgr().active_targets.write().await;
        w.insert(chat_id.to_string(), (client, tg_chat_id));
    }

    pub async fn enqueue_upload(
        chat_id: &str,
        path: &str,
        kind: &str,
        caption: Option<String>,
    ) -> Result<UploadJobInfo, String> {
        let (client, tg_chat_id) = {
            let r = upload_mgr().active_targets.read().await;
            r.get(chat_id)
                .cloned()
                .ok_or_else(|| "No active Telegram target for this chat session.".to_string())?
        };
        Ok(Self::enqueue_with_client(client, tg_chat_id, chat_id, path, kind, caption).await)
    }

    pub async fn enqueue_with_client(
        client: TelegramClient,
        tg_chat_id: i64,
        chat_id: &str,
        path: &str,
        kind: &str,
        caption: Option<String>,
    ) -> UploadJobInfo {
        let upload_id = uuid::Uuid::new_v4().to_string();
        let resolved_kind = if kind.trim().is_empty() || kind.eq_ignore_ascii_case("auto") {
            let lower = path.to_ascii_lowercase();
            if [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|e| lower.ends_with(e)) {
                "photo"
            } else if [".mp3", ".wav", ".ogg", ".m4a", ".opus", ".flac"].iter().any(|e| lower.ends_with(e)) {
                "audio"
            } else if [".mp4", ".webm", ".mov", ".mkv"].iter().any(|e| lower.ends_with(e)) {
                "video"
            } else {
                "document"
            }
        } else {
            kind
        };

        let info = UploadJobInfo {
            upload_id: upload_id.clone(),
            chat_id: chat_id.to_string(),
            path: path.to_string(),
            kind: resolved_kind.to_string(),
            status: "queued".to_string(),
            error: None,
        };
        let runtime = Arc::new(tokio::sync::Mutex::new(UploadJobRuntime {
            info: info.clone(),
            cancelled: Arc::new(AtomicBool::new(false)),
            cancel_notify: Arc::new(tokio::sync::Notify::new()),
        }));
        {
            let mut w = upload_mgr().jobs.write().await;
            w.insert(upload_id.clone(), runtime.clone());
        }

        let path_owned = path.to_string();
        let kind_owned = resolved_kind.to_string();
        tokio::spawn(async move {
            let (cancelled, cancel_notify) = {
                let mut g = runtime.lock().await;
                if g.cancelled.load(Ordering::SeqCst) {
                    g.info.status = "cancelled".to_string();
                    return;
                }
                g.info.status = "uploading".to_string();
                (g.cancelled.clone(), g.cancel_notify.clone())
            };

            let send_fut = async {
                match kind_owned.as_str() {
                    "photo" => client.send_photo(tg_chat_id, &path_owned).await,
                    "audio" => client.send_audio(tg_chat_id, &path_owned).await,
                    "video" => client.send_video(tg_chat_id, &path_owned).await,
                    _ => client.send_document(tg_chat_id, &path_owned, caption.as_deref()).await,
                }
            };

            let outcome = tokio::select! {
                biased;
                _ = cancel_notify.notified() => Err("Cancelled by agent".to_string()),
                res = send_fut => res,
            };

            let mut g = runtime.lock().await;
            if cancelled.load(Ordering::SeqCst) {
                g.info.status = "cancelled".to_string();
            } else {
                match outcome {
                    Ok(()) => {
                        g.info.status = "sent".to_string();
                    }
                    Err(e) => {
                        g.info.status = "failed".to_string();
                        g.info.error = Some(e.clone());
                        let display_name = std::path::Path::new(&path_owned)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&path_owned);
                        let warn = format!("⚠️ Не удалось отправить файл <code>{}</code>: {}", crate::telegram::formatting::escape_html(display_name), crate::telegram::formatting::escape_html(&e));
                        let _ = client.send_html(tg_chat_id, &warn, None).await;
                    }
                }
            }
        });

        info
    }

    pub async fn get_status(chat_id: &str, upload_id: Option<&str>) -> Vec<UploadJobInfo> {
        let jobs_snap: Vec<Arc<tokio::sync::Mutex<UploadJobRuntime>>> = {
            let r = upload_mgr().jobs.read().await;
            if let Some(uid) = upload_id {
                r.get(uid).cloned().into_iter().collect()
            } else {
                r.values().cloned().collect()
            }
        };
        let mut out = Vec::new();
        for j in jobs_snap {
            let g = j.lock().await;
            if g.info.chat_id == chat_id {
                out.push(g.info.clone());
            }
        }
        out
    }

    pub async fn cancel_upload(upload_id: &str) -> Result<UploadJobInfo, String> {
        let job = {
            let r = upload_mgr().jobs.read().await;
            r.get(upload_id)
                .cloned()
                .ok_or_else(|| format!("Upload job '{}' not found", upload_id))?
        };
        let mut g = job.lock().await;
        g.cancelled.store(true, Ordering::SeqCst);
        g.cancel_notify.notify_waiters();
        g.info.status = "cancelled".to_string();
        Ok(g.info.clone())
    }
}
use formatting::{compact_text, escape_html, format_user_error, markdown_to_telegram_html};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;
use tokio::sync::RwLock;
use tool_stream::LiveToolStream;

const CHAT_PAGE_SIZE: usize = 8;
const PROVIDER_PAGE_SIZE: usize = 8;
const MODEL_PAGE_SIZE: usize = 10;
const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedModelCap {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_ctx")]
    pub context_window: u32,
    #[serde(default = "default_out")]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub supports_reasoning: bool,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default = "default_true")]
    pub supports_tools: bool,
}

fn default_ctx() -> u32 {
    128_000
}
fn default_out() -> u32 {
    16_384
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_protocol: Option<String>,
    #[serde(default)]
    pub proxy_enabled: Option<bool>,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub available_models: Vec<String>,
    #[serde(default)]
    pub model_capabilities: Option<HashMap<String, SyncedModelCap>>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersSnapshotFile {
    pub providers: Vec<SyncedProviderConfig>,
    pub active_provider: String,
    pub active_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedProject {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedTgModel {
    pub id: String,
    pub display_name: String,
    pub provider_id: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub supports_reasoning: bool,
    pub supports_vision: bool,
    pub supports_tools: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedTgProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub api_protocol: Option<String>,
    pub proxy_enabled: bool,
    pub proxy_url: Option<String>,
    pub models: Vec<ResolvedTgModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedChatEntry {
    pub chat_id: String,
    pub name: String,
    pub paired_at: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramConfigFile {
    pub token: String,
    pub paired_chats: Vec<PairedChatEntry>,
    pub notify_on_complete: bool,
    pub notify_on_approval: bool,
    pub notify_on_error: bool,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub chat_sessions: HashMap<i64, String>,
    #[serde(default)]
    pub chat_providers: HashMap<i64, String>,
    #[serde(default)]
    pub chat_models: HashMap<i64, String>,
    #[serde(default)]
    pub chat_reasoning: HashMap<i64, String>,
    #[serde(default)]
    pub chat_projects: HashMap<i64, String>,
}

impl Default for TelegramConfigFile {
    fn default() -> Self {
        Self {
            token: String::new(),
            paired_chats: Vec::new(),
            notify_on_complete: true,
            notify_on_approval: true,
            notify_on_error: true,
            auto_start: false,
            proxy_url: String::new(),
            offset: 0,
            chat_sessions: HashMap::new(),
            chat_providers: HashMap::new(),
            chat_models: HashMap::new(),
            chat_reasoning: HashMap::new(),
            chat_projects: HashMap::new(),
        }
    }
}

#[derive(Clone)]
pub struct TelegramRuntimeContext {
    pub app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    pub app_state: Arc<crate::state::AppState>,
    pub stream_mgr: Arc<crate::engine::StreamManager>,
    pub tool_dispatcher: Arc<crate::tools::ToolDispatcher>,
    pub key_pool: Arc<crate::llm::KeyPool>,
    pub caps_registry: Arc<crate::llm::CapabilityRegistry>,
    pub http_client: reqwest::Client,
    pub speech_engine: Arc<crate::speech::SpeechEngine>,
    pub prompt_queue: Arc<crate::engine::prompt_queue::PromptQueue>,
    pub active_contracts: Arc<
        RwLock<
            HashMap<
                String,
                Arc<tokio::sync::Mutex<Option<crate::engine::task_contract::TaskContract>>>,
            >,
        >,
    >,
}

#[derive(Debug, Clone, Default)]
struct TelegramChatSession {
    active_chat_id: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
    project_path: Option<String>,
    reasoning_effort: String,
    pending_rename: bool,
}

pub struct TelegramBotManager {
    token: Arc<RwLock<Option<String>>>,
    allowed_chat_ids: Arc<RwLock<HashSet<i64>>>,
    pairing_code: Arc<RwLock<Option<String>>>,
    is_running: Arc<AtomicBool>,
    media_collector: MediaGroupCollector,
    config: Arc<RwLock<TelegramConfigFile>>,
    config_path: Arc<RwLock<Option<PathBuf>>>,
    providers_snapshot: Arc<RwLock<ProvidersSnapshotFile>>,
    providers_path: Arc<RwLock<Option<PathBuf>>>,
    projects_snapshot: Arc<RwLock<Vec<SyncedProject>>>,
    projects_path: Arc<RwLock<Option<PathBuf>>>,
    runtime_ctx: Arc<RwLock<Option<TelegramRuntimeContext>>>,
    chat_sessions: Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    callback_choices: Arc<RwLock<HashMap<String, Vec<String>>>>,
    processed_updates: Arc<RwLock<VecDeque<i64>>>,
}

impl Default for TelegramBotManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TelegramBotManager {
    pub fn new() -> Self {
        Self {
            token: Arc::new(RwLock::new(None)),
            allowed_chat_ids: Arc::new(RwLock::new(HashSet::new())),
            pairing_code: Arc::new(RwLock::new(None)),
            is_running: Arc::new(AtomicBool::new(false)),
            media_collector: MediaGroupCollector::new(),
            config: Arc::new(RwLock::new(TelegramConfigFile::default())),
            config_path: Arc::new(RwLock::new(None)),
            providers_snapshot: Arc::new(RwLock::new(ProvidersSnapshotFile::default())),
            providers_path: Arc::new(RwLock::new(None)),
            projects_snapshot: Arc::new(RwLock::new(Vec::new())),
            projects_path: Arc::new(RwLock::new(None)),
            runtime_ctx: Arc::new(RwLock::new(None)),
            chat_sessions: Arc::new(RwLock::new(HashMap::new())),
            callback_choices: Arc::new(RwLock::new(HashMap::new())),
            processed_updates: Arc::new(RwLock::new(VecDeque::with_capacity(320))),
        }
    }

    pub async fn set_runtime_context(&self, ctx: TelegramRuntimeContext) {
        let mut lock = self.runtime_ctx.write().await;
        *lock = Some(ctx);
    }

    pub async fn load_from_disk(&self, data_dir: &Path) {
        let path = data_dir.join("telegram_config.json");
        {
            let mut cp = self.config_path.write().await;
            *cp = Some(path.clone());
        }
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<TelegramConfigFile>(&raw) {
                if !cfg.token.trim().is_empty() {
                    self.set_token(cfg.token.trim().to_string()).await;
                }
                {
                    let mut allowed = self.allowed_chat_ids.write().await;
                    allowed.clear();
                    for entry in &cfg.paired_chats {
                        if let Ok(id) = entry.chat_id.trim().parse::<i64>() {
                            allowed.insert(id);
                        }
                    }
                }
                {
                    let mut sessions = self.chat_sessions.write().await;
                    for (&tg_id, cid) in &cfg.chat_sessions {
                        sessions.entry(tg_id).or_default().active_chat_id = Some(cid.clone());
                    }
                    for (&tg_id, pid) in &cfg.chat_providers {
                        sessions.entry(tg_id).or_default().provider_id = Some(pid.clone());
                    }
                    for (&tg_id, mid) in &cfg.chat_models {
                        sessions.entry(tg_id).or_default().model_id = Some(mid.clone());
                    }
                    for (&tg_id, r) in &cfg.chat_reasoning {
                        sessions.entry(tg_id).or_default().reasoning_effort = r.clone();
                    }
                    for (&tg_id, pp) in &cfg.chat_projects {
                        sessions.entry(tg_id).or_default().project_path = Some(pp.clone());
                    }
                }
                let mut c = self.config.write().await;
                *c = cfg;
            }
        }

        let p_path = data_dir.join("providers_snapshot.json");
        {
            let mut pp = self.providers_path.write().await;
            *pp = Some(p_path.clone());
        }
        if let Ok(raw) = std::fs::read_to_string(&p_path) {
            if let Ok(snap) = serde_json::from_str::<ProvidersSnapshotFile>(&raw) {
                let mut lock = self.providers_snapshot.write().await;
                *lock = snap;
            }
        }

        let ws_path = data_dir.join("workspaces_snapshot.json");
        {
            let mut wp = self.projects_path.write().await;
            *wp = Some(ws_path.clone());
        }
        if let Ok(raw) = std::fs::read_to_string(&ws_path) {
            if let Ok(list) = serde_json::from_str::<Vec<SyncedProject>>(&raw) {
                let mut lock = self.projects_snapshot.write().await;
                *lock = list;
            }
        }
    }

    pub async fn sync_projects(&self, projects: Vec<SyncedProject>) -> Result<(), String> {
        {
            let mut lock = self.projects_snapshot.write().await;
            *lock = projects.clone();
        }
        let path_opt = self.projects_path.read().await.clone();
        if let Some(path) = path_opt {
            let data = serde_json::to_vec_pretty(&projects).map_err(|e| e.to_string())?;
            crate::db::atomic_write(&path, &data).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn sync_providers(&self, snap: ProvidersSnapshotFile) -> Result<(), String> {
        let (changed_prov, changed_model) = {
            let mut lock = self.providers_snapshot.write().await;
            let cp = if !snap.active_provider.is_empty() && snap.active_provider != lock.active_provider {
                Some(snap.active_provider.clone())
            } else {
                None
            };
            let cm = if !snap.active_model.is_empty() && snap.active_model != lock.active_model {
                Some(snap.active_model.clone())
            } else {
                None
            };
            *lock = snap.clone();
            (cp, cm)
        };
        if changed_prov.is_some() || changed_model.is_some() {
            {
                let mut sess = self.chat_sessions.write().await;
                for entry in sess.values_mut() {
                    if let Some(ref cp) = changed_prov {
                        entry.provider_id = Some(cp.clone());
                    }
                    if let Some(ref cm) = changed_model {
                        entry.model_id = Some(cm.clone());
                    }
                }
            }
            persist_session_snapshot(&self.config, &self.config_path, &self.chat_sessions, None).await;
        }
        let path_opt = self.providers_path.read().await.clone();
        if let Some(path) = path_opt {
            let data = serde_json::to_vec_pretty(&snap).map_err(|e| e.to_string())?;
            crate::db::atomic_write(&path, &data).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn save_config(&self, mut cfg: TelegramConfigFile) -> Result<(), String> {
        let clean_token = cfg.token.trim().to_string();
        if !clean_token.is_empty() {
            self.set_token(clean_token.clone()).await;
        } else {
            let mut t = self.token.write().await;
            *t = None;
        }
        {
            let prev = self.config.read().await;
            let mut merged = prev.paired_chats.clone();
            for entry in &cfg.paired_chats {
                if !merged.iter().any(|m| m.chat_id == entry.chat_id) {
                    merged.push(entry.clone());
                }
            }
            cfg.paired_chats = merged;
            cfg.offset = prev.offset;
            cfg.chat_sessions = prev.chat_sessions.clone();
            cfg.chat_providers = prev.chat_providers.clone();
            cfg.chat_models = prev.chat_models.clone();
            cfg.chat_reasoning = prev.chat_reasoning.clone();
            cfg.chat_projects = prev.chat_projects.clone();
        }
        {
            let mut allowed = self.allowed_chat_ids.write().await;
            for entry in &cfg.paired_chats {
                if let Ok(id) = entry.chat_id.trim().parse::<i64>() {
                    allowed.insert(id);
                }
            }
        }
        {
            let mut c = self.config.write().await;
            *c = cfg;
        }
        self.persist_config().await
    }

    pub async fn remove_paired_chat(&self, chat_id: &str) -> Result<(), String> {
        let trimmed = chat_id.trim();
        if let Ok(id) = trimmed.parse::<i64>() {
            self.allowed_chat_ids.write().await.remove(&id);
        }
        {
            let mut c = self.config.write().await;
            c.paired_chats.retain(|e| e.chat_id.trim() != trimmed);
        }
        self.persist_config().await
    }

    async fn persist_config(&self) -> Result<(), String> {
        let path_opt = self.config_path.read().await.clone();
        if let Some(path) = path_opt {
            let cfg = self.config.read().await.clone();
            let data = serde_json::to_vec_pretty(&cfg).map_err(|e| e.to_string())?;
            crate::db::atomic_write(&path, &data).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn get_config_snapshot(&self) -> TelegramConfigFile {
        self.config.read().await.clone()
    }

    pub async fn set_token(&self, token: String) {
        let mut t = self.token.write().await;
        *t = Some(token);
    }

    pub async fn get_token(&self) -> Option<String> {
        let t = self.token.read().await;
        t.clone()
    }

    pub fn is_running_sync(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    pub async fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    pub async fn add_allowed_chat(&self, chat_id: i64) {
        let mut set = self.allowed_chat_ids.write().await;
        set.insert(chat_id);
    }

    pub async fn remove_allowed_chat(&self, chat_id: i64) {
        let mut set = self.allowed_chat_ids.write().await;
        set.remove(&chat_id);
    }

    pub async fn get_allowed_chats(&self) -> Vec<i64> {
        let set = self.allowed_chat_ids.read().await;
        set.iter().copied().collect()
    }

    pub async fn generate_pairing_code(&self) -> String {
        use rand::Rng;
        let num: u32 = rand::thread_rng().gen_range(100_000..1_000_000);
        let code = format!("{:06}", num);
        let mut lock = self.pairing_code.write().await;
        *lock = Some(code.clone());
        code
    }

    pub async fn get_or_generate_pairing_code(&self) -> String {
        {
            let lock = self.pairing_code.read().await;
            if let Some(code) = &*lock {
                return code.clone();
            }
        }
        self.generate_pairing_code().await
    }

    pub async fn verify_pairing_code(&self, code: &str, chat_id: i64) -> bool {
        let mut lock = self.pairing_code.write().await;
        if let Some(expected) = &*lock {
            if expected.trim() == code.trim() {
                *lock = None;
                drop(lock);
                self.add_allowed_chat(chat_id).await;
                return true;
            }
        }
        false
    }

    pub async fn is_chat_authorized(&self, chat_id: i64) -> bool {
        let set = self.allowed_chat_ids.read().await;
        set.contains(&chat_id)
    }

    pub async fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }

    pub async fn start(&self) -> Result<(), String> {
        if self.is_running.load(Ordering::SeqCst) {
            return Ok(());
        }
        let bot_token = self
            .get_token()
            .await
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Bot token not configured".to_string())?;

        let tg_client = TelegramClient::new(&bot_token)?;
        let identity = tg_client.get_me().await.map_err(|e| e.to_string())?;
        tg_client.delete_webhook().await.map_err(|e| e.to_string())?;
        let bot_id = identity["id"].as_i64().unwrap_or(0).to_string();
        let bot_username = identity["username"].as_str().unwrap_or("").to_string();

        tg_client
            .set_my_commands(json!([
                { "command": "start", "description": "Открыть бота" },
                { "command": "menu", "description": "Главное меню" },
                { "command": "new", "description": "Новая сессия" },
                { "command": "chats", "description": "Список сессий" },
                { "command": "providers", "description": "Выбор провайдера" },
                { "command": "models", "description": "Выбор модели" },
                { "command": "settings", "description": "Настройки чата" },
                { "command": "status", "description": "Статус задачи" },
                { "command": "stop", "description": "Остановить задачу" },
                { "command": "help", "description": "Справка" }
            ]))
            .await;

        self.is_running.store(true, Ordering::SeqCst);

        let running_flag = self.is_running.clone();
        let allowed_chats = self.allowed_chat_ids.clone();
        let pairing_code_lock = self.pairing_code.clone();
        let config_arc = self.config.clone();
        let config_path_arc = self.config_path.clone();
        let providers_snap_arc = self.providers_snapshot.clone();
        let projects_snap_arc = self.projects_snapshot.clone();
        let runtime_ctx_arc = self.runtime_ctx.clone();
        let sessions_arc = self.chat_sessions.clone();
        let choices_arc = self.callback_choices.clone();
        let processed_arc = self.processed_updates.clone();
        let _media_collector = self.media_collector.clone();

        tokio::spawn(async move {
            let startup_ts = chrono::Utc::now().timestamp();
            let mut offset = config_arc.read().await.offset;
            if let Ok(drain_val) = tg_client
                .request(
                    "getUpdates",
                    json!({
                        "offset": -1,
                        "timeout": 0,
                        "allowed_updates": ["message", "channel_post", "callback_query"]
                    }),
                    1,
                )
                .await
            {
                if let Some(arr) = drain_val.as_array() {
                    if let Some(last_id) = arr.iter().filter_map(|u| u["update_id"].as_i64()).max() {
                        if last_id + 1 > offset {
                            offset = last_id + 1;
                            persist_session_snapshot(&config_arc, &config_path_arc, &sessions_arc, Some(offset)).await;
                        }
                    }
                }
            }

            while running_flag.load(Ordering::SeqCst) {
                let prev_offset = offset;
                let res = tg_client
                    .request(
                        "getUpdates",
                        json!({
                            "offset": offset,
                            "timeout": 25,
                            "allowed_updates": ["message", "channel_post", "callback_query"]
                        }),
                        2,
                    )
                    .await;

                match res {
                    Ok(updates_val) => {
                        if let Some(updates) = updates_val.as_array() {
                            for u in updates {
                                let update_id = u["update_id"].as_i64().unwrap_or(0);
                                if update_id >= offset {
                                    offset = update_id + 1;
                                }
                                if !claim_update(&processed_arc, update_id).await {
                                    continue;
                                }

                                if let Some(msg) = u.get("message").or_else(|| u.get("channel_post")) {
                                    let msg_date = msg["date"].as_i64().unwrap_or(startup_ts);
                                    if msg_date < startup_ts - 5 {
                                        continue;
                                    }
                                    let chat_id = msg["chat"]["id"].as_i64().unwrap_or(0);
                                    if chat_id == 0 {
                                        continue;
                                    }
                                    let raw_text = msg["text"]
                                        .as_str()
                                        .or_else(|| msg["caption"].as_str())
                                        .unwrap_or("")
                                        .trim()
                                        .to_string();

                                    let chat_type = msg["chat"]["type"].as_str().unwrap_or("");
                                    let sender_id = msg["from"]["id"].as_i64().unwrap_or(0);
                                    let authorized = {
                                        let allowed = allowed_chats.read().await;
                                        if chat_type == "private" || chat_type == "channel" {
                                            allowed.contains(&chat_id)
                                        } else {
                                            allowed.contains(&chat_id) && allowed.contains(&sender_id)
                                        }
                                    };
                                    if !authorized {
                                        let ctx_opt_unauth = runtime_ctx_arc.read().await.clone();
                                        handle_unauthorized_message(
                                            &tg_client,
                                            chat_id,
                                            msg,
                                            &raw_text,
                                            &allowed_chats,
                                            &pairing_code_lock,
                                            &config_arc,
                                            &config_path_arc,
                                            ctx_opt_unauth.as_ref(),
                                        )
                                        .await;
                                        continue;
                                    }

                                    let (text, accepted) =
                                        filter_group_message(msg, &raw_text, &bot_id, &bot_username);
                                    if !accepted {
                                        continue;
                                    }

                                    let ctx_opt = runtime_ctx_arc.read().await.clone();
                                    let Some(ctx) = ctx_opt else {
                                        let _ = tg_client
                                            .send_html(chat_id, "Runtime context is not ready yet.", None)
                                            .await;
                                        continue;
                                    };

                                    dispatch_tg_message(
                                        &tg_client,
                                        chat_id,
                                        msg.clone(),
                                        text,
                                        &ctx,
                                        &providers_snap_arc,
                                        &projects_snap_arc,
                                        &sessions_arc,
                                        &choices_arc,
                                        &config_arc,
                                        &config_path_arc,
                                    )
                                    .await;
                                } else if let Some(cb) = u.get("callback_query") {
                                    let cb_id = cb["id"].as_str().unwrap_or("");
                                    let cb_data = cb["data"].as_str().unwrap_or("");
                                    let msg_id = cb["message"]["message_id"].as_i64().unwrap_or(0);
                                    let chat_id = cb["message"]["chat"]["id"].as_i64().unwrap_or(0);

                                    let chat_type = cb["message"]["chat"]["type"].as_str().unwrap_or("");
                                    let sender_id = cb["from"]["id"].as_i64().unwrap_or(0);
                                    let authorized = {
                                        let allowed = allowed_chats.read().await;
                                        if chat_type == "private" || chat_type == "channel" {
                                            allowed.contains(&chat_id)
                                        } else {
                                            allowed.contains(&chat_id) && allowed.contains(&sender_id)
                                        }
                                    };
                                    if !authorized || chat_id == 0 || msg_id == 0 {
                                        tg_client
                                            .answer_callback(cb_id, Some("Доступ запрещён"))
                                            .await;
                                        continue;
                                    }

                                    let ctx_opt = runtime_ctx_arc.read().await.clone();
                                    let Some(ctx) = ctx_opt else {
                                        tg_client.answer_callback(cb_id, None).await;
                                        continue;
                                    };

                                    handle_callback_query(
                                        &tg_client,
                                        chat_id,
                                        msg_id,
                                        cb_id,
                                        cb_data,
                                        &ctx,
                                        &providers_snap_arc,
                                        &projects_snap_arc,
                                        &sessions_arc,
                                        &choices_arc,
                                        &pairing_code_lock,
                                        &config_arc,
                                        &config_path_arc,
                                    )
                                    .await;
                                }
                            }
                            if offset > 0 {
                                persist_session_snapshot(
                                    &config_arc,
                                    &config_path_arc,
                                    &sessions_arc,
                                    Some(offset),
                                )
                                .await;
                            }
                        }
                    }
                    Err(err) => {
                        if !err.retryable {
                            running_flag.store(false, Ordering::SeqCst);
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }

                if offset > prev_offset {
                    persist_session_snapshot(&config_arc, &config_path_arc, &sessions_arc, Some(offset)).await;
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        });

        Ok(())
    }
}

async fn claim_update(processed: &Arc<RwLock<VecDeque<i64>>>, update_id: i64) -> bool {
    if update_id <= 0 {
        return true;
    }
    let mut queue = processed.write().await;
    if queue.contains(&update_id) {
        return false;
    }
    queue.push_back(update_id);
    while queue.len() > 300 {
        queue.pop_front();
    }
    true
}

async fn persist_session_snapshot(
    config_arc: &Arc<RwLock<TelegramConfigFile>>,
    config_path_arc: &Arc<RwLock<Option<PathBuf>>>,
    sessions_arc: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    new_offset: Option<i64>,
) {
    let sessions = sessions_arc.read().await.clone();
    let mut cfg = config_arc.write().await;
    if let Some(off) = new_offset {
        cfg.offset = off;
    }
    cfg.chat_sessions.clear();
    cfg.chat_providers.clear();
    cfg.chat_models.clear();
    cfg.chat_reasoning.clear();
    cfg.chat_projects.clear();
    for (tg_id, sess) in sessions {
        if let Some(cid) = sess.active_chat_id {
            cfg.chat_sessions.insert(tg_id, cid);
        }
        if let Some(pid) = sess.provider_id {
            cfg.chat_providers.insert(tg_id, pid);
        }
        if let Some(mid) = sess.model_id {
            cfg.chat_models.insert(tg_id, mid);
        }
        if !sess.reasoning_effort.is_empty() {
            cfg.chat_reasoning.insert(tg_id, sess.reasoning_effort);
        }
        if let Some(pp) = sess.project_path {
            cfg.chat_projects.insert(tg_id, pp);
        }
    }
    if let Some(path) = config_path_arc.read().await.as_ref() {
        if let Ok(data) = serde_json::to_vec_pretty(&*cfg) {
            let _ = crate::db::atomic_write(path, &data);
        }
    }
}

fn filter_group_message(
    msg: &Value,
    raw_text: &str,
    bot_id: &str,
    bot_username: &str,
) -> (String, bool) {
    let chat_type = msg["chat"]["type"].as_str().unwrap_or("");
    if chat_type != "group" && chat_type != "supergroup" {
        return (raw_text.to_string(), true);
    }
    let is_cmd = raw_text.starts_with('/');
    let mention_tag = if bot_username.is_empty() {
        String::new()
    } else {
        format!("@{}", bot_username.to_ascii_lowercase())
    };
    let has_mention = !mention_tag.is_empty() && raw_text.to_ascii_lowercase().contains(&mention_tag);
    let reply_from = &msg["reply_to_message"]["from"];
    let replied_to_bot = reply_from["is_bot"].as_bool() == Some(true)
        && (reply_from["id"].as_i64().map(|v| v.to_string()) == Some(bot_id.to_string())
            || (!bot_username.is_empty()
                && reply_from["username"]
                    .as_str()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(bot_username)));

    if !is_cmd && !has_mention && !replied_to_bot {
        return (String::new(), false);
    }
    let cleaned = if has_mention {
        raw_text.replace(&format!("@{}", bot_username), "").trim().to_string()
    } else {
        raw_text.to_string()
    };
    (cleaned, true)
}

async fn handle_unauthorized_message(
    tg_client: &TelegramClient,
    chat_id: i64,
    msg: &Value,
    text: &str,
    allowed_chats: &Arc<RwLock<HashSet<i64>>>,
    pairing_code_lock: &Arc<RwLock<Option<String>>>,
    config_arc: &Arc<RwLock<TelegramConfigFile>>,
    config_path_arc: &Arc<RwLock<Option<PathBuf>>>,
    ctx_opt: Option<&TelegramRuntimeContext>,
) {
    let entered_code = if let Some(rest) = text.strip_prefix("/pair") {
        rest.trim()
    } else if let Some(rest) = text.strip_prefix("/start") {
        let arg = rest.trim();
        arg.strip_prefix("pair_").unwrap_or(arg)
    } else {
        text
    };

    let mut pair_lock = pairing_code_lock.write().await;
    let mut paired = false;
    if let Some(expected) = &*pair_lock {
        if !entered_code.is_empty() && expected.trim() == entered_code {
            *pair_lock = None;
            paired = true;
        }
    }
    drop(pair_lock);

    if paired {
        {
            let mut allowed = allowed_chats.write().await;
            allowed.insert(chat_id);
            if let Some(uid) = msg["from"]["id"].as_i64().filter(|&id| id > 0) {
                allowed.insert(uid);
            }
        }
        let sender_name = msg["chat"]["title"]
            .as_str()
            .or_else(|| msg["chat"]["first_name"].as_str())
            .or_else(|| msg["chat"]["username"].as_str())
            .unwrap_or("User")
            .to_string();
        let now_str = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();
        let entry = PairedChatEntry {
            chat_id: chat_id.to_string(),
            name: sender_name,
            paired_at: now_str.clone(),
            role: "admin".to_string(),
        };
        {
            let mut cfg = config_arc.write().await;
            if !cfg.paired_chats.iter().any(|c| c.chat_id == entry.chat_id) {
                cfg.paired_chats.push(entry);
            }
            if let Some(uid) = msg["from"]["id"].as_i64().filter(|&id| id > 0 && id != chat_id) {
                let uid_str = uid.to_string();
                if !cfg.paired_chats.iter().any(|c| c.chat_id == uid_str) {
                    let from_name = msg["from"]["first_name"]
                        .as_str()
                        .or_else(|| msg["from"]["username"].as_str())
                        .unwrap_or("Admin")
                        .to_string();
                    cfg.paired_chats.push(PairedChatEntry {
                        chat_id: uid_str,
                        name: from_name,
                        paired_at: now_str,
                        role: "admin".to_string(),
                    });
                }
            }
            if let Some(path) = config_path_arc.read().await.as_ref() {
                if let Ok(data) = serde_json::to_vec_pretty(&*cfg) {
                    let _ = crate::db::atomic_write(path, &data);
                }
            }
        }
        if let Some(ctx) = ctx_opt {
            if let Some(ref app) = *ctx.app_handle.read().await {
                let _ = app.emit("telegram-config-updated", json!({ "chatId": chat_id.to_string() }));
            }
        }
        let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
        let _ = tg_client
            .send_html(
                chat_id,
                "<b>Бот подключён к этому чату.</b>\n\nДоступ сохранён. Отправьте задачу или откройте /menu.",
                rm.as_ref(),
            )
            .await;
    } else {
        let _ = tg_client
            .send_html(
                chat_id,
                "Доступ запрещён. Отправьте <code>/pair &lt;код&gt;</code> из настроек OmniHarness → Telegram.",
                None,
            )
            .await;
    }
}

async fn store_choices(
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    tg_chat_id: i64,
    kind: &str,
    items: Vec<String>,
) -> String {
    let token = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let key = format!("{}:{}:{}", tg_chat_id, kind, token);
    let mut map = choices_arc.write().await;
    if map.len() > 200 {
        map.clear();
    }
    map.insert(key, items);
    token
}

async fn resolve_choice(
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    tg_chat_id: i64,
    kind: &str,
    token: &str,
    index: usize,
) -> Option<String> {
    let key = format!("{}:{}:{}", tg_chat_id, kind, token);
    let map = choices_arc.read().await;
    map.get(&key).and_then(|v| v.get(index)).cloned()
}

async fn list_available_providers(
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
) -> (Vec<ResolvedTgProvider>, String, String) {
    let snap = providers_snap.read().await.clone();
    if !snap.providers.is_empty() {
        let mut out = Vec::new();
        for p in snap.providers.into_iter().filter(|p| p.enabled) {
            let mut models = Vec::new();
            for mid in &p.available_models {
                let cap = p.model_capabilities.as_ref().and_then(|m| m.get(mid));
                let fallback_caps = crate::llm::capabilities::ModelCapabilities::default_for_model(mid);
                let md = crate::llm::models_dev::lookup(mid);
                let ctx_w = cap
                    .map(|c| c.context_window)
                    .filter(|&w| w > 0)
                    .or_else(|| md.as_ref().map(|m| m.context_window).filter(|&w| w > 0))
                    .unwrap_or(fallback_caps.context_window);
                let out_t = cap
                    .map(|c| c.max_output_tokens)
                    .filter(|&w| w > 0)
                    .or_else(|| md.as_ref().map(|m| m.max_output_tokens).filter(|&w| w > 0))
                    .unwrap_or(fallback_caps.max_output_tokens);
                models.push(ResolvedTgModel {
                    id: mid.clone(),
                    display_name: cap
                        .map(|c| c.name.clone())
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| mid.clone()),
                    provider_id: p.id.clone(),
                    context_window: ctx_w,
                    max_output_tokens: out_t,
                    supports_reasoning: cap
                        .map(|c| c.supports_reasoning)
                        .or_else(|| md.as_ref().and_then(|m| m.reasoning))
                        .unwrap_or(fallback_caps.supports_reasoning),
                    supports_vision: cap
                        .map(|c| c.supports_vision)
                        .or_else(|| md.as_ref().and_then(|m| m.vision))
                        .unwrap_or(fallback_caps.supports_vision),
                    supports_tools: cap
                        .map(|c| c.supports_tools)
                        .or_else(|| md.as_ref().and_then(|m| m.tool_call))
                        .unwrap_or(fallback_caps.supports_tools),
                });
            }
            if !models.is_empty() || !p.default_model.is_empty() {
                if models.is_empty() && !p.default_model.is_empty() {
                    let fallback_caps = crate::llm::capabilities::ModelCapabilities::default_for_model(&p.default_model);
                    models.push(ResolvedTgModel {
                        id: p.default_model.clone(),
                        display_name: p.default_model.clone(),
                        provider_id: p.id.clone(),
                        context_window: fallback_caps.context_window,
                        max_output_tokens: fallback_caps.max_output_tokens,
                        supports_reasoning: fallback_caps.supports_reasoning,
                        supports_vision: fallback_caps.supports_vision,
                        supports_tools: fallback_caps.supports_tools,
                    });
                }
                out.push(ResolvedTgProvider {
                    id: p.id,
                    name: p.name,
                    base_url: p.base_url,
                    api_key: p.api_key,
                    api_protocol: p.api_protocol,
                    proxy_enabled: p.proxy_enabled.unwrap_or(false),
                    proxy_url: p.proxy_url,
                    models,
                });
            }
        }
        return (out, snap.active_provider, snap.active_model);
    }

    let db_providers = ctx.app_state.db.list_providers().unwrap_or_default();
    let db_models = ctx.app_state.db.list_models(None).unwrap_or_default();
    let mut out = Vec::new();
    for p in db_providers {
        let api_key = match ctx.app_state.get_credential(&p.api_key_name).await {
            Ok(k) if !k.trim().is_empty() => k,
            _ => ctx.app_state.get_credential(&p.id).await.unwrap_or_default(),
        };
        let models: Vec<ResolvedTgModel> = db_models
            .iter()
            .filter(|m| m.provider_id == p.id)
            .map(|m| ResolvedTgModel {
                id: m.id.clone(),
                display_name: m.display_name.clone(),
                provider_id: p.id.clone(),
                context_window: m.context_window as u32,
                max_output_tokens: m.max_output_tokens as u32,
                supports_reasoning: m.supports_reasoning,
                supports_vision: m.supports_vision,
                supports_tools: m.supports_tools,
            })
            .collect();
        out.push(ResolvedTgProvider {
            id: p.id,
            name: p.name,
            base_url: p.base_url,
            api_key,
            api_protocol: if p.provider_type.is_empty() { None } else { Some(p.provider_type) },
            proxy_enabled: false,
            proxy_url: None,
            models,
        });
    }
    (out, String::new(), String::new())
}

async fn resolve_provider_and_model(
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    tg_chat_id: i64,
) -> Result<(ResolvedTgProvider, ResolvedTgModel), String> {
    let (providers, active_prov_id, active_model_id) =
        list_available_providers(ctx, providers_snap).await;
    if providers.is_empty() {
        return Err("В OmniHarness не настроены провайдеры или модели.".to_string());
    }

    let sess = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .cloned()
        .unwrap_or_default();

    let target_pid = sess.provider_id.as_deref().or_else(|| {
        if active_prov_id.is_empty() {
            None
        } else {
            Some(active_prov_id.as_str())
        }
    });

    let provider = target_pid
        .and_then(|pid| providers.iter().find(|p| p.id == pid || p.name == pid))
        .or_else(|| providers.first())
        .cloned()
        .ok_or_else(|| "Провайдер не найден.".to_string())?;

    let target_mid = sess.model_id.as_deref().or_else(|| {
        if active_model_id.is_empty() {
            None
        } else {
            Some(active_model_id.as_str())
        }
    });

    let model = target_mid
        .and_then(|mid| {
            provider
                .models
                .iter()
                .find(|m| m.id == mid || m.display_name == mid)
        })
        .or_else(|| provider.models.first())
        .cloned()
        .ok_or_else(|| "У провайдера нет моделей.".to_string())?;

    Ok((provider, model))
}

async fn ensure_active_chat(
    ctx: &TelegramRuntimeContext,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    tg_chat_id: i64,
    model_id: &str,
    initial_title: Option<&str>,
) -> Result<String, String> {
    {
        let map = sessions.read().await;
        if let Some(sess) = map.get(&tg_chat_id) {
            if let Some(cid) = &sess.active_chat_id {
                if let Ok(c) = ctx.app_state.db.get_chat(cid) {
                    if (c.title == "Telegram Chat" || c.title == "Нова�� сессия") && initial_title.is_some() {
                        let short = compact_text(initial_title.unwrap(), 42);
                        if !short.is_empty() {
                            let _ = ctx.app_state.db.update_chat_title(cid, &short);
                            emit_chat_updated(ctx, cid).await;
                        }
                    }
                    if !model_id.trim().is_empty() && c.model_id != model_id {
                        let _ = ctx.app_state.db.update_chat_model(cid, model_id);
                        emit_chat_updated(ctx, cid).await;
                    }
                    return Ok(cid.clone());
                }
            }
        }
    }

    let chat_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let title = initial_title
        .map(|s| {
            let short = compact_text(s, 42);
            if short.is_empty() {
                "Telegram Chat".to_string()
            } else {
                short
            }
        })
        .unwrap_or_else(|| "Telegram Chat".to_string());
    let project_id = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .and_then(|s| s.project_path.clone());
    let chat = crate::db::chats::Chat {
        id: chat_id.clone(),
        project_id,
        title,
        model_id: model_id.to_string(),
        system_prompt: String::new(),
        is_pinned: false,
        created_at: now.clone(),
        updated_at: now,
    };
    ctx.app_state.db.create_chat(&chat).map_err(|e| e.to_string())?;
    {
        let mut map = sessions.write().await;
        map.entry(tg_chat_id).or_default().active_chat_id = Some(chat_id.clone());
    }
    emit_chat_updated(ctx, &chat_id).await;
    Ok(chat_id)
}

async fn create_new_telegram_chat(
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    tg_chat_id: i64,
) -> Result<String, String> {
    let existing_empty = {
        let map = sessions.read().await;
        if let Some(sess) = map.get(&tg_chat_id) {
            if let Some(ref cid) = sess.active_chat_id {
                let msgs = ctx.app_state.db.list_messages(cid).unwrap_or_default();
                if msgs.is_empty() {
                    Some(cid.clone())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some(cid) = existing_empty {
        return Ok(cid);
    }

    let (_, model) = resolve_provider_and_model(ctx, providers_snap, sessions, tg_chat_id).await?;
    let chat_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let project_id = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .and_then(|s| s.project_path.clone());
    let chat = crate::db::chats::Chat {
        id: chat_id.clone(),
        project_id,
        title: "Новая сессия".to_string(),
        model_id: model.id,
        system_prompt: String::new(),
        is_pinned: false,
        created_at: now.clone(),
        updated_at: now,
    };
    ctx.app_state.db.create_chat(&chat).map_err(|e| e.to_string())?;
    {
        let mut map = sessions.write().await;
        let entry = map.entry(tg_chat_id).or_default();
        entry.active_chat_id = Some(chat_id.clone());
        entry.pending_rename = false;
    }
    emit_chat_updated(ctx, &chat_id).await;
    Ok(chat_id)
}


static TG_PENDING_INPUT: std::sync::OnceLock<tokio::sync::Mutex<std::collections::HashMap<i64, String>>> = std::sync::OnceLock::new();

fn tg_pending_input_map() -> &'static tokio::sync::Mutex<std::collections::HashMap<i64, String>> {
    TG_PENDING_INPUT.get_or_init(|| tokio::sync::Mutex::new(std::collections::HashMap::new()))
}

const TG_MERGE_WINDOW_MS: u64 = 1800;

struct TgMergeSlot {
    generation: u64,
    text: String,
    msg: Value,
}

static TG_MERGE: std::sync::OnceLock<tokio::sync::Mutex<std::collections::HashMap<i64, TgMergeSlot>>> =
    std::sync::OnceLock::new();

fn tg_merge_map() -> &'static tokio::sync::Mutex<std::collections::HashMap<i64, TgMergeSlot>> {
    TG_MERGE.get_or_init(|| tokio::sync::Mutex::new(std::collections::HashMap::new()))
}

fn tg_queue_keyboard(token: &str) -> Value {
    json!({
        "inline_keyboard": [[
            { "text": "▶️ Отправить сейчас", "callback_data": format!("tg:q:send:{}", token) },
            { "text": "🗑 Удалить", "callback_data": format!("tg:q:del:{}", token) }
        ]]
    })
}

fn tg_queue_preview_text(items: &[crate::engine::prompt_queue::QueuedPrompt]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = format!("<b>Очередь задач ({})</b>\n", items.len());
    for (idx, it) in items.iter().take(10).enumerate() {
        let short: String = it.text.chars().take(90).collect();
        let tail = if it.text.chars().count() > 90 { "…" } else { "" };
        out.push_str(&format!(
            "{}. <code>{}</code> <i>({})</i>\n",
            idx + 1,
            escape_html(short.trim()),
            escape_html(&it.source),
        ));
        if !tail.is_empty() {
            out.push_str(&format!("   {}\n", tail));
        }
    }
    if items.len() > 10 {
        out.push_str(&format!("… ещё {}\n", items.len() - 10));
    }
    out
}

async fn emit_prompt_queue_updated(
    ctx: &TelegramRuntimeContext,
    chat_id: &str,
    items: &[crate::engine::prompt_queue::QueuedPrompt],
) {
    if let Some(app) = ctx.app_handle.read().await.as_ref() {
        let _ = app.emit(
            "prompt-queue-updated",
            json!({ "chatId": chat_id, "items": items }),
        );
    }
}

async fn emit_chat_updated(ctx: &TelegramRuntimeContext, chat_id: &str) {
    if let Some(app) = ctx.app_handle.read().await.as_ref() {
        let _ = app.emit("telegram-chat-updated", json!({ "chatId": chat_id }));
    }
}

async fn emit_settings_updated(
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    reasoning_effort: Option<&str>,
    chat_id: Option<&str>,
) {
    if provider_id.is_some() || model_id.is_some() {
        let mut snap = providers_snap.write().await;
        if let Some(pid) = provider_id {
            snap.active_provider = pid.to_string();
        }
        if let Some(mid) = model_id {
            snap.active_model = mid.to_string();
        }
    }
    let app_opt = ctx.app_handle.read().await.clone();
    if let Some(app) = app_opt {
        let _ = app.emit(
            "telegram-settings-updated",
            json!({
                "providerId": provider_id,
                "modelId": model_id,
                "reasoningEffort": reasoning_effort,
                "chatId": chat_id,
            }),
        );
    }
}

async fn render_projects_page(
    tg_client: &TelegramClient,
    projects_snap: &Arc<RwLock<Vec<SyncedProject>>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    tg_chat_id: i64,
    edit_msg_id: Option<i64>,
) {
    let projects = projects_snap.read().await.clone();
    let active_pp = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .and_then(|s| s.project_path.clone());
    let paths: Vec<String> = projects.iter().map(|p| p.path.clone()).collect();
    let token = store_choices(choices_arc, tg_chat_id, "proj", paths).await;
    let items: Vec<(usize, String, bool)> = projects
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.name.clone(), active_pp.as_deref() == Some(p.path.as_str())))
        .collect();
    let rm = serde_json::to_value(TelegramMenuBuilder::projects_menu(&items, &token)).ok();
    let text = if projects.is_empty() {
        "<b>Проекты</b>\n\nНа ПК пока нет открытых проектов.".to_string()
    } else {
        "<b>Проекты</b>\nВыберите папку проекта для новых чатов:".to_string()
    };
    send_or_edit(tg_client, tg_chat_id, edit_msg_id, &text, rm.as_ref()).await;
}

async fn render_chat_history(
    tg_client: &TelegramClient,
    ctx: &TelegramRuntimeContext,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    tg_chat_id: i64,
    page: Option<usize>,
    edit_msg_id: Option<i64>,
) {
    let active_cid = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .and_then(|s| s.active_chat_id.clone());
    let Some(cid) = active_cid else {
        let rm = serde_json::to_value(TelegramMenuBuilder::back_menu()).ok();
        send_or_edit(tg_client, tg_chat_id, edit_msg_id, "Сессия не выбрана.", rm.as_ref()).await;
        return;
    };
    let title = ctx
        .app_state
        .db
        .get_chat(&cid)
        .map(|c| c.title)
        .unwrap_or_else(|_| "Чат".to_string());
    let msgs: Vec<_> = ctx
        .app_state
        .db
        .list_messages(&cid)
        .unwrap_or_default()
        .into_iter()
        .filter(|m| (m.role == "user" || m.role == "assistant") && !m.content.trim().is_empty())
        .collect();

    let mut turns: Vec<(Option<String>, Option<String>)> = Vec::new();
    for m in msgs {
        if m.role == "user" {
            turns.push((Some(m.content), None));
        } else if let Some(last) = turns.last_mut() {
            if last.1.is_none() {
                last.1 = Some(m.content);
            } else {
                turns.push((None, Some(m.content)));
            }
        } else {
            turns.push((None, Some(m.content)));
        }
    }

    let page_count = turns.len().max(1);
    let safe_page = match page {
        Some(p) => p.min(page_count - 1),
        None => page_count - 1,
    };

    let mut parts = vec![format!("<b>{}</b> · {}/{}", escape_html(&compact_text(&title, 48)), safe_page + 1, page_count)];
    if turns.is_empty() {
        parts.push("<i>История сообщений пуста. Отправьте сообщение, чтобы начать.</i>".to_string());
    } else {
        let (u_opt, a_opt) = &turns[safe_page];
        if let Some(u) = u_opt {
            let u_clean = compact_text(u, 1400);
            let u_html = markdown_to_telegram_html(&u_clean);
            parts.push(format!("👤 <b>Запрос:</b>\n{}", if u_html.trim().is_empty() { escape_html(&u_clean) } else { u_html }));
        }
        if let Some(a) = a_opt {
            let media_list = extract_media_attachments(a);
            let clean_a = if media_list.is_empty() {
                a.clone()
            } else {
                regex::Regex::new(r"!\[[^\]]*\]\([^)]+\)")
                    .map(|re| re.replace_all(a, "").trim().to_string())
                    .unwrap_or_else(|_| a.clone())
            };
            let mut compacted = compact_text(&clean_a, 2400);
            let fence_count = compacted.matches("```").count();
            if fence_count % 2 != 0 {
                compacted.push_str("\n```");
            }
            let a_html = markdown_to_telegram_html(&compacted);
            parts.push(format!("🤖 <b>Ответ:</b>\n{}", if a_html.trim().is_empty() { escape_html(&compacted) } else { a_html }));
        }
    }
    let rm = serde_json::to_value(TelegramMenuBuilder::history_menu(safe_page, turns.len())).ok();
    send_or_edit(tg_client, tg_chat_id, edit_msg_id, &parts.join("

"), rm.as_ref()).await;
}

async fn build_status_html(
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    tg_chat_id: i64,
) -> (String, bool, bool) {
    let sess = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .cloned()
        .unwrap_or_default();
    let (chat_label, has_chat) = match sess.active_chat_id.as_deref() {
        Some(cid) => match ctx.app_state.db.get_chat(cid) {
            Ok(c) => (format!("<b>{}</b>", escape_html(&compact_text(&c.title, 48))), true),
            Err(_) => ("Нет активной сессии".to_string(), false),
        },
        None => ("Нет активной сессии".to_string(), false),
    };
    let running = if let Some(cid) = sess.active_chat_id.as_deref() {
        ctx.stream_mgr.get_session(cid).await.is_some()
    } else {
        false
    };
    let all_active = ctx.stream_mgr.list_active_sessions().await;
    let bg_running = all_active
        .iter()
        .filter(|cid| Some(cid.as_str()) != sess.active_chat_id.as_deref())
        .count();
    let bg_suffix = if bg_running > 0 {
        format!("\n⚡ <i>Фоновых задач: {}</i>", bg_running)
    } else {
        String::new()
    };
    let reasoning = if sess.reasoning_effort.is_empty() {
        "medium"
    } else {
        sess.reasoning_effort.as_str()
    };
    let model_lines = match resolve_provider_and_model(ctx, providers_snap, sessions, tg_chat_id).await {
        Ok((p, m)) => format!(
            "Провайдер: <code>{}</code>\nМодель: <code>{}</code>\nКонтекст: <b>{}K</b> · Reasoning: <code>{}</code>",
            escape_html(&p.name),
            escape_html(&m.display_name),
            (m.context_window / 1000).max(1),
            escape_html(reasoning)
        ),
        Err(e) => format!("Модель: <i>{}</i>", escape_html(&e)),
    };
    let state_badge = if running { "● В работе" } else { "○ Ожидание" };
    let html = format!(
        "<b>OmniHarness Control Center</b>\n{}{}\n\nСессия: {}\n{}",
        state_badge, bg_suffix, chat_label, model_lines
    );
    (html, running, has_chat)
}

fn format_model_details_html(
    provider: &ResolvedTgProvider,
    model: &ResolvedTgModel,
    reasoning: &str,
) -> String {
    let mut inputs = vec!["текст"];
    if model.supports_vision {
        inputs.push("изображения");
    }
    let tools = if model.supports_tools { "да" } else { "нет" };
    let rs = if model.supports_reasoning {
        format!("да ({})", reasoning)
    } else {
        "нет".to_string()
    };
    format!(
        "<b>Модель выбрана</b>\nПровайдер: <code>{}</code>\nМодель: <code>{}</code>\nКонтекст: <b>{}K</b>\nВход: {}\nИнструменты: {} · Рассуждение: {}",
        escape_html(&provider.name),
        escape_html(&model.display_name),
        (model.context_window / 1000).max(1),
        inputs.join(" · "),
        tools,
        escape_html(&rs)
    )
}

async fn render_chats_page(
    tg_client: &TelegramClient,
    ctx: &TelegramRuntimeContext,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    tg_chat_id: i64,
    page: usize,
    edit_msg_id: Option<i64>,
) {
    let chats = ctx.app_state.db.list_chats(None).unwrap_or_default();
    let active_id = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .and_then(|s| s.active_chat_id.clone());
    let total = chats.len();
    let page_count = ((total + CHAT_PAGE_SIZE - 1) / CHAT_PAGE_SIZE).max(1);
    let safe_page = page.min(page_count - 1);
    let start = safe_page * CHAT_PAGE_SIZE;
    let slice: Vec<_> = chats.into_iter().skip(start).take(CHAT_PAGE_SIZE).collect();
    let ids: Vec<String> = slice.iter().map(|c| c.id.clone()).collect();
    let token = store_choices(choices_arc, tg_chat_id, "chat", ids).await;
    let mut items: Vec<(usize, String, bool)> = Vec::with_capacity(slice.len());
    for (i, c) in slice.iter().enumerate() {
        let is_running = ctx.stream_mgr.get_session(&c.id).await.is_some();
        let is_sel = active_id.as_deref() == Some(c.id.as_str());
        let display_title = if is_running {
            format!("▶ {}", c.title)
        } else {
            c.title.clone()
        };
        items.push((i, display_title, is_sel));
    }
    let rm = serde_json::to_value(TelegramMenuBuilder::chats_menu(
        &items,
        &token,
        safe_page,
        page_count,
    ))
    .ok();
    let text = if total == 0 {
        "<b>Сессии</b>\n\nСессий пока нет.".to_string()
    } else {
        format!(
            "<b>Сессии</b>\nСтраница {} из {} · всего {}",
            safe_page + 1,
            page_count,
            total
        )
    };
    send_or_edit(tg_client, tg_chat_id, edit_msg_id, &text, rm.as_ref()).await;
}

async fn render_providers_page(
    tg_client: &TelegramClient,
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    tg_chat_id: i64,
    page: usize,
    edit_msg_id: Option<i64>,
) {
    let (providers, active_prov_id, _) = list_available_providers(ctx, providers_snap).await;
    let sess_pid = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .and_then(|s| s.provider_id.clone());
    let active_id = sess_pid.or(if active_prov_id.is_empty() { None } else { Some(active_prov_id) });
    let total = providers.len();
    let page_count = ((total + PROVIDER_PAGE_SIZE - 1) / PROVIDER_PAGE_SIZE).max(1);
    let safe_page = page.min(page_count - 1);
    let start = safe_page * PROVIDER_PAGE_SIZE;
    let slice: Vec<_> = providers.into_iter().skip(start).take(PROVIDER_PAGE_SIZE).collect();
    let ids: Vec<String> = slice.iter().map(|p| p.id.clone()).collect();
    let token = store_choices(choices_arc, tg_chat_id, "prov", ids).await;
    let items: Vec<(usize, String, bool)> = slice
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.name.clone(), active_id.as_deref() == Some(p.id.as_str())))
        .collect();
    let rm = serde_json::to_value(TelegramMenuBuilder::providers_menu(
        &items,
        &token,
        safe_page,
        page_count,
    ))
    .ok();
    let text = if total == 0 {
        "<b>Провайдеры</b>\n\nПровайдеры не настроены.".to_string()
    } else {
        format!("<b>Провайдеры</b> · {}/{}", safe_page + 1, page_count)
    };
    send_or_edit(tg_client, tg_chat_id, edit_msg_id, &text, rm.as_ref()).await;
}

async fn render_models_page(
    tg_client: &TelegramClient,
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    tg_chat_id: i64,
    page: usize,
    edit_msg_id: Option<i64>,
) {
    let (providers, active_prov_id, active_model_id) =
        list_available_providers(ctx, providers_snap).await;
    let (sess_pid, sess_mid) = {
        let map = sessions.read().await;
        let s = map.get(&tg_chat_id);
        (
            s.and_then(|x| x.provider_id.clone()),
            s.and_then(|x| x.model_id.clone()),
        )
    };
    let pid = sess_pid.or(if active_prov_id.is_empty() { None } else { Some(active_prov_id) });
    let active_mid = sess_mid.or(if active_model_id.is_empty() { None } else { Some(active_model_id) });

    let models: Vec<ResolvedTgModel> = if let Some(ref target_pid) = pid {
        providers
            .iter()
            .find(|p| &p.id == target_pid || &p.name == target_pid)
            .map(|p| p.models.clone())
            .unwrap_or_else(|| providers.iter().flat_map(|p| p.models.clone()).collect())
    } else {
        providers.iter().flat_map(|p| p.models.clone()).collect()
    };

    let total = models.len();
    let page_count = ((total + MODEL_PAGE_SIZE - 1) / MODEL_PAGE_SIZE).max(1);
    let safe_page = page.min(page_count - 1);
    let start = safe_page * MODEL_PAGE_SIZE;
    let slice: Vec<_> = models.into_iter().skip(start).take(MODEL_PAGE_SIZE).collect();
    let ids: Vec<String> = slice
        .iter()
        .map(|m| format!("{}||{}", m.provider_id, m.id))
        .collect();
    let token = store_choices(choices_arc, tg_chat_id, "model", ids).await;
    let items: Vec<(usize, String, bool)> = slice
        .iter()
        .enumerate()
        .map(|(i, m)| {
            (
                i,
                m.display_name.clone(),
                active_mid.as_deref() == Some(m.id.as_str()),
            )
        })
        .collect();
    let rm = serde_json::to_value(TelegramMenuBuilder::models_menu(
        &items,
        &token,
        safe_page,
        page_count,
    ))
    .ok();
    let text = if total == 0 {
        "<b>Модели</b>\n\nМодели не найдены.".to_string()
    } else {
        format!("<b>Модели</b> · {}/{}", safe_page + 1, page_count)
    };
    send_or_edit(tg_client, tg_chat_id, edit_msg_id, &text, rm.as_ref()).await;
}

async fn send_or_edit(
    tg_client: &TelegramClient,
    chat_id: i64,
    edit_msg_id: Option<i64>,
    html: &str,
    reply_markup: Option<&Value>,
) {
    if let Some(mid) = edit_msg_id {
        if tg_client.edit_html(chat_id, mid, html, reply_markup).await.is_ok() {
            return;
        }
    }
    let _ = tg_client.send_html(chat_id, html, reply_markup).await;
}

#[derive(Debug, Clone)]
struct TgAttachment {
    name: String,
    path: String,
    kind: String,
}

async fn collect_tg_attachments(
    tg_client: &TelegramClient,
    msg: &Value,
    chat_id: &str,
) -> Result<Vec<TgAttachment>, String> {
    if msg.get("sticker").is_some() {
        return Err("Стикеры не поддерживаются.".to_string());
    }
    let mut items: Vec<(String, String, String)> = Vec::new();
    if let Some(doc) = msg.get("document") {
        if let Some(fid) = doc["file_id"].as_str() {
            let name = doc["file_name"].as_str().unwrap_or("document.bin").to_string();
            let mime = doc["mime_type"].as_str().unwrap_or("");
            let kind = if mime.starts_with("audio/") {
                "audio"
            } else if mime.starts_with("image/") {
                "image"
            } else {
                "file"
            };
            items.push((fid.to_string(), name, kind.to_string()));
        }
    }
    if let Some(photos) = msg.get("photo").and_then(|v| v.as_array()) {
        if let Some(best) = photos.iter().max_by_key(|p| p["file_size"].as_u64().unwrap_or(0)) {
            if let Some(fid) = best["file_id"].as_str() {
                let uid = best["file_unique_id"].as_str().unwrap_or("photo");
                items.push((
                    fid.to_string(),
                    format!("photo_{}.jpg", uid),
                    "image".to_string(),
                ));
            }
        }
    }
    for (field, fallback, kind) in [
        ("audio", "audio.mp3", "audio"),
        ("voice", "voice.ogg", "voice"),
        ("video", "video.mp4", "video"),
        ("video_note", "video_note.mp4", "video"),
    ] {
        if let Some(obj) = msg.get(field) {
            if let Some(fid) = obj["file_id"].as_str() {
                let name = obj["file_name"].as_str().unwrap_or(fallback).to_string();
                items.push((fid.to_string(), name, kind.to_string()));
            }
        }
    }
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let dest_dir = crate::state::AppState::resolve_chat_temp_dir(Some(chat_id)).join("tg_uploads");
    let mut out = Vec::new();
    for (idx, (fid, fname, kind)) in items.into_iter().enumerate() {
        let target_name = format!("{}_{}", idx + 1, fname);
        let p = tg_client
            .download_file(&fid, &dest_dir, &target_name, MAX_UPLOAD_BYTES)
            .await?;
        out.push(TgAttachment {
            name: fname,
            path: p.to_string_lossy().to_string(),
            kind,
        });
    }
    Ok(out)
}

async fn build_tg_user_msg(
    ctx: &TelegramRuntimeContext,
    model_id: &str,
    agent_prompt: &str,
    audio_parts: Vec<crate::llm::request_compat::ContentPart>,
    file_attachments: &[TgAttachment],
) -> Option<crate::llm::request_compat::ChatMessage> {
    use base64::Engine;
    if agent_prompt.trim().is_empty() && audio_parts.is_empty() && file_attachments.is_empty() {
        return None;
    }
    let native_vision = ctx.caps_registry.get(model_id).supports_vision;
    let mut parts: Vec<crate::llm::request_compat::ContentPart> = Vec::new();
    if !agent_prompt.trim().is_empty() {
        parts.push(crate::llm::request_compat::ContentPart::Text { text: agent_prompt.to_string() });
    }
    for ap in audio_parts {
        parts.push(ap);
    }
    for att in file_attachments {
        if att.kind == "image" && native_vision {
            if let Ok(bytes) = std::fs::read(&att.path) {
                if !bytes.is_empty() {
                    let low = att.path.to_ascii_lowercase();
                    let mime = if low.ends_with(".jpg") || low.ends_with(".jpeg") {
                        "image/jpeg"
                    } else if low.ends_with(".webp") {
                        "image/webp"
                    } else if low.ends_with(".gif") {
                        "image/gif"
                    } else {
                        "image/png"
                    };
                    parts.push(crate::llm::request_compat::ContentPart::ImageUrl {
                        image_url: crate::llm::request_compat::ImageUrlDetail {
                            url: format!(
                                "data:{};base64,{}",
                                mime,
                                base64::engine::general_purpose::STANDARD.encode(&bytes)
                            ),
                            detail: None,
                        },
                    });
                }
            }
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(crate::llm::request_compat::ChatMessage {
        role: "user".to_string(),
        content: crate::llm::request_compat::MessageContent::Parts(parts),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    })
}

async fn is_chat_running(
    ctx: &TelegramRuntimeContext,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    tg_chat_id: i64,
) -> bool {
    let active_cid = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .and_then(|s| s.active_chat_id.clone());
    if let Some(cid) = active_cid {
        ctx.stream_mgr.get_session(&cid).await.is_some()
    } else {
        false
    }
}

async fn take_tg_merge(tg_chat_id: i64) -> Option<(String, Value)> {
    let mut map = tg_merge_map().lock().await;
    map.remove(&tg_chat_id).map(|s| (s.text, s.msg))
}

fn tg_message_has_media(msg: &Value) -> bool {
    msg.get("document").is_some()
        || msg.get("photo").is_some()
        || msg.get("audio").is_some()
        || msg.get("voice").is_some()
        || msg.get("video").is_some()
        || msg.get("video_note").is_some()
        || msg.get("sticker").is_some()
}

async fn dispatch_tg_message(
    tg_client: &TelegramClient,
    tg_chat_id: i64,
    msg: Value,
    text: String,
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    projects_snap: &Arc<RwLock<Vec<SyncedProject>>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    config_arc: &Arc<RwLock<TelegramConfigFile>>,
    config_path_arc: &Arc<RwLock<Option<PathBuf>>>,
) {
    let is_command = text.trim_start().starts_with('/');
    let has_media = tg_message_has_media(&msg);

    if is_command || has_media || text.trim().is_empty() {
        if let Some((pending_text, pending_msg)) = take_tg_merge(tg_chat_id).await {
            handle_authorized_message(
                tg_client,
                tg_chat_id,
                &pending_msg,
                &pending_text,
                ctx,
                providers_snap,
                projects_snap,
                sessions,
                choices_arc,
                config_arc,
                config_path_arc,
            )
            .await;
        }
        handle_authorized_message(
            tg_client,
            tg_chat_id,
            &msg,
            &text,
            ctx,
            providers_snap,
            projects_snap,
            sessions,
            choices_arc,
            config_arc,
            config_path_arc,
        )
        .await;
        return;
    }

    let generation = {
        let mut map = tg_merge_map().lock().await;
        let slot = map.entry(tg_chat_id).or_insert_with(|| TgMergeSlot {
            generation: 0,
            text: String::new(),
            msg: msg.clone(),
        });
        slot.generation += 1;
        if !slot.text.is_empty() {
            slot.text.push('\n');
        }
        slot.text.push_str(&text);
        slot.msg = msg.clone();
        slot.generation
    };

    let tg_client_c = tg_client.clone();
    let ctx_c = ctx.clone();
    let providers_c = providers_snap.clone();
    let projects_c = projects_snap.clone();
    let sessions_c = sessions.clone();
    let choices_c = choices_arc.clone();
    let config_c = config_arc.clone();
    let config_path_c = config_path_arc.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(TG_MERGE_WINDOW_MS)).await;
        let flushed = {
            let mut map = tg_merge_map().lock().await;
            match map.get(&tg_chat_id) {
                Some(slot) if slot.generation == generation => {
                    map.remove(&tg_chat_id).map(|s| (s.text, s.msg))
                }
                _ => None,
            }
        };
        if let Some((merged_text, merged_msg)) = flushed {
            handle_authorized_message(
                &tg_client_c,
                tg_chat_id,
                &merged_msg,
                &merged_text,
                &ctx_c,
                &providers_c,
                &projects_c,
                &sessions_c,
                &choices_c,
                &config_c,
                &config_path_c,
            )
            .await;
        }
    });
}

async fn handle_authorized_message(
    tg_client: &TelegramClient,
    tg_chat_id: i64,
    msg: &Value,
    text: &str,
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    projects_snap: &Arc<RwLock<Vec<SyncedProject>>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    config_arc: &Arc<RwLock<TelegramConfigFile>>,
    config_path_arc: &Arc<RwLock<Option<PathBuf>>>,
) {
    let has_media = msg.get("document").is_some()
        || msg.get("photo").is_some()
        || msg.get("audio").is_some()
        || msg.get("voice").is_some()
        || msg.get("video").is_some()
        || msg.get("sticker").is_some();

    if text.is_empty() && !has_media {
        return;
    }

    let mut parts = text.splitn(2, char::is_whitespace);
    let raw_cmd = parts.next().unwrap_or("");
    let cmd = raw_cmd.split('@').next().unwrap_or(raw_cmd).to_ascii_lowercase();
    let arg = parts.next().unwrap_or("").trim();

    if !cmd.starts_with('/') && !text.trim().is_empty() {
        let pending_action = {
            let mut m = tg_pending_input_map().lock().await;
            m.remove(&tg_chat_id)
        };
        if let Some(act) = pending_action {
            let raw = text.trim();
            match act.as_str() {
                "prov_add" => {
                    let segs: Vec<&str> = raw.split('|').map(|x| x.trim()).collect();
                    if segs.len() < 3 || segs[0].is_empty() || segs[1].is_empty() {
                        let _ = tg_client.send_html(tg_chat_id, "Неверный формат. Используйте: <code>Название | BaseURL | API_Key | Protocol</code>", None).await;
                        return;
                    }
                    let pid = format!("tg-prov-{}", uuid::Uuid::new_v4().simple());
                    let proto = segs.get(3).copied().filter(|x| !x.is_empty()).unwrap_or("auto").to_string();
                    let new_prov = SyncedProviderConfig {
                        id: pid.clone(),
                        name: segs[0].to_string(),
                        base_url: segs[1].to_string(),
                        api_key: segs[2].to_string(),
                        api_protocol: Some(proto),
                        proxy_enabled: Some(false),
                        proxy_url: None,
                        default_model: String::new(),
                        available_models: Vec::new(),
                        model_capabilities: None,
                        enabled: true,
                    };
                    {
                        let mut snap = providers_snap.write().await;
                        snap.providers.push(new_prov.clone());
                        snap.active_provider = pid.clone();
                    }
                    if let Some(app) = ctx.app_handle.read().await.as_ref() {
                        let _ = app.emit("telegram-providers-updated", serde_json::json!({ "action": "add_provider", "provider": new_prov }));
                    }
                    let _ = tg_client.send_html(tg_chat_id, &format!("Провайдер <b>{}</b> добавлен и выбран. Теперь добавьте модель через меню Провайдеры -> ➕ Модель.", escape_html(segs[0])), None).await;
                    return;
                }
                "prov_edit" => {
                    let active_pid = {
                        let sess_pid = sessions.read().await.get(&tg_chat_id).and_then(|s| s.provider_id.clone());
                        let snap = providers_snap.read().await;
                        sess_pid.unwrap_or_else(|| snap.active_provider.clone())
                    };
                    let segs: Vec<&str> = raw.split('|').map(|x| x.trim()).collect();
                    let mut updated_item: Option<SyncedProviderConfig> = None;
                    {
                        let mut snap = providers_snap.write().await;
                        if let Some(prov) = snap.providers.iter_mut().find(|p| p.id == active_pid) {
                            if segs.len() >= 2 {
                                if !segs[0].is_empty() { prov.base_url = segs[0].to_string(); }
                                if !segs[1].is_empty() { prov.api_key = segs[1].to_string(); }
                            } else {
                                prov.api_key = raw.to_string();
                            }
                            updated_item = Some(prov.clone());
                        }
                    }
                    if let Some(up) = updated_item {
                        if let Some(app) = ctx.app_handle.read().await.as_ref() {
                            let _ = app.emit("telegram-providers-updated", serde_json::json!({ "action": "edit_provider", "provider": up }));
                        }
                        let _ = tg_client.send_html(tg_chat_id, &format!("Провайдер <b>{}</b> обновлён.", escape_html(&up.name)), None).await;
                    } else {
                        let _ = tg_client.send_html(tg_chat_id, "Активный провайдер не найден.", None).await;
                    }
                    return;
                }
                "model_add" => {
                    let active_pid = {
                        let sess_pid = sessions.read().await.get(&tg_chat_id).and_then(|s| s.provider_id.clone());
                        let snap = providers_snap.read().await;
                        sess_pid.unwrap_or_else(|| snap.active_provider.clone())
                    };
                    let new_models: Vec<String> = raw.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                    if new_models.is_empty() {
                        let _ = tg_client.send_html(tg_chat_id, "Пустой список моделей.", None).await;
                        return;
                    }
                    let mut first_added = String::new();
                    {
                        let mut snap = providers_snap.write().await;
                        if let Some(prov) = snap.providers.iter_mut().find(|p| p.id == active_pid) {
                            for m in &new_models {
                                if !prov.available_models.contains(m) {
                                    prov.available_models.push(m.clone());
                                }
                            }
                            first_added = new_models[0].clone();
                            if prov.default_model.is_empty() {
                                prov.default_model = first_added.clone();
                            }
                            snap.active_model = first_added.clone();
                        }
                    }
                    if !first_added.is_empty() {
                        {
                            let mut map = sessions.write().await;
                            let entry = map.entry(tg_chat_id).or_default();
                            entry.provider_id = Some(active_pid.clone());
                            entry.model_id = Some(first_added.clone());
                        }
                        if let Some(app) = ctx.app_handle.read().await.as_ref() {
                            let _ = app.emit("telegram-providers-updated", serde_json::json!({ "action": "add_models", "providerId": active_pid, "models": new_models }));
                        }
                        let _ = tg_client.send_html(tg_chat_id, &format!("Добавлены модели: <code>{}</code>", escape_html(&new_models.join(", "))), None).await;
                    } else {
                        let _ = tg_client.send_html(tg_chat_id, "Активный провайдер не найден.", None).await;
                    }
                    return;
                }
                _ => {}
            }
        }
    }

    let pending_rename = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .map(|s| s.pending_rename)
        .unwrap_or(false);
    if pending_rename && !cmd.starts_with('/') && !text.is_empty() {
        let active_cid = {
            let mut map = sessions.write().await;
            let entry = map.entry(tg_chat_id).or_default();
            entry.pending_rename = false;
            entry.active_chat_id.clone()
        };
        if let Some(cid) = active_cid {
            let clean_title = compact_text(text, 60);
            if ctx.app_state.db.update_chat_title(&cid, &clean_title).is_ok() {
                emit_chat_updated(ctx, &cid).await;
                let (html, running, has_chat) =
                    build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
                let rm = serde_json::to_value(TelegramMenuBuilder::session_menu(running, has_chat)).ok();
                let _ = tg_client.send_html(tg_chat_id, &html, rm.as_ref()).await;
                return;
            }
        }
    }

    if matches!(cmd.as_str(), "/providers" | "/provider" | "/models" | "/model")
        && is_chat_running(ctx, sessions, tg_chat_id).await
    {
        let _ = tg_client
            .send_html(tg_chat_id, "Сначала остановите текущую задачу: /stop", None)
            .await;
        return;
    }

    match cmd.as_str() {
        "/start" | "/menu" => {
            let (html, _, _) =
                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
            let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
            let _ = tg_client.send_html(tg_chat_id, &html, rm.as_ref()).await;
        }
        "/help" => {
            let help_html = "<b>Команды OmniHarness</b>\n\n/menu — главное меню\n/new — новая сессия\n/chats — список сессий\n/use &lt;id&gt; — переключиться на сессию\n/rename &lt;имя&gt; — переименовать сессию\n/delete — удалить сессию\n/providers — выбрать провайдера\n/models — выбрать модель\n/settings — настройки чата\n/status — текущее состояние\n/stop — остановить задачу\n\nЛюбой текст или файл запускает агента (или добавляется как инструкция к активной задаче).";
            let rm = serde_json::to_value(TelegramMenuBuilder::back_menu()).ok();
            let _ = tg_client.send_html(tg_chat_id, help_html, rm.as_ref()).await;
        }
        "/new" => {
            let res = create_new_telegram_chat(ctx, providers_snap, sessions, tg_chat_id).await;
            persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
            let (html, running, has_chat) =
                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
            let rm = serde_json::to_value(TelegramMenuBuilder::session_menu(running, has_chat)).ok();
            let note = match res {
                Ok(_) => "<i>Новая сессия готова — отправьте сообщение, чтобы запустить задачу.</i>",
                Err(ref e) => &format!("<i>Не удалось создать сессию: {}</i>", escape_html(e)),
            };
            let _ = tg_client
                .send_html(
                    tg_chat_id,
                    &format!("{}\n\n{}", html, note),
                    rm.as_ref(),
                )
                .await;
        }
        "/chats" | "/sessions" => {
            render_chats_page(tg_client, ctx, sessions, choices_arc, tg_chat_id, 0, None).await;
        }
        "/projects" | "/project" => {
            render_projects_page(tg_client, projects_snap, sessions, choices_arc, tg_chat_id, None).await;
        }
        "/history" => {
            render_chat_history(tg_client, ctx, sessions, tg_chat_id, None, None).await;
        }
        "/use" => {
            if arg.is_empty() {
                let _ = tg_client
                    .send_html(tg_chat_id, "Использование: <code>/use &lt;id&gt;</code>", None)
                    .await;
                return;
            }
            let chats = ctx.app_state.db.list_chats(None).unwrap_or_default();
            if let Some(found) = chats.into_iter().find(|c| c.id == arg || c.id.starts_with(arg)) {
                {
                    let mut map = sessions.write().await;
                    let entry = map.entry(tg_chat_id).or_default();
                    entry.active_chat_id = Some(found.id.clone());
                    entry.project_path = found.project_id.clone().filter(|s| !s.trim().is_empty());
                }
                persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
                emit_chat_updated(ctx, &found.id).await;
                let (html, running, has_chat) =
                    build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
                let rm = serde_json::to_value(TelegramMenuBuilder::session_menu(running, has_chat)).ok();
                let _ = tg_client.send_html(tg_chat_id, &html, rm.as_ref()).await;
            } else {
                let _ = tg_client.send_html(tg_chat_id, "Сессия не найдена.", None).await;
            }
        }
        "/rename" => {
            let active_cid = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .and_then(|s| s.active_chat_id.clone());
            let Some(cid) = active_cid else {
                let _ = tg_client.send_html(tg_chat_id, "Сначала выберите сессию.", None).await;
                return;
            };
            if arg.is_empty() {
                sessions.write().await.entry(tg_chat_id).or_default().pending_rename = true;
                let _ = tg_client
                    .send_html(tg_chat_id, "Отправьте новое название сессии следующим сообщением.", None)
                    .await;
            } else if ctx.app_state.db.update_chat_title(&cid, &compact_text(arg, 60)).is_ok() {
                emit_chat_updated(ctx, &cid).await;
                let (html, running, has_chat) =
                    build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
                let rm = serde_json::to_value(TelegramMenuBuilder::session_menu(running, has_chat)).ok();
                let _ = tg_client.send_html(tg_chat_id, &html, rm.as_ref()).await;
            }
        }
        "/delete" => {
            let active_cid = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .and_then(|s| s.active_chat_id.clone());
            let Some(cid) = active_cid else {
                let _ = tg_client.send_html(tg_chat_id, "Сначала выберите сессию.", None).await;
                return;
            };
            let title = ctx
                .app_state
                .db
                .get_chat(&cid)
                .map(|c| c.title)
                .unwrap_or_else(|_| cid.clone());
            let token = store_choices(choices_arc, tg_chat_id, "del", vec![cid]).await;
            let rm = serde_json::to_value(TelegramMenuBuilder::confirm_delete_menu(&token)).ok();
            let _ = tg_client
                .send_html(
                    tg_chat_id,
                    &format!("<b>Удалить сессию?</b>\n\n{}", escape_html(&title)),
                    rm.as_ref(),
                )
                .await;
        }
        "/status" | "/current" => {
            let (html, running, has_chat) =
                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
            let rm = serde_json::to_value(TelegramMenuBuilder::session_menu(running, has_chat)).ok();
            let _ = tg_client.send_html(tg_chat_id, &html, rm.as_ref()).await;
        }
        "/settings" | "/config" => {
            let (html, _, _) =
                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
            let reasoning = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .map(|s| s.reasoning_effort.clone())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "medium".to_string());
            let rm = serde_json::to_value(TelegramMenuBuilder::settings_menu(&reasoning)).ok();
            let _ = tg_client.send_html(tg_chat_id, &html, rm.as_ref()).await;
        }
        "/providers" | "/provider" => {
            render_providers_page(
                tg_client,
                ctx,
                providers_snap,
                sessions,
                choices_arc,
                tg_chat_id,
                0,
                None,
            )
            .await;
        }
        "/models" | "/model" => {
            render_models_page(
                tg_client,
                ctx,
                providers_snap,
                sessions,
                choices_arc,
                tg_chat_id,
                0,
                None,
            )
            .await;
        }
        "/stop" | "/cancel" | "/flush" => {
            let active_id = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .and_then(|s| s.active_chat_id.clone());
            if let Some(cid) = active_id {
                let stopped = ctx.stream_mgr.cancel_session(&cid).await;
                let _ = ctx.tool_dispatcher.pty_manager.kill_all_for_chat(&cid);
                if stopped {
                    if let Some(ref app) = *ctx.app_handle.read().await {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": cid, "type": "stopped" }),
                        );
                    }
                }
                let msg = if stopped {
                    "Работа агента остановлена."
                } else {
                    "Нет активной задачи в текущей сессии."
                };
                let _ = tg_client.send_html(tg_chat_id, msg, None).await;
            } else {
                let _ = tg_client.send_html(tg_chat_id, "Нет активной сессии.", None).await;
            }
        }
        "/btw" => {
            if arg.is_empty() {
                let _ = tg_client
                    .send_html(tg_chat_id, "Использование: <code>/btw &lt;быстрый вопрос&gt;</code>", None)
                    .await;
                return;
            }
            let user_msg_id = msg["message_id"].as_i64().unwrap_or(0);
            let question = arg.to_string();
            let tg_client_owned = tg_client.clone();
            let ctx_owned = ctx.clone();
            let snap_owned = providers_snap.clone();
            let sessions_owned = sessions.clone();
            tokio::spawn(async move {
                run_telegram_btw(
                    tg_client_owned,
                    tg_chat_id,
                    user_msg_id,
                    question,
                    ctx_owned,
                    snap_owned,
                    sessions_owned,
                )
                .await;
            });
        }
        _ if cmd.starts_with('/') => {
            let _ = tg_client
                .send_html(tg_chat_id, "Неизвестная команда. Откройте /menu или /help.", None)
                .await;
        }
        _ => {
            let prompt_text = text.to_string();
            if is_chat_running(ctx, sessions, tg_chat_id).await {
                let active_cid = sessions
                    .read()
                    .await
                    .get(&tg_chat_id)
                    .and_then(|s| s.active_chat_id.clone());
                let Some(cid) = active_cid else {
                    let _ = tg_client
                        .send_html(tg_chat_id, "Нет активной сессии.", None)
                        .await;
                    return;
                };
                let mut item =
                    crate::engine::prompt_queue::QueuedPrompt::new(&cid, prompt_text.clone(), "telegram");
                item.tg_chat_id = Some(tg_chat_id);
                item.tg_message_id = msg["message_id"].as_i64();
                let items = ctx.prompt_queue.add(item.clone()).await;
                emit_prompt_queue_updated(ctx, &cid, &items).await;
                let token = store_choices(choices_arc, tg_chat_id, "queue", vec![item.id.clone(), cid.clone()]).await;
                let short: String = prompt_text.chars().take(120).collect();
                let html = format!(
                    "<b>Задача добавлена в очередь</b>\n\n<code>{}</code>\n\n<i>Всего в очереди: {}</i>",
                    escape_html(short.trim()),
                    items.len()
                );
                let rm = tg_queue_keyboard(&token);
                let _ = tg_client.send_html(tg_chat_id, &html, Some(&rm)).await;
                return;
            }
            let tg_client_owned = tg_client.clone();
            let ctx_owned = ctx.clone();
            let snap_owned = providers_snap.clone();
            let sessions_owned = sessions.clone();
            let msg_owned = msg.clone();
            let config_owned = config_arc.clone();
            let config_path_owned = config_path_arc.clone();
            let choices_owned = choices_arc.clone();

            tokio::spawn(async move {
                run_tg_turn_draining(
                    tg_client_owned,
                    tg_chat_id,
                    msg_owned,
                    prompt_text,
                    ctx_owned,
                    snap_owned,
                    sessions_owned,
                    choices_owned,
                    config_owned,
                    config_path_owned,
                )
                .await;
            });
        }
    }
}

async fn handle_callback_query(
    tg_client: &TelegramClient,
    tg_chat_id: i64,
    msg_id: i64,
    cb_id: &str,
    cb_data: &str,
    ctx: &TelegramRuntimeContext,
    providers_snap: &Arc<RwLock<ProvidersSnapshotFile>>,
    projects_snap: &Arc<RwLock<Vec<SyncedProject>>>,
    sessions: &Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    pairing_code_lock: &Arc<RwLock<Option<String>>>,
    config_arc: &Arc<RwLock<TelegramConfigFile>>,
    config_path_arc: &Arc<RwLock<Option<PathBuf>>>,
) {
    let mutates_model = cb_data.starts_with("tg:prov:")
        || cb_data.starts_with("tg:model:")
        || cb_data.starts_with("tg:reasoning:");
    if mutates_model && is_chat_running(ctx, sessions, tg_chat_id).await {
        tg_client
            .answer_callback(cb_id, Some("Сначала остановите текущую задачу"))
            .await;
        return;
    }
    tg_client.answer_callback(cb_id, None).await;

    if let Some(rest) = cb_data.strip_prefix("btw_del:") {
        tg_client.delete_message(tg_chat_id, msg_id).await;
        if let Ok(user_mid) = rest.trim().parse::<i64>() {
            if user_mid > 0 {
                tg_client.delete_message(tg_chat_id, user_mid).await;
            }
        }
        return;
    }

    if let Some(token) = cb_data.strip_prefix("tg:q:del:") {
        let item_id = resolve_choice(choices_arc, tg_chat_id, "queue", token, 0).await;
        let chat_id = resolve_choice(choices_arc, tg_chat_id, "queue", token, 1).await;
        let (Some(item_id), Some(chat_id)) = (item_id, chat_id) else {
            send_or_edit(tg_client, tg_chat_id, Some(msg_id), "Задача уже неактуальна.", None).await;
            return;
        };
        let items = ctx.prompt_queue.remove(&chat_id, &item_id).await;
        emit_prompt_queue_updated(ctx, &chat_id, &items).await;
        let text = format!("🗑 <b>Задача удалена из очереди</b>\n\n<i>Осталось: {}</i>", items.len());
        send_or_edit(tg_client, tg_chat_id, Some(msg_id), &text, None).await;
        return;
    }

    if let Some(token) = cb_data.strip_prefix("tg:q:send:") {
        let item_id = resolve_choice(choices_arc, tg_chat_id, "queue", token, 0).await;
        let chat_id = resolve_choice(choices_arc, tg_chat_id, "queue", token, 1).await;
        let (Some(item_id), Some(chat_id)) = (item_id, chat_id) else {
            send_or_edit(tg_client, tg_chat_id, Some(msg_id), "Задача уже неактуальна.", None).await;
            return;
        };
        let Some(item) = ctx.prompt_queue.take_specific(&chat_id, &item_id).await else {
            send_or_edit(tg_client, tg_chat_id, Some(msg_id), "Задача уже отправлена.", None).await;
            return;
        };
        let remaining = ctx.prompt_queue.list(&chat_id).await;
        emit_prompt_queue_updated(ctx, &chat_id, &remaining).await;
        if ctx.stream_mgr.get_session(&chat_id).await.is_some() {
            match ctx.stream_mgr.inject_intervention(&chat_id, item.text.clone()).await {
                Ok(()) => {
                    let text = format!(
                        "▶️ <b>Задача отправлена в текущий ход</b>\n\n<code>{}</code>",
                        escape_html(item.text.chars().take(200).collect::<String>().trim())
                    );
                    send_or_edit(tg_client, tg_chat_id, Some(msg_id), &text, None).await;
                }
                Err(e) => {
                    send_or_edit(
                        tg_client,
                        tg_chat_id,
                        Some(msg_id),
                        &escape_html(&format!("Не удалось доставить задачу: {}", e)),
                        None,
                    )
                    .await;
                }
            }
            return;
        }
        send_or_edit(tg_client, tg_chat_id, Some(msg_id), "▶️ <b>Запускаю задачу...</b>", None).await;
        let tg_client_owned = tg_client.clone();
        let ctx_owned = ctx.clone();
        let snap_owned = providers_snap.clone();
        let sessions_owned = sessions.clone();
        let config_owned = config_arc.clone();
        let config_path_owned = config_path_arc.clone();
        let choices_owned = choices_arc.clone();
        let item_text = item.text.clone();
        tokio::spawn(async move {
            run_tg_turn_draining(
                tg_client_owned,
                tg_chat_id,
                json!({ "message_id": msg_id }),
                item_text,
                ctx_owned,
                snap_owned,
                sessions_owned,
                choices_owned,
                config_owned,
                config_path_owned,
            )
            .await;
        });
        return;
    }

    match cb_data {
        "tg:cmd:menu" => {
            let (html, _, _) =
                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
            let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
            send_or_edit(tg_client, tg_chat_id, Some(msg_id), &html, rm.as_ref()).await;
        }
        "tg:cmd:status" => {
            let (html, running, has_chat) =
                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
            let rm = serde_json::to_value(TelegramMenuBuilder::session_menu(running, has_chat)).ok();
            send_or_edit(tg_client, tg_chat_id, Some(msg_id), &html, rm.as_ref()).await;
        }
        "tg:cmd:new" => {
            let res = create_new_telegram_chat(ctx, providers_snap, sessions, tg_chat_id).await;
            persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
            let (html, running, has_chat) =
                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
            let rm = serde_json::to_value(TelegramMenuBuilder::session_menu(running, has_chat)).ok();
            let note = match res {
                Ok(_) => "<i>Новая сессия готова — отправьте сообщение, чтобы запустить задачу.</i>",
                Err(ref e) => &format!("<i>Не удалось создать сессию: {}</i>", escape_html(e)),
            };
            send_or_edit(
                tg_client,
                tg_chat_id,
                Some(msg_id),
                &format!("{}\n\n{}", html, note),
                rm.as_ref(),
            )
            .await;
        }
        "tg:cmd:chats" => {
            render_chats_page(tg_client, ctx, sessions, choices_arc, tg_chat_id, 0, Some(msg_id))
                .await;
        }
        "tg:cmd:projects" => {
            render_projects_page(tg_client, projects_snap, sessions, choices_arc, tg_chat_id, Some(msg_id))
                .await;
        }
        "tg:cmd:history" => {
            render_chat_history(tg_client, ctx, sessions, tg_chat_id, None, Some(msg_id)).await;
        }
        "tg:hist:noop" => {}
        "tg:cmd:providers" => {
            render_providers_page(
                tg_client,
                ctx,
                providers_snap,
                sessions,
                choices_arc,
                tg_chat_id,
                0,
                Some(msg_id),
            )
            .await;
        }
        "tg:cmd:models" => {
            render_models_page(
                tg_client,
                ctx,
                providers_snap,
                sessions,
                choices_arc,
                tg_chat_id,
                0,
                Some(msg_id),
            )
            .await;
        }
        "tg:prov-add" => {
            tg_pending_input_map().lock().await.insert(tg_chat_id, "prov_add".to_string());
            send_or_edit(
                tg_client,
                tg_chat_id,
                Some(msg_id),
                "Отправьте новым сообщением данные провайдера в формате:\n<code>Название | BaseURL | API_Key | Protocol</code>\n<i>Protocol опционально (auto, chat_completions, responses, anthropic_messages, google_gemini)</i>",
                None,
            ).await;
        }
        "tg:prov-edit" => {
            tg_pending_input_map().lock().await.insert(tg_chat_id, "prov_edit".to_string());
            send_or_edit(
                tg_client,
                tg_chat_id,
                Some(msg_id),
                "Отправьте новым сообщением для активного провайдера:\n<code>BaseURL | API_Key</code> или только новый <code>API_Key</code>",
                None,
            ).await;
        }
        "tg:model-add" => {
            tg_pending_input_map().lock().await.insert(tg_chat_id, "model_add".to_string());
            send_or_edit(
                tg_client,
                tg_chat_id,
                Some(msg_id),
                "Отправьте ID модели (или несколько через запятую) для добавления в активный провайдер:",
                None,
            ).await;
        }
        "tg:cmd:settings" => {
            let (html, _, _) =
                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
            let reasoning = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .map(|s| s.reasoning_effort.clone())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "medium".to_string());
            let rm = serde_json::to_value(TelegramMenuBuilder::settings_menu(&reasoning)).ok();
            send_or_edit(tg_client, tg_chat_id, Some(msg_id), &html, rm.as_ref()).await;
        }
        "tg:cmd:reasoning" => {
            let current = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .map(|s| s.reasoning_effort.clone())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "medium".to_string());
            let rm = serde_json::to_value(TelegramMenuBuilder::reasoning_menu(&current)).ok();
            send_or_edit(
                tg_client,
                tg_chat_id,
                Some(msg_id),
                "<b>Уровень рассуждения (Reasoning Effort)</b>",
                rm.as_ref(),
            )
            .await;
        }
        "tg:cmd:rename" => {
            sessions.write().await.entry(tg_chat_id).or_default().pending_rename = true;
            let rm = serde_json::to_value(TelegramMenuBuilder::back_menu()).ok();
            send_or_edit(
                tg_client,
                tg_chat_id,
                Some(msg_id),
                "<b>Переименование сессии</b>\n\nОтправьте новое название следующим сообщением.",
                rm.as_ref(),
            )
            .await;
        }
        "tg:cmd:delete" => {
            let active_cid = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .and_then(|s| s.active_chat_id.clone());
            if let Some(cid) = active_cid {
                let title = ctx
                    .app_state
                    .db
                    .get_chat(&cid)
                    .map(|c| c.title)
                    .unwrap_or_else(|_| cid.clone());
                let token = store_choices(choices_arc, tg_chat_id, "del", vec![cid]).await;
                let rm = serde_json::to_value(TelegramMenuBuilder::confirm_delete_menu(&token)).ok();
                send_or_edit(
                    tg_client,
                    tg_chat_id,
                    Some(msg_id),
                    &format!("<b>Удалить сессию?</b>\n\n{}", escape_html(&title)),
                    rm.as_ref(),
                )
                .await;
            }
        }
        "tg:cmd:help" => {
            let help_html = "<b>Команды OmniHarness</b>\n\n/menu — главное меню\n/new — новая сессия\n/chats — список сессий\n/use &lt;id&gt; — переключиться на сессию\n/rename &lt;имя&gt; — переименовать сессию\n/delete — удалить сессию\n/providers — выбрать провайдера\n/models — выбрать модель\n/settings — настройки чата\n/status — текущее состояние\n/stop — остановить задачу";
            let rm = serde_json::to_value(TelegramMenuBuilder::back_menu()).ok();
            send_or_edit(tg_client, tg_chat_id, Some(msg_id), help_html, rm.as_ref()).await;
        }
        "tg:cmd:stop" => {
            let active_id = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .and_then(|s| s.active_chat_id.clone());
            let stopped = if let Some(cid) = active_id {
                let s = ctx.stream_mgr.cancel_session(&cid).await;
                let _ = ctx.tool_dispatcher.pty_manager.kill_all_for_chat(&cid);
                if s {
                    if let Some(ref app) = *ctx.app_handle.read().await {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": cid, "type": "stopped" }),
                        );
                    }
                }
                s
            } else {
                false
            };
            let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
            let text = if stopped {
                "Работа агента остановлена."
            } else {
                "Нет активной задачи."
            };
            send_or_edit(tg_client, tg_chat_id, Some(msg_id), text, rm.as_ref()).await;
        }
        "tg:action:abort_all" => {
            let active = ctx.stream_mgr.list_active_sessions().await;
            let count = active.len();
            for cid in active {
                ctx.stream_mgr.cancel_session(&cid).await;
                let _ = ctx.tool_dispatcher.pty_manager.kill_all_for_chat(&cid);
                if let Some(ref app) = *ctx.app_handle.read().await {
                    let _ = app.emit(
                        "telegram-chat-stream",
                        json!({ "chatId": cid, "type": "stopped" }),
                    );
                }
            }
            let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
            send_or_edit(
                tg_client,
                tg_chat_id,
                Some(msg_id),
                &format!("Остановлено активных задач: {}", count),
                rm.as_ref(),
            )
            .await;
        }
        "tg:action:reset_pairing" => {
            *pairing_code_lock.write().await = None;
            let reasoning = sessions
                .read()
                .await
                .get(&tg_chat_id)
                .map(|s| s.reasoning_effort.clone())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "medium".to_string());
            let rm = serde_json::to_value(TelegramMenuBuilder::settings_menu(&reasoning)).ok();
            send_or_edit(
                tg_client,
                tg_chat_id,
                Some(msg_id),
                "Код сопряжения сброшен.",
                rm.as_ref(),
            )
            .await;
        }
        _ => {
            if let Some(lvl) = cb_data.strip_prefix("tg:reasoning:") {
                if matches!(lvl, "low" | "medium" | "high") {
                    sessions.write().await.entry(tg_chat_id).or_default().reasoning_effort =
                        lvl.to_string();
                    persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
                    let ac = sessions.read().await.get(&tg_chat_id).and_then(|s| s.active_chat_id.clone());
                    emit_settings_updated(ctx, providers_snap, None, None, Some(lvl), ac.as_deref()).await;
                    let (html, _, _) =
                        build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
                    let rm = serde_json::to_value(TelegramMenuBuilder::settings_menu(lvl)).ok();
                    send_or_edit(tg_client, tg_chat_id, Some(msg_id), &html, rm.as_ref()).await;
                }
            } else if let Some(p_str) = cb_data.strip_prefix("tg:chats-page:") {
                if let Ok(page) = p_str.parse::<usize>() {
                    render_chats_page(
                        tg_client,
                        ctx,
                        sessions,
                        choices_arc,
                        tg_chat_id,
                        page,
                        Some(msg_id),
                    )
                    .await;
                }
            } else if let Some(p_str) = cb_data.strip_prefix("tg:providers-page:") {
                if let Ok(page) = p_str.parse::<usize>() {
                    render_providers_page(
                        tg_client,
                        ctx,
                        providers_snap,
                        sessions,
                        choices_arc,
                        tg_chat_id,
                        page,
                        Some(msg_id),
                    )
                    .await;
                }
            } else if let Some(p_str) = cb_data.strip_prefix("tg:models-page:") {
                if let Ok(page) = p_str.parse::<usize>() {
                    render_models_page(
                        tg_client,
                        ctx,
                        providers_snap,
                        sessions,
                        choices_arc,
                        tg_chat_id,
                        page,
                        Some(msg_id),
                    )
                    .await;
                }
            } else if cb_data == "tg:proj-clear" {
                sessions.write().await.entry(tg_chat_id).or_default().project_path = None;
                persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
                let (html, _, _) =
                    build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
                let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
                send_or_edit(tg_client, tg_chat_id, Some(msg_id), &html, rm.as_ref()).await;
            } else if let Some(rest) = cb_data.strip_prefix("tg:proj:") {
                let mut parts = rest.splitn(2, ':');
                if let (Some(tok), Some(idx_s)) = (parts.next(), parts.next()) {
                    if let Ok(idx) = idx_s.parse::<usize>() {
                        if let Some(pp) = resolve_choice(choices_arc, tg_chat_id, "proj", tok, idx).await {
                            sessions.write().await.entry(tg_chat_id).or_default().project_path =
                                Some(pp);
                            persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
                            let (html, _, _) =
                                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
                            let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
                            send_or_edit(tg_client, tg_chat_id, Some(msg_id), &html, rm.as_ref()).await;
                        }
                    }
                }
            } else if let Some(rest) = cb_data.strip_prefix("tg:chat:") {
                let mut parts = rest.splitn(2, ':');
                if let (Some(tok), Some(idx_s)) = (parts.next(), parts.next()) {
                    if let Ok(idx) = idx_s.parse::<usize>() {
                        if let Some(cid) = resolve_choice(choices_arc, tg_chat_id, "chat", tok, idx).await {
                            let proj_opt = ctx
                                .app_state
                                .db
                                .get_chat(&cid)
                                .ok()
                                .and_then(|c| c.project_id)
                                .filter(|s| !s.trim().is_empty());
                            {
                                let mut map = sessions.write().await;
                                let entry = map.entry(tg_chat_id).or_default();
                                entry.active_chat_id = Some(cid.clone());
                                entry.project_path = proj_opt;
                            }
                            persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
                            emit_chat_updated(ctx, &cid).await;
                            render_chat_history(tg_client, ctx, sessions, tg_chat_id, None, Some(msg_id)).await;
                        }
                    }
                }
            } else if let Some(rest) = cb_data.strip_prefix("tg:hist:page:") {
                if let Ok(p) = rest.trim().parse::<usize>() {
                    render_chat_history(tg_client, ctx, sessions, tg_chat_id, Some(p), Some(msg_id)).await;
                }
            } else if let Some(rest) = cb_data.strip_prefix("tg:prov:") {
                let mut parts = rest.splitn(2, ':');
                if let (Some(tok), Some(idx_s)) = (parts.next(), parts.next()) {
                    if let Ok(idx) = idx_s.parse::<usize>() {
                        if let Some(pid) = resolve_choice(choices_arc, tg_chat_id, "prov", tok, idx).await {
                            let (providers, _, _) =
                                list_available_providers(ctx, providers_snap).await;
                            let first_mid = providers
                                .iter()
                                .find(|p| p.id == pid)
                                .and_then(|p| p.models.first())
                                .map(|m| m.id.clone());
                            {
                                let mut map = sessions.write().await;
                                let entry = map.entry(tg_chat_id).or_default();
                                entry.provider_id = Some(pid);
                                entry.model_id = first_mid;
                            }
                            let (sel_pid, sel_mid) = {
                                let map = sessions.read().await;
                                let e = map.get(&tg_chat_id);
                                (e.and_then(|s| s.provider_id.clone()), e.and_then(|s| s.model_id.clone()))
                            };
                            let ac = sessions.read().await.get(&tg_chat_id).and_then(|s| s.active_chat_id.clone());
                            if let (Some(cid), Some(mid)) = (ac.as_deref(), sel_mid.as_deref()) {
                                if !mid.is_empty() {
                                    let _ = ctx.app_state.db.update_chat_model(cid, mid);
                                    emit_chat_updated(ctx, cid).await;
                                }
                            }
                            emit_settings_updated(ctx, providers_snap, sel_pid.as_deref(), sel_mid.as_deref(), None, ac.as_deref()).await;
                            persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
                            render_models_page(
                                tg_client,
                                ctx,
                                providers_snap,
                                sessions,
                                choices_arc,
                                tg_chat_id,
                                0,
                                Some(msg_id),
                            )
                            .await;
                        }
                    }
                }
            } else if let Some(rest) = cb_data.strip_prefix("tg:model:") {
                let mut parts = rest.splitn(2, ':');
                if let (Some(tok), Some(idx_s)) = (parts.next(), parts.next()) {
                    if let Ok(idx) = idx_s.parse::<usize>() {
                        if let Some(packed) = resolve_choice(choices_arc, tg_chat_id, "model", tok, idx).await {
                            let mut sp = packed.splitn(2, "||");
                            let (pid, mid) = (
                                sp.next().unwrap_or("").to_string(),
                                sp.next().unwrap_or("").to_string(),
                            );
                            let reasoning = {
                                let mut map = sessions.write().await;
                                let entry = map.entry(tg_chat_id).or_default();
                                if !pid.is_empty() {
                                    entry.provider_id = Some(pid.clone());
                                }
                                entry.model_id = Some(mid.clone());
                                if entry.reasoning_effort.is_empty() {
                                    "medium".to_string()
                                } else {
                                    entry.reasoning_effort.clone()
                                }
                            };
                            let ac = sessions.read().await.get(&tg_chat_id).and_then(|s| s.active_chat_id.clone());
                            if let Some(cid) = ac.as_deref() {
                                if !mid.is_empty() {
                                    let _ = ctx.app_state.db.update_chat_model(cid, &mid);
                                    emit_chat_updated(ctx, cid).await;
                                }
                            }
                            emit_settings_updated(
                                ctx,
                                providers_snap,
                                if pid.is_empty() { None } else { Some(pid.as_str()) },
                                if mid.is_empty() { None } else { Some(mid.as_str()) },
                                None,
                                ac.as_deref(),
                            ).await;
                            persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
                            let (providers, _, _) =
                                list_available_providers(ctx, providers_snap).await;
                            if let Some(p) = providers.iter().find(|p| p.id == pid) {
                                if let Some(m) = p.models.iter().find(|m| m.id == mid) {
                                    let html = format_model_details_html(p, m, &reasoning);
                                    let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
                                    send_or_edit(
                                        tg_client,
                                        tg_chat_id,
                                        Some(msg_id),
                                        &html,
                                        rm.as_ref(),
                                    )
                                    .await;
                                    return;
                                }
                            }
                            let (html, _, _) =
                                build_status_html(ctx, providers_snap, sessions, tg_chat_id).await;
                            let rm = serde_json::to_value(TelegramMenuBuilder::main_menu()).ok();
                            send_or_edit(tg_client, tg_chat_id, Some(msg_id), &html, rm.as_ref()).await;
                        }
                    }
                }
            } else if let Some(rest) = cb_data.strip_prefix("tg:delete-chat:") {
                let mut parts = rest.splitn(2, ':');
                if let (Some(tok), Some(idx_s)) = (parts.next(), parts.next()) {
                    if let Ok(idx) = idx_s.parse::<usize>() {
                        if let Some(cid) = resolve_choice(choices_arc, tg_chat_id, "del", tok, idx).await {
                            let _ = ctx.stream_mgr.cancel_session(&cid).await;
                            let _ = ctx.app_state.db.delete_chat(&cid);
                            emit_chat_updated(ctx, &cid).await;
                            {
                                let mut map = sessions.write().await;
                                if let Some(entry) = map.get_mut(&tg_chat_id) {
                                    if entry.active_chat_id.as_deref() == Some(cid.as_str()) {
                                        entry.active_chat_id = None;
                                    }
                                }
                            }
                            persist_session_snapshot(config_arc, config_path_arc, sessions, None).await;
                            render_chats_page(
                                tg_client,
                                ctx,
                                sessions,
                                choices_arc,
                                tg_chat_id,
                                0,
                                Some(msg_id),
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }
}

async fn run_telegram_agent_turn(
    tg_client: TelegramClient,
    tg_chat_id: i64,
    msg: Value,
    prompt_text: String,
    ctx: TelegramRuntimeContext,
    providers_snap: Arc<RwLock<ProvidersSnapshotFile>>,
    sessions: Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: Arc<RwLock<HashMap<String, Vec<String>>>>,
    config_arc: Arc<RwLock<TelegramConfigFile>>,
    config_path_arc: Arc<RwLock<Option<PathBuf>>>,
) -> String {
    let (provider, model) =
        match resolve_provider_and_model(&ctx, &providers_snap, &sessions, tg_chat_id).await {
            Ok(res) => res,
            Err(e) => {
                let _ = tg_client
                    .send_html(tg_chat_id, &escape_html(&format_user_error(&e)), None)
                    .await;
                return String::new();
            }
        };

    let chat_id = match ensure_active_chat(
        &ctx,
        &sessions,
        tg_chat_id,
        &model.id,
        Some(if prompt_text.is_empty() {
            "Telegram Files"
        } else {
            &prompt_text
        }),
    )
    .await
    {
        Ok(cid) => cid,
        Err(e) => {
            let _ = tg_client
                .send_html(tg_chat_id, &escape_html(&format_user_error(&e)), None)
                .await;
            return String::new();
        }
    };

    let chat_proj = ctx
        .app_state
        .db
        .get_chat(&chat_id)
        .ok()
        .and_then(|c| c.project_id)
        .filter(|s| !s.trim().is_empty());
    let bootstrap_cfg = ctx.app_state.get_bootstrap_config().await;
    let (sandbox_root_opt, workspace_path) = match chat_proj {
        Some(dir) => {
            let p = PathBuf::from(dir);
            (Some(p.clone()), p)
        }
        None => match &bootstrap_cfg.workspace_dir {
            Some(dir) if !dir.trim().is_empty() => {
                let p = PathBuf::from(dir);
                (Some(p.clone()), p)
            }
            _ => {
                let chat_ws = crate::state::AppState::resolve_install_workspace().join(&chat_id);
                let _ = std::fs::create_dir_all(&chat_ws);
                (None, chat_ws)
            }
        },
    };
    let workspace_str = workspace_path.to_string_lossy().to_string();

    let tg_attachments = match collect_tg_attachments(&tg_client, &msg, &chat_id).await {
        Ok(items) => items,
        Err(e) => {
            let _ = tg_client.send_html(tg_chat_id, &escape_html(&e), None).await;
            return String::new();
        }
    };

    let (voice_atts, file_atts): (Vec<TgAttachment>, Vec<TgAttachment>) = tg_attachments
        .into_iter()
        .partition(|a| a.kind == "voice" || a.kind == "audio");

    use base64::Engine;
    let native_audio = crate::llm::capabilities::model_supports_native_audio(&model.id);
    let fallback_key = ctx.app_state.get_credential("openai_api_key").await.ok();
    let mut voice_transcripts = Vec::new();
    let mut audio_parts = Vec::new();

    for att in &voice_atts {
        let path = std::path::Path::new(&att.path);
        let wav_res = crate::speech::convert::decode_to_wav_16k_mono(path);
        match wav_res {
            Ok(bytes) if !bytes.is_empty() => {
                if native_audio {
                    audio_parts.push(crate::llm::request_compat::ContentPart::InputAudio {
                        input_audio: crate::llm::request_compat::InputAudioDetail {
                            data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                            format: "wav".to_string(),
                        },
                    });
                }
                match ctx
                    .speech_engine
                    .transcribe_audio(&bytes, fallback_key.as_deref(), None, None)
                    .await
                {
                    Ok(res) if !res.text.trim().is_empty() => {
                        voice_transcripts.push(res.text.trim().to_string());
                    }
                    Ok(_) => {}
                    Err(e) => {
                        crate::logger::warn("Telegram", &format!("Voice transcription failed: {}", e));
                    }
                }
            }
            Err(e) => {
                crate::logger::warn("Telegram", &format!("Failed to decode voice audio {}: {}", att.name, e));
            }
            Ok(_) => {}
        }
    }

    let combined_voice_text = voice_transcripts.join("\n\n");
    let mut user_display_text = prompt_text.trim().to_string();
    if !combined_voice_text.is_empty() {
        if user_display_text.is_empty() {
            user_display_text = combined_voice_text;
        } else {
            user_display_text.push_str("\n\n");
            user_display_text.push_str(&combined_voice_text);
        }
    } else if user_display_text.is_empty() {
        if !voice_atts.is_empty() {
            user_display_text = "[Голосовое сообщение]".to_string();
        } else if !file_atts.is_empty() {
            let names: Vec<String> = file_atts.iter().map(|f| f.name.clone()).collect();
            let all_images = file_atts.iter().all(|f| f.kind == "image");
            if all_images {
                user_display_text = format!("[Прикреплено изображение: {}]", names.join(", "));
            } else {
                user_display_text = format!("[Прикреплён файл: {}]", names.join(", "));
            }
        }
    }

    if user_display_text.trim().is_empty() && audio_parts.is_empty() && file_atts.is_empty() {
        return String::new();
    }

    let mut agent_prompt = if user_display_text.is_empty() || user_display_text.starts_with('[') {
        if !file_atts.is_empty() {
            "Проанализируй приложенные файлы и выполни задачу полностью.".to_string()
        } else {
            user_display_text.clone()
        }
    } else {
        user_display_text.clone()
    };

    if !file_atts.is_empty() {
        agent_prompt.push_str("\n\n[Attached files]\n");
        for att in &file_atts {
            agent_prompt.push_str(&format!("- {} ({})\n", att.path, att.name));
        }
    }

    let pending_user_msg = build_tg_user_msg(&ctx, &model.id, &agent_prompt, audio_parts, &file_atts).await;

    if ctx.stream_mgr.get_session(&chat_id).await.is_some() {
        match ctx
            .stream_mgr
            .inject_intervention(&chat_id, user_display_text)
            .await
        {
            Ok(()) => {
                let _ = tg_client
                    .send_html(tg_chat_id, "Инструкция добавлена в текущую задачу.", None)
                    .await;
            }
            Err(_) => {
                let _ = tg_client
                    .send_html(
                        tg_chat_id,
                        "Текущая задача занята. Отправьте /stop или /new для новой сессии.",
                        None,
                    )
                    .await;
            }
        }
        return String::new();
    }

    let chat_lock = get_chat_lock(&chat_id).await;
    let _chat_turn_guard = chat_lock.lock().await;
    TelegramUploadManager::register_active_chat(&chat_id, tg_client.clone(), tg_chat_id).await;

    persist_session_snapshot(&config_arc, &config_path_arc, &sessions, None).await;

    let existing_msgs = ctx.app_state.db.list_messages(&chat_id).unwrap_or_default();
    let seq = existing_msgs.last().map(|m| m.sequence_num + 1).unwrap_or(1);
    let user_msg = crate::db::chats::Message {
        id: uuid::Uuid::new_v4().to_string(),
        chat_id: chat_id.clone(),
        role: "user".to_string(),
        content: user_display_text.clone(),
        model_id: None,
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        sequence_num: seq,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let _ = ctx.app_state.db.create_message(&user_msg);
    let stream_app_handle = ctx.app_handle.read().await.clone();
    if let Some(ref app) = stream_app_handle {
        let _ = app.emit(
            "telegram-chat-stream",
            json!({
                "chatId": chat_id,
                "type": "start",
                "userText": user_display_text,
                "modelId": model.id,
                "providerId": provider.id,
            }),
        );
    }

    let key_pool = (*ctx.key_pool).clone();
    if !provider.api_key.trim().is_empty() {
        key_pool.set_keys(&provider.name, vec![provider.api_key.trim().to_string()]);
    }
    let caps_reg = (*ctx.caps_registry).clone();
    caps_reg.update_model_caps(
        &model.id,
        Some(model.context_window),
        Some(model.max_output_tokens),
        Some(model.supports_reasoning),
    );

    let is_oauth_proto = matches!(
        provider.api_protocol.as_deref().unwrap_or(""),
        "antigravity" | "openai_codex" | "claude_oauth" | "kimi_oauth" | "xai_oauth" | "zcode_oauth" | "perplexity_oauth"
    );
    let (eff_px_en, eff_px_url) = if is_oauth_proto {
        crate::llm::oauth_accounts::resolve_oauth_proxy(Some(provider.proxy_enabled), provider.proxy_url.as_deref())
    } else {
        (Some(provider.proxy_enabled), provider.proxy_url.clone())
    };
    let http_client = crate::build_http_client(eff_px_en, eff_px_url.as_deref(), Some(&ctx.http_client))
        .unwrap_or_else(|_| ctx.http_client.clone());

    let session = ctx.stream_mgr.create_session(&chat_id).await;
    let contract_mutex = {
        let mut map = ctx.active_contracts.write().await;
        map.entry(chat_id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None)))
            .clone()
    };

    let typing_active = Arc::new(AtomicBool::new(true));
    let typing_flag = typing_active.clone();
    let typing_client = tg_client.clone();
    let typing_sessions = sessions.clone();
    let typing_chat_id = chat_id.clone();
    let typing_task = tokio::spawn(async move {
        while typing_flag.load(Ordering::SeqCst) {
            let is_focused = {
                let map = typing_sessions.read().await;
                map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(typing_chat_id.as_str())
            };
            if is_focused {
                typing_client.send_chat_action(tg_chat_id, "typing").await;
            }
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
    });

    let (tx, mut rx) = tokio::sync::mpsc::channel(128);
    let mut tool_stream = LiveToolStream::new(
        tg_client.clone(),
        tg_chat_id,
        model.display_name.clone(),
    );
    let is_initially_focused = {
        let map = sessions.read().await;
        map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(chat_id.as_str())
    };
    if is_initially_focused {
        tool_stream.start().await;
    }

    let stream_chat_id = chat_id.clone();
    let stream_app_ev = stream_app_handle.clone();
    let event_sessions = sessions.clone();
    let event_task = tokio::spawn(async move {
        let mut final_text = String::new();
        let mut reasoning_text = String::new();
        let mut error_text: Option<String> = None;

        while let Some(evt) = rx.recv().await {
            match evt {
                crate::llm::StreamEvent::ContentDelta { delta } => {
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": stream_chat_id, "type": "content", "delta": delta }),
                        );
                    }
                    final_text.push_str(&delta);
                }
                crate::llm::StreamEvent::ReasoningDelta { delta } => {
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": stream_chat_id, "type": "thinking", "delta": delta }),
                        );
                    }
                    reasoning_text.push_str(&delta);
                }
                crate::llm::StreamEvent::ToolCallComplete {
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": stream_chat_id, "type": "tool_start", "id": id, "name": name, "arguments": arguments }),
                        );
                    }
                    tool_stream.on_tool_start(&id, &name, &arguments);
                    let is_focused = {
                        let map = event_sessions.read().await;
                        map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(stream_chat_id.as_str())
                    };
                    if is_focused {
                        tool_stream.maybe_flush(false).await;
                    }
                }
                crate::llm::StreamEvent::ToolExecEnd {
                    id,
                    success,
                    output,
                    error,
                    elapsed_ms,
                    ..
                } => {
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({
                                "chatId": stream_chat_id,
                                "type": "tool_end",
                                "id": id,
                                "success": success,
                                "output": output,
                                "error": error,
                                "elapsedMs": elapsed_ms
                            }),
                        );
                    }
                    tool_stream.on_tool_end(&id, success, elapsed_ms);
                    let is_focused = {
                        let map = event_sessions.read().await;
                        map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(stream_chat_id.as_str())
                    };
                    if is_focused {
                        tool_stream.maybe_flush(false).await;
                    }
                }
                crate::llm::StreamEvent::RetryNotice {
                    attempt, reason, ..
                } => {
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": stream_chat_id, "type": "retry", "attempt": attempt, "reason": reason }),
                        );
                    }
                    tool_stream.on_retry(attempt, &reason);
                    let is_focused = {
                        let map = event_sessions.read().await;
                        map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(stream_chat_id.as_str())
                    };
                    if is_focused {
                        tool_stream.maybe_flush(false).await;
                    }
                }
                crate::llm::StreamEvent::ContextCompacted {
                    before_tokens,
                    after_tokens,
                    ref summary,
                    ..
                } => {
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({
                                "chatId": stream_chat_id,
                                "type": "context_compacted",
                                "beforeTokens": before_tokens,
                                "afterTokens": after_tokens,
                                "summary": summary,
                            }),
                        );
                    }
                    tool_stream.on_note(&format!(
                        "📦 Контекст сжат: {} → {} токенов",
                        before_tokens, after_tokens
                    ));
                    let is_focused = {
                        let map = event_sessions.read().await;
                        map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(stream_chat_id.as_str())
                    };
                    if is_focused {
                        tool_stream.maybe_flush(false).await;
                    }
                }
                crate::llm::StreamEvent::GoalStatusUpdated {
                    status,
                    turn,
                    max_turns,
                    ..
                } => {
                    tool_stream.on_note(&format!(
                        "◎ /goal {} (ход {}/{})",
                        status, turn, max_turns
                    ));
                    let is_focused = {
                        let map = event_sessions.read().await;
                        map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(stream_chat_id.as_str())
                    };
                    if is_focused {
                        tool_stream.maybe_flush(false).await;
                    }
                }
                crate::llm::StreamEvent::PlanApprovalRequired { plan_summary } => {
                    if !plan_summary.trim().is_empty() {
                        final_text.push_str(&format!(
                            "\n\n<b>План готов к проверке:</b>\n{}",
                            plan_summary
                        ));
                    }
                }
                crate::llm::StreamEvent::TurnBufferReset => {
                    final_text.clear();
                    reasoning_text.clear();
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": stream_chat_id, "type": "turn_buffer_reset" }),
                        );
                    }
                }
                crate::llm::StreamEvent::RetroactiveReasoningMove { text } => {
                    if final_text.ends_with(&text) {
                        final_text.truncate(final_text.len() - text.len());
                    }
                    reasoning_text.push_str(&text);
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": stream_chat_id, "type": "retroactive_reasoning_move", "reasoningContent": text }),
                        );
                    }
                }
                crate::llm::StreamEvent::TaskBoardUpdated { items } => {
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": stream_chat_id, "type": "task_board_updated", "items": items }),
                        );
                    }
                }
                crate::llm::StreamEvent::Error { error } => {
                    if let Some(ref app) = stream_app_ev {
                        let _ = app.emit(
                            "telegram-chat-stream",
                            json!({ "chatId": stream_chat_id, "type": "error", "error": error }),
                        );
                    }
                    error_text = Some(error);
                }
                _ => {}
            }
        }
        let is_finally_focused = {
            let map = event_sessions.read().await;
            map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(stream_chat_id.as_str())
        };
        if is_finally_focused {
            tool_stream.finish_and_cleanup().await;
        }
        (final_text, reasoning_text, error_text)
    });

    let mut contract_lock = contract_mutex.lock().await;
    let tg_ctx_key = format!("tg_ctx:{}", chat_id);
    let mut history: Vec<crate::llm::request_compat::ChatMessage> = ctx
        .app_state
        .db
        .load_checkpoint(&chat_id)
        .ok()
        .flatten()
        .or_else(|| ctx.app_state.db.load_checkpoint(&tg_ctx_key).ok().flatten())
        .and_then(|j| serde_json::from_str(&j).ok())
        .map(|mut h: Vec<crate::llm::request_compat::ChatMessage>| {
            let known_texts: std::collections::HashSet<String> = h
                .iter()
                .filter(|m| m.role == "user" || m.role == "assistant")
                .map(|m| format!("{}:{}", m.role, m.content.as_text_lossy()))
                .collect();
            for m in &existing_msgs {
                if m.content.trim().is_empty() {
                    continue;
                }
                let sig = format!("{}:{}", m.role, m.content);
                if !known_texts.contains(&sig) {
                    if m.role == "assistant" {
                        h.push(crate::llm::request_compat::ChatMessage::assistant(m.content.clone()));
                    } else if m.role == "user" {
                        h.push(crate::llm::request_compat::ChatMessage::user(m.content.clone()));
                    }
                }
            }
            h
        })
        .unwrap_or_else(|| {
            let mut h = Vec::new();
            if !existing_msgs.is_empty() {
                let sys_prompt = crate::engine::prompt_builder::PromptBuilder::build_system_prompt(
                    &crate::engine::role_permissions::AgentRole::Main,
                    &workspace_str,
                    None,
                    None,
                    Some(&model.id),
                );
                h.push(crate::llm::request_compat::ChatMessage::system(sys_prompt));
                for m in &existing_msgs {
                    if m.content.trim().is_empty() {
                        continue;
                    }
                    if m.role == "assistant" {
                        h.push(crate::llm::request_compat::ChatMessage::assistant(
                            m.content.clone(),
                        ));
                    } else if m.role == "user" {
                        h.push(crate::llm::request_compat::ChatMessage::user(
                            m.content.clone(),
                        ));
                    }
                }
            }
            h
        });

    let reasoning_effort = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .map(|s| s.reasoning_effort.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "medium".to_string());

    if let Some(pu) = pending_user_msg {
        history.push(pu);
    }

    let is_plan_req = agent_prompt
        .split_whitespace()
        .any(|t| t.eq_ignore_ascii_case("/plan"));
    let ws_tools = ctx
        .tool_dispatcher
        .for_workspace_with_cwd(sandbox_root_opt.as_deref(), &workspace_path, &chat_id);
    ws_tools.set_plan_mode(is_plan_req);

    let goal_cmd = crate::engine::goal_loop::parse_goal_command(&agent_prompt);
    let run_res = if let Some(crate::engine::goal_loop::GoalCommandAction::Clear) = goal_cmd {
        let _ = ctx.app_state.db.delete_goal_state(&chat_id);
        let _ = tx
            .send(crate::llm::StreamEvent::ContentDelta {
                delta: "Goal cleared.".to_string(),
            })
            .await;
        Ok(())
    } else if let Some(crate::engine::goal_loop::GoalCommandAction::Status) = goal_cmd {
        let loaded = ctx
            .app_state
            .db
            .load_goal_state(&chat_id)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<crate::engine::goal_loop::GoalState>(&s).ok());
        let msg = if let Some(g) = loaded {
            format!(
                "Goal status: **{}** (turn {}/{}, elapsed {}s)\nCondition: {}",
                g.status.as_str(),
                g.turn,
                g.max_turns,
                g.elapsed_secs(),
                g.condition
            )
        } else {
            "No active goal in this session.".to_string()
        };
        let _ = tx
            .send(crate::llm::StreamEvent::ContentDelta { delta: msg })
            .await;
        Ok(())
    } else {
        let mut active_goal = match goal_cmd {
            Some(crate::engine::goal_loop::GoalCommandAction::Start { condition }) => {
                let clean_cond = condition
                    .split_whitespace()
                    .filter(|t| !t.eq_ignore_ascii_case("/plan"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let g = crate::engine::goal_loop::GoalState::new(&chat_id, &clean_cond);
                if let Ok(j) = serde_json::to_string(&g) {
                    let _ = ctx.app_state.db.save_goal_state(&chat_id, &j);
                }
                let _ = tx.send(g.to_stream_event()).await;
                Some(g)
            }
            _ => None,
        };
        let mut cur_prompt = if let Some(ref g) = active_goal {
            format!(
                "GOAL MODE ACTIVE (Autonomous Completion Loop).\nCondition to satisfy and prove with actual tool outputs:\n{}",
                g.condition
            )
        } else {
            agent_prompt.clone()
        };
        loop {
            let hist_len_before = history.len();
            let turn_res = crate::engine::single_agent::SingleAgentRunner::run_turn(
                &http_client,
                &chat_id,
                &cur_prompt,
                &workspace_str,
                ws_tools.clone(),
                &key_pool,
                &caps_reg,
                &provider.name,
                &provider.base_url,
                provider.api_protocol.as_deref(),
                &model.id,
                &reasoning_effort,
                None,
                Some(&ctx.app_state.db),
                false,
                &session,
                &mut *contract_lock,
                &mut history,
                tx.clone(),
            )
            .await
            .map(|_| ());
            if let Err(e) = turn_res {
                break Err(e);
            }
            let Some(ref mut g) = active_goal else {
                break Ok(());
            };
            if !session.is_active() {
                break Ok(());
            }
            let used_tools = history[hist_len_before..]
                .iter()
                .any(|m| m.role == "tool" || m.tool_calls.as_ref().map(|v| !v.is_empty()).unwrap_or(false));
            if used_tools {
                g.zero_tool_streak = 0;
            } else {
                g.zero_tool_streak += 1;
            }
            if g.zero_tool_streak >= 2 {
                let _ = tx
                    .send(crate::llm::StreamEvent::ContentDelta {
                        delta: "\n\n**Goal Paused (Stagnation Breaker)**".to_string(),
                    })
                    .await;
                break Ok(());
            }
            let verdict = crate::engine::goal_loop::evaluate_transcript(
                &http_client,
                &provider.name,
                &provider.base_url,
                provider.api_protocol.as_deref(),
                &key_pool,
                &caps_reg,
                &model.id,
                &g.condition,
                &history,
            )
            .await;
            match verdict {
                Ok(crate::engine::goal_loop::EvaluatorVerdict::Met { reason }) => {
                    let _ = ctx.app_state.db.delete_goal_state(&chat_id);
                    let _ = tx
                        .send(crate::llm::StreamEvent::ContentDelta {
                            delta: format!("\n\n**Goal Achieved:** {}", reason),
                        })
                        .await;
                    break Ok(());
                }
                Ok(crate::engine::goal_loop::EvaluatorVerdict::Impossible { reason }) => {
                    let _ = ctx.app_state.db.delete_goal_state(&chat_id);
                    let _ = tx
                        .send(crate::llm::StreamEvent::ContentDelta {
                            delta: format!("\n\n**Goal Failed:** {}", reason),
                        })
                        .await;
                    break Ok(());
                }
                Ok(crate::engine::goal_loop::EvaluatorVerdict::NotYetMet { reason }) => {
                    g.turn += 1;
                    if g.turn > g.max_turns {
                        let _ = tx
                            .send(crate::llm::StreamEvent::ContentDelta {
                                delta: "\n\n**Goal Paused:** Turn budget reached.".to_string(),
                            })
                            .await;
                        break Ok(());
                    }
                    let _ = tx.send(g.to_stream_event()).await;
                    cur_prompt = format!(
                        "[Goal Stop-Hook Feedback — NOT YET MET]\nCondition: {}\nEvaluator Reason: {}\nContinue working.",
                        g.condition, reason
                    );
                }
                Err(eval_err) => {
                    let _ = tx
                        .send(crate::llm::StreamEvent::ContentDelta {
                            delta: format!("\n\n**Goal Paused (Evaluator error):** {}", eval_err),
                        })
                        .await;
                    break Ok(());
                }
            }
        }
    };
    ws_tools.set_plan_mode(false);
    drop(tx);

    ctx.stream_mgr.remove_session_if_current(&chat_id, &session).await;
    drop(contract_lock);
    typing_active.store(false, Ordering::SeqCst);
    typing_task.abort();

    let (final_text, reasoning_text, error_text) = match event_task.await {
        Ok(tuple) => tuple,
        Err(_) => (
            String::new(),
            String::new(),
            Some("Internal task error".to_string()),
        ),
    };

    if !history.is_empty() {
        if let Ok(hist_json) = serde_json::to_string(&history) {
            let _ = ctx.app_state.db.save_checkpoint(&chat_id, &hist_json);
            let _ = ctx.app_state.db.save_checkpoint(&tg_ctx_key, &hist_json);
        }
    }

    if !final_text.trim().is_empty() {
        let asst_msg = crate::db::chats::Message {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id: chat_id.clone(),
            role: "assistant".to_string(),
            content: final_text.clone(),
            model_id: Some(model.id.clone()),
            reasoning_content: if reasoning_text.is_empty() {
                None
            } else {
                Some(reasoning_text)
            },
            tool_calls: None,
            tool_call_id: None,
            sequence_num: seq + 1,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = ctx.app_state.db.create_message(&asst_msg);
        if let Ok(all_db_msgs) = ctx.app_state.db.list_messages(&chat_id) {
            let chat_title = ctx.app_state.db.get_chat(&chat_id).map(|c| c.title).unwrap_or_else(|_| "Telegram Chat".to_string());
            let json_msgs: Vec<_> = all_db_msgs.into_iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "chatId": m.chat_id,
                    "role": m.role,
                    "content": m.content,
                    "modelId": m.model_id,
                    "reasoningContent": m.reasoning_content,
                    "createdAt": m.created_at,
                })
            }).collect();
            if let Ok(j) = serde_json::to_string(&json_msgs) {
                let _ = ctx.app_state.db.sync_chat_context(&chat_id, &chat_title, &j);
            }
        }
        if let Some(ref app) = stream_app_handle {
            let _ = app.emit(
                "telegram-chat-stream",
                json!({
                    "chatId": chat_id,
                    "type": "done",
                    "modelId": model.id,
                    "providerId": provider.id,
                }),
            );
        }
        emit_chat_updated(&ctx, &chat_id).await;

        let media_list = extract_media_attachments(&final_text);
        let clean_text = if media_list.is_empty() {
            final_text.clone()
        } else {
            regex::Regex::new(r"!\[[^\]]*\]\([^)]+\)")
                .map(|re| re.replace_all(&final_text, "").trim().to_string())
                .unwrap_or_else(|_| final_text.clone())
        };
        let is_active = {
            let map = sessions.read().await;
            map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(chat_id.as_str())
        };
        if is_active {
            if !clean_text.trim().is_empty() {
                let html = markdown_to_telegram_html(&clean_text);
                let send_text = if html.trim().is_empty() { escape_html(&clean_text) } else { html };
                if tg_client.send_html(tg_chat_id, &send_text, None).await.is_err() {
                    let _ = tg_client.send_html(tg_chat_id, &escape_html(&clean_text), None).await;
                }
            }
            for (kind, src) in media_list {
                let _ = TelegramUploadManager::enqueue_with_client(
                    tg_client.clone(),
                    tg_chat_id,
                    &chat_id,
                    &src,
                    &kind,
                    None,
                )
                .await;
            }
        } else {
            let chat_title = ctx
                .app_state
                .db
                .get_chat(&chat_id)
                .map(|c| c.title)
                .unwrap_or_else(|_| "Сессия".to_string());
            let preview = compact_text(&clean_text, 140);
            let token = store_choices(&choices_arc, tg_chat_id, "chat", vec![chat_id.clone()]).await;
            let kb = serde_json::json!({
                "inline_keyboard": [[
                    { "text": "💬 Открыть результат", "callback_data": format!("tg:chat:{}:0", token) }
                ]]
            });
            let notif = format!(
                "✅ <b>Фоновая сессия завершена</b>\nСессия: <b>{}</b>\n\n<i>{}</i>",
                escape_html(&compact_text(&chat_title, 42)),
                escape_html(&preview)
            );
            let _ = tg_client.send_html(tg_chat_id, &notif, Some(&kb)).await;
        }
    } else {
        let err_msg = error_text
            .or_else(|| run_res.err())
            .unwrap_or_else(|| "Задача завершена без текстового ответа.".to_string());
        if err_msg == "User canceled execution" {
            if let Some(ref app) = stream_app_handle {
                let _ = app.emit(
                    "telegram-chat-stream",
                    json!({ "chatId": chat_id, "type": "stopped" }),
                );
            }
            emit_chat_updated(&ctx, &chat_id).await;
            return chat_id.clone();
        }
        let asst_msg = crate::db::chats::Message {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id: chat_id.clone(),
            role: "assistant".to_string(),
            content: format!("[Telegram] {}", err_msg),
            model_id: Some(model.id.clone()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            sequence_num: seq + 1,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = ctx.app_state.db.create_message(&asst_msg);
        if let Ok(all_db_msgs) = ctx.app_state.db.list_messages(&chat_id) {
            let chat_title = ctx.app_state.db.get_chat(&chat_id).map(|c| c.title).unwrap_or_else(|_| "Telegram Chat".to_string());
            let json_msgs: Vec<_> = all_db_msgs.into_iter().map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "chatId": m.chat_id,
                    "role": m.role,
                    "content": m.content,
                    "modelId": m.model_id,
                    "reasoningContent": m.reasoning_content,
                    "createdAt": m.created_at,
                })
            }).collect();
            if let Ok(j) = serde_json::to_string(&json_msgs) {
                let _ = ctx.app_state.db.sync_chat_context(&chat_id, &chat_title, &j);
            }
        }
        if let Some(ref app) = stream_app_handle {
            let _ = app.emit(
                "telegram-chat-stream",
                json!({
                    "chatId": chat_id,
                    "type": "done",
                    "modelId": model.id,
                    "providerId": provider.id,
                }),
            );
        }
        emit_chat_updated(&ctx, &chat_id).await;
        let is_active = {
            let map = sessions.read().await;
            map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(chat_id.as_str())
        };
        if is_active {
            let _ = tg_client
                .send_html(tg_chat_id, &escape_html(&format_user_error(&err_msg)), None)
                .await;
        } else {
            let chat_title = ctx
                .app_state
                .db
                .get_chat(&chat_id)
                .map(|c| c.title)
                .unwrap_or_else(|_| "Сессия".to_string());
            let token = store_choices(&choices_arc, tg_chat_id, "chat", vec![chat_id.clone()]).await;
            let kb = serde_json::json!({
                "inline_keyboard": [[
                    { "text": "💬 Открыть сессию", "callback_data": format!("tg:chat:{}:0", token) }
                ]]
            });
            let notif = format!(
                "⚠️ <b>Фоновая сессия завершена с ошибкой</b>\nСессия: <b>{}</b>\n\n<i>{}</i>",
                escape_html(&compact_text(&chat_title, 42)),
                escape_html(&compact_text(&err_msg, 140))
            );
            let _ = tg_client.send_html(tg_chat_id, &notif, Some(&kb)).await;
        }
    }

    chat_id
}

async fn run_tg_turn_draining(
    tg_client: TelegramClient,
    tg_chat_id: i64,
    msg: Value,
    prompt_text: String,
    ctx: TelegramRuntimeContext,
    providers_snap: Arc<RwLock<ProvidersSnapshotFile>>,
    sessions: Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
    choices_arc: Arc<RwLock<HashMap<String, Vec<String>>>>,
    config_arc: Arc<RwLock<TelegramConfigFile>>,
    config_path_arc: Arc<RwLock<Option<PathBuf>>>,
) {
    let mut next_msg = msg;
    let mut next_text = prompt_text;
    loop {
        let chat_id = run_telegram_agent_turn(
            tg_client.clone(),
            tg_chat_id,
            next_msg,
            next_text,
            ctx.clone(),
            providers_snap.clone(),
            sessions.clone(),
            choices_arc.clone(),
            config_arc.clone(),
            config_path_arc.clone(),
        )
        .await;
        if chat_id.is_empty() {
            return;
        }
        let Some(item) = ctx.prompt_queue.take_next(&chat_id).await else {
            return;
        };
        let remaining = ctx.prompt_queue.list(&chat_id).await;
        emit_prompt_queue_updated(&ctx, &chat_id, &remaining).await;
        let is_active = {
            let map = sessions.read().await;
            map.get(&tg_chat_id).and_then(|s| s.active_chat_id.as_deref()) == Some(chat_id.as_str())
        };
        if is_active {
            let _ = tg_client
                .send_html(
                    tg_chat_id,
                    &format!(
                        "▶️ <b>Запускаю следующую задачу из очереди</b>\n\n<code>{}</code>",
                        escape_html(item.text.chars().take(200).collect::<String>().trim())
                    ),
                    None,
                )
                .await;
        }
        next_msg = json!({ "message_id": item.tg_message_id.unwrap_or(0) });
        next_text = item.text;
    }
}

async fn run_telegram_btw(
    tg_client: TelegramClient,
    tg_chat_id: i64,
    user_msg_id: i64,
    question: String,
    ctx: TelegramRuntimeContext,
    providers_snap: Arc<RwLock<ProvidersSnapshotFile>>,
    sessions: Arc<RwLock<HashMap<i64, TelegramChatSession>>>,
) {
    let (provider, model) =
        match resolve_provider_and_model(&ctx, &providers_snap, &sessions, tg_chat_id).await {
            Ok(res) => res,
            Err(e) => {
                let _ = tg_client
                    .send_html(tg_chat_id, &escape_html(&format_user_error(&e)), None)
                    .await;
                return;
            }
        };

    let active_cid = sessions
        .read()
        .await
        .get(&tg_chat_id)
        .and_then(|s| s.active_chat_id.clone());

    let caps_reg = (*ctx.caps_registry).clone();
    caps_reg.update_model_caps(
        &model.id,
        Some(model.context_window),
        Some(model.max_output_tokens),
        Some(model.supports_reasoning),
    );

    let msgs = if let Some(ref cid) = active_cid {
        crate::engine::btw_context::build_btw_messages(
            &ctx.app_state.db,
            &ctx.stream_mgr,
            cid,
            &model.id,
            &caps_reg,
            None,
            &question,
        )
        .await
    } else {
        vec![
            crate::llm::request_compat::ChatMessage::system(
                "You are answering a quick ephemeral side question (/btw). No active chat session is selected. Answer concisely using any information in the question itself or state clearly that no active session context is selected.".to_string(),
            ),
            crate::llm::request_compat::ChatMessage::user(question),
        ]
    };

    let key_pool = (*ctx.key_pool).clone();
    if !provider.api_key.trim().is_empty() {
        key_pool.set_keys(&provider.name, vec![provider.api_key.trim().to_string()]);
    }
    let req = crate::llm::request_compat::ChatRequest {
        model: model.id.clone(),
        messages: msgs,
        temperature: Some(0.3),
        top_p: None,
        max_tokens: Some(4096),
        max_completion_tokens: Some(4096),
        reasoning_effort: None,
        thinking_budget: None,
        tools: None,
        tool_choice: None,
        response_format: None,
        stream: true,
        prompt_cache_key: None,
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let client = ctx.http_client.clone();
    let pname = provider.name.clone();
    let burl = provider.base_url.clone();
    let proto = provider.api_protocol.clone();
    tokio::spawn(async move {
        let _ = crate::llm::dispatch_stream_with_protocol(
            &client,
            &pname,
            &burl,
            proto.as_deref(),
            &key_pool,
            &caps_reg,
            req,
            tx,
        )
        .await;
    });

    let mut answer = String::new();
    let mut err_msg: Option<String> = None;
    while let Some(ev) = rx.recv().await {
        match ev {
            crate::llm::StreamEvent::ContentDelta { delta } => {
                answer.push_str(&delta);
            }
            crate::llm::StreamEvent::TurnBufferReset => {
                answer.clear();
            }
            crate::llm::StreamEvent::RetroactiveReasoningMove { text } => {
                if answer.ends_with(&text) {
                    answer.truncate(answer.len() - text.len());
                }
            }
            crate::llm::StreamEvent::Error { error } => {
                err_msg = Some(error);
            }
            _ => {}
        }
    }
    if answer.trim().is_empty() {
        answer = match err_msg {
            Some(e) if !e.trim().is_empty() => format!("Ошибка /btw: {}", format_user_error(&e)),
            _ => "Не удалось получить ответ на /btw.".to_string(),
        };
    }

    let kb = json!({
        "inline_keyboard": [[{
            "text": "🗑 Удалить",
            "callback_data": format!("btw_del:{}", user_msg_id)
        }]]
    });
    let html = format!("<b>/btw</b>

{}", markdown_to_telegram_html(&answer));
    let _ = tg_client.send_html(tg_chat_id, &html, Some(&kb)).await;
}


fn extract_media_attachments(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push_unique = |kind: &str, src: String| {
        let s = src.trim().to_string();
        if s.is_empty() || s.starts_with("data:") {
            return;
        }
        if !out.iter().any(|(_, existing)| existing == &s) && out.len() < 10 {
            out.push((kind.to_string(), s));
        }
    };

    if let Ok(re_img_tag) = regex::Regex::new(r#"(?i)<img[^>]*src=["']([^"']+)["'][^>]*>"#) {
        for cap in re_img_tag.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                push_unique("photo", m.as_str().to_string());
            }
        }
    }
    if let Ok(re_aud_tag) = regex::Regex::new(r#"(?i)<audio[^>]*src=["']([^"']+)["'][^>]*>"#) {
        for cap in re_aud_tag.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                push_unique("audio", m.as_str().to_string());
            }
        }
    }
    if let Ok(re_vid_tag) = regex::Regex::new(r#"(?i)<video[^>]*src=["']([^"']+)["'][^>]*>"#) {
        for cap in re_vid_tag.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                push_unique("video", m.as_str().to_string());
            }
        }
    }
    if let Ok(re_md_img) = regex::Regex::new(r#"!\[[^\]]*\]\(([^)\s]+)\)"#) {
        for cap in re_md_img.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                let url = m.as_str();
                let lower = url.to_ascii_lowercase();
                if [".mp3", ".wav", ".ogg", ".m4a", ".opus", ".flac"].iter().any(|ext| lower.contains(ext)) {
                    push_unique("audio", url.to_string());
                } else if [".mp4", ".webm", ".mov", ".mkv"].iter().any(|ext| lower.contains(ext)) {
                    push_unique("video", url.to_string());
                } else {
                    push_unique("photo", url.to_string());
                }
            }
        }
    }
    if let Ok(re_md_link) = regex::Regex::new(r#"(?:^|[^!])\[[^\]]*\]\(([^)\s]+)\)"#) {
        for cap in re_md_link.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                let url = m.as_str();
                let lower = url.to_ascii_lowercase();
                if [".mp3", ".wav", ".ogg", ".m4a", ".opus", ".flac"].iter().any(|ext| lower.ends_with(ext)) {
                    push_unique("audio", url.to_string());
                } else if [".mp4", ".webm", ".mov", ".mkv"].iter().any(|ext| lower.ends_with(ext)) {
                    push_unique("video", url.to_string());
                } else if [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|ext| lower.ends_with(ext)) {
                    push_unique("photo", url.to_string());
                }
            }
        }
    }
    out
}
