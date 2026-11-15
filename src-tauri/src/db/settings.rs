use crate::db::Database;
use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key_name: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub provider_id: String,
    pub model_name: String,
    pub display_name: String,
    pub context_window: i64,
    pub max_output_tokens: i64,
    pub supports_reasoning: bool,
    pub supports_vision: bool,
    pub supports_tools: bool,
    pub input_price_per_m: f64,
    pub output_price_per_m: f64,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport_type: String,
    pub command: Option<String>,
    pub args: Option<String>,
    pub env: Option<String>,
    pub url: Option<String>,
    pub is_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledJob {
    pub id: String,
    pub name: String,
    pub cron_expression: String,
    pub prompt: String,
    pub model_id: String,
    pub chat_id: Option<String>,
    pub is_active: bool,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Database {
    pub fn create_provider(&self, provider: &Provider) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO providers (id, name, provider_type, base_url, api_key_name, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                provider.id,
                provider.name,
                provider.provider_type,
                provider.base_url,
                provider.api_key_name,
                if provider.is_active { 1 } else { 0 },
                provider.created_at,
                provider.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, provider_type, base_url, api_key_name, is_active, created_at, updated_at
             FROM providers ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let active: i64 = row.get(5)?;
            Ok(Provider {
                id: row.get(0)?,
                name: row.get(1)?,
                provider_type: row.get(2)?,
                base_url: row.get(3)?,
                api_key_name: row.get(4)?,
                is_active: active != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    pub fn get_provider(&self, id: &str) -> Result<Provider, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, provider_type, base_url, api_key_name, is_active, created_at, updated_at
             FROM providers WHERE id = ?1",
        )?;
        let provider = stmt.query_row([id], |row| {
            let active: i64 = row.get(5)?;
            Ok(Provider {
                id: row.get(0)?,
                name: row.get(1)?,
                provider_type: row.get(2)?,
                base_url: row.get(3)?,
                api_key_name: row.get(4)?,
                is_active: active != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?;
        Ok(provider)
    }

    pub fn update_provider(&self, provider: &Provider) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE providers SET name = ?1, provider_type = ?2, base_url = ?3, api_key_name = ?4, is_active = ?5, updated_at = ?6
             WHERE id = ?7",
            rusqlite::params![
                provider.name,
                provider.provider_type,
                provider.base_url,
                provider.api_key_name,
                if provider.is_active { 1 } else { 0 },
                provider.updated_at,
                provider.id
            ],
        )?;
        Ok(())
    }

    pub fn delete_provider(&self, id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM providers WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn create_model(&self, model: &Model) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO models (id, provider_id, model_name, display_name, context_window, max_output_tokens, supports_reasoning, supports_vision, supports_tools, input_price_per_m, output_price_per_m, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                model.id,
                model.provider_id,
                model.model_name,
                model.display_name,
                model.context_window,
                model.max_output_tokens,
                if model.supports_reasoning { 1 } else { 0 },
                if model.supports_vision { 1 } else { 0 },
                if model.supports_tools { 1 } else { 0 },
                model.input_price_per_m,
                model.output_price_per_m,
                if model.is_active { 1 } else { 0 },
                model.created_at
            ],
        )?;
        Ok(())
    }

    pub fn list_models(&self, provider_id: Option<&str>) -> Result<Vec<Model>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut result = Vec::new();
        match provider_id {
            Some(pid) => {
                let mut stmt = conn.prepare(
                    "SELECT id, provider_id, model_name, display_name, context_window, max_output_tokens, supports_reasoning, supports_vision, supports_tools, input_price_per_m, output_price_per_m, is_active, created_at
                     FROM models WHERE provider_id = ?1 ORDER BY display_name ASC",
                )?;
                let rows = stmt.query_map([pid], |row| {
                    let reasoning: i64 = row.get(6)?;
                    let vision: i64 = row.get(7)?;
                    let tools: i64 = row.get(8)?;
                    let active: i64 = row.get(11)?;
                    Ok(Model {
                        id: row.get(0)?,
                        provider_id: row.get(1)?,
                        model_name: row.get(2)?,
                        display_name: row.get(3)?,
                        context_window: row.get(4)?,
                        max_output_tokens: row.get(5)?,
                        supports_reasoning: reasoning != 0,
                        supports_vision: vision != 0,
                        supports_tools: tools != 0,
                        input_price_per_m: row.get(9)?,
                        output_price_per_m: row.get(10)?,
                        is_active: active != 0,
                        created_at: row.get(12)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, provider_id, model_name, display_name, context_window, max_output_tokens, supports_reasoning, supports_vision, supports_tools, input_price_per_m, output_price_per_m, is_active, created_at
                     FROM models ORDER BY display_name ASC",
                )?;
                let rows = stmt.query_map([], |row| {
                    let reasoning: i64 = row.get(6)?;
                    let vision: i64 = row.get(7)?;
                    let tools: i64 = row.get(8)?;
                    let active: i64 = row.get(11)?;
                    Ok(Model {
                        id: row.get(0)?,
                        provider_id: row.get(1)?,
                        model_name: row.get(2)?,
                        display_name: row.get(3)?,
                        context_window: row.get(4)?,
                        max_output_tokens: row.get(5)?,
                        supports_reasoning: reasoning != 0,
                        supports_vision: vision != 0,
                        supports_tools: tools != 0,
                        input_price_per_m: row.get(9)?,
                        output_price_per_m: row.get(10)?,
                        is_active: active != 0,
                        created_at: row.get(12)?,
                    })
                })?;
                for item in rows {
                    result.push(item?);
                }
            }
        }
        Ok(result)
    }

    pub fn get_model(&self, id: &str) -> Result<Model, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, provider_id, model_name, display_name, context_window, max_output_tokens, supports_reasoning, supports_vision, supports_tools, input_price_per_m, output_price_per_m, is_active, created_at
             FROM models WHERE id = ?1",
        )?;
        let model = stmt.query_row([id], |row| {
            let reasoning: i64 = row.get(6)?;
            let vision: i64 = row.get(7)?;
            let tools: i64 = row.get(8)?;
            let active: i64 = row.get(11)?;
            Ok(Model {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                model_name: row.get(2)?,
                display_name: row.get(3)?,
                context_window: row.get(4)?,
                max_output_tokens: row.get(5)?,
                supports_reasoning: reasoning != 0,
                supports_vision: vision != 0,
                supports_tools: tools != 0,
                input_price_per_m: row.get(9)?,
                output_price_per_m: row.get(10)?,
                is_active: active != 0,
                created_at: row.get(12)?,
            })
        })?;
        Ok(model)
    }

    pub fn update_model(&self, model: &Model) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE models SET provider_id = ?1, model_name = ?2, display_name = ?3, context_window = ?4, max_output_tokens = ?5, supports_reasoning = ?6, supports_vision = ?7, supports_tools = ?8, input_price_per_m = ?9, output_price_per_m = ?10, is_active = ?11
             WHERE id = ?12",
            rusqlite::params![
                model.provider_id,
                model.model_name,
                model.display_name,
                model.context_window,
                model.max_output_tokens,
                if model.supports_reasoning { 1 } else { 0 },
                if model.supports_vision { 1 } else { 0 },
                if model.supports_tools { 1 } else { 0 },
                model.input_price_per_m,
                model.output_price_per_m,
                if model.is_active { 1 } else { 0 },
                model.id
            ],
        )?;
        Ok(())
    }

    pub fn delete_model(&self, id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM models WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn create_mcp_server(&self, server: &McpServer) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO mcp_servers (id, name, transport_type, command, args, env, url, is_enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                server.id,
                server.name,
                server.transport_type,
                server.command,
                server.args,
                server.env,
                server.url,
                if server.is_enabled { 1 } else { 0 },
                server.created_at,
                server.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_mcp_servers(&self) -> Result<Vec<McpServer>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, transport_type, command, args, env, url, is_enabled, created_at, updated_at
             FROM mcp_servers ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let enabled: i64 = row.get(7)?;
            Ok(McpServer {
                id: row.get(0)?,
                name: row.get(1)?,
                transport_type: row.get(2)?,
                command: row.get(3)?,
                args: row.get(4)?,
                env: row.get(5)?,
                url: row.get(6)?,
                is_enabled: enabled != 0,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    pub fn get_mcp_server(&self, id: &str) -> Result<McpServer, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, transport_type, command, args, env, url, is_enabled, created_at, updated_at
             FROM mcp_servers WHERE id = ?1",
        )?;
        let server = stmt.query_row([id], |row| {
            let enabled: i64 = row.get(7)?;
            Ok(McpServer {
                id: row.get(0)?,
                name: row.get(1)?,
                transport_type: row.get(2)?,
                command: row.get(3)?,
                args: row.get(4)?,
                env: row.get(5)?,
                url: row.get(6)?,
                is_enabled: enabled != 0,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        Ok(server)
    }

    pub fn update_mcp_server(&self, server: &McpServer) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE mcp_servers SET name = ?1, transport_type = ?2, command = ?3, args = ?4, env = ?5, url = ?6, is_enabled = ?7, updated_at = ?8
             WHERE id = ?9",
            rusqlite::params![
                server.name,
                server.transport_type,
                server.command,
                server.args,
                server.env,
                server.url,
                if server.is_enabled { 1 } else { 0 },
                server.updated_at,
                server.id
            ],
        )?;
        Ok(())
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM mcp_servers WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn create_scheduled_job(&self, job: &ScheduledJob) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO scheduled_jobs (id, name, cron_expression, prompt, model_id, chat_id, is_active, last_run_at, next_run_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                job.id,
                job.name,
                job.cron_expression,
                job.prompt,
                job.model_id,
                job.chat_id,
                if job.is_active { 1 } else { 0 },
                job.last_run_at,
                job.next_run_at,
                job.created_at,
                job.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, cron_expression, prompt, model_id, chat_id, is_active, last_run_at, next_run_at, created_at, updated_at
             FROM scheduled_jobs ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let active: i64 = row.get(6)?;
            Ok(ScheduledJob {
                id: row.get(0)?,
                name: row.get(1)?,
                cron_expression: row.get(2)?,
                prompt: row.get(3)?,
                model_id: row.get(4)?,
                chat_id: row.get(5)?,
                is_active: active != 0,
                last_run_at: row.get(7)?,
                next_run_at: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?;
        let mut result = Vec::new();
        for item in rows {
            result.push(item?);
        }
        Ok(result)
    }

    pub fn get_scheduled_job(&self, id: &str) -> Result<ScheduledJob, AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, cron_expression, prompt, model_id, chat_id, is_active, last_run_at, next_run_at, created_at, updated_at
             FROM scheduled_jobs WHERE id = ?1",
        )?;
        let job = stmt.query_row([id], |row| {
            let active: i64 = row.get(6)?;
            Ok(ScheduledJob {
                id: row.get(0)?,
                name: row.get(1)?,
                cron_expression: row.get(2)?,
                prompt: row.get(3)?,
                model_id: row.get(4)?,
                chat_id: row.get(5)?,
                is_active: active != 0,
                last_run_at: row.get(7)?,
                next_run_at: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?;
        Ok(job)
    }

    pub fn update_scheduled_job(&self, job: &ScheduledJob) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE scheduled_jobs SET name = ?1, cron_expression = ?2, prompt = ?3, model_id = ?4, chat_id = ?5, is_active = ?6, last_run_at = ?7, next_run_at = ?8, updated_at = ?9
             WHERE id = ?10",
            rusqlite::params![
                job.name,
                job.cron_expression,
                job.prompt,
                job.model_id,
                job.chat_id,
                if job.is_active { 1 } else { 0 },
                job.last_run_at,
                job.next_run_at,
                job.updated_at,
                job.id
            ],
        )?;
        Ok(())
    }

    pub fn delete_scheduled_job(&self, id: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().map_err(|e| AppError::Database(e.to_string()))?;
        conn.execute("DELETE FROM scheduled_jobs WHERE id = ?1", [id])?;
        Ok(())
    }
}
