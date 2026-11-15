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

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
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
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(&temp_path, path)?;
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
            CREATE INDEX IF NOT EXISTS idx_usage_provider_model ON model_usage_stats (provider_id, model_id);",
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
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
}
