use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OmniTheme {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub css: String,
    #[serde(default)]
    pub icon_overrides: HashMap<String, String>,
    #[serde(default)]
    pub text_overrides: HashMap<String, String>,
    #[serde(default)]
    pub assets: HashMap<String, String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

fn themes_dir() -> PathBuf {
    let dir = crate::state::AppState::resolve_data_dir().join("themes");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn active_theme_file() -> PathBuf {
    themes_dir().join("active_theme.txt")
}

fn sanitize_id(id: &str) -> String {
    let clean: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    if clean.is_empty() {
        format!("theme-{}", uuid::Uuid::new_v4().simple())
    } else {
        clean
    }
}

pub fn list_themes() -> Result<Vec<OmniTheme>, String> {
    let dir = themes_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(theme) = serde_json::from_str::<OmniTheme>(&raw) {
                out.push(theme);
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

pub fn load_theme(id: &str) -> Result<OmniTheme, String> {
    let path = themes_dir().join(format!("{}.json", sanitize_id(id)));
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("Theme not found: {}", e))?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn save_theme(mut theme: OmniTheme) -> Result<OmniTheme, String> {
    let now = chrono::Utc::now().timestamp_millis();
    theme.id = sanitize_id(&theme.id);
    if theme.created_at == 0 {
        theme.created_at = now;
    }
    theme.updated_at = now;
    let path = themes_dir().join(format!("{}.json", theme.id));
    let json = serde_json::to_string_pretty(&theme).map_err(|e| e.to_string())?;
    crate::tools::workspace::atomic_write_bytes(&path, json.as_bytes())?;
    Ok(theme)
}

pub fn delete_theme(id: &str) -> Result<(), String> {
    let safe = sanitize_id(id);
    let path = themes_dir().join(format!("{}.json", safe));
    if path.is_file() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    if get_active_theme_id()?.as_deref() == Some(safe.as_str()) {
        set_active_theme_id(None)?;
    }
    Ok(())
}

pub fn get_active_theme_id() -> Result<Option<String>, String> {
    let p = active_theme_file();
    if !p.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

pub fn set_active_theme_id(id: Option<String>) -> Result<(), String> {
    let p = active_theme_file();
    let val = id.map(|s| sanitize_id(&s)).unwrap_or_default();
    crate::tools::workspace::atomic_write_bytes(&p, val.as_bytes())
}
