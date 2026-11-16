use std::path::PathBuf;

fn find_webview2_dll(dir: &std::path::Path, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.eq_ignore_ascii_case("WebView2Loader.dll") {
                    return Some(path);
                }
            }
        } else if path.is_dir() {
            if let Some(found) = find_webview2_dll(&path, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=tauri.conf.json");

    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let out_path = PathBuf::from(&out_dir);
        let dest = out_path.join("WebView2Loader.dll");
        let mut copied = false;
        if let Some(target_profile_dir) = out_path.ancestors().nth(3) {
            if let Some(dll) = find_webview2_dll(target_profile_dir, 4) {
                if std::fs::copy(&dll, &dest).is_ok() {
                    copied = true;
                }
                let profile_dest = target_profile_dir.join("WebView2Loader.dll");
                let _ = std::fs::copy(&dll, &profile_dest);
            }
        }
        if !copied && !dest.exists() {
            let _ = std::fs::write(&dest, b"");
        }
    }

    tauri_build::build()
}
