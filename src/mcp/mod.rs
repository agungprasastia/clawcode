//! Synchronous MCP client and tool registry.
//!
//! Local servers speak newline-delimited JSON-RPC over stdio; remote servers
//! use streamable HTTP. Connections are lazy: the first generation that needs
//! tools pays the handshake cost on its own thread, never the UI thread.

use crate::config::McpServerConfig;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(test)]
mod tests;

const PROTOCOL_VERSION: &str = "2025-06-18";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One tool advertised by a server.
#[derive(Clone, Debug)]
pub struct McpTool {
    pub server: String,
    pub name: String,
    pub exposed_name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Tool namespace and model-facing name used by OpenCode:
/// `${sanitized_server}_${sanitized_tool}`.
pub fn namespace(server: &str) -> String {
    server
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn exposed_name(server: &str, tool: &str) -> String {
    format!("{}_{}", namespace(server), namespace(tool))
}

/// Point-in-time connection state used by the TUI and server APIs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpServerStatus {
    pub name: String,
    pub enabled: bool,
    pub connected: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

/// MCP servers for a runtime. A registry can be shared by every generation;
/// individual servers are serialized by one lifecycle mutex each.
pub struct McpRegistry {
    servers: Vec<Arc<McpServerEntry>>,
}

struct McpServerEntry {
    name: String,
    config: McpServerConfig,
    cwd: PathBuf,
    client: Mutex<Option<McpClient>>,
    tools: Mutex<Vec<McpTool>>,
    loaded: Mutex<bool>,
    last_error: Mutex<Option<String>>,
    lifecycle: Mutex<()>,
    enabled: AtomicBool,
    connected: AtomicBool,
    tool_count: AtomicUsize,
}

impl McpRegistry {
    /// Build a registry for every configured server. Disabled servers stay
    /// present so the UI can connect them, but generation skips them.
    pub fn new(servers: &BTreeMap<String, McpServerConfig>, cwd: &Path) -> Option<Arc<Self>> {
        let entries = servers
            .iter()
            .map(|(name, config)| {
                Arc::new(McpServerEntry {
                    name: name.clone(),
                    config: config.clone(),
                    cwd: cwd.to_path_buf(),
                    client: Mutex::new(None),
                    tools: Mutex::new(Vec::new()),
                    loaded: Mutex::new(false),
                    last_error: Mutex::new(None),
                    lifecycle: Mutex::new(()),
                    enabled: AtomicBool::new(config.enabled),
                    connected: AtomicBool::new(false),
                    tool_count: AtomicUsize::new(0),
                })
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return None;
        }
        Some(Arc::new(Self { servers: entries }))
    }

    /// Snapshot every configured server without opening connections.
    pub fn statuses(&self) -> Vec<McpServerStatus> {
        self.servers.iter().map(|server| server.status()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.servers
            .iter()
            .map(|server| server.name.clone())
            .collect()
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.entry(name)
            .is_some_and(|entry| entry.enabled.load(Ordering::SeqCst))
    }

    pub fn is_connected(&self, name: &str) -> bool {
        self.entry(name)
            .is_some_and(|entry| entry.connected.load(Ordering::SeqCst))
    }

    pub fn tool_count(&self, name: &str) -> usize {
        self.entry(name)
            .map(|entry| entry.tool_count.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    /// Connect an enabled-or-disabled server and load its tool list.
    pub fn probe(&self, name: &str) -> Result<usize, String> {
        let entry = self
            .entry(name)
            .ok_or_else(|| format!("MCP server not found: {name}"))?;
        entry.enabled.store(true, Ordering::SeqCst);
        entry.connected.store(false, Ordering::SeqCst);
        entry.ensure_tools()?;
        Ok(entry.tool_count.load(Ordering::SeqCst))
    }

    /// Stop a connected server and keep it disabled until an explicit probe.
    pub fn disconnect(&self, name: &str) -> bool {
        let Some(entry) = self.entry(name) else {
            return false;
        };
        entry.disconnect();
        true
    }

    fn entry(&self, name: &str) -> Option<&Arc<McpServerEntry>> {
        self.servers.iter().find(|entry| entry.name == name)
    }

    /// All currently known tools, connecting servers that have not started.
    pub fn tools(&self) -> Vec<McpTool> {
        let mut tools = Vec::new();
        for server in &self.servers {
            if !server.enabled.load(Ordering::SeqCst) {
                continue;
            }
            if let Err(error) = server.ensure_tools() {
                tracing::warn!(server = %server.name, %error, "mcp server unavailable");
                continue;
            }
            tools.extend(
                server
                    .tools
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter()
                    .cloned(),
            );
        }
        tools
    }

    /// Provider tool schemas for all connected servers.
    pub fn tool_schemas(&self) -> Vec<Value> {
        self.tools()
            .into_iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.exposed_name,
                        "description": tool.description,
                        "parameters": normalized_schema(&tool.input_schema),
                    }
                })
            })
            .collect()
    }

    /// True when `name` belongs to a connected or connectable MCP server.
    pub fn has_tool(&self, name: &str) -> bool {
        self.find_tool(name).is_some()
    }

    /// Cheap check against the tools already fetched. Tool threads use this so
    /// a local tool never triggers an MCP handshake.
    pub fn has_known_tool(&self, name: &str) -> bool {
        self.servers.iter().any(|server| {
            server
                .tools
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .any(|tool| tool.exposed_name == name)
        })
    }

    /// Execute a tool by its model-facing name.
    pub fn call_tool(&self, name: &str, arguments: &Value) -> Result<String, String> {
        let (server, tool) = self
            .find_tool(name)
            .ok_or_else(|| format!("Unknown MCP tool: {name}"))?;
        let entry = self
            .servers
            .iter()
            .find(|entry| entry.name == server)
            .ok_or_else(|| format!("MCP server not found: {server}"))?;
        entry.call(&tool.name, arguments)
    }

    /// Last connection error for a server, used by diagnostics.
    pub fn status(&self, name: &str) -> Option<String> {
        self.entry(name).and_then(|entry| entry.error())
    }

    fn find_tool(&self, name: &str) -> Option<(String, McpTool)> {
        for server in &self.servers {
            if !server.enabled.load(Ordering::SeqCst) {
                continue;
            }
            if let Err(error) = server.ensure_tools() {
                tracing::warn!(server = %server.name, %error, "mcp server unavailable");
                continue;
            }
            let tools = server.tools.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(tool) = tools.iter().find(|tool| tool.exposed_name == name) {
                return Some((server.name.clone(), tool.clone()));
            }
        }
        None
    }
}

impl McpServerEntry {
    fn ensure_tools(&self) -> Result<(), String> {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|p| p.into_inner());
        if !self.enabled.load(Ordering::SeqCst) {
            return Err(format!("MCP server \"{}\" is disabled", self.name));
        }
        if *self.loaded.lock().unwrap_or_else(|p| p.into_inner()) {
            return Ok(());
        }
        let mut client = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if client.is_none() {
            match self.connect() {
                Ok(connected) => *client = Some(connected),
                Err(error) => {
                    self.fail(error.clone());
                    return Err(error);
                }
            }
        }
        let connected = client.as_mut().expect("client was just connected");
        let tools = match connected.list_tools() {
            Ok(tools) => tools,
            Err(error) => {
                *client = None;
                *self.loaded.lock().unwrap_or_else(|p| p.into_inner()) = false;
                self.fail(error.clone());
                return Err(error);
            }
        };
        let tool_count = tools.len();
        *self.tools.lock().unwrap_or_else(|p| p.into_inner()) = tools
            .into_iter()
            .map(|(name, description, input_schema)| McpTool {
                server: self.name.clone(),
                exposed_name: exposed_name(&self.name, &name),
                name,
                description,
                input_schema,
            })
            .collect();
        *self.loaded.lock().unwrap_or_else(|p| p.into_inner()) = true;
        *self.last_error.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.tool_count.store(tool_count, Ordering::SeqCst);
        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn connect(&self) -> Result<McpClient, String> {
        McpClient::connect(&self.name, &self.config, &self.cwd)
    }

