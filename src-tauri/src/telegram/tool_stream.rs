use crate::telegram::client::TelegramClient;
use crate::telegram::formatting::{compact_text, escape_html};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ToolStreamEntry {
    pub id: String,
    pub name: String,
    pub preview: String,
    pub status: ToolEntryStatus,
    pub elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolEntryStatus {
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone)]
pub enum LiveTimelineItem {
    Tool(ToolStreamEntry),
    Notice(String),
}

pub struct LiveToolStream {
    client: TelegramClient,
    chat_id: i64,
    model_label: String,
    message_id: Option<i64>,
    last_edit: Instant,
    last_rendered: String,
    items: Vec<LiveTimelineItem>,
    total_steps: usize,
    dirty: bool,
}

impl LiveToolStream {
    pub fn new(client: TelegramClient, chat_id: i64, model_label: String) -> Self {
        Self {
            client,
            chat_id,
            model_label,
            message_id: None,
            last_edit: Instant::now() - Duration::from_secs(5),
            last_rendered: String::new(),
            items: Vec::new(),
            total_steps: 0,
            dirty: false,
        }
    }

    pub async fn start(&mut self) {
        let html = self.render_html();
        if let Ok(ids) = self.client.send_html(self.chat_id, &html, None).await {
            self.message_id = ids.last().copied();
            self.last_rendered = html;
            self.last_edit = Instant::now();
            self.dirty = false;
        }
    }

    pub fn on_tool_start(&mut self, id: &str, name: &str, args_json: &str) {
        self.total_steps += 1;
        let preview = summarize_tool_args(args_json);
        let existing = self.items.iter_mut().rev().find_map(|item| {
            if let LiveTimelineItem::Tool(t) = item {
                if t.id == id && !id.is_empty() {
                    return Some(t);
                }
            }
            None
        });
        if let Some(t) = existing {
            t.name = name.to_string();
            t.preview = preview;
            t.status = ToolEntryStatus::Running;
        } else {
            self.items.push(LiveTimelineItem::Tool(ToolStreamEntry {
                id: id.to_string(),
                name: name.to_string(),
                preview,
                status: ToolEntryStatus::Running,
                elapsed_ms: None,
            }));
        }
        self.dirty = true;
    }

    pub fn on_tool_end(&mut self, id: &str, success: bool, elapsed_ms: u64) {
        let existing = self.items.iter_mut().rev().find_map(|item| {
            if let LiveTimelineItem::Tool(t) = item {
                if t.id == id && !id.is_empty() {
                    return Some(t);
                }
            }
            None
        });
        if let Some(t) = existing {
            t.status = if success {
                ToolEntryStatus::Success
            } else {
                ToolEntryStatus::Failed
            };
            t.elapsed_ms = Some(elapsed_ms);
            self.dirty = true;
        }
    }

    pub fn on_retry(&mut self, attempt: u32, reason: &str) {
        let short = compact_text(reason, 110);
        self.items.push(LiveTimelineItem::Notice(format!("🔄 Retry #{}: {}", attempt, short)));
        self.dirty = true;
    }

    pub fn on_note(&mut self, note: &str) {
        let short = compact_text(note, 110);
        if !short.is_empty() {
            self.items.push(LiveTimelineItem::Notice(short));
            self.dirty = true;
        }
    }

    pub async fn maybe_flush(&mut self, force: bool) {
        if !self.dirty {
            return;
        }
        if !force && self.last_edit.elapsed() < Duration::from_millis(650) {
            return;
        }
        let mut html = self.render_html();
        if html.encode_utf16().count() > 3300 {
            self.trim_for_rollover();
            if self.items.len() > 16 {
                let keep_from = self.items.len().saturating_sub(12);
                self.items.drain(0..keep_from);
            }
            html = self.render_html();
        }
        if html == self.last_rendered {
            self.dirty = false;
            return;
        }
        if let Some(mid) = self.message_id {
            match self.client.edit_html(self.chat_id, mid, &html, None).await {
                Ok(()) => {
                    self.last_rendered = html;
                    self.last_edit = Instant::now();
                    self.dirty = false;
                }
                Err(err) if err.message.contains("single-message edit limit") => {
                    if let Ok(ids) = self.client.send_html(self.chat_id, &html, None).await {
                        self.message_id = ids.last().copied();
                        self.last_rendered = html;
                        self.last_edit = Instant::now();
                        self.dirty = false;
                    }
                }
                Err(_) => {
                    self.last_edit = Instant::now();
                }
            }
        } else if let Ok(ids) = self.client.send_html(self.chat_id, &html, None).await {
            self.message_id = ids.last().copied();
            self.last_rendered = html;
            self.last_edit = Instant::now();
            self.dirty = false;
        }
    }

    pub async fn finish_and_cleanup(&mut self) {
        if self.items.is_empty() {
            if let Some(mid) = self.message_id.take() {
                self.client.delete_message(self.chat_id, mid).await;
            }
            return;
        }
        self.maybe_flush(true).await;
    }

    fn trim_for_rollover(&mut self) {
        if self.items.len() > 14 {
            let cut = self.items.len() - 10;
            self.items.drain(0..cut);
        }
    }

    fn render_html(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "<b>⏳ Выполнение</b> · <code>{}</code>",
            escape_html(&compact_text(&self.model_label, 48))
        ));
        if self.items.is_empty() {
            lines.push("Подготовка контекста...".to_string());
        } else {
            let visible = if self.items.len() > 18 {
                let skipped = self.items.len() - 18;
                lines.push(format!("<i>… ещё {} шагов</i>", skipped));
                &self.items[self.items.len() - 18..]
            } else {
                &self.items[..]
            };
            for item in visible {
                match item {
                    LiveTimelineItem::Notice(note) => {
                        lines.push(escape_html(note));
                    }
                    LiveTimelineItem::Tool(entry) => {
                        let icon = match entry.status {
                            ToolEntryStatus::Running => "⚙",
                            ToolEntryStatus::Success => "✓",
                            ToolEntryStatus::Failed => "✗",
                        };
                        let time_str = entry
                            .elapsed_ms
                            .map(|ms| {
                                if ms >= 1000 {
                                    format!(" ({:.1}s)", ms as f64 / 1000.0)
                                } else {
                                    format!(" ({}ms)", ms)
                                }
                            })
                            .unwrap_or_default();
                        if entry.preview.is_empty() {
                            lines.push(format!(
                                "{} <code>{}</code>{}",
                                icon,
                                escape_html(&entry.name),
                                escape_html(&time_str)
                            ));
                        } else {
                            lines.push(format!(
                                "{} <code>{}</code> · {}{}",
                                icon,
                                escape_html(&entry.name),
                                escape_html(&entry.preview),
                                escape_html(&time_str)
                            ));
                        }
                    }
                }
            }
        }
        lines.join("\n")
    }
}

fn summarize_tool_args(args_json: &str) -> String {
    let val: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return compact_text(args_json, 60),
    };
    let obj = match val.as_object() {
        Some(o) => o,
        None => return String::new(),
    };
    for key in [
        "description",
        "path",
        "file_path",
        "command",
        "query_or_url",
        "query",
        "url",
        "pattern",
        "action",
    ] {
        if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
            let clean = compact_text(s, 64);
            if !clean.is_empty() {
                return clean;
            }
        }
    }
    String::new()
}
