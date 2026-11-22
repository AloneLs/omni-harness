pub mod protocol;

pub use protocol::*;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigFile {
    #[serde(alias = "mcpServers", default)]
    pub mcp_servers: HashMap<String, McpServerConfigRaw>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfigRaw {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolWithServer {
    pub server_name: String,
    pub tool: McpTool,
}

pub struct StdioConnection {
    writer: Arc<Mutex<tokio::process::ChildStdin>>,
    responses: Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<JsonRpcResponse>>>>,
    child: Arc<Mutex<tokio::process::Child>>,
}

impl StdioConnection {
    pub async fn is_alive(&self) -> bool {
        let mut child = self.child.lock().await;
        matches!(child.try_wait(), Ok(None))
    }
}

pub enum McpTransportConnection {
    Stdio(StdioConnection),
    Sse {
        base_url: String,
        post_endpoint: Arc<RwLock<String>>,
        client: reqwest::Client,
    },
    HttpStream {
        endpoint: String,
        client: reqwest::Client,
    },
}

pub struct McpServerInstance {
    pub config: McpServerConfig,
    pub capabilities: McpServerCapabilities,
    pub connection: McpTransportConnection,
    pub tools: Vec<McpTool>,
    req_counter: AtomicU64,
}

async fn parse_http_or_sse_response(resp: reqwest::Response) -> Result<Value, String> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("MCP server returned HTTP {}: {}", status.as_u16(), body));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read MCP HTTP response: {}", e))?;

    if status.as_u16() == 202 && text.trim().is_empty() {
        return Ok(Value::Null);
    }

    let rpc_resp: JsonRpcResponse = if content_type.contains("text/event-stream") {
        let mut found: Option<JsonRpcResponse> = None;
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                let trimmed = data.trim();
                if trimmed.is_empty() || trimmed == "[DONE]" {
                    continue;
                }
                if let Ok(parsed) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                    found = Some(parsed);
                    break;
                }
            }
        }
        found.ok_or_else(|| "No JSON-RPC payload found in SSE response".to_string())?
    } else {
        serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse JSON-RPC response: {}", e))?
    };

    if let Some(err) = rpc_resp.error {
        Err(format!("MCP Error {}: {}", err.code, err.message))
    } else {
        Ok(rpc_resp.result.unwrap_or(Value::Null))
    }
}

impl McpServerInstance {
    pub async fn send_request(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, String> {
        let req_id = self.req_counter.fetch_add(1, Ordering::SeqCst);
        let rpc_req = JsonRpcRequest::new(Some(json!(req_id)), method, params);

        match &self.connection {
            McpTransportConnection::Stdio(conn) => {
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                {
                    let mut resp_map = conn.responses.lock().await;
                    resp_map.insert(req_id, reply_tx);
                }

                let mut line = match serde_json::to_string(&rpc_req) {
                    Ok(s) => s,
                    Err(e) => {
                        conn.responses.lock().await.remove(&req_id);
                        return Err(format!("Serialization error: {}", e));
                    }
                };
                line.push('\n');

                let write_res = {
                    let mut stdin = conn.writer.lock().await;
                    if let Err(e) = stdin.write_all(line.as_bytes()).await {
                        Err(format!("Stdin write error: {}", e))
                    } else if let Err(e) = stdin.flush().await {
                        Err(format!("Stdin flush error: {}", e))
                    } else {
                        Ok(())
                    }
                };
                if let Err(err) = write_res {
                    conn.responses.lock().await.remove(&req_id);
                    return Err(err);
                }

                match tokio::time::timeout(std::time::Duration::from_secs(30), reply_rx).await {
                    Ok(Ok(resp)) => {
                        if let Some(err) = resp.error {
                            Err(format!("MCP Error {}: {}", err.code, err.message))
                        } else {
                            Ok(resp.result.unwrap_or(Value::Null))
                        }
                    }
                    Ok(Err(_)) => {
                        conn.responses.lock().await.remove(&req_id);
                        Err("Response channel dropped".to_string())
                    }
                    Err(_) => {
                        conn.responses.lock().await.remove(&req_id);
                        Err("MCP request timed out after 30s".to_string())
                    }
                }
            }
            McpTransportConnection::Sse {
                post_endpoint,
                client,
                ..
            } => {
                let url = post_endpoint.read().await.clone();
                let resp = client
                    .post(&url)
                    .header("Accept", "application/json, text/event-stream")
                    .json(&rpc_req)
                    .send()
                    .await
                    .map_err(|e| format!("SSE POST request failed: {}", e))?;
                parse_http_or_sse_response(resp).await
            }
            McpTransportConnection::HttpStream { endpoint, client } => {
                let resp = client
                    .post(endpoint)
                    .header("Accept", "application/json, text/event-stream")
                    .json(&rpc_req)
                    .send()
                    .await
                    .map_err(|e| format!("HTTP request failed: {}", e))?;
                parse_http_or_sse_response(resp).await
            }
        }
    }

    pub async fn initialize(&mut self) -> Result<(), String> {
        let init_params = McpInitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: McpClientCapabilities {
                roots: Some(json!({"listChanged": false})),
                sampling: None,
            },
            client_info: McpImplementation {
                name: "omni-harness".to_string(),
                version: "0.1.0".to_string(),
            },
        };

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.send_request("initialize", Some(json!(init_params))),
        )
        .await
        .map_err(|_| "MCP initialize timed out after 15s".to_string())??;
        let init_result: McpInitializeResult = serde_json::from_value(res)
            .map_err(|e| format!("Failed to parse initialize result: {}", e))?;

