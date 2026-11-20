use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::Emitter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const fn xor_deobf<const N: usize>(encoded: [u8; N]) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0usize;
    while i < N {
        out[i] = encoded[i] ^ 0x5A;
        i += 1;
    }
    out
}

const ANTIGRAVITY_CLIENT_ID_BYTES: [u8; 73] = xor_deobf([
    107, 106, 109, 107, 106, 106, 108, 106, 108, 106, 111, 99, 107, 119, 46, 55, 50, 41,
    41, 51, 52, 104, 50, 104, 107, 54, 57, 40, 63, 104, 105, 111, 44, 46, 53, 54, 53, 48,
    50, 110, 61, 110, 106, 105, 63, 42, 116, 59, 42, 42, 41, 116, 61, 53, 53, 61, 54, 63,
    47, 41, 63, 40, 57, 53, 52, 46, 63, 52, 46, 116, 57, 53, 55,
]);

const ANTIGRAVITY_CLIENT_SECRET_BYTES: [u8; 35] = xor_deobf([
    29, 21, 25, 9, 10, 2, 119, 17, 111, 98, 28, 13, 8, 110, 98, 108, 22, 62, 22, 16, 107,
    55, 22, 24, 98, 41, 2, 25, 110, 32, 108, 43, 30, 27, 60,
]);

pub const ANTIGRAVITY_CLIENT_ID: &str =
    unsafe { std::str::from_utf8_unchecked(&ANTIGRAVITY_CLIENT_ID_BYTES) };
pub const ANTIGRAVITY_CLIENT_SECRET: &str =
    unsafe { std::str::from_utf8_unchecked(&ANTIGRAVITY_CLIENT_SECRET_BYTES) };
pub const ANTIGRAVITY_REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";

pub const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const OPENAI_CODEX_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";

pub const CLAUDE_OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const CLAUDE_OAUTH_REDIRECT_URI: &str = "http://localhost:54545/callback";

pub const KIMI_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
pub const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const XAI_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
pub const ZCODE_CLIENT_ID: &str = "client_P8X5CMWmlaRO9gyO-KSqtg";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAccount {
    pub id: String,
    pub provider: String,
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: i64,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub cooldown_until_ms: Option<i64>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_used_ms: i64,
    #[serde(default)]
    pub quota_5h_remaining: Option<f64>,
    #[serde(default)]
    pub quota_5h_reset_at_ms: Option<i64>,
    #[serde(default)]
    pub quota_weekly_remaining: Option<f64>,
    #[serde(default)]
    pub quota_weekly_reset_at_ms: Option<i64>,
    #[serde(default)]
    pub quota_label: Option<String>,
    #[serde(default)]
    pub quota_updated_at_ms: Option<i64>,
    #[serde(default)]
    pub plan_tier: Option<String>,
    #[serde(default)]
    pub allowed_models: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAccountPublic {
    pub id: String,
    pub provider: String,
    pub email: String,
    pub project_id: Option<String>,
    pub account_id: Option<String>,
    pub enabled: bool,
    pub expires_at_ms: i64,
    pub cooldown_until_ms: Option<i64>,
    pub last_error: Option<String>,
    pub quota_5h_remaining: Option<f64>,
    pub quota_5h_reset_at_ms: Option<i64>,
    pub quota_weekly_remaining: Option<f64>,
    pub quota_weekly_reset_at_ms: Option<i64>,
    pub quota_label: Option<String>,
    pub quota_updated_at_ms: Option<i64>,
    pub plan_tier: Option<String>,
    pub allowed_models: Option<Vec<String>>,
}

impl From<&OAuthAccount> for OAuthAccountPublic {
    fn from(a: &OAuthAccount) -> Self {
        Self {
            id: a.id.clone(),
            provider: a.provider.clone(),
            email: a.email.clone(),
            project_id: a.project_id.clone(),
            account_id: a.account_id.clone(),
            enabled: a.enabled,
            expires_at_ms: a.expires_at_ms,
            cooldown_until_ms: a.cooldown_until_ms,
            last_error: a.last_error.clone(),
            quota_5h_remaining: a.quota_5h_remaining,
            quota_5h_reset_at_ms: a.quota_5h_reset_at_ms,
            quota_weekly_remaining: a.quota_weekly_remaining,
            quota_weekly_reset_at_ms: a.quota_weekly_reset_at_ms,
            quota_label: a.quota_label.clone(),
            quota_updated_at_ms: a.quota_updated_at_ms,
            plan_tier: a.plan_tier.clone(),
            allowed_models: a.allowed_models.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProxyConfig {
    pub enabled: bool,
    pub url: String,
}

fn oauth_proxy_file_path() -> PathBuf {
    crate::state::AppState::resolve_data_dir().join("oauth_proxy.json")
}

pub fn load_oauth_proxy() -> OAuthProxyConfig {
    let p = oauth_proxy_file_path();
    if let Ok(raw) = std::fs::read_to_string(&p) {
        if let Ok(cfg) = serde_json::from_str::<OAuthProxyConfig>(&raw) {
            return cfg;
        }
    }
    OAuthProxyConfig::default()
}

pub fn save_oauth_proxy(cfg: OAuthProxyConfig) -> Result<(), String> {
    let p = oauth_proxy_file_path();
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    crate::tools::workspace::atomic_write_bytes(&p, json.as_bytes())
}

pub fn resolve_oauth_proxy(proxy_enabled: Option<bool>, proxy_url: Option<&str>) -> (Option<bool>, Option<String>) {
    if proxy_enabled == Some(true) {
        if let Some(u) = proxy_url.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            return (Some(true), Some(u.to_string()));
        }
    }
    let saved = load_oauth_proxy();
    if saved.enabled && !saved.url.trim().is_empty() {
        return (Some(true), Some(saved.url.trim().to_string()));
    }
    (proxy_enabled, proxy_url.map(|s| s.to_string()))
}

#[derive(Debug, Clone)]
struct PendingOAuthFlow {
    provider: String,
    verifier: String,
    state: String,
    redirect_uri: String,
}

static PENDING_FLOWS: OnceLock<Mutex<HashMap<String, PendingOAuthFlow>>> = OnceLock::new();
static STORE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn pending_flows() -> &'static Mutex<HashMap<String, PendingOAuthFlow>> {
    PENDING_FLOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn store_lock() -> &'static tokio::sync::Mutex<()> {
    STORE_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn accounts_file_path() -> PathBuf {
    crate::state::AppState::resolve_data_dir().join("oauth_accounts.json")
}

fn read_accounts_unlocked() -> Vec<OAuthAccount> {
    let p = accounts_file_path();
    if !p.is_file() {
        return Vec::new();
    }
    let Ok(raw) = std::fs::read_to_string(&p) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<OAuthAccount>>(&raw).unwrap_or_default()
}

fn write_accounts_unlocked(accounts: &[OAuthAccount]) -> Result<(), String> {
    let p = accounts_file_path();
    let json = serde_json::to_string_pretty(accounts).map_err(|e| e.to_string())?;
    crate::tools::workspace::atomic_write_bytes(&p, json.as_bytes())
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..(i + 1) * 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

fn generate_pkce_pair() -> (String, String, String) {
    let raw = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha256_bytes(verifier.as_bytes()));
    let state = uuid::Uuid::new_v4().simple().to_string();
    (verifier, challenge, state)
}

fn open_url_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

fn decode_jwt_payload(jwt: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(parts[1]))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn format_chatgpt_plan_label(raw: &str) -> Option<String> {
    let clean = raw.trim();
    if clean.is_empty() {
        return None;
    }
    let low = clean.to_ascii_lowercase();
    if low.starts_with("gpt ") {
        return Some(clean.to_string());
    }
    let mapped = match low.as_str() {
        "pro" | "chatgpt_pro" | "chatgptpro" => "GPT Pro",
        "prolite" | "pro_lite" | "chatgpt_prolite" => "GPT Pro Lite",
        "plus" | "chatgpt_plus" | "chatgptplus" => "GPT Plus",
        "go" | "chatgpt_go" => "GPT Go",
        "team" | "chatgpt_team" => "GPT Team",
        "business" | "chatgpt_business" => "GPT Business",
        "enterprise" | "chatgpt_enterprise" => "GPT Enterprise",
        "edu" | "chatgpt_edu" => "GPT Edu",
        "free" | "chatgpt_free" => "GPT Free",
        _ => return Some(format!("GPT {}", clean)),
    };
    Some(mapped.to_string())
}

pub fn format_antigravity_tier_label(tier_id: &str, tier_name: Option<&str>) -> Option<String> {
    let low = tier_id.trim().to_ascii_lowercase();
    if low.contains("ultra") {
        return Some("Gemini Ultra".to_string());
    }
    if low.contains("pro") {
        return Some("Gemini Pro".to_string());
    }
    if low.contains("enterprise") {
        return Some("Gemini Enterprise".to_string());
    }
    if low == "standard-tier" || low.contains("standard") {
        return Some("Standard".to_string());
    }
    if low == "free-tier" || low == "legacy-tier" || low.contains("free") {
        return Some("Free".to_string());
    }
    if let Some(name) = tier_name.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let nlow = name.to_ascii_lowercase();
        if nlow.contains("ultra") {
            return Some("Gemini Ultra".to_string());
        }
        if nlow.contains("pro") {
            return Some("Gemini Pro".to_string());
        }
        if nlow.contains("free") {
            return Some("Free".to_string());
        }
        return Some(name.to_string());
    }
    None
}

fn extract_chatgpt_claims_full(
    id_token: Option<&str>,
    access_token: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let mut email_out: Option<String> = None;
    let mut acc_out: Option<String> = None;
    let mut plan_out: Option<String> = None;

    for tok in [id_token, Some(access_token)].into_iter().flatten() {
        if let Some(val) = decode_jwt_payload(tok) {
            if email_out.is_none() {
                email_out = val.get("email").and_then(|v| v.as_str()).map(|s| s.to_string());
            }
            let auth_obj = val.get("https://api.openai.com/auth");
            if acc_out.is_none() {
                acc_out = val
                    .get("chatgpt_account_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| auth_obj.and_then(|o| o.get("chatgpt_account_id")).and_then(|v| v.as_str()))
                    .or_else(|| {
                        val.get("organizations")
                            .and_then(|o| o.as_array())
                            .and_then(|a| a.first())
                            .and_then(|o| o.get("id"))
                            .and_then(|v| v.as_str())
                    })
                    .map(|s| s.to_string());
            }
            if plan_out.is_none() {
                let raw_plan = val
                    .get("chatgpt_plan_type")
                    .and_then(|v| v.as_str())
                    .or_else(|| auth_obj.and_then(|o| o.get("chatgpt_plan_type")).and_then(|v| v.as_str()))
                    .or_else(|| val.get("plan_type").and_then(|v| v.as_str()));
                if let Some(rp) = raw_plan {
                    plan_out = format_chatgpt_plan_label(rp);
                }
            }
        }
    }
    (email_out, acc_out, plan_out)
}

fn extract_chatgpt_claims(id_token: Option<&str>, access_token: &str) -> (Option<String>, Option<String>) {
    let (e, a, _) = extract_chatgpt_claims_full(id_token, access_token);
    (e, a)
}

async fn upsert_account(acc: OAuthAccount) -> Result<OAuthAccount, String> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    if let Some(existing) = list.iter_mut().find(|x| {
        x.provider == acc.provider && (x.id == acc.id || (!acc.email.is_empty() && x.email.eq_ignore_ascii_case(&acc.email)))
    }) {
        existing.access_token = acc.access_token.clone();
        if !acc.refresh_token.is_empty() {
            existing.refresh_token = acc.refresh_token.clone();
        }
        existing.expires_at_ms = acc.expires_at_ms;
        if acc.project_id.is_some() {
            existing.project_id = acc.project_id.clone();
        }
        if acc.account_id.is_some() {
            existing.account_id = acc.account_id.clone();
        }
        if acc.quota_label.is_some() {
            existing.quota_label = acc.quota_label.clone();
            if acc.plan_tier.is_some() { existing.plan_tier = acc.plan_tier.clone(); }
            if acc.allowed_models.is_some() { existing.allowed_models = acc.allowed_models.clone(); }
        }
        existing.enabled = true;
        existing.cooldown_until_ms = None;
        existing.last_error = None;
        let out = existing.clone();
        write_accounts_unlocked(&list)?;
        return Ok(out);
    }
    list.push(acc.clone());
    write_accounts_unlocked(&list)?;
    Ok(acc)
}

pub async fn list_accounts(provider: Option<&str>) -> Vec<OAuthAccountPublic> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    let mut dirty = false;
    for acc in list.iter_mut() {
        if acc.provider == "openai_codex" {
            if let Some(ref cur) = acc.quota_label.clone() {
                if let Some(norm) = format_chatgpt_plan_label(cur) {
                    if &norm != cur {
                        acc.quota_label = Some(norm);
                        dirty = true;
                    }
                }
            } else {
                let (_, _, plan_opt) = extract_chatgpt_claims_full(None, &acc.access_token);
                if plan_opt.is_some() {
                    acc.quota_label = plan_opt;
                    dirty = true;
                }
            }
        }
    }
    if dirty {
        let _ = write_accounts_unlocked(&list);
    }
    list.iter()
        .filter(|a| provider.map(|p| a.provider == p).unwrap_or(true))
        .map(OAuthAccountPublic::from)
        .collect()
}

pub async fn toggle_account(id: &str, enabled: bool) -> Result<(), String> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    let item = list.iter_mut().find(|a| a.id == id).ok_or("Account not found")?;
    item.enabled = enabled;
    if enabled {
        item.cooldown_until_ms = None;
        item.last_error = None;
    }
    write_accounts_unlocked(&list)
}

