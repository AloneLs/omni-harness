use futures_util::StreamExt;
use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;
use tauri::Emitter;

const GITHUB_REPO: &str = "wasrecordrecorder/omni-harness";
const ALLOWED_UPDATE_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "github-releases.githubusercontent.com",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub has_update: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_name: String,
    pub release_url: String,
    pub release_notes: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub published_at: String,
}

fn validate_update_url(raw_url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(raw_url).map_err(|e| format!("Invalid update URL: {}", e))?;
    if parsed.scheme() != "https" {
        return Err("Update URL must use HTTPS".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "Update URL missing host".to_string())?
        .to_ascii_lowercase();
    if !ALLOWED_UPDATE_HOSTS.contains(&host.as_str()) {
        return Err(format!("Untrusted update host: {}", host));
    }
    if host == "github.com" && !parsed.path().starts_with(&format!("/{}/", GITHUB_REPO)) {
        return Err("Update URL repository mismatch".to_string());
    }
    Ok(parsed)
}

fn parse_semver(v: &str) -> (u64, u64, u64) {
    let clean = v.trim().trim_start_matches('v').trim_start_matches('V');
    let core = clean.split('-').next().unwrap_or(clean);
    let mut parts = core.split('.').map(|s| s.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    parse_semver(latest) > parse_semver(current)
}

pub async fn check_github_update(client: &reqwest::Client) -> Result<UpdateInfo, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);
    let resp = client
        .get(&url)
        .header("User-Agent", "OmniHarness-Updater/1.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Failed to check updates: {}", e))?;

    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub API error ({}): {}", code, body));
    }

    let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = val
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let latest_clean = tag.trim_start_matches('v').trim_start_matches('V').to_string();
    let current = env!("CARGO_PKG_VERSION").to_string();
    let has_update = !latest_clean.is_empty() && is_newer_version(&latest_clean, &current);

    let mut chosen_name: Option<String> = None;
    let mut chosen_url: Option<String> = None;
    if let Some(assets) = val.get("assets").and_then(|a| a.as_array()) {
        for a in assets {
            let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let dl = a.get("browser_download_url").and_then(|v| v.as_str()).unwrap_or("");
            let low = name.to_ascii_lowercase();
            if !dl.is_empty()
                && (low.ends_with(".zip") || low.ends_with(".exe"))
                && (low.contains("win") || low.contains("x64"))
                && !low.contains("linux")
                && !low.contains("macos")
                && validate_update_url(dl).is_ok()
            {
                chosen_name = Some(name.to_string());
                chosen_url = Some(dl.to_string());
                if low.contains("win64") || (low.contains("win") && low.contains("x64")) {
                    break;
                }
            }
        }
    }

    Ok(UpdateInfo {
        has_update,
        current_version: current,
        latest_version: if latest_clean.is_empty() { tag.clone() } else { latest_clean },
        release_name: val.get("name").and_then(|v| v.as_str()).unwrap_or(&tag).to_string(),
        release_url: val
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://github.com/wasrecordrecorder/omni-harness/releases")
            .to_string(),
        release_notes: val.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        asset_name: chosen_name,
        asset_url: chosen_url,
        published_at: val.get("published_at").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    })
}