        self.capabilities = init_result.capabilities;

        let _ = self
            .send_notification("notifications/initialized", None)
            .await;

        let tools_res = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.send_request("tools/list", None),
        )
        .await
        .map_err(|_| "MCP tools/list timed out after 15s".to_string())??;
        let list_tools: McpListToolsResult = serde_json::from_value(tools_res)
            .map_err(|e| format!("Failed to parse tools/list result: {}", e))?;

        self.tools = list_tools.tools;
        Ok(())
    }

    pub async fn send_notification(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        let rpc_req = JsonRpcRequest::new(None, method, params);

        match &self.connection {
            McpTransportConnection::Stdio(conn) => {
                let mut line = serde_json::to_string(&rpc_req)
                    .map_err(|e| format!("Serialization error: {}", e))?;
                line.push('\n');

                let mut stdin = conn.writer.lock().await;
                stdin
                    .write_all(line.as_bytes())
                    .await
                    .map_err(|e| format!("Stdin write error: {}", e))?;
                stdin.flush().await.map_err(|e| format!("Flush error: {}", e))?;
                Ok(())
            }
            McpTransportConnection::Sse {
                post_endpoint,
                client,
                ..
            } => {
                let url = post_endpoint.read().await.clone();
                let _ = client.post(&url).json(&rpc_req).send().await;
                Ok(())
            }
            McpTransportConnection::HttpStream { endpoint, client } => {
                let _ = client.post(endpoint).json(&rpc_req).send().await;
                Ok(())
            }
        }
    }

    pub async fn ping(&self) -> Result<bool, String> {
        if let McpTransportConnection::Stdio(conn) = &self.connection {
            if !conn.is_alive().await {
                return Ok(false);
            }
        }
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.send_request("ping", None),
        )
        .await;
        Ok(matches!(res, Ok(Ok(_))))
    }
}

#[derive(Clone, Default)]
pub struct McpServerCatalog {
    servers: Arc<RwLock<HashMap<String, Arc<McpServerInstance>>>>,
}

impl McpServerCatalog {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn connect_server(&self, config: &McpServerConfig) -> Result<(), String> {
        let transport_type = config.transport.to_lowercase();
        let connection = if transport_type == "stdio" {
            let cmd_path = config
                .command
                .as_ref()
                .ok_or_else(|| "Missing command for stdio MCP server".to_string())?;

            let mut std_cmd = std::process::Command::new(cmd_path);
            #[cfg(windows)]
            {
                std_cmd.creation_flags(0x08000000);
            }
            for arg in &config.args {
                std_cmd.arg(arg);
            }
            for (k, v) in &config.env {
                std_cmd.env(k, v);
            }
            std_cmd.stdin(std::process::Stdio::piped());
            std_cmd.stdout(std::process::Stdio::piped());
            std_cmd.stderr(std::process::Stdio::null());

            let mut tokio_cmd = tokio::process::Command::from(std_cmd);
            tokio_cmd.kill_on_drop(true);
            let mut child = tokio_cmd
                .spawn()
                .map_err(|e| format!("Failed to spawn MCP server '{}': {}", config.name, e))?;

            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| "Failed to capture stdin for MCP server".to_string())?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| "Failed to capture stdout for MCP server".to_string())?;

