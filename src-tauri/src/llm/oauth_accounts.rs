use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::Emitter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static ANTIGRAVITY_ID: OnceLock<String> = OnceLock::new();
static ANTIGRAVITY_SECRET: OnceLock<String> = OnceLock::new();

pub fn get_antigravity_client_id() -> &'static str {
    ANTIGRAVITY_ID.get_or_init(|| {
        const B: [u8; 73] = [
            107, 106, 109, 107, 106, 106, 108, 106, 108, 106, 111, 99, 107, 119, 46, 55, 50, 41,
            41, 51, 52, 104, 50, 104, 107, 54, 57, 40, 63, 104, 105, 111, 44, 46, 53, 54, 53, 48,
            50, 110, 61, 110, 106, 105, 63, 42, 116, 59, 42, 42, 41, 116, 61, 53, 53, 61, 54, 63,
            47, 41, 63, 40, 57, 53, 52, 46, 63, 52, 46, 116, 57, 53, 55,
        ];
        B.iter().map(|b| (b ^ 0x5A) as char).collect()
    })
}

pub fn get_antigravity_client_secret() -> &'static str {
    ANTIGRAVITY_SECRET.get_or_init(|| {
        const B: [u8; 35] = [
            29, 21, 25, 9, 10, 2, 119, 17, 111, 98, 28, 13, 8, 110, 98, 108, 22, 62, 22, 16, 107,
            55, 22, 24, 98, 41, 2, 25, 110, 32, 108, 43, 30, 27, 60,
        ];
        B.iter().map(|b| (b ^ 0x5A) as char).collect()
    })
}

pub const ANTIGRAVITY_REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";

pub const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const OPENAI_CODEX_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";

pub const CLAUDE_OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const CLAUDE_OAUTH_REDIRECT_URI: &str = "http://localhost:54545/callback";

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
        }
    }
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

