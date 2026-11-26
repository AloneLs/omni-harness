pub mod db;
pub mod engine;
pub mod error;
pub mod llm;
pub mod logger;
pub mod mcp;
pub mod scheduler;
pub mod skills;
pub mod speech;
pub mod state;
mod themes;
pub mod telegram;
pub mod tools;
pub mod updater;

use engine::task_contract::TaskContract;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State};
use tokio::sync::RwLock;

pub struct AppManagedState {
    pub app_state: Arc<state::AppState>,
    pub stream_mgr: Arc<engine::StreamManager>,
    pub mcp_catalog: Arc<mcp::McpServerCatalog>,
    pub telegram_mgr: Arc<telegram::TelegramBotManager>,
    pub speech_engine: Arc<speech::SpeechEngine>,
    pub prompt_queue: Arc<engine::prompt_queue::PromptQueue>,
    pub tool_dispatcher: Arc<tools::ToolDispatcher>,
    pub key_pool: Arc<llm::KeyPool>,
    pub caps_registry: Arc<llm::CapabilityRegistry>,
    pub http_client: reqwest::Client,
    pub active_contracts: Arc<RwLock<std::collections::HashMap<String, Arc<tokio::sync::Mutex<Option<TaskContract>>>>>>,
}

impl AppManagedState {
    pub fn has_active_tasks(&self) -> bool {
        self.stream_mgr.has_running_sessions_sync() || self.telegram_mgr.is_running_sync()
    }
}

#[cfg(target_os = "windows")]
static SYS_PROXY_CACHE: std::sync::OnceLock<std::sync::RwLock<(Option<String>, std::time::Instant)>> = std::sync::OnceLock::new();

#[cfg(target_os = "windows")]
pub fn resolve_windows_system_proxy() -> Option<String> {
    let cache_lock = SYS_PROXY_CACHE.get_or_init(|| {
        std::sync::RwLock::new((None, std::time::Instant::now() - std::time::Duration::from_secs(10)))
    });
    if let Ok(guard) = cache_lock.read() {
        if guard.1.elapsed() < std::time::Duration::from_secs(3) {
            return guard.0.clone();
        }
    }
    use std::os::windows::process::CommandExt;
    let fresh_val = (|| {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            "ProxyEnable",
        ])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let en_txt = String::from_utf8_lossy(&out.stdout);
    let enabled = en_txt.contains("0x1") || en_txt.lines().any(|l| l.trim().ends_with(" 1"));
    if !enabled {
        return None;
    }
    let out2 = std::process::Command::new("reg")
        .args([
            "query",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            "ProxyServer",
        ])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let srv_txt = String::from_utf8_lossy(&out2.stdout);
    let line = srv_txt.lines().find(|l| l.contains("ProxyServer"))?;
    let raw = line.split_whitespace().last()?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut picked = raw.to_string();
    let mut is_socks = false;
    if raw.contains('=') {
        let parts: Vec<&str> = raw.split(';').collect();
        if let Some(s) = parts.iter().find(|x| x.to_ascii_lowercase().starts_with("socks=")) {
            if let Some((_, v)) = s.split_once('=') {
                picked = v.to_string();
                is_socks = true;
            }
        } else if let Some(h) = parts.iter().find(|x| x.to_ascii_lowercase().starts_with("https=")) {
            if let Some((_, v)) = h.split_once('=') {
                picked = v.to_string();
            }
        } else if let Some(h) = parts.iter().find(|x| x.to_ascii_lowercase().starts_with("http=")) {
            if let Some((_, v)) = h.split_once('=') {
                picked = v.to_string();
            }
        }
    } else if raw.to_ascii_lowercase().contains("socks") {
        is_socks = true;
    }
    let lower = picked.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("socks") {
        Some(picked)
    } else if is_socks {
        Some(format!("socks5h://{}", picked))
    } else {
        Some(format!("http://{}", picked))
    }
    })();

    if let Ok(mut guard) = cache_lock.write() {
        *guard = (fresh_val.clone(), std::time::Instant::now());
    }
    fresh_val
}

#[cfg(not(target_os = "windows"))]
pub fn resolve_windows_system_proxy() -> Option<String> {
    None
}

fn is_direct_image_model(model_id: &str) -> bool {
    let m = model_id.to_ascii_lowercase();
    m.contains("-image")
        || m.ends_with("image")
        || m.contains("imagen")
        || m.contains("dall-e")
        || m.contains("flux")
        || m.contains("imagine-image")
}

pub fn build_http_client(
    proxy_enabled: Option<bool>,
    proxy_url: Option<&str>,
    fallback: Option<&reqwest::Client>,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .tcp_keepalive(std::time::Duration::from_secs(15))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(10)
        .http2_keep_alive_interval(std::time::Duration::from_secs(15))
        .http2_keep_alive_timeout(std::time::Duration::from_secs(10))
        .http2_keep_alive_while_idle(true);
    let explicit_proxy = if proxy_enabled.unwrap_or(false) {
        proxy_url.map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string())
    } else {
        let (global_en, global_url) = llm::oauth_accounts::resolve_global_proxy_for_api();
        if global_en == Some(true) {
            global_url
        } else {
            None
        }
    };

    if let Some(ref purl) = explicit_proxy {
        let proxy = reqwest::Proxy::all(purl)
            .map_err(|e| format!("Invalid proxy URL: {}", e))?
            .no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1,::1,*.local,*.internal"));
        return builder.proxy(proxy).build().map_err(|e| e.to_string());
    }
    if let Some(sys_proxy) = resolve_windows_system_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(&sys_proxy) {
            builder = builder.proxy(
                proxy.no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1,::1,*.local,*.internal"))
            );
            return builder.build().map_err(|e| e.to_string());
        }
    }
    if let Some(c) = fallback {
        return Ok(c.clone());
    }
    builder.build().map_err(|e| e.to_string())
}

#[tauri::command]
fn window_show(app: tauri::AppHandle, window: tauri::Window) {
    if let Some(icon) = app.default_window_icon() {
        let _ = window.set_icon(icon.clone());
    }
    let _ = window.show();
    let _ = window.set_focus();
}

