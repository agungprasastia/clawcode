//! Agent Client Protocol server over newline-delimited JSON-RPC on stdio.
//!
//! This is the synchronous ACP foundation: session lifecycle, prompt streaming,
//! cancellation, history replay, and tool-call updates. Editors start it with
//! `clawcode acp`. The runtime remains the source of truth for generation.

use crate::cli::opencode::{CliError, data_dir, load_config, select_model};
use crate::cli::spec::Invocation;
use crate::config::McpServerConfig;
use crate::persistence::{Db, WriterHandle};
use crate::runtime::client::{GenerationRequest, RuntimeClient};
use crate::runtime::{EventBus, RuntimeEvent};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::io::{self, BufRead, BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

const PROMPT_TIMEOUT: Duration = Duration::from_secs(600);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

pub fn run(invocation: &Invocation) -> Result<(), CliError> {
    let mut server = AcpServer::new(invocation)?;
    server.serve()?;
    server.shutdown()?;
    Ok(())
}

struct AcpServer {
    runtime: Option<RuntimeClient>,
    db: Db,
    bus: EventBus,
    provider: String,
    model: String,
    agent: String,
    models: Vec<(String, String)>,
    base_mcp: BTreeMap<String, McpServerConfig>,
    session_cwds: HashMap<i64, String>,
    session_agents: HashMap<i64, String>,
    session_models: HashMap<i64, (String, String)>,
    session_mcps: HashMap<i64, Arc<crate::mcp::McpRegistry>>,
}

impl AcpServer {
    fn new(invocation: &Invocation) -> Result<Self, CliError> {
        let config = load_config()?;
        let (provider, model) = select_model(&config, invocation.flag("model"))?;
        if provider.is_empty() || model.is_empty() {
            return Err(CliError(
                "No model configured. Add a provider and model to your configuration.".into(),
            ));
        }
        let data = data_dir();
        fs::create_dir_all(&data)?;
        let db_path = data.join("clawcode.db");
        let mut models = Vec::new();
        for (provider_id, provider_config) in &config.providers {
            for model_id in provider_config.models.keys() {
                models.push((provider_id.clone(), model_id.clone()));
            }
        }
        if models.is_empty() && !provider.is_empty() && !model.is_empty() {
            models.push((provider.clone(), model.clone()));
        }
        let runtime_db = Db::open(&db_path)?;
        let writer = WriterHandle::spawn(Db::open(&db_path)?);
        let db = Db::open(&db_path)?;
        let bus = EventBus::new();
        let mcp_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let base_mcp = config.mcp.clone();
        let mcp = crate::mcp::McpRegistry::new(&config.mcp, &mcp_cwd);
        let router = crate::adapters::ConfiguredRouter::new(config);
        let runtime =
            RuntimeClient::spawn_with_mcp(runtime_db, writer, Box::new(router), bus.clone(), mcp);
        Ok(Self {
            runtime: Some(runtime),
            db,
            bus,
            provider,
            model,
            agent: invocation.flag("agent").unwrap_or("build").to_string(),
            models,
            base_mcp,
            session_cwds: HashMap::new(),
            session_agents: HashMap::new(),
            session_models: HashMap::new(),
            session_mcps: HashMap::new(),
        })
    }

    fn serve(&mut self) -> Result<(), CliError> {
        let (sender, receiver) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });

        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        let mut queued = VecDeque::new();

        loop {
            let value = if let Some(value) = queued.pop_front() {
                value
            } else {
                match receiver.recv() {
                    Ok(line) => match serde_json::from_str::<Value>(&line) {
                        Ok(value) => value,
                        Err(error) => {
                            write_json(
                                &mut writer,
                                &json!({
                                    "jsonrpc": "2.0",
                                    "id": Value::Null,
                                    "error": {"code": -32700, "message": format!("Parse error: {error}")},
                                }),
                            )?;
                            continue;
                        }
                    },
                    Err(_) => break,
                }
            };

            let id = value.get("id").cloned();
            let method = value
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let params = value.get("params").cloned().unwrap_or_else(|| json!({}));

            if id.is_none() {
                if method == "session/cancel"
                    && let Some(session_id) = params
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .and_then(|id| parse_session_id(id).ok())
                {
                    self.cancel(session_id);
                }
                continue;
            }

            let Some(id) = id else {
                continue;
            };
            if method == "session/prompt" {
                self.prompt(id, params, &mut queued, &receiver, &mut writer)?;
                continue;
            }

            let response = match self.handle_request(&method, &params, &mut writer) {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(error) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": error.to_json(),
                }),
            };
            write_json(&mut writer, &response)?;
        }
        Ok(())
    }

    fn handle_request(
        &mut self,
        method: &str,
        params: &Value,
        writer: &mut impl Write,
    ) -> Result<Value, RpcError> {
        match method {
            "initialize" => {
                let client_capabilities = params.get("clientCapabilities");
                let terminal_auth = client_capabilities
                    .and_then(|capabilities| capabilities.get("auth"))
                    .and_then(|auth| auth.get("terminal"))
                    .and_then(Value::as_bool)
                    == Some(true);
                let terminal_meta = client_capabilities
                    .and_then(|capabilities| capabilities.get("_meta"))
                    .and_then(|meta| meta.get("terminal-auth"))
                    .and_then(Value::as_bool)
                    == Some(true);
                let mut auth = json!({
                    "id": "clawcode-login",
                    "name": "Login with clawcode",
                    "description": "Run `clawcode auth login` in the terminal"
                });
                if terminal_auth {
                    auth["type"] = json!("terminal");
                    auth["args"] = json!(["--login"]);
                }
                if terminal_meta {
                    auth["_meta"] = json!({
                        "terminal-auth": {
                            "command": "clawcode",
                            "args": ["auth", "login"],
                            "label": "Clawcode Login"
                        }
                    });
                }
                Ok(json!({
                    "protocolVersion": 1,
                    "agentCapabilities": {
                        "loadSession": true,
                        "mcpCapabilities": {
                            "http": true,
                            "sse": false,
                        },
                        "promptCapabilities": {
                            "embeddedContext": true,
                            "image": false,
                        },
                        "sessionCapabilities": {
                            "close": {},
                            "delete": {},
                            "fork": {},
                            "list": {},
                            "resume": {},
                        },
                    },
                    "authMethods": [auth],
                    "agentInfo": {
                        "name": "clawcode",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }))
            }
            "authenticate" => {
                let method_id = params
                    .get("methodId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if method_id == "clawcode-login" {
                    Ok(json!({}))
                } else {
                    Err(RpcError::invalid_params(format!(
                        "Unknown auth method: {method_id}"
                    )))
                }
            }
            "session/new" => {
                let cwd = cwd_from_params(params)?;
                let mcp = session_mcp_registry(&self.base_mcp, params, &cwd)?;
                let title = params
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or("ACP session");
                let display_name = workspace_display_name(&cwd);
                let workspace = self
                    .db
                    .create_workspace(&cwd, display_name)
                    .map_err(RpcError::from_display)?;
                let runtime = self
                    .runtime
                    .as_ref()
                    .ok_or_else(|| RpcError::internal("runtime is not available"))?;
                let session_id = runtime
                    .create_session(workspace.id, title)
                    .map_err(RpcError::from_display)?;
                self.session_cwds.insert(session_id, cwd);
                self.session_agents.insert(session_id, self.agent.clone());
                self.session_models
                    .insert(session_id, (self.provider.clone(), self.model.clone()));
                if let Some(mcp) = mcp {
                    self.session_mcps.insert(session_id, mcp);
                }
                Ok(json!({
                    "sessionId": session_id_string(session_id),
                    "configOptions": self.config_options(session_id),
                }))
            }
            "session/load" => {
                let session_id = session_id_from_params(params)?;
                let cwd = cwd_from_params(params)?;
                let mcp = session_mcp_registry(&self.base_mcp, params, &cwd)?;
                self.require_session(session_id)?;
                self.require_session_cwd(session_id, &cwd)?;
                self.activate_workspace(session_id, &cwd)?;
                self.session_cwds.insert(session_id, cwd);
                if let Some(mcp) = mcp {
                    self.session_mcps.insert(session_id, mcp);
                } else {
                    self.session_mcps.remove(&session_id);
                }
                self.hydrate_session(session_id)?;
                self.replay_history(session_id, writer)?;
                Ok(json!({"configOptions": self.config_options(session_id)}))
            }
            "session/list" => {
                let filter = match params.get("cwd") {
                    Some(Value::Null) | None => None,
                    Some(Value::String(cwd)) => {
                        if !Path::new(cwd).is_absolute() && !cwd.starts_with('/') {
                            return Err(RpcError::invalid_field(
                                format!("cwd must be an absolute path: {cwd}"),
                                "cwd",
                            ));
                        }
                        Some(cwd.clone())
                    }
                    Some(_) => return Err(RpcError::invalid_field("cwd must be a string", "cwd")),
                };
                let sessions = self
                    .db
                    .session_summaries(false, 100)
                    .map_err(RpcError::from_display)?;
                let mut values = Vec::new();
                for session in sessions {
                    let cwd = self
                        .cwd_for_session(session.id)
                        .map_err(RpcError::from_display)?
                        .unwrap_or_else(current_dir);
                    if filter
                        .as_deref()
                        .is_some_and(|filter| !same_path(&cwd, filter))
                    {
                        continue;
                    }
                    values.push(json!({
                        "sessionId": session_id_string(session.id),
                        "cwd": cwd,
                        "title": session.title,
                        "updatedAt": session.updated_at,
                    }));
                }
                Ok(json!({"sessions": values}))
            }
            "session/delete" => {
                let session_id = session_id_from_params(params)?;
                self.cancel(session_id);
                self.db
                    .delete_session(session_id)
                    .map_err(RpcError::from_display)?;
                self.session_cwds.remove(&session_id);
                self.session_agents.remove(&session_id);
                self.session_models.remove(&session_id);
                self.session_mcps.remove(&session_id);
                Ok(json!({}))
            }
            "session/resume" => {
                let session_id = session_id_from_params(params)?;
                let cwd = cwd_from_params(params)?;
                let mcp = session_mcp_registry(&self.base_mcp, params, &cwd)?;
                self.require_session(session_id)?;
                self.require_session_cwd(session_id, &cwd)?;
                self.activate_workspace(session_id, &cwd)?;
                self.session_cwds.insert(session_id, cwd);
                if let Some(mcp) = mcp {
                    self.session_mcps.insert(session_id, mcp);
                } else {
                    self.session_mcps.remove(&session_id);
                }
                self.hydrate_session(session_id)?;
                Ok(json!({"configOptions": self.config_options(session_id)}))
            }
            "session/fork" => {
                let session_id = session_id_from_params(params)?;
                let cwd = cwd_from_params(params)?;
                let mcp = session_mcp_registry(&self.base_mcp, params, &cwd)?;
                self.require_session(session_id)?;
                self.require_session_cwd(session_id, &cwd)?;
                self.hydrate_session(session_id)?;
                let agent = self.session_agent(session_id);
                let (provider, model) = self.session_model(session_id);
                let forked = self
                    .db
                    .fork_session(session_id)
                    .map_err(RpcError::from_display)?;
                self.session_cwds.insert(forked.id, cwd);
                self.session_agents.insert(forked.id, agent);
                self.session_models.insert(forked.id, (provider, model));
                if let Some(mcp) = mcp {
                    self.session_mcps.insert(forked.id, mcp);
                }
                Ok(json!({
                    "sessionId": session_id_string(forked.id),
                    "configOptions": self.config_options(forked.id),
                }))
            }
            "session/close" => {
                let session_id = session_id_from_params(params)?;
                self.cancel(session_id);
                self.session_mcps.remove(&session_id);
                Ok(json!({}))
            }
            "session/set_mode" => {
                let session_id = session_id_from_params(params)?;
                self.require_session(session_id)?;
                let mode = params
                    .get("modeId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if mode != "plan" && mode != "build" {
                    return Err(RpcError::invalid_params(format!("Unknown mode: {mode}")));
                }
                self.hydrate_session(session_id)?;
                self.session_agents.insert(session_id, mode.to_string());
                Ok(json!({}))
            }
            "session/set_config_option" => {
                let session_id = session_id_from_params(params)?;
                self.require_session(session_id)?;
                self.hydrate_session(session_id)?;
                let config_id = params
                    .get("configId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let value = params
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match config_id {
                    "model" => {
                        let Some((provider, model)) = value.split_once('/') else {
                            return Err(RpcError::invalid_field(
                                "Model must use the provider/model form",
                                "value",
                            ));
                        };
                        if provider.is_empty() || model.is_empty() {
                            return Err(RpcError::invalid_field(
                                "Model must use the provider/model form",
                                "value",
                            ));
                        }
                        if !self.models.is_empty()
                            && !self.models.iter().any(|(option_provider, option_model)| {
                                option_provider == provider && option_model == model
                            })
                        {
                            return Err(RpcError::invalid_field(
                                format!("Unknown model: {value}"),
                                "value",
                            ));
                        }
                        self.session_models
                            .insert(session_id, (provider.to_string(), model.to_string()));
                    }
                    "mode" => {
                        if value != "plan" && value != "build" {
                            return Err(RpcError::invalid_field(
                                format!("Unknown mode: {value}"),
                                "value",
                            ));
                        }
                        self.session_agents.insert(session_id, value.to_string());
                    }
                    _ => {
                        return Err(RpcError::invalid_field(
                            format!("Unknown config option: {config_id}"),
                            "configId",
                        ));
                    }
                }
                Ok(json!({"configOptions": self.config_options(session_id)}))
            }
            _ => Err(RpcError {
                code: -32601,
                message: format!("Method not found: {method}"),
                data: None,
            }),
        }
    }

    fn prompt(
        &mut self,
        id: Value,
        params: Value,
        queued: &mut VecDeque<Value>,
        receiver: &Receiver<String>,
        writer: &mut impl Write,
    ) -> Result<(), CliError> {
        let session_id = match session_id_from_params(&params) {
            Ok(session_id) => session_id,
            Err(error) => return self.write_error(writer, id, error),
        };
        if let Err(error) = self.require_session(session_id) {
            return self.write_error(writer, id, error);
        }
        if let Err(error) = self.hydrate_session(session_id) {
            return self.write_error(writer, id, error);
        }
        let prompt = match prompt_text(&params) {
            Ok(prompt) if !prompt.trim().is_empty() => prompt,
            Ok(_) => {
                return self.write_error(
                    writer,
                    id,
                    RpcError::invalid_params("Prompt contains no text"),
                );
            }
            Err(error) => return self.write_error(writer, id, error),
        };
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| self.session_cwds.get(&session_id).cloned())
            .unwrap_or_else(current_dir);
        self.session_cwds.insert(session_id, cwd);

        let Some(runtime) = self.runtime.as_ref() else {
            return self.write_error(writer, id, RpcError::internal("runtime is not available"));
        };
        let agent = self.session_agent(session_id);
        let (provider, model) = self.session_model(session_id);
        let mcp = self.session_mcps.get(&session_id).cloned();
        let (subscription_id, events) = self.bus.subscribe(Some(session_id));
        if let Err(error) = runtime.start_generation_request(GenerationRequest {
            session_id,
            agent_mode: &agent,
            provider: &provider,
            model: &model,
            prompt: &prompt,
            mcp,
        }) {
            self.bus.unsubscribe(subscription_id);
            return self.write_error(writer, id, RpcError::internal(error));
        }

        let started = Instant::now();
        let mut cancelled = false;
        let outcome = loop {
            loop {
                match receiver.try_recv() {
                    Ok(line) => {
                        let Ok(value) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        if value.get("method").and_then(Value::as_str) == Some("session/cancel") {
                            let requested = value
                                .get("params")
                                .and_then(|params| params.get("sessionId"))
                                .and_then(Value::as_str)
                                .and_then(|id| parse_session_id(id).ok());
                            if requested == Some(session_id) {
                                cancelled = true;
                                self.cancel(session_id);
                            }
                        } else {
                            queued.push_back(value);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            if cancelled {
                break PromptOutcome::Cancelled;
            }
            if started.elapsed() > PROMPT_TIMEOUT {
                self.cancel(session_id);
                break PromptOutcome::Failed("generation timed out".to_string());
            }
            match events.recv_timeout(POLL_INTERVAL) {
                Ok(event) => {
                    if event.kind == "generation_finished" {
                        let status = serde_json::from_str::<Value>(&event.payload_json)
                            .ok()
                            .and_then(|payload| {
                                payload
                                    .get("status")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            })
                            .unwrap_or_else(|| "completed".to_string());
                        break match status.as_str() {
                            "completed" => PromptOutcome::Completed,
                            "cancelled" => PromptOutcome::Cancelled,
                            other => PromptOutcome::Failed(format!("generation {other}")),
                        };
                    }
                    if event.kind == "error" {
                        let message = serde_json::from_str::<Value>(&event.payload_json)
                            .ok()
                            .and_then(|payload| {
                                payload
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                            })
                            .unwrap_or_else(|| "generation failed".to_string());
                        break PromptOutcome::Failed(message);
                    }
                    if let Some(notification) = translate_event(&event, session_id) {
                        write_json(writer, &notification)?;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break PromptOutcome::Failed(
                        "runtime stopped before the generation completed".to_string(),
                    );
                }
            }
        };
        self.bus.unsubscribe(subscription_id);

        match outcome {
            PromptOutcome::Completed => write_json(
                writer,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"stopReason": "end_turn"},
                }),
            )?,
            PromptOutcome::Cancelled => write_json(
                writer,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"stopReason": "cancelled"},
                }),
            )?,
            PromptOutcome::Failed(message) => {
                write_json(
                    writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32000, "message": message},
                    }),
                )?;
            }
        }
        Ok(())
    }

    fn replay_history(&self, session_id: i64, writer: &mut impl Write) -> Result<(), RpcError> {
        let messages = self
            .db
            .messages(session_id)
            .map_err(RpcError::from_display)?;
        for message in messages {
            let update = match message.role.as_str() {
                "user" => json!({
                    "sessionUpdate": "user_message_chunk",
                    "content": {"type": "text", "text": message.content},
                }),
                "assistant" => json!({
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": message.content},
                }),
                _ => continue,
            };
            let notification = json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": session_id_string(session_id),
                    "update": update,
                },
            });
            write_json(writer, &notification).map_err(RpcError::from_display)?;
        }
        Ok(())
    }

    fn require_session(&self, session_id: i64) -> Result<(), RpcError> {
        let exists = self
            .db
            .session(session_id)
            .map_err(RpcError::from_display)?
            .is_some();
        if exists {
            Ok(())
        } else {
            Err(RpcError::invalid_params(format!(
                "Session not found: {}",
                session_id_string(session_id)
            )))
        }
    }

    fn activate_workspace(&self, session_id: i64, cwd: &str) -> Result<(), RpcError> {
        let workspace = self
            .db
            .create_workspace(cwd, workspace_display_name(cwd))
            .map_err(RpcError::from_display)?;
        self.db
            .set_session_workspace(session_id, workspace.id)
            .map_err(RpcError::from_display)
    }

    fn cwd_for_session(&self, session_id: i64) -> Result<Option<String>, rusqlite::Error> {
        if let Some(cwd) = self.session_cwds.get(&session_id) {
            return Ok(Some(cwd.clone()));
        }
        let Some(session) = self.db.session(session_id)? else {
            return Ok(None);
        };
        let Some(workspace) = self.db.workspace(session.workspace_id)? else {
            return Ok(None);
        };
        let root = workspace.root_path.trim();
        Ok(Some(if root.is_empty() {
            current_dir()
        } else {
            root.to_string()
        }))
    }

    fn require_session_cwd(&self, session_id: i64, cwd: &str) -> Result<(), RpcError> {
        let actual = self
            .cwd_for_session(session_id)
            .map_err(RpcError::from_display)?
            .unwrap_or_else(current_dir);
        if same_path(&actual, cwd) {
            return Ok(());
        }
        Err(RpcError::invalid_field(
            format!(
                "Session {} belongs to {actual}, not {cwd}",
                session_id_string(session_id)
            ),
            "cwd",
        )
        .with_data(json!({
            "sessionId": session_id_string(session_id),
            "cwd": actual,
        })))
    }

    /// Restore the state that is persisted with the session but is not part of
    /// the ACP handshake: its directory, mode, and model.
    fn hydrate_session(&mut self, session_id: i64) -> Result<(), RpcError> {
        if !self.session_cwds.contains_key(&session_id)
            && let Some(cwd) = self
                .cwd_for_session(session_id)
                .map_err(RpcError::from_display)?
        {
            self.session_cwds.insert(session_id, cwd);
        }
        let last = self
            .db
            .last_generation(session_id)
            .map_err(RpcError::from_display)?;
        if !self.session_agents.contains_key(&session_id) {
            let agent = last
                .as_ref()
                .map(|generation| generation.agent_mode.clone())
                .unwrap_or_else(|| self.agent.clone());
            self.session_agents.insert(session_id, agent);
        }
        if !self.session_models.contains_key(&session_id) {
            let model = last
                .map(|generation| (generation.provider, generation.model))
                .unwrap_or_else(|| (self.provider.clone(), self.model.clone()));
            self.session_models.insert(session_id, model);
        }
        Ok(())
    }

    fn session_agent(&self, session_id: i64) -> String {
        self.session_agents
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| self.agent.clone())
    }

    fn session_model(&self, session_id: i64) -> (String, String) {
        self.session_models
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| (self.provider.clone(), self.model.clone()))
    }

    fn config_options(&self, session_id: i64) -> Value {
        let (provider, model) = self.session_model(session_id);
        let current_model = format!("{provider}/{model}");
        let mut model_options = self
            .models
            .iter()
            .map(|(provider, model)| {
                json!({
                    "value": format!("{provider}/{model}"),
                    "name": format!("{provider}/{model}"),
                })
            })
            .collect::<Vec<_>>();
        if !model_options
            .iter()
            .any(|option| option["value"] == current_model)
        {
            model_options.push(json!({
                "value": current_model,
                "name": current_model,
            }));
        }
        let mode = self.session_agent(session_id);
        json!([
            {
                "id": "model",
                "name": "Model",
                "category": "model",
                "type": "select",
                "currentValue": current_model,
                "options": model_options,
            },
            {
                "id": "mode",
                "name": "Session Mode",
                "category": "mode",
                "type": "select",
                "currentValue": mode,
                "options": [
                    {"value": "plan", "name": "Plan"},
                    {"value": "build", "name": "Build"},
                ],
            }
        ])
    }

    fn cancel(&self, session_id: i64) {
        if let Some(runtime) = self.runtime.as_ref() {
            let _ = runtime.cancel_generation(session_id);
        }
    }

    fn write_error(
        &self,
        writer: &mut impl Write,
        id: Value,
        error: RpcError,
    ) -> Result<(), CliError> {
        write_json(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": error.to_json(),
            }),
        )
    }

    fn shutdown(&mut self) -> Result<(), CliError> {
        if let Some(runtime) = self.runtime.take() {
            let _ = runtime.shutdown();
        }
        Ok(())
    }
}

