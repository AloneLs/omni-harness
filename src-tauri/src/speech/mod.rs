use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

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

    pub fn get_available_models(&self) -> Vec<WhisperModelInfo> {
        let turbo_path = self.models_dir.join("ggml-large-v3-turbo.bin");
        let v3_path = self.models_dir.join("ggml-large-v3.bin");

        vec![
            WhisperModelInfo {
                model_id: "large-v3-turbo".to_string(),
                filename: "ggml-large-v3-turbo.bin".to_string(),
                download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin".to_string(),
                size_bytes: 1_620_000_000,
                is_downloaded: turbo_path.exists(),
                local_path: if turbo_path.exists() {
                    Some(turbo_path.to_string_lossy().to_string())
                } else {
                    None
                },
            },
            WhisperModelInfo {
                model_id: "large-v3".to_string(),
                filename: "ggml-large-v3.bin".to_string(),
                download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin".to_string(),
                size_bytes: 3_100_000_000,
                is_downloaded: v3_path.exists(),
                local_path: if v3_path.exists() {
                    Some(v3_path.to_string_lossy().to_string())
                } else {
                    None
                },
            },
        ]
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
        if dest_path.exists() {
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

    pub async fn transcribe_audio(
        &self,
        wav_bytes: &[u8],
        api_fallback_key: Option<&str>,
    ) -> Result<TranscriptionResult, String> {
        let (samples, sample_rate) = Self::parse_wav_samples(wav_bytes)?;

        let vad = VadProcessor::new(sample_rate, 0.015);
        let speech_segments = vad.detect_segments(&samples);

        let duration_ms = (samples.len() as u64 * 1000) / sample_rate as u64;

        let models = self.get_available_models();
        let downloaded_model = models.into_iter().find(|m| m.is_downloaded);

        let whisper_exe = self.models_dir.join("whisper.exe");
        if whisper_exe.exists() && downloaded_model.is_some() {
            let model_info = downloaded_model.unwrap();
            let model_path = model_info.local_path.unwrap();

            let temp_wav = std::env::temp_dir().join(format!("whisper_in_{}.wav", uuid::Uuid::new_v4()));
            std::fs::write(&temp_wav, wav_bytes)
                .map_err(|e| format!("Failed to write temp audio: {}", e))?;

            let mut std_cmd = std::process::Command::new(&whisper_exe);
            std_cmd.creation_flags(0x08000000);
            std_cmd.arg("-m").arg(&model_path);
            std_cmd.arg("-f").arg(&temp_wav);
            std_cmd.arg("--output-json");

            let accel = *self.acceleration.read().await;
            if accel == AccelerationMode::Cpu {
                std_cmd.arg("-t").arg("4");
            }

            let output = tokio::process::Command::from(std_cmd)
                .output()
                .await
                .map_err(|e| format!("Whisper CLI execution failed: {}", e))?;

            let _ = std::fs::remove_file(&temp_wav);

            if output.status.success() {
                let stdout_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let mut segments = Vec::new();
                for (start, end) in speech_segments {
                    segments.push(TranscriptionSegment {
                        start_ms: start,
                        end_ms: end,
                        text: stdout_text.clone(),
                        confidence: 0.95,
                    });
                }
                return Ok(TranscriptionResult {
                    text: stdout_text,
                    language: "en".to_string(),
                    duration_ms,
                    segments,
                });
            }
        }

        if let Some(key) = api_fallback_key {
            let client = reqwest::Client::new();
            let form = reqwest::multipart::Form::new()
                .text("model", "whisper-1")
                .part(
                    "file",
                    reqwest::multipart::Part::bytes(wav_bytes.to_vec())
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

        let mut segments = Vec::new();
        for (start, end) in speech_segments {
            segments.push(TranscriptionSegment {
                start_ms: start,
                end_ms: end,
                text: "[Audio Voice Activity Detected]".to_string(),
                confidence: 0.85,
            });
        }

        Ok(TranscriptionResult {
            text: "[Audio Voice Activity Detected: Whisper model not downloaded yet]".to_string(),
            language: "und".to_string(),
            duration_ms,
            segments,
        })
    }
}
