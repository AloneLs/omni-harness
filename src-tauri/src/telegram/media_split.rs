use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct MediaGroupItem {
    pub file_id: String,
    pub mime_type: Option<String>,
    pub caption: Option<String>,
}

pub struct MediaGroupEntry {
    pub chat_id: i64,
    pub items: Vec<MediaGroupItem>,
    pub last_updated: Instant,
}

#[derive(Clone, Default)]
pub struct MediaGroupCollector {
    entries: Arc<Mutex<HashMap<String, MediaGroupEntry>>>,
}

impl MediaGroupCollector {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_media(
        &self,
        group_id: &str,
        chat_id: i64,
        file_id: String,
        mime_type: Option<String>,
        caption: Option<String>,
    ) {
        let mut map = self.entries.lock().await;
        let entry = map
            .entry(group_id.to_string())
            .or_insert_with(|| MediaGroupEntry {
                chat_id,
                items: Vec::new(),
                last_updated: Instant::now(),
            });

        entry.items.push(MediaGroupItem {
            file_id,
            mime_type,
            caption,
        });
        entry.last_updated = Instant::now();
    }

    pub async fn wait_and_drain(&self, group_id: &str, debounce_ms: u64) -> Option<MediaGroupEntry> {
        let wait_dur = Duration::from_millis(debounce_ms);
        loop {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let mut map = self.entries.lock().await;
            if let Some(entry) = map.get(group_id) {
                if entry.last_updated.elapsed() >= wait_dur {
                    return map.remove(group_id);
                }
            } else {
                return None;
            }
        }
    }
}

pub struct TelegramTextSplitter;

impl TelegramTextSplitter {
    pub fn split_utf16_markdown(text: &str, max_len_utf16: usize) -> Vec<String> {
        if text.encode_utf16().count() <= max_len_utf16 {
            return vec![text.to_string()];
        }

        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        let mut current_utf16_len = 0;
        let mut in_code_block = false;
        let mut code_block_lang = String::new();

        let lines: Vec<&str> = text.split_inclusive('\n').collect();

        for line in lines {
            let line_trimmed = line.trim_end_matches(&['\r', '\n'][..]);
            let is_fence = line_trimmed.starts_with("```");

            let line_utf16 = line.encode_utf16().count();

            if current_utf16_len + line_utf16 + 10 > max_len_utf16 && !current_chunk.is_empty() {
                if in_code_block {
                    current_chunk.push_str("\n```\n");
                }
                chunks.push(current_chunk);
                current_chunk = String::new();
                current_utf16_len = 0;
                if in_code_block {
                    let reopen = format!("```{}\n", code_block_lang);
                    current_utf16_len += reopen.encode_utf16().count();
                    current_chunk.push_str(&reopen);
                }
            }

            if is_fence {
                if in_code_block {
                    in_code_block = false;
                    code_block_lang.clear();
                } else {
                    in_code_block = true;
                    code_block_lang = line_trimmed.trim_start_matches('`').trim().to_string();
                }
            }

            if line_utf16 > max_len_utf16 {
                let chars: Vec<char> = line.chars().collect();
                for c in chars {
                    let c_len = c.len_utf16();
                    if current_utf16_len + c_len + 10 > max_len_utf16 {
                        if in_code_block {
                            current_chunk.push_str("\n```\n");
                        }
                        chunks.push(current_chunk);
                        current_chunk = String::new();
                        current_utf16_len = 0;
                        if in_code_block {
                            let reopen = format!("```{}\n", code_block_lang);
                            current_utf16_len += reopen.encode_utf16().count();
                            current_chunk.push_str(&reopen);
                        }
                    }
                    current_chunk.push(c);
                    current_utf16_len += c_len;
                }
            } else {
                current_chunk.push_str(line);
                current_utf16_len += line_utf16;
            }
        }

        if !current_chunk.is_empty() {
            if in_code_block {
                current_chunk.push_str("\n```\n");
            }
            chunks.push(current_chunk);
        }

        chunks
    }
}
