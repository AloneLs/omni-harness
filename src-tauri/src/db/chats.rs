use crate::db::Database;
use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub model_id: String,
    pub system_prompt: String,
    pub is_pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub model_id: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub sequence_num: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageStat {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub chat_id: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost: f64,
    pub created_at: String,
}

impl Database {
    pub fn create_project(&self, project: &Project) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO projects (id, name, root_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                project.id,
                project.name,
                project.root_path,
                project.created_at,
                project.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT id, name, root_path, created_at, updated_at FROM projects ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                root_path: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    pub fn get_project(&self, id: &str) -> Result<Project, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT id, name, root_path, created_at, updated_at FROM projects WHERE id = ?1")?;
        let project = stmt.query_row([id], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                root_path: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        Ok(project)
    }

    pub fn delete_project(&self, id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM projects WHERE id = ?1", [id])?;
        Ok(())
    }

    fn ensure_project_row(conn: &rusqlite::Connection, project_id: Option<&str>) {
        if let Some(pid) = project_id.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let exists: bool = conn
                .query_row("SELECT 1 FROM projects WHERE id = ?1", [pid], |_| Ok(true))
                .unwrap_or(false);
            if !exists {
                let name = pid
                    .trim_end_matches(&['/', '\\'][..])
                    .rsplit(&['/', '\\'][..])
                    .next()
                    .filter(|n| !n.is_empty())
                    .unwrap_or(pid);
                let now = chrono::Utc::now().to_rfc3339();
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO projects (id, name, root_path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![pid, name, pid, now, now],
                );
            }
        }
    }