pub async fn delete_account(id: &str) -> Result<(), String> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    let before = list.len();
    list.retain(|a| a.id != id);
    if list.len() == before {
        return Err("Account not found".to_string());
    }
    write_accounts_unlocked(&list)
}

pub async fn mark_account_cooldown(id: &str, cooldown_secs: u64, reason: &str) {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    if let Some(a) = list.iter_mut().find(|x| x.id == id) {
        let now = chrono::Utc::now().timestamp_millis();
        a.cooldown_until_ms = Some(now + (cooldown_secs as i64) * 1000);
        a.last_error = Some(reason.chars().take(240).collect());
        let _ = write_accounts_unlocked(&list);
    }
}

pub async fn mark_account_quota_exhausted(id: &str, q5h: Option<f64>, q7d: Option<f64>) {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    if let Some(a) = list.iter_mut().find(|x| x.id == id) {
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(v) = q5h {
            a.quota_5h_remaining = Some(v.clamp(0.0, 100.0));
            if a.quota_5h_reset_at_ms.map(|r| r <= now).unwrap_or(true) {
                a.quota_5h_reset_at_ms = Some(now + 5 * 3600 * 1000);
            }
        }
        if let Some(v) = q7d {
            a.quota_weekly_remaining = Some(v.clamp(0.0, 100.0));
            if a.quota_weekly_reset_at_ms.map(|r| r <= now).unwrap_or(true) {
                a.quota_weekly_reset_at_ms = Some(now + 7 * 24 * 3600 * 1000);
            }
        }
        a.quota_updated_at_ms = Some(now);
        let _ = write_accounts_unlocked(&list);
    }
}

pub async fn update_account_quota_partial(
    id: &str,
    q5h: Option<f64>,
    r5h_ms: Option<i64>,
    q7d: Option<f64>,
    r7d_ms: Option<i64>,
) {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    if let Some(a) = list.iter_mut().find(|x| x.id == id) {
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(v) = q5h {
            a.quota_5h_remaining = Some(v.clamp(0.0, 100.0));
        }
        if let Some(r) = r5h_ms {
            a.quota_5h_reset_at_ms = Some(r);
        }
        if let Some(v) = q7d {
            a.quota_weekly_remaining = Some(v.clamp(0.0, 100.0));
        }
        if let Some(r) = r7d_ms {
            a.quota_weekly_reset_at_ms = Some(r);
        }
        a.quota_updated_at_ms = Some(now);
        let _ = write_accounts_unlocked(&list);
    }
}

async fn refresh_account_tokens(client: &reqwest::Client, acc: &mut OAuthAccount) -> Result<(), String> {
    if acc.refresh_token.trim().is_empty() {
        return Err("Missing refresh_token".to_string());
    }
    let now = chrono::Utc::now().timestamp_millis();
    match acc.provider.as_str() {
        "antigravity" => {
            let params = [
                ("client_id", ANTIGRAVITY_CLIENT_ID),
                ("client_secret", ANTIGRAVITY_CLIENT_SECRET),
                ("grant_type", "refresh_token"),
                ("refresh_token", acc.refresh_token.as_str()),
            ];
            let resp = client
                .post("https://oauth2.googleapis.com/token")
                .header("User-Agent", antigravity_hub_ua())
                .form(&params)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let st_code = resp.status().as_u16();
            if !resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                crate::logger::record_http("OAuthRefresh", "POST", "https://oauth2.googleapis.com/token", st_code, &txt);
                return Err(format!("Google refresh failed ({}): {}", st_code, txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let at = data.get("access_token").and_then(|v| v.as_str()).ok_or("No access_token")?;
            let exp_in = data.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
            acc.access_token = at.to_string();
            if let Some(rt) = data.get("refresh_token").and_then(|v| v.as_str()) {
                if !rt.is_empty() {
                    acc.refresh_token = rt.to_string();
                }
            }
            if acc.quota_label.is_none() || acc.project_id.as_deref().map(|p| p.is_empty()).unwrap_or(true) {
                let (pid, tier) = fetch_antigravity_account_meta(client, &acc.access_token).await;
                if !pid.is_empty() && acc.project_id.as_deref().map(|p| p.is_empty()).unwrap_or(true) {
                    acc.project_id = Some(pid);
                }
                if tier.is_some() {
                    acc.quota_label = tier;
                }
            }
            let jitter_ms = (now.abs() % 15_000) + 10_000;
            acc.expires_at_ms = now + (exp_in * 1000).saturating_sub(jitter_ms);
            acc.last_error = None;
            crate::logger::record_http("OAuthRefresh", "POST", "https://oauth2.googleapis.com/token", st_code, &format!("refreshed {} exp={}s", acc.email, exp_in));
            Ok(())
        }
        "openai_codex" => {
            let params = [
                ("client_id", OPENAI_CODEX_CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", acc.refresh_token.as_str()),
            ];
            let resp = client
                .post("https://auth.openai.com/oauth/token")
                .form(&params)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let status = resp.status();
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("OpenAI refresh failed ({}): {}", status, txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let at = data.get("access_token").and_then(|v| v.as_str()).ok_or("No access_token")?;
            let id_tok = data.get("id_token").and_then(|v| v.as_str());
            let exp_in = data.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
            let (_, aid_opt, plan_opt) = extract_chatgpt_claims_full(id_tok, at);
            acc.access_token = at.to_string();
            if acc.account_id.is_none() && aid_opt.is_some() {
                acc.account_id = aid_opt;
            }
            if plan_opt.is_some() {
                acc.quota_label = plan_opt;
            }
            if let Some(rt) = data.get("refresh_token").and_then(|v| v.as_str()) {
                if !rt.is_empty() {
                    acc.refresh_token = rt.to_string();
                }
            }
            acc.expires_at_ms = now + exp_in * 1000 - 60_000;
            acc.last_error = None;
            Ok(())
        }
        "claude_oauth" => {
            let body = serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": acc.refresh_token,
                "client_id": CLAUDE_OAUTH_CLIENT_ID
            });
            let resp = client
                .post("https://platform.claude.com/v1/oauth/token")
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let status = resp.status();
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Claude refresh failed ({}): {}", status, txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let at = data.get("access_token").and_then(|v| v.as_str()).ok_or("No access_token")?;
            let exp_in = data.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
            acc.access_token = at.to_string();
            if let Some(rt) = data.get("refresh_token").and_then(|v| v.as_str()) {
                if !rt.is_empty() {
                    acc.refresh_token = rt.to_string();
                }
            }
            acc.expires_at_ms = now + exp_in * 1000 - 60_000;
            acc.last_error = None;
            Ok(())
        }
        "kimi_oauth" => {
            let params = [
                ("client_id", KIMI_CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", acc.refresh_token.as_str()),
            ];
            let host = if acc.account_id.as_deref() == Some("kimi.ai") {
                "https://auth.kimi.ai"
            } else {
                "https://auth.kimi.com"
            };
            let resp = client
                .post(format!("{}/api/oauth/token", host))
                .header("X-Msh-Platform", "kimi_cli")
                .header("X-Msh-Version", "1.0.0")
                .header("X-Msh-Device-Id", machine_session_id())
                .form(&params)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let status = resp.status();
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Kimi refresh failed ({}): {}", status, txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let at = data.get("access_token").and_then(|v| v.as_str()).ok_or("No access_token")?;
            let exp_in = data.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
            acc.access_token = at.to_string();
            if let Some(rt) = data.get("refresh_token").and_then(|v| v.as_str()) {
                if !rt.is_empty() {
                    acc.refresh_token = rt.to_string();
                }
            }
            acc.expires_at_ms = now + exp_in * 1000 - 60_000;
            acc.last_error = None;
            Ok(())
        }
        "xai_oauth" => {
            let token_ep = client
                .get("https://auth.x.ai/.well-known/openid-configuration")
                .send()
                .await
                .ok()
                .and_then(|_| None::<String>)
                .unwrap_or_else(|| "https://auth.x.ai/oauth/token".to_string());
            let mut resolved_ep = token_ep;
            if let Ok(disc_resp) = client.get("https://auth.x.ai/.well-known/openid-configuration").send().await {
                if let Ok(disc) = disc_resp.json::<serde_json::Value>().await {
                    if let Some(ep) = disc.get("token_endpoint").and_then(|v| v.as_str()) {
                        resolved_ep = ep.to_string();
                    }
                }
            }
            let params = [
                ("client_id", XAI_CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", acc.refresh_token.as_str()),
            ];
            let resp = client
                .post(&resolved_ep)
                .form(&params)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let status = resp.status();
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("xAI refresh failed ({}): {}", status, txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let at = data.get("access_token").and_then(|v| v.as_str()).ok_or("No access_token")?;
            let exp_in = data.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
            acc.access_token = at.to_string();
            if let Some(rt) = data.get("refresh_token").and_then(|v| v.as_str()) {
                if !rt.is_empty() {
                    acc.refresh_token = rt.to_string();
                }
            }
            acc.expires_at_ms = now + exp_in * 1000 - 60_000;
            acc.last_error = None;
            Ok(())
        }
        "zcode_oauth" | "perplexity_oauth" => Ok(()),
        other => Err(format!("Unsupported OAuth provider: {}", other)),
    }
}

fn is_quota_exhausted_for_now(a: &OAuthAccount, now: i64) -> bool {
    let q5_dead = matches!(a.quota_5h_remaining, Some(v) if v <= 0.01)
        && a.quota_5h_reset_at_ms.map(|r| r > now).unwrap_or(true);
    let q7_dead = matches!(a.quota_weekly_remaining, Some(v) if v <= 0.01)
        && a.quota_weekly_reset_at_ms.map(|r| r > now).unwrap_or(true);
    q5_dead || q7_dead
}

fn account_supports_model(acc: &OAuthAccount, requested_model: Option<&str>) -> bool {
    let Some(raw_m) = requested_model.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        return true;
    };
    let clean = raw_m.strip_prefix("models/").unwrap_or(raw_m).to_ascii_lowercase();
    if let Some(ref allowed) = acc.allowed_models {
        if !allowed.is_empty() {
            let found = allowed.iter().any(|m| {
                let mc = m.trim().strip_prefix("models/").unwrap_or(m.trim()).to_ascii_lowercase();
                mc == clean || clean.ends_with(&format!("/{}", mc)) || mc.ends_with(&format!("/{}", clean))
            });
            if !found {
                return false;
            }
        }
    }
    if let Some(tier) = acc.plan_tier.as_deref().or(acc.quota_label.as_deref()) {
        let t = tier.to_ascii_lowercase();
        if (t == "free" || t.contains("free")) && (clean.contains("pro") || clean.contains("max") || clean.contains("ultra") || clean.contains("opus")) {
            return false;
        }
    }
    true
}

pub async fn update_account_subscription(
    id: &str,
    plan_tier: Option<String>,
    allowed_models: Option<Vec<String>>,
) {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    if let Some(acc) = list.iter_mut().find(|a| a.id == id) {
        if let Some(pt) = plan_tier {
            if !pt.trim().is_empty() {
                acc.plan_tier = Some(pt.trim().to_string());
            }
        }
        if let Some(am) = allowed_models {
            if !am.is_empty() {
                acc.allowed_models = Some(am);
            }
        }
        let _ = write_accounts_unlocked(&list);
    }
}

pub async fn acquire_valid_account(
    client: &reqwest::Client,
    provider: &str,
    excluded_ids: &std::collections::HashSet<String>,
) -> Result<OAuthAccount, String> {
    acquire_valid_account_for_model(client, provider, excluded_ids, None).await
}

pub async fn acquire_valid_account_for_model(
    client: &reqwest::Client,
    provider: &str,
    excluded_ids: &std::collections::HashSet<String>,
    requested_model: Option<&str>,
) -> Result<OAuthAccount, String> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    let now = chrono::Utc::now().timestamp_millis();

    for a in list.iter_mut() {
        if a.provider == provider {
            if let Some(r5) = a.quota_5h_reset_at_ms {
                if r5 <= now && matches!(a.quota_5h_remaining, Some(v) if v <= 0.01) {
                    a.quota_5h_remaining = None;
                    a.quota_5h_reset_at_ms = None;
                }
            }
            if let Some(r7) = a.quota_weekly_reset_at_ms {
                if r7 <= now && matches!(a.quota_weekly_remaining, Some(v) if v <= 0.01) {
                    a.quota_weekly_remaining = None;
                    a.quota_weekly_reset_at_ms = None;
                }
            }
        }
    }

    let total_for_provider = list.iter().filter(|a| a.provider == provider).count();
    if total_for_provider == 0 {
        return Err(format!(
            "No OAuth accounts configured for '{}'. Connect an account in Settings -> Models & Providers.",
            provider
        ));
    }

    let enabled_for_provider = list.iter().filter(|a| a.provider == provider && a.enabled).count();
    if enabled_for_provider == 0 {
        return Err(format!(
            "All {} OAuth account(s) for '{}' are currently disabled.",
            total_for_provider, provider
        ));
    }

    let mut candidates: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            a.provider == provider
                && a.enabled
                && !excluded_ids.contains(&a.id)
                && a.cooldown_until_ms.map(|c| c <= now).unwrap_or(true)
                && !is_quota_exhausted_for_now(a, now)
                && account_supports_model(a, requested_model)
        })
        .map(|(i, _)| i)
        .collect();

    if candidates.is_empty() {
        candidates = list
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                a.provider == provider
                    && a.enabled
                    && !excluded_ids.contains(&a.id)
                    && !is_quota_exhausted_for_now(a, now)
                    && account_supports_model(a, requested_model)
            })
            .map(|(i, _)| i)
            .collect();
    }

    if candidates.is_empty() {
        let last_msg = list
            .iter()
            .filter(|a| a.provider == provider && a.enabled)
            .find_map(|a| a.last_error.clone());
        return Err(last_msg.unwrap_or_else(|| {
            format!(
                "All active OAuth accounts for '{}' are rate-limited, quota-exhausted, or failed during this request.",
                provider
            )
        }));
    }

    candidates.sort_by_key(|&idx| list[idx].last_used_ms);
    let chosen_idx = candidates[0];
    let mut chosen = list[chosen_idx].clone();

    if chosen.access_token.is_empty() || (chosen.expires_at_ms > 0 && chosen.expires_at_ms <= now + 10_000) {
        if let Err(e) = refresh_account_tokens(client, &mut chosen).await {
            list[chosen_idx].last_error = Some(e.clone());
            list[chosen_idx].cooldown_until_ms = Some(now + 120_000);
            let _ = write_accounts_unlocked(&list);
            return Err(e);
        }
    }

    chosen.last_used_ms = now;
    chosen.cooldown_until_ms = None;
    list[chosen_idx] = chosen.clone();
    write_accounts_unlocked(&list)?;
    Ok(chosen)
}

