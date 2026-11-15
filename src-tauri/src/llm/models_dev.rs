use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;

const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const CACHE_TTL_SECS: u64 = 6 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevEntry {
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub reasoning: Option<bool>,
    pub vision: Option<bool>,
    pub audio: Option<bool>,
    pub files: Option<bool>,
    pub tool_call: Option<bool>,
}

#[derive(Default)]
struct CatalogState {
    entries: HashMap<String, ModelsDevEntry>,
    loaded: bool,
    cache_path: Option<PathBuf>,
}

static CATALOG: OnceLock<RwLock<CatalogState>> = OnceLock::new();
static FETCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn catalog() -> &'static RwLock<CatalogState> {
    CATALOG.get_or_init(|| RwLock::new(CatalogState::default()))
}

fn fetch_lock() -> &'static Mutex<()> {
    FETCH_LOCK.get_or_init(|| Mutex::new(()))
}

fn canonical_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn parse_models_dev_json(raw: &str) -> HashMap<String, ModelsDevEntry> {
    let mut map: HashMap<String, ModelsDevEntry> = HashMap::new();
    let Ok(root) = serde_json::from_str::<Value>(raw) else {
        return map;
    };
    let Some(providers) = root.as_object() else {
        return map;
    };

    for (prov_id, prov_val) in providers {
        let Some(models_obj) = prov_val.get("models").and_then(|v| v.as_object()) else {
            continue;
        };
        let prov_lower = prov_id.to_ascii_lowercase();
        for (model_key, m_val) in models_obj {
            let Some(m_obj) = m_val.as_object() else {
                continue;
            };
            let limit = m_obj.get("limit").and_then(|v| v.as_object());
            let ctx = limit
                .and_then(|l| l.get("context"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let max_out = limit
                .and_then(|l| l.get("output"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            if ctx == 0 && max_out == 0 {
                continue;
            }

            let reasoning = m_obj.get("reasoning").and_then(|v| v.as_bool());
            let tool_call = m_obj.get("tool_call").and_then(|v| v.as_bool());
            let input_mods: Vec<String> = m_obj
                .get("modalities")
                .and_then(|m| m.get("input"))
                .and_then(|i| i.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_ascii_lowercase()))
                        .collect()
                })
                .unwrap_or_default();

            let (vision, audio, files) = if input_mods.is_empty() {
                (None, None, None)
            } else {
                (
                    Some(input_mods.iter().any(|s| s == "image")),
                    Some(input_mods.iter().any(|s| s == "audio")),
                    Some(input_mods.iter().any(|s| s == "pdf" || s == "file")),
                )
            };

            let entry = ModelsDevEntry {
                context_window: ctx,
                max_output_tokens: max_out,
                reasoning,
                vision,
                audio,
                files,
                tool_call,
            };

            let mid = m_obj
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(model_key)
                .to_ascii_lowercase();
            let short = mid.rsplit('/').next().unwrap_or(&mid).to_string();
            let full_key = format!("{}/{}", prov_lower, short);
            let is_free_variant = mid.contains("-free") || mid.contains(":free") || prov_lower.contains("zen");

            let canon_short = canonical_key(&short);
            insert_best(&mut map, full_key, &entry, true);
            insert_best(&mut map, mid.clone(), &entry, !is_free_variant);
            if short != mid {
                insert_best(&mut map, short, &entry, !is_free_variant);
            }
            if !canon_short.is_empty() {
                insert_best(&mut map, format!("__canon:{}", canon_short), &entry, !is_free_variant);
            }
        }
    }
    map
}

fn insert_best(
    map: &mut HashMap<String, ModelsDevEntry>,
    key: String,
    candidate: &ModelsDevEntry,
    prefer_larger: bool,
) {
    if key.is_empty() {
        return;
    }
    match map.get(&key) {
        None => {
            map.insert(key, candidate.clone());
        }
        Some(existing) => {
            if prefer_larger && candidate.context_window > existing.context_window {
                map.insert(key, candidate.clone());
            }
        }
    }
}

fn cache_is_fresh(path: &PathBuf) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|d| d.as_secs() < CACHE_TTL_SECS)
        .unwrap_or(false)
}

fn load_from_disk(path: &PathBuf) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let parsed = parse_models_dev_json(&raw);
    if parsed.is_empty() {
        return false;
    }
    if let Ok(mut guard) = catalog().write() {
        guard.entries = parsed;
        guard.loaded = true;
        return true;
    }
    false
}

async fn fetch_and_cache(cache_path: Option<PathBuf>) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Ok(resp) = client.get(MODELS_DEV_URL).send().await else {
        return false;
    };
    if !resp.status().is_success() {
        return false;
    }
    let Ok(body) = resp.text().await else {
        return false;
    };
    let parsed = parse_models_dev_json(&body);
    if parsed.is_empty() {
        return false;
    }
    if let Some(ref p) = cache_path {
        let _ = crate::db::atomic_write(p, body.as_bytes());
    }
    if let Ok(mut guard) = catalog().write() {
        guard.entries = parsed;
        guard.loaded = true;
    }
    true
}

pub async fn init_and_refresh(data_dir: PathBuf) {
    let cache_path = data_dir.join("models_dev_cache.json");
    if let Ok(mut guard) = catalog().write() {
        guard.cache_path = Some(cache_path.clone());
    }
    let loaded_disk = load_from_disk(&cache_path);
    if loaded_disk && cache_is_fresh(&cache_path) {
        return;
    }
    let _guard = fetch_lock().lock().await;
    let _ = fetch_and_cache(Some(cache_path)).await;
}

pub async fn ensure_loaded() {
    let already = catalog().read().map(|g| g.loaded).unwrap_or(false);
    if already {
        return;
    }
    let _guard = fetch_lock().lock().await;
    let already_after = catalog().read().map(|g| g.loaded).unwrap_or(false);
    if already_after {
        return;
    }
    let cache_path = catalog().read().ok().and_then(|g| g.cache_path.clone());
    if let Some(ref p) = cache_path {
        if load_from_disk(p) && cache_is_fresh(p) {
            return;
        }
    }
    let _ = fetch_and_cache(cache_path).await;
}

pub fn lookup(model_id: &str) -> Option<ModelsDevEntry> {
    let guard = catalog().read().ok()?;
    if guard.entries.is_empty() {
        return None;
    }
    let lower = model_id.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    if let Some(e) = guard.entries.get(&lower) {
        return Some(e.clone());
    }
    let no_tag = lower.split(':').next().unwrap_or(&lower);
    if let Some(e) = guard.entries.get(no_tag) {
        return Some(e.clone());
    }
    let short = no_tag.rsplit('/').next().unwrap_or(no_tag);
    if let Some(e) = guard.entries.get(short) {
        return Some(e.clone());
    }
    let stripped_latest = short.strip_suffix("-latest").unwrap_or(short);
    if let Some(e) = guard.entries.get(stripped_latest) {
        return Some(e.clone());
    }
    let canon = canonical_key(stripped_latest);
    if !canon.is_empty() {
        if let Some(e) = guard.entries.get(&format!("__canon:{}", canon)) {
            return Some(e.clone());
        }
    }
    let canon_full = canonical_key(short);
    if !canon_full.is_empty() {
        if let Some(e) = guard.entries.get(&format!("__canon:{}", canon_full)) {
            return Some(e.clone());
        }
    }
    None
}
