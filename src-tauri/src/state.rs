use crate::db::{atomic_write, Database};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapConfig {
    pub data_dir: Option<String>,
    pub workspace_dir: Option<String>,
    pub first_run_completed: bool,
    pub default_model_id: Option<String>,
    pub telemetry_enabled: bool,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            data_dir: None,
            workspace_dir: None,
            first_run_completed: false,
            default_model_id: None,
            telemetry_enabled: false,
        }
    }
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub workspace_dir: Arc<RwLock<PathBuf>>,
    pub db: Database,
    pub keyring_service: String,
    pub bootstrap: Arc<RwLock<BootstrapConfig>>,
    credentials_fallback: Arc<RwLock<HashMap<String, String>>>,
}

impl AppState {
    pub fn resolve_install_workspace() -> PathBuf {
        Self::resolve_data_dir().join("workspace")
    }

    pub fn resolve_chats_root_dir() -> PathBuf {
        Self::resolve_data_dir().join("chats")
    }

    pub fn resolve_chat_temp_dir(chat_id: Option<&str>) -> PathBuf {
        let base = Self::resolve_chats_root_dir();
        let sanitized: String = chat_id
            .unwrap_or("")
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(80)
            .collect();
        if sanitized.is_empty() {
            base.join("_shared").join("tmp")
        } else {
            base.join(sanitized).join("tmp")
        }
    }

    pub fn resolve_skills_dir() -> PathBuf {
        Self::resolve_data_dir().join("skills")
    }

    pub fn resolve_agents_dir() -> PathBuf {
        Self::resolve_data_dir().join("agents")
    }

    pub fn resolve_projects_root_dir() -> PathBuf {
        Self::resolve_data_dir().join("projects")
    }

    pub fn resolve_project_data_dir(workspace_root: &str) -> PathBuf {
        let normalized = workspace_root
            .trim()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase();
        let key = if normalized.is_empty() {
            "default".to_string()
        } else {
            let hex = blake3::hash(normalized.as_bytes()).to_hex().to_string();
            hex.chars().take(16).collect()
        };
        Self::resolve_projects_root_dir().join(key)
    }

    pub fn new() -> Result<Self, AppError> {
        let data_dir = Self::resolve_data_dir();
        std::fs::create_dir_all(&data_dir)?;
        let _ = std::fs::create_dir_all(Self::resolve_install_workspace());
        let _ = std::fs::create_dir_all(Self::resolve_chats_root_dir());
        let _ = std::fs::create_dir_all(Self::resolve_projects_root_dir());
        let _ = std::fs::create_dir_all(Self::resolve_agents_dir());
        let _ = crate::skills::deploy_builtin_skills(&Self::resolve_skills_dir());

        let bootstrap_path = data_dir.join("bootstrap.json");
        let bootstrap_config = if bootstrap_path.exists() {
            let data = std::fs::read_to_string(&bootstrap_path)?;
            serde_json::from_str::<BootstrapConfig>(&data).unwrap_or_default()
        } else {
            let default_cfg = BootstrapConfig::default();
            let data = serde_json::to_string_pretty(&default_cfg)?;
            atomic_write(&bootstrap_path, data.as_bytes())?;
            default_cfg
        };

        let initial_workspace = match &bootstrap_config.workspace_dir {
            Some(path_str) if !path_str.trim().is_empty() => PathBuf::from(path_str),
            _ => {
                let default_ws = Self::resolve_install_workspace();
                if bootstrap_config.first_run_completed {
                    let _ = std::fs::create_dir_all(&default_ws);
                }
                default_ws
            }
        };

        let db_path = data_dir.join("omni_harness.db");
        let db = Database::new(&db_path)?;

        let creds_file = data_dir.join("credentials.json");
        let fallback_creds = if creds_file.exists() {
            let raw = std::fs::read_to_string(&creds_file).unwrap_or_default();
            serde_json::from_str::<HashMap<String, String>>(&raw).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Self {
            data_dir,
            workspace_dir: Arc::new(RwLock::new(initial_workspace)),
            db,
            keyring_service: "omni-harness".to_string(),
            bootstrap: Arc::new(RwLock::new(bootstrap_config)),
            credentials_fallback: Arc::new(RwLock::new(fallback_creds)),
        })
    }

    pub fn resolve_data_dir() -> PathBuf {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let marker_in_exe = exe_dir.join("portable.marker");
        let marker_in_cur = Path::new("portable.marker");

        if marker_in_exe.exists() || marker_in_cur.exists() {
            let new_dir = exe_dir.join(".omni-harness");
            let legacy_dir = exe_dir.join("data");
            if !new_dir.exists() && legacy_dir.exists() {
                let _ = std::fs::rename(&legacy_dir, &new_dir);
            }
            new_dir
        } else {
            let local = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
            let new_dir = local.join(".omni-harness");
            let legacy_dir = local.join("OmniHarness");
            if !new_dir.exists() && legacy_dir.exists() {
                let _ = std::fs::rename(&legacy_dir, &new_dir);
            }
            new_dir
        }
    }

    pub async fn get_workspace_dir(&self) -> PathBuf {
        let lock = self.workspace_dir.read().await;
        lock.clone()
    }