pub async fn force_refresh_account(client: &reqwest::Client, id: &str) -> Result<OAuthAccount, String> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    let idx = list.iter().position(|a| a.id == id).ok_or("Account not found")?;
    let mut acc = list[idx].clone();
    refresh_account_tokens(client, &mut acc).await?;
    list[idx] = acc.clone();
    write_accounts_unlocked(&list)?;
    Ok(acc)
}

async fn exchange_code_for_account(
    client: &reqwest::Client,
    flow: &PendingOAuthFlow,
    code: &str,
    returned_state: Option<&str>,
) -> Result<OAuthAccount, String> {
    let now = chrono::Utc::now().timestamp_millis();
    match flow.provider.as_str() {
        "antigravity" => {
            let params = [
                ("client_id", ANTIGRAVITY_CLIENT_ID),
                ("client_secret", ANTIGRAVITY_CLIENT_SECRET),
                ("code", code),
                ("grant_type", "authorization_code"),
                ("redirect_uri", flow.redirect_uri.as_str()),
                ("code_verifier", flow.verifier.as_str()),
            ];
            let resp = client
                .post("https://oauth2.googleapis.com/token")
                .header("User-Agent", antigravity_hub_ua())
                .form(&params)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Google token exchange failed: {}", txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let at = data.get("access_token").and_then(|v| v.as_str()).ok_or("Missing access_token")?.to_string();
            let rt = data.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let exp_in = data.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);

            let email = match client
                .get("https://www.googleapis.com/oauth2/v3/userinfo")
                .bearer_auth(&at)
                .header("User-Agent", antigravity_hub_ua())
                .header("X-Goog-Api-Client", antigravity_api_client())
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => r
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|v| v.get("email").and_then(|e| e.as_str()).map(|s| s.to_string()))
                    .unwrap_or_else(|| "antigravity-user".to_string()),
                _ => "antigravity-user".to_string(),
            };

            let (project_id, tier_label) = fetch_antigravity_account_meta(client, &at).await;
            let acc = OAuthAccount {
                id: format!("ag-{}", uuid::Uuid::new_v4().simple()),
                provider: "antigravity".to_string(),
                email,
                access_token: at,
                refresh_token: rt,
                expires_at_ms: now + exp_in * 1000 - 10_000,
                project_id: Some(project_id),
                account_id: None,
                enabled: true,
                cooldown_until_ms: None,
                last_error: None,
                last_used_ms: 0,
                quota_5h_remaining: None,
                quota_5h_reset_at_ms: None,
                quota_weekly_remaining: None,
                quota_weekly_reset_at_ms: None,
                quota_label: tier_label,
                quota_updated_at_ms: None,
                plan_tier: None,
                allowed_models: None,
            };
            upsert_account(acc).await
        }
        "openai_codex" => {
            let params = [
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", flow.redirect_uri.as_str()),
                ("client_id", OPENAI_CODEX_CLIENT_ID),
                ("code_verifier", flow.verifier.as_str()),
            ];
            let resp = client
                .post("https://auth.openai.com/oauth/token")
                .form(&params)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("OpenAI token exchange failed: {}", txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let at = data.get("access_token").and_then(|v| v.as_str()).ok_or("Missing access_token")?.to_string();
            let rt = data.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let id_tok = data.get("id_token").and_then(|v| v.as_str());
            let exp_in = data.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
            let (email_opt, account_id_opt, plan_opt) = extract_chatgpt_claims_full(id_tok, &at);

            let acc = OAuthAccount {
                id: format!("codex-{}", uuid::Uuid::new_v4().simple()),
                provider: "openai_codex".to_string(),
                email: email_opt.unwrap_or_else(|| "chatgpt-codex".to_string()),
                access_token: at,
                refresh_token: rt,
                expires_at_ms: now + exp_in * 1000 - 60_000,
                project_id: None,
                account_id: account_id_opt,
                enabled: true,
                cooldown_until_ms: None,
                last_error: None,
                last_used_ms: 0,
                quota_5h_remaining: None,
                quota_5h_reset_at_ms: None,
                quota_weekly_remaining: None,
                quota_weekly_reset_at_ms: None,
                quota_label: plan_opt,
                quota_updated_at_ms: None,
                plan_tier: None,
                allowed_models: None,
            };
            upsert_account(acc).await
        }
        "claude_oauth" => {
            let mut body = serde_json::json!({
                "grant_type": "authorization_code",
                "code": code,
                "code_verifier": flow.verifier,
                "client_id": CLAUDE_OAUTH_CLIENT_ID,
                "redirect_uri": flow.redirect_uri,
            });
            if let Some(st) = returned_state.filter(|s| !s.is_empty()) {
                body["state"] = serde_json::json!(st);
            } else {
                body["state"] = serde_json::json!(flow.state);
            }
            let resp = client
                .post("https://platform.claude.com/v1/oauth/token")
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Claude token exchange failed: {}", txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let at = data.get("access_token").and_then(|v| v.as_str()).ok_or("Missing access_token")?.to_string();
            let rt = data.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let exp_in = data.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
            let email = data
                .get("account")
                .and_then(|a| a.get("email_address"))
                .and_then(|v| v.as_str())
                .unwrap_or("claude-subscription")
                .to_string();

            let acc = OAuthAccount {
                id: format!("claude-{}", uuid::Uuid::new_v4().simple()),
                provider: "claude_oauth".to_string(),
                email,
                access_token: at,
                refresh_token: rt,
                expires_at_ms: now + exp_in * 1000 - 60_000,
                project_id: None,
                account_id: None,
                enabled: true,
                cooldown_until_ms: None,
                last_error: None,
                last_used_ms: 0,
                quota_5h_remaining: None,
                quota_5h_reset_at_ms: None,
                quota_weekly_remaining: None,
                quota_weekly_reset_at_ms: None,
                quota_label: None,
                quota_updated_at_ms: None,
                plan_tier: None,
                allowed_models: None,
            };
            upsert_account(acc).await
        }
        "zcode_oauth" => {
            let resp = client
                .post("https://zcode.z.ai/api/v1/oauth/token")
                .json(&serde_json::json!({
                    "provider": "zai",
                    "code": code,
                    "redirect_uri": flow.redirect_uri,
                    "state": returned_state.unwrap_or(&flow.state),
                }))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("ZCode token exchange failed: {}", txt));
            }
            let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let zai_at = data
                .pointer("/data/zai/access_token")
                .or_else(|| data.pointer("/data/token"))
                .or_else(|| data.get("access_token"))
                .and_then(|v| v.as_str())
                .ok_or("Missing Z.ai access_token in ZCode response")?
                .to_string();
            let mut zcode_jwt = zai_at.clone();
            if let Ok(biz_resp) = client
                .post("https://api.z.ai/api/auth/z/login")
                .json(&serde_json::json!({ "token": zai_at }))
                .send()
                .await
            {
                if biz_resp.status().is_success() {
                    if let Ok(biz_val) = biz_resp.json::<serde_json::Value>().await {
                        if let Some(jwt) = biz_val
                            .pointer("/data/access_token")
                            .or_else(|| biz_val.pointer("/data/token"))
                            .and_then(|v| v.as_str())
                        {
                            if !jwt.is_empty() {
                                zcode_jwt = jwt.to_string();
                            }
                        }
                    }
                }
            }
            let email = data
                .pointer("/data/user/username")
                .or_else(|| data.pointer("/data/user/email"))
                .and_then(|v| v.as_str())
                .unwrap_or("zcode-user")
                .to_string();
            let exp_in = data.pointer("/data/expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
            let acc = OAuthAccount {
                id: format!("zcode-{}", uuid::Uuid::new_v4().simple()),
                provider: "zcode_oauth".to_string(),
                email,
                access_token: zcode_jwt,
                refresh_token: String::new(),
                expires_at_ms: now + exp_in * 1000 - 60_000,
                project_id: None,
                account_id: None,
                enabled: true,
                cooldown_until_ms: None,
                last_error: None,
                last_used_ms: 0,
                quota_5h_remaining: None,
                quota_5h_reset_at_ms: None,
                quota_weekly_remaining: None,
                quota_weekly_reset_at_ms: None,
                quota_label: None,
                quota_updated_at_ms: None,
                plan_tier: None,
                allowed_models: None,
            };
            upsert_account(acc).await
        }
        other => Err(format!("Unsupported OAuth provider: {}", other)),
    }
}

