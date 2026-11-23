use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

pub mod convert;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelInfo {
    pub model_id: String,
    pub filename: String,
    pub download_url: String,
    pub size_bytes: u64,
    pub is_downloaded: bool,
    pub local_path: Option<String>,
    pub is_downloading: bool,
    pub download_progress: Option<u32>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWhisperDownload {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    pub text: String,
    pub language: String,
    pub duration_ms: u64,
    pub segments: Vec<TranscriptionSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccelerationMode {
    Cuda,
    Vulkan,
    Cpu,
}

pub struct VadProcessor {
    sample_rate: u32,
    frame_size: usize,
    energy_threshold: f32,
}

impl VadProcessor {
    pub fn new(sample_rate: u32, energy_threshold: f32) -> Self {
        let frame_size = (sample_rate as usize * 20) / 1000;
        Self {
            sample_rate,
            frame_size,
            energy_threshold,
        }
    }

    pub fn filter_speech(&self, samples: &[f32]) -> Vec<f32> {
        let mut speech_samples = Vec::with_capacity(samples.len());
        for chunk in samples.chunks(self.frame_size) {
            if chunk.len() < self.frame_size {
                continue;
            }
            let sum_sq: f32 = chunk.iter().map(|&s| s * s).sum();
            let rms = (sum_sq / chunk.len() as f32).sqrt();
            if rms >= self.energy_threshold {
                speech_samples.extend_from_slice(chunk);
            }
        }
        speech_samples
    }

    pub fn detect_segments(&self, samples: &[f32]) -> Vec<(u64, u64)> {
        let mut segments = Vec::new();
        let mut in_speech = false;
        let mut start_idx = 0usize;

        for (i, chunk) in samples.chunks(self.frame_size).enumerate() {
            let sum_sq: f32 = chunk.iter().map(|&s| s * s).sum();
            let rms = (sum_sq / chunk.len() as f32).sqrt();
            let is_voice = rms >= self.energy_threshold;

            if is_voice && !in_speech {
                in_speech = true;
                start_idx = i * self.frame_size;
            } else if !is_voice && in_speech {
                in_speech = false;
                let end_idx = i * self.frame_size;
                let start_ms = (start_idx as u64 * 1000) / self.sample_rate as u64;
                let end_ms = (end_idx as u64 * 1000) / self.sample_rate as u64;
                if end_ms > start_ms + 100 {
                    segments.push((start_ms, end_ms));
                }
            }
        }

        if in_speech {
            let start_ms = (start_idx as u64 * 1000) / self.sample_rate as u64;
            let end_ms = (samples.len() as u64 * 1000) / self.sample_rate as u64;
            segments.push((start_ms, end_ms));
        }

        segments
    }
}

pub struct SpeechEngine {
    models_dir: PathBuf,
    #[allow(dead_code)]
    current_model: Arc<RwLock<String>>,
    acceleration: Arc<RwLock<AccelerationMode>>,
    is_downloading: Arc<AtomicBool>,
    active_download: Arc<std::sync::RwLock<Option<ActiveWhisperDownload>>>,
}

impl SpeechEngine {
    pub fn new(data_dir: &Path) -> Self {
        let models_dir = data_dir.join("models").join("whisper");
        let _ = std::fs::create_dir_all(&models_dir);

        let acceleration = if std::env::var("CUDA_PATH").is_ok() {
            AccelerationMode::Cuda
        } else {
            AccelerationMode::Cpu
        };

        Self {
            models_dir,
            current_model: Arc::new(RwLock::new("large-v3-turbo".to_string())),
            acceleration: Arc::new(RwLock::new(acceleration)),
            is_downloading: Arc::new(AtomicBool::new(false)),
            active_download: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    fn is_valid_weight_file(path: &Path) -> bool {
        std::fs::metadata(path).map(|m| m.is_file() && m.len() > 1_000_000).unwrap_or(false)
    }

    pub fn get_available_models(&self) -> Vec<WhisperModelInfo> {
        let active = self.active_download.read().ok().and_then(|g| g.clone());
        let defs = [
            ("large-v3-turbo", "ggml-large-v3-turbo.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin", 1_620_000_000u64),
            ("large-v3", "ggml-large-v3.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin", 3_100_000_000u64),
            ("medium", "ggml-medium.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin", 1_530_000_000u64),
            ("base", "ggml-base.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin", 148_000_000u64),
        ];
        defs.iter()
            .map(|(id, fname, url, sz)| {
                let p = self.models_dir.join(fname);
                let ok = Self::is_valid_weight_file(&p);
                let is_active_dl = active.as_ref().map(|a| a.model_id == *id).unwrap_or(false);
                let part_path = self.models_dir.join(format!("{}.part", fname));
                let part_len = std::fs::metadata(&part_path).map(|m| m.len()).ok();
                
                let (dl_bytes, total_bytes, pct) = if let Some(ref a) = active {
                    if a.model_id == *id {
                        (Some(a.downloaded_bytes), Some(a.total_bytes), Some(a.percent))
                    } else {
                        (None, None, None)
                    }
                } else if self.is_downloading.load(Ordering::SeqCst) && part_len.is_some() {
                    let dl = part_len.unwrap_or(0);
                    let p = if *sz > 0 { ((dl as f64 / *sz as f64) * 100.0).round() as u32 } else { 0 };
                    (Some(dl), Some(*sz), Some(p.min(100)))
                } else {
                    (None, None, None)
                };

                WhisperModelInfo {
                    model_id: (*id).to_string(),
                    filename: (*fname).to_string(),
                    download_url: (*url).to_string(),
                    size_bytes: *sz,
                    is_downloaded: ok,
                    local_path: if ok { Some(p.to_string_lossy().to_string()) } else { None },
                    is_downloading: is_active_dl || (self.is_downloading.load(Ordering::SeqCst) && part_len.is_some()),
                    download_progress: pct,
                    downloaded_bytes: dl_bytes,
                    total_bytes,
                }
            })
            .collect()
    }

    pub fn delete_model(&self, model_id: &str) -> Result<(), String> {
        let models = self.get_available_models();
        let target = models
            .into_iter()
            .find(|m| m.model_id == model_id)
            .ok_or_else(|| format!("Unknown model ID: {}", model_id))?;
        let dest = self.models_dir.join(&target.filename);
        let part = self.models_dir.join(format!("{}.part", target.filename));
        if dest.exists() {
            std::fs::remove_file(&dest).map_err(|e| e.to_string())?;
        }
        if part.exists() {
            let _ = std::fs::remove_file(&part);
        }
        Ok(())
    }

    pub async fn ensure_whisper_cli(&self) -> Result<PathBuf, String> {
        let whisper_exe = self.models_dir.join("whisper.exe");
        if whisper_exe.is_file() {
            return Ok(whisper_exe);
        }

        let zip_url = "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-x64.zip";
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .map_err(|e| format!("Failed to create client: {}", e))?;

        let resp = client
            .get(zip_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download whisper CLI binaries: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Whisper CLI download returned HTTP {}", resp.status()));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Failed to read whisper CLI bytes: {}", e))?;

        let reader = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|e| format!("Failed to open whisper binary zip: {}", e))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = match file.enclosed_name() {
                Some(p) => p.file_name().map(|f| f.to_os_string()),
                None => continue,
            };
            if let Some(file_name) = name {
                let out_path = self.models_dir.join(file_name);
                let mut out_file = std::fs::File::create(&out_path)
                    .map_err(|e| format!("Failed to create file {:?}: {}", out_path, e))?;
                std::io::copy(&mut file, &mut out_file)
                    .map_err(|e| format!("Failed to extract file {:?}: {}", out_path, e))?;
            }
        }

        let cli_exe = self.models_dir.join("whisper-cli.exe");
        let main_exe = self.models_dir.join("main.exe");
        if cli_exe.is_file() {
            let _ = std::fs::copy(&cli_exe, &whisper_exe);
        } else if main_exe.is_file() {
            let _ = std::fs::copy(&main_exe, &whisper_exe);
        }

        if whisper_exe.is_file() {
            Ok(whisper_exe)
        } else {
            Err("whisper.exe was not found in extracted archive".to_string())
        }
    }

    pub async fn download_model(
        &self,
        model_id: &str,
        progress_tx: Option<tokio::sync::mpsc::Sender<(u64, u64)>>,
    ) -> Result<PathBuf, String> {
        if self.is_downloading.load(Ordering::SeqCst) {
            return Err("A model download is already in progress".to_string());
        }

        let models = self.get_available_models();
        let target = models
            .into_iter()
            .find(|m| m.model_id == model_id)
            .ok_or_else(|| format!("Unknown model ID: {}", model_id))?;

        let dest_path = self.models_dir.join(&target.filename);
        if Self::is_valid_weight_file(&dest_path) {
            return Ok(dest_path);
        }

        let part_path = self.models_dir.join(format!("{}.part", target.filename));

        // Check if existing .part file is already fully downloaded
        if let Ok(meta) = std::fs::metadata(&part_path) {
            if meta.len() >= target.size_bytes.saturating_sub(1024 * 1024) && meta.len() > 10_000_000 {
                let _ = tokio::fs::rename(&part_path, &dest_path).await;
                if Self::is_valid_weight_file(&dest_path) {
                    return Ok(dest_path);
                }
            }
        }

        self.is_downloading.store(true, Ordering::SeqCst);
        let total_hint = target.size_bytes;
        if let Ok(mut g) = self.active_download.write() {
            *g = Some(ActiveWhisperDownload {
                model_id: model_id.to_string(),
                downloaded_bytes: std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0),
                total_bytes: total_hint,
                percent: 0,
            });
        }

        struct DownloadGuard {
            is_dl: Arc<AtomicBool>,
            active: Arc<std::sync::RwLock<Option<ActiveWhisperDownload>>>,
        }
        impl Drop for DownloadGuard {
            fn drop(&mut self) {
                self.is_dl.store(false, Ordering::SeqCst);
                if let Ok(mut g) = self.active.write() {
                    *g = None;
                }
            }
        }
        let _guard = DownloadGuard {
            is_dl: self.is_downloading.clone(),
            active: self.active_download.clone(),
        };

        let mut builder = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
            .connect_timeout(Duration::from_secs(25))
            .tcp_keepalive(Duration::from_secs(15));
        if let Some(sys_proxy) = crate::resolve_windows_system_proxy() {
            if let Ok(px) = reqwest::Proxy::all(&sys_proxy) {
                builder = builder.proxy(px);
            }
        }
        let client = builder.build().unwrap_or_else(|_| reqwest::Client::new());

        let primary_url = target.download_url.clone();
        let mirror_url = if primary_url.contains("huggingface.co") {
            primary_url.replace("https://huggingface.co/", "https://hf-mirror.com/")
        } else {
            primary_url.clone()
        };

        let max_retries = 8usize;
        let mut last_error = String::new();

        for attempt in 0..max_retries {
            let active_url = if attempt >= 2 && mirror_url != primary_url {
                &mirror_url
            } else {
                &primary_url
            };

            let mut existing_bytes = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
            if existing_bytes >= target.size_bytes && target.size_bytes > 0 {
                let _ = tokio::fs::rename(&part_path, &dest_path).await;
                if Self::is_valid_weight_file(&dest_path) {
                    return Ok(dest_path);
                }
            }

            let mut req = client.get(active_url);
            if existing_bytes > 0 {
                req = req.header("Range", format!("bytes={}-", existing_bytes));
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    last_error = format!("Network connect error (attempt {}): {}", attempt + 1, e);
                    let backoff = (attempt as u64 + 1) * 1500;
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                    continue;
                }
            };

            let status = resp.status();
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                if existing_bytes >= target.size_bytes.saturating_sub(1024 * 1024) && existing_bytes > 10_000_000 {
                    let _ = tokio::fs::rename(&part_path, &dest_path).await;
                    if Self::is_valid_weight_file(&dest_path) {
                        return Ok(dest_path);
                    }
                }
                let _ = tokio::fs::remove_file(&part_path).await;
                existing_bytes = 0;
                continue;
            }