    fn call(&self, tool_name: &str, arguments: &Value) -> Result<String, String> {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|p| p.into_inner());
        if !self.enabled.load(Ordering::SeqCst) {
            return Err(format!("MCP server \"{}\" is disabled", self.name));
        }
        let mut client = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if client.is_none() {
            *client = Some(self.connect()?);
        }
        client
            .as_mut()
            .expect("client was just connected")
            .call_tool(tool_name, arguments)
    }

    fn disconnect(&self) {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|p| p.into_inner());
        *self.client.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.tools.lock().unwrap_or_else(|p| p.into_inner()).clear();
        *self.loaded.lock().unwrap_or_else(|p| p.into_inner()) = false;
        *self.last_error.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.enabled.store(false, Ordering::SeqCst);
        self.connected.store(false, Ordering::SeqCst);
        self.tool_count.store(0, Ordering::SeqCst);
    }

    fn fail(&self, error: String) {
        *self.last_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(error);
        self.connected.store(false, Ordering::SeqCst);
        self.tool_count.store(0, Ordering::SeqCst);
    }

    fn error(&self) -> Option<String> {
        self.last_error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn status(&self) -> McpServerStatus {
        McpServerStatus {
            name: self.name.clone(),
            enabled: self.enabled.load(Ordering::SeqCst),
            connected: self.connected.load(Ordering::SeqCst),
            tool_count: self.tool_count.load(Ordering::SeqCst),
            error: self.error(),
        }
    }
}