enum PromptOutcome {
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    fn invalid_field(message: impl Into<String>, field: &str) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: Some(json!({ "field": field })),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }

    fn from_display(error: impl std::fmt::Display) -> Self {
        Self::internal(error.to_string())
    }

    fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    fn to_json(&self) -> Value {
        let mut error = json!({
            "code": self.code,
            "message": self.message,
        });
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        error
    }
}

fn write_json(writer: &mut impl Write, value: &Value) -> Result<(), CliError> {
    serde_json::to_writer(&mut *writer, value)
        .map_err(|error| CliError(format!("failed to encode ACP message: {error}")))?;
    writer
        .write_all(b"\n")
        .map_err(|error| CliError(format!("failed to write ACP message: {error}")))?;
    writer
        .flush()
        .map_err(|error| CliError(format!("failed to flush ACP message: {error}")))
}

fn session_id_from_params(params: &Value) -> Result<i64, RpcError> {
    params
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("Missing sessionId"))
        .and_then(parse_session_id)
}

fn parse_session_id(value: &str) -> Result<i64, RpcError> {
    let raw = value.strip_prefix("ses_").unwrap_or(value);
    raw.parse::<i64>()
        .map_err(|_| RpcError::invalid_params(format!("Invalid session id: {value}")))
}

fn session_id_string(session_id: i64) -> String {
    format!("ses_{session_id}")
}

