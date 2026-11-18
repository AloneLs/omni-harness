use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;
use tauri::Emitter;

const GITHUB_REPO: &str = "wasrecordrecorder/omni-harness";

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
            if !dl.is_empty() && (low.ends_with(".zip") || low.ends_with(".exe")) {
                chosen_name = Some(name.to_string());
                chosen_url = Some(dl.to_string());
                if low.contains("win64") || low.contains("x64") {
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

    let bytes = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();
    let _ = app.emit(
        "update-download-progress",
        serde_json::json!({ "status": "extracting", "percent": 75 }),
    );

    let stage_dir = std::env::temp_dir().join(format!("omni_update_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&stage_dir).map_err(|e| e.to_string())?;

    let low = asset_name.to_ascii_lowercase();
    let mut new_exe: Option<PathBuf> = None;

    if low.ends_with(".zip") {
        let reader = std::io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("Invalid zip archive: {}", e))?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
            if file.is_dir() {
                continue;
            }
            let fname = PathBuf::from(file.name())
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if fname.is_empty() {
                continue;
            }
            let out_p = stage_dir.join(&fname);
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            std::fs::write(&out_p, &buf).map_err(|e| e.to_string())?;
            if fname.eq_ignore_ascii_case("omni-harness.exe") {
                new_exe = Some(out_p);
            }
        }
    } else if low.ends_with(".exe") {
        let out_p = stage_dir.join("omni-harness.exe");
        std::fs::write(&out_p, &bytes).map_err(|e| e.to_string())?;
        new_exe = Some(out_p);
    }

    let staged_exe = new_exe.ok_or("Update archive does not contain omni-harness.exe")?;
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
        let script_body = format!(
            "$pidToWait = {pid}\n\
            for ($i = 0; $i -lt 60; $i++) {{\n\
              if (-not (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue)) {{ break }}\n\
              Start-Sleep -Milliseconds 250\n\
            }}\n\
            Copy-Item -Path \"{staged}\" -Destination \"{target_exe}\" -Force\n\
            $dll = Join-Path \"{stage_dir}\" \"WebView2Loader.dll\"\n\
            if (Test-Path $dll) {{ Copy-Item -Path $dll -Destination (Join-Path \"{target_dir}\" \"WebView2Loader.dll\") -Force }}\n\
            Start-Process -FilePath \"{target_exe}\"\n",
            pid = pid,
            staged = staged_exe.to_string_lossy().replace('"', "`\""),
            target_exe = current_exe.to_string_lossy().replace('"', "`\""),
            stage_dir = stage_dir.to_string_lossy().replace('"', "`\""),
            target_dir = target_dir.to_string_lossy().replace('"', "`\""),
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