fn extract_chatgpt_claims(id_token: Option<&str>, access_token: &str) -> (Option<String>, Option<String>) {
    for tok in [id_token, Some(access_token)].into_iter().flatten() {
        if let Some(val) = decode_jwt_payload(tok) {
            let email = val.get("email").and_then(|v| v.as_str()).map(|s| s.to_string());
            let auth_obj = val.get("https://api.openai.com/auth");
            let account_id = val
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
            if account_id.is_some() || email.is_some() {
                return (email, account_id);
            }
        }
    }
    (None, None)
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
    read_accounts_unlocked()
        .iter()
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

async fn refresh_account_tokens(client: &reqwest::Client, acc: &mut OAuthAccount) -> Result<(), String> {
    if acc.refresh_token.trim().is_empty() {
        return Err("Missing refresh_token".to_string());
    }
    let now = chrono::Utc::now().timestamp_millis();
    match acc.provider.as_str() {
        "antigravity" => {
            let params = [
                ("client_id", get_antigravity_client_id()),
                ("client_secret", get_antigravity_client_secret()),
                ("grant_type", "refresh_token"),
                ("refresh_token", acc.refresh_token.as_str()),
            ];
            let resp = client
                .post("https://oauth2.googleapis.com/token")
                .form(&params)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                let status = resp.status();
                let txt = resp.text().await.unwrap_or_default();
                return Err(format!("Google refresh failed ({}): {}", status, txt));
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
        other => Err(format!("Unsupported OAuth provider: {}", other)),
    }
}

pub async fn acquire_valid_account(
    client: &reqwest::Client,
    provider: &str,
    excluded_ids: &std::collections::HashSet<String>,
) -> Result<OAuthAccount, String> {
    let _guard = store_lock().lock().await;
    let mut list = read_accounts_unlocked();
    let now = chrono::Utc::now().timestamp_millis();

    let mut candidates: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            a.provider == provider
                && a.enabled
                && !excluded_ids.contains(&a.id)
                && a.cooldown_until_ms.map(|c| c <= now).unwrap_or(true)
        })
        .map(|(i, _)| i)
        .collect();

    if candidates.is_empty() {
        let any_enabled = list.iter().any(|a| a.provider == provider && a.enabled && !excluded_ids.contains(&a.id));
        if any_enabled {
            candidates = list
                .iter()
                .enumerate()
                .filter(|(_, a)| a.provider == provider && a.enabled && !excluded_ids.contains(&a.id))
                .map(|(i, _)| i)
                .collect();
        }
    }

    if candidates.is_empty() {
        return Err(format!("No active OAuth accounts configured for '{}'. Connect an account in Settings -> Models & Providers.", provider));
    }

    candidates.sort_by_key(|&idx| list[idx].last_used_ms);
    let chosen_idx = candidates[0];
    let mut chosen = list[chosen_idx].clone();

    if chosen.access_token.is_empty() || chosen.expires_at_ms <= now {
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
                ("client_id", get_antigravity_client_id()),
                ("client_secret", get_antigravity_client_secret()),
                ("code", code),
                ("grant_type", "authorization_code"),
                ("redirect_uri", flow.redirect_uri.as_str()),
                ("code_verifier", flow.verifier.as_str()),
            ];
            let resp = client
                .post("https://oauth2.googleapis.com/token")
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
                .get("https://www.googleapis.com/oauth2/v1/userinfo?alt=json")
                .bearer_auth(&at)
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

            let project_id = fetch_antigravity_project_id(client, &at).await;
            let acc = OAuthAccount {
                id: format!("ag-{}", uuid::Uuid::new_v4().simple()),
                provider: "antigravity".to_string(),
                email,
                access_token: at,
                refresh_token: rt,
                expires_at_ms: now + exp_in * 1000 - 60_000,
                project_id: Some(project_id),
                account_id: None,
                enabled: true,
                cooldown_until_ms: None,
                last_error: None,
                last_used_ms: 0,
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
            let (email_opt, account_id_opt) = extract_chatgpt_claims(id_tok, &at);

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
            };
            upsert_account(acc).await
        }
        other => Err(format!("Unsupported OAuth provider: {}", other)),
    }
}

pub fn antigravity_hub_ua() -> &'static str {
    "antigravity/hub/2.31.2 (aidev_client; os_type=windows; arch=amd64; cl=1019359392)"
}

pub fn machine_session_id() -> &'static str {
    static SID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SID.get_or_init(|| format!("{}-{}", uuid::Uuid::new_v4(), chrono::Utc::now().timestamp_millis()))
}

pub async fn fetch_antigravity_project_id(client: &reqwest::Client, access_token: &str) -> String {
    let meta = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY"
        }
    });
    for ep in [
        "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
        "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    ] {
        if let Ok(resp) = client
            .post(ep)
            .bearer_auth(access_token)
            .header("User-Agent", antigravity_hub_ua())
            .header("X-Machine-Session-Id", machine_session_id())
            .json(&meta)
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(val) = resp.json::<serde_json::Value>().await {
                    if let Some(pid) = val.get("cloudaicompanionProject").and_then(|v| v.as_str()) {
                        if !pid.trim().is_empty() {
                            return pid.trim().to_string();
                        }
                    }
                }
            }
        }
    }
    String::new()
}