fn cwd_from_params(params: &Value) -> Result<String, RpcError> {
    let cwd = params
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if cwd.is_empty() {
        return Err(RpcError::invalid_field("Missing cwd", "cwd"));
    }
    if !Path::new(cwd).is_absolute() && !cwd.starts_with('/') {
        return Err(RpcError::invalid_field(
            format!("cwd must be an absolute path: {cwd}"),
            "cwd",
        ));
    }
    Ok(cwd.to_string())
}

fn session_mcp_registry(
    base: &BTreeMap<String, McpServerConfig>,
    params: &Value,
    cwd: &str,
) -> Result<Option<Arc<crate::mcp::McpRegistry>>, RpcError> {
    let mut configs = base.clone();
    if let Some(attached) = parse_acp_mcp_servers(params)? {
        configs.extend(attached);
    }
    if configs.is_empty() {
        return Ok(None);
    }
    Ok(crate::mcp::McpRegistry::new(&configs, Path::new(cwd)))
}

/// Parse the ACP `mcpServers` array. Typed `http` servers map to remote MCP;
/// untyped entries map to local stdio servers. `sse`/`acp` are not supported.
fn parse_acp_mcp_servers(
    params: &Value,
) -> Result<Option<BTreeMap<String, McpServerConfig>>, RpcError> {
    let Some(value) = params.get("mcpServers") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(servers) = value.as_array() else {
        return Err(RpcError::invalid_field(
            "mcpServers must be an array",
            "mcpServers",
        ));
    };
    if servers.is_empty() {
        return Ok(None);
    }
    let mut configs = BTreeMap::new();
    for server in servers {
        let Some(object) = server.as_object() else {
            return Err(RpcError::invalid_field(
                "MCP server entries must be objects",
                "mcpServers",
            ));
        };
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| RpcError::invalid_field("MCP server is missing a name", "mcpServers"))?;
        let kind = object.get("type").and_then(Value::as_str);
        let config = match kind {
            None => {
                let command = match object.get("command").and_then(Value::as_str) {
                    Some(command) if !command.is_empty() => command.to_string(),
                    _ => {
                        return Err(RpcError::invalid_field(
                            format!("MCP server `{name}` is missing a command"),
                            "mcpServers",
                        ));
                    }
                };
                let mut command = vec![command];
                if let Some(args) = object.get("args") {
                    let Some(args) = args.as_array() else {
                        return Err(RpcError::invalid_field(
                            format!("MCP server `{name}` args must be an array"),
                            "mcpServers",
                        ));
                    };
                    for arg in args {
                        let Some(arg) = arg.as_str() else {
                            return Err(RpcError::invalid_field(
                                format!("MCP server `{name}` args must be strings"),
                                "mcpServers",
                            ));
                        };
                        command.push(arg.to_string());
                    }
                }
                McpServerConfig {
                    enabled: true,
                    command,
                    environment: parse_named_values(
                        object.get("env"),
                        &format!("MCP server `{name}` env"),
                    )?,
                    oauth: false,
                    ..McpServerConfig::default()
                }
            }
            Some("http") => {
                let url = object
                    .get("url")
                    .and_then(Value::as_str)
                    .filter(|url| !url.is_empty())
                    .ok_or_else(|| {
                        RpcError::invalid_field(
                            format!("MCP server `{name}` is missing a url"),
                            "mcpServers",
                        )
                    })?;
                McpServerConfig {
                    enabled: true,
                    url: Some(url.to_string()),
                    headers: parse_named_values(
                        object.get("headers"),
                        &format!("MCP server `{name}` headers"),
                    )?,
                    oauth: false,
                    ..McpServerConfig::default()
                }
            }
            Some(_) => {
                return Err(RpcError::invalid_field(
                    "Only stdio and HTTP MCP servers are supported",
                    "mcpServers",
                ));
            }
        };
        configs.insert(name.to_string(), config);
    }
    Ok(Some(configs))
}