pub async fn install_github_update(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    asset_url: &str,
    asset_name: &str,
) -> Result<(), String> {
    validate_update_url(asset_url)?;

    let _ = app.emit(
        "update-download-progress",
        serde_json::json!({ "status": "downloading", "percent": 0 }),
    );

    let resp = client
        .get(asset_url)
        .header("User-Agent", "OmniHarness-Updater/1.0")
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Download failed with HTTP {}", resp.status().as_u16()));
    }
    validate_update_url(resp.url().as_str())?;

    let stage_dir = std::env::temp_dir().join(format!("omni_update_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&stage_dir).map_err(|e| e.to_string())?;

    let total_len = resp.content_length().unwrap_or(0);
    let download_path = stage_dir.join("download.tmp");
    let mut out_file = std::fs::File::create(&download_path).map_err(|e| e.to_string())?;
    let mut downloaded: u64 = 0;
    let mut last_pct: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| format!("Download stream error: {}", e))?;
        out_file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if total_len > 0 {
            let pct = ((downloaded * 70) / total_len).min(70);
            if pct >= last_pct + 5 {
                last_pct = pct;
                let _ = app.emit(
                    "update-download-progress",
                    serde_json::json!({ "status": "downloading", "percent": pct }),
                );
            }
        }
    }
    drop(out_file);

    let _ = app.emit(
        "update-download-progress",
        serde_json::json!({ "status": "extracting", "percent": 75 }),
    );

    let low = asset_name.to_ascii_lowercase();
    let stage_clone = stage_dir.clone();
    let dl_clone = download_path.clone();
    let staged_exe = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
        let mut new_exe: Option<PathBuf> = None;
        if low.ends_with(".zip") {
            let f = std::fs::File::open(&dl_clone).map_err(|e| e.to_string())?;
            let mut archive = zip::ZipArchive::new(f).map_err(|e| format!("Invalid zip archive: {}", e))?;
            for i in 0..archive.len() {
                let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
                if file.is_dir() {
                    continue;
                }
                let enclosed = match file.enclosed_name() {
                    Some(p) => p.to_path_buf(),
                    None => continue,
                };
                let fname = enclosed
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if fname.is_empty() {
                    continue;
                }
                let out_p = stage_clone.join(&fname);
                let mut dest = std::fs::File::create(&out_p).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut dest).map_err(|e| e.to_string())?;
                if fname.eq_ignore_ascii_case("omni-harness.exe") {
                    new_exe = Some(out_p);
                }
            }
            let _ = std::fs::remove_file(&dl_clone);
        } else if low.ends_with(".exe") {
            let out_p = stage_clone.join("omni-harness.exe");
            std::fs::rename(&dl_clone, &out_p).map_err(|e| e.to_string())?;
            new_exe = Some(out_p);
        }
        new_exe.ok_or_else(|| "Update archive does not contain omni-harness.exe".to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target_dir = current_exe.parent().ok_or("Cannot resolve executable directory")?;
    let pid = std::process::id();

    let _ = app.emit(
        "update-download-progress",
        serde_json::json!({ "status": "installing", "percent": 95 }),
    );

    #[cfg(target_os = "windows")]
    {
        let ps_script = stage_dir.join("apply_update.ps1");
        let esc_ps = |s: &str| s.replace('\'', "''");
        let script_body = format!(
            "$pidToWait = {pid}\n\
            for ($i = 0; $i -lt 60; $i++) {{\n\
              if (-not (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue)) {{ break }}\n\
              Start-Sleep -Milliseconds 250\n\
            }}\n\
            Copy-Item -LiteralPath '{staged}' -Destination '{target_exe}' -Force\n\
            $dll = Join-Path '{stage_dir}' 'WebView2Loader.dll'\n\
            if (Test-Path -LiteralPath $dll) {{ Copy-Item -LiteralPath $dll -Destination (Join-Path '{target_dir}' 'WebView2Loader.dll') -Force }}\n\
            Start-Process -FilePath '{target_exe}'\n",
            pid = pid,
            staged = esc_ps(&staged_exe.to_string_lossy()),
            target_exe = esc_ps(&current_exe.to_string_lossy()),
            stage_dir = esc_ps(&stage_dir.to_string_lossy()),
            target_dir = esc_ps(&target_dir.to_string_lossy()),
        );
        std::fs::write(&ps_script, script_body).map_err(|e| e.to_string())?;
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-WindowStyle",
                "Hidden",
                "-File",
                &ps_script.to_string_lossy(),
            ])
            .spawn()
            .map_err(|e| format!("Failed to launch updater script: {}", e))?;
    }

    let _ = app.emit(
        "update-download-progress",
        serde_json::json!({ "status": "restarting", "percent": 100 }),
    );

    app.exit(0);
    Ok(())
}