pub fn antigravity_hub_ua() -> &'static str {
    "antigravity/hub/2.32.1 (aidev_client; os_type=windows; arch=amd64; cl=1021296516)"
}

pub fn persistent_device_id() -> &'static str {
    static DID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DID.get_or_init(|| {
        let p = crate::state::AppState::resolve_data_dir().join("oauth_device.json");
        if let Ok(raw) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(id) = v.get("deviceId").and_then(|x| x.as_str()) {
                    if !id.trim().is_empty() {
                        return id.trim().to_string();
                    }
                }
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&p, serde_json::json!({ "deviceId": id }).to_string());
        id
    })
}

pub fn machine_session_id() -> &'static str {
    static SID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SID.get_or_init(|| format!("{}-{}", persistent_device_id(), chrono::Utc::now().timestamp_millis()))
}

pub fn antigravity_api_client() -> &'static str {
    "gl-go/1.22.7 gdcl/20240101"
}

pub fn antigravity_trace_id() -> String {
    let bytes = uuid::Uuid::new_v4().as_bytes()[0..8].try_into().unwrap_or([0u8; 8]);
    let val = u64::from_le_bytes(bytes);
    format!("0x{:016x}", val)
}

pub fn antigravity_client_metadata() -> &'static str {
    r#"{"ideType":"ANTIGRAVITY","platform":"WINDOWS","pluginType":"GEMINI"}"#
}

pub async fn fetch_antigravity_account_meta(
    client: &reqwest::Client,
    access_token: &str,
) -> (String, Option<String>) {
    let meta = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "WINDOWS",
            "pluginType": "GEMINI"
        },
        "mode": "FULL_ELIGIBILITY_CHECK"
    });
    let mut found_pid = String::new();
    let mut found_tier: Option<String> = None;

    let parse_tier_val = |t_val: &serde_json::Value| -> Option<String> {
        if let Some(s) = t_val.as_str() {
            return format_antigravity_tier_label(s, None);
        }
        let tid = t_val.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let tname = t_val.get("name").and_then(|v| v.as_str());
        if tid.is_empty() && tname.is_none() {
            return None;
        }
        format_antigravity_tier_label(tid, tname)
    };

    for ep in [
        "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
        "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    ] {
        if let Ok(resp) = client
            .post(ep)
            .bearer_auth(access_token)
            .header("User-Agent", antigravity_hub_ua())
            .header("X-Goog-Api-Client", antigravity_api_client())
            .header("Client-Metadata", antigravity_client_metadata())
            .header("X-Machine-Session-Id", machine_session_id())
            .header("X-Goog-Request-Reason", "user_initiated")
            .header("x-cloudaicompanion-trace-id", antigravity_trace_id())
            .json(&meta)
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(val) = resp.json::<serde_json::Value>().await {
                    if let Some(proj_v) = val.get("cloudaicompanionProject") {
                        let pid = proj_v
                            .as_str()
                            .or_else(|| proj_v.get("id").and_then(|v| v.as_str()))
                            .unwrap_or("")
                            .trim();
                        if !pid.is_empty() {
                            found_pid = pid.to_string();
                        }
                    }
                    let paid_lbl = val.get("paidTier").and_then(parse_tier_val);
                    let cur_lbl = val.get("currentTier").and_then(parse_tier_val);
                    found_tier = match (paid_lbl, cur_lbl) {
                        (Some(p), _) if p != "Free" => Some(p),
                        (_, Some(c)) => Some(c),
                        (Some(p), None) => Some(p),
                        (None, None) => None,
                    };
                    if !found_pid.is_empty() || found_tier.is_some() {
                        return (found_pid, found_tier);
                    }
                }
            }
        }
    }
    (found_pid, found_tier)
}

pub async fn fetch_antigravity_project_id(client: &reqwest::Client, access_token: &str) -> String {
    fetch_antigravity_account_meta(client, access_token).await.0
}