fn parse_named_values(
    value: Option<&Value>,
    field: &str,
) -> Result<BTreeMap<String, String>, RpcError> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    if value.is_null() {
        return Ok(BTreeMap::new());
    }
    if let Some(object) = value.as_object() {
        return object
            .iter()
            .map(|(name, value)| {
                value
                    .as_str()
                    .map(|value| (name.clone(), value.to_string()))
                    .ok_or_else(|| {
                        RpcError::invalid_field(format!("{field}.{name} must be a string"), field)
                    })
            })
            .collect();
    }
    let Some(items) = value.as_array() else {
        return Err(RpcError::invalid_field(
            format!("{field} must be an array"),
            field,
        ));
    };
    let mut values = BTreeMap::new();
    for item in items {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                RpcError::invalid_field(format!("{field} entries need a name"), field)
            })?;
        let value = item.get("value").and_then(Value::as_str).ok_or_else(|| {
            RpcError::invalid_field(format!("{field} entries need a value"), field)
        })?;
        values.insert(name.to_string(), value.to_string());
    }
    Ok(values)
}

fn current_dir() -> String {
    std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

fn same_path(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let (Ok(left), Ok(right)) = (fs::canonicalize(left), fs::canonicalize(right)) else {
        return Path::new(left) == Path::new(right);
    };
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn workspace_display_name(cwd: &str) -> &str {
    Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("ACP workspace")
}

fn prompt_text(params: &Value) -> Result<String, RpcError> {
    let Some(blocks) = params.get("prompt") else {
        return Err(RpcError::invalid_params("Missing prompt"));
    };
    let Some(blocks) = blocks.as_array() else {
        return Err(RpcError::invalid_params("prompt must be an array"));
    };
    let mut text = String::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push_str(value);
                }
            }
            Some("resource") => {
                if let Some(value) = block
                    .get("resource")
                    .and_then(|resource| resource.get("text"))
                    .and_then(Value::as_str)
                {
                    text.push_str(value);
                }
            }
            _ => {}
        }
    }
    Ok(text)
}

