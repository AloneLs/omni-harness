pub mod chats;
pub mod memory;
pub mod settings;

use crate::error::AppError;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSnapshot {
    pub id: String,
    pub chat_id: String,
    pub message_id: Option<String>,
    pub file_path: String,
    pub before_blob: Option<Vec<u8>>,
    pub after_blob: Option<Vec<u8>>,
    pub existed_before: bool,
    pub added_lines: i64,
    pub removed_lines: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRecord {
    pub seq: i64,
    pub session_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointRecord {
    pub id: String,
    pub chat_id: String,
    pub turn_seq: i64,
    pub kind: String,
    pub message_seq: i64,
    pub manifest_json: String,
    pub created_at: String,
}

struct EventWriteItem {
    session_id: String,
    kind: String,
    payload: String,
    created_at: String,
}

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    event_tx: std::sync::mpsc::SyncSender<EventWriteItem>,
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp_name = format!(".tmp.{}", uuid::Uuid::new_v4());
    let temp_path = match path.parent() {
        Some(parent) => parent.join(temp_name),
        None => PathBuf::from(temp_name),
    };
    std::fs::write(&temp_path, data)?;
    if let Err(err) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err.into());
    }
    Ok(())
}

impl Database {
    pub fn new(db_path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS providers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                provider_type TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key_name TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS models (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                model_name TEXT NOT NULL,
                display_name TEXT NOT NULL,
                context_window INTEGER NOT NULL DEFAULT 128000,
                max_output_tokens INTEGER NOT NULL DEFAULT 8192,
                supports_reasoning INTEGER NOT NULL DEFAULT 0,
                supports_vision INTEGER NOT NULL DEFAULT 0,
                supports_tools INTEGER NOT NULL DEFAULT 1,
                input_price_per_m REAL NOT NULL DEFAULT 0.0,
                output_price_per_m REAL NOT NULL DEFAULT 0.0,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                FOREIGN KEY (provider_id) REFERENCES providers (id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                root_path TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS chats (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                title TEXT NOT NULL,
                model_id TEXT NOT NULL,
                system_prompt TEXT NOT NULL,
                is_pinned INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects (id) ON DELETE SET NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                model_id TEXT,
                reasoning_content TEXT,
                tool_calls TEXT,
                tool_call_id TEXT,
                sequence_num INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (chat_id) REFERENCES chats (id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS file_snapshots (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                message_id TEXT,
                file_path TEXT NOT NULL,
                before_blob BLOB,
                after_blob BLOB,
                existed_before INTEGER NOT NULL,
                added_lines INTEGER NOT NULL DEFAULT 0,
                removed_lines INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                FOREIGN KEY (chat_id) REFERENCES chats (id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS chat_context_cache (
                chat_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                messages_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_checkpoints (
                chat_id TEXT PRIMARY KEY,
                history_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS goal_states (
                chat_id TEXT PRIMARY KEY,
                state_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memory_entries (
                id TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                key TEXT NOT NULL,
                content TEXT NOT NULL,
                token_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mcp_servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                transport_type TEXT NOT NULL,
                command TEXT,
                args TEXT,
                env TEXT,
                url TEXT,
                is_enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS scheduled_jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                cron_expression TEXT NOT NULL,
                prompt TEXT NOT NULL,
                model_id TEXT NOT NULL,
                chat_id TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                last_run_at TEXT,
                next_run_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS model_usage_stats (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                estimated_cost REAL NOT NULL DEFAULT 0.0,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_messages_chat_seq ON messages (chat_id, sequence_num);
            CREATE INDEX IF NOT EXISTS idx_snapshots_chat_path ON file_snapshots (chat_id, file_path);
            CREATE INDEX IF NOT EXISTS idx_memory_cat_key ON memory_entries (category, key);
            CREATE TABLE IF NOT EXISTS plan_states (
                chat_id TEXT PRIMARY KEY,
                plan_mode INTEGER NOT NULL DEFAULT 0,
                plan_markdown TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS token_cache (
                hash TEXT PRIMARY KEY,
                tokens INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_memory (
                chat_id TEXT PRIMARY KEY,
                memory_markdown TEXT NOT NULL,
                last_msg_count INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS session_artifacts (
                chat_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                value TEXT NOT NULL,
                touched_at TEXT NOT NULL,
                PRIMARY KEY (chat_id, kind, value)
            );

            CREATE TABLE IF NOT EXISTS compaction_events (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                tier TEXT NOT NULL,
                tokens_before INTEGER NOT NULL,
                tokens_after INTEGER NOT NULL,
                details TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_usage_provider_model ON model_usage_stats (provider_id, model_id);
            CREATE INDEX IF NOT EXISTS idx_artifacts_chat_kind ON session_artifacts (chat_id, kind, touched_at DESC);
            CREATE TABLE IF NOT EXISTS checkpoints (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                turn_seq INTEGER NOT NULL DEFAULT 0,
                kind TEXT NOT NULL,
                message_seq INTEGER NOT NULL DEFAULT 0,
                manifest_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_compaction_chat ON compaction_events (chat_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_checkpoints_chat ON checkpoints (chat_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_events_session_seq ON events (session_id, seq);",
        )?;

        let _ = conn.execute("ALTER TABLE messages ADD COLUMN model_id TEXT", []);

        let (event_tx, event_rx) = std::sync::mpsc::sync_channel::<EventWriteItem>(2048);
        let writer_db_path = db_path.to_path_buf();
        std::thread::spawn(move || {
            if let Ok(wconn) = Connection::open(&writer_db_path) {
                let _ = wconn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;");
                while let Ok(item) = event_rx.recv() {
                    let _ = wconn.execute(
                        "INSERT INTO events (session_id, kind, payload, created_at) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![item.session_id, item.kind, item.payload, item.created_at],
                    );
                }
            }
        });

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            event_tx,
        })
    }

    pub fn conn(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    pub fn record_file_snapshot(&self, snapshot: &FileSnapshot) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO file_snapshots (id, chat_id, message_id, file_path, before_blob, after_blob, existed_before, added_lines, removed_lines, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                snapshot.id,
                snapshot.chat_id,
                snapshot.message_id,
                snapshot.file_path,
                snapshot.before_blob,
                snapshot.after_blob,
                if snapshot.existed_before { 1 } else { 0 },
                snapshot.added_lines,
                snapshot.removed_lines,
                snapshot.created_at
            ],
        )?;
        Ok(())
    }

    pub fn get_file_snapshot(&self, snapshot_id: &str) -> Result<FileSnapshot, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, message_id, file_path, before_blob, after_blob, existed_before, added_lines, removed_lines, created_at
             FROM file_snapshots WHERE id = ?1",
        )?;
        let snapshot = stmt.query_row([snapshot_id], |row| {
            let existed: i64 = row.get(6)?;
            Ok(FileSnapshot {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                message_id: row.get(2)?,
                file_path: row.get(3)?,
                before_blob: row.get(4)?,
                after_blob: row.get(5)?,
                existed_before: existed != 0,
                added_lines: row.get(7)?,
                removed_lines: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        Ok(snapshot)
    }

    pub fn list_snapshots_for_chat(&self, chat_id: &str) -> Result<Vec<FileSnapshot>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, message_id, file_path, before_blob, after_blob, existed_before, added_lines, removed_lines, created_at
             FROM file_snapshots WHERE chat_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([chat_id], |row| {
            let existed: i64 = row.get(6)?;
            Ok(FileSnapshot {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                message_id: row.get(2)?,
                file_path: row.get(3)?,
                before_blob: row.get(4)?,
                after_blob: row.get(5)?,
                existed_before: existed != 0,
                added_lines: row.get(7)?,
                removed_lines: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    pub fn rollback_file_snapshot(&self, snapshot_id: &str) -> Result<FileSnapshot, AppError> {
        let snapshot = self.get_file_snapshot(snapshot_id)?;
        let target_path = PathBuf::from(&snapshot.file_path);
        if snapshot.existed_before {
            if let Some(ref data) = snapshot.before_blob {
                atomic_write(&target_path, data)?;
            }
        } else if target_path.exists() {
            std::fs::remove_file(&target_path)?;
        }
        Ok(snapshot)
    }

    pub fn save_goal_state(&self, chat_id: &str, state_json: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO goal_states (chat_id, state_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(chat_id) DO UPDATE SET state_json = excluded.state_json, updated_at = excluded.updated_at",
            rusqlite::params![chat_id, state_json, now],
        )?;
        Ok(())
    }

    pub fn load_goal_state(&self, chat_id: &str) -> Result<Option<String>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT state_json FROM goal_states WHERE chat_id = ?1")?;
        let mut rows = stmt.query([chat_id])?;
        if let Some(row) = rows.next()? {
            let s: String = row.get(0)?;
            Ok(Some(s))
        } else {
            Ok(None)
        }
    }

    pub fn delete_goal_state(&self, chat_id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM goal_states WHERE chat_id = ?1", [chat_id])?;
        Ok(())
    }

    pub fn save_plan_state(&self, chat_id: &str, plan_mode: bool, plan_markdown: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO plan_states (chat_id, plan_mode, plan_markdown, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat_id) DO UPDATE SET plan_mode = excluded.plan_mode, plan_markdown = excluded.plan_markdown, updated_at = excluded.updated_at",
            rusqlite::params![chat_id, if plan_mode { 1 } else { 0 }, plan_markdown, now],
        )?;
        Ok(())
    }

    pub fn load_plan_state(&self, chat_id: &str) -> Result<Option<(bool, String)>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT plan_mode, plan_markdown FROM plan_states WHERE chat_id = ?1")?;
        let mut rows = stmt.query([chat_id])?;
        if let Some(row) = rows.next()? {
            let pm: i64 = row.get(0)?;
            let md: String = row.get(1)?;
            Ok(Some((pm != 0, md)))
        } else {
            Ok(None)
        }
    }

    pub fn delete_plan_state(&self, chat_id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM plan_states WHERE chat_id = ?1", [chat_id])?;
        Ok(())
    }

    pub fn get_cached_token_count(&self, hash: &str) -> Result<Option<u32>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT tokens FROM token_cache WHERE hash = ?1")?;
        let mut rows = stmt.query([hash])?;
        if let Some(row) = rows.next()? {
            let t: i64 = row.get(0)?;
            Ok(Some(t as u32))
        } else {
            Ok(None)
        }
    }

    pub fn set_cached_token_count(&self, hash: &str, tokens: u32) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO token_cache (hash, tokens, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(hash) DO UPDATE SET tokens = excluded.tokens, updated_at = excluded.updated_at",
            rusqlite::params![hash, tokens as i64, now],
        )?;
        Ok(())
    }

    pub fn save_session_memory(&self, chat_id: &str, memory_markdown: &str, last_msg_count: usize) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO session_memory (chat_id, memory_markdown, last_msg_count, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat_id) DO UPDATE SET memory_markdown = excluded.memory_markdown, last_msg_count = excluded.last_msg_count, updated_at = excluded.updated_at",
            rusqlite::params![chat_id, memory_markdown, last_msg_count as i64, now],
        )?;
        Ok(())
    }

    pub fn load_session_memory(&self, chat_id: &str) -> Result<Option<(String, usize)>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT memory_markdown, last_msg_count FROM session_memory WHERE chat_id = ?1")?;
        let mut rows = stmt.query([chat_id])?;
        if let Some(row) = rows.next()? {
            let md: String = row.get(0)?;
            let cnt: i64 = row.get(1)?;
            Ok(Some((md, cnt as usize)))
        } else {
            Ok(None)
        }
    }

    pub fn record_session_artifact(&self, chat_id: &str, kind: &str, value: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO session_artifacts (chat_id, kind, value, touched_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(chat_id, kind, value) DO UPDATE SET touched_at = excluded.touched_at",
            rusqlite::params![chat_id, kind, value, now],
        )?;
        Ok(())
    }

    pub fn list_recent_artifacts(&self, chat_id: &str, kind: &str, limit: usize) -> Result<Vec<String>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT value FROM session_artifacts WHERE chat_id = ?1 AND kind = ?2 ORDER BY touched_at DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(rusqlite::params![chat_id, kind, limit as i64], |row| row.get(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn record_compaction_event(
        &self,
        chat_id: &str,
        tier: &str,
        tokens_before: u32,
        tokens_after: u32,
        details: &str,
    ) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO compaction_events (id, chat_id, tier, tokens_before, tokens_after, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, chat_id, tier, tokens_before as i64, tokens_after as i64, details, now],
        )?;
        Ok(())
    }

    pub fn emit_event(&self, session_id: &str, kind: &str, payload: serde_json::Value) {
        let item = EventWriteItem {
            session_id: session_id.to_string(),
            kind: kind.to_string(),
            payload: payload.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = self.event_tx.try_send(item);
    }

    pub fn list_events_since(&self, session_id: &str, last_seq: i64) -> Result<Vec<EventRecord>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT seq, session_id, kind, payload, created_at FROM events WHERE session_id = ?1 AND seq > ?2 ORDER BY seq ASC LIMIT 500",
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id, last_seq], |row| {
            let p_str: String = row.get(3)?;
            let payload = serde_json::from_str(&p_str).unwrap_or(serde_json::Value::Null);
            Ok(EventRecord {
                seq: row.get(0)?,
                session_id: row.get(1)?,
                kind: row.get(2)?,
                payload,
                created_at: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn save_checkpoint_record(
        &self,
        chat_id: &str,
        checkpoint_id: &str,
        turn_seq: i64,
        kind: &str,
        message_seq: i64,
        manifest_json: &str,
        max_per_session: usize,
    ) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO checkpoints (id, chat_id, turn_seq, kind, message_seq, manifest_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![checkpoint_id, chat_id, turn_seq, kind, message_seq, manifest_json, now],
        )?;
        conn.execute(
            "DELETE FROM checkpoints WHERE chat_id = ?1 AND id NOT IN (SELECT id FROM checkpoints WHERE chat_id = ?1 ORDER BY created_at DESC LIMIT ?2)",
            rusqlite::params![chat_id, max_per_session as i64],
        )?;
        Ok(())
    }

    pub fn list_checkpoints(&self, chat_id: &str) -> Result<Vec<CheckpointRecord>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, turn_seq, kind, message_seq, manifest_json, created_at FROM checkpoints WHERE chat_id = ?1 OR chat_id LIKE ?2 ORDER BY created_at DESC LIMIT 100",
        )?;
        let pattern = format!("{}:%", chat_id);
        let rows = stmt.query_map(rusqlite::params![chat_id, pattern], |row| {
            Ok(CheckpointRecord {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                turn_seq: row.get(2)?,
                kind: row.get(3)?,
                message_seq: row.get(4)?,
                manifest_json: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_checkpoint(&self, checkpoint_id: &str) -> Result<CheckpointRecord, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, turn_seq, kind, message_seq, manifest_json, created_at FROM checkpoints WHERE id = ?1",
        )?;
        let rec = stmt.query_row([checkpoint_id], |row| {
            Ok(CheckpointRecord {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                turn_seq: row.get(2)?,
                kind: row.get(3)?,
                message_seq: row.get(4)?,
                manifest_json: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        Ok(rec)
    }

    pub fn list_all_referenced_blob_hashes(&self) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        let Ok(conn) = self.conn.lock() else {
            return set;
        };
        let Ok(mut stmt) = conn.prepare("SELECT manifest_json FROM checkpoints") else {
            return set;
        };
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for r in rows.flatten() {
                if let Ok(m) = serde_json::from_str::<crate::tools::blob_store::CheckpointManifest>(&r) {
                    for e in m.entries {
                        set.insert(e.blob_hash);
                    }
                }
            }
        }
        set
    }

    pub fn max_message_seq(&self, chat_id: &str) -> i64 {
        let Ok(conn) = self.conn.lock() else {
            return 0;
        };
        conn.query_row(
            "SELECT COALESCE(MAX(sequence_num), 0) FROM messages WHERE chat_id = ?1",
            [chat_id],
            |row| row.get(0),
        )
        .unwrap_or(0)
    }

    pub fn truncate_messages_after_seq(&self, chat_id: &str, message_seq: i64) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "DELETE FROM messages WHERE chat_id = ?1 AND sequence_num > ?2",
            rusqlite::params![chat_id, message_seq],
        )?;
        conn.execute("DELETE FROM agent_checkpoints WHERE chat_id = ?1", [chat_id])?;
        Ok(())
    }
}