pub async fn fetch_antigravity_models(client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let excluded = std::collections::HashSet::new();
    let acc = acquire_valid_account(client, "antigravity", &excluded).await?;
    let pid = match acc.project_id.clone() {
        Some(p) if !p.is_empty() => p,
        _ => fetch_antigravity_project_id(client, &acc.access_token).await,
    };
    let mut req = client
        .post("https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels")
        .bearer_auth(&acc.access_token)
        .header("User-Agent", antigravity_hub_ua())
        .header("X-Goog-Api-Client", antigravity_api_client())
        .header("Client-Metadata", antigravity_client_metadata())
        .header("X-Machine-Session-Id", machine_session_id())
        .header("X-Goog-Request-Reason", "user_initiated")
        .header("x-cloudaicompanion-trace-id", antigravity_trace_id());
    if !pid.is_empty() {
        req = req.header("X-Goog-User-Project", &pid);
    }
    let resp = req
        .json(&serde_json::json!({ "project": pid }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Antigravity models fetch failed ({}): {}", status, txt));
    }
    let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(models_obj) = val.get("models").and_then(|m| m.as_object()) {
        let mut ordered_ids: Vec<String> = Vec::new();
        if let Some(sorts) = val.get("agentModelSorts").and_then(|s| s.as_array()) {
            for s in sorts {
                if let Some(groups) = s.get("groups").and_then(|g| g.as_array()) {
                    for g in groups {
                        if let Some(mids) = g.get("modelIds").and_then(|m| m.as_array()) {
                            for mid in mids {
                                if let Some(id_str) = mid.as_str() {
                                    if seen.insert(id_str.to_string()) {
                                        ordered_ids.push(id_str.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut extra_ids: Vec<String> = models_obj.keys().cloned().collect();
        extra_ids.sort();
        for id in extra_ids {
            if seen.insert(id.clone()) {
                ordered_ids.push(id);
            }
        }

        for id in ordered_ids {
            let Some(info) = models_obj.get(&id) else {
                continue;
            };
            let is_internal = info.get("isInternal").and_then(|v| v.as_bool()).unwrap_or(false);
            let is_disabled = info.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
            let display_name = info.get("displayName").and_then(|v| v.as_str()).unwrap_or("").trim();
            let id_low = id.to_ascii_lowercase();
            let is_image_gen = id_low.contains("image") || id_low.contains("imagen");
            let is_legacy_gemini = !is_image_gen
                && (id_low.starts_with("gemini-2.5")
                    || id_low.starts_with("gemini-3-")
                    || id_low.starts_with("gemini-3.0")
                    || id_low.starts_with("gemini-3.1")
                    || id_low.starts_with("gemini-3.5"));
            if is_internal || is_disabled || display_name.is_empty() || id.starts_with("chat_") || id.starts_with("tab_") || is_legacy_gemini {
                continue;
            }
            let remaining = info
                .get("quotaInfo")
                .and_then(|q| q.get("remainingFraction"))
                .and_then(|v| v.as_f64());
            let is_exhausted = info
                .get("quotaInfo")
                .and_then(|q| q.get("isExhausted"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let reset_time = info
                .get("quotaInfo")
                .and_then(|q| q.get("resetTime"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let lower = id.to_ascii_lowercase();
            let supports_thinking = info.get("supportsThinking").and_then(|v| v.as_bool()).unwrap_or(false);
            let supports_reasoning = supports_thinking
                || lower.contains("thinking")
                || lower.contains("gemini-3")
                || lower.contains("gemini-4")
                || lower.contains("opus")
                || lower.contains("sonnet");
            let def_caps = crate::llm::capabilities::ModelCapabilities::default_for_model(&id);
            let context_window = info
                .get("maxTokens")
                .or_else(|| info.get("maxInputTokens"))
                .or_else(|| info.get("inputTokenLimit"))
                .and_then(|v| v.as_u64())
                .unwrap_or(def_caps.context_window as u64);
            let max_output_tokens = info
                .get("maxOutputTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(def_caps.max_output_tokens as u64);
            out.push(serde_json::json!({
                "id": id,
                "name": display_name,
                "contextWindow": context_window,
                "maxOutputTokens": max_output_tokens,
                "supportsReasoning": supports_reasoning,
                "supportsVision": info.get("supportsImages").and_then(|v| v.as_bool()).unwrap_or(true),
                "supportsTools": true,
                "supportsAudio": lower.contains("gemini"),
                "supportsFiles": true,
                "supportsImageGen": is_image_gen,
                "remainingFraction": remaining,
                "isExhausted": is_exhausted,
                "resetTime": reset_time
            }));
        }
    }
    if out.iter().any(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.starts_with("gemini-3.8-flash")).unwrap_or(false))
        && !out.iter().any(|m| m.get("id").and_then(|v| v.as_str()) == Some("gemini-3.8-flash"))
    {
        let flash_caps = crate::llm::capabilities::ModelCapabilities::default_for_model("gemini-3.8-flash");
        let ref_m = out.iter().find(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.starts_with("gemini-3.8-flash")).unwrap_or(false));
        out.insert(0, serde_json::json!({
            "id": "gemini-3.8-flash",
            "name": "Gemini 3.8 Flash",
            "contextWindow": flash_caps.context_window,
            "maxOutputTokens": flash_caps.max_output_tokens,
            "supportsReasoning": true,
            "supportsVision": true,
            "supportsTools": true,
            "supportsAudio": true,
            "supportsFiles": true,
            "supportsImageGen": false,
            "remainingFraction": ref_m.and_then(|m| m.get("remainingFraction")).cloned(),
            "isExhausted": ref_m.and_then(|m| m.get("isExhausted")).and_then(|v| v.as_bool()).unwrap_or(false),
            "resetTime": ref_m.and_then(|m| m.get("resetTime")).cloned()
        }));
    }
    if out.iter().any(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.starts_with("gpt-oss-120b")).unwrap_or(false))
        && !out.iter().any(|m| m.get("id").and_then(|v| v.as_str()) == Some("gpt-oss-120b"))
    {
        let gpt_caps = crate::llm::capabilities::ModelCapabilities::default_for_model("gpt-oss-120b");
        out.push(serde_json::json!({
            "id": "gpt-oss-120b",
            "name": "GPT-OSS 120B",
            "contextWindow": gpt_caps.context_window,
            "maxOutputTokens": gpt_caps.max_output_tokens,
            "supportsReasoning": true,
            "supportsVision": true,
            "supportsTools": true,
            "supportsAudio": false,
            "supportsFiles": true,
            "supportsImageGen": false,
            "remainingFraction": null,
            "isExhausted": false,
            "resetTime": null
        }));
    }
    Ok(out)
}

pub async fn start_oauth_login(
    app_handle: tauri::AppHandle,
    http_client: reqwest::Client,
    provider: &str,
) -> Result<serde_json::Value, String> {
    if provider == "kimi_oauth" {
        return start_kimi_device_flow(app_handle, http_client).await;
    }
    if provider == "xai_oauth" {
        return start_xai_device_flow(app_handle, http_client).await;
    }

    let (verifier, challenge, state) = generate_pkce_pair();
    let (auth_url, redirect_uri, callback_port, callback_path, mode) = match provider {
        "antigravity" => {
            let scope = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs https://www.googleapis.com/auth/aicode";
            let bound_port = std::net::TcpListener::bind("127.0.0.1:0")
                .ok()
                .and_then(|l| l.local_addr().ok().map(|a| a.port()))
                .unwrap_or(51121);
            let dyn_redirect = format!("http://localhost:{}/auth/callback", bound_port);
            let u = format!(
                "https://accounts.google.com/o/oauth2/auth?client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
                urlencoding::encode(ANTIGRAVITY_CLIENT_ID),
                urlencoding::encode(&dyn_redirect),
                urlencoding::encode(scope),
                urlencoding::encode(&challenge),
                urlencoding::encode(&state),
            );
            (u, dyn_redirect, Some(bound_port), "/auth/callback", "callback")
        }
        "openai_codex" => {
            let scope = "openid profile email offline_access";
            let u = format!(
                "https://auth.openai.com/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}&id_token_add_organizations=true&codex_cli_simplified_flow=true&originator=codex_cli_rs",
                urlencoding::encode(OPENAI_CODEX_CLIENT_ID),
                urlencoding::encode(OPENAI_CODEX_REDIRECT_URI),
                urlencoding::encode(scope),
                urlencoding::encode(&challenge),
                urlencoding::encode(&state),
            );
            (u, OPENAI_CODEX_REDIRECT_URI.to_string(), Some(1455u16), "/auth/callback", "callback")
        }
        "claude_oauth" => {
            let scope = "org:create_api_key user:profile user:inference";
            let claude_state = verifier.clone();
            let u = format!(
                "https://claude.ai/oauth/authorize?code=true&response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
                urlencoding::encode(CLAUDE_OAUTH_CLIENT_ID),
                urlencoding::encode(CLAUDE_OAUTH_REDIRECT_URI),
                urlencoding::encode(scope),
                urlencoding::encode(&challenge),
                urlencoding::encode(&claude_state),
            );
            (u, CLAUDE_OAUTH_REDIRECT_URI.to_string(), Some(54545u16), "/callback", "callback")
        }
        "zcode_oauth" => {
            let zcode_redirect = "zcode://zai-auth/callback";
            let u = format!(
                "https://chat.z.ai/api/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&state={}",
                urlencoding::encode(ZCODE_CLIENT_ID),
                urlencoding::encode(zcode_redirect),
                urlencoding::encode(&state),
            );
            (u, zcode_redirect.to_string(), None, "/callback", "manual")
        }
        other => return Err(format!("Unknown OAuth provider: {}", other)),
    };

    let flow = PendingOAuthFlow {
        provider: provider.to_string(),
        verifier: verifier.clone(),
        state: if provider == "claude_oauth" { verifier } else { state.clone() },
        redirect_uri,
    };

    if let Ok(mut map) = pending_flows().lock() {
        map.insert(provider.to_string(), flow.clone());
    }

    let mut effective_mode = mode.to_string();
    if let Some(port) = callback_port {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => {
                let expected_path = callback_path.to_string();
                let client_clone = http_client.clone();
                let flow_clone = flow.clone();
                let app_clone = app_handle.clone();
                tokio::spawn(async move {
                    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
                    loop {
                        let accepted = tokio::select! {
                            res = listener.accept() => res,
                            _ = tokio::time::sleep_until(deadline) => break,
                        };
                        let Ok((mut socket, _)) = accepted else {
                            break;
                        };
                        let mut buf = vec![0u8; 8192];
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            continue;
                        }
                        let req_text = String::from_utf8_lossy(&buf[..n]);
                        let first_line = req_text.lines().next().unwrap_or("");
                        let raw_target = first_line.split_whitespace().nth(1).unwrap_or("");
                        if !raw_target.starts_with(&expected_path) {
                            let _ = socket
                                .write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\nNot Found")
                                .await;
                            continue;
                        }
                        let full_url = format!("http://localhost{}", raw_target);
                        let parsed = url::Url::parse(&full_url).ok();
                        let code_opt = parsed.as_ref().and_then(|u| {
                            u.query_pairs()
                                .find(|(k, _)| k == "code")
                                .map(|(_, v)| v.to_string())
                        });
                        let st_opt = parsed.as_ref().and_then(|u| {
                            u.query_pairs()
                                .find(|(k, _)| k == "state")
                                .map(|(_, v)| v.to_string())
                        });
                        if let Some(code) = code_opt {
                            match exchange_code_for_account(&client_clone, &flow_clone, &code, st_opt.as_deref()).await {
                                Ok(acc) => {
                                    let body = "<html><body style=\"font-family:sans-serif;background:#111;color:#eee;padding:40px;text-align:center\"><h2>OmniHarness</h2><p>Account connected successfully. You can close this tab.</p></body></html>";
                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                        body.len(),
                                        body
                                    );
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    let _ = app_clone.emit("oauth-accounts-updated", serde_json::json!({ "provider": acc.provider, "email": acc.email }));
                                    break;
                                }
                                Err(err) => {
                                    let body = format!("<html><body><h3>OAuth Error</h3><pre>{}</pre></body></html>", err);
                                    let resp = format!(
                                        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                        body.len(),
                                        body
                                    );
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    break;
                                }
                            }
                        } else {
                            let _ = socket
                                .write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nMissing code")
                                .await;
                        }
                    }
                });
            }
            Err(_) => {
                effective_mode = "manual".to_string();
            }
        }
    }

    if provider != "zcode_oauth" {
        open_url_in_browser(&auth_url);
    }

    Ok(serde_json::json!({
        "authUrl": auth_url,
        "mode": effective_mode,
        "provider": provider
    }))
}

pub async fn complete_manual_oauth(
    app_handle: tauri::AppHandle,
    http_client: reqwest::Client,
    provider: &str,
    code_or_url: &str,
) -> Result<OAuthAccountPublic, String> {
    let flow = pending_flows()
        .lock()
        .ok()
        .and_then(|m| m.get(provider).cloned())
        .ok_or("No pending OAuth session found. Click 'Login in Browser' first.")?;

    let raw = code_or_url.trim();
    if raw.is_empty() {
        return Err("Authorization code or redirect URL cannot be empty".to_string());
    }

    let (code, st) = if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("zcode://") {
        let u = url::Url::parse(raw).map_err(|e| format!("Invalid redirect URL: {}", e))?;
        let c = u
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.to_string())
            .ok_or("Redirect URL does not contain 'code' parameter")?;
        let s = u.query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.to_string());
        (c, s)
    } else if let Some((c, s)) = raw.split_once('#') {
        (c.trim().to_string(), Some(s.trim().to_string()))
    } else {
        (raw.to_string(), None)
    };

    let acc = exchange_code_for_account(&http_client, &flow, &code, st.as_deref()).await?;
    let pub_acc = OAuthAccountPublic::from(&acc);
    let _ = app_handle.emit(
        "oauth-accounts-updated",
        serde_json::json!({ "provider": acc.provider, "email": acc.email }),
    );
    Ok(pub_acc)
}

pub async fn import_local_accounts(
    app_handle: tauri::AppHandle,
    http_client: reqwest::Client,
) -> Result<Vec<OAuthAccountPublic>, String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "Cannot resolve user home directory".to_string())?;
    let h = std::path::Path::new(&home);
    let mut imported = Vec::new();
    let now = chrono::Utc::now().timestamp_millis();

    let ag_opencode = h.join(".config").join("opencode").join("antigravity-accounts.json");
    if ag_opencode.is_file() {
        if let Ok(raw) = std::fs::read_to_string(&ag_opencode) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(arr) = val.get("accounts").and_then(|a| a.as_array()) {
                    for item in arr {
                        let rt = item.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("");
                        if rt.is_empty() {
                            continue;
                        }
                        let email = item.get("email").and_then(|v| v.as_str()).unwrap_or("antigravity-local").to_string();
                        let pid = item
                            .get("projectId")
                            .or_else(|| item.get("managedProjectId"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let mut acc = OAuthAccount {
                            id: format!("ag-{}", uuid::Uuid::new_v4().simple()),
                            provider: "antigravity".to_string(),
                            email,
                            access_token: String::new(),
                            refresh_token: rt.to_string(),
                            expires_at_ms: 0,
                            project_id: pid,
                            account_id: None,
                            enabled: true,
                            cooldown_until_ms: None,
                            last_error: None,
                            last_used_ms: 0,
                            quota_5h_remaining: None,
                            quota_5h_reset_at_ms: None,
                            quota_weekly_remaining: None,
                            quota_weekly_reset_at_ms: None,
                            quota_label: None,
                            quota_updated_at_ms: None,
                            plan_tier: None,
                            allowed_models: None,
                        };
                        let _ = refresh_account_tokens(&http_client, &mut acc).await;
                        if acc.project_id.is_none() && !acc.access_token.is_empty() {
                            acc.project_id = Some(fetch_antigravity_project_id(&http_client, &acc.access_token).await);
                        }
                        if let Ok(saved) = upsert_account(acc).await {
                            imported.push(OAuthAccountPublic::from(&saved));
                        }
                    }
                }
            }
        }
    }

    let mut ag_raw_blobs: Vec<String> = Vec::new();
    for (svc, usr) in [("gemini", "antigravity"), ("gemini:antigravity", "antigravity")] {
        if let Ok(entry) = keyring::Entry::new(svc, usr) {
            if let Ok(secret) = entry.get_password() {
                if !secret.trim().is_empty() && !ag_raw_blobs.contains(&secret) {
                    ag_raw_blobs.push(secret);
                }
            }
        }
    }
    for rel in [
        h.join(".gemini").join("jetski-standalone-oauth-token"),
        h.join(".gemini").join("antigravity-cli").join("antigravity-oauth-token"),
    ] {
        if rel.is_file() {
            if let Ok(raw) = std::fs::read_to_string(&rel) {
                ag_raw_blobs.push(raw);
            }
        }
    }
    for raw in ag_raw_blobs {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
            let tok = val.get("token").unwrap_or(&val);
            let rt = tok.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let at = tok.get("access_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !rt.is_empty() || !at.is_empty() {
                let mut email = val
                    .get("id_token_email")
                    .or_else(|| val.get("email"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut acc = OAuthAccount {
                    id: format!("ag-{}", uuid::Uuid::new_v4().simple()),
                    provider: "antigravity".to_string(),
                    email: if email.is_empty() { "antigravity-local".to_string() } else { email.clone() },
                    access_token: at,
                    refresh_token: rt,
                    expires_at_ms: if tok.get("access_token").is_some() && !tok.get("refresh_token").is_some() { now + 1800_000 } else { 0 },
                    project_id: None,
                    account_id: None,
                    enabled: true,
                    cooldown_until_ms: None,
                    last_error: None,
                    last_used_ms: 0,
                    quota_5h_remaining: None,
                    quota_5h_reset_at_ms: None,
                    quota_weekly_remaining: None,
                    quota_weekly_reset_at_ms: None,
                    quota_label: None,
                    quota_updated_at_ms: None,
                    plan_tier: None,
                    allowed_models: None,
                };
                if !acc.refresh_token.is_empty() {
                    let _ = refresh_account_tokens(&http_client, &mut acc).await;
                }
                if !acc.access_token.is_empty() {
                    if email.is_empty() {
                        if let Ok(r) = http_client
                            .get("https://www.googleapis.com/oauth2/v1/userinfo?alt=json")
                            .bearer_auth(&acc.access_token)
                            .send()
                            .await
                        {
                            if let Ok(u) = r.json::<serde_json::Value>().await {
                                if let Some(em) = u.get("email").and_then(|v| v.as_str()) {
                                    if !em.is_empty() {
                                        email = em.to_string();
                                        acc.email = email;
                                    }
                                }
                            }
                        }
                    }
                    let (pid, tier_lbl) = fetch_antigravity_account_meta(&http_client, &acc.access_token).await;
                    acc.project_id = Some(pid);
                    if tier_lbl.is_some() {
                        acc.quota_label = tier_lbl;
                    }
                }
                if let Ok(saved) = upsert_account(acc).await {
                    imported.push(OAuthAccountPublic::from(&saved));
                }
            }
        }
    }

    let claude_creds = h.join(".claude").join(".credentials.json");
    if claude_creds.is_file() {
        if let Ok(raw) = std::fs::read_to_string(&claude_creds) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                let oauth_obj = val.get("claudeAiOauth").unwrap_or(&val);
                let at = oauth_obj.get("accessToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let rt = oauth_obj.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let exp = oauth_obj.get("expiresAt").and_then(|v| v.as_i64()).unwrap_or(now + 1800_000);
                if !at.is_empty() || !rt.is_empty() {
                    let acc = OAuthAccount {
                        id: format!("claude-{}", uuid::Uuid::new_v4().simple()),
                        provider: "claude_oauth".to_string(),
                        email: "claude-local".to_string(),
                        access_token: at,
                        refresh_token: rt,
                        expires_at_ms: exp,
                        project_id: None,
                        account_id: None,
                        enabled: true,
                        cooldown_until_ms: None,
                        last_error: None,
                        last_used_ms: 0,
                        quota_5h_remaining: None,
                        quota_5h_reset_at_ms: None,
                        quota_weekly_remaining: None,
                        quota_weekly_reset_at_ms: None,
                        quota_label: None,
                        quota_updated_at_ms: None,
                        plan_tier: None,
                        allowed_models: None,
                    };
                    if let Ok(saved) = upsert_account(acc).await {
                        imported.push(OAuthAccountPublic::from(&saved));
                    }
                }
            }
        }
    }

    let codex_auth = h.join(".codex").join("auth.json");
    if codex_auth.is_file() {
        if let Ok(raw) = std::fs::read_to_string(&codex_auth) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                let tokens = val.get("tokens").unwrap_or(&val);
                let at = tokens.get("access_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let rt = tokens.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let id_tok = tokens.get("id_token").and_then(|v| v.as_str());
                if !at.is_empty() || !rt.is_empty() {
                    let (email_opt, account_id_opt, plan_opt) = extract_chatgpt_claims_full(id_tok, &at);
                    let acc = OAuthAccount {
                        id: format!("codex-{}", uuid::Uuid::new_v4().simple()),
                        provider: "openai_codex".to_string(),
                        email: email_opt.unwrap_or_else(|| "codex-local".to_string()),
                        access_token: at,
                        refresh_token: rt,
                        expires_at_ms: now + 1800_000,
                        project_id: None,
                        account_id: account_id_opt,
                        enabled: true,
                        cooldown_until_ms: None,
                        last_error: None,
                        last_used_ms: 0,
                        quota_5h_remaining: None,
                        quota_5h_reset_at_ms: None,
                        quota_weekly_remaining: None,
                        quota_weekly_reset_at_ms: None,
                        quota_label: plan_opt,
                        quota_updated_at_ms: None,
                        plan_tier: None,
                        allowed_models: None,
                    };
                    if let Ok(saved) = upsert_account(acc).await {
                        imported.push(OAuthAccountPublic::from(&saved));
                    }
                }
            }
        }
    }

    if !imported.is_empty() {
        let _ = app_handle.emit("oauth-accounts-updated", serde_json::json!({ "imported": imported.len() }));
    }
    Ok(imported)
}

pub async fn fetch_codex_models(client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let excluded = std::collections::HashSet::new();
    if let Ok(acc) = acquire_valid_account(client, "openai_codex", &excluded).await {
        let mut req = client
            .get("https://chatgpt.com/backend-api/codex/models?client_version=0.153.4")
            .bearer_auth(&acc.access_token)
            .header("User-Agent", "codex_cli_rs/0.153.4 (Windows NT 10.0; x86_64)")
            .header("Originator", "codex_cli_rs")
            .header("Accept", "application/json");
        if let Some(ref aid) = acc.account_id {
            if !aid.trim().is_empty() {
                req = req.header("ChatGPT-Account-Id", aid.trim());
            }
        }
        if let Ok(resp) = req.send().await {
            if resp.status().is_success() {
                if let Ok(val) = resp.json::<serde_json::Value>().await {
                    if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
                        let mut out = Vec::new();
                        let mut slugs = Vec::new();
                        for m in arr {
                            let id = m.get("slug").or_else(|| m.get("id")).and_then(|v| v.as_str()).unwrap_or("").trim();
                            if id.is_empty() {
                                continue;
                            }
                            slugs.push(id.to_string());
                            let name = m.get("display_name").or_else(|| m.get("name")).and_then(|v| v.as_str()).unwrap_or(id);
                            let def_caps = crate::llm::capabilities::ModelCapabilities::default_for_model(id);
                            let ctx = m.get("context_window")
                                .or_else(|| m.get("max_context_length"))
                                .or_else(|| m.get("max_tokens"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(def_caps.context_window as u64);
                            let max_out = m.get("max_output_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(def_caps.max_output_tokens as u64);
                            out.push(serde_json::json!({
                                "id": id,
                                "name": name,
                                "contextWindow": ctx,
                                "maxOutputTokens": max_out,
                                "supportsReasoning": def_caps.supports_reasoning,
                                "supportsVision": def_caps.supports_vision,
                                "supportsTools": def_caps.supports_tools,
                                "supportsAudio": false,
                                "supportsFiles": true
                            }));
                        }
                        if !out.is_empty() {
                            update_account_subscription(&acc.id, acc.plan_tier.clone(), Some(slugs)).await;
                            return Ok(out);
                        }
                    }
                }
            }
        }
    }
    let fallback = [
        ("gpt-5.4", "GPT-5.4"),
        ("gpt-5.3-codex", "GPT-5.3 Codex"),
        ("gpt-5.2-codex", "GPT-5.2 Codex"),
        ("gpt-5.1-codex-max", "GPT-5.1 Codex Max"),
        ("gpt-5-codex", "GPT-5 Codex"),
        ("gpt-5", "GPT-5"),
        ("codex-mini-latest", "Codex Mini"),
    ];
    Ok(fallback
        .iter()
        .map(|(id, name)| {
            let def_caps = crate::llm::capabilities::ModelCapabilities::default_for_model(id);
            serde_json::json!({
                "id": id,
                "name": name,
                "contextWindow": def_caps.context_window,
                "maxOutputTokens": def_caps.max_output_tokens,
                "supportsReasoning": def_caps.supports_reasoning,
                "supportsVision": def_caps.supports_vision,
                "supportsTools": def_caps.supports_tools,
                "supportsAudio": false,
                "supportsFiles": true
            })
        })
        .collect())
}

pub async fn fetch_claude_models(client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let excluded = std::collections::HashSet::new();
    if let Ok(acc) = acquire_valid_account(client, "claude_oauth", &excluded).await {
        if let Ok(resp) = client
            .get("https://api.anthropic.com/v1/models")
            .header("Authorization", format!("Bearer {}", acc.access_token))
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "oauth-2025-04-20,claude-code-20250219")
            .header("user-agent", "claude-cli/1.0.0 (external, cli)")
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(val) = resp.json::<serde_json::Value>().await {
                    if let Some(arr) = val.get("data").and_then(|d| d.as_array()) {
                        let mut out = Vec::new();
                        for m in arr {
                            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("").trim();
                            if id.is_empty() {
                                continue;
                            }
                            let name = m.get("display_name").and_then(|v| v.as_str()).unwrap_or(id);
                            let def_caps = crate::llm::capabilities::ModelCapabilities::default_for_model(id);
                            out.push(serde_json::json!({
                                "id": id,
                                "name": name,
                                "contextWindow": def_caps.context_window,
                                "maxOutputTokens": def_caps.max_output_tokens,
                                "supportsReasoning": def_caps.supports_reasoning,
                                "supportsVision": def_caps.supports_vision,
                                "supportsTools": def_caps.supports_tools,
                                "supportsAudio": false,
                                "supportsFiles": true
                            }));
                        }
                        if !out.is_empty() {
                            return Ok(out);
                        }
                    }
                }
            }
        }
    }
    let fallback = [
        ("claude-opus-4-6", "Claude Opus 4.6"),
        ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
        ("claude-haiku-4-5", "Claude Haiku 4.5"),
    ];
    Ok(fallback
        .iter()
        .map(|(id, name)| {
            let def_caps = crate::llm::capabilities::ModelCapabilities::default_for_model(id);
            serde_json::json!({
                "id": id,
                "name": name,
                "contextWindow": def_caps.context_window,
                "maxOutputTokens": def_caps.max_output_tokens,
                "supportsReasoning": def_caps.supports_reasoning,
                "supportsVision": def_caps.supports_vision,
                "supportsTools": def_caps.supports_tools,
                "supportsAudio": false,
                "supportsFiles": true
            })
        })
        .collect())
}


async fn start_kimi_device_flow(
    app_handle: tauri::AppHandle,
    http_client: reqwest::Client,
) -> Result<serde_json::Value, String> {
    let device_id = uuid::Uuid::new_v4().to_string();
    let resp = http_client
        .post("https://auth.kimi.com/api/oauth/device_authorization")
        .header("X-Msh-Platform", "kimi_cli")
        .header("X-Msh-Version", "1.0.0")
        .header("X-Msh-Device-Id", &device_id)
        .form(&[("client_id", KIMI_CLIENT_ID)])
        .send()
        .await
        .map_err(|e| format!("Kimi device auth failed: {}", e))?;
    if !resp.status().is_success() {
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("Kimi device auth error: {}", txt));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let device_code = data.get("device_code").and_then(|v| v.as_str()).ok_or("Missing device_code")?.to_string();
    let user_code = data.get("user_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let v_uri = data
        .get("verification_uri_complete")
        .or_else(|| data.get("verification_uri"))
        .and_then(|v| v.as_str())
        .ok_or("Missing verification_uri")?
        .to_string();
    let interval = data.get("interval").and_then(|v| v.as_u64()).unwrap_or(5).max(3);

    open_url_in_browser(&v_uri);

    let app_clone = app_handle.clone();
    let client_clone = http_client.clone();
    let dev_id_clone = device_id.clone();
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(600);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            let Ok(r) = client_clone
                .post("https://auth.kimi.com/api/oauth/token")
                .header("X-Msh-Platform", "kimi_cli")
                .header("X-Msh-Version", "1.0.0")
                .header("X-Msh-Device-Id", &dev_id_clone)
                .form(&[
                    ("client_id", KIMI_CLIENT_ID),
                    ("device_code", device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await
            else {
                continue;
            };
            let Ok(val) = r.json::<serde_json::Value>().await else {
                continue;
            };
            if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
                if err == "authorization_pending" || err == "slow_down" {
                    continue;
                }
                break;
            }
            if let Some(at) = val.get("access_token").and_then(|v| v.as_str()) {
                let rt = val.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let exp_in = val.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
                let now = chrono::Utc::now().timestamp_millis();
                let acc = OAuthAccount {
                    id: format!("kimi-{}", uuid::Uuid::new_v4().simple()),
                    provider: "kimi_oauth".to_string(),
                    email: format!("kimi-{}", &dev_id_clone[..6]),
                    access_token: at.to_string(),
                    refresh_token: rt,
                    expires_at_ms: now + exp_in * 1000 - 60_000,
                    project_id: None,
                    account_id: Some("kimi.com".to_string()),
                    enabled: true,
                    cooldown_until_ms: None,
                    last_error: None,
                    last_used_ms: 0,
                    quota_5h_remaining: None,
                    quota_5h_reset_at_ms: None,
                    quota_weekly_remaining: None,
                    quota_weekly_reset_at_ms: None,
                    quota_label: None,
                    quota_updated_at_ms: None,
                    plan_tier: None,
                    allowed_models: None,
                };
                if let Ok(saved) = upsert_account(acc).await {
                    let _ = app_clone.emit(
                        "oauth-accounts-updated",
                        serde_json::json!({ "provider": "kimi_oauth", "email": saved.email }),
                    );
                }
                break;
            }
        }
    });

    Ok(serde_json::json!({
        "authUrl": v_uri,
        "userCode": user_code,
        "mode": "device_code",
        "provider": "kimi_oauth"
    }))
}

async fn start_xai_device_flow(
    app_handle: tauri::AppHandle,
    http_client: reqwest::Client,
) -> Result<serde_json::Value, String> {
    let disc_resp = http_client
        .get("https://auth.x.ai/.well-known/openid-configuration")
        .send()
        .await
        .map_err(|e| format!("xAI discovery failed: {}", e))?;
    if !disc_resp.status().is_success() {
        let txt = disc_resp.text().await.unwrap_or_default();
        return Err(format!("xAI discovery error: {}", txt));
    }
    let disc: serde_json::Value = disc_resp.json().await.map_err(|e| e.to_string())?;
    let dev_ep = disc
        .get("device_authorization_endpoint")
        .and_then(|v| v.as_str())
        .ok_or("xAI discovery missing device_authorization_endpoint")?
        .to_string();
    let tok_ep = disc
        .get("token_endpoint")
        .and_then(|v| v.as_str())
        .ok_or("xAI discovery missing token_endpoint")?
        .to_string();

    let resp = http_client
        .post(&dev_ep)
        .form(&[("client_id", XAI_CLIENT_ID), ("scope", XAI_SCOPE)])
        .send()
        .await
        .map_err(|e| format!("xAI device code request failed: {}", e))?;
    if !resp.status().is_success() {
        let txt = resp.text().await.unwrap_or_default();
        return Err(format!("xAI device code error: {}", txt));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let device_code = data.get("device_code").and_then(|v| v.as_str()).ok_or("Missing device_code")?.to_string();
    let user_code = data.get("user_code").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let v_uri = data
        .get("verification_uri_complete")
        .or_else(|| data.get("verification_uri"))
        .and_then(|v| v.as_str())
        .ok_or("Missing verification_uri")?
        .to_string();
    let interval = data.get("interval").and_then(|v| v.as_u64()).unwrap_or(5).max(3);

    open_url_in_browser(&v_uri);

    let app_clone = app_handle.clone();
    let client_clone = http_client.clone();
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(600);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            let Ok(r) = client_clone
                .post(&tok_ep)
                .form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device_code.as_str()),
                    ("client_id", XAI_CLIENT_ID),
                ])
                .send()
                .await
            else {
                continue;
            };
            let Ok(val) = r.json::<serde_json::Value>().await else {
                continue;
            };
            if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
                if err == "authorization_pending" || err == "slow_down" {
                    continue;
                }
                break;
            }
            if let Some(at) = val.get("access_token").and_then(|v| v.as_str()) {
                let rt = val.get("refresh_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let id_tok = val.get("id_token").and_then(|v| v.as_str());
                let exp_in = val.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
                let (email_opt, _) = extract_chatgpt_claims(id_tok, at);
                let now = chrono::Utc::now().timestamp_millis();
                let acc = OAuthAccount {
                    id: format!("xai-{}", uuid::Uuid::new_v4().simple()),
                    provider: "xai_oauth".to_string(),
                    email: email_opt.unwrap_or_else(|| "grok-user".to_string()),
                    access_token: at.to_string(),
                    refresh_token: rt,
                    expires_at_ms: now + exp_in * 1000 - 60_000,
                    project_id: None,
                    account_id: None,
                    enabled: true,
                    cooldown_until_ms: None,
                    last_error: None,
                    last_used_ms: 0,
                    quota_5h_remaining: None,
                    quota_5h_reset_at_ms: None,
                    quota_weekly_remaining: None,
                    quota_weekly_reset_at_ms: None,
                    quota_label: None,
                    quota_updated_at_ms: None,
                    plan_tier: None,
                    allowed_models: None,
                };
                if let Ok(saved) = upsert_account(acc).await {
                    let _ = app_clone.emit(
                        "oauth-accounts-updated",
                        serde_json::json!({ "provider": "xai_oauth", "email": saved.email }),
                    );
                }
                break;
            }
        }
    });

    Ok(serde_json::json!({
        "authUrl": v_uri,
        "userCode": user_code,
        "mode": "device_code",
        "provider": "xai_oauth"
    }))
}