fn translate_event(event: &RuntimeEvent, session_id: i64) -> Option<Value> {
    let payload = serde_json::from_str::<Value>(&event.payload_json).ok()?;
    let update = match event.kind.as_str() {
        "text_delta" => {
            let delta = payload.get("delta").and_then(Value::as_str)?;
            json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": delta},
            })
        }
        "reasoning_delta" => {
            let delta = payload.get("delta").and_then(Value::as_str)?;
            json!({
                "sessionUpdate": "agent_thought_chunk",
                "content": {"type": "text", "text": delta},
            })
        }
        "tool_call_start" => {
            let id = payload.get("id").and_then(Value::as_str)?;
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            json!({
                "sessionUpdate": "tool_call",
                "toolCallId": id,
                "title": name,
                "kind": tool_kind(name),
                "status": "pending",
            })
        }
        "tool_executing" => {
            let id = payload.get("id").and_then(Value::as_str)?;
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": id,
                "title": name,
                "kind": tool_kind(name),
                "status": "in_progress",
                "rawInput": payload.get("arguments").cloned().unwrap_or(Value::Null),
            })
        }
        "tool_call_settled" => {
            let id = payload
                .get("call_id")
                .or_else(|| payload.get("id"))
                .and_then(Value::as_str)?;
            let status = payload
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            let acp_status = match status {
                "completed" => "completed",
                "failed" => "failed",
                "cancelled" => "failed",
                _ => "in_progress",
            };
            let output = payload
                .get("result")
                .and_then(Value::as_str)
                .or_else(|| payload.get("error").and_then(Value::as_str))
                .unwrap_or_default();
            json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": id,
                "status": acp_status,
                "content": [{
                    "type": "content",
                    "content": {"type": "text", "text": output},
                }],
            })
        }
        _ => return None,
    };
    Some(json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session_id_string(session_id),
            "update": update,
        },
    }))
}