pub async fn fetch_antigravity_models(client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let excluded = std::collections::HashSet::new();
    let acc = acquire_valid_account(client, "antigravity", &excluded).await?;
    let pid = match acc.project_id.clone() {
        Some(p) if !p.is_empty() => p,
        _ => fetch_antigravity_project_id(client, &acc.access_token).await,
    };
    let ua = "antigravity/hub/2.31.2 (aidev_client; os_type=windows; arch=amd64; cl=1019359392)";
    let resp = client
        .post("https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels")
        .bearer_auth(&acc.access_token)
        .header("User-Agent", ua)
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
            let is_legacy_gemini = id_low.starts_with("gemini-2.5")
                || id_low.starts_with("gemini-3-")
                || id_low.starts_with("gemini-3.0")
                || id_low.starts_with("gemini-3.1")
                || id_low.starts_with("gemini-3.5");
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
            out.push(serde_json::json!({
                "id": id,
                "name": display_name,
                "contextWindow": info.get("maxInputTokens").and_then(|v| v.as_u64()).unwrap_or(200000),
                "maxOutputTokens": info.get("maxOutputTokens").and_then(|v| v.as_u64()).unwrap_or(32768),
                "supportsReasoning": supports_reasoning,
                "supportsVision": info.get("supportsImages").and_then(|v| v.as_bool()).unwrap_or(true),
                "supportsTools": true,
                "supportsAudio": lower.contains("gemini"),
                "supportsFiles": true,
                "remainingFraction": remaining,
                "isExhausted": is_exhausted,
                "resetTime": reset_time
            }));
        }
    }
    Ok(out)
}

pub async fn start_oauth_login(
    app_handle: tauri::AppHandle,
    http_client: reqwest::Client,
    provider: &str,
) -> Result<serde_json::Value, String> {
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
                urlencoding::encode(get_antigravity_client_id()),
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

    open_url_in_browser(&auth_url);

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

    let (code, st) = if raw.starts_with("http://") || raw.starts_with("https://") {
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
    if let Ok(entry) = keyring::Entry::new("gemini", "antigravity") {
        if let Ok(secret) = entry.get_password() {
            if !secret.trim().is_empty() {
                ag_raw_blobs.push(secret);
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
                    acc.project_id = Some(fetch_antigravity_project_id(&http_client, &acc.access_token).await);
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
                    let (email_opt, account_id_opt) = extract_chatgpt_claims(id_tok, &at);
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
                        for m in arr {
                            let id = m.get("slug").or_else(|| m.get("id")).and_then(|v| v.as_str()).unwrap_or("").trim();
                            if id.is_empty() {
                                continue;
                            }
                            let name = m.get("display_name").or_else(|| m.get("name")).and_then(|v| v.as_str()).unwrap_or(id);
                            let ctx = m.get("context_window").or_else(|| m.get("max_context_length")).and_then(|v| v.as_u64()).unwrap_or(400000);
                            out.push(serde_json::json!({
                                "id": id,
                                "name": name,
                                "contextWindow": ctx,
                                "maxOutputTokens": 65536,
                                "supportsReasoning": true,
                                "supportsVision": true,
                                "supportsTools": true,
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
        ("gpt-6.1-sol", "GPT-6.1 Sol", 524288u64),
        ("gpt-6-astra", "GPT-6 Astra", 524288u64),
        ("gpt-6-sol", "GPT-6 Sol", 524288u64),
        ("gpt-6.1-terra", "GPT-6.1 Terra", 400000u64),
        ("gpt-6-luna", "GPT-6 Luna", 272000u64),
        ("gpt-6-chat-thinking", "GPT-6 Chat Thinking", 272000u64),
    ];
    Ok(fallback
        .iter()
        .map(|(id, name, ctx)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "contextWindow": ctx,
                "maxOutputTokens": 65536,
                "supportsReasoning": true,
                "supportsVision": true,
                "supportsTools": true,
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
                            out.push(serde_json::json!({
                                "id": id,
                                "name": name,
                                "contextWindow": 200000,
                                "maxOutputTokens": 64000,
                                "supportsReasoning": true,
                                "supportsVision": true,
                                "supportsTools": true,
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
            serde_json::json!({
                "id": id,
                "name": name,
                "contextWindow": 200000,
                "maxOutputTokens": 64000,
                "supportsReasoning": true,
                "supportsVision": true,
                "supportsTools": true,
                "supportsAudio": false,
                "supportsFiles": true
            })
        })
        .collect())
}