    pub fn create_chat(&self, chat: &Chat) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        Self::ensure_project_row(&conn, chat.project_id.as_deref());
        conn.execute(
            "INSERT INTO chats (id, project_id, title, model_id, system_prompt, is_pinned, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                chat.id,
                chat.project_id,
                chat.title,
                chat.model_id,
                chat.system_prompt,
                if chat.is_pinned { 1 } else { 0 },
                chat.created_at,
                chat.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_chats(&self, project_id: Option<&str>) -> Result<Vec<Chat>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut result = Vec::new();
        match project_id {
            Some(pid) => {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, title, model_id, system_prompt, is_pinned, created_at, updated_at
                     FROM chats WHERE project_id = ?1 ORDER BY is_pinned DESC, updated_at DESC",
                )?;
                let rows = stmt.query_map([pid], |row| {
                    let pinned: i64 = row.get(5)?;
                    Ok(Chat {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        title: row.get(2)?,
                        model_id: row.get(3)?,
                        system_prompt: row.get(4)?,
                        is_pinned: pinned != 0,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, title, model_id, system_prompt, is_pinned, created_at, updated_at
                     FROM chats ORDER BY is_pinned DESC, updated_at DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    let pinned: i64 = row.get(5)?;
                    Ok(Chat {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        title: row.get(2)?,
                        model_id: row.get(3)?,
                        system_prompt: row.get(4)?,
                        is_pinned: pinned != 0,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
        }
        Ok(result)
    }

    pub fn get_chat(&self, id: &str) -> Result<Chat, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, title, model_id, system_prompt, is_pinned, created_at, updated_at
             FROM chats WHERE id = ?1",
        )?;
        let chat = stmt.query_row([id], |row| {
            let pinned: i64 = row.get(5)?;
            Ok(Chat {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                model_id: row.get(3)?,
                system_prompt: row.get(4)?,
                is_pinned: pinned != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        Ok(chat)
    }

    pub fn update_chat_title(&self, id: &str, title: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE chats SET title = ?1 WHERE id = ?2",
            rusqlite::params![title, id],
        )?;
        Ok(())
    }

    pub fn update_chat_pinned(&self, id: &str, is_pinned: bool) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE chats SET is_pinned = ?1 WHERE id = ?2",
            rusqlite::params![if is_pinned { 1 } else { 0 }, id],
        )?;
        Ok(())
    }

    pub fn update_chat_project(&self, id: &str, project_id: Option<&str>) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        Self::ensure_project_row(&conn, project_id);
        conn.execute(
            "UPDATE chats SET project_id = ?1 WHERE id = ?2",
            rusqlite::params![project_id, id],
        )?;
        Ok(())
    }

    pub fn update_chat_model(&self, id: &str, model_id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE chats SET model_id = ?1 WHERE id = ?2",
            rusqlite::params![model_id, id],
        )?;
        Ok(())
    }

    pub fn replace_chat_messages(&self, chat_id: &str, items: &[(String, String, Option<String>)]) -> Result<(), AppError> {
        let mut conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM messages WHERE chat_id = ?1", [chat_id])?;
        let now = chrono::Utc::now().to_rfc3339();
        for (idx, (role, content, model_id)) in items.iter().enumerate() {
            let id = uuid::Uuid::new_v4().to_string();
            let seq = (idx + 1) as i64;
            tx.execute(
                "INSERT INTO messages (id, chat_id, role, content, model_id, reasoning_content, tool_calls, tool_call_id, sequence_num, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, ?6, ?7)",
                rusqlite::params![id, chat_id, role, content, model_id, seq, now],
            )?;
        }
        if !items.is_empty() {
            let _ = tx.execute(
                "UPDATE chats SET updated_at = ?1 WHERE id = ?2 AND updated_at < ?1",
                rusqlite::params![now, chat_id],
            );
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_chat(&self, id: &str) -> Result<(), AppError> {
        let mut conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let tx = conn.transaction()?;
        let _ = tx.execute("DELETE FROM agent_checkpoints WHERE chat_id = ?1", [id]);
        let _ = tx.execute(
            "DELETE FROM agent_checkpoints WHERE chat_id = ?1",
            [format!("full_ctx:{}", id)],
        );
        let _ = tx.execute(
            "DELETE FROM agent_checkpoints WHERE chat_id = ?1",
            [format!("tg_ctx:{}", id)],
        );
        for tbl in [
            "chat_context_cache",
            "goal_states",
            "plan_states",
            "session_memory",
            "session_artifacts",
            "compaction_events",
            "checkpoints",
        ] {
            let _ = tx.execute(&format!("DELETE FROM {} WHERE chat_id = ?1", tbl), [id]);
        }
        tx.execute("DELETE FROM chats WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn create_message(&self, message: &Message) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        if message.role == "assistant" && !message.content.trim().is_empty() {
            let mut chk = conn.prepare(
                "SELECT role, content FROM messages WHERE chat_id = ?1 ORDER BY sequence_num DESC LIMIT 1"
            )?;
            let last_pair: Option<(String, String)> = chk
                .query_row([&message.chat_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .ok();
            if let Some((lr, lc)) = last_pair {
                if lr == "assistant" && lc.trim() == message.content.trim() {
                    return Ok(());
                }
            }
        }
        conn.execute(
            "INSERT INTO messages (id, chat_id, role, content, model_id, reasoning_content, tool_calls, tool_call_id, sequence_num, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                message.id,
                message.chat_id,
                message.role,
                message.content,
                message.model_id,
                message.reasoning_content,
                message.tool_calls,
                message.tool_call_id,
                message.sequence_num,
                message.created_at
            ],
        )?;
        let _ = conn.execute(
            "UPDATE chats SET updated_at = ?1 WHERE id = ?2 AND updated_at < ?1",
            rusqlite::params![message.created_at, message.chat_id],
        );
        Ok(())
    }

    pub fn list_messages(&self, chat_id: &str) -> Result<Vec<Message>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, role, content, model_id, reasoning_content, tool_calls, tool_call_id, sequence_num, created_at
             FROM messages WHERE chat_id = ?1 ORDER BY sequence_num ASC",
        )?;
        let rows = stmt.query_map([chat_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                model_id: row.get(4)?,
                reasoning_content: row.get(5)?,
                tool_calls: row.get(6)?,
                tool_call_id: row.get(7)?,
                sequence_num: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    pub fn delete_messages_since(&self, chat_id: &str, sequence_num: i64) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "DELETE FROM messages WHERE chat_id = ?1 AND sequence_num >= ?2",
            rusqlite::params![chat_id, sequence_num],
        )?;
        Ok(())
    }

    pub fn delete_message(&self, id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM messages WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn record_usage_stat(&self, stat: &ModelUsageStat) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO model_usage_stats (id, provider_id, model_id, chat_id, prompt_tokens, completion_tokens, total_tokens, estimated_cost, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                stat.id,
                stat.provider_id,
                stat.model_id,
                stat.chat_id,
                stat.prompt_tokens,
                stat.completion_tokens,
                stat.total_tokens,
                stat.estimated_cost,
                stat.created_at
            ],
        )?;
        Ok(())
    }

    pub fn list_usage_stats(&self, provider_id: Option<&str>, model_id: Option<&str>) -> Result<Vec<ModelUsageStat>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut result = Vec::new();
        match (provider_id, model_id) {
            (Some(p), Some(m)) => {
                let mut stmt = conn.prepare(
                    "SELECT id, provider_id, model_id, chat_id, prompt_tokens, completion_tokens, total_tokens, estimated_cost, created_at
                     FROM model_usage_stats WHERE provider_id = ?1 AND model_id = ?2 ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map([p, m], |row| {
                    Ok(ModelUsageStat {
                        id: row.get(0)?,
                        provider_id: row.get(1)?,
                        model_id: row.get(2)?,
                        chat_id: row.get(3)?,
                        prompt_tokens: row.get(4)?,
                        completion_tokens: row.get(5)?,
                        total_tokens: row.get(6)?,
                        estimated_cost: row.get(7)?,
                        created_at: row.get(8)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
            (Some(p), None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, provider_id, model_id, chat_id, prompt_tokens, completion_tokens, total_tokens, estimated_cost, created_at
                     FROM model_usage_stats WHERE provider_id = ?1 ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map([p], |row| {
                    Ok(ModelUsageStat {
                        id: row.get(0)?,
                        provider_id: row.get(1)?,
                        model_id: row.get(2)?,
                        chat_id: row.get(3)?,
                        prompt_tokens: row.get(4)?,
                        completion_tokens: row.get(5)?,
                        total_tokens: row.get(6)?,
                        estimated_cost: row.get(7)?,
                        created_at: row.get(8)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
            (None, Some(m)) => {
                let mut stmt = conn.prepare(
                    "SELECT id, provider_id, model_id, chat_id, prompt_tokens, completion_tokens, total_tokens, estimated_cost, created_at
                     FROM model_usage_stats WHERE model_id = ?1 ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map([m], |row| {
                    Ok(ModelUsageStat {
                        id: row.get(0)?,
                        provider_id: row.get(1)?,
                        model_id: row.get(2)?,
                        chat_id: row.get(3)?,
                        prompt_tokens: row.get(4)?,
                        completion_tokens: row.get(5)?,
                        total_tokens: row.get(6)?,
                        estimated_cost: row.get(7)?,
                        created_at: row.get(8)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
            (None, None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, provider_id, model_id, chat_id, prompt_tokens, completion_tokens, total_tokens, estimated_cost, created_at
                     FROM model_usage_stats ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(ModelUsageStat {
                        id: row.get(0)?,
                        provider_id: row.get(1)?,
                        model_id: row.get(2)?,
                        chat_id: row.get(3)?,
                        prompt_tokens: row.get(4)?,
                        completion_tokens: row.get(5)?,
                        total_tokens: row.get(6)?,
                        estimated_cost: row.get(7)?,
                        created_at: row.get(8)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
        }
        Ok(result)
    }

    pub fn save_checkpoint(&self, chat_id: &str, history_json: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO agent_checkpoints (chat_id, history_json, updated_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![chat_id, history_json, now],
        )?;
        Ok(())
    }

    pub fn load_checkpoint(&self, chat_id: &str) -> Result<Option<String>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT history_json FROM agent_checkpoints WHERE chat_id = ?1")?;
        let mut rows = stmt.query([chat_id])?;
        if let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            Ok(Some(json))
        } else {
            Ok(None)
        }
    }

    pub fn delete_checkpoint(&self, chat_id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM agent_checkpoints WHERE chat_id = ?1", [chat_id])?;
        Ok(())
    }


    pub fn sync_chat_context(&self, chat_id: &str, title: &str, messages_json: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO chat_context_cache (chat_id, title, messages_json, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![chat_id, title, messages_json, now],
        )?;
        Ok(())
    }

    pub fn list_cached_chats(&self) -> Result<Vec<(String, String, String)>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT chat_id, title, updated_at FROM chat_context_cache ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_cached_chat_messages(&self, chat_id: &str) -> Result<Option<(String, String)>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT title, messages_json FROM chat_context_cache WHERE chat_id = ?1")?;
        let mut rows = stmt.query([chat_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get(1)?)))
        } else {
            Ok(None)
        }
    }

}