fn tool_kind(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "shell" | "bash" | "run_command" | "run_shell" => "execute",
        "webfetch" | "fetch" => "fetch",
        "edit" | "write" | "patch" | "edit_file" | "write_file" => "edit",
        "grep" | "glob" | "list_dir" | "search" => "search",
        "read" | "read_file" => "read",
        "task" | "subagent" => "think",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, payload: Value) -> RuntimeEvent {
        RuntimeEvent {
            seq: 1,
            session_id: 7,
            generation_id: Some(1),
            kind: kind.to_string(),
            payload_json: payload.to_string(),
        }
    }

    #[test]
    fn parses_session_ids_with_and_without_prefix() {
        assert_eq!(parse_session_id("ses_42").unwrap(), 42);
        assert_eq!(parse_session_id("42").unwrap(), 42);
        assert!(parse_session_id("nope").is_err());
    }

    fn test_server() -> AcpServer {
        AcpServer {
            runtime: None,
            db: Db::open_in_memory().unwrap(),
            bus: EventBus::new(),
            provider: "test".to_string(),
            model: "model".to_string(),
            agent: "build".to_string(),
            models: vec![
                ("test".to_string(), "model".to_string()),
                ("test".to_string(), "other".to_string()),
            ],
            base_mcp: BTreeMap::new(),
            session_cwds: HashMap::new(),
            session_agents: HashMap::new(),
            session_models: HashMap::new(),
            session_mcps: HashMap::new(),
        }
    }

    fn attach_session(server: &mut AcpServer, cwd: &str, title: &str) -> i64 {
        let workspace = server.db.create_workspace(cwd, "test").unwrap();
        let session = server
            .db
            .create_session_in_workspace(workspace.id, title)
            .unwrap();
        server.session_cwds.insert(session.id, cwd.to_string());
        server
            .session_agents
            .insert(session.id, "build".to_string());
        server
            .session_models
            .insert(session.id, (server.provider.clone(), server.model.clone()));
        session.id
    }

    #[test]
    fn initialize_advertises_fork_and_requested_terminal_auth() {
        let mut server = test_server();
        let result = server
            .handle_request(
                "initialize",
                &json!({
                    "clientCapabilities": {
                        "auth": {"terminal": true},
                        "_meta": {"terminal-auth": true}
                    }
                }),
                &mut Vec::new(),
            )
            .unwrap();
        assert_eq!(result["protocolVersion"], 1);
        assert_eq!(
            result["agentCapabilities"]["sessionCapabilities"]["fork"],
            json!({})
        );
        assert_eq!(
            result["agentCapabilities"]["mcpCapabilities"],
            json!({"http": true, "sse": false})
        );
        assert_eq!(result["authMethods"][0]["type"], "terminal");
        assert_eq!(
            result["authMethods"][0]["_meta"]["terminal-auth"]["command"],
            "clawcode"
        );
    }

    #[test]
    fn fork_copies_history_and_keeps_the_session_cwd() {
        let mut server = test_server();
        let cwd = current_dir();
        let session_id = attach_session(&mut server, &cwd, "source");
        server
            .db
            .append_message(session_id, "user", "hello")
            .unwrap();
        server
            .db
            .append_message(session_id, "assistant", "hi")
            .unwrap();

        let result = server
            .handle_request(
                "session/fork",
                &json!({"sessionId": session_id_string(session_id), "cwd": cwd}),
                &mut Vec::new(),
            )
            .unwrap();
        let forked = parse_session_id(result["sessionId"].as_str().unwrap()).unwrap();
        assert_ne!(forked, session_id);
        assert_eq!(server.db.messages(forked).unwrap().len(), 2);
        assert_eq!(server.session_cwds.get(&forked), Some(&cwd));
        assert_eq!(
            result["configOptions"][0]["currentValue"],
            json!("test/model")
        );
    }

    #[test]
    fn rejects_sessions_loaded_from_a_different_cwd() {
        let mut server = test_server();
        let cwd = current_dir();
        let session_id = attach_session(&mut server, &cwd, "source");
        let other = Path::new(&cwd).join("other").display().to_string();
        let error = server
            .handle_request(
                "session/load",
                &json!({"sessionId": session_id_string(session_id), "cwd": other}),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert_eq!(error.code, -32602);
        assert_eq!(error.data.as_ref().unwrap()["sessionId"], "ses_1");
        assert_eq!(error.data.as_ref().unwrap()["cwd"], cwd);
    }

    #[test]
    fn sets_mode_and_model_per_session() {
        let mut server = test_server();
        let cwd = current_dir();
        let session_id = attach_session(&mut server, &cwd, "source");

        server
            .handle_request(
                "session/set_mode",
                &json!({"sessionId": session_id_string(session_id), "modeId": "plan"}),
                &mut Vec::new(),
            )
            .unwrap();
        let result = server
            .handle_request(
                "session/set_config_option",
                &json!({
                    "sessionId": session_id_string(session_id),
                    "configId": "model",
                    "value": "test/other"
                }),
                &mut Vec::new(),
            )
            .unwrap();
        assert_eq!(server.session_agent(session_id), "plan");
        assert_eq!(server.session_model(session_id).1, "other");
        assert_eq!(result["configOptions"][0]["currentValue"], "test/other");
        assert_eq!(result["configOptions"][1]["currentValue"], "plan");

        let error = server
            .handle_request(
                "session/set_config_option",
                &json!({
                    "sessionId": session_id_string(session_id),
                    "configId": "model",
                    "value": "test/unknown"
                }),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert_eq!(error.code, -32602);
        assert_eq!(error.data.as_ref().unwrap()["field"], "value");
    }

    #[test]
    fn lists_only_sessions_from_the_requested_cwd() {
        let mut server = test_server();
        let cwd = current_dir();
        let other = Path::new(&cwd).join("other").display().to_string();
        let first = attach_session(&mut server, &cwd, "first");
        let second = attach_session(&mut server, &other, "second");
        let result = server
            .handle_request("session/list", &json!({"cwd": cwd}), &mut Vec::new())
            .unwrap();
        let mut ids = result["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|session| session["sessionId"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids, vec![session_id_string(first)]);

        let result = server
            .handle_request("session/list", &json!({"cwd": other}), &mut Vec::new())
            .unwrap();
        let mut ids = result["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|session| session["sessionId"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids, vec![session_id_string(second)]);
    }

    #[test]
    fn parses_stdio_and_http_mcp_attachments() {
        let parsed = parse_acp_mcp_servers(&json!({
            "mcpServers": [
                {
                    "name": "local",
                    "command": "node",
                    "args": ["server.js"],
                    "env": [{"name": "TOKEN", "value": "abc"}]
                },
                {
                    "type": "http",
                    "name": "remote",
                    "url": "https://example.test/mcp",
                    "headers": [{"name": "Authorization", "value": "Bearer test"}]
                }
            ]
        }))
        .unwrap()
        .unwrap();
        let local = parsed.get("local").unwrap();
        assert_eq!(local.command, ["node", "server.js"]);
        assert_eq!(
            local.environment.get("TOKEN").map(String::as_str),
            Some("abc")
        );
        let remote = parsed.get("remote").unwrap();
        assert_eq!(remote.url.as_deref(), Some("https://example.test/mcp"));
        assert_eq!(
            remote.headers.get("Authorization").map(String::as_str),
            Some("Bearer test")
        );
        assert!(
            parse_acp_mcp_servers(&json!({"mcpServers": []}))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rejects_relative_cwds_and_unsupported_mcp_transports() {
        let relative = cwd_from_params(&json!({"cwd": "relative/path"})).unwrap_err();
        assert_eq!(relative.data.as_ref().unwrap()["field"], "cwd");
        let mcp = parse_acp_mcp_servers(&json!({
            "mcpServers": [{"type": "sse", "name": "x", "url": "https://x"}]
        }))
        .unwrap_err();
        assert_eq!(mcp.data.as_ref().unwrap()["field"], "mcpServers");
        let unnamed = parse_acp_mcp_servers(&json!({
            "mcpServers": [{"command": "server"}]
        }))
        .unwrap_err();
        assert_eq!(unnamed.data.as_ref().unwrap()["field"], "mcpServers");
    }

    #[test]
    fn extracts_text_and_resource_prompt_blocks() {
        let prompt = json!([
            {"type": "text", "text": "hello "},
            {"type": "resource", "resource": {"text": "world"}},
            {"type": "image", "data": "..."}
        ]);
        assert_eq!(
            prompt_text(&json!({"prompt": prompt})).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn translates_text_and_tool_events_to_session_updates() {
        let text = translate_event(&event("text_delta", json!({"delta": "hi"})), 7).unwrap();
        assert_eq!(text["method"], "session/update");
        assert_eq!(
            text["params"]["update"]["sessionUpdate"],
            "agent_message_chunk"
        );
        assert_eq!(text["params"]["update"]["content"]["text"], "hi");

        let tool = translate_event(
            &event(
                "tool_call_start",
                json!({"id": "call-1", "name": "read_file"}),
            ),
            7,
        )
        .unwrap();
        assert_eq!(tool["params"]["update"]["sessionUpdate"], "tool_call");
        assert_eq!(tool["params"]["update"]["kind"], "read");

        let settled = translate_event(
            &event(
                "tool_call_settled",
                json!({
                    "call_id": "call-1",
                    "tool_name": "read_file",
                    "status": "completed",
                    "result": "file contents"
                }),
            ),
            7,
        )
        .unwrap();
        assert_eq!(settled["params"]["update"]["status"], "completed");
    }
}