#[tauri::command]
fn window_minimize(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
fn window_toggle_fullscreen(window: tauri::Window) -> Result<bool, String> {
    let is_fs = window.is_fullscreen().map_err(|e| e.to_string())?;
    window.set_fullscreen(!is_fs).map_err(|e| e.to_string())?;
    Ok(!is_fs)
}

#[tauri::command]
fn window_toggle_maximize(window: tauri::Window) {
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
fn window_close(window: tauri::Window) {
    let _ = window.close();
}

#[tauri::command]
fn window_start_dragging(window: tauri::Window) {
    let _ = window.start_dragging();
}

#[tauri::command]
async fn create_project(
    state: State<'_, AppManagedState>,
    project: db::chats::Project,
) -> Result<db::chats::Project, String> {
    state
        .app_state
        .db
        .create_project(&project)
        .map_err(|e| e.to_string())?;
    Ok(project)
}

#[tauri::command]
async fn list_projects(
    state: State<'_, AppManagedState>,
) -> Result<Vec<db::chats::Project>, String> {
    state.app_state.db.list_projects().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_project(
    state: State<'_, AppManagedState>,
    id: String,
) -> Result<db::chats::Project, String> {
    state
        .app_state
        .db
        .get_project(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_project(state: State<'_, AppManagedState>, id: String) -> Result<(), String> {
    state
        .app_state
        .db
        .delete_project(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_chat(
    state: State<'_, AppManagedState>,
    chat: db::chats::Chat,
) -> Result<db::chats::Chat, String> {
    state
        .app_state
        .db
        .create_chat(&chat)
        .map_err(|e| e.to_string())?;
    Ok(chat)
}

#[tauri::command]
async fn list_chats(
    state: State<'_, AppManagedState>,
    project_id: Option<String>,
) -> Result<Vec<db::chats::Chat>, String> {
    state
        .app_state
        .db
        .list_chats(project_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_chat(state: State<'_, AppManagedState>, id: String) -> Result<db::chats::Chat, String> {
    state.app_state.db.get_chat(&id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_chat_title(
    state: State<'_, AppManagedState>,
    id: String,
    title: String,
) -> Result<(), String> {
    state
        .app_state
        .db
        .update_chat_title(&id, &title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_chat_pinned(
    state: State<'_, AppManagedState>,
    id: String,
    is_pinned: bool,
) -> Result<(), String> {
    state
        .app_state
        .db
        .update_chat_pinned(&id, is_pinned)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_chat(state: State<'_, AppManagedState>, id: String) -> Result<(), String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("Invalid chat id".to_string());
    }
    let _ = state.stream_mgr.cancel_session(&id).await;
    let _ = state.tool_dispatcher.pty_manager.kill_all_for_chat(&id);
    let chat_ws = state::AppState::resolve_install_workspace().join(&id);
    if chat_ws.exists() {
        let _ = std::fs::remove_dir_all(&chat_ws);
    }
    let chat_tmp = state::AppState::resolve_chat_temp_dir(Some(&id));
    if chat_tmp.exists() {
        let _ = std::fs::remove_dir_all(&chat_tmp);
    }
    state
        .app_state
        .db
        .delete_chat(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn read_file_full(
    state: State<'_, AppManagedState>,
    path: String,
    workspace_dir: Option<String>,
    chat_id: Option<String>,
) -> Result<String, String> {
    let raw = path.trim();
    let direct = std::path::PathBuf::from(raw);
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if direct.is_absolute() {
        candidates.push(direct);
    } else {
        if let Some(wd) = workspace_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            candidates.push(std::path::PathBuf::from(wd).join(raw));
        }
        if let Some(cid) = chat_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if let Ok(chat) = state.app_state.db.get_chat(cid) {
                if let Some(proj) = chat.project_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    candidates.push(std::path::PathBuf::from(proj).join(raw));
                }
            }
            candidates.push(state::AppState::resolve_install_workspace().join(cid).join(raw));
        }
        let ws_dir = state.app_state.workspace_dir.read().await.clone();
        if !ws_dir.as_os_str().is_empty() {
            candidates.push(ws_dir.join(raw));
        }
        candidates.push(state::AppState::resolve_data_dir().join(raw));
        candidates.push(direct);
    }

    let mut chosen: Option<std::path::PathBuf> = None;
    for cand in &candidates {
        tools::safety::SecretShield::check_path(cand)?;
        if cand.is_file() {
            chosen = Some(cand.clone());
            break;
        }
    }
    let target = chosen.unwrap_or_else(|| candidates.into_iter().next().unwrap_or_else(|| std::path::PathBuf::from(raw)));
    tools::safety::SecretShield::check_path(&target)?;
    tokio::task::spawn_blocking(move || std::fs::read_to_string(&target).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn create_message(
    state: State<'_, AppManagedState>,
    message: db::chats::Message,
) -> Result<db::chats::Message, String> {
    state
        .app_state
        .db
        .create_message(&message)
        .map_err(|e| e.to_string())?;
    Ok(message)
}

#[tauri::command]
async fn list_messages(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Vec<db::chats::Message>, String> {
    state
        .app_state
        .db
        .list_messages(&chat_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_messages_since(
    state: State<'_, AppManagedState>,
    chat_id: String,
    sequence_num: i64,
) -> Result<(), String> {
    state
        .app_state
        .db
        .delete_messages_since(&chat_id, sequence_num)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_message(state: State<'_, AppManagedState>, id: String) -> Result<(), String> {
    state
        .app_state
        .db
        .delete_message(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn record_file_snapshot(
    state: State<'_, AppManagedState>,
    snapshot: db::FileSnapshot,
) -> Result<(), String> {
    state
        .app_state
        .db
        .record_file_snapshot(&snapshot)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_file_snapshot(
    state: State<'_, AppManagedState>,
    snapshot_id: String,
) -> Result<db::FileSnapshot, String> {
    state
        .app_state
        .db
        .get_file_snapshot(&snapshot_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_snapshots_for_chat(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Vec<db::FileSnapshot>, String> {
    state
        .app_state
        .db
        .list_snapshots_for_chat(&chat_id)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnFileDiff {
    path: String,
    before_text: String,
    after_text: String,
    added_lines: i64,
    removed_lines: i64,
    existed_before: bool,
}

#[tauri::command]
async fn get_turn_file_diffs(
    state: State<'_, AppManagedState>,
    snapshot_ids: Vec<String>,
) -> Result<Vec<TurnFileDiff>, String> {
    let mut order: Vec<String> = Vec::new();
    let mut by_path: std::collections::HashMap<String, (Option<Vec<u8>>, Option<Vec<u8>>, bool)> =
        std::collections::HashMap::new();

    for sid in snapshot_ids {
        if let Ok(snap) = state.app_state.db.get_file_snapshot(&sid) {
            let entry = by_path.entry(snap.file_path.clone()).or_insert_with(|| {
                order.push(snap.file_path.clone());
                (snap.before_blob.clone(), snap.after_blob.clone(), snap.existed_before)
            });
            if snap.after_blob.is_some() {
                entry.1 = snap.after_blob;
            }
            entry.2 = entry.2 || snap.existed_before;
        }
    }

    let mut out = Vec::with_capacity(order.len());
    for path in order {
        let Some((before_b, after_b, existed_before)) = by_path.remove(&path) else { continue };
            let before_text = before_b
                .as_deref()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .unwrap_or_default();
            let after_text = after_b
                .as_deref()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .unwrap_or_default();
            let blobs_known = before_b.is_some() && after_b.is_some();
            let (added, removed) = if existed_before && before_b.is_some() {
                crate::tools::workspace::compute_line_diff(&before_text, &after_text)
            } else if after_b.is_some() {
                (after_text.lines().count() as i64, 0)
            } else {
                (0, before_text.lines().count() as i64)
            };
            if blobs_known && added == 0 && removed == 0 {
                continue;
            }
            out.push(TurnFileDiff {
                path,
                before_text,
                after_text,
                added_lines: added,
                removed_lines: removed,
                existed_before,
            });
    }
    Ok(out)
}

#[tauri::command]
async fn rollback_file_snapshot(
    state: State<'_, AppManagedState>,
    snapshot_id: String,
) -> Result<db::FileSnapshot, String> {
    state
        .app_state
        .db
        .rollback_file_snapshot(&snapshot_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_events_since(
    state: State<'_, AppManagedState>,
    session_id: String,
    last_seq: i64,
) -> Result<Vec<db::EventRecord>, String> {
    state
        .app_state
        .db
        .list_events_since(&session_id, last_seq)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_session_checkpoints(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Vec<db::CheckpointRecord>, String> {
    state
        .app_state
        .db
        .list_checkpoints(&chat_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rewind_checkpoint(
    state: State<'_, AppManagedState>,
    chat_id: String,
    checkpoint_id: String,
    mode: String,
) -> Result<tools::blob_store::RewindResult, String> {
    let cp = state
        .app_state
        .db
        .get_checkpoint(&checkpoint_id)
        .map_err(|e| e.to_string())?;

    let norm_mode = mode.trim().to_lowercase();
    let do_code = norm_mode == "code" || norm_mode == "both";
    let do_conv = norm_mode == "conversation" || norm_mode == "both";
    if !do_code && !do_conv {
        return Err("Invalid rewind mode. Use code, conversation, or both.".to_string());
    }

    let mut result = tools::blob_store::RewindResult {
        checkpoint_id: checkpoint_id.clone(),
        mode: norm_mode.clone(),
        restored_files: Vec::new(),
        deleted_files: Vec::new(),
        skipped_large_files: Vec::new(),
        dirty_external_files: Vec::new(),
    };

    if do_code {
        let manifest: tools::blob_store::CheckpointManifest =
            serde_json::from_str(&cp.manifest_json).map_err(|e| e.to_string())?;
        let mut expected_hashes: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        if let Ok(evts) = state.app_state.db.list_events_since(&chat_id, 0) {
            for ev in evts {
                if ev.kind == "file_patched" {
                    if let (Some(fp), Some(ah)) = (
                        ev.payload.get("path").and_then(|v| v.as_str()),
                        ev.payload.get("afterHash").and_then(|v| v.as_str()),
                    ) {
                        if !fp.is_empty() && !ah.is_empty() {
                            let rel = std::path::Path::new(fp)
                                .strip_prefix(&manifest.workspace_root)
                                .map(|r| r.to_string_lossy().replace('\\', "/"))
                                .unwrap_or_else(|_| fp.replace('\\', "/"));
                            expected_hashes.insert(rel, ah.to_string());
                        }
                    }
                }
            }
        }
        let code_res = tools::blob_store::atomic_restore_batch(&manifest, Some(&expected_hashes))?;
        result.restored_files = code_res.restored_files;
        result.deleted_files = code_res.deleted_files;
        result.skipped_large_files = code_res.skipped_large_files;
        result.dirty_external_files = code_res.dirty_external_files;
    }

    if do_conv {
        state
            .app_state
            .db
            .truncate_messages_after_seq(&chat_id, cp.message_seq)
            .map_err(|e| e.to_string())?;
    }

    state.app_state.db.emit_event(
        &chat_id,
        "rewind",
        serde_json::json!({
            "checkpointId": checkpoint_id,
            "mode": norm_mode,
            "restoredFiles": result.restored_files,
            "skippedLargeFiles": result.skipped_large_files,
            "dirtyExternalFiles": result.dirty_external_files
        }),
    );

    Ok(result)
}

#[tauri::command]
async fn record_usage_stat(
    state: State<'_, AppManagedState>,
    stat: db::chats::ModelUsageStat,
) -> Result<(), String> {
    state
        .app_state
        .db
        .record_usage_stat(&stat)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_usage_stats(
    state: State<'_, AppManagedState>,
    provider_id: Option<String>,
    model_id: Option<String>,
) -> Result<Vec<db::chats::ModelUsageStat>, String> {
    state
        .app_state
        .db
        .list_usage_stats(provider_id.as_deref(), model_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_provider(
    state: State<'_, AppManagedState>,
    provider: db::settings::Provider,
) -> Result<(), String> {
    state
        .app_state
        .db
        .create_provider(&provider)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_providers(
    state: State<'_, AppManagedState>,
) -> Result<Vec<db::settings::Provider>, String> {
    state
        .app_state
        .db
        .list_providers()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_provider(
    state: State<'_, AppManagedState>,
    id: String,
) -> Result<db::settings::Provider, String> {
    state
        .app_state
        .db
        .get_provider(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_provider(
    state: State<'_, AppManagedState>,
    provider: db::settings::Provider,
) -> Result<(), String> {
    state
        .app_state
        .db
        .update_provider(&provider)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_provider(state: State<'_, AppManagedState>, id: String) -> Result<(), String> {
    state
        .app_state
        .db
        .delete_provider(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn ping_provider(state: State<'_, AppManagedState>, id: String) -> Result<bool, String> {
    let provider = state
        .app_state
        .db
        .get_provider(&id)
        .map_err(|e| e.to_string())?;

    let client = reqwest::Client::new();
    let res = client
        .get(&provider.base_url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;

    Ok(res.is_ok())
}

#[tauri::command]
async fn create_model(
    state: State<'_, AppManagedState>,
    model: db::settings::Model,
) -> Result<(), String> {
    state
        .app_state
        .db
        .create_model(&model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_models(
    state: State<'_, AppManagedState>,
    provider_id: Option<String>,
) -> Result<Vec<db::settings::Model>, String> {
    state
        .app_state
        .db
        .list_models(provider_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_model(
    state: State<'_, AppManagedState>,
    id: String,
) -> Result<db::settings::Model, String> {
    state
        .app_state
        .db
        .get_model(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_model(
    state: State<'_, AppManagedState>,
    model: db::settings::Model,
) -> Result<(), String> {
    state
        .app_state
        .db
        .update_model(&model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_model(state: State<'_, AppManagedState>, id: String) -> Result<(), String> {
    state
        .app_state
        .db
        .delete_model(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_mcp_server(
    state: State<'_, AppManagedState>,
    server: db::settings::McpServer,
) -> Result<(), String> {
    state
        .app_state
        .db
        .create_mcp_server(&server)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_mcp_servers(
    state: State<'_, AppManagedState>,
) -> Result<Vec<db::settings::McpServer>, String> {
    state
        .app_state
        .db
        .list_mcp_servers()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_mcp_server(
    state: State<'_, AppManagedState>,
    id: String,
) -> Result<db::settings::McpServer, String> {
    state
        .app_state
        .db
        .get_mcp_server(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_mcp_server(
    state: State<'_, AppManagedState>,
    server: db::settings::McpServer,
) -> Result<(), String> {
    state
        .app_state
        .db
        .update_mcp_server(&server)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_mcp_server(state: State<'_, AppManagedState>, id: String) -> Result<(), String> {
    state
        .app_state
        .db
        .delete_mcp_server(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect_mcp_server(
    state: State<'_, AppManagedState>,
    server: mcp::McpServerConfig,
) -> Result<(), String> {
    state.mcp_catalog.connect_server(&server).await
}

#[tauri::command]
async fn list_mcp_tools(
    state: State<'_, AppManagedState>,
) -> Result<Vec<mcp::McpToolWithServer>, String> {
    Ok(state.mcp_catalog.list_all_tools().await)
}

#[tauri::command]
async fn disconnect_mcp_server(
    state: State<'_, AppManagedState>,
    name: String,
) -> Result<(), String> {
    state.mcp_catalog.disconnect_server(&name).await
}

#[tauri::command]
async fn import_mcp_config(
    state: State<'_, AppManagedState>,
    json_config: String,
) -> Result<usize, String> {
    let configs = mcp::McpServerCatalog::import_json_config(&json_config)?;
    let count = configs.len();
    for cfg in configs {
        let _ = state.mcp_catalog.connect_server(&cfg).await;
    }
    Ok(count)
}

#[tauri::command]
async fn call_mcp_tool(
    state: State<'_, AppManagedState>,
    server_name: String,
    tool_name: String,
    arguments: Option<serde_json::Value>,
) -> Result<mcp::McpCallToolResult, String> {
    state
        .mcp_catalog
        .call_tool(&server_name, &tool_name, arguments)
        .await
}

#[tauri::command]
async fn create_memory_entry(
    state: State<'_, AppManagedState>,
    entry: db::memory::MemoryEntry,
) -> Result<(), String> {
    state
        .app_state
        .db
        .create_memory_entry(&entry)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_memory_entries(
    state: State<'_, AppManagedState>,
    category: Option<String>,
) -> Result<Vec<db::memory::MemoryEntry>, String> {
    state
        .app_state
        .db
        .list_memory_entries(category.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_memory_entry(
    state: State<'_, AppManagedState>,
    id: String,
) -> Result<db::memory::MemoryEntry, String> {
    state
        .app_state
        .db
        .get_memory_entry(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_memory_entry(
    state: State<'_, AppManagedState>,
    entry: db::memory::MemoryEntry,
) -> Result<(), String> {
    state
        .app_state
        .db
        .update_memory_entry(&entry)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_memory_entry(state: State<'_, AppManagedState>, id: String) -> Result<(), String> {
    state
        .app_state
        .db
        .delete_memory_entry(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_memory_entries(
    state: State<'_, AppManagedState>,
    query: String,
) -> Result<Vec<db::memory::MemoryEntry>, String> {
    state
        .app_state
        .db
        .search_memory_entries(&query)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_total_memory_tokens(state: State<'_, AppManagedState>) -> Result<i64, String> {
    state
        .app_state
        .db
        .get_total_memory_tokens()
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CompactMemoryResult {
    pruned_count: usize,
    saved_tokens: i64,
}

#[tauri::command]
async fn compact_memory_entries(
    state: State<'_, AppManagedState>,
    max_tokens: i64,
) -> Result<CompactMemoryResult, String> {
    let (pruned_count, saved_tokens) = state
        .app_state
        .db
        .compact_memory_entries(max_tokens)
        .map_err(|e| e.to_string())?;
    Ok(CompactMemoryResult {
        pruned_count,
        saved_tokens,
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceFileEntry {
    path: String,
    name: String,
    is_directory: bool,
    size: Option<u64>,
    extension: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSearchResult {
    path: String,
    name: String,
    match_line: Option<usize>,
    match_snippet: Option<String>,
    is_directory: bool,
}

#[tauri::command]
async fn list_workspace_files(path: String) -> Result<Vec<WorkspaceFileEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let root = std::path::PathBuf::from(&path);
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        let walker = ignore::WalkBuilder::new(&root)
            .max_depth(Some(4))
            .hidden(false)
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                n != ".git" && n != "node_modules" && n != "target"
            })
            .build();
        for entry in walker.flatten() {
            let p = entry.path();
            if p == root || tools::safety::SecretShield::check_path(p).is_err() {
                continue;
            }
            let is_directory = p.is_dir();
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let size = if is_directory {
                None
            } else {
                entry.metadata().ok().map(|m| m.len())
            };
            let extension = if is_directory {
                None
            } else {
                p.extension().and_then(|e| e.to_str()).map(|s| s.to_string())
            };
            entries.push(WorkspaceFileEntry {
                path: p.to_string_lossy().to_string(),
                name,
                is_directory,
                size,
                extension,
            });
            if entries.len() >= 1000 {
                break;
            }
        }
        Ok(entries)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn search_workspace_files(
    path: String,
    query: String,
) -> Result<Vec<WorkspaceSearchResult>, String> {
    tokio::task::spawn_blocking(move || {
        let root = std::path::PathBuf::from(&path);
        let q = query.trim().to_lowercase();
        if !root.is_dir() || q.is_empty() {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        let walker = ignore::WalkBuilder::new(&root)
            .max_depth(Some(6))
            .hidden(false)
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                n != ".git" && n != "node_modules" && n != "target"
            })
            .build();
        for entry in walker.flatten() {
            let p = entry.path();
            if p == root || tools::safety::SecretShield::check_path(p).is_err() {
                continue;
            }
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if name.to_lowercase().contains(&q) {
                results.push(WorkspaceSearchResult {
                    path: p.to_string_lossy().to_string(),
                    name,
                    match_line: None,
                    match_snippet: None,
                    is_directory: p.is_dir(),
                });
                if results.len() >= 200 {
                    break;
                }
            }
        }
        Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn sync_chat_context(
    state: State<'_, AppManagedState>,
    chat_id: String,
    title: String,
    messages_json: String,
    project_path: Option<String>,
    model_id: Option<String>,
    is_pinned: Option<bool>,
    streaming_only: Option<bool>,
) -> Result<(), String> {
    let db = &state.app_state.db;
    db.sync_chat_context(&chat_id, &title, &messages_json)
        .map_err(|e| e.to_string())?;
    let is_streaming = streaming_only.unwrap_or(false);

    let has_messages = serde_json::from_str::<Vec<serde_json::Value>>(&messages_json)
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let resolved_title = if title.trim().is_empty() || title == "New task" {
        if !has_messages {
            return Ok(());
        }
        "Chat".to_string()
    } else {
        title.clone()
    };

    let now = chrono::Utc::now().to_rfc3339();
    if let Ok(existing) = db.get_chat(&chat_id) {
        if !title.trim().is_empty() && title != "New task" && existing.title != title {
            let _ = db.update_chat_title(&chat_id, &title);
        }
        let next_proj = project_path.as_deref().map(|x| x.trim()).filter(|x| !x.is_empty());
        if existing.project_id.as_deref() != next_proj {
            let _ = db.update_chat_project(&chat_id, next_proj);
        }
        if let Some(mid) = model_id.as_deref().map(|x| x.trim()).filter(|x| !x.is_empty()) {
            if existing.model_id != mid {
                let _ = db.update_chat_model(&chat_id, mid);
            }
        }
        if let Some(pinned) = is_pinned {
            if existing.is_pinned != pinned {
                let _ = db.update_chat_pinned(&chat_id, pinned);
            }
        }
    } else {
        let chat = db::chats::Chat {
            id: chat_id.clone(),
            project_id: project_path.clone().filter(|s| !s.trim().is_empty()),
            title: resolved_title,
            model_id: model_id.clone().unwrap_or_default(),
            system_prompt: String::new(),
            is_pinned: is_pinned.unwrap_or(false),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let _ = db.create_chat(&chat);
    }

    if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(&messages_json) {
        let clean_pairs: Vec<(String, String, Option<String>)> = parsed
            .iter()
            .filter_map(|item| {
                let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user").trim().to_string();
                let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let msg_model = item
                    .get("modelId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let has_work = item
                    .get("steps")
                    .and_then(|v| v.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false)
                    || item
                        .get("toolCalls")
                        .and_then(|v| v.as_array())
                        .map(|a| !a.is_empty())
                        .unwrap_or(false)
                    || item.get("paused").and_then(|v| v.as_bool()).unwrap_or(false);
                if content.trim().is_empty() && !has_work {
                    None
                } else {
                    Some((role, content, msg_model))
                }
            })
            .collect();
        let existing = db.list_messages(&chat_id).unwrap_or_default();
        let prefix_matches = clean_pairs.len() >= existing.len()
            && existing
                .iter()
                .zip(clean_pairs.iter())
                .all(|(ex, (r, c, _))| ex.role == *r && ex.content.trim() == c.trim());

        if prefix_matches {
            if clean_pairs.len() > existing.len() {
                let mut seq = existing.last().map(|m| m.sequence_num).unwrap_or(0);
                for (role, content, msg_model) in clean_pairs.into_iter().skip(existing.len()) {
                    seq += 1;
                    let msg = db::chats::Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        chat_id: chat_id.clone(),
                        role,
                        content,
                        model_id: msg_model,
                        reasoning_content: None,
                        tool_calls: None,
                        tool_call_id: None,
                        sequence_num: seq,
                        created_at: now.clone(),
                    };
                    let _ = db.create_message(&msg);
                }
            }
        } else if !is_streaming {
            let _ = db.replace_chat_messages(&chat_id, &clean_pairs);
            let _ = db.delete_checkpoint(&format!("tg_ctx:{}", chat_id));
            let _ = db.delete_checkpoint(&format!("full_ctx:{}", chat_id));
        }
    }
    if is_streaming {
        db.checkpoint_wal();
    }
    Ok(())
}

#[tauri::command]
async fn sync_workspaces_to_backend(
    state: State<'_, AppManagedState>,
    workspaces: Vec<telegram::SyncedProject>,
) -> Result<(), String> {
    state.telegram_mgr.sync_projects(workspaces).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatRecoverySnapshot {
    title: String,
    messages_json: String,
    updated_at: String,
    recovered_from_checkpoint: bool,
}

fn convert_checkpoint_to_ui_messages(
    chat_id: &str,
    checkpoint_json: &str,
    cached_json: Option<&str>,
    model_id: Option<&str>,
) -> Option<String> {
    let llm_msgs: Vec<llm::request_compat::ChatMessage> = serde_json::from_str(checkpoint_json).ok()?;
    if llm_msgs.is_empty() {
        return None;
    }

    let cached_arr: Vec<serde_json::Value> = cached_json
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let mut ui_msgs: Vec<serde_json::Value> = Vec::new();
    let mut tool_results: std::collections::HashMap<String, (String, bool)> = std::collections::HashMap::new();
    for m in &llm_msgs {
        if m.role == "tool" {
            if let Some(id) = &m.tool_call_id {
                let text = m.content.as_text_lossy();
                let is_err = text.starts_with("Error:") || text.contains("\"error\":");
                tool_results.insert(id.clone(), (text, is_err));
            }
        }
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut active_assistant: Option<serde_json::Value> = None;

    let flush_assistant = |acc: &mut Option<serde_json::Value>, out: &mut Vec<serde_json::Value>| {
        if let Some(mut msg) = acc.take() {
            let cached_match = cached_arr.get(out.len());
            if let Some(cm) = cached_match {
                if cm.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                    if let Some(id) = cm.get("id").and_then(|v| v.as_str()) {
                        msg["id"] = serde_json::Value::String(id.to_string());
                    }
                    if let Some(ca) = cm.get("createdAt").and_then(|v| v.as_i64()) {
                        msg["createdAt"] = serde_json::json!(ca);
                    }
                }
            }
            out.push(msg);
        }
    };

    for m in &llm_msgs {
        if m.role == "system" || m.role == "tool" {
            continue;
        }
        if m.role == "user" {
            flush_assistant(&mut active_assistant, &mut ui_msgs);
            let idx = ui_msgs.len();
            let cached_match = cached_arr.get(idx);
            let id = cached_match
                .and_then(|cm| cm.get("id").and_then(|v| v.as_str()))
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("rec-u-{}", idx));
            let created_at = cached_match
                .and_then(|cm| cm.get("createdAt").and_then(|v| v.as_i64()))
                .unwrap_or(now_ms);
            let mut obj = serde_json::json!({
                "id": id,
                "chatId": chat_id,
                "role": "user",
                "content": m.content.as_text_lossy(),
                "createdAt": created_at,
                "streaming": false
            });
            if let Some(atts) = cached_match.and_then(|cm| cm.get("attachments")) {
                obj["attachments"] = atts.clone();
            }
            ui_msgs.push(obj);
            continue;
        }
        if m.role == "assistant" {
            let entry = active_assistant.get_or_insert_with(|| {
                serde_json::json!({
                    "id": format!("rec-a-{}", ui_msgs.len()),
                    "chatId": chat_id,
                    "role": "assistant",
                    "content": "",
                    "modelId": model_id,
                    "reasoningContent": "",
                    "toolCalls": [],
                    "steps": [],
                    "streaming": false,
                    "paused": false,
                    "createdAt": now_ms
                })
            });

            if let Some(r) = &m.reasoning_content {
                if !r.trim().is_empty() {
                    let prev_r = entry["reasoningContent"].as_str().unwrap_or("");
                    let next_r = if prev_r.is_empty() {
                        r.clone()
                    } else {
                        format!("{}\n\n{}", prev_r, r)
                    };
                    entry["reasoningContent"] = serde_json::Value::String(next_r);
                    if let Some(steps) = entry["steps"].as_array_mut() {
                        steps.push(serde_json::json!({
                            "kind": "thought",
                            "text": r,
                            "startedAt": now_ms,
                            "finishedAt": now_ms
                        }));
                    }
                }
            }

            let msg_text = m.content.as_text_lossy();
            if !msg_text.trim().is_empty() {
                let has_tools = m.tool_calls.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
                if has_tools {
                    if let Some(steps) = entry["steps"].as_array_mut() {
                        steps.push(serde_json::json!({
                            "kind": "message",
                            "text": msg_text
                        }));
                    }
                } else {
                    let prev_c = entry["content"].as_str().unwrap_or("");
                    let next_c = if prev_c.is_empty() {
                        msg_text
                    } else {
                        format!("{}\n\n{}", prev_c, msg_text)
                    };
                    entry["content"] = serde_json::Value::String(next_c);
                }
            }

            if let Some(tcs) = &m.tool_calls {
                for tc in tcs {
                    let (output, is_err) = tool_results
                        .get(&tc.id)
                        .cloned()
                        .unwrap_or_else(|| (String::new(), false));
                    let status = if tool_results.contains_key(&tc.id) {
                        if is_err { "error" } else { "success" }
                    } else {
                        "paused"
                    };
                    let args_val: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
                    let rec = serde_json::json!({
                        "id": tc.id,
                        "name": tc.function.name,
                        "args": args_val,
                        "rawArgs": tc.function.arguments,
                        "status": status,
                        "output": output,
                        "startedAt": now_ms,
                        "finishedAt": now_ms
                    });
                    if let Some(arr) = entry["toolCalls"].as_array_mut() {
                        arr.push(rec.clone());
                    }
                    if let Some(steps) = entry["steps"].as_array_mut() {
                        steps.push(serde_json::json!({
                            "kind": "tool",
                            "record": rec
                        }));
                    }
                }
            }
        }
    }

    flush_assistant(&mut active_assistant, &mut ui_msgs);
    if let Some(last) = ui_msgs.last_mut() {
        if last.get("role").and_then(|v| v.as_str()) == Some("assistant") {
            let has_paused_tool = last
                .get("toolCalls")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().any(|tc| tc.get("status").and_then(|s| s.as_str()) == Some("paused")))
                .unwrap_or(false);
            let empty_content = last
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
            if has_paused_tool || empty_content {
                last["paused"] = serde_json::Value::Bool(true);
            }
        }
    }

    if ui_msgs.is_empty() {
        None
    } else {
        serde_json::to_string(&ui_msgs).ok()
    }
}

#[tauri::command]
async fn get_cached_chat_messages(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Option<(String, String)>, String> {
    let db = state.app_state.db.clone();
    tokio::task::spawn_blocking(move || db.get_cached_chat_messages(&chat_id).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_chat_recovery_snapshot(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Option<ChatRecoverySnapshot>, String> {
    let db = state.app_state.db.clone();
    tokio::task::spawn_blocking(move || {
        let cached = db.get_cached_chat_messages_with_ts(&chat_id).map_err(|e| e.to_string())?;
        let full_ckpt = db
            .load_checkpoint_with_ts(&format!("full_ctx:{}", chat_id))
            .map_err(|e| e.to_string())?;
        let chat_meta = db.get_chat(&chat_id).ok();

        let cached_ts_ms = cached
            .as_ref()
            .and_then(|(_, _, ts)| chrono::DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);
        let ckpt_ts_ms = full_ckpt
            .as_ref()
            .and_then(|(_, ts)| chrono::DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);

        if let Some((ckpt_json, ckpt_ts)) = full_ckpt {
            if cached.is_none() || ckpt_ts_ms > cached_ts_ms + 2000 {
                let cached_json_ref = cached.as_ref().map(|(_, j, _)| j.as_str());
                let model_id = chat_meta.as_ref().map(|c| c.model_id.as_str()).filter(|s| !s.is_empty());
                if let Some(recovered_json) =
                    convert_checkpoint_to_ui_messages(&chat_id, &ckpt_json, cached_json_ref, model_id)
                {
                    let title = cached
                        .as_ref()
                        .map(|(t, _, _)| t.clone())
                        .or_else(|| chat_meta.as_ref().map(|c| c.title.clone()))
                        .unwrap_or_else(|| chat_id.clone());
                    return Ok(Some(ChatRecoverySnapshot {
                        title,
                        messages_json: recovered_json,
                        updated_at: ckpt_ts,
                        recovered_from_checkpoint: true,
                    }));
                }
            }
        }

        if let Some((title, messages_json, updated_at)) = cached {
            return Ok(Some(ChatRecoverySnapshot {
                title,
                messages_json,
                updated_at,
                recovered_from_checkpoint: false,
            }));
        }

        Ok(None)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn save_ui_pending_queues(
    state: State<'_, AppManagedState>,
    queues_json: String,
) -> Result<(), String> {
    let db = &state.app_state.db;
    db.save_checkpoint("ui_pending_queues", &queues_json)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn load_ui_pending_queues(state: State<'_, AppManagedState>) -> Result<Option<String>, String> {
    let db = &state.app_state.db;
    db.load_checkpoint("ui_pending_queues").map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_bootstrap_config(
    state: State<'_, AppManagedState>,
) -> Result<state::BootstrapConfig, String> {
    Ok(state.app_state.get_bootstrap_config().await)
}

#[tauri::command]
async fn complete_first_run(
    state: State<'_, AppManagedState>,
    workspace_dir: Option<String>,
    default_model_id: Option<String>,
) -> Result<(), String> {
    state
        .app_state
        .complete_first_run(workspace_dir, default_model_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_workspace_dir(state: State<'_, AppManagedState>) -> Result<String, String> {
    let ws = state.app_state.get_workspace_dir().await;
    Ok(ws.to_string_lossy().to_string())
}

#[tauri::command]
async fn set_workspace_dir(state: State<'_, AppManagedState>, path: String) -> Result<(), String> {
    state
        .app_state
        .set_workspace_dir(PathBuf::from(path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_credential(
    state: State<'_, AppManagedState>,
    key_name: String,
) -> Result<String, String> {
    state
        .app_state
        .get_credential(&key_name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_credential(
    state: State<'_, AppManagedState>,
    key_name: String,
    secret: String,
) -> Result<(), String> {
    state
        .app_state
        .set_credential(&key_name, &secret)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_credential(
    state: State<'_, AppManagedState>,
    key_name: String,
) -> Result<(), String> {
    state
        .app_state
        .delete_credential(&key_name)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct HistoryInputItem {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AttachmentInput {
    pub name: Option<String>,
    pub path: Option<String>,
    pub kind: Option<String>,
    pub data_url: Option<String>,
}

fn decode_attachment_audio_bytes(att: &AttachmentInput) -> Option<Vec<u8>> {
    use base64::Engine;
    if let Some(du) = att.data_url.as_deref() {
        if let Some(pos) = du.find("base64,") {
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&du[pos + 7..]) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
    }
    let path = att.path.as_deref().unwrap_or("").trim();
    if path.is_empty() {
        return None;
    }
    let p = std::path::Path::new(path);
    if speech::convert::is_wav_file(p) {
        std::fs::read(p).ok()
    } else {
        speech::convert::decode_to_wav_16k_mono(p).ok()
    }
}

async fn transcribe_attachment_audio(
    mgr: &AppManagedState,
    att: &AttachmentInput,
    preferred_model: Option<&str>,
    preferred_language: Option<&str>,
) -> Result<String, String> {
    let bytes = decode_attachment_audio_bytes(att)
        .ok_or_else(|| "Failed to decode audio attachment".to_string())?;
    let fallback_key = mgr.app_state.get_credential("openai_api_key").await.ok();
    let res = mgr
        .speech_engine
        .transcribe_audio(&bytes, fallback_key.as_deref(), preferred_model, preferred_language)
        .await?;
    Ok(res.text)
}

async fn build_user_msg_with_attachments(
    mgr: &AppManagedState,
    model_id: &str,
    prompt: &str,
    attachments: Option<Vec<AttachmentInput>>,
    whisper_model: Option<&str>,
) -> Option<llm::request_compat::ChatMessage> {
    use base64::Engine;
    let atts = attachments?;
    if atts.is_empty() {
        return None;
    }
    let mut parts: Vec<llm::request_compat::ContentPart> = vec![
        llm::request_compat::ContentPart::Text { text: prompt.to_string() },
    ];
    let mut has_media = false;
    let mut extra_notes: Vec<String> = Vec::new();
    let native_audio = llm::capabilities::model_supports_native_audio(model_id);
    for att in atts {
        let name = att.name.clone().unwrap_or_else(|| "file".to_string());
        let path_str = att.path.clone().unwrap_or_default();
        let kind = att.kind.clone().unwrap_or_default();
        let low = format!("{} {}", name, path_str).to_lowercase();
        let is_audio = kind == "audio"
            || low.ends_with(".wav")
            || low.ends_with(".mp3")
            || low.ends_with(".ogg")
            || low.ends_with(".opus")
            || low.ends_with(".m4a")
            || low.ends_with(".webm")
            || low.ends_with(".flac");
        let is_img = kind == "image"
            || low.ends_with(".png")
            || low.ends_with(".jpg")
            || low.ends_with(".jpeg")
            || low.ends_with(".webp")
            || low.ends_with(".gif");
        if is_img {
            let url_opt = if let Some(du) = att.data_url.clone().filter(|u| u.starts_with("data:image/")) {
                Some(du)
            } else if !path_str.trim().is_empty() {
                if let Ok(bytes) = std::fs::read(path_str.trim()) {
                    let mime = if low.ends_with(".jpg") || low.ends_with(".jpeg") {
                        "image/jpeg"
                    } else if low.ends_with(".webp") {
                        "image/webp"
                    } else if low.ends_with(".gif") {
                        "image/gif"
                    } else {
                        "image/png"
                    };
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    Some(format!("data:{};base64,{}", mime, b64))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(u) = url_opt {
                has_media = true;
                let resized = llm::request_compat::resize_image_data_url_if_needed(&u, 1568);
                parts.push(llm::request_compat::ContentPart::ImageUrl {
                    image_url: llm::request_compat::ImageUrlDetail { url: resized, detail: Some("auto".to_string()) },
                });
            }
        } else if is_audio {
            if native_audio {
                if let Some(bytes) = decode_attachment_audio_bytes(&att) {
                    has_media = true;
                    parts.push(llm::request_compat::ContentPart::InputAudio {
                        input_audio: llm::request_compat::InputAudioDetail {
                            data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                            format: "wav".to_string(),
                        },
                    });
                    if prompt.trim().is_empty() {
                        extra_notes.push(format!("[Voice message from user: {}]", name));
                    }
                } else {
                    extra_notes.push(format!("[Voice message attachment could not be decoded: {}]", name));
                }
            } else {
                match transcribe_attachment_audio(mgr, &att, whisper_model, None).await {
                    Ok(text) if !text.trim().is_empty() => {
                        if prompt.trim().is_empty() {
                            if let Some(llm::request_compat::ContentPart::Text { text: t }) = parts.first_mut() {
                                *t = text.trim().to_string();
                            }
                        }
                    }
                    Ok(_) => {
                        if prompt.trim().is_empty() {
                            extra_notes.push(format!("[Voice message from user: {} — transcription is empty]", name));
                        }
                    }
                    Err(e) => {
                        extra_notes.push(format!("[Voice message from user: {} — transcription failed: {}]", name, e));
                    }
                }
            }
        } else if !path_str.trim().is_empty() {
            extra_notes.push(format!("[Attached file sent by user: {} ({})]", name, path_str));
        }
    }
    if !extra_notes.is_empty() {
        if let Some(llm::request_compat::ContentPart::Text { text }) = parts.first_mut() {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&extra_notes.join("\n"));
        }
    }
    if !has_media && extra_notes.is_empty() {
        return None;
    }
    Some(llm::request_compat::ChatMessage {
        role: "user".to_string(),
        content: llm::request_compat::MessageContent::Parts(parts),
        name: None,
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    })
}

#[tauri::command]
async fn start_chat_stream(
    state: State<'_, AppManagedState>,
    app: tauri::AppHandle,
    chat_id: String,
    prompt: String,
    model_id: String,
    reasoning_effort: String,
    provider_name: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_protocol: Option<String>,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
    context_window: Option<u32>,
    max_output_tokens: Option<u32>,
    supports_reasoning: Option<bool>,
    workspace_dir: Option<String>,
    plan_mode: Option<bool>,
    force_execute: Option<bool>,
    resume: Option<bool>,
    history: Option<Vec<HistoryInputItem>>,
    attachments: Option<Vec<AttachmentInput>>,
    whisper_model: Option<String>,
    theme_id: Option<String>,
    on_event: Channel<serde_json::Value>,
) -> Result<String, String> {
    let session = state.stream_mgr.create_session(&chat_id).await;
    let session_id = chat_id.clone();

    let install_ws = state::AppState::resolve_install_workspace();
    let explicit_ws = workspace_dir
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| {
            if s.is_empty() {
                return false;
            }
            PathBuf::from(s) != install_ws
        });
    let (tools_ws_path, workspace_str) = match explicit_ws {
        Some(ref dir) => (PathBuf::from(dir), dir.clone()),
        None => {
            let chat_ws = install_ws.join(&chat_id);
            let _ = std::fs::create_dir_all(&chat_ws);
            (PathBuf::new(), chat_ws.to_string_lossy().to_string())
        }
    };
    let is_theme_mode = theme_id.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    if !is_theme_mode && state.app_state.db.get_chat(&chat_id).is_err() {
        let now = chrono::Utc::now().to_rfc3339();
        let chat = db::chats::Chat {
            id: chat_id.clone(),
            project_id: explicit_ws,
            title: prompt.chars().take(48).collect::<String>(),
            model_id: model_id.clone(),
            system_prompt: String::new(),
            is_pinned: false,
            created_at: now.clone(),
            updated_at: now,
        };
        let _ = state.app_state.db.create_chat(&chat);
    }
    let tools = if let Some(ref tid) = theme_id.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        state.tool_dispatcher.for_theme_studio(app.clone(), tid)
    } else {
        let sandbox_opt = if tools_ws_path.as_os_str().is_empty() {
            None
        } else {
            Some(tools_ws_path.as_path())
        };
        state
            .tool_dispatcher
            .for_workspace_with_cwd(sandbox_opt, std::path::Path::new(&workspace_str), &chat_id)
    };
    let force_exec = force_execute.unwrap_or(false);
    let combined_plan = !force_exec && (plan_mode.unwrap_or(false) || prompt.split_whitespace().any(|t| t.eq_ignore_ascii_case("/plan")));
    tools.set_plan_mode(combined_plan);
    tools.set_plan_mode_locked(force_exec);
    let key_pool = (*state.key_pool).clone();
    let caps_reg = (*state.caps_registry).clone();
    if context_window.is_some() || max_output_tokens.is_some() || supports_reasoning.is_some() {
        caps_reg.update_model_caps(&model_id, context_window, max_output_tokens, supports_reasoning);
    }
    let is_oauth_proto = matches!(
        api_protocol.as_deref().unwrap_or(""),
        "antigravity" | "openai_codex" | "claude_oauth" | "kimi_oauth" | "xai_oauth" | "zcode_oauth" | "perplexity_oauth"
    );
    let (eff_px_en, eff_px_url) = if is_oauth_proto {
        llm::oauth_accounts::resolve_oauth_proxy(proxy_enabled, proxy_url.as_deref())
    } else {
        (proxy_enabled, proxy_url.clone())
    };
    let http_client = build_http_client(eff_px_en, eff_px_url.as_deref(), Some(&state.http_client))?;

    let (mut resolved_provider, mut resolved_url) = ("OpenAI".to_string(), "https://api.openai.com/v1".to_string());
    if let Ok(model_rec) = state.app_state.db.get_model(&model_id) {
        if let Ok(prov_rec) = state.app_state.db.get_provider(&model_rec.provider_id) {
            resolved_provider = prov_rec.name;
            resolved_url = prov_rec.base_url;
        }
    }
    if let Some(pn) = provider_name {
        if !pn.trim().is_empty() {
            resolved_provider = pn;
        }
    }
    if let Some(bu) = base_url {
        if !bu.trim().is_empty() {
            resolved_url = bu;
        }
    }
    if let Some(ref ak) = api_key {
        if !ak.trim().is_empty() {
            let key_str = ak.trim().to_string();
            key_pool.set_keys(&resolved_provider, vec![key_str.clone()]);
            key_pool.clear_auth_blocks(&resolved_url, &[key_str]).await;
        }
    }
    let provider_name = resolved_provider;
    let base_url = resolved_url;

    if !is_theme_mode && is_direct_image_model(&model_id) {
        let out_base = if !tools_ws_path.as_os_str().is_empty() {
            tools_ws_path.clone()
        } else {
            PathBuf::from(&workspace_str)
        };
        let img_dir = out_base.join("generated_images");
        let _ = std::fs::create_dir_all(&img_dir);
        let prompt_clean = prompt.clone();
        let model_clean = model_id.clone();
        let http_for_img = http_client.clone();
        let on_evt_img = on_event.clone();
        let db_img = state.app_state.db.clone();
        let cid_img = chat_id.clone();
        let proto_img = api_protocol.clone().unwrap_or_default();
        let base_img = base_url.clone();
        let key_img = api_key.clone().unwrap_or_default();
        let sess_img = session.clone();
        let mgr_img = state.stream_mgr.clone();

        tokio::spawn(async move {
            let gen_res: Result<String, String> = async {
                let (bytes, mime) = if proto_img == "antigravity" || proto_img.is_empty() || key_img.trim().is_empty() {
                    let (_, b, m) = llm::antigravity_client::generate_antigravity_image(
                        &http_for_img,
                        Some(&model_clean),
                        &prompt_clean,
                        None,
                    )
                    .await?;
                    (b, m)
                } else {
                    use base64::Engine;
                    let url = format!("{}/images/generations", base_img.trim_end_matches('/'));
                    let resp = http_for_img
                        .post(&url)
                        .bearer_auth(key_img.trim())
                        .json(&json!({
                            "model": model_clean,
                            "prompt": prompt_clean,
                            "n": 1,
                            "response_format": "b64_json"
                        }))
                        .send()
                        .await
                        .map_err(|e| e.to_string())?;
                    if !resp.status().is_success() {
                        let st = resp.status();
                        let txt = resp.text().await.unwrap_or_default();
                        return Err(format!("Image API HTTP {}: {}", st, txt));
                    }
                    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
                    let b64 = v
                        .get("data")
                        .and_then(|d| d.get(0))
                        .and_then(|x| x.get("b64_json"))
                        .and_then(|s| s.as_str())
                        .ok_or_else(|| "No b64_json in image response".to_string())?;
                    let b = base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .map_err(|e| e.to_string())?;
                    (b, "image/png".to_string())
                };
                let ext = if mime.contains("jpeg") || mime.contains("jpg") {
                    "jpg"
                } else if mime.contains("webp") {
                    "webp"
                } else {
                    "png"
                };
                let out_file = img_dir.join(format!("gen_{}.{}", chrono::Utc::now().timestamp_millis(), ext));
                std::fs::write(&out_file, &bytes).map_err(|e| e.to_string())?;
                Ok(out_file.to_string_lossy().to_string())
            }
            .await;

            if !sess_img.is_active() {
                mgr_img.remove_session(&cid_img).await;
                return;
            }
            match gen_res {
                Ok(path_str) => {
                    let md = format!("![generated]({})", path_str);
                    let _ = on_evt_img.send(json!({ "type": "delta", "content": md }));
                    let seq = db_img.max_message_seq(&cid_img) + 1;
                    let _ = db_img.create_message(&db::chats::Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        chat_id: cid_img.clone(),
                        role: "assistant".to_string(),
                        content: md,
                        model_id: None,
                        reasoning_content: None,
                        tool_calls: None,
                        tool_call_id: None,
                        sequence_num: seq,
                        created_at: chrono::Utc::now().to_rfc3339(),
                    });
                    let _ = on_evt_img.send(json!({ "type": "done", "usage": null }));
                }
                Err(e) => {
                    let _ = on_evt_img.send(json!({ "type": "error", "message": e }));
                }
            }
            mgr_img.remove_session(&cid_img).await;
        });
        return Ok(session_id);
    }

    let is_swarm = prompt.starts_with("/swarm");
    let (tx, mut rx) = tokio::sync::mpsc::channel(128);

    let on_event_clone = on_event.clone();
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let val = match evt {
                llm::StreamEvent::ContentDelta { delta } => {
                    json!({ "type": "delta", "content": delta })
                }
                llm::StreamEvent::ReasoningDelta { delta } => {
                    json!({ "type": "reasoning_delta", "content": delta })
                }
                llm::StreamEvent::ToolCallDelta {
                    id,
                    name,
                    arguments_delta,
                    ..
                } => {
                    json!({
                        "type": "tool_call_delta",
                        "id": id.unwrap_or_default(),
                        "name": name.unwrap_or_default(),
                        "argumentsDelta": arguments_delta
                    })
                }
                llm::StreamEvent::ToolCallComplete {
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    json!({
                        "type": "tool_call_complete",
                        "id": id,
                        "name": name,
                        "arguments": arguments
                    })
                }
                llm::StreamEvent::ToolOutputDelta { id, chunk } => {
                    json!({
                        "type": "tool_output_delta",
                        "id": id,
                        "chunk": chunk
                    })
                }
                llm::StreamEvent::ToolExecEnd {
                    id,
                    success,
                    output,
                    error,
                    elapsed_ms,
                } => {
                    json!({
                        "type": "tool_end",
                        "id": id,
                        "success": success,
                        "output": output,
                        "error": error,
                        "elapsedMs": elapsed_ms
                    })
                }
                llm::StreamEvent::RetroactiveReasoningMove { text } => {
                    json!({ "type": "retroactive_reasoning_move", "reasoningContent": text })
                }
                llm::StreamEvent::TurnBufferReset => {
                    json!({ "type": "turn_buffer_reset" })
                }
                llm::StreamEvent::RetryNotice { attempt, reason, .. } => {
                    json!({ "type": "retry_notice", "attempt": attempt, "reason": reason })
                }
                llm::StreamEvent::Finish { usage, .. } => {
                    json!({ "type": "done", "usage": usage })
                }
                llm::StreamEvent::ContextTelemetry {
                    used_tokens,
                    context_window,
                    cached_tokens,
                    cache_hit_rate,
                } => {
                    json!({
                        "type": "context_telemetry",
                        "usedTokens": used_tokens,
                        "contextWindow": context_window,
                        "cachedTokens": cached_tokens,
                        "cacheHitRate": cache_hit_rate
                    })
                }
                llm::StreamEvent::ContextCompacted {
                    before_tokens,
                    after_tokens,
                    summary,
                } => {
                    json!({
                        "type": "context_compacted",
                        "beforeTokens": before_tokens,
                        "afterTokens": after_tokens,
                        "summary": summary
                    })
                }
                llm::StreamEvent::TaskBoardUpdated { items } => {
                    json!({
                        "type": "task_board_updated",
                        "items": items
                    })
                }
                llm::StreamEvent::PlanApprovalRequired { plan_summary } => {
                    json!({
                        "type": "plan_approval_required",
                        "planSummary": plan_summary
                    })
                }
                llm::StreamEvent::GoalStatusUpdated {
                    status,
                    condition,
                    turn,
                    max_turns,
                    reason,
                    elapsed_secs,
                } => {
                    json!({
                        "type": "goal_status_updated",
                        "status": status,
                        "condition": condition,
                        "turn": turn,
                        "maxTurns": max_turns,
                        "reason": reason,
                        "elapsedSecs": elapsed_secs
                    })
                }
                llm::StreamEvent::Error { error } => {
                    json!({ "type": "error", "message": error })
                }
            };
            let _ = on_event_clone.send(val);
        }
    });

    let contract_mutex = {
        let mut map = state.active_contracts.write().await;
        map.entry(chat_id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None)))
            .clone()
    };
    let stream_mgr = state.stream_mgr.clone();
    let db_arc = state.app_state.db.clone();
    let is_resume = resume.unwrap_or(false);
    let resumed_history: Option<Vec<llm::request_compat::ChatMessage>> = if is_resume {
        db_arc
            .load_checkpoint(&chat_id)
            .ok()
            .flatten()
            .or_else(|| db_arc.load_checkpoint(&format!("tg_ctx:{}", chat_id)).ok().flatten())
            .and_then(|j| serde_json::from_str(&j).ok())
    } else {
        None
    };
    let chat_id_str = chat_id.clone();

    let initial_history: Vec<llm::request_compat::ChatMessage> = history
        .unwrap_or_default()
        .into_iter()
        .filter(|h| !h.content.trim().is_empty() && (h.role == "user" || h.role == "assistant"))
        .map(|h| {
            if h.role == "assistant" {
                llm::request_compat::ChatMessage::assistant(h.content)
            } else {
                llm::request_compat::ChatMessage::user(h.content)
            }
        })
        .collect();

    let pending_user_msg = build_user_msg_with_attachments(
        &state,
        &model_id,
        &prompt,
        attachments,
        whisper_model.as_deref(),
    )
    .await;
    let api_proto = api_protocol.clone();
    logger::info("ChatStream", &format!("start chat_id={} model={} provider={:?} theme={:?} has_attachments={}", chat_id, model_id, provider_name, theme_id, pending_user_msg.is_some()));
    tokio::spawn(async move {
        let mut contract_lock = contract_mutex.lock().await;
        let mut history = if let Some(rh) = resumed_history {
            rh
        } else {
            let mut h = Vec::new();
            if is_theme_mode {
                let sys_prompt = "You are OmniHarness Theme Designer, a specialized UI/CSS/SVG engineer. Customize the OmniHarness interface theme in real time using ONLY your theme_* tools. Always call theme_get_dom_spec and theme_read_current first, then apply clean valid CSS (using [data-omni-*] selectors, CSS variables, @keyframes, @font-face, omni-asset:// URLs), SVG icons via theme_set_icon, and UI text/language overrides via theme_set_texts.".to_string();
                h.push(llm::request_compat::ChatMessage::system(sys_prompt));
                h.extend(initial_history);
            } else if !initial_history.is_empty() {
                let sys_prompt = engine::prompt_builder::PromptBuilder::build_system_prompt(
                    &engine::role_permissions::AgentRole::Main,
                    &workspace_str,
                    None,
                    None,
                    Some(&model_id),
                );
                h.push(llm::request_compat::ChatMessage::system(sys_prompt));
                h.extend(initial_history);
            }
            if let Some(att_msg) = pending_user_msg {
                h.push(att_msg);
            }
            h
        };

        let goal_cmd = engine::goal_loop::parse_goal_command(&prompt);
        let res = if let Some(engine::goal_loop::GoalCommandAction::Clear) = goal_cmd {
            let _ = db_arc.delete_goal_state(&chat_id_str);
            let _ = tx
                .send(llm::StreamEvent::GoalStatusUpdated {
                    status: "cleared".to_string(),
                    condition: String::new(),
                    turn: 0,
                    max_turns: 0,
                    reason: None,
                    elapsed_secs: 0,
                })
                .await;
            let _ = tx
                .send(llm::StreamEvent::ContentDelta {
                    delta: "Goal cleared.".to_string(),
                })
                .await;
            Ok(())
        } else if let Some(engine::goal_loop::GoalCommandAction::Status) = goal_cmd {
            let loaded = db_arc
                .load_goal_state(&chat_id_str)
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str::<engine::goal_loop::GoalState>(&s).ok());
            let msg = if let Some(g) = loaded {
                let _ = tx.send(g.to_stream_event()).await;
                format!(
                    "Goal status: **{}** (turn {}/{}, elapsed {}s)\nCondition: {}{}",
                    g.status.as_str(),
                    g.turn,
                    g.max_turns,
                    g.elapsed_secs(),
                    g.condition,
                    g.last_reason
                        .as_deref()
                        .map(|r| format!("\nLast evaluator reason: {}", r))
                        .unwrap_or_default()
                )
            } else {
                "No active goal in this session.".to_string()
            };
            let _ = tx
                .send(llm::StreamEvent::ContentDelta { delta: msg })
                .await;
            Ok(())
        } else if is_swarm {
            let swarm_prompt = prompt.strip_prefix("/swarm").unwrap_or(&prompt).trim();
            engine::debate_swarm::DebateSwarmRunner::run_swarm(
                &chat_id_str,
                swarm_prompt,
                &workspace_str,
                tools,
                &key_pool,
                &caps_reg,
                &provider_name,
                &base_url,
                &model_id,
                None,
                &session,
                &mut *contract_lock,
                tx.clone(),
            )
            .await
            .map(|_| ())
        } else {
            let mut active_goal: Option<engine::goal_loop::GoalState> = match goal_cmd {
                Some(engine::goal_loop::GoalCommandAction::Start { condition }) => {
                    let clean_cond = condition
                        .split_whitespace()
                        .filter(|t| !t.eq_ignore_ascii_case("/plan"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let g = engine::goal_loop::GoalState::new(&chat_id_str, &clean_cond);
                    if let Ok(j) = serde_json::to_string(&g) {
                        let _ = db_arc.save_goal_state(&chat_id_str, &j);
                    }
                    let _ = tx.send(g.to_stream_event()).await;
                    Some(g)
                }
                _ => {
                    let existing = db_arc
                        .load_goal_state(&chat_id_str)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str::<engine::goal_loop::GoalState>(&s).ok());
                    if let Some(mut g) = existing {
                        if g.status == engine::goal_loop::GoalStatus::Paused
                            || g.status == engine::goal_loop::GoalStatus::Idle
                        {
                            g.status = engine::goal_loop::GoalStatus::Active;
                            g.zero_tool_streak = 0;
                            if let Ok(j) = serde_json::to_string(&g) {
                                let _ = db_arc.save_goal_state(&chat_id_str, &j);
                            }
                            let _ = tx.send(g.to_stream_event()).await;
                            Some(g)
                        } else if g.status == engine::goal_loop::GoalStatus::Active {
                            Some(g)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            };

            let mut current_prompt = if let Some(ref g) = active_goal {
                if prompt.trim().starts_with("/goal") {
                    if tools.is_plan_mode() {
                        format!(
                            "GOAL + PLAN MODE ACTIVE.\nPhase 1 (Read-Only Plan): Explore the codebase, build a concrete step-by-step plan via task_board, then call exit_plan_mode(plan_summary) to unlock file edits.\nPhase 2 (Execute & Prove): Implement the plan and prove with actual tool outputs that the condition is satisfied:\n{}",
                            g.condition
                        )
                    } else {
                        format!(
                            "GOAL MODE ACTIVE (Autonomous Completion Loop).\nCondition to satisfy and prove with actual tool outputs:\n{}",
                            g.condition
                        )
                    }
                } else {
                    prompt.clone()
                }
            } else {
                prompt.clone()
            };

            let mut first_turn = true;
            loop {
                let hist_len_before = history.len();
                let turn_res = engine::single_agent::SingleAgentRunner::run_turn(
                    &http_client,
                    &chat_id_str,
                    &current_prompt,
                    &workspace_str,
                    tools.clone(),
                    &key_pool,
                    &caps_reg,
                    &provider_name,
                    &base_url,
                    api_proto.as_deref(),
                    &model_id,
                    &reasoning_effort,
                    None,
                    Some(&db_arc),
                    if first_turn { is_resume } else { false },
                    &session,
                    &mut *contract_lock,
                    &mut history,
                    tx.clone(),
                )
                .await
                .map(|_| ());
                first_turn = false;

                if let Err(err) = turn_res {
                    if let Some(ref mut g) = active_goal {
                        if engine::goal_loop::is_unrecoverable_goal_error(&err) {
                            let _ = db_arc.delete_goal_state(&chat_id_str);
                            g.status = engine::goal_loop::GoalStatus::Failed;
                            g.last_reason = Some(err.clone());
                            let _ = tx.send(g.to_stream_event()).await;
                            break Err(format!("Goal cleared after an unrecoverable error: {}", err));
                        } else if !session.is_active() {
                            g.status = engine::goal_loop::GoalStatus::Paused;
                            if let Ok(j) = serde_json::to_string(g) {
                                let _ = db_arc.save_goal_state(&chat_id_str, &j);
                            }
                            let _ = tx.send(g.to_stream_event()).await;
                        }
                    }
                    break Err(err);
                }

                let Some(ref mut g) = active_goal else {
                    break Ok(());
                };

                let still_active_in_db = db_arc
                    .load_goal_state(&chat_id_str)
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str::<engine::goal_loop::GoalState>(&s).ok())
                    .map(|db_g| db_g.status == engine::goal_loop::GoalStatus::Active)
                    .unwrap_or(false);

                if !still_active_in_db || !session.is_active() {
                    let _ = tx
                        .send(llm::StreamEvent::ContentDelta {
                            delta: "\n\nGoal cleared · finishing current step.".to_string(),
                        })
                        .await;
                    break Ok(());
                }

                let used_tools_this_turn = history[hist_len_before..]
                    .iter()
                    .any(|m| m.role == "tool" || m.tool_calls.as_ref().map(|v| !v.is_empty()).unwrap_or(false));
                if used_tools_this_turn {
                    g.zero_tool_streak = 0;
                } else {
                    g.zero_tool_streak += 1;
                }

                if g.zero_tool_streak >= 2 {
                    g.status = engine::goal_loop::GoalStatus::Paused;
                    g.last_reason = Some("No tools executed for 2 consecutive turns (stagnation breaker).".to_string());
                    if let Ok(j) = serde_json::to_string(g) {
                        let _ = db_arc.save_goal_state(&chat_id_str, &j);
                    }
                    let _ = tx.send(g.to_stream_event()).await;
                    let _ = tx
                        .send(llm::StreamEvent::ContentDelta {
                            delta: "\n\n**Goal Paused (Stagnation Breaker):** Worker produced 2 consecutive turns without tool execution. Send a message to resume.".to_string(),
                        })
                        .await;
                    break Ok(());
                }

                let verdict = engine::goal_loop::evaluate_transcript(
                    &http_client,
                    &provider_name,
                    &base_url,
                    api_proto.as_deref(),
                    &key_pool,
                    &caps_reg,
                    &model_id,
                    &g.condition,
                    &history,
                )
                .await;

                match verdict {
                    Ok(engine::goal_loop::EvaluatorVerdict::Met { reason }) => {
                        g.status = engine::goal_loop::GoalStatus::Achieved;
                        g.last_reason = Some(reason.clone());
                        let _ = db_arc.delete_goal_state(&chat_id_str);
                        let _ = tx.send(g.to_stream_event()).await;
                        let _ = tx
                            .send(llm::StreamEvent::ContentDelta {
                                delta: format!("\n\n**Goal Achieved:** {}", reason),
                            })
                            .await;
                        break Ok(());
                    }
                    Ok(engine::goal_loop::EvaluatorVerdict::Impossible { reason }) => {
                        g.status = engine::goal_loop::GoalStatus::Failed;
                        g.last_reason = Some(reason.clone());
                        let _ = db_arc.delete_goal_state(&chat_id_str);
                        let _ = tx.send(g.to_stream_event()).await;
                        let _ = tx
                            .send(llm::StreamEvent::ContentDelta {
                                delta: format!("\n\n**Goal Failed (Impossible):** {}", reason),
                            })
                            .await;
                        break Ok(());
                    }
                    Ok(engine::goal_loop::EvaluatorVerdict::NotYetMet { reason }) => {
                        g.turn += 1;
                        g.last_reason = Some(reason.clone());
                        if g.turn > g.max_turns {
                            g.status = engine::goal_loop::GoalStatus::Paused;
                            if let Ok(j) = serde_json::to_string(g) {
                                let _ = db_arc.save_goal_state(&chat_id_str, &j);
                            }
                            let _ = tx.send(g.to_stream_event()).await;
                            let _ = tx
                                .send(llm::StreamEvent::ContentDelta {
                                    delta: format!("\n\n**Goal Paused:** Turn budget reached ({}/{}). Send a message to continue.", g.max_turns, g.max_turns),
                                })
                                .await;
                            break Ok(());
                        }
                        if let Ok(j) = serde_json::to_string(g) {
                            let _ = db_arc.save_goal_state(&chat_id_str, &j);
                        }
                        let _ = tx.send(g.to_stream_event()).await;
                        let _ = tx
                            .send(llm::StreamEvent::ContentDelta {
                                delta: format!("\n\n**Goal Evaluator (Turn {}/{}) — NOT YET MET:** {}\n\n", g.turn, g.max_turns, reason),
                            })
                            .await;
                        current_prompt = format!(
                            "[Goal Stop-Hook Feedback — NOT YET MET]\nCondition: {}\nEvaluator Reason: {}\n\nContinue working and produce concrete tool output proving the condition is met.",
                            g.condition, reason
                        );
                    }
                    Err(eval_err) => {
                        g.status = engine::goal_loop::GoalStatus::Paused;
                        g.last_reason = Some(format!("Evaluator error: {}", eval_err));
                        if let Ok(j) = serde_json::to_string(g) {
                            let _ = db_arc.save_goal_state(&chat_id_str, &j);
                        }
                        let _ = tx.send(g.to_stream_event()).await;
                        let _ = tx
                            .send(llm::StreamEvent::ContentDelta {
                                delta: format!("\n\n**Goal Paused (Evaluator error):** {}", eval_err),
                            })
                            .await;
                        break Ok(());
                    }
                }
            }
        };

        match res {
            Ok(()) => {
                let _ = tx
                    .send(llm::StreamEvent::Finish {
                        finish_reason: Some("stop".to_string()),
                        usage: None,
                    })
                    .await;
            }
            Err(err) => {
                let _ = tx.send(llm::StreamEvent::Error { error: err }).await;
            }
        }

        stream_mgr.remove_session_if_current(&chat_id_str, &session).await;
    });

    Ok(session_id)
}


#[tauri::command]
async fn ask_side_question(
    state: State<'_, AppManagedState>,
    chat_id: String,
    question: String,
    model_id: String,
    provider_name: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_protocol: Option<String>,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
    context_window: Option<u32>,
    max_output_tokens: Option<u32>,
    history: Option<Vec<HistoryInputItem>>,
    on_event: Channel<serde_json::Value>,
) -> Result<(), String> {
    let key_pool = (*state.key_pool).clone();
    let caps_reg = (*state.caps_registry).clone();
    if context_window.is_some() || max_output_tokens.is_some() {
        caps_reg.update_model_caps(&model_id, context_window, max_output_tokens, None);
    }
    let http_client = build_http_client(proxy_enabled, proxy_url.as_deref(), Some(&state.http_client))?;

    let (mut resolved_provider, mut resolved_url) = ("OpenAI".to_string(), "https://api.openai.com/v1".to_string());
    if let Ok(model_rec) = state.app_state.db.get_model(&model_id) {
        if let Ok(prov_rec) = state.app_state.db.get_provider(&model_rec.provider_id) {
            resolved_provider = prov_rec.name;
            resolved_url = prov_rec.base_url;
        }
    }
    if let Some(pn) = provider_name {
        if !pn.trim().is_empty() {
            resolved_provider = pn;
        }
    }
    if let Some(bu) = base_url {
        if !bu.trim().is_empty() {
            resolved_url = bu;
        }
    }
    if let Some(ak) = api_key {
        if !ak.trim().is_empty() {
            key_pool.set_keys(&resolved_provider, vec![ak.trim().to_string()]);
        }
    }

let fallback_history: Vec<engine::btw_context::BtwFallbackMessage> = history
    .unwrap_or_default()
    .into_iter()
    .map(|h| engine::btw_context::BtwFallbackMessage {
        role: h.role,
        content: h.content,
    })
    .collect();

let msgs = engine::btw_context::build_btw_messages(
    &state.app_state.db,
    &state.stream_mgr,
    &chat_id,
    &model_id,
    &caps_reg,
    if fallback_history.is_empty() {
        None
    } else {
        Some(&fallback_history)
    },
    &question,
)
.await;

    let req = llm::request_compat::ChatRequest {
        model: model_id,
        messages: msgs,
        temperature: Some(0.3),
        top_p: None,
        max_tokens: None,
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
    let on_ev = on_event.clone();
    let fwd = tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let val = match evt {
                llm::StreamEvent::ContentDelta { delta } => json!({ "type": "delta", "content": delta }),
                llm::StreamEvent::ReasoningDelta { delta } => json!({ "type": "reasoning_delta", "content": delta }),
                llm::StreamEvent::Finish { .. } => json!({ "type": "done" }),
                llm::StreamEvent::Error { error } => json!({ "type": "error", "message": error }),
                _ => continue,
            };
            let _ = on_ev.send(val);
        }
    });

    let res = llm::dispatch_stream_with_protocol(
        &http_client,
        &resolved_provider,
        &resolved_url,
        api_protocol.as_deref(),
        &key_pool,
        &caps_reg,
        req,
        tx,
    )
    .await;

    let _ = fwd.await;
    if let Err(e) = res {
        let _ = on_event.send(json!({ "type": "error", "message": e.to_string() }));
        return Err(e.to_string());
    }
    Ok(())
}


#[tauri::command]
async fn get_goal_state(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Option<engine::goal_loop::GoalState>, String> {
    let raw = state
        .app_state
        .db
        .load_goal_state(&chat_id)
        .map_err(|e| e.to_string())?;
    Ok(raw.and_then(|s| serde_json::from_str::<engine::goal_loop::GoalState>(&s).ok()))
}

#[tauri::command]
async fn clear_goal_state(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<(), String> {
    state
        .app_state
        .db
        .delete_goal_state(&chat_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_stream(
    state: State<'_, AppManagedState>,
    session_id: String,
) -> Result<(), String> {
    let _ = state.stream_mgr.cancel_session(&session_id).await;
    let _ = state.tool_dispatcher.pty_manager.kill_all_for_chat(&session_id);
    Ok(())
}

#[tauri::command]
async fn send_intervention(
    state: State<'_, AppManagedState>,
    session_id: String,
    message: String,
) -> Result<(), String> {
    state.stream_mgr.inject_intervention(&session_id, message).await
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedModelInfo {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub supports_reasoning: bool,
    pub supports_vision: bool,
    pub supports_tools: bool,
    pub supports_audio: bool,
    pub supports_files: bool,
}

fn build_detected_model(id: &str, name: Option<&str>, raw: Option<&serde_json::Value>) -> DetectedModelInfo {
    let caps = llm::capabilities::ModelCapabilities::default_for_model(id);
    let lower = id.to_lowercase();
    let mut ctx = caps.context_window;
    let mut max_out = caps.max_output_tokens;
    let mut reasoning = caps.supports_reasoning;
    let mut vision = caps.supports_vision;
    let mut tools = caps.supports_tools;
    let mut audio = lower.contains("gpt-4o") || lower.contains("gemini") || lower.contains("audio");
    let mut files = lower.contains("claude") || lower.contains("gemini") || lower.contains("gpt-4") || lower.contains("gpt-5");

    let mut md_ctx_found = false;
    let mut md_out_found = false;
    if let Some(md) = llm::models_dev::lookup(id) {
        if md.context_window > 0 {
            ctx = md.context_window;
            md_ctx_found = true;
        }
        if md.max_output_tokens > 0 {
            max_out = md.max_output_tokens;
            md_out_found = true;
        }
        if let Some(r) = md.reasoning {
            reasoning = r;
        }
        if let Some(v) = md.vision {
            vision = v;
        }
        if let Some(a) = md.audio {
            audio = a;
        }
        if let Some(f) = md.files {
            files = f;
        }
        if let Some(t) = md.tool_call {
            tools = t;
        }
    }

    if let Some(obj) = raw.and_then(|v| v.as_object()) {
        if let Some(c) = obj.get("context_length")
            .or_else(|| obj.get("context_window"))
            .or_else(|| obj.get("max_context_length"))
            .or_else(|| obj.get("input_token_limit"))
            .or_else(|| obj.get("inputTokenLimit"))
            .or_else(|| obj.get("max_input_tokens"))
            .or_else(|| obj.get("limit").and_then(|v| v.get("context")))
            .or_else(|| obj.get("limits").and_then(|v| v.get("context")))
            .or_else(|| obj.get("top_provider").and_then(|v| v.get("context_length")))
            .and_then(|v| v.as_u64())
        {
            if c > 0 { ctx = if md_ctx_found { ctx.max(c as u32) } else { c as u32 }; }
        }
        if let Some(m) = obj.get("top_provider")
            .and_then(|v| v.get("max_completion_tokens"))
            .or_else(|| obj.get("max_output_tokens"))
            .or_else(|| obj.get("output_token_limit"))
            .or_else(|| obj.get("outputTokenLimit"))
            .or_else(|| obj.get("limit").and_then(|v| v.get("output")))
            .or_else(|| obj.get("limits").and_then(|v| v.get("output")))
            .and_then(|v| v.as_u64())
        {
            if m > 0 { max_out = if md_out_found { max_out.max(m as u32) } else { m as u32 }; }
        }
        if let Some(cap_obj) = obj.get("capabilities").and_then(|v| v.as_object()) {
            if let Some(t) = cap_obj.get("tools").and_then(|v| v.as_bool()) {
                tools = t;
            }
            if let Some(r) = cap_obj.get("reasoning").and_then(|v| v.as_bool()) {
                reasoning = r;
            }
            if let Some(inp) = cap_obj.get("input").and_then(|v| v.as_array()) {
                let mods: Vec<String> = inp.iter().filter_map(|m| m.as_str().map(|s| s.to_lowercase())).collect();
                vision = mods.iter().any(|m| m == "image");
                audio = mods.iter().any(|m| m == "audio");
                files = mods.iter().any(|m| m == "file" || m == "pdf");
            }
        }
        if let Some(r_obj) = obj.get("reasoning").and_then(|v| v.as_object()) {
            let has_efforts = r_obj.get("supported_efforts").and_then(|v| v.as_array()).map(|a| !a.is_empty()).unwrap_or(false);
            let mandatory = r_obj.get("mandatory").and_then(|v| v.as_bool()).unwrap_or(false);
            let def_en = r_obj.get("default_enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            if has_efforts || mandatory || def_en {
                reasoning = true;
            }
        }
        if let Some(params) = obj.get("supported_parameters").and_then(|v| v.as_array()) {
            let pstrs: Vec<String> = params.iter().filter_map(|p| p.as_str().map(|s| s.to_lowercase())).collect();
            if pstrs.iter().any(|p| p.contains("reasoning") || p.contains("thinking")) {
                reasoning = true;
            }
            if pstrs.iter().any(|p| p.contains("tools")) {
                tools = true;
            } else if !pstrs.is_empty() {
                tools = false;
            }
        }
        if let Some(arch) = obj.get("architecture").and_then(|v| v.get("input_modalities")).and_then(|v| v.as_array()) {
            let mods: Vec<String> = arch.iter().filter_map(|m| m.as_str().map(|s| s.to_lowercase())).collect();
            vision = mods.iter().any(|m| m == "image");
            audio = mods.iter().any(|m| m == "audio");
            files = mods.iter().any(|m| m == "file");
        }
    }

    DetectedModelInfo {
        id: id.to_string(),
        name: name.unwrap_or(id).to_string(),
        context_window: ctx,
        max_output_tokens: max_out,
        supports_reasoning: reasoning,
        supports_vision: vision,
        supports_tools: tools,
        supports_audio: audio,
        supports_files: files,
    }
}

#[tauri::command]
async fn detect_model_capabilities(model_id: String) -> Result<DetectedModelInfo, String> {
    llm::models_dev::ensure_loaded().await;
    Ok(build_detected_model(&model_id, None, None))
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[tauri::command]
async fn test_provider_model(
    base_url: String,
    api_key: String,
    model_id: String,
    api_protocol: Option<String>,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<ModelTestResult, String> {
    let started = std::time::Instant::now();
    let client = build_http_client(proxy_enabled, proxy_url.as_deref(), None)?;
    let api_key = llm::KeyPool::parse_api_keys(&api_key).into_iter().next().unwrap_or_default();
    let clean_base = base_url.trim_end_matches('/');
    let proto = api_protocol.unwrap_or_else(|| "auto".to_string()).to_lowercase();
    let bl = clean_base.to_lowercase();
    let is_google = proto == "google_gemini" || (proto == "auto" && bl.contains("generativelanguage.googleapis.com") && !bl.contains("/openai"));
    let is_anth = proto == "anthropic_messages" || (proto == "auto" && bl.contains("anthropic"));
    let is_resp = proto == "responses" || (proto == "auto" && clean_base.ends_with("/responses"));

    let req = if is_google {
        let m = model_id.strip_prefix("models/").unwrap_or(&model_id);
        let url = format!("{}/models/{}:generateContent", clean_base, m);
        client
            .post(&url)
            .header("x-goog-api-key", api_key.trim())
            .json(&json!({
                "contents": [{ "role": "user", "parts": [{ "text": "ping" }] }],
                "generationConfig": { "maxOutputTokens": 1 }
            }))
    } else if is_anth {
        let url = if clean_base.ends_with("/messages") { clean_base.to_string() } else { format!("{}/messages", clean_base) };
        client
            .post(&url)
            .header("x-api-key", api_key.trim())
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model_id,
                "max_tokens": 1,
                "messages": [{ "role": "user", "content": "ping" }]
            }))
    } else if is_resp {
        let url = if clean_base.ends_with("/responses") { clean_base.to_string() } else { format!("{}/responses", clean_base) };
        client
            .post(&url)
            .bearer_auth(api_key.trim())
            .json(&json!({
                "model": model_id,
                "input": "ping",
                "max_output_tokens": 1
            }))
    } else {
        let url = if clean_base.ends_with("/chat/completions") { clean_base.to_string() } else { format!("{}/chat/completions", clean_base) };
        client
            .post(&url)
            .bearer_auth(api_key.trim())
            .json(&json!({
                "model": model_id,
                "messages": [{ "role": "user", "content": "ping" }],
                "max_tokens": 1
            }))
    };

    match req.timeout(std::time::Duration::from_secs(15)).send().await {
        Ok(resp) => {
            let ms = started.elapsed().as_millis() as u64;
            if resp.status().is_success() {
                Ok(ModelTestResult { ok: true, latency_ms: ms, error: None })
            } else {
                let code = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                Ok(ModelTestResult { ok: false, latency_ms: ms, error: Some(format!("HTTP {}: {}", code, body.chars().take(180).collect::<String>())) })
            }
        }
        Err(e) => Ok(ModelTestResult {
            ok: false,
            latency_ms: started.elapsed().as_millis() as u64,
            error: Some(e.to_string()),
        }),
    }
}

#[tauri::command]
async fn fetch_provider_models(
    base_url: String,
    api_key: String,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<DetectedModelInfo>, String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let api_key = llm::KeyPool::parse_api_keys(&api_key).into_iter().next().unwrap_or_default();
    let client = build_http_client(proxy_enabled, proxy_url.as_deref(), None)?;

    let mut req = client.get(&url);
    if !api_key.trim().is_empty() {
        let bl = base_url.to_lowercase();
        if bl.contains("anthropic") {
            req = req.header("x-api-key", api_key.trim()).header("anthropic-version", "2023-06-01");
        } else if bl.contains("generativelanguage.googleapis.com") && !bl.contains("/openai") {
            req = req.header("x-goog-api-key", api_key.trim());
        } else {
            req = req.bearer_auth(api_key.trim());
        }
    }

    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} from {}", resp.status(), url));
    }

    let json_val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    llm::models_dev::ensure_loaded().await;
    let list = json_val.get("data").and_then(|v| v.as_array())
        .or_else(|| json_val.get("models").and_then(|v| v.as_array()))
        .or_else(|| json_val.as_array())
        .ok_or_else(|| "Invalid /models response format".to_string())?;

    let mut out = Vec::new();
    for item in list {
        let raw_id = item.get("id").and_then(|v| v.as_str())
            .or_else(|| item.get("name").and_then(|v| v.as_str()).map(|s| s.strip_prefix("models/").unwrap_or(s)));
        if let Some(id) = raw_id {
            let name = item.get("displayName").and_then(|v| v.as_str())
                .or_else(|| item.get("name").and_then(|v| v.as_str()));
            out.push(build_detected_model(id, name, Some(item)));
        }
    }
    Ok(out)
}

#[tauri::command]
async fn get_default_install_dir() -> Result<String, String> {
    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        return Ok(PathBuf::from(local_app).join("OmniHarness").to_string_lossy().to_string());
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(exe_dir.to_string_lossy().to_string())
}

static EMBEDDED_WEBVIEW2_DLL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/WebView2Loader.dll"));

#[tauri::command]
async fn install_app_to_dir(install_dir: String) -> Result<String, String> {
    let target_dir = PathBuf::from(install_dir.trim());
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_name = current_exe
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("omni-harness.exe"));
    let target_exe = target_dir.join(exe_name);
    if current_exe != target_exe {
        let _ = std::fs::copy(&current_exe, &target_exe);
        if let Some(src_dir) = current_exe.parent() {
            if let Ok(entries) = std::fs::read_dir(src_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                            if ext.eq_ignore_ascii_case("dll") {
                                if let Some(fname) = p.file_name() {
                                    let _ = std::fs::copy(&p, target_dir.join(fname));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let wv2_dest = target_dir.join("WebView2Loader.dll");
    if !wv2_dest.exists() && !EMBEDDED_WEBVIEW2_DLL.is_empty() {
        let _ = std::fs::write(&wv2_dest, EMBEDDED_WEBVIEW2_DLL);
    }
    Ok(target_exe.to_string_lossy().to_string())
}

#[tauri::command]
async fn set_windows_autostart(enabled: bool, target_path: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        if enabled {
            let exe_path = match target_path {
                Some(p) if !p.trim().is_empty() => p,
                _ => std::env::current_exe().map_err(|e| e.to_string())?.to_string_lossy().to_string(),
            };
            let quoted = format!("\"{}\"", exe_path);
            let _ = std::process::Command::new("reg")
                .args(["add", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "OmniHarness", "/t", "REG_SZ", "/d", &quoted, "/f"])
                .creation_flags(0x08000000)
                .output();
        } else {
            let _ = std::process::Command::new("reg")
                .args(["delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "OmniHarness", "/f"])
                .creation_flags(0x08000000)
                .output();
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (enabled, target_path);
    }
    Ok(())
}

#[tauri::command]
async fn create_desktop_shortcut(target_path: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let exe_path = match target_path {
            Some(p) if !p.trim().is_empty() => PathBuf::from(p),
            _ => std::env::current_exe().map_err(|e| e.to_string())?,
        };
        let exe_str = exe_path.to_string_lossy().replace("'", "''");
        let work_dir = exe_path
            .parent()
            .map(|p| p.to_string_lossy().replace("'", "''"))
            .unwrap_or_default();
        let ps = format!(
            "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Desktop')) 'OmniHarness.lnk')); $s.TargetPath = '{}'; $s.WorkingDirectory = '{}'; $s.IconLocation = '{}'; $s.Save()",
            exe_str, work_dir, exe_str
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &ps])
            .creation_flags(0x08000000)
            .output()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target_path;
    }
    Ok(())
}

#[tauri::command]
async fn open_path_in_explorer(path: String) -> Result<(), String> {
    let raw = path.trim();
    let stripped = raw
        .strip_prefix("file:///")
        .or_else(|| raw.strip_prefix("file://"))
        .unwrap_or(raw);
    let decoded = urlencoding::decode(stripped)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| stripped.replace("%20", " "));
    let mut normalized = decoded.replace('/', "\\");
    if let Some(rest) = normalized.strip_prefix(r"\\?\") {
        normalized = rest.to_string();
    }
    let p = std::path::Path::new(&normalized);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        if p.is_file() {
            std::process::Command::new("explorer.exe")
                .raw_arg(format!("/select,\"{}\"", normalized))
                .spawn()
                .map_err(|e| e.to_string())?;
        } else if p.is_dir() {
            std::process::Command::new("explorer.exe")
                .raw_arg(format!("\"{}\"", normalized))
                .spawn()
                .map_err(|e| e.to_string())?;
        } else if let Some(parent) = p.parent().filter(|pp| pp.exists()) {
            std::process::Command::new("explorer.exe")
                .raw_arg(format!("\"{}\"", parent.to_string_lossy()))
                .spawn()
                .map_err(|e| e.to_string())?;
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = p;
    }
    Ok(())
}

#[tauri::command]
async fn oauth_list_accounts(provider: Option<String>) -> Result<Vec<llm::oauth_accounts::OAuthAccountPublic>, String> {
    Ok(llm::oauth_accounts::list_accounts(provider.as_deref()).await)
}

fn oauth_client_with_fallback(proxy_enabled: Option<bool>, proxy_url: Option<&str>) -> Result<reqwest::Client, String> {
    let (en, url) = llm::oauth_accounts::resolve_oauth_proxy(proxy_enabled, proxy_url);
    build_http_client(en, url.as_deref(), None)
}

#[tauri::command]
async fn oauth_get_proxy() -> Result<llm::oauth_accounts::OAuthProxyConfig, String> {
    Ok(llm::oauth_accounts::load_oauth_proxy())
}

#[tauri::command]
async fn oauth_set_proxy(
    enabled: bool,
    url: String,
    proxy_pool: Option<Vec<String>>,
    rotation_mode: Option<String>,
) -> Result<(), String> {
    let prev = llm::oauth_accounts::load_oauth_proxy();
    let pool = proxy_pool.unwrap_or(prev.proxy_pool);
    let mode = rotation_mode
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(prev.rotation_mode);
    llm::oauth_accounts::save_oauth_proxy(llm::oauth_accounts::OAuthProxyConfig {
        enabled,
        url,
        proxy_pool: pool,
        rotation_mode: mode,
    })
}

#[tauri::command]
async fn oauth_set_account_proxy(id: String, proxy_url: Option<String>) -> Result<(), String> {
    llm::oauth_accounts::set_account_proxy(&id, proxy_url).await
}

#[tauri::command]
async fn oauth_start_login(
    app: tauri::AppHandle,
    provider: String,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::start_oauth_login(app, client, &provider).await
}

#[tauri::command]
async fn oauth_complete_manual_login(
    app: tauri::AppHandle,
    provider: String,
    code_or_url: String,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<llm::oauth_accounts::OAuthAccountPublic, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::complete_manual_oauth(app, client, &provider, &code_or_url).await
}

#[tauri::command]
async fn oauth_add_manual_token(
    app: tauri::AppHandle,
    provider: String,
    label: String,
    token: String,
) -> Result<llm::oauth_accounts::OAuthAccountPublic, String> {
    llm::oauth_accounts::add_manual_token_account(app, &provider, &label, &token).await
}

#[tauri::command]
async fn oauth_import_local_accounts(
    app: tauri::AppHandle,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<llm::oauth_accounts::OAuthAccountPublic>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::import_local_accounts(app, client).await
}

#[tauri::command]
async fn oauth_toggle_account(id: String, enabled: bool) -> Result<(), String> {
    llm::oauth_accounts::toggle_account(&id, enabled).await
}

#[tauri::command]
async fn oauth_delete_account(id: String) -> Result<(), String> {
    llm::oauth_accounts::delete_account(&id).await
}

#[tauri::command]
async fn oauth_check_account(
    id: String,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<llm::oauth_accounts::OAuthAccountPublic, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::check_account_liveness(&client, &id).await
}

#[tauri::command]
async fn oauth_refresh_quotas(
    provider: Option<String>,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<llm::oauth_accounts::OAuthAccountPublic>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::refresh_all_quotas(&client, provider.as_deref()).await
}

#[tauri::command]
async fn oauth_fetch_antigravity_models(
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::fetch_antigravity_models(&client).await
}

#[tauri::command]
async fn oauth_fetch_codex_models(
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::fetch_codex_models(&client).await
}

#[tauri::command]
async fn oauth_fetch_claude_models(
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::fetch_claude_models(&client).await
}

#[tauri::command]
async fn oauth_fetch_kimi_models(
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::fetch_kimi_models(&client).await
}

#[tauri::command]
async fn oauth_fetch_xai_models(
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::fetch_xai_models(&client).await
}

#[tauri::command]
async fn oauth_fetch_zcode_models(
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::fetch_zcode_models(&client).await
}

#[tauri::command]
async fn oauth_fetch_perplexity_models(
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let client = oauth_client_with_fallback(proxy_enabled, proxy_url.as_deref())?;
    llm::oauth_accounts::fetch_perplexity_models(&client).await
}

#[tauri::command]
async fn list_subagents(
    state: State<'_, AppManagedState>,
) -> Result<Vec<engine::subagents::SubagentInfo>, String> {
    let ws = state.app_state.get_workspace_dir().await;
    Ok(engine::subagents::list_all_subagents(&ws.to_string_lossy()))
}

#[tauri::command]
async fn save_custom_subagent(
    state: State<'_, AppManagedState>,
    name: String,
    description: String,
    tools: Vec<String>,
    disallowed_tools: Vec<String>,
    model: Option<String>,
    max_turns: Option<u32>,
    system_prompt: String,
    scope: String,
) -> Result<engine::subagents::SubagentInfo, String> {
    let ws = state.app_state.get_workspace_dir().await;
    let def = engine::subagents::CustomSubagentDef {
        name,
        description,
        tools,
        disallowed_tools,
        model,
        max_turns,
        system_prompt,
    };
    engine::subagents::save_custom_subagent(&ws.to_string_lossy(), def, &scope)
}

#[tauri::command]
async fn delete_custom_subagent(
    state: State<'_, AppManagedState>,
    name: String,
    file_path: Option<String>,
) -> Result<(), String> {
    let ws = state.app_state.get_workspace_dir().await;
    engine::subagents::delete_custom_subagent(&ws.to_string_lossy(), &name, file_path.as_deref())
}

#[tauri::command]
async fn list_skills(
    state: State<'_, AppManagedState>,
) -> Result<Vec<skills::SkillPackage>, String> {
    let ws = state.app_state.get_workspace_dir().await;
    Ok(skills::discover_all_skills(&ws.to_string_lossy())
        .into_iter()
        .filter(|sk| !sk.metadata.name.starts_with("system:"))
        .collect())
}

#[tauri::command]
async fn delete_skill(
    state: State<'_, AppManagedState>,
    skill_path: String,
    skill_name: String,
) -> Result<(), String> {
    let _ = &state;
    let s_name = skill_name.trim();
    if s_name.eq_ignore_ascii_case("skill-creator") || s_name.starts_with("system:") {
        return Err("Системный навык 'skill-creator' защищен от удаления".to_string());
    }
    let skills_root = state::AppState::resolve_skills_dir();
    let mut target = if !skill_path.trim().is_empty() {
        PathBuf::from(skill_path.trim())
    } else {
        skills_root.join(s_name)
    };
    if target.is_file() && target.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
        if let Some(parent) = target.parent() {
            target = parent.to_path_buf();
        }
    }
    let canon_target = crate::tools::path_norm::canonicalize_best_effort(&target)
        .map_err(|e| format!("Skill path not found: {}", e))?;
    let canon_root = crate::tools::path_norm::canonicalize_best_effort(&skills_root)
        .map_err(|e| format!("Skills root directory not found: {}", e))?;

    if canon_target
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("skill-creator"))
        .unwrap_or(false)
    {
        return Err("Системный навык 'skill-creator' защищен от удаления".to_string());
    }

    if !crate::tools::path_norm::is_within_root(&canon_target, &canon_root)
        || crate::tools::path_norm::path_key(&canon_target) == crate::tools::path_norm::path_key(&canon_root)
        || !canon_target.join("SKILL.md").exists()
    {
        return Err("Refusing to delete path outside registered skills directory".to_string());
    }
    skills::remove_dir_all_force(&canon_target).map_err(|e| e.to_string())
}

#[tauri::command]
async fn import_skill_zip(state: State<'_, AppManagedState>, path: String) -> Result<(), String> {
    let _ = &state;
    let skills_dir = state::AppState::resolve_skills_dir();
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    skills::import_skill_zip(&bytes, &skills_dir).map(|_| ())
}

#[tauri::command]
async fn export_skill_zip(
    state: State<'_, AppManagedState>,
    skill_name: String,
    destination: String,
) -> Result<(), String> {
    let _ = &state;
    let skills_dir = state::AppState::resolve_skills_dir();
    let bytes = skills::export_skill_zip(&skill_name, &skills_dir)?;
    std::fs::write(&destination, bytes).map_err(|e| e.to_string())
}

#[tauri::command]
async fn transcribe_audio(
    state: State<'_, AppManagedState>,
    audio_base64: Option<String>,
    audio_data: Option<Vec<u8>>,
    model: Option<String>,
    language: Option<String>,
) -> Result<String, String> {
    use base64::Engine;
    let bytes = if let Some(b64) = audio_base64.filter(|s| !s.trim().is_empty()) {
        let payload = match b64.find("base64,") {
            Some(pos) => b64[pos + 7..].to_string(),
            None => b64,
        };
        base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .map_err(|e| format!("Invalid base64 audio payload: {}", e))?
    } else if let Some(raw) = audio_data.filter(|d| !d.is_empty()) {
        raw
    } else {
        return Err("No audio payload received".to_string());
    };
    let fallback_key = state.app_state.get_credential("openai_api_key").await.ok();
    let res = state
        .speech_engine
        .transcribe_audio(&bytes, fallback_key.as_deref(), model.as_deref(), language.as_deref())
        .await?;
    Ok(res.text)
}

#[tauri::command]
async fn queue_list(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Vec<engine::prompt_queue::QueuedPrompt>, String> {
    Ok(state.prompt_queue.list(&chat_id).await)
}

#[tauri::command]
async fn queue_add(
    app: tauri::AppHandle,
    state: State<'_, AppManagedState>,
    chat_id: String,
    text: String,
    attachments: Option<Vec<AttachmentInput>>,
    priority: Option<String>,
) -> Result<Vec<engine::prompt_queue::QueuedPrompt>, String> {
    let mut item = engine::prompt_queue::QueuedPrompt::new(&chat_id, text, "app");
    if let Some(p) = priority.filter(|p| !p.trim().is_empty()) {
        item.priority = p;
    }
    if let Some(atts) = attachments {
        item.attachments = atts
            .into_iter()
            .map(|a| engine::prompt_queue::QueuedAttachment {
                name: a.name.unwrap_or_default(),
                path: a.path.unwrap_or_default(),
                kind: a.kind.unwrap_or_default(),
                data_url: a.data_url,
            })
            .collect();
    }
    let items = state.prompt_queue.add(item).await;
    let _ = app.emit(
        "prompt-queue-updated",
        json!({ "chatId": chat_id, "items": items }),
    );
    Ok(items)
}

#[tauri::command]
async fn queue_remove(
    app: tauri::AppHandle,
    state: State<'_, AppManagedState>,
    chat_id: String,
    id: String,
) -> Result<Vec<engine::prompt_queue::QueuedPrompt>, String> {
    let items = state.prompt_queue.remove(&chat_id, &id).await;
    let _ = app.emit(
        "prompt-queue-updated",
        json!({ "chatId": chat_id, "items": items }),
    );
    Ok(items)
}

#[tauri::command]
async fn queue_clear(
    app: tauri::AppHandle,
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Vec<engine::prompt_queue::QueuedPrompt>, String> {
    let items = state.prompt_queue.clear(&chat_id).await;
    let _ = app.emit(
        "prompt-queue-updated",
        json!({ "chatId": chat_id, "items": items }),
    );
    Ok(items)
}

#[tauri::command]
async fn queue_take_next(
    app: tauri::AppHandle,
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<Option<engine::prompt_queue::QueuedPrompt>, String> {
    let taken = state.prompt_queue.take_next(&chat_id).await;
    if taken.is_some() {
        let items = state.prompt_queue.list(&chat_id).await;
        let _ = app.emit(
            "prompt-queue-updated",
            json!({ "chatId": chat_id, "items": items }),
        );
    }
    Ok(taken)
}

#[tauri::command]
async fn queue_take_specific(
    app: tauri::AppHandle,
    state: State<'_, AppManagedState>,
    chat_id: String,
    id: String,
) -> Result<Option<engine::prompt_queue::QueuedPrompt>, String> {
    let taken = state.prompt_queue.take_specific(&chat_id, &id).await;
    if taken.is_some() {
        let items = state.prompt_queue.list(&chat_id).await;
        let _ = app.emit(
            "prompt-queue-updated",
            json!({ "chatId": chat_id, "items": items }),
        );
    }
    Ok(taken)
}

#[tauri::command]
async fn model_native_audio(model_id: String) -> Result<bool, String> {
    Ok(llm::capabilities::model_supports_native_audio(&model_id))
}

#[tauri::command]
async fn get_speech_status(
    state: State<'_, AppManagedState>,
) -> Result<Vec<speech::WhisperModelInfo>, String> {
    Ok(state.speech_engine.get_available_models())
}

#[tauri::command]
async fn download_whisper_model(
    app: tauri::AppHandle,
    state: State<'_, AppManagedState>,
    model_id: String,
) -> Result<String, String> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(u64, u64)>(32);
    let app_c = app.clone();
    let mid_c = model_id.clone();
    tokio::spawn(async move {
        let mut last_pct: i32 = -1;
        while let Some((dl, total)) = rx.recv().await {
            let pct = if total > 0 { ((dl as f64 / total as f64) * 100.0).round() as i32 } else { 0 };
            if pct != last_pct {
                last_pct = pct;
                let _ = tauri::Emitter::emit(&app_c, "whisper-download-progress", serde_json::json!({
                    "modelId": mid_c,
                    "downloaded": dl,
                    "total": total,
                    "percent": pct.clamp(0, 100)
                }));
            }
        }
    });
    let app_done = app.clone();
    let mid_done = model_id.clone();
    let res = state
        .speech_engine
        .download_model(&model_id, Some(tx))
        .await;
    match res {
        Ok(path) => {
            let _ = tauri::Emitter::emit(&app_done, "whisper-download-finished", serde_json::json!({
                "modelId": mid_done,
                "status": "installed"
            }));
            Ok(path.to_string_lossy().to_string())
        }
        Err(e) => {
            let _ = tauri::Emitter::emit(&app_done, "whisper-download-finished", serde_json::json!({
                "modelId": mid_done,
                "status": "error",
                "error": e
            }));
            Err(e)
        }
    }
}

#[tauri::command]
async fn delete_whisper_model(
    state: State<'_, AppManagedState>,
    model_id: String,
) -> Result<(), String> {
    state.speech_engine.delete_model(&model_id)
}


#[tauri::command]
async fn sync_providers_to_backend(
    state: State<'_, AppManagedState>,
    providers: Vec<telegram::SyncedProviderConfig>,
    active_provider: String,
    active_model: String,
) -> Result<(), String> {
    for prov in &providers {
        if let Some(ref caps_map) = prov.model_capabilities {
            for (mid, cap) in caps_map {
                state.caps_registry.update_full_model_caps(
                    mid,
                    Some(cap.context_window),
                    Some(cap.max_output_tokens),
                    Some(cap.supports_reasoning),
                    Some(cap.supports_vision),
                    Some(cap.supports_tools),
                );
            }
        }
    }
    let snap = telegram::ProvidersSnapshotFile {
        providers,
        active_provider,
        active_model,
    };
    state.telegram_mgr.sync_providers(snap).await
}

#[tauri::command]
async fn remove_telegram_paired_chat(
    state: State<'_, AppManagedState>,
    chat_id: String,
) -> Result<(), String> {
    state.telegram_mgr.remove_paired_chat(&chat_id).await
}

#[tauri::command]
async fn get_telegram_config(
    state: State<'_, AppManagedState>,
) -> Result<serde_json::Value, String> {
    let cfg = state.telegram_mgr.get_config_snapshot().await;
    let token = state.telegram_mgr.get_token().await.unwrap_or(cfg.token);
    let chats = state.telegram_mgr.get_allowed_chats().await;
    let running = state.telegram_mgr.is_running().await;
    let code = state.telegram_mgr.get_or_generate_pairing_code().await;

    Ok(json!({
        "botToken": token,
        "pairingCode": code,
        "pairedChats": cfg.paired_chats,
        "allowedChatIds": chats,
        "isRunning": running,
        "notifyOnComplete": cfg.notify_on_complete,
        "notifyOnApproval": cfg.notify_on_approval,
        "notifyOnError": cfg.notify_on_error,
        "autoStart": cfg.auto_start,
        "proxyUrl": cfg.proxy_url,
    }))
}

#[tauri::command]
async fn start_telegram_bot(
    state: State<'_, AppManagedState>,
    token: String,
    allowed_chats: Option<Vec<String>>,
) -> Result<(), String> {
    let clean = token.trim().to_string();
    if clean.is_empty() {
        return Err("Bot token cannot be empty".to_string());
    }
    state.telegram_mgr.set_token(clean).await;
    if let Some(list) = allowed_chats {
        for raw in list {
            if let Ok(id) = raw.trim().parse::<i64>() {
                state.telegram_mgr.add_allowed_chat(id).await;
            }
        }
    }
    state.telegram_mgr.start().await
}

#[tauri::command]
async fn stop_telegram_bot(state: State<'_, AppManagedState>) -> Result<(), String> {
    state.telegram_mgr.stop().await;
    Ok(())
}

#[tauri::command]
async fn save_telegram_config(
    state: State<'_, AppManagedState>,
    token: String,
    paired_chats: Vec<telegram::PairedChatEntry>,
    notify_on_complete: bool,
    notify_on_approval: bool,
    notify_on_error: bool,
    auto_start: Option<bool>,
    proxy_url: Option<String>,
) -> Result<(), String> {
    let prev = state.telegram_mgr.get_config_snapshot().await;
    let cfg = telegram::TelegramConfigFile {
        token: token.trim().to_string(),
        paired_chats,
        notify_on_complete,
        notify_on_approval,
        notify_on_error,
        auto_start: auto_start.unwrap_or(prev.auto_start),
        proxy_url: proxy_url.map(|s| s.trim().to_string()).unwrap_or_else(|| prev.proxy_url.clone()),
        offset: prev.offset,
        chat_sessions: prev.chat_sessions,
        chat_providers: prev.chat_providers,
        chat_models: prev.chat_models,
        chat_reasoning: prev.chat_reasoning,
        ..prev
    };
    state.telegram_mgr.save_config(cfg).await
}

#[tauri::command]
async fn set_telegram_running(state: State<'_, AppManagedState>, run: bool) -> Result<(), String> {
    if run {
        state.telegram_mgr.start().await
    } else {
        state.telegram_mgr.stop().await;
        Ok(())
    }
}

#[tauri::command]
async fn generate_pairing_code(state: State<'_, AppManagedState>) -> Result<String, String> {
    Ok(state.telegram_mgr.generate_pairing_code().await)
}

#[tauri::command]
async fn verify_pairing_code(
    state: State<'_, AppManagedState>,
    code: String,
    chat_id: i64,
) -> Result<bool, String> {
    Ok(state.telegram_mgr.verify_pairing_code(&code, chat_id).await)
}

pub fn run() {
    let app_state_inner = state::AppState::new().expect("Failed to initialize AppState");
    let data_dir = app_state_inner.data_dir.clone();
    let initial_ws = state::AppState::resolve_install_workspace();

    let speech_eng = Arc::new(speech::SpeechEngine::new(&data_dir));
    let stream_m = Arc::new(engine::StreamManager::new());
    let mcp_cat = Arc::new(mcp::McpServerCatalog::new());
    let tele_m = Arc::new(telegram::TelegramBotManager::new());
    let kp = Arc::new(llm::KeyPool::new());
    let cr = Arc::new(llm::CapabilityRegistry::new());
    let chat_tmp_root = state::AppState::resolve_chat_temp_dir(None);
    let td = Arc::new(tools::ToolDispatcher::new(vec![initial_ws, chat_tmp_root], None));
    td.set_db(app_state_inner.db.clone());
    let shared_http = build_http_client(None, None, None).unwrap_or_else(|_| reqwest::Client::new());

    let prompt_queue_arc = Arc::new(engine::prompt_queue::PromptQueue::new(&data_dir));

    let app_state_arc = Arc::new(app_state_inner);
    let active_contracts_arc = Arc::new(RwLock::new(std::collections::HashMap::new()));
    let tg_app_handle = Arc::new(RwLock::new(None));
    let tg_app_handle_setup = tg_app_handle.clone();
    let tg_ctx = telegram::TelegramRuntimeContext {
        app_handle: tg_app_handle,
        app_state: app_state_arc.clone(),
        stream_mgr: stream_m.clone(),
        tool_dispatcher: td.clone(),
        key_pool: kp.clone(),
        caps_registry: cr.clone(),
        http_client: shared_http.clone(),
        speech_engine: speech_eng.clone(),
        active_contracts: active_contracts_arc.clone(),
        prompt_queue: prompt_queue_arc.clone(),
    };
    let md_dir = data_dir.clone();
    tauri::async_runtime::spawn(async move {
        llm::models_dev::init_and_refresh(md_dir).await;
    });
    tauri::async_runtime::block_on(async {
        tele_m.load_from_disk(&data_dir).await;
        tele_m.set_runtime_context(tg_ctx).await;
    });

    let managed = AppManagedState {
        app_state: app_state_arc,
        stream_mgr: stream_m,
        mcp_catalog: mcp_cat,
        telegram_mgr: tele_m,
        speech_engine: speech_eng,
        prompt_queue: prompt_queue_arc,
        tool_dispatcher: td,
        key_pool: kp,
        caps_registry: cr,
        http_client: shared_http,
        active_contracts: active_contracts_arc,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .manage(managed)
        .setup(move |app| {
            let h = app.handle().clone();
            let target = tg_app_handle_setup.clone();
            let tg_mgr_boot = h.state::<AppManagedState>().telegram_mgr.clone();
            tauri::async_runtime::spawn(async move {
                *target.write().await = Some(h);
                let cfg = tg_mgr_boot.get_config_snapshot().await;
                if cfg.auto_start && !cfg.token.trim().is_empty() {
                    let _ = tg_mgr_boot.start().await;
                }
            });
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show OmniHarness", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let mut tray_builder = TrayIconBuilder::new().menu(&menu);
            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }
            let _tray = tray_builder
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        let st = app.state::<AppManagedState>();
                        let _ = st.tool_dispatcher.pty_manager.kill_all();
                        let tg = st.telegram_mgr.clone();
                        let mcp = st.mcp_catalog.clone();
                        let _ = std::thread::spawn(move || {
                            tauri::async_runtime::block_on(async move {
                                let _ = tokio::time::timeout(
                                    std::time::Duration::from_secs(2),
                                    async {
                                        tg.stop().await;
                                        mcp.disconnect_all().await;
                                    },
                                )
                                .await;
                            });
                        })
                        .join();
                        let _ = tools::phantom_session::restore_all_staged_windows(true);
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(win) = tray.app_handle().get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            logger::init(state::AppState::resolve_data_dir());
            logger::info("App", "OmniHarness started");
            tools::phantom_session::recover_orphaned_staged_windows();
            if let (Some(win), Some(icon)) = (app.get_webview_window("main"), app.default_window_icon()) {
                let _ = win.set_icon(icon.clone());
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppManagedState>();
                if state.has_active_tasks() {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    let _ = state.tool_dispatcher.pty_manager.kill_all();
                    let _ = tools::phantom_session::restore_all_staged_windows(true);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            window_show,
            window_minimize,
            window_toggle_maximize,
            window_toggle_fullscreen,
            window_close,
            window_start_dragging,
            create_project,
            list_projects,
            get_project,
            delete_project,
            create_chat,
            list_chats,
            get_chat,
            update_chat_title,
            update_chat_pinned,
            delete_chat,
            read_file_full,
            create_message,
            list_messages,
            delete_messages_since,
            delete_message,
            record_file_snapshot,
            get_file_snapshot,
            list_snapshots_for_chat,
            rollback_file_snapshot,
            get_turn_file_diffs,
            get_events_since,
            list_session_checkpoints,
            rewind_checkpoint,
            record_usage_stat,
            list_usage_stats,
            create_provider,
            list_providers,
            get_provider,
            update_provider,
            delete_provider,
            ping_provider,
            create_model,
            list_models,
            get_model,
            update_model,
            delete_model,
            create_mcp_server,
            list_mcp_servers,
            get_mcp_server,
            update_mcp_server,
            delete_mcp_server,
            connect_mcp_server,
            disconnect_mcp_server,
            list_mcp_tools,
            import_mcp_config,
            call_mcp_tool,
            create_memory_entry,
            list_memory_entries,
            get_memory_entry,
            update_memory_entry,
            delete_memory_entry,
            search_memory_entries,
            get_total_memory_tokens,
            compact_memory_entries,
            list_workspace_files,
            search_workspace_files,
            sync_chat_context,
            get_cached_chat_messages,
            get_chat_recovery_snapshot,
            save_ui_pending_queues,
            load_ui_pending_queues,
            get_bootstrap_config,
            complete_first_run,
            get_workspace_dir,
            set_workspace_dir,
            get_credential,
            set_credential,
            delete_credential,
            start_chat_stream,
            ask_side_question,
            cancel_stream,
            get_goal_state,
            clear_goal_state,
            send_intervention,
            fetch_provider_models,
            test_provider_model,
            detect_model_capabilities,
            open_path_in_explorer,
            get_default_install_dir,
            install_app_to_dir,
            set_windows_autostart,
            create_desktop_shortcut,
            oauth_list_accounts,
            oauth_start_login,
            oauth_complete_manual_login,
            oauth_import_local_accounts,
            oauth_toggle_account,
            oauth_delete_account,
            oauth_fetch_antigravity_models,
            oauth_fetch_codex_models,
            oauth_get_proxy,
            oauth_set_proxy,
            oauth_set_account_proxy,
            oauth_add_manual_token,
            oauth_check_account,
            oauth_refresh_quotas,
            oauth_fetch_claude_models,
            oauth_fetch_kimi_models,
            oauth_fetch_xai_models,
            oauth_fetch_zcode_models,
            oauth_fetch_perplexity_models,
            list_themes,
            save_theme,
            delete_theme,
            get_active_theme_id,
            set_active_theme_id,
            start_theme_agent_stream,
            open_zcode_oauth_window,
            get_debug_log_path,
            get_settings,
            save_settings,
            check_github_update,
            install_github_update,
            list_subagents,
            save_custom_subagent,
            delete_custom_subagent,
            list_skills,
            delete_skill,
            import_skill_zip,
            export_skill_zip,
            transcribe_audio,
            queue_list,
            queue_add,
            queue_remove,
            queue_clear,
            queue_take_next,
            queue_take_specific,
            model_native_audio,
            get_speech_status,
            download_whisper_model,
            delete_whisper_model,
            get_telegram_config,
            start_telegram_bot,
            stop_telegram_bot,
            save_telegram_config,
            remove_telegram_paired_chat,
            sync_providers_to_backend,
            sync_workspaces_to_backend,
            set_telegram_running,
            generate_pairing_code,
            verify_pairing_code,
            cu_get_status,
            cu_set_policy,
            cu_stage_window,
            cu_restore_all_windows,
            cu_preview_window,
            cu_close_window,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running OmniHarness application");
}


#[tauri::command]
async fn list_themes() -> Result<Vec<themes::OmniTheme>, String> {
    themes::list_themes()
}

#[tauri::command]
async fn save_theme(theme: themes::OmniTheme) -> Result<themes::OmniTheme, String> {
    themes::save_theme(theme)
}

#[tauri::command]
async fn delete_theme(id: String) -> Result<(), String> {
    themes::delete_theme(&id)
}

#[tauri::command]
async fn get_active_theme_id() -> Result<Option<String>, String> {
    themes::get_active_theme_id()
}

#[tauri::command]
async fn set_active_theme_id(id: Option<String>) -> Result<(), String> {
    themes::set_active_theme_id(id)
}

#[tauri::command]
async fn start_theme_agent_stream(
    state: State<'_, AppManagedState>,
    app: tauri::AppHandle,
    theme_id: String,
    prompt: String,
    model_id: String,
    reasoning_effort: String,
    provider_name: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_protocol: Option<String>,
    proxy_enabled: Option<bool>,
    proxy_url: Option<String>,
    history: Option<Vec<HistoryInputItem>>,
    on_event: Channel<serde_json::Value>,
) -> Result<String, String> {
    let chat_id = format!("theme-studio-{}", theme_id);
    let session = state.stream_mgr.create_session(&chat_id).await;
    let tools = state.tool_dispatcher.for_theme_studio(app.clone(), &theme_id);
    let key_pool = (*state.key_pool).clone();
    let caps_reg = (*state.caps_registry).clone();

    let is_oauth_proto = matches!(
        api_protocol.as_deref().unwrap_or(""),
        "antigravity" | "openai_codex" | "claude_oauth" | "kimi_oauth" | "xai_oauth" | "zcode_oauth" | "perplexity_oauth"
    );
    let (eff_px_en, eff_px_url) = if is_oauth_proto {
        llm::oauth_accounts::resolve_oauth_proxy(proxy_enabled, proxy_url.as_deref())
    } else {
        (proxy_enabled, proxy_url.clone())
    };
    let http_client = build_http_client(eff_px_en, eff_px_url.as_deref(), Some(&state.http_client))?;
    let prov_name = provider_name.unwrap_or_else(|| "OpenAI".to_string());
    let b_url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    if let Some(ak) = api_key {
        if !ak.trim().is_empty() {
            key_pool.set_keys(&prov_name, vec![ak.trim().to_string()]);
        }
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel(128);
    let on_event_clone = on_event.clone();
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let val = match evt {
                llm::StreamEvent::ContentDelta { delta } => json!({ "type": "delta", "content": delta }),
                llm::StreamEvent::ReasoningDelta { delta } => json!({ "type": "reasoning_delta", "content": delta }),
                llm::StreamEvent::ToolCallComplete { id, name, arguments, .. } => {
                    json!({ "type": "tool_call_complete", "id": id, "name": name, "arguments": arguments })
                }
                llm::StreamEvent::ToolExecEnd { id, success, output, error, elapsed_ms } => {
                    json!({ "type": "tool_end", "id": id, "success": success, "output": output, "error": error, "elapsedMs": elapsed_ms })
                }
                llm::StreamEvent::Finish { usage, .. } => json!({ "type": "done", "usage": usage }),
                llm::StreamEvent::Error { error } => json!({ "type": "error", "error": error }),
                _ => continue,
            };
            let _ = on_event_clone.send(val);
        }
    });

    let sys_prompt = "You are OmniHarness Theme Designer, a specialized UI/CSS/SVG engineer. Your ONLY job is to customize the OmniHarness interface theme in real time using your theme_* tools. Always call theme_get_dom_spec and theme_read_current first, then apply clean, valid CSS (using [data-omni-*] selectors, CSS variables, @keyframes, @font-face, omni-asset:// URLs), custom SVG icons via theme_set_icon, and UI text/language overrides via theme_set_texts. Never invent files or commands outside your theme_* tools.".to_string();

    let mut msgs = vec![llm::request_compat::ChatMessage::system(sys_prompt)];
    if let Some(hist) = history {
        for h in hist {
            if h.role == "user" {
                msgs.push(llm::request_compat::ChatMessage::user(h.content));
            } else if h.role == "assistant" {
                msgs.push(llm::request_compat::ChatMessage::assistant(h.content));
            }
        }
    }

    let ws_dir = state::AppState::resolve_data_dir().join("themes").to_string_lossy().to_string();
    let app_state = state.app_state.clone();
    let stream_mgr = state.stream_mgr.clone();
    let cid_clone = chat_id.clone();

    tokio::spawn(async move {
        let mut contract: Option<TaskContract> = None;
        let res = engine::single_agent::SingleAgentRunner::run_turn(
            &http_client,
            &cid_clone,
            &prompt,
            &ws_dir,
            tools,
            &key_pool,
            &caps_reg,
            &prov_name,
            &b_url,
            api_protocol.as_deref(),
            &model_id,
            &reasoning_effort,
            None,
            Some(&app_state.db),
            false,
            &session,
            &mut contract,
            &mut msgs,
            tx.clone(),
        )
        .await;
        if let Err(e) = res {
            let _ = tx.send(llm::StreamEvent::Error { error: e }).await;
        }
        stream_mgr.remove_session_if_current(&cid_clone, &session).await;
    });

    Ok(chat_id)
}


#[tauri::command]
async fn get_debug_log_path() -> String {
    logger::get_log_path()
}

#[tauri::command]
async fn open_zcode_oauth_window(app: tauri::AppHandle, auth_url: String) -> Result<(), String> {
    use tauri::Emitter;
    let parsed: tauri::Url = auth_url.parse().map_err(|e| format!("Invalid auth URL: {}", e))?;
    if let Some(existing) = app.get_webview_window("zcode-oauth") {
        let _ = existing.close();
    }
    let app_for_nav = app.clone();
    let app_for_close = app.clone();
    let win = tauri::WebviewWindowBuilder::new(&app, "zcode-oauth", tauri::WebviewUrl::External(parsed))
        .title("ZCode OAuth Login")
        .inner_size(520.0, 700.0)
        .center()
        .on_navigation(move |nav_url| {
            let s = nav_url.as_str();
            if s.starts_with("zcode://") || (s.contains("code=") && s.contains("state=")) {
                let _ = app_for_nav.emit("zcode-oauth-callback", s.to_string());
                if let Some(w) = app_for_close.get_webview_window("zcode-oauth") {
                    let _ = w.close();
                }
                return false;
            }
            true
        })
        .build()
        .map_err(|e| e.to_string())?;
    let _ = win.set_focus();
    Ok(())
}

#[tauri::command]
async fn cu_get_status() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(tools::phantom_session::get_status_snapshot)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cu_set_policy(
    policy: tools::phantom_session::ComputerUsePolicy,
) -> Result<tools::phantom_session::ComputerUsePolicy, String> {
    tokio::task::spawn_blocking(move || tools::phantom_session::set_policy(policy))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cu_stage_window(hwnd: usize, offscreen: bool) -> Result<tools::desktop_uia::WindowInfo, String> {
    tokio::task::spawn_blocking(move || {
        if offscreen {
            tools::phantom_session::stage_window_offscreen(hwnd, false, None, None)
        } else {
            tools::phantom_session::restore_staged_window(hwnd)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cu_restore_all_windows() -> Result<usize, String> {
    tokio::task::spawn_blocking(|| tools::phantom_session::restore_all_staged_windows(false))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cu_preview_window(hwnd: usize) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        tools::desktop_uia::capture_window_or_desktop_png(Some(hwnd), None, None)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn cu_close_window(hwnd: usize) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || tools::desktop_uia::close_window_background(hwnd))
        .await
        .map_err(|e| e.to_string())?
}


#[tauri::command]
async fn get_settings() -> Result<Option<serde_json::Value>, String> {
    tokio::task::spawn_blocking(|| {
        let path = state::AppState::resolve_data_dir().join("settings.json");
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        Ok(Some(parsed))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn save_settings(settings: serde_json::Value) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let path = state::AppState::resolve_data_dir().join("settings.json");
        let json_str = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
        db::atomic_write(&path, json_str.as_bytes()).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn check_github_update(state: State<'_, AppManagedState>) -> Result<updater::UpdateInfo, String> {
    updater::check_github_update(&state.http_client).await
}

#[tauri::command]
async fn install_github_update(
    app: tauri::AppHandle,
    state: State<'_, AppManagedState>,
    asset_url: String,
    asset_name: String,
) -> Result<(), String> {
    updater::install_github_update(&app, &state.http_client, &asset_url, &asset_name).await
}
