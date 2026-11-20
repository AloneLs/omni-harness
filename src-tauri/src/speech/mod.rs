use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
        }
    }

    fn is_valid_weight_file(path: &Path) -> bool {
        std::fs::metadata(path).map(|m| m.is_file() && m.len() > 1_000_000).unwrap_or(false)
    }

    pub fn get_available_models(&self) -> Vec<WhisperModelInfo> {
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
                WhisperModelInfo {
                    model_id: (*id).to_string(),
                    filename: (*fname).to_string(),
                    download_url: (*url).to_string(),
                    size_bytes: *sz,
                    is_downloaded: ok,
                    local_path: if ok { Some(p.to_string_lossy().to_string()) } else { None },
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

        self.is_downloading.store(true, Ordering::SeqCst);
        let client = reqwest::Client::new();

        let resp = match client.get(&target.download_url).send().await {
            Ok(r) => r,
            Err(e) => {
                self.is_downloading.store(false, Ordering::SeqCst);
                return Err(format!("Download failed: {}", e));
            }
        };

        if !resp.status().is_success() {
            self.is_downloading.store(false, Ordering::SeqCst);
            return Err(format!("HTTP error downloading model: {}", resp.status()));
        }

        let total_size = resp.content_length().unwrap_or(target.size_bytes);
        let part_path = self.models_dir.join(format!("{}.part", target.filename));

        let mut file = match tokio::fs::File::create(&part_path).await {
            Ok(f) => f,
            Err(e) => {
                self.is_downloading.store(false, Ordering::SeqCst);
                return Err(format!("Failed to create model file: {}", e));
            }
        };

        let mut stream = resp.bytes_stream();
        let mut downloaded = 0u64;

        while let Some(chunk_res) = stream.next().await {
            match chunk_res {
                Ok(chunk) => {
                    if let Err(e) = file.write_all(&chunk).await {
                        self.is_downloading.store(false, Ordering::SeqCst);
                        return Err(format!("Failed to write chunk: {}", e));
                    }
                    downloaded += chunk.len() as u64;
                    if let Some(tx) = &progress_tx {
                        let _ = tx.send((downloaded, total_size)).await;
                    }
                }
                Err(e) => {
                    self.is_downloading.store(false, Ordering::SeqCst);
                    return Err(format!("Stream error during model download: {}", e));
                }
            }
        }

        let _ = file.flush().await;
        drop(file);

        if let Err(e) = tokio::fs::rename(&part_path, &dest_path).await {
            self.is_downloading.store(false, Ordering::SeqCst);
            return Err(format!("Failed to finalize model file: {}", e));
        }

        self.is_downloading.store(false, Ordering::SeqCst);
        Ok(dest_path)
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

                let accel = *self.acceleration.read().await;
                if accel == AccelerationMode::Cpu {
                    std_cmd.arg("-t").arg("4");
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
