use crate::llm::LlmError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub const NVIDIA_RPM_PER_KEY: f64 = 40.0;
pub const NVIDIA_REQUEST_INTERVAL: Duration = Duration::from_millis(1550);
pub const AUTH_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);
pub const TRANSIENT_FAILURE_COOLDOWN: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    Auth,
    Rate,
    Transient,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPoolStats {
    pub provider: String,
    pub total_keys: usize,
    pub active_keys: usize,
    pub in_cooldown_keys: usize,
}

#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub key: String,
    pub provider: String,
    pub is_active: bool,
    pub fail_count: u32,
    pub cooldown_until: Option<Instant>,
    pub last_used: Instant,
    pub total_requests: u64,
    pub total_errors: u64,
}

impl KeyEntry {
    pub fn new(provider: String, key: String) -> Self {
        Self {
            key,
            provider,
            is_active: true,
            fail_count: 0,
            cooldown_until: None,
            last_used: Instant::now(),
            total_requests: 0,
            total_errors: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct ScopedKeyState {
    next_allowed: Instant,
    cooldown_until: Option<Instant>,
    failure_count: u32,
    issued_count: u64,
    block_reason: Option<BlockReason>,
}

impl ScopedKeyState {
    fn new(now: Instant) -> Self {
        Self {
            next_allowed: now,
            cooldown_until: None,
            failure_count: 0,
            issued_count: 0,
            block_reason: None,
        }
    }
}

#[derive(Debug, Default)]
struct ScopeState {
    keys: HashMap<String, ScopedKeyState>,
    ticket: usize,
}

#[derive(Debug, Clone)]
pub struct KeyPool {
    entries: Arc<RwLock<HashMap<String, Vec<KeyEntry>>>>,
    cursor: Arc<RwLock<HashMap<String, usize>>>,
    scopes: Arc<RwLock<HashMap<String, Arc<Mutex<ScopeState>>>>>,
}

impl Default for KeyPool {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyPool {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            cursor: Arc::new(RwLock::new(HashMap::new())),
            scopes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn parse_api_keys(raw: &str) -> Vec<String> {
        let mut out = Vec::new();
        for part in raw.split(['\n', ',', ';']) {
            let trimmed = part.trim();
            if !trimmed.is_empty() && !out.iter().any(|k: &String| k == trimmed) {
                out.push(trimmed.to_string());
            }
        }
        out
    }

    pub fn scope_for_url(base_url: &str) -> String {
        let trimmed = base_url.trim();
        if trimmed.is_empty() {
            return "unbound".to_string();
        }
        let host = url::Url::parse(trimmed)
            .ok()
            .and_then(|u| {
                let h = u.host_str()?.to_ascii_lowercase();
                Some(match u.port() {
                    Some(p) => format!("{}:{}", h, p),
                    None => h,
                })
            })
            .unwrap_or_else(|| {
                trimmed
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
            });
        if host.is_empty() {
            "unbound".to_string()
        } else if host.contains("nvidia") {
            "nvidia".to_string()
        } else {
            host
        }
    }

    pub fn fingerprint(key: &str) -> String {
        let mut h1: u64 = 0xcbf29ce484222325;
        let mut h2: u64 = 0x9e3779b97f4a7c15;
        for &b in key.as_bytes() {
            h1 ^= b as u64;
            h1 = h1.wrapping_mul(0x100000001b3);
            h2 = h2.wrapping_add((b as u64).wrapping_mul(0xbf58476d1ce4e5b9));
            h2 ^= h2 >> 30;
        }
        format!("{:016x}{:08x}", h1, (h2 as u32))
    }

    pub fn response_retry_delay(
        retry_after_header: Option<&str>,
        attempt: u32,
        rate_limited: bool,
    ) -> Duration {
        if let Some(raw) = retry_after_header {
            if let Ok(secs) = raw.trim().parse::<f64>() {
                let clamped = secs.clamp(0.25, 30.0);
                return Duration::from_secs_f64(clamped);
            }
        }
        if rate_limited {
            let exp = attempt.min(3);
            let secs = (2.0_f64 * (1_u32 << exp) as f64).min(20.0);
            Duration::from_secs_f64(secs)
        } else {
            let exp = attempt.min(4);
            let secs = (0.75_f64 * (1_u32 << exp) as f64).min(8.0);
            Duration::from_secs_f64(secs)
        }
    }

    fn get_or_create_scope(&self, scope: &str) -> Arc<Mutex<ScopeState>> {
        if let Ok(guard) = self.scopes.read() {
            if let Some(s) = guard.get(scope) {
                return s.clone();
            }
        }
        if let Ok(mut guard) = self.scopes.write() {
            return guard
                .entry(scope.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(ScopeState::default())))
                .clone();
        }
        Arc::new(Mutex::new(ScopeState::default()))
    }

    pub fn add_key(&self, provider: &str, key: String) {
        let parsed = Self::parse_api_keys(&key);
        if parsed.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.entries.write() {
            let list = guard.entry(provider.to_string()).or_default();
            for k in parsed {
                if !list.iter().any(|e| e.key == k) {
                    list.push(KeyEntry::new(provider.to_string(), k));
                }
            }
        }
    }

    pub fn set_keys(&self, provider: &str, keys: Vec<String>) {
        let mut deduped = Vec::new();
        for raw in keys {
            for k in Self::parse_api_keys(&raw) {
                if !deduped.iter().any(|existing: &String| existing == &k) {
                    deduped.push(k);
                }
            }
        }
        if let Ok(mut guard) = self.entries.write() {
            let list: Vec<KeyEntry> = deduped
                .into_iter()
                .map(|k| KeyEntry::new(provider.to_string(), k))
                .collect();
            guard.insert(provider.to_string(), list);
        }
    }

    pub fn remove_key(&self, provider: &str, key: &str) {
        if let Ok(mut guard) = self.entries.write() {
            if let Some(list) = guard.get_mut(provider) {
                list.retain(|e| e.key != key);
            }
        }
    }

    pub fn pool_size(&self, provider: &str) -> usize {
        self.entries
            .read()
            .ok()
            .and_then(|g| g.get(provider).map(|l| l.iter().filter(|e| e.is_active).count()))
            .unwrap_or(0)
    }

    fn active_keys_for(&self, provider: &str) -> Vec<String> {
        self.entries
            .read()
            .ok()
            .and_then(|g| {
                g.get(provider).map(|l| {
                    l.iter()
                        .filter(|e| e.is_active)
                        .map(|e| e.key.clone())
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    pub async fn next_key_async(&self, provider: &str, base_url: &str) -> Result<String, LlmError> {
        let keys = self.active_keys_for(provider);
        if keys.is_empty() {
            return Err(LlmError::NoAvailableKeys(provider.to_string()));
        }
        let fps: Vec<String> = keys.iter().map(|k| Self::fingerprint(k)).collect();
        let scope_name = Self::scope_for_url(base_url);
        let is_nvidia = scope_name == "nvidia";
        let scope_lock = self.get_or_create_scope(&scope_name);
        let size = keys.len();

        loop {
            let wait_dur = {
                let mut state = scope_lock.lock().await;
                let now = Instant::now();
                for fp in &fps {
                    state
                        .keys
                        .entry(fp.clone())
                        .or_insert_with(|| ScopedKeyState::new(now));
                }

                let mut available = Vec::new();
                for (idx, fp) in fps.iter().enumerate() {
                    if let Some(entry) = state.keys.get(fp) {
                        let cd_ok = entry.cooldown_until.map(|u| u <= now).unwrap_or(true);
                        let nv_ok = !is_nvidia || entry.next_allowed <= now;
                        if cd_ok && nv_ok {
                            available.push(idx);
                        }
                    }
                }

                if !available.is_empty() {
                    let start = state.ticket % size;
                    let selected_idx = (0..size)
                        .map(|offset| (start + offset) % size)
                        .find(|idx| available.contains(idx))
                        .unwrap_or(available[0]);
                    state.ticket = state.ticket.wrapping_add(1);
                    let fp = &fps[selected_idx];
                    if let Some(entry) = state.keys.get_mut(fp) {
                        entry.issued_count = entry.issued_count.saturating_add(1);
                        if is_nvidia {
                            entry.next_allowed = entry.next_allowed.max(now) + NVIDIA_REQUEST_INTERVAL;
                        }
                    }
                    let chosen_key = keys[selected_idx].clone();
                    if let Ok(mut guard) = self.entries.write() {
                        if let Some(list) = guard.get_mut(provider) {
                            if let Some(e) = list.iter_mut().find(|e| e.key == chosen_key) {
                                e.last_used = now;
                                e.total_requests = e.total_requests.saturating_add(1);
                            }
                        }
                    }
                    return Ok(chosen_key);
                }

                let all_auth_blocked = fps.iter().all(|fp| {
                    state
                        .keys
                        .get(fp)
                        .map(|e| {
                            e.block_reason == Some(BlockReason::Auth)
                                && e.cooldown_until.map(|u| u > now).unwrap_or(false)
                        })
                        .unwrap_or(false)
                });
                if all_auth_blocked {
                    return Err(LlmError::InvalidRequest(
                        "invalid API key: all configured API keys were rejected".to_string(),
                    ));
                }

                let mut earliest: Option<Instant> = None;
                for fp in &fps {
                    if let Some(entry) = state.keys.get(fp) {
                        let cd = entry.cooldown_until.unwrap_or(now);
                        let nv = if is_nvidia { entry.next_allowed } else { now };
                        let ready_at = cd.max(nv);
                        earliest = Some(match earliest {
                            Some(prev) => prev.min(ready_at),
                            None => ready_at,
                        });
                    }
                }
                let ready_at = earliest.unwrap_or(now);
                ready_at
                    .saturating_duration_since(now)
                    .max(Duration::from_millis(20))
            };

            tokio::time::sleep(wait_dur.min(Duration::from_millis(200))).await;
        }
    }

    pub async fn clear_auth_blocks(&self, base_url: &str, keys: &[String]) {
        let scope_name = Self::scope_for_url(base_url);
        let scope_lock = self.get_or_create_scope(&scope_name);
        let mut state = scope_lock.lock().await;
        for raw in keys {
            for k in Self::parse_api_keys(raw) {
                let fp = Self::fingerprint(&k);
                if let Some(entry) = state.keys.get_mut(&fp) {
                    if entry.block_reason == Some(BlockReason::Auth) {
                        entry.block_reason = None;
                        entry.cooldown_until = None;
                        entry.failure_count = 0;
                    }
                }
            }
        }
    }

    pub async fn mark_success(&self, provider: &str, base_url: &str, key: &str) {
        let scope_name = Self::scope_for_url(base_url);
        let fp = Self::fingerprint(key);
        let scope_lock = self.get_or_create_scope(&scope_name);
        {
            let mut state = scope_lock.lock().await;
            if let Some(entry) = state.keys.get_mut(&fp) {
                entry.failure_count = 0;
            }
        }
        self.record_success(provider, key);
    }

    pub async fn mark_rate_limited(
        &self,
        provider: &str,
        base_url: &str,
        key: &str,
        delay: Duration,
    ) {
        let cd = delay.max(Duration::from_millis(250));
        self.apply_scoped_failure(provider, base_url, key, cd, BlockReason::Rate, 429)
            .await;
    }

    pub async fn mark_unavailable(
        &self,
        provider: &str,
        base_url: &str,
        key: &str,
        delay: Option<Duration>,
    ) {
        let cd = delay
            .unwrap_or(AUTH_FAILURE_COOLDOWN)
            .max(Duration::from_secs(1));
        self.apply_scoped_failure(provider, base_url, key, cd, BlockReason::Auth, 401)
            .await;
    }

    pub async fn mark_transient(
        &self,
        provider: &str,
        base_url: &str,
        key: &str,
        delay: Option<Duration>,
    ) {
        let cd = delay
            .unwrap_or(TRANSIENT_FAILURE_COOLDOWN)
            .max(Duration::from_millis(250));
        self.apply_scoped_failure(provider, base_url, key, cd, BlockReason::Transient, 503)
            .await;
    }

    async fn apply_scoped_failure(
        &self,
        provider: &str,
        base_url: &str,
        key: &str,
        cooldown: Duration,
        reason: BlockReason,
        status_code: u16,
    ) {
        let scope_name = Self::scope_for_url(base_url);
        let fp = Self::fingerprint(key);
        let scope_lock = self.get_or_create_scope(&scope_name);
        let now = Instant::now();
        let until = now + cooldown;
        {
            let mut state = scope_lock.lock().await;
            let entry = state
                .keys
                .entry(fp)
                .or_insert_with(|| ScopedKeyState::new(now));
            entry.failure_count = entry.failure_count.saturating_add(1);
            entry.block_reason = Some(reason);
            entry.cooldown_until = Some(match entry.cooldown_until {
                Some(prev) => prev.max(until),
                None => until,
            });
        }
        if let Ok(mut guard) = self.entries.write() {
            if let Some(list) = guard.get_mut(provider) {
                if let Some(entry) = list.iter_mut().find(|e| e.key == key) {
                    entry.total_errors = entry.total_errors.saturating_add(1);
                    entry.fail_count = entry.fail_count.saturating_add(1);
                    entry.cooldown_until = Some(match entry.cooldown_until {
                        Some(prev) => prev.max(until),
                        None => until,
                    });
                }
            }
        }
        let _ = status_code;
    }

    pub fn next_key(&self, provider: &str) -> Result<String, LlmError> {
        let mut guard = self
            .entries
            .write()
            .map_err(|_| LlmError::StreamError("Lock poisoned".to_string()))?;

        let list = match guard.get_mut(provider) {
            Some(l) if !l.is_empty() => l,
            _ => return Err(LlmError::NoAvailableKeys(provider.to_string())),
        };

        let now = Instant::now();
        for entry in list.iter_mut() {
            if let Some(until) = entry.cooldown_until {
                if now >= until {
                    entry.cooldown_until = None;
                }
            }
        }

        let usable_indices: Vec<usize> = list
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_active && e.cooldown_until.is_none())
            .map(|(i, _)| i)
            .collect();

        if usable_indices.is_empty() {
            let mut min_wait = u64::MAX;
            for entry in list.iter() {
                if entry.is_active {
                    if let Some(until) = entry.cooldown_until {
                        if until > now {
                            let diff = until.duration_since(now).as_secs();
                            if diff < min_wait {
                                min_wait = diff;
                            }
                        }
                    }
                }
            }
            let wait_secs = if min_wait == u64::MAX { 1 } else { min_wait.max(1) };
            return Err(LlmError::AllKeysRateLimited {
                provider: provider.to_string(),
                wait_secs,
            });
        }

        let mut cursor_guard = self
            .cursor
            .write()
            .map_err(|_| LlmError::StreamError("Lock poisoned".to_string()))?;
        let cur = cursor_guard.entry(provider.to_string()).or_insert(0);
        let selected_idx = usable_indices[*cur % usable_indices.len()];
        *cur = (*cur + 1) % usable_indices.len();

        let chosen = &mut list[selected_idx];
        chosen.last_used = now;
        chosen.total_requests = chosen.total_requests.saturating_add(1);

        Ok(chosen.key.clone())
    }

    pub fn record_success(&self, provider: &str, key: &str) {
        if let Ok(mut guard) = self.entries.write() {
            if let Some(list) = guard.get_mut(provider) {
                if let Some(entry) = list.iter_mut().find(|e| e.key == key) {
                    entry.fail_count = 0;
                    entry.cooldown_until = None;
                }
            }
        }
    }

    pub fn record_failure(
        &self,
        provider: &str,
        key: &str,
        status_code: u16,
        retry_after_secs: Option<u64>,
    ) {
        if let Ok(mut guard) = self.entries.write() {
            if let Some(list) = guard.get_mut(provider) {
                if let Some(entry) = list.iter_mut().find(|e| e.key == key) {
                    entry.total_errors = entry.total_errors.saturating_add(1);
                    if status_code == 429 || (500..=599).contains(&status_code) {
                        entry.fail_count = entry.fail_count.saturating_add(1);
                        let cooldown_secs = match retry_after_secs {
                            Some(s) => s.max(1),
                            None => {
                                let shift = entry.fail_count.min(8).saturating_sub(1);
                                let base = 2_u64.checked_shl(shift).unwrap_or(256);
                                base.clamp(2, 300)
                            }
                        };
                        entry.cooldown_until =
                            Some(Instant::now() + Duration::from_secs(cooldown_secs));
                    }
                }
            }
        }
    }

    pub fn get_stats(&self, provider: &str) -> Option<KeyPoolStats> {
        let guard = self.entries.read().ok()?;
        let list = guard.get(provider)?;
        let now = Instant::now();
        let total_keys = list.len();
        let active_keys = list.iter().filter(|e| e.is_active).count();
        let in_cooldown_keys = list
            .iter()
            .filter(|e| e.cooldown_until.map(|u| u > now).unwrap_or(false))
            .count();

        Some(KeyPoolStats {
            provider: provider.to_string(),
            total_keys,
            active_keys,
            in_cooldown_keys,
        })
    }
}