pub async fn add_manual_token_account(
    app_handle: tauri::AppHandle,
    provider: &str,
    label: &str,
    token: &str,
) -> Result<OAuthAccountPublic, String> {
    let tok = token.trim();
    if tok.is_empty() {
        return Err("Token or API key cannot be empty".to_string());
    }
    let email = if label.trim().is_empty() {
        format!("{}-{}", provider, &uuid::Uuid::new_v4().simple().to_string()[..6])
    } else {
        label.trim().to_string()
    };
    let acc = OAuthAccount {
        id: format!("{}-{}", provider, uuid::Uuid::new_v4().simple()),
        provider: provider.to_string(),
        email,
        access_token: tok.to_string(),
        refresh_token: String::new(),
        expires_at_ms: 0,
        project_id: None,
        account_id: None,
        enabled: true,
        cooldown_until_ms: None,
        last_error: None,
        last_used_ms: 0,
        quota_5h_remaining: None,
        quota_5h_reset_at_ms: None,
        quota_weekly_remaining: None,
        quota_weekly_reset_at_ms: None,
        quota_label: None,
        quota_updated_at_ms: None,
        plan_tier: None,
        allowed_models: None,
    };
    let saved = upsert_account(acc).await?;
    let pub_acc = OAuthAccountPublic::from(&saved);
    let _ = app_handle.emit(
        "oauth-accounts-updated",
        serde_json::json!({ "provider": saved.provider, "email": saved.email }),
    );
    Ok(pub_acc)
}

