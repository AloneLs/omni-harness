use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static FFMPEG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

fn candidate_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        out.push(PathBuf::from(&local).join("Microsoft/WinGet/Links/ffmpeg.exe"));
    }
    if let Ok(pf) = std::env::var("ProgramFiles") {
        out.push(PathBuf::from(&pf).join("ffmpeg/bin/ffmpeg.exe"));
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        out.push(PathBuf::from(&pf86).join("ffmpeg/bin/ffmpeg.exe"));
    }
    if let Ok(choco) = std::env::var("ChocolateyInstall") {
        out.push(PathBuf::from(&choco).join("bin/ffmpeg.exe"));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join("ffmpeg.exe");
            if cand.is_file() {
                out.push(cand);
            }
        }
    }
    out
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    FFMPEG_PATH
        .get_or_init(|| {
            for cand in candidate_paths() {
                if cand.is_file() {
                    return Some(cand);
                }
            }
            None
        })
        .clone()
}

pub fn is_wav_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("wav"))
        .unwrap_or(false)
}

pub fn decode_to_wav_16k_mono(input: &Path) -> Result<Vec<u8>, String> {
    if !input.is_file() {
        return Err(format!("Audio file not found: {}", input.to_string_lossy()));
    }
    if is_wav_file(input) {
        if let Ok(bytes) = std::fs::read(input) {
            if crate::speech::SpeechEngine::parse_wav_samples(&bytes).is_ok() {
                return Ok(bytes);
            }
        }
    }
    let ffmpeg = find_ffmpeg().ok_or_else(|| {
        "ffmpeg not found. Install ffmpeg to convert audio formats, or send WAV audio.".to_string()
    })?;
    let out_path = std::env::temp_dir().join(format!("omni_conv_{}.wav", uuid::Uuid::new_v4()));
    let mut cmd = std::process::Command::new(&ffmpeg);
    cmd.creation_flags_no_window();
    cmd.arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-f")
        .arg("wav")
        .arg(&out_path);
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to launch ffmpeg: {}", e))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&out_path);
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "Audio conversion failed: {}",
            if err.is_empty() { "ffmpeg exited with error".to_string() } else { err }
        ));
    }
    let bytes = std::fs::read(&out_path)
        .map_err(|e| format!("Failed to read converted audio: {}", e))?;
    let _ = std::fs::remove_file(&out_path);
    if bytes.len() <= 44 {
        return Err("Audio conversion produced an empty stream".to_string());
    }
    Ok(bytes)
}

pub fn convert_to_file(input: &Path, output: &Path) -> Result<PathBuf, String> {
    let bytes = decode_to_wav_16k_mono(input)?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(output, &bytes).map_err(|e| e.to_string())?;
    Ok(output.to_path_buf())
}

pub fn audio_format_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "wav".to_string())
}

pub fn is_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "wav" | "mp3" | "ogg" | "opus" | "m4a" | "aac" | "flac" | "webm" | "amr" | "wma"
            )
        })
        .unwrap_or(false)
}

pub fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"))
        .unwrap_or(false)
}

pub fn image_mime_of(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "image/png",
    }
}

trait NoWindowExt {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

impl NoWindowExt for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x08000000);
        }
        self
    }
}