            let responses: Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<JsonRpcResponse>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let resp_map = responses.clone();

            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                        if let Some(id_val) = &resp.id {
                            if let Some(id_num) = id_val.as_u64() {
                                let mut map = resp_map.lock().await;
                                if let Some(sender) = map.remove(&id_num) {
                                    let _ = sender.send(resp);
                                }
                            }
                        }
                    }
                }
            });

            McpTransportConnection::Stdio(StdioConnection {
                writer: Arc::new(Mutex::new(stdin)),
                responses,
                child: Arc::new(Mutex::new(child)),
            })
        } else if transport_type == "sse" {
            let base_url = config
                .url
                .clone()
                .ok_or_else(|| "Missing URL for SSE MCP server".to_string())?;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default();
            let post_url = format!("{}/message", base_url.trim_end_matches('/'));

            McpTransportConnection::Sse {
                base_url,
                post_endpoint: Arc::new(RwLock::new(post_url)),
                client,
            }
        } else {
            let endpoint = config
                .url
                .clone()
                .ok_or_else(|| "Missing URL for HTTP MCP server".to_string())?;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default();
            McpTransportConnection::HttpStream { endpoint, client }
        };

        let mut instance = McpServerInstance {
            config: config.clone(),
            capabilities: McpServerCapabilities::default(),
            connection,
            tools: Vec::new(),
            req_counter: AtomicU64::new(1),
        };

        instance.initialize().await?;

        let mut map = self.servers.write().await;
        map.insert(config.name.clone(), Arc::new(instance));
        Ok(())
    }

    pub async fn disconnect_server(&self, name: &str) -> Result<(), String> {
        let mut map = self.servers.write().await;
        if let Some(srv) = map.remove(name) {
            if let McpTransportConnection::Stdio(conn) = &srv.connection {
                let mut child = conn.child.lock().await;
                let _ = child.kill().await;
            }
            Ok(())
        } else {
            Err(format!("Server '{}' not connected", name))
        }
    }

    pub async fn list_all_tools(&self) -> Vec<McpToolWithServer> {
        let map = self.servers.read().await;
        let mut out = Vec::new();
        for (name, srv) in map.iter() {
            for t in &srv.tools {
                out.push(McpToolWithServer {
                    server_name: name.clone(),
                    tool: t.clone(),
                });
            }
        }
        out
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<McpCallToolResult, String> {
        let srv = {
            let map = self.servers.read().await;
            map.get(server_name)
                .cloned()
                .ok_or_else(|| format!("Server '{}' not found", server_name))?
        };

        let cfg = srv.config.clone();
        let is_stdio = matches!(srv.connection, McpTransportConnection::Stdio(_));
        let mut active_srv = srv;

        if let McpTransportConnection::Stdio(conn) = &active_srv.connection {
            if !conn.is_alive().await {
                self.connect_server(&cfg).await?;
                let map = self.servers.read().await;
                active_srv = map
                    .get(server_name)
                    .cloned()
                    .ok_or_else(|| format!("Server '{}' not found after reconnect", server_name))?;
            }
        }

        let call_params = McpCallToolParams {
            name: tool_name.to_string(),
            arguments: arguments.clone(),
        };

        let res = match active_srv
            .send_request("tools/call", Some(json!(call_params)))
            .await
        {
            Ok(v) => v,
            Err(err) if is_stdio => {
                self.connect_server(&cfg).await.map_err(|re| format!("{}; reconnect failed: {}", err, re))?;
                let map = self.servers.read().await;
                let reconnected = map
                    .get(server_name)
                    .cloned()
                    .ok_or_else(|| format!("Server '{}' missing after reconnect", server_name))?;
                let retry_params = McpCallToolParams {
                    name: tool_name.to_string(),
                    arguments,
                };
                reconnected
                    .send_request("tools/call", Some(json!(retry_params)))
                    .await?
            }
            Err(err) => return Err(err),
        };

        let parsed: McpCallToolResult = serde_json::from_value(res)
            .map_err(|e| format!("Failed to parse tools/call result: {}", e))?;
        Ok(parsed)
    }

    pub async fn health_check(&self, server_name: &str) -> Result<bool, String> {
        let map = self.servers.read().await;
        let srv = map
            .get(server_name)
            .ok_or_else(|| format!("Server '{}' not found", server_name))?;
        srv.ping().await
    }

    pub fn import_json_config(json_str: &str) -> Result<Vec<McpServerConfig>, String> {
        let parsed: McpConfigFile = serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse MCP config JSON: {}", e))?;

        let mut configs = Vec::new();
        for (name, raw) in parsed.mcp_servers {
            let transport = raw.transport.unwrap_or_else(|| {
                if raw.command.is_some() {
                    "stdio".to_string()
                } else {
                    "sse".to_string()
                }
            });

            configs.push(McpServerConfig {
                name,
                transport,
                command: raw.command,
                args: raw.args,
                env: raw.env,
                url: raw.url,
                enabled: raw.enabled,
            });
        }
        Ok(configs)
    }
}