async fn fetch_account_quota_inner(client: &reqwest::Client, acc: &mut OAuthAccount) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    if acc.access_token.is_empty() || (acc.expires_at_ms > 0 && acc.expires_at_ms <= now) {
        if !acc.refresh_token.is_empty() {
            refresh_account_tokens(client, acc).await?;
        }
    }
    match acc.provider.as_str() {
        "openai_codex" => {
            let mut req = client
                .get("https://chatgpt.com/backend-api/wham/usage")
                .bearer_auth(&acc.access_token)
                .header("Accept", "application/json");
            if let Some(ref aid) = acc.account_id {
                if !aid.trim().is_empty() {
                    req = req.header("ChatGPT-Account-Id", aid.trim());
                }
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Codex usage HTTP {}: {}", status.as_u16(), txt));
            }
            let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if let Some(plan) = val.get("plan_type").and_then(|v| v.as_str()) {
                acc.quota_label = format_chatgpt_plan_label(plan);
            } else if acc.quota_label.is_none() {
                let (_, _, plan_opt) = extract_chatgpt_claims_full(None, &acc.access_token);
                acc.quota_label = plan_opt;
            }
            let rl = val.get("rate_limit");
            let allowed = rl.and_then(|r| r.get("allowed")).and_then(|v| v.as_bool()).unwrap_or(true);
            for win_key in ["primary_window", "secondary_window"] {
                if let Some(win) = rl.and_then(|r| r.get(win_key)) {
                    if win.is_null() {
                        continue;
                    }
                    let used = win.get("used_percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let rem = if allowed { (100.0 - used).clamp(0.0, 100.0) } else { 0.0 };
                    let secs = win.get("limit_window_seconds").and_then(|v| v.as_i64()).unwrap_or(18000);
                    let reset_ms = win
                        .get("reset_at")
                        .and_then(|v| v.as_i64())
                        .map(|s| s * 1000)
                        .or_else(|| {
                            win.get("reset_after_seconds")
                                .and_then(|v| v.as_i64())
                                .map(|s| now + s * 1000)
                        });
                    if secs <= 21600 {
                        acc.quota_5h_remaining = Some(rem);
                        acc.quota_5h_reset_at_ms = reset_ms;
                    } else {
                        acc.quota_weekly_remaining = Some(rem);
                        acc.quota_weekly_reset_at_ms = reset_ms;
                    }
                }
            }
            acc.quota_updated_at_ms = Some(now);
            acc.last_error = None;
            acc.cooldown_until_ms = None;
            Ok(())
        }
        "claude_oauth" => {
            let resp = client
                .get("https://api.anthropic.com/api/oauth/usage")
                .header("Authorization", format!("Bearer {}", acc.access_token))
                .header("anthropic-version", "2023-06-01")
                .header("anthropic-beta", "oauth-2025-04-20")
                .header("user-agent", "claude-cli/1.0.0 (external, cli)")
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Claude usage HTTP {}: {}", status.as_u16(), txt));
            }
            let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if let Some(fh) = val.get("five_hour") {
                if let Some(util) = fh.get("utilization").and_then(|v| v.as_f64()) {
                    let pct = if util <= 1.0 { util * 100.0 } else { util };
                    acc.quota_5h_remaining = Some((100.0 - pct).clamp(0.0, 100.0));
                }
                if let Some(ra) = fh.get("resets_at").and_then(|v| v.as_str()) {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ra) {
                        acc.quota_5h_reset_at_ms = Some(dt.timestamp_millis());
                    }
                }
            }
            if let Some(sd) = val.get("seven_day") {
                if let Some(util) = sd.get("utilization").and_then(|v| v.as_f64()) {
                    let pct = if util <= 1.0 { util * 100.0 } else { util };
                    acc.quota_weekly_remaining = Some((100.0 - pct).clamp(0.0, 100.0));
                }
                if let Some(ra) = sd.get("resets_at").and_then(|v| v.as_str()) {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ra) {
                        acc.quota_weekly_reset_at_ms = Some(dt.timestamp_millis());
                    }
                }
            }
            acc.quota_updated_at_ms = Some(now);
            acc.last_error = None;
            acc.cooldown_until_ms = None;
            Ok(())
        }
        "antigravity" => {
            let need_meta = acc.project_id.as_deref().map(|p| p.is_empty()).unwrap_or(true)
                || acc.quota_label.as_deref().map(|l| l.is_empty()).unwrap_or(true);
            let pid = if need_meta {
                let (fetched_pid, fetched_tier) = fetch_antigravity_account_meta(client, &acc.access_token).await;
                if !fetched_pid.is_empty() {
                    acc.project_id = Some(fetched_pid.clone());
                }
                if fetched_tier.is_some() {
                    acc.quota_label = fetched_tier;
                }
                acc.project_id.clone().unwrap_or_default()
            } else {
                if let (_, Some(fetched_tier)) = fetch_antigravity_account_meta(client, &acc.access_token).await {
                    acc.quota_label = Some(fetched_tier);
                }
                acc.project_id.clone().unwrap_or_default()
            };
            let resp = client
                .post("https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels")
                .bearer_auth(&acc.access_token)
                .header("User-Agent", antigravity_hub_ua())
                .header("X-Machine-Session-Id", machine_session_id())
                .json(&serde_json::json!({ "project": pid }))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Antigravity check HTTP {}: {}", status.as_u16(), txt));
            }
            let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let mut min_5h: Option<f64> = None;
            let mut reset_5h: Option<i64> = None;
            let mut min_weekly: Option<f64> = None;
            let mut reset_weekly: Option<i64> = None;
            let mut fallback_min: Option<f64> = None;
            let mut fallback_reset: Option<i64> = None;
            if let Some(models) = val.get("models").and_then(|m| m.as_object()) {
                for (_, info) in models {
                    if info.get("isInternal").and_then(|v| v.as_bool()).unwrap_or(false) {
                        continue;
                    }
                    if let Some(qi) = info.get("quotaInfo") {
                        if let Some(w_frac) = qi.get("weeklyRemainingFraction").or_else(|| qi.get("weeklyQuotaInfo").and_then(|w| w.get("remainingFraction"))).and_then(|v| v.as_f64()) {
                            min_weekly = Some(min_weekly.map(|m| m.min(w_frac)).unwrap_or(w_frac));
                        }
                        if let Some(wrt) = qi.get("weeklyResetTime").or_else(|| qi.get("weeklyQuotaInfo").and_then(|w| w.get("resetTime"))).and_then(|v| v.as_str()) {
                            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(wrt) {
                                let ms = dt.timestamp_millis();
                                reset_weekly = Some(reset_weekly.map(|e| e.min(ms)).unwrap_or(ms));
                            }
                        }
                        let frac_opt = qi.get("remainingFraction").and_then(|v| v.as_f64());
                        let rt_ms = qi.get("resetTime").and_then(|v| v.as_str()).and_then(|rt| chrono::DateTime::parse_from_rfc3339(rt).ok()).map(|dt| dt.timestamp_millis());
                        if let Some(frac) = frac_opt {
                            fallback_min = Some(fallback_min.map(|m| m.min(frac)).unwrap_or(frac));
                            if let Some(ms) = rt_ms {
                                fallback_reset = Some(fallback_reset.map(|e| e.min(ms)).unwrap_or(ms));
                                if ms - now > 18 * 3600 * 1000 {
                                    min_weekly = Some(min_weekly.map(|m| m.min(frac)).unwrap_or(frac));
                                    reset_weekly = Some(reset_weekly.map(|e| e.min(ms)).unwrap_or(ms));
                                } else {
                                    min_5h = Some(min_5h.map(|m| m.min(frac)).unwrap_or(frac));
                                    reset_5h = Some(reset_5h.map(|e| e.min(ms)).unwrap_or(ms));
                                }
                            } else {
                                min_5h = Some(min_5h.map(|m| m.min(frac)).unwrap_or(frac));
                            }
                        }
                    }
                }
            }
            if let Some(frac) = min_5h.or(fallback_min) {
                acc.quota_5h_remaining = Some((frac * 100.0).clamp(0.0, 100.0));
                acc.quota_5h_reset_at_ms = reset_5h.or(fallback_reset);
            }
            if let Some(w_frac) = min_weekly {
                acc.quota_weekly_remaining = Some((w_frac * 100.0).clamp(0.0, 100.0));
                acc.quota_weekly_reset_at_ms = reset_weekly;
            }
            acc.quota_updated_at_ms = Some(now);
            acc.last_error = None;
            acc.cooldown_until_ms = None;
            Ok(())
        }
        "zcode_oauth" => {
            let resp = client
                .get("https://api.z.ai/api/monitor/usage/quota/limit")
                .bearer_auth(&acc.access_token)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let status = resp.status();
            if !status.is_success() {
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("ZCode quota HTTP {}: {}", status.as_u16(), txt));
            }
            let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            if let Some(arr) = val.get("plans").or_else(|| val.get("balances")).and_then(|v| v.as_array()) {
                if let Some(first) = arr.first() {
                    let total = first.get("total_units").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let avail = first.get("available_units").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    if total > 0.0 {
                        acc.quota_weekly_remaining = Some(((avail / total) * 100.0).clamp(0.0, 100.0));
                    }
                    if let Some(pe) = first.get("period_end").and_then(|v| v.as_i64()) {
                        acc.quota_weekly_reset_at_ms = Some(pe * 1000);
                    }
                }
            }
            acc.quota_updated_at_ms = Some(now);
            acc.last_error = None;
            acc.cooldown_until_ms = None;
            Ok(())
        }
        "kimi_oauth" => {
            let host = if acc.account_id.as_deref() == Some("kimi.ai") {
                "https://api.kimi.ai/coding/v1/models"
            } else {
                "https://api.kimi.com/coding/v1/models"
            };
            let resp = client.get(host).bearer_auth(&acc.access_token).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let st = resp.status().as_u16();
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Kimi check HTTP {}: {}", st, txt));
            }
            acc.quota_updated_at_ms = Some(now);
            acc.last_error = None;
            acc.cooldown_until_ms = None;
            Ok(())
        }
        "xai_oauth" => {
            let resp = client
                .get("https://api.x.ai/v1/models")
                .bearer_auth(&acc.access_token)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let st = resp.status().as_u16();
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("xAI check HTTP {}: {}", st, txt));
            }
            acc.quota_updated_at_ms = Some(now);
            acc.last_error = None;
            acc.cooldown_until_ms = None;
            Ok(())
        }
        "perplexity_oauth" => {
            let resp = client
                .post("https://api.perplexity.ai/chat/completions")
                .bearer_auth(&acc.access_token)
                .json(&serde_json::json!({
                    "model": "sonar",
                    "messages": [{ "role": "user", "content": "ping" }],
                    "max_tokens": 1
                }))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let st = resp.status().as_u16();
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Perplexity check HTTP {}: {}", st, txt));
            }
            acc.quota_updated_at_ms = Some(now);
            acc.last_error = None;
            acc.cooldown_until_ms = None;
            Ok(())
        }
        _ => Ok(()),
    }
}