fn normalized_schema(schema: &Value) -> Value {
    if schema.is_object() {
        schema.clone()
    } else {
        json!({"type": "object", "properties": {}})
    }
}

struct McpClient {
    server: String,
    next_id: u64,
    transport: Transport,
}

enum Transport {
    Stdio {
        child: Child,
        stdin: ChildStdin,
        lines: Receiver<String>,
    },
    Http {
        agent: ureq::Agent,
        url: String,
        headers: BTreeMap<String, String>,
        session_id: Option<String>,
    },
}

impl McpClient {
    fn connect(server: &str, config: &McpServerConfig, cwd: &Path) -> Result<Self, String> {
        let transport = if let Some(url) = &config.url {
            Transport::Http {
                agent: ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build(),
                url: url.clone(),
                headers: config.headers.clone(),
                session_id: None,
            }
        } else {
            let Some(program) = config.command.first() else {
                return Err(format!("MCP server `{server}` has no command"));
            };
            let mut command = Command::new(program);
            command
                .args(&config.command[1..])
                .current_dir(cwd)
                .envs(&config.environment)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut child = command
                .spawn()
                .map_err(|error| format!("failed to start MCP server `{server}`: {error}"))?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| format!("MCP server `{server}` has no stdin"))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| format!("MCP server `{server}` has no stdout"))?;
            let (sender, lines) = mpsc::channel();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
            Transport::Stdio {
                child,
                stdin,
                lines,
            }
        };
        let mut client = Self {
            server: server.to_string(),
            next_id: 1,
            transport,
        };
        client.initialize()?;
        Ok(client)
    }

    fn initialize(&mut self) -> Result<(), String> {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "clawcode",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        )?;
        if let Some(version) = result.get("protocolVersion").and_then(Value::as_str) {
            tracing::debug!(
                server = %self.server,
                version,
                "mcp protocol negotiated"
            );
        }
        self.notify("notifications/initialized", json!({}))?;
        Ok(())
    }

    fn list_tools(&mut self) -> Result<Vec<(String, String, Value)>, String> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = self.request("tools/list", params)?;
            let Some(items) = result.get("tools").and_then(Value::as_array) else {
                return Ok(tools);
            };
            for item in items {
                let Some(name) = item.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let description = item
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let input_schema = item
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
                tools.push((name.to_string(), description, input_schema));
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|cursor| !cursor.is_empty());
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<String, String> {
        let result = self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
        )?;
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let text = tool_result_text(&result);
        if is_error {
            Err(if text.is_empty() {
                "MCP tool returned an error".to_string()
            } else {
                text
            })
        } else {
            Ok(text)
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        match &mut self.transport {
            Transport::Stdio { stdin, lines, .. } => {
                write_stdio(stdin, &request)?;
                loop {
                    let line = lines
                        .recv_timeout(REQUEST_TIMEOUT)
                        .map_err(|_| format!("MCP server `{}` timed out", self.server))?;
                    let Ok(value) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if value.get("id").and_then(Value::as_u64) == Some(id) {
                        return rpc_result(&self.server, value);
                    }
                    if value.get("method").is_some() && value.get("id").is_some() {
                        let _ = write_stdio(
                            stdin,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "error": {
                                    "code": -32601,
                                    "message": format!(
                                        "Client method not supported: {}",
                                        value["method"].as_str().unwrap_or_default()
                                    )
                                }
                            }),
                        );
                    }
                }
            }
            Transport::Http {
                agent,
                url,
                headers,
                session_id,
            } => {
                let response = http_send(agent, url, headers, session_id.as_deref(), &request)?;
                if let Some(value) = response.session_id {
                    *session_id = Some(value);
                }
                let body = response.body;
                if body.trim().is_empty() {
                    return Err(format!("MCP server `{}` returned no response", self.server));
                }
                let values = parse_http_body(&body)?;
                for value in values {
                    if value.get("id").and_then(Value::as_u64) == Some(id) {
                        return rpc_result(&self.server, value);
                    }
                }
                Err(format!(
                    "MCP server `{}` sent no response for {method}",
                    self.server
                ))
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        match &mut self.transport {
            Transport::Stdio { stdin, .. } => write_stdio(stdin, &notification),
            Transport::Http {
                agent,
                url,
                headers,
                session_id,
            } => {
                let response =
                    http_send(agent, url, headers, session_id.as_deref(), &notification)?;
                if let Some(value) = response.session_id {
                    *session_id = Some(value);
                }
                Ok(())
            }
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        if let Transport::Stdio { child, .. } = &mut self.transport {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn write_stdio(stdin: &mut ChildStdin, value: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *stdin, value)
        .map_err(|error| format!("failed to encode MCP request: {error}"))?;
    stdin
        .write_all(b"\n")
        .and_then(|()| stdin.flush())
        .map_err(|error| format!("failed to write MCP request: {error}"))
}

fn rpc_result(server: &str, value: Value) -> Result<Value, String> {
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(format!("MCP server `{server}`: {message}"));
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

struct HttpResponse {
    body: String,
    session_id: Option<String>,
}

fn http_send(
    agent: &ureq::Agent,
    url: &str,
    headers: &BTreeMap<String, String>,
    session_id: Option<&str>,
    body: &Value,
) -> Result<HttpResponse, String> {
    let mut request = agent
        .post(url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream");
    for (name, value) in headers {
        request = request.set(name, value);
    }
    if let Some(session_id) = session_id {
        request = request.set("Mcp-Session-Id", session_id);
    }
    let response = match request.send_json(body) {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_default();
            return Err(format!("MCP HTTP {code}: {}", body.trim()));
        }
        Err(error) => return Err(format!("MCP HTTP request failed: {error}")),
    };
    let session_id = response.header("Mcp-Session-Id").map(str::to_string);
    let body = response
        .into_string()
        .map_err(|error| format!("failed to read MCP response: {error}"))?;
    Ok(HttpResponse { body, session_id })
}

fn parse_http_body(body: &str) -> Result<Vec<Value>, String> {
    let trimmed = body.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let value: Value = serde_json::from_str(trimmed)
            .map_err(|error| format!("invalid MCP JSON response: {error}"))?;
        return Ok(match value {
            Value::Array(values) => values,
            value => vec![value],
        });
    }
    let mut values = Vec::new();
    for event in trimmed.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            values.push(value);
        }
    }
    if values.is_empty() {
        return Err("MCP response contained no JSON events".to_string());
    }
    Ok(values)
}

fn tool_result_text(result: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        for part in content {
            let kind = part.get("type").and_then(Value::as_str).unwrap_or("text");
            match kind {
                "text" => {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    }
                }
                "resource" => {
                    let resource = part.get("resource").unwrap_or(&Value::Null);
                    if let Some(text) = resource.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    } else if let Some(uri) = resource.get("uri").and_then(Value::as_str) {
                        parts.push(uri.to_string());
                    }
                }
                "image" | "audio" => {
                    let mime = part.get("mimeType").and_then(Value::as_str).unwrap_or(kind);
                    parts.push(format!("[{mime}]"));
                }
                _ => parts.push(part.to_string()),
            }
        }
    }
    if parts.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        parts.push(structured.to_string());
    }
    parts.join("\n")
}