            if !status.is_success() {
                last_error = format!("HTTP {} from {} (attempt {})", status, active_url, attempt + 1);
                let backoff = (attempt as u64 + 1) * 2000;
                tokio::time::sleep(Duration::from_millis(backoff)).await;
                continue;
            }

            let is_partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
            let total_size = if is_partial {
                resp.content_length().map(|cl| cl + existing_bytes).unwrap_or(target.size_bytes)
            } else {
                resp.content_length().unwrap_or(target.size_bytes)
            };

            let mut file = match if is_partial && existing_bytes > 0 {
                tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&part_path)
                    .await
            } else {
                existing_bytes = 0;
                tokio::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&part_path)
                    .await
            } {
                Ok(f) => f,
                Err(e) => {
                    last_error = format!("Failed to open part file: {}", e);
                    let backoff = (attempt as u64 + 1) * 1500;
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                    continue;
                }
            };

            let mut stream = resp.bytes_stream();
            let mut downloaded = existing_bytes;
            let mut stream_interrupted = false;

            loop {
                let chunk_step = tokio::time::timeout(Duration::from_secs(30), stream.next()).await;
                match chunk_step {
                    Ok(Some(Ok(chunk))) => {
                        if let Err(e) = file.write_all(&chunk).await {
                            last_error = format!("Failed to write chunk: {}", e);
                            stream_interrupted = true;
                            break;
                        }
                        downloaded += chunk.len() as u64;
                        let pct = if total_size > 0 {
                            ((downloaded as f64 / total_size as f64) * 100.0).round() as u32
                        } else {
                            0
                        };
                        if let Ok(mut g) = self.active_download.write() {
                            if let Some(ref mut ad) = *g {
                                ad.downloaded_bytes = downloaded;
                                ad.total_bytes = total_size;
                                ad.percent = pct.min(100);
                            }
                        }
                        if let Some(tx) = &progress_tx {
                            let _ = tx.send((downloaded, total_size)).await;
                        }
                    }
                    Ok(Some(Err(e))) => {
                        last_error = format!("Stream read error: {}", e);
                        stream_interrupted = true;
                        break;
                    }
                    Ok(None) => {
                        // Stream completed
                        break;
                    }
                    Err(_) => {
                        // Chunk 30s timeout
                        last_error = "Download stream stalled (30s timeout on chunk)".to_string();
                        stream_interrupted = true;
                        break;
                    }
                }
            }

            let _ = file.flush().await;
            drop(file);

            if !stream_interrupted {
                let final_len = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
                if final_len >= target.size_bytes.saturating_sub(1024 * 1024) && final_len > 10_000_000 {
                    if let Err(e) = tokio::fs::rename(&part_path, &dest_path).await {
                        return Err(format!("Failed to finalize model file: {}", e));
                    }
                    return Ok(dest_path);
                } else if total_size > 0 && final_len < total_size {
                    // Incomplete stream without explicit error, loop will resume with Range
                    let backoff = (attempt as u64 + 1) * 1000;
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                    continue;
                } else {
                    let _ = tokio::fs::rename(&part_path, &dest_path).await;
                    return Ok(dest_path);
                }
            } else {
                let backoff = (attempt as u64 + 1) * 2000;
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }

        Err(format!("Whisper model download failed after {} attempts. Last error: {}", max_retries, last_error))
    }

    pub fn parse_wav_samples(wav_bytes: &[u8]) -> Result<(Vec<f32>, u32), String> {
        if wav_bytes.len() < 44 {
            return Err("Invalid WAV file: data too short".to_string());
        }

        if &wav_bytes[0..4] != b"RIFF" || &wav_bytes[8..12] != b"WAVE" {
            return Err("Invalid WAV header".to_string());
        }

        let channels = u16::from_le_bytes([wav_bytes[22], wav_bytes[23]]);
        let sample_rate = u32::from_le_bytes([
            wav_bytes[24],
            wav_bytes[25],
            wav_bytes[26],
            wav_bytes[27],
        ]);
        let bits_per_sample = u16::from_le_bytes([wav_bytes[34], wav_bytes[35]]);

        let mut data_offset = 36usize;
        while data_offset + 8 <= wav_bytes.len() {
            let chunk_id = &wav_bytes[data_offset..data_offset + 4];
            let chunk_size = u32::from_le_bytes([
                wav_bytes[data_offset + 4],
                wav_bytes[data_offset + 5],
                wav_bytes[data_offset + 6],
                wav_bytes[data_offset + 7],
            ]) as usize;

            if chunk_id == b"data" {
                data_offset += 8;
                break;
            }
            data_offset += 8 + chunk_size;
        }

        if data_offset >= wav_bytes.len() {
            return Err("WAV missing data chunk".to_string());
        }

        let raw_data = &wav_bytes[data_offset..];
        let mut samples = Vec::new();

        if bits_per_sample == 16 {
            for chunk in raw_data.chunks_exact(2 * channels as usize) {
                let sample_i16 = i16::from_le_bytes([chunk[0], chunk[1]]);
                samples.push(sample_i16 as f32 / 32768.0);
            }
        } else {
            return Err(format!("Unsupported bits per sample: {}", bits_per_sample));
        }

        Ok((samples, sample_rate))
    }

    fn resolve_model_path(&self, preferred: Option<&str>) -> Option<PathBuf> {
        let models = self.get_available_models();
        if let Some(p) = preferred {
            let raw = p.trim().to_ascii_lowercase();
            if !raw.is_empty() {
                let wanted = raw.strip_prefix("ggml-").unwrap_or(raw.as_str()).to_string();
                if let Some(m) = models.iter().find(|m| {
                    m.is_downloaded
                        && (m.model_id.eq_ignore_ascii_case(&wanted)
                            || m.filename.to_ascii_lowercase().contains(&format!("{}.bin", wanted)))
                }) {
                    return m.local_path.clone().map(PathBuf::from);
                }
            }
        }
        models
            .into_iter()
            .find(|m| m.is_downloaded)
            .and_then(|m| m.local_path)
            .map(PathBuf::from)
    }

    fn resample_linear(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
        if from == to || from == 0 || samples.is_empty() {
            return samples.to_vec();
        }
        let ratio = from as f64 / to as f64;
        let out_len = ((samples.len() as f64) / ratio).floor() as usize;
        let mut out = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = samples.get(idx).copied().unwrap_or(0.0);
            let b = samples.get(idx + 1).copied().unwrap_or(a);
            out.push(a + (b - a) * frac);
        }
        out
    }

    fn encode_wav_16k(samples: &[f32]) -> Vec<u8> {
        let data_len = samples.len() * 2;
        let mut buf = Vec::with_capacity(44 + data_len);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&16000u32.to_le_bytes());
        buf.extend_from_slice(&32000u32.to_le_bytes());
        buf.extend_from_slice(&2u16.to_le_bytes());
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&(data_len as u32).to_le_bytes());
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf
    }

    fn normalize_to_16k(wav_bytes: &[u8]) -> Result<(Vec<f32>, u64), String> {
        let (samples, sample_rate) = Self::parse_wav_samples(wav_bytes)?;
        let duration_ms = if sample_rate == 0 {
            0
        } else {
            (samples.len() as u64 * 1000) / sample_rate as u64
        };
        let pcm = Self::resample_linear(&samples, sample_rate, 16000);
        Ok((pcm, duration_ms))
    }

    fn extract_whisper_text(json_path: &Path, stdout_text: &str) -> String {
        if let Ok(raw) = std::fs::read_to_string(json_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(arr) = val.get("transcription").and_then(|v| v.as_array()) {
                    let mut joined = String::new();
                    for seg in arr {
                        if let Some(t) = seg.get("text").and_then(|v| v.as_str()) {
                            let t = t.trim();
                            if t.is_empty() {
                                continue;
                            }
                            if !joined.is_empty() {
                                joined.push(' ');
                            }
                            joined.push_str(t);
                        }
                    }
                    if !joined.trim().is_empty() {
                        return joined.trim().to_string();
                    }
                }
                if let Some(t) = val.get("text").and_then(|v| v.as_str()) {
                    if !t.trim().is_empty() {
                        return t.trim().to_string();
                    }
                }
            }
        }
        let mut cleaned = String::new();
        for line in stdout_text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let body = if line.starts_with('[') {
                match line.find(']') {
                    Some(pos) => line[pos + 1..].trim(),
                    None => line,
                }
            } else {
                line
            };
            if body.is_empty() {
                continue;
            }
            if !cleaned.is_empty() {
                cleaned.push(' ');
            }
            cleaned.push_str(body);
        }
        cleaned.trim().to_string()
    }

    pub async fn transcribe_file(
        &self,
        path: &Path,
        api_fallback_key: Option<&str>,
        preferred_model: Option<&str>,
    ) -> Result<TranscriptionResult, String> {
        let wav = convert::decode_to_wav_16k_mono(path)?;
        self.transcribe_audio(&wav, api_fallback_key, preferred_model)
            .await
    }

    pub async fn transcribe_audio(
        &self,
        wav_bytes: &[u8],
        api_fallback_key: Option<&str>,
        preferred_model: Option<&str>,
    ) -> Result<TranscriptionResult, String> {
        let (pcm16k, duration_ms) = Self::normalize_to_16k(wav_bytes)?;

        let vad = VadProcessor::new(16000, 0.015);
        let speech_segments = vad.detect_segments(&pcm16k);
        let normalized_wav = Self::encode_wav_16k(&pcm16k);

        let whisper_exe = self.models_dir.join("whisper.exe");
        if let Some(model_path) = self.resolve_model_path(preferred_model) {
            let _ = self.ensure_whisper_cli().await;
            if whisper_exe.exists() {
                let base = std::env::temp_dir().join(format!("whisper_in_{}", uuid::Uuid::new_v4()));
                let temp_wav = base.with_extension("wav");
                std::fs::write(&temp_wav, &normalized_wav)
                    .map_err(|e| format!("Failed to write temp audio: {}", e))?;

                let mut std_cmd = std::process::Command::new(&whisper_exe);
                std_cmd.creation_flags(0x08000000);
                std_cmd.arg("-m").arg(&model_path);
                std_cmd.arg("-f").arg(&temp_wav);
                std_cmd.arg("-oj");
                std_cmd.arg("-of").arg(&base);
                std_cmd.arg("-l").arg("auto");

                let accel = *self.acceleration.read().await;
                if accel == AccelerationMode::Cpu {
                    let threads = std::thread::available_parallelism()
                        .map(|n| n.get().min(8).to_string())
                        .unwrap_or_else(|_| "4".to_string());
                    std_cmd.arg("-t").arg(&threads);
                }

                let output = tokio::process::Command::from(std_cmd)
                    .output()
                    .await
                    .map_err(|e| format!("Whisper CLI execution failed: {}", e))?;

                let json_path = base.with_extension("json");
                let stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
                let text = Self::extract_whisper_text(&json_path, &stdout_text);
                let _ = std::fs::remove_file(&temp_wav);
                let _ = std::fs::remove_file(&json_path);

                if !text.trim().is_empty() {
                    let mut segments = Vec::new();
                    for (start, end) in speech_segments {
                        segments.push(TranscriptionSegment {
                            start_ms: start,
                            end_ms: end,
                            text: text.clone(),
                            confidence: 0.95,
                        });
                    }
                    return Ok(TranscriptionResult {
                        text,
                        language: "auto".to_string(),
                        duration_ms,
                        segments,
                    });
                }
            }
        }

        if let Some(key) = api_fallback_key {
            let client = crate::build_http_client(None, None, None)
                .unwrap_or_else(|_| reqwest::Client::new());
            let form = reqwest::multipart::Form::new()
                .text("model", "whisper-1")
                .part(
                    "file",
                    reqwest::multipart::Part::bytes(normalized_wav.clone())
                        .file_name("audio.wav")
                        .mime_str("audio/wav")
                        .map_err(|e| format!("Mime error: {}", e))?,
                );

            let res = client
                .post("https://api.openai.com/v1/audio/transcriptions")
                .bearer_auth(key)
                .multipart(form)
                .send()
                .await
                .map_err(|e| format!("Whisper API request failed: {}", e))?;

            if res.status().is_success() {
                let val: serde_json::Value = res
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse Whisper response: {}", e))?;
                let text = val["text"].as_str().unwrap_or("").to_string();
                if !text.trim().is_empty() {
                    let mut segments = Vec::new();
                    for (start, end) in speech_segments {
                        segments.push(TranscriptionSegment {
                            start_ms: start,
                            end_ms: end,
                            text: text.clone(),
                            confidence: 0.98,
                        });
                    }
                    return Ok(TranscriptionResult {
                        text,
                        language: "auto".to_string(),
                        duration_ms,
                        segments,
                    });
                }
            }
        }

        let mut segments = Vec::new();
        for (start, end) in speech_segments {
            segments.push(TranscriptionSegment {
                start_ms: start,
                end_ms: end,
                text: "[Audio Voice Activity Detected]".to_string(),
                confidence: 0.85,
            });
        }

        Err("Whisper model is not installed and no transcription API key is configured".to_string())
    }
}