pub async fn check_account_liveness(client: &reqwest::Client, id: &str) -> Result<OAuthAccountPublic, String> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    let idx = list.iter().position(|a| a.id == id).ok_or("Account not found")?;
    let mut acc = list[idx].clone();
    let res = fetch_account_quota_inner(client, &mut acc).await;
    if let Err(ref e) = res {
        acc.last_error = Some(e.chars().take(240).collect());
    }
    list[idx] = acc.clone();
    write_accounts_unlocked(&list)?;
    match res {
        Ok(()) => Ok(OAuthAccountPublic::from(&acc)),
        Err(e) => Err(e),
    }
}

pub async fn refresh_all_quotas(client: &reqwest::Client, provider: Option<&str>) -> Result<Vec<OAuthAccountPublic>, String> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    for acc in list.iter_mut() {
        if let Some(p) = provider {
            if acc.provider != p {
                continue;
            }
        }
        if !acc.enabled {
            continue;
        }
        if let Err(e) = fetch_account_quota_inner(client, acc).await {
            acc.last_error = Some(e.chars().take(240).collect());
        }
    }
    write_accounts_unlocked(&list)?;
    Ok(list
        .iter()
        .filter(|a| provider.map(|p| a.provider == p).unwrap_or(true))
        .map(OAuthAccountPublic::from)
        .collect())
}

pub async fn fetch_kimi_models(_client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let models = [
        ("kimi-k3.1-pro", "Kimi K3.1 Pro", 1_000_000u64, 65536u64, true, true),
        ("kimi-k3", "Kimi K3", 1_000_000u64, 65536u64, true, true),
        ("kimi-k2.8-code", "Kimi K2.8 Code", 262_144u64, 65536u64, true, true),
        ("kimi-k2.6", "Kimi K2.6", 262_144u64, 65536u64, true, true),
        ("kimi-k2.5", "Kimi K2.5", 262_144u64, 32768u64, true, true),
        ("kimi-k2-thinking", "Kimi K2 Thinking", 131_072u64, 32768u64, true, false),
    ];
    Ok(models
        .iter()
        .map(|(id, name, ctx, out, r, v)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "contextWindow": ctx,
                "maxOutputTokens": out,
                "supportsReasoning": r,
                "supportsVision": v,
                "supportsTools": true,
                "supportsAudio": false,
                "supportsFiles": true
            })
        })
        .collect())
}

pub async fn fetch_xai_models(_client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let models = [
        ("grok-4.7", "Grok 4.7", 500_000u64, 128_000u64, true, true),
        ("grok-4.7-build-fast", "Grok 4.7 Fast", 500_000u64, 128_000u64, true, true),
        ("grok-4.6", "Grok 4.6", 500_000u64, 65_536u64, true, true),
        ("grok-4.5", "Grok 4.5", 500_000u64, 65_536u64, true, true),
        ("grok-build-0.1", "Grok Build 0.1", 256_000u64, 65_536u64, false, true),
    ];
    Ok(models
        .iter()
        .map(|(id, name, ctx, out, r, v)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "contextWindow": ctx,
                "maxOutputTokens": out,
                "supportsReasoning": r,
                "supportsVision": v,
                "supportsTools": true,
                "supportsAudio": false,
                "supportsFiles": true
            })
        })
        .collect())
}

pub async fn fetch_zcode_models(_client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let models = [
        ("glm-5.3", "GLM-5.3", 200_000u64, 64_000u64, true, false),
        ("glm-5.3-flash", "GLM-5.3 Flash", 200_000u64, 64_000u64, true, true),
        ("glm-5.1", "GLM-5.1", 200_000u64, 64_000u64, true, false),
        ("glm-5-turbo", "GLM-5 Turbo", 200_000u64, 64_000u64, true, false),
        ("glm-4.7", "GLM-4.7", 200_000u64, 128_000u64, true, false),
    ];
    Ok(models
        .iter()
        .map(|(id, name, ctx, out, r, v)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "contextWindow": ctx,
                "maxOutputTokens": out,
                "supportsReasoning": r,
                "supportsVision": v,
                "supportsTools": true,
                "supportsAudio": false,
                "supportsFiles": true
            })
        })
        .collect())
}

pub async fn fetch_perplexity_models(_client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let models = [
        ("sonar-pro", "Sonar Pro", 200_000u64, 16_384u64, false),
        ("sonar-reasoning-pro", "Sonar Reasoning Pro", 200_000u64, 16_384u64, true),
        ("sonar-deep-research", "Sonar Deep Research", 200_000u64, 16_384u64, true),
        ("sonar", "Sonar", 128_000u64, 8_192u64, false),
    ];
    Ok(models
        .iter()
        .map(|(id, name, ctx, out, r)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "contextWindow": ctx,
                "maxOutputTokens": out,
                "supportsReasoning": r,
                "supportsVision": true,
                "supportsTools": false,
                "supportsAudio": false,
                "supportsFiles": false
            })
        })
        .collect())
}
