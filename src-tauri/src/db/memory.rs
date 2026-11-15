use crate::db::Database;
use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id: String,
    pub category: String,
    pub key: String,
    pub content: String,
    pub token_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl Database {
    pub fn create_memory_entry(&self, entry: &MemoryEntry) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO memory_entries (id, category, key, content, token_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                entry.id,
                entry.category,
                entry.key,
                entry.content,
                entry.token_count,
                entry.created_at,
                entry.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_memory_entries(&self, category: Option<&str>) -> Result<Vec<MemoryEntry>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut result = Vec::new();
        match category {
            Some(cat) => {
                let mut stmt = conn.prepare(
                    "SELECT id, category, key, content, token_count, created_at, updated_at
                     FROM memory_entries WHERE category = ?1 ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([cat], |row| {
                    Ok(MemoryEntry {
                        id: row.get(0)?,
                        category: row.get(1)?,
                        key: row.get(2)?,
                        content: row.get(3)?,
                        token_count: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, category, key, content, token_count, created_at, updated_at
                     FROM memory_entries ORDER BY category ASC, updated_at DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(MemoryEntry {
                        id: row.get(0)?,
                        category: row.get(1)?,
                        key: row.get(2)?,
                        content: row.get(3)?,
                        token_count: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
        }
        Ok(result)
    }

    pub fn get_memory_entry(&self, id: &str) -> Result<MemoryEntry, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, key, content, token_count, created_at, updated_at
             FROM memory_entries WHERE id = ?1",
        )?;
        let entry = stmt.query_row([id], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                category: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                token_count: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        Ok(entry)
    }

    pub fn get_memory_entry_by_key(&self, category: &str, key: &str) -> Result<Option<MemoryEntry>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, key, content, token_count, created_at, updated_at
             FROM memory_entries WHERE category = ?1 AND key = ?2",
        )?;
        let mut rows = stmt.query_map([category, key], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                category: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                token_count: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        if let Some(first) = rows.next() {
            Ok(Some(first?))
        } else {
            Ok(None)
        }
    }

    pub fn update_memory_entry(&self, entry: &MemoryEntry) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE memory_entries SET category = ?1, key = ?2, content = ?3, token_count = ?4, updated_at = ?5
             WHERE id = ?6",
            rusqlite::params![
                entry.category,
                entry.key,
                entry.content,
                entry.token_count,
                entry.updated_at,
                entry.id
            ],
        )?;
        Ok(())
    }

    pub fn delete_memory_entry(&self, id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM memory_entries WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn search_memory_entries(&self, query: &str) -> Result<Vec<MemoryEntry>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT id, category, key, content, token_count, created_at, updated_at
             FROM memory_entries WHERE key LIKE ?1 OR content LIKE ?1 ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([pattern], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                category: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                token_count: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    pub fn get_total_memory_tokens(&self) -> Result<i64, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare("SELECT COALESCE(SUM(token_count), 0) FROM memory_entries")?;
        let total: i64 = stmt.query_row([], |row| row.get(0))?;
        Ok(total)
    }
}