    pub async fn set_workspace_dir(&self, new_dir: PathBuf) -> Result<(), AppError> {
        let is_empty = new_dir.as_os_str().is_empty();
        let target = if is_empty {
            Self::resolve_install_workspace()
        } else {
            new_dir.clone()
        };
        let target_for_fs = target.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(&target_for_fs))
            .await
            .map_err(|e| AppError::Execution(e.to_string()))??;

        let serialized = {
            let mut b_lock = self.bootstrap.write().await;
            let mut ws_lock = self.workspace_dir.write().await;
            *ws_lock = target;
            b_lock.workspace_dir = if is_empty {
                None
            } else {
                Some(new_dir.to_string_lossy().to_string())
            };
            serde_json::to_string_pretty(&*b_lock)?
        };

        let bootstrap_path = self.data_dir.join("bootstrap.json");
        tokio::task::spawn_blocking(move || atomic_write(&bootstrap_path, serialized.as_bytes()))
            .await
            .map_err(|e| AppError::Execution(e.to_string()))??;
        Ok(())
    }

    pub async fn complete_first_run(
        &self,
        workspace_dir: Option<String>,
        default_model_id: Option<String>,
    ) -> Result<(), AppError> {
        let parsed_ws = workspace_dir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| (s.to_string(), PathBuf::from(s)));

        if let Some((_, ref p)) = parsed_ws {
            let p_clone = p.clone();
            tokio::task::spawn_blocking(move || std::fs::create_dir_all(&p_clone))
                .await
                .map_err(|e| AppError::Execution(e.to_string()))??;
        }

        let serialized = {
            let mut b_lock = self.bootstrap.write().await;
            let mut ws_lock = self.workspace_dir.write().await;
            b_lock.first_run_completed = true;
            if let Some((ws_str, p)) = parsed_ws {
                b_lock.workspace_dir = Some(ws_str);
                *ws_lock = p;
            }
            if let Some(mid) = default_model_id {
                b_lock.default_model_id = Some(mid);
            }
            serde_json::to_string_pretty(&*b_lock)?
        };

        let bootstrap_path = self.data_dir.join("bootstrap.json");
        tokio::task::spawn_blocking(move || atomic_write(&bootstrap_path, serialized.as_bytes()))
            .await
            .map_err(|e| AppError::Execution(e.to_string()))??;
        Ok(())
    }

    pub async fn get_bootstrap_config(&self) -> BootstrapConfig {
        let lock = self.bootstrap.read().await;
        lock.clone()
    }

    pub async fn set_credential(&self, key_name: &str, secret: &str) -> Result<(), AppError> {
        let service = self.keyring_service.clone();
        let key_owned = key_name.to_string();
        let secret_owned = secret.to_string();
        let keyring_saved = tokio::task::spawn_blocking(move || {
            match keyring::Entry::new(&service, &key_owned) {
                Ok(entry) => entry.set_password(&secret_owned).is_ok(),
                Err(_) => false,
            }
        })
        .await
        .unwrap_or(false);

        if keyring_saved {
            let maybe_raw = {
                let mut lock = self.credentials_fallback.write().await;
                if lock.remove(key_name).is_some() {
                    serde_json::to_string_pretty(&*lock).ok()
                } else {
                    None
                }
            };
            if let Some(raw) = maybe_raw {
                let creds_file = self.data_dir.join("credentials.json");
                let _ = tokio::task::spawn_blocking(move || atomic_write(&creds_file, raw.as_bytes())).await;
            }
            return Ok(());
        }

        let raw = {
            let mut lock = self.credentials_fallback.write().await;
            lock.insert(key_name.to_string(), secret.to_string());
            serde_json::to_string_pretty(&*lock)?
        };
        let creds_file = self.data_dir.join("credentials.json");
        tokio::task::spawn_blocking(move || atomic_write(&creds_file, raw.as_bytes()))
            .await
            .map_err(|e| AppError::Execution(e.to_string()))??;
        Ok(())
    }

    pub async fn get_credential(&self, key_name: &str) -> Result<String, AppError> {
        let service = self.keyring_service.clone();
        let key_owned = key_name.to_string();
        let from_keyring = tokio::task::spawn_blocking(move || {
            keyring::Entry::new(&service, &key_owned)
                .ok()
                .and_then(|entry| entry.get_password().ok())
        })
        .await
        .ok()
        .flatten();

        if let Some(pwd) = from_keyring {
            return Ok(pwd);
        }

        let lock = self.credentials_fallback.read().await;
        if let Some(val) = lock.get(key_name) {
            return Ok(val.clone());
        }

        Err(AppError::NotFound(format!("Credential '{}' not found", key_name)))
    }

    pub async fn delete_credential(&self, key_name: &str) -> Result<(), AppError> {
        let service = self.keyring_service.clone();
        let key_owned = key_name.to_string();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(entry) = keyring::Entry::new(&service, &key_owned) {
                let _ = entry.delete_password();
            }
        })
        .await;

        let maybe_raw = {
            let mut lock = self.credentials_fallback.write().await;
            if lock.remove(key_name).is_some() {
                Some(serde_json::to_string_pretty(&*lock)?)
            } else {
                None
            }
        };
        if let Some(raw) = maybe_raw {
            let creds_file = self.data_dir.join("credentials.json");
            tokio::task::spawn_blocking(move || atomic_write(&creds_file, raw.as_bytes()))
                .await
                .map_err(|e| AppError::Execution(e.to_string()))??;
        }
        Ok(())
    }
}
