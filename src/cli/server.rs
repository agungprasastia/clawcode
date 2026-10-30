//! HTTP API and web surface.
//!
//! The server is deliberately synchronous: one bounded request per connection
//! on a worker thread, with SQLite opened per request. Generation shares one
//! runtime worker with the rest of the process and streams through the same
//! event bus as the TUI.

use crate::adapters::ConfiguredRouter;
use crate::cli::opencode::{CliError, select_model};
use crate::cli::spec::Invocation;
use crate::config::{Config, ConfigLoader};
use crate::persistence::{
    Db, ForkSessionError, RevertError, SavedPermission, SavedPermissionError, SessionSummary,
    WriterHandle,
};
use crate::runtime::client::RuntimeClient;
use crate::runtime::{EventBus, RuntimeEvent};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const EVENT_HEARTBEAT: Duration = Duration::from_secs(15);

pub fn serve(invocation: &Invocation) -> Result<(), CliError> {
    let stdio = invocation.flag_present("stdio");
    if invocation.flag_present("service") && stdio {
        return Err(CliError("--service and --stdio cannot be combined".into()));
    }
    let hostname = invocation.flag("hostname").unwrap_or("127.0.0.1");
    let port = invocation
        .flag("port")
        .map(|value| value.parse::<u16>())
        .transpose()
        .map_err(|_| CliError("--port expects an integer".into()))?
        .unwrap_or(if stdio { 0 } else { 4096 });
    let listener = TcpListener::bind((hostname, port))
        .map_err(|error| CliError(format!("Cannot bind {hostname}:{port}: {error}")))?;
    let address = listener
        .local_addr()
        .map_err(|error| CliError(error.to_string()))?;
    let service_file = data_dir().join("service.json");
    if invocation.flag_present("service") {
        let _ = fs::create_dir_all(data_dir());
        let _ = fs::write(
            &service_file,
            format!(
                "{{\"pid\":{},\"hostname\":\"{}\",\"port\":{}}}\n",
                std::process::id(),
                hostname,
                address.port()
            ),
        );
    }
    if stdio {
        println!("{}", json!({"url": format!("http://{address}")}));
    } else {
        println!("clawcode server listening on http://{address}");
    }
    let cors = invocation.flag_values("cors").to_vec();
    let state = Arc::new(ServerState::new());
    if let Some(error) = state.runtime_error.as_deref() {
        eprintln!("clawcode server: {error}");
    }
    for incoming in listener.incoming() {
        let Ok(stream) = incoming else {
            continue;
        };
        let cors = cors.clone();
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            let _ = handle_connection(stream, &cors, &state);
        });
    }
    Ok(())
}

fn data_dir() -> PathBuf {
    crate::tui::default_data_dir()
}

fn db_path() -> PathBuf {
    data_dir().join("clawcode.db")
}

fn open_db() -> Result<Db, CliError> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Db::open(&path).map_err(CliError::from)
}

#[derive(Clone)]
struct PermissionRequest {
    id: String,
    session_id: i64,
    action: String,
    resources: Vec<String>,
    save: Option<Vec<String>>,
    metadata: Option<Value>,
    source: Option<Value>,
    message: Option<String>,
}

struct ServerState {
    bus: EventBus,
    runtime: Option<RuntimeClient>,
    provider: String,
    model: String,
    runtime_error: Option<String>,
    event_counter: AtomicU64,
    permission_counter: AtomicU64,
    pending_permissions: std::sync::Mutex<Vec<PermissionRequest>>,
    directory: String,
}

impl ServerState {
    fn new() -> Self {
        let bus = EventBus::new();
        let mut state = Self {
            bus: bus.clone(),
            runtime: None,
            provider: String::new(),
            model: String::new(),
            runtime_error: None,
            event_counter: AtomicU64::new(1),
            permission_counter: AtomicU64::new(1),
            pending_permissions: std::sync::Mutex::new(Vec::new()),
            directory: std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string()),
        };
        match ConfigLoader.load() {
            Ok(config) => {
                let (provider, model) = select_model(&config, None).unwrap_or_default();
                state.provider = provider;
                state.model = model;
                match start_runtime(&config, bus) {
                    Ok(runtime) => state.runtime = Some(runtime),
                    Err(error) => state.runtime_error = Some(error),
                }
            }
            Err(error) => state.runtime_error = Some(error.to_string()),
        }
        state
    }

    fn runtime(&self) -> Result<&RuntimeClient, String> {
        self.runtime.as_ref().ok_or_else(|| {
            self.runtime_error
                .clone()
                .unwrap_or_else(|| "runtime is unavailable".to_string())
        })
    }

    fn generation_model(&self) -> Result<(&str, &str), String> {
        if self.provider.is_empty() || self.model.is_empty() {
            return Err(
                "No model configured. Add a provider and model to your configuration.".into(),
            );
        }
        Ok((&self.provider, &self.model))
    }

    fn next_event_id(&self, prefix: &str) -> String {
        let counter = self.event_counter.fetch_add(1, Ordering::Relaxed);
        format!("evt_{prefix}_{counter}")
    }

    fn subscribe(
        &self,
        session_id: Option<i64>,
    ) -> (BusSubscription, mpsc::Receiver<RuntimeEvent>) {
        let (id, receiver) = self.bus.subscribe(session_id);
        (
            BusSubscription {
                bus: self.bus.clone(),
                id,
            },
            receiver,
        )
    }
}

fn start_runtime(config: &Config, bus: EventBus) -> Result<RuntimeClient, String> {
    let data = data_dir();
    fs::create_dir_all(&data).map_err(|error| error.to_string())?;
    let path = data.join("clawcode.db");
    let runtime_db = Db::open(&path).map_err(|error| error.to_string())?;
    let writer_db = Db::open(&path).map_err(|error| error.to_string())?;
    let writer = WriterHandle::spawn(writer_db);
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mcp = crate::mcp::McpRegistry::new(&config.mcp, &cwd);
    let router = ConfiguredRouter::new(config.clone());
    Ok(RuntimeClient::spawn_with_mcp(
        runtime_db,
        writer,
        Box::new(router),
        bus,
        mcp,
    ))
}

struct BusSubscription {
    bus: EventBus,
    id: u64,
}

impl Drop for BusSubscription {
    fn drop(&mut self) {
        self.bus.unsubscribe(self.id);
    }
}

struct Request {
    method: String,
    path: String,
    body: String,
    origin: Option<String>,
}

fn handle_connection(
    mut stream: TcpStream,
    cors: &[String],
    state: &ServerState,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let request = match read_request(&mut stream) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) => {
            write_json_error(&mut stream, 400, &error.to_string())?;
            return Ok(());
        }
    };
    if request.method == "OPTIONS" {
        write_response(&mut stream, 204, "text/plain", "", &request, cors)?;
        return Ok(());
    }
    let path = request.path.split('?').next().unwrap_or("/");
    if request.method == "GET" && matches!(path, "/api/event" | "/event") {
        return stream_events(&mut stream, &request, cors, state);
    }
    let (status, content_type, body) = route(&request, state);
    write_response(&mut stream, status, content_type, &body, &request, cors)
}

fn route(request: &Request, state: &ServerState) -> (u16, &'static str, String) {
    let path = request.path.split('?').next().unwrap_or("/");
    match (request.method.as_str(), path) {
        ("GET", "/global/health") => (
            200,
            "application/json",
            json!({
                "healthy": true,
                "version": env!("CARGO_PKG_VERSION"),
                "name": "clawcode",
            })
            .to_string(),
        ),
        ("GET", "/config") => match ConfigLoader.load() {
            Ok(config) => (
                200,
                "application/json",
                json!({
                    "model": config.model,
                    "providers": config.providers.keys().collect::<Vec<_>>(),
                    "agents": config.agents.keys().collect::<Vec<_>>(),
                })
                .to_string(),
            ),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        ("GET", "/session") => match list_sessions() {
            Ok(value) => (200, "application/json", value.to_string()),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        ("POST", "/session") => match create_session(&request.body) {
            Ok(value) => (200, "application/json", value.to_string()),
            Err(error) => (400, "application/json", error_body(&error.to_string())),
        },
        ("GET", "/api/session") => match list_sessions_v2() {
            Ok(value) => (200, "application/json", value.to_string()),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        ("POST", "/api/session") => match create_session_v2(&request.body) {
            Ok(value) => (200, "application/json", value.to_string()),
            Err(error) => (400, "application/json", error_body(&error.to_string())),
        },
        ("GET", "/") => (200, "text/html; charset=utf-8", web_page()),
        ("GET", "/doc") => (200, "application/json", openapi_document().to_string()),
        ("GET", "/api/permission/request") => {
            let requests = pending_permission_requests(state);
            let value = json!({
                "location": { "directory": state.directory },
                "data": requests,
            });
            (200, "application/json", value.to_string())
        }
        ("GET", "/api/permission/saved") => {
            match list_saved_permissions(query_param(&request.path, "projectID")) {
                Ok(value) => (200, "application/json", value.to_string()),
                Err((status, message)) => (status, "application/json", message),
            }
        }
        _ if path.starts_with("/api/permission/saved/") => saved_permission_route(request, path),
        _ if path.starts_with("/api/session/") => api_session_route(request, path, state),
        _ if path.starts_with("/session/") => session_route(request, path),
        _ => (404, "application/json", error_body("Not Found")),
    }
}

fn session_route(request: &Request, path: &str) -> (u16, &'static str, String) {
    let Some((id, tail)) = parse_session_path(path) else {
        return (404, "application/json", error_body("Not Found"));
    };
    match (request.method.as_str(), tail.as_slice()) {
        ("GET", []) => match get_session(id) {
            Ok(Some(value)) => (200, "application/json", value.to_string()),
            Ok(None) => (404, "application/json", error_body("Session not found")),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        ("DELETE", []) => match delete_session(id) {
            Ok(true) => (
                200,
                "application/json",
                json!({ "deleted": true }).to_string(),
            ),
            Ok(false) => (404, "application/json", error_body("Session not found")),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        ("GET", ["message"]) => match list_messages(id) {
            Ok(Some(value)) => (200, "application/json", value.to_string()),
            Ok(None) => (404, "application/json", error_body("Session not found")),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        _ => (404, "application/json", error_body("Not Found")),
    }
}

fn api_session_route(
    request: &Request,
    path: &str,
    state: &ServerState,
) -> (u16, &'static str, String) {
    let Some((id, tail)) = parse_session_path(path) else {
        return (404, "application/json", error_body("Not Found"));
    };
    match (request.method.as_str(), tail.as_slice()) {
        ("GET", []) => match public_session(id, state) {
            Ok(Some(value)) => (
                200,
                "application/json",
                json!({ "data": value }).to_string(),
            ),
            Ok(None) => (404, "application/json", error_body("Session not found")),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        ("DELETE", []) => match delete_session(id) {
            Ok(true) => (
                200,
                "application/json",
                json!({ "data": { "id": session_id(id) } }).to_string(),
            ),
            Ok(false) => (404, "application/json", error_body("Session not found")),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        ("GET", ["message"]) => match public_messages(id, state) {
            Ok(Some(value)) => (200, "application/json", value.to_string()),
            Ok(None) => (404, "application/json", error_body("Session not found")),
            Err(error) => (500, "application/json", error_body(&error.to_string())),
        },
        ("POST", ["prompt"]) => match prompt_session(id, &request.body, state) {
            Ok(value) => (
                200,
                "application/json",
                json!({ "data": value }).to_string(),
            ),
            Err((status, message)) => (status, "application/json", error_body(&message)),
        },
        ("POST", ["interrupt"]) => match interrupt_session(id, state) {
            Ok(()) => (200, "application/json", json!({ "data": {} }).to_string()),
            Err((status, message)) => (status, "application/json", error_body(&message)),
        },
        ("POST", ["fork"]) => match fork_session(id, &request.body, state) {
            Ok(value) => (
                200,
                "application/json",
                json!({ "data": value }).to_string(),
            ),
            Err((status, message)) => (status, "application/json", message),
        },
        ("POST", ["revert", "stage"]) => match stage_revert(id, &request.body, state) {
            Ok(value) => (
                200,
                "application/json",
                json!({ "data": value }).to_string(),
            ),
            Err((status, message)) => (status, "application/json", message),
        },
        ("DELETE", ["revert"]) => match clear_revert(id, state) {
            Ok(()) => (204, "application/json", String::new()),
            Err((status, message)) => (status, "application/json", message),
        },
        ("POST", ["revert", "commit"]) => match commit_revert(id, state) {
            Ok(()) => (204, "application/json", String::new()),
            Err((status, message)) => (status, "application/json", message),
        },
        ("GET", ["permission"]) => match session_permission_requests(id, state) {
            Ok(value) => (200, "application/json", value.to_string()),
            Err((status, message)) => (status, "application/json", message),
        },
        ("POST", ["permission"]) => match create_permission(id, &request.body, state) {
            Ok(value) => (
                200,
                "application/json",
                json!({ "data": value }).to_string(),
            ),
            Err((status, message)) => (status, "application/json", message),
        },
        ("GET", ["permission", request_id]) => {
            match get_permission_request(id, request_id, state) {
                Ok(value) => (
                    200,
                    "application/json",
                    json!({ "data": value }).to_string(),
                ),
                Err((status, message)) => (status, "application/json", message),
            }
        }
        ("POST", ["permission", request_id, "reply"]) => {
            match reply_permission(id, request_id, &request.body, state) {
                Ok(()) => (204, "application/json", String::new()),
                Err((status, message)) => (status, "application/json", message),
            }
        }
        _ => (404, "application/json", error_body("Not Found")),
    }
}

fn parse_session_path(path: &str) -> Option<(i64, Vec<&str>)> {
    let mut parts = path.trim_start_matches('/').split('/');
    let raw_id = parts.find(|part| {
        part.strip_prefix("ses_")
            .unwrap_or(part)
            .parse::<i64>()
            .is_ok()
    })?;
    let id = raw_id.strip_prefix("ses_").unwrap_or(raw_id);
    let id = id.parse::<i64>().ok()?;
    Some((id, parts.collect()))
}

fn session_id(id: i64) -> String {
    format!("ses_{id}")
}

fn list_sessions() -> Result<Value, CliError> {
    let db = open_db()?;
    let sessions = db.session_summaries(false, 100)?;
    Ok(Value::Array(
        sessions.iter().map(session_json).collect::<Vec<_>>(),
    ))
}

fn list_sessions_v2() -> Result<Value, CliError> {
    let db = open_db()?;
    let sessions = db.session_summaries(true, 1_000)?;
    let directory = std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_string());
    Ok(json!({
        "data": sessions
            .iter()
            .map(|session| public_session_summary(session, &directory))
            .collect::<Vec<_>>(),
        "cursor": {},
    }))
}

fn create_session_v2(body: &str) -> Result<Value, CliError> {
    Ok(json!({ "data": create_session(body)? }))
}

fn public_session(id: i64, state: &ServerState) -> Result<Option<Value>, CliError> {
    let db = open_db()?;
    let sessions = db.session_summaries(true, 1_000)?;
    if let Some(session) = sessions.iter().find(|session| session.id == id) {
        return Ok(Some(public_session_summary(session, &state.directory)));
    }
    let Some(session) = db.session(id)? else {
        return Ok(None);
    };
    let now = now_millis();
    Ok(Some(json!({
        "id": session_id(session.id),
        "projectID": "prj_clawcode",
        "cost": 0.0,
        "tokens": zero_tokens(),
        "time": { "created": now, "updated": now },
        "title": session.title,
        "location": { "directory": state.directory },
        "metadata": {
            "pinned": session.pinned,
            "workspaceID": session.workspace_id,
        },
    })))
}

fn public_session_summary(session: &SessionSummary, directory: &str) -> Value {
    let created = rfc3339_millis(&session.created_at).unwrap_or(0);
    let updated = rfc3339_millis(&session.updated_at).unwrap_or(created);
    let mut value = json!({
        "id": session_id(session.id),
        "projectID": "prj_clawcode",
        "cost": 0.0,
        "tokens": zero_tokens(),
        "time": { "created": created, "updated": updated },
        "title": session.title,
        "location": { "directory": directory },
        "metadata": {
            "pinned": session.pinned,
            "archived": session.archived,
            "workspaceID": session.workspace_id,
        },
    });
    if let Some(parent_id) = session.parent_id {
        value["parentID"] = json!(session_id(parent_id));
        if let (Some(boundary_type), Some(message_id)) =
            (session.fork_boundary_type, session.fork_boundary_message_id)
        {
            value["fork"] = json!({
                "sessionID": session_id(parent_id),
                "boundary": {
                    "type": boundary_type.as_str(),
                    "messageID": format!("msg_{message_id}"),
                },
            });
        }
    }
    if let Some(message_id) = session.revert_message_id {
        value["revert"] = public_revert(message_id);
    }
    value
}

fn zero_tokens() -> Value {
    json!({
        "input": 0,
        "output": 0,
        "reasoning": 0,
        "cache": { "read": 0, "write": 0 },
    })
}

fn public_messages(id: i64, state: &ServerState) -> Result<Option<Value>, CliError> {
    let db = open_db()?;
    if db.session(id)?.is_none() {
        return Ok(None);
    }
    let created = now_millis();
    let messages = db
        .messages(id)?
        .into_iter()
        .map(|message| public_message(&message, created, &state.provider, &state.model))
        .collect::<Vec<_>>();
    Ok(Some(json!({ "data": messages, "cursor": {} })))
}

fn public_message(
    message: &crate::persistence::Message,
    created: u64,
    provider: &str,
    model: &str,
) -> Value {
    let id = format!("msg_{}", message.id);
    match message.role.as_str() {
        "user" => json!({
            "id": id,
            "time": { "created": created },
            "type": "user",
            "text": message.content,
        }),
        "assistant" => json!({
            "id": id,
            "time": { "created": created, "completed": created },
            "type": "assistant",
            "agent": "build",
            "model": { "id": model, "providerID": provider },
            "content": [{ "type": "text", "text": message.content }],
        }),
        _ => json!({
            "id": id,
            "time": { "created": created },
            "type": "system",
            "text": message.content,
        }),
    }
}

fn prompt_session(id: i64, body: &str, state: &ServerState) -> Result<Value, (u16, String)> {
    let (text, delivery) = parse_prompt(body)?;
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    if db
        .session(id)
        .map_err(|error| (500, error.to_string()))?
        .is_none()
    {
        return Err((404, "Session not found".to_string()));
    }
    let (provider, model) = state.generation_model().map_err(|error| (503, error))?;
    let runtime = state.runtime().map_err(|error| (503, error))?;
    let was_active = runtime.is_active(id);
    let (_subscription, receiver) = state.subscribe(Some(id));
    let input_id = runtime
        .start_generation_admitted(id, "build", provider, model, &text)
        .map_err(|error| (500, error))?;
    let user_message_id = if was_active {
        input_id
    } else {
        wait_for_promoted(&receiver, input_id, Duration::from_secs(2)).unwrap_or(input_id)
    };
    Ok(json!({
        "id": format!("msg_{user_message_id}"),
        "sessionID": session_id(id),
        "time": { "created": now_millis() },
        "type": "user",
        "payload": { "text": text },
        "delivery": delivery,
    }))
}

fn interrupt_session(id: i64, state: &ServerState) -> Result<(), (u16, String)> {
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    if db
        .session(id)
        .map_err(|error| (500, error.to_string()))?
        .is_none()
    {
        return Err((404, "Session not found".to_string()));
    }
    state
        .runtime()
        .map_err(|error| (503, error))?
        .cancel_generation(id)
        .map_err(|error| (500, error))
}

fn fork_session(id: i64, body: &str, state: &ServerState) -> Result<Value, (u16, String)> {
    let before = parse_before(body)?;
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    let forked = db
        .fork_session_before(id, before)
        .map_err(fork_error_body)?;
    public_session(forked.id, state)
        .map_err(|error| (500, error.to_string()))?
        .ok_or_else(|| (500, error_body("forked session could not be read")))
}

fn parse_before(body: &str) -> Result<Option<i64>, (u16, String)> {
    if body.trim().is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|error| (400, error_body(&format!("invalid JSON body: {error}"))))?;
    match value.get("before") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => parse_message_id(raw)
            .map(Some)
            .ok_or_else(|| (400, error_body(&format!("invalid message ID: {raw}")))),
        Some(_) => Err((400, error_body("before must be a message ID string"))),
    }
}

fn parse_message_id(raw: &str) -> Option<i64> {
    raw.strip_prefix("msg_")?.parse::<i64>().ok()
}

fn fork_error_body(error: ForkSessionError) -> (u16, String) {
    match error {
        ForkSessionError::SessionNotFound => (404, error_body("Session not found")),
        ForkSessionError::MessageNotFound(message_id) => (
            404,
            error_body(&format!("Message not found: msg_{message_id}")),
        ),
        ForkSessionError::Empty(session_id) => (
            400,
            error_body_kind(
                &format!("Cannot fork empty session: ses_{session_id}"),
                "empty_session",
            ),
        ),
        ForkSessionError::Database(error) => (500, error_body(&format!("database error: {error}"))),
    }
}

fn stage_revert(id: i64, body: &str, state: &ServerState) -> Result<Value, (u16, String)> {
    ensure_idle(id, state)?;
    let message_id = parse_revert_stage(body)?;
    let include_files = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("files").and_then(Value::as_bool))
        .unwrap_or(true);
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    let revert = db.stage_revert(id, message_id).map_err(revert_error_body)?;
    let files = if include_files {
        reverted_files_from_db(&db, id, revert.message_id)
    } else {
        Vec::new()
    };
    Ok(public_revert_with_files(revert.message_id, files))
}

fn parse_revert_stage(body: &str) -> Result<i64, (u16, String)> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| (400, error_body(&format!("invalid JSON body: {error}"))))?;
    let raw = value
        .get("messageID")
        .and_then(Value::as_str)
        .ok_or_else(|| (400, error_body("messageID is required")))?;
    let message_id = parse_message_id(raw)
        .ok_or_else(|| (400, error_body(&format!("invalid message ID: {raw}"))))?;
    if let Some(files) = value.get("files")
        && !files.is_boolean()
    {
        return Err((400, error_body("files must be a boolean")));
    }
    Ok(message_id)
}

fn clear_revert(id: i64, state: &ServerState) -> Result<(), (u16, String)> {
    ensure_idle(id, state)?;
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    db.clear_revert(id).map_err(revert_error_body)
}

fn commit_revert(id: i64, state: &ServerState) -> Result<(), (u16, String)> {
    ensure_idle(id, state)?;
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    if let Ok(summaries) = db.session_summaries(true, 1_000)
        && let Some(boundary) = summaries
            .into_iter()
            .find(|s| s.id == id)
            .and_then(|s| s.revert_message_id)
    {
        restore_reverted_files_from_db(&db, id, boundary, std::path::Path::new(&state.directory));
    }
    db.commit_revert(id).map_err(revert_error_body)
}

fn reverted_files_from_db(db: &Db, session_id: i64, boundary_message_id: i64) -> Vec<String> {
    let Ok(calls) = db.tool_calls(session_id) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for call in calls {
        if call.assistant_message_id < boundary_message_id {
            continue;
        }
        if let Some(meta_str) = call.metadata_json.as_deref()
            && let Ok(meta) = serde_json::from_str::<Value>(meta_str)
            && let Some(path) = meta
                .get("filediff")
                .and_then(|fd| fd.get("path"))
                .and_then(Value::as_str)
        {
            let path_s = path.to_string();
            if !files.contains(&path_s) {
                files.push(path_s);
            }
        }
    }
    files
}

fn restore_reverted_files_from_db(
    db: &Db,
    session_id: i64,
    boundary_message_id: i64,
    workspace_dir: &std::path::Path,
) {
    let Ok(calls) = db.tool_calls(session_id) else {
        return;
    };
    for call in calls.iter().rev() {
        if call.assistant_message_id < boundary_message_id {
            continue;
        }
        if let Some(meta_str) = call.metadata_json.as_deref()
            && let Ok(meta) = serde_json::from_str::<Value>(meta_str)
            && let Some(filediff) = meta.get("filediff")
        {
            let _ = crate::conversation::tools::restore_filediff_snapshot(
                workspace_dir,
                filediff,
                true,
            );
        }
    }
}

fn ensure_idle(id: i64, state: &ServerState) -> Result<(), (u16, String)> {
    if state
        .runtime
        .as_ref()
        .is_some_and(|runtime| runtime.is_active(id))
    {
        return Err((
            409,
            error_body(&format!("Session is busy: {}", session_id(id))),
        ));
    }
    Ok(())
}

fn revert_error_body(error: RevertError) -> (u16, String) {
    match error {
        RevertError::SessionNotFound => (404, error_body("Session not found")),
        RevertError::MessageNotFound(message_id) => (
            404,
            error_body(&format!("Message not found: msg_{message_id}")),
        ),
        RevertError::Database(error) => (500, error_body(&format!("database error: {error}"))),
    }
}

fn public_revert(message_id: i64) -> Value {
    public_revert_with_files(message_id, Vec::new())
}

fn public_revert_with_files(message_id: i64, files: Vec<String>) -> Value {
    json!({
        "messageID": format!("msg_{message_id}"),
        "files": files,
    })
}

const PROJECT_ID: &str = "prj_clawcode";

struct NewPermission {
    id: Option<String>,
    action: String,
    resources: Vec<String>,
    save: Option<Vec<String>>,
    metadata: Option<Value>,
    source: Option<Value>,
}

fn parse_new_permission(body: &str) -> Result<NewPermission, (u16, String)> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| (400, error_body(&format!("invalid JSON body: {error}"))))?;
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| (400, error_body("action is required")))?
        .to_string();
    let resources = string_array(&value, "resources")?
        .ok_or_else(|| (400, error_body("resources is required")))?;
    let save = string_array(&value, "save")?;
    let metadata = match value.get("metadata") {
        None => None,
        Some(metadata) if metadata.is_object() => Some(metadata.clone()),
        Some(_) => return Err((400, error_body("metadata must be an object"))),
    };
    let source = match value.get("source") {
        None => None,
        Some(source) if valid_permission_source(source) => Some(source.clone()),
        Some(_) => {
            return Err((
                400,
                error_body("source must be a tool source with messageID and id"),
            ));
        }
    };
    let id = match value.get("id") {
        None => None,
        Some(id) => {
            let id = id
                .as_str()
                .ok_or_else(|| (400, error_body("id must be a string")))?;
            if !id.starts_with("per_") {
                return Err((400, error_body("permission IDs must start with per_")));
            }
            Some(id.to_string())
        }
    };
    if let Some(agent) = value.get("agent")
        && !agent.is_string()
    {
        return Err((400, error_body("agent must be a string")));
    }
    Ok(NewPermission {
        id,
        action,
        resources,
        save,
        metadata,
        source,
    })
}

fn string_array(value: &Value, field: &str) -> Result<Option<Vec<String>>, (u16, String)> {
    let Some(items) = value.get(field) else {
        return Ok(None);
    };
    let items = items
        .as_array()
        .ok_or_else(|| (400, error_body(&format!("{field} must be an array"))))?;
    let mut result = Vec::with_capacity(items.len());
    for item in items {
        let item = item
            .as_str()
            .ok_or_else(|| (400, error_body(&format!("{field} must contain strings"))))?;
        result.push(item.to_string());
    }
    Ok(Some(result))
}

fn valid_permission_source(source: &Value) -> bool {
    source.get("type").and_then(Value::as_str) == Some("tool")
        && source.get("messageID").is_some_and(Value::is_string)
        && source.get("id").is_some_and(Value::is_string)
}

fn permission_effect(
    saved: &[SavedPermission],
    action: &str,
    resources: &[String],
) -> &'static str {
    let allowed = resources.iter().all(|resource| {
        saved.iter().any(|rule| {
            wildcard_match(action, &rule.action) && wildcard_match(resource, &rule.resource)
        })
    });
    if allowed { "allow" } else { "ask" }
}

fn wildcard_match(input: &str, pattern: &str) -> bool {
    fn normalize(value: &str) -> Vec<char> {
        let value = value.replace('\\', "/");
        if cfg!(windows) {
            value.to_lowercase().chars().collect()
        } else {
            value.chars().collect()
        }
    }
    let input = normalize(input);
    let pattern = normalize(pattern);
    let mut matched = vec![false; input.len() + 1];
    matched[0] = true;
    for token in pattern {
        match token {
            '*' => {
                for index in 1..=input.len() {
                    matched[index] = matched[index] || matched[index - 1];
                }
            }
            '?' => {
                for index in (1..=input.len()).rev() {
                    matched[index] = matched[index - 1];
                }
                matched[0] = false;
            }
            token => {
                for index in (1..=input.len()).rev() {
                    matched[index] = matched[index - 1] && input[index - 1] == token;
                }
                matched[0] = false;
            }
        }
    }
    matched[input.len()]
}

fn permission_request_json(request: &PermissionRequest) -> Value {
    let mut value = json!({
        "id": request.id,
        "sessionID": session_id(request.session_id),
        "action": request.action,
        "resources": request.resources,
    });
    if let Some(save) = &request.save {
        value["save"] = json!(save);
    }
    if let Some(metadata) = &request.metadata {
        value["metadata"] = metadata.clone();
    }
    if let Some(source) = &request.source {
        value["source"] = source.clone();
    }
    if let Some(message) = &request.message {
        value["message"] = json!(message);
    }
    value
}

fn pending_permission_requests(state: &ServerState) -> Vec<Value> {
    state
        .pending_permissions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .map(permission_request_json)
        .collect()
}

fn session_permission_requests(
    session_id: i64,
    state: &ServerState,
) -> Result<Value, (u16, String)> {
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    if db
        .session(session_id)
        .map_err(|error| (500, error.to_string()))?
        .is_none()
    {
        return Err((
            404,
            error_body(&format!("Session not found: ses_{session_id}")),
        ));
    }
    let requests = state
        .pending_permissions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter(|request| request.session_id == session_id)
        .map(permission_request_json)
        .collect::<Vec<_>>();
    Ok(json!({ "data": requests }))
}

fn create_permission(
    session_id: i64,
    body: &str,
    state: &ServerState,
) -> Result<Value, (u16, String)> {
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    if db
        .session(session_id)
        .map_err(|error| (500, error.to_string()))?
        .is_none()
    {
        return Err((
            404,
            error_body(&format!("Session not found: ses_{session_id}")),
        ));
    }
    let input = parse_new_permission(body)?;
    let saved = db
        .saved_permissions(Some(PROJECT_ID))
        .map_err(|error| (500, error_body(&format!("database error: {error}"))))?;
    let effect = permission_effect(&saved, &input.action, &input.resources);
    let id = input.id.unwrap_or_else(|| {
        let counter = state.permission_counter.fetch_add(1, Ordering::Relaxed);
        format!("per_{counter}")
    });
    let request = PermissionRequest {
        id: id.clone(),
        session_id,
        action: input.action,
        resources: input.resources,
        save: input.save,
        metadata: input.metadata,
        source: input.source,
        message: None,
    };
    if effect == "ask" {
        let request_json = permission_request_json(&request);
        {
            let mut pending = state
                .pending_permissions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if pending.iter().any(|item| item.id == id) {
                return Err((
                    409,
                    error_body(&format!("Permission request already pending: {id}")),
                ));
            }
            pending.push(request);
        }
        publish_permission_event(state, "permission.asked", session_id, request_json);
    }
    Ok(json!({ "id": id, "effect": effect }))
}

fn get_permission_request(
    session_id: i64,
    request_id: &str,
    state: &ServerState,
) -> Result<Value, (u16, String)> {
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    if db
        .session(session_id)
        .map_err(|error| (500, error.to_string()))?
        .is_none()
    {
        return Err((
            404,
            error_body(&format!("Session not found: ses_{session_id}")),
        ));
    }
    let pending = state
        .pending_permissions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let request = pending
        .iter()
        .find(|request| request.id == request_id && request.session_id == session_id)
        .ok_or_else(|| {
            (
                404,
                error_body(&format!("Permission request not found: {request_id}")),
            )
        })?;
    Ok(permission_request_json(request))
}

fn reply_permission(
    session_id: i64,
    request_id: &str,
    body: &str,
    state: &ServerState,
) -> Result<(), (u16, String)> {
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    if db
        .session(session_id)
        .map_err(|error| (500, error.to_string()))?
        .is_none()
    {
        return Err((
            404,
            error_body(&format!("Session not found: ses_{session_id}")),
        ));
    }
    let reply = parse_permission_reply(body)?;
    let request = {
        let pending = state
            .pending_permissions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        pending
            .iter()
            .find(|request| request.id == request_id && request.session_id == session_id)
            .cloned()
            .ok_or_else(|| {
                (
                    404,
                    error_body(&format!("Permission request not found: {request_id}")),
                )
            })?
    };
    let auto_saved = if reply == "always"
        && let Some(save) = request.save.as_ref()
        && !save.is_empty()
    {
        db.add_saved_permissions(PROJECT_ID, &request.action, save)
            .map_err(|error| (500, error_body(&format!("database error: {error}"))))?;
        Some(
            db.saved_permissions(Some(PROJECT_ID))
                .map_err(|error| (500, error_body(&format!("database error: {error}"))))?,
        )
    } else {
        None
    };
    let mut replied = Vec::new();
    {
        let mut pending = state
            .pending_permissions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if reply == "reject" {
            let mut index = 0;
            while index < pending.len() {
                if pending[index].session_id == session_id {
                    let removed = pending.remove(index);
                    replied.push((removed.id, "reject".to_string()));
                } else {
                    index += 1;
                }
            }
        } else {
            pending.retain(|item| item.id != request_id);
            replied.push((request_id.to_string(), reply.clone()));
        }
        if let Some(saved) = auto_saved {
            let mut index = 0;
            while index < pending.len() {
                let item = &pending[index];
                if item.session_id == session_id
                    && permission_effect(&saved, &item.action, &item.resources) == "allow"
                {
                    let removed = pending.remove(index);
                    replied.push((removed.id, "always".to_string()));
                } else {
                    index += 1;
                }
            }
        }
    }
    for (id, decision) in replied {
        publish_permission_event(
            state,
            "permission.replied",
            session_id,
            json!({
                "sessionID": format!("ses_{session_id}"),
                "requestID": id,
                "reply": decision,
            }),
        );
    }
    Ok(())
}

fn parse_permission_reply(body: &str) -> Result<String, (u16, String)> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| (400, error_body(&format!("invalid JSON body: {error}"))))?;
    let decision = value
        .get("decision")
        .and_then(Value::as_str)
        .ok_or_else(|| (400, error_body("decision is required")))?;
    if !matches!(decision, "once" | "always" | "reject") {
        return Err((400, error_body("decision must be once, always, or reject")));
    }
    if let Some(message) = value.get("message")
        && !message.is_string()
    {
        return Err((400, error_body("message must be a string")));
    }
    Ok(decision.to_string())
}

fn publish_permission_event(state: &ServerState, kind: &str, session_id: i64, payload: Value) {
    state.bus.publish(RuntimeEvent {
        seq: 0,
        session_id,
        generation_id: None,
        kind: kind.to_string(),
        payload_json: payload.to_string(),
    });
}

fn list_saved_permissions(project_id: Option<&str>) -> Result<Value, (u16, String)> {
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    let project = project_id.unwrap_or(PROJECT_ID);
    let saved = db
        .saved_permissions(Some(project))
        .map_err(|error| (500, error_body(&format!("database error: {error}"))))?;
    Ok(json!({
        "data": saved.iter().map(public_saved_permission).collect::<Vec<_>>(),
    }))
}

fn public_saved_permission(saved: &SavedPermission) -> Value {
    let created = rfc3339_millis(&saved.created_at).unwrap_or(0);
    let updated = rfc3339_millis(&saved.updated_at).unwrap_or(created);
    json!({
        "id": format!("psv_{}", saved.id),
        "projectID": saved.project_id,
        "action": saved.action,
        "resource": saved.resource,
        "time": { "created": created, "updated": updated },
    })
}

fn saved_permission_route(request: &Request, path: &str) -> (u16, &'static str, String) {
    let raw_id = path.trim_start_matches("/api/permission/saved/");
    if raw_id.is_empty() || raw_id.contains('/') {
        return (
            404,
            "application/json",
            error_body("Saved permission not found"),
        );
    }
    let id = raw_id
        .strip_prefix("psv_")
        .unwrap_or(raw_id)
        .parse::<i64>()
        .ok();
    match request.method.as_str() {
        "PATCH" => {
            let Some(id) = id else {
                return (
                    404,
                    "application/json",
                    error_body(&format!("Saved permission not found: {raw_id}")),
                );
            };
            match update_saved_permission(id, &request.body) {
                Ok(value) => (
                    200,
                    "application/json",
                    json!({ "data": value }).to_string(),
                ),
                Err((status, message)) => (status, "application/json", message),
            }
        }
        "DELETE" => {
            if let Some(id) = id {
                let result =
                    open_db().and_then(|db| db.remove_saved_permission(id).map_err(CliError::from));
                if let Err(error) = result {
                    return (
                        500,
                        "application/json",
                        error_body(&format!("database error: {error}")),
                    );
                }
            }
            (204, "application/json", String::new())
        }
        _ => (404, "application/json", error_body("Not Found")),
    }
}

fn update_saved_permission(id: i64, body: &str) -> Result<Value, (u16, String)> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| (400, error_body(&format!("invalid JSON body: {error}"))))?;
    let action = optional_string(&value, "action")?;
    let resource = optional_string(&value, "resource")?;
    let db = open_db().map_err(|error| (500, error.to_string()))?;
    match db.update_saved_permission(id, action.as_deref(), resource.as_deref()) {
        Ok(saved) => Ok(public_saved_permission(&saved)),
        Err(SavedPermissionError::NotFound(id)) => Err((
            404,
            error_body(&format!("Saved permission not found: psv_{id}")),
        )),
        Err(SavedPermissionError::Conflict { action, resource }) => Err((
            409,
            error_body(&format!(
                "A saved permission already allows {action} for {resource}"
            )),
        )),
        Err(SavedPermissionError::Database(error)) => {
            Err((500, error_body(&format!("database error: {error}"))))
        }
    }
}

fn optional_string(value: &Value, field: &str) -> Result<Option<String>, (u16, String)> {
    match value.get(field) {
        None => Ok(None),
        Some(item) => item
            .as_str()
            .map(|item| Some(item.to_string()))
            .ok_or_else(|| (400, error_body(&format!("{field} must be a string")))),
    }
}

fn parse_prompt(body: &str) -> Result<(String, String), (u16, String)> {
    let value: Value =
        serde_json::from_str(body).map_err(|error| (400, format!("invalid JSON body: {error}")))?;
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| (400, "prompt text is required".to_string()))?
        .to_string();
    if text.trim().is_empty() {
        return Err((400, "prompt text is required".to_string()));
    }
    for field in ["files", "agents", "skills"] {
        if value
            .get(field)
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        {
            return Err((
                400,
                format!("{field} attachments are not supported by this runtime"),
            ));
        }
    }
    if value.get("resume").and_then(Value::as_bool) == Some(false) {
        return Err((
            400,
            "resume=false is not supported by this runtime".to_string(),
        ));
    }
    let delivery = value
        .get("delivery")
        .and_then(Value::as_str)
        .unwrap_or("queue");
    if delivery != "queue" {
        return Err((
            400,
            format!("delivery {delivery:?} is not supported by this runtime"),
        ));
    }
    Ok((text, delivery.to_string()))
}

fn wait_for_promoted(
    receiver: &mpsc::Receiver<RuntimeEvent>,
    input_id: i64,
    timeout: Duration,
) -> Option<i64> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        let event = receiver.recv_timeout(remaining).ok()?;
        if event.kind != "prompt_promoted" {
            continue;
        }
        let payload: Value = serde_json::from_str(&event.payload_json).ok()?;
        if payload.get("input_id").and_then(Value::as_i64) != Some(input_id) {
            continue;
        }
        return payload.get("user_message_id").and_then(Value::as_i64);
    }
}

fn get_session(id: i64) -> Result<Option<Value>, CliError> {
    let db = open_db()?;
    Ok(db.session(id)?.map(|session| {
        json!({
            "id": format!("ses_{}", session.id),
            "title": session.title,
            "workspaceID": session.workspace_id,
            "status": session.status.as_str(),
            "pinned": session.pinned,
        })
    }))
}

fn create_session(body: &str) -> Result<Value, CliError> {
    let value: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("New session");
    let db = open_db()?;
    let session = db.create_session(title)?;
    Ok(json!({
        "id": format!("ses_{}", session.id),
        "title": session.title,
        "workspaceID": session.workspace_id,
        "status": session.status.as_str(),
    }))
}

fn delete_session(id: i64) -> Result<bool, CliError> {
    let db = open_db()?;
    if db.session(id)?.is_none() {
        return Ok(false);
    }
    db.delete_session(id)?;
    Ok(true)
}

fn list_messages(id: i64) -> Result<Option<Value>, CliError> {
    let db = open_db()?;
    if db.session(id)?.is_none() {
        return Ok(None);
    }
    let messages = db
        .messages(id)?
        .into_iter()
        .map(|message| {
            json!({
                "id": format!("msg_{}", message.id),
                "role": message.role,
                "content": message.content,
            })
        })
        .collect::<Vec<_>>();
    Ok(Some(Value::Array(messages)))
}

fn session_json(session: &SessionSummary) -> Value {
    json!({
        "id": format!("ses_{}", session.id),
        "title": session.title,
        "created": session.created_at,
        "updated": session.updated_at,
        "pinned": session.pinned,
        "archived": session.archived,
    })
}

fn stream_events(
    stream: &mut TcpStream,
    request: &Request,
    cors: &[String],
    state: &ServerState,
) -> std::io::Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let filter = query_param(&request.path, "sessionID")
        .or_else(|| query_param(&request.path, "session"))
        .and_then(|raw| raw.strip_prefix("ses_").or(Some(raw)))
        .and_then(|raw| raw.parse::<i64>().ok());
    let (subscription, receiver) = state.subscribe(filter);
    let (sender, outgoing) = mpsc::sync_channel::<RuntimeEvent>(512);
    let disconnected = Arc::new(AtomicBool::new(false));
    let forwarder_disconnected = Arc::clone(&disconnected);
    let forwarder = std::thread::spawn(move || {
        loop {
            if forwarder_disconnected.load(Ordering::Relaxed) {
                break;
            }
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(event) => {
                    if sender.try_send(event).is_err() {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        drop(subscription);
    });
    let result = stream_event_frames(stream, request, cors, state, &outgoing);
    disconnected.store(true, Ordering::Relaxed);
    let _ = forwarder.join();
    result
}

fn stream_event_frames(
    stream: &mut TcpStream,
    request: &Request,
    cors: &[String],
    state: &ServerState,
    outgoing: &mpsc::Receiver<RuntimeEvent>,
) -> std::io::Result<()> {
    let allowed = request
        .origin
        .as_deref()
        .filter(|origin| origin_allowed(origin, cors));
    let mut response = String::from(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/event-stream\r\n\
         Cache-Control: no-cache, no-transform\r\n\
         X-Accel-Buffering: no\r\n\
         Connection: keep-alive\r\n",
    );
    if let Some(origin) = allowed {
        response.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
        response.push_str("Vary: Origin\r\n");
        response.push_str("Access-Control-Allow-Methods: GET, POST, PATCH, DELETE, OPTIONS\r\n");
        response.push_str("Access-Control-Allow-Headers: Content-Type, Authorization\r\n");
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes())?;
    let connected = event_envelope(
        state.next_event_id("connected"),
        "server.connected",
        now_millis(),
        json!({}),
        None,
    );
    write_sse_frame(stream, &connected)?;
    let mut translator = EventTranslator::default();
    loop {
        match outgoing.recv_timeout(EVENT_HEARTBEAT) {
            Ok(event) => {
                for envelope in translator.translate(&event, state) {
                    write_sse_frame(stream, &envelope)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                stream.write_all(b": heartbeat\n\n")?;
                stream.flush()?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn query_param<'a>(path: &'a str, name: &str) -> Option<&'a str> {
    path.split_once('?')?
        .1
        .split('&')
        .find_map(|pair| pair.split_once('=').filter(|(key, _)| *key == name))
        .map(|(_, value)| value)
}

fn write_sse_frame(stream: &mut TcpStream, event: &Value) -> std::io::Result<()> {
    stream.write_all(sse_frame(event).as_bytes())?;
    stream.flush()
}

fn sse_frame(event: &Value) -> String {
    format!("data: {event}\n\n")
}

fn event_envelope(
    id: String,
    kind: &str,
    created: u64,
    data: Value,
    durable: Option<Value>,
) -> Value {
    let mut envelope = json!({
        "id": id,
        "type": kind,
        "created": created,
        "data": data,
    });
    if let Some(durable) = durable {
        envelope["durable"] = durable;
    }
    envelope
}

fn durable_meta(session: &str, seq: i64) -> Value {
    json!({
        "aggregateID": session,
        "seq": seq.max(0),
        "version": 1,
    })
}

#[derive(Default)]
struct EventTranslator {
    text_started: HashSet<String>,
    reasoning_started: HashSet<String>,
    text_buffers: HashMap<String, String>,
    reasoning_buffers: HashMap<String, String>,
    tool_inputs: HashMap<String, String>,
    errors: HashMap<i64, String>,
}

impl EventTranslator {
    fn translate(&mut self, event: &RuntimeEvent, state: &ServerState) -> Vec<Value> {
        let payload: Value =
            serde_json::from_str(&event.payload_json).unwrap_or_else(|_| json!({}));
        let session = session_id(event.session_id);
        let message = assistant_message_id(event);
        let created = now_millis();
        let mut events = Vec::new();
        match event.kind.as_str() {
            "permission.asked" | "permission.replied" => {
                events.push(event_envelope(
                    state.next_event_id("permission"),
                    &event.kind,
                    created,
                    payload,
                    None,
                ));
            }
            "prompt_promoted" => {
                if let Some(message_id) = payload.get("user_message_id").and_then(Value::as_i64) {
                    events.push(event_envelope(
                        state.next_event_id("inbox"),
                        "session.inbox.delivered",
                        created,
                        json!({
                            "sessionID": session,
                            "inboxID": format!("msg_{message_id}"),
                        }),
                        Some(durable_meta(&session, event.seq)),
                    ));
                }
            }
            "generation_started" => {
                self.errors.remove(&event.session_id);
                events.push(event_envelope(
                    state.next_event_id("execution"),
                    "session.execution.started",
                    created,
                    json!({ "sessionID": session }),
                    Some(durable_meta(&session, event.seq)),
                ));
            }
            "text_delta" => {
                let Some(delta) = payload.get("delta").and_then(Value::as_str) else {
                    return events;
                };
                if self.text_started.insert(message.clone()) {
                    events.push(event_envelope(
                        state.next_event_id("text"),
                        "session.text.started",
                        created,
                        json!({
                            "sessionID": session,
                            "assistantMessageID": message,
                            "ordinal": 0,
                        }),
                        Some(durable_meta(&session, 0)),
                    ));
                }
                self.text_buffers
                    .entry(message.clone())
                    .or_default()
                    .push_str(delta);
                events.push(event_envelope(
                    state.next_event_id("text"),
                    "session.text.delta",
                    created,
                    json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "ordinal": 0,
                        "delta": delta,
                    }),
                    None,
                ));
            }
            "reasoning_delta" => {
                let Some(delta) = payload.get("delta").and_then(Value::as_str) else {
                    return events;
                };
                if self.reasoning_started.insert(message.clone()) {
                    events.push(event_envelope(
                        state.next_event_id("reasoning"),
                        "session.reasoning.started",
                        created,
                        json!({
                            "sessionID": session,
                            "assistantMessageID": message,
                            "ordinal": 0,
                        }),
                        Some(durable_meta(&session, 0)),
                    ));
                }
                self.reasoning_buffers
                    .entry(message.clone())
                    .or_default()
                    .push_str(delta);
                events.push(event_envelope(
                    state.next_event_id("reasoning"),
                    "session.reasoning.delta",
                    created,
                    json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "ordinal": 0,
                        "delta": delta,
                    }),
                    None,
                ));
            }
            "assistant_message" => {
                let text = payload
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.text_buffers.insert(message.clone(), text.to_string());
                if payload.get("partial").and_then(Value::as_bool) == Some(true) {
                    if self.text_started.insert(message.clone()) {
                        events.push(event_envelope(
                            state.next_event_id("text"),
                            "session.text.started",
                            created,
                            json!({
                                "sessionID": session,
                                "assistantMessageID": message,
                                "ordinal": 0,
                            }),
                            Some(durable_meta(&session, 0)),
                        ));
                    }
                } else {
                    events.extend(self.finish_text(&session, &message, created, state));
                }
            }
            "tool_call_start" => {
                let Some(call_id) = payload.get("id").and_then(Value::as_str) else {
                    return events;
                };
                self.tool_inputs.insert(call_id.to_string(), String::new());
                events.push(event_envelope(
                    state.next_event_id("tool"),
                    "session.tool.input.started",
                    created,
                    json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "id": call_id,
                        "name": payload.get("name").and_then(Value::as_str).unwrap_or("tool"),
                    }),
                    Some(durable_meta(&session, 0)),
                ));
            }
            "tool_call_delta" => {
                let Some(call_id) = payload.get("id").and_then(Value::as_str) else {
                    return events;
                };
                let delta = payload
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.tool_inputs
                    .entry(call_id.to_string())
                    .or_default()
                    .push_str(delta);
                events.push(event_envelope(
                    state.next_event_id("tool"),
                    "session.tool.input.delta",
                    created,
                    json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "id": call_id,
                        "delta": delta,
                    }),
                    None,
                ));
            }
            "tool_call_end" => {
                let Some(call_id) = payload.get("id").and_then(Value::as_str) else {
                    return events;
                };
                let input = self.tool_inputs.remove(call_id).unwrap_or_default();
                events.push(event_envelope(
                    state.next_event_id("tool"),
                    "session.tool.input.ended",
                    created,
                    json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "id": call_id,
                        "text": input,
                    }),
                    Some(durable_meta(&session, 0)),
                ));
            }
            "tool_executing" => {
                let Some(call_id) = payload.get("id").and_then(Value::as_str) else {
                    return events;
                };
                events.push(event_envelope(
                    state.next_event_id("tool"),
                    "session.tool.called",
                    created,
                    json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "id": call_id,
                        "input": payload.get("arguments").cloned().unwrap_or_else(|| json!({})),
                        "executed": true,
                    }),
                    Some(durable_meta(&session, 0)),
                ));
            }
            "tool_call_progress" => {
                let Some(call_id) = payload.get("id").and_then(Value::as_str) else {
                    return events;
                };
                events.push(event_envelope(
                    state.next_event_id("tool"),
                    "session.tool.progress",
                    created,
                    json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "id": call_id,
                        "metadata": {
                            "output": payload.get("output").cloned().unwrap_or(Value::Null),
                        },
                    }),
                    None,
                ));
            }
            "tool_call_settled" => {
                let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
                    return events;
                };
                let status = payload
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("failed");
                let metadata = payload
                    .get("metadata")
                    .filter(|value| value.is_object())
                    .cloned();
                if status == "completed" {
                    let result = payload
                        .get("result")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let text = if result.is_empty() {
                        "tool completed".to_string()
                    } else {
                        result.to_string()
                    };
                    let mut data = json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "id": call_id,
                        "content": [{ "type": "text", "text": text }],
                        "executed": true,
                    });
                    if let Some(metadata) = metadata {
                        data["metadata"] = metadata;
                    }
                    events.push(event_envelope(
                        state.next_event_id("tool"),
                        "session.tool.success",
                        created,
                        data,
                        Some(durable_meta(&session, event.seq)),
                    ));
                } else {
                    let error = payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("tool execution failed");
                    let result = payload.get("result").and_then(Value::as_str);
                    let mut data = json!({
                        "sessionID": session,
                        "assistantMessageID": message,
                        "id": call_id,
                        "error": {
                            "type": if status == "cancelled" { "cancelled" } else { "tool_error" },
                            "message": error,
                        },
                        "executed": true,
                    });
                    if let Some(result) = result.filter(|value| !value.is_empty()) {
                        data["content"] = json!([{ "type": "text", "text": result }]);
                    }
                    if let Some(metadata) = metadata {
                        data["metadata"] = metadata;
                    }
                    events.push(event_envelope(
                        state.next_event_id("tool"),
                        "session.tool.failed",
                        created,
                        data,
                        Some(durable_meta(&session, event.seq)),
                    ));
                }
            }
            "error" => {
                let message = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("generation failed")
                    .to_string();
                if event.generation_id.is_some() {
                    self.errors.insert(event.session_id, message);
                } else {
                    events.push(event_envelope(
                        state.next_event_id("execution"),
                        "session.execution.failed",
                        created,
                        json!({
                            "sessionID": session,
                            "error": { "type": "unknown", "message": message },
                        }),
                        Some(durable_meta(&session, event.seq)),
                    ));
                    events.push(self.idle(&session, created, state));
                }
            }
            "generation_finished" => {
                events.extend(self.flush_pending_text(&session, &message, created, state));
                let status = payload
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                match status {
                    "completed" => events.push(event_envelope(
                        state.next_event_id("execution"),
                        "session.execution.succeeded",
                        created,
                        json!({ "sessionID": session }),
                        Some(durable_meta(&session, event.seq)),
                    )),
                    "cancelled" => events.push(event_envelope(
                        state.next_event_id("execution"),
                        "session.execution.interrupted",
                        created,
                        json!({ "sessionID": session, "reason": "user" }),
                        Some(durable_meta(&session, event.seq)),
                    )),
                    _ => {
                        let message = self
                            .errors
                            .remove(&event.session_id)
                            .unwrap_or_else(|| "generation failed".to_string());
                        events.push(event_envelope(
                            state.next_event_id("execution"),
                            "session.execution.failed",
                            created,
                            json!({
                                "sessionID": session,
                                "error": { "type": "unknown", "message": message },
                            }),
                            Some(durable_meta(&session, event.seq)),
                        ));
                    }
                }
                events.push(self.idle(&session, created, state));
            }
            _ => {}
        }
        events
    }

    fn finish_text(
        &mut self,
        session: &str,
        message: &str,
        created: u64,
        state: &ServerState,
    ) -> Vec<Value> {
        let mut events = Vec::new();
        if !self.text_started.remove(message) {
            events.push(event_envelope(
                state.next_event_id("text"),
                "session.text.started",
                created,
                json!({
                    "sessionID": session,
                    "assistantMessageID": message,
                    "ordinal": 0,
                }),
                Some(durable_meta(session, 0)),
            ));
        }
        let text = self.text_buffers.remove(message).unwrap_or_default();
        events.push(event_envelope(
            state.next_event_id("text"),
            "session.text.ended",
            created,
            json!({
                "sessionID": session,
                "assistantMessageID": message,
                "ordinal": 0,
                "text": text,
            }),
            Some(durable_meta(session, 0)),
        ));
        events
    }

    fn flush_pending_text(
        &mut self,
        session: &str,
        message: &str,
        created: u64,
        state: &ServerState,
    ) -> Vec<Value> {
        let mut events = Vec::new();
        if self.text_started.contains(message) || self.text_buffers.contains_key(message) {
            events.extend(self.finish_text(session, message, created, state));
        }
        if self.reasoning_started.remove(message) {
            let text = self.reasoning_buffers.remove(message).unwrap_or_default();
            events.push(event_envelope(
                state.next_event_id("reasoning"),
                "session.reasoning.ended",
                created,
                json!({
                    "sessionID": session,
                    "assistantMessageID": message,
                    "ordinal": 0,
                    "text": text,
                }),
                Some(durable_meta(session, 0)),
            ));
        }
        events
    }

    fn idle(&self, session: &str, created: u64, state: &ServerState) -> Value {
        event_envelope(
            state.next_event_id("idle"),
            "session.idle",
            created,
            json!({ "sessionID": session }),
            None,
        )
    }
}

fn assistant_message_id(event: &RuntimeEvent) -> String {
    format!(
        "msg_s{}_g{}",
        event.session_id,
        event.generation_id.unwrap_or_default()
    )
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn rfc3339_millis(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }
    let year = value.get(0..4)?.parse::<i64>().ok()?;
    let month = value.get(5..7)?.parse::<u32>().ok()?;
    let day = value.get(8..10)?.parse::<u32>().ok()?;
    let hour = value.get(11..13)?.parse::<u32>().ok()?;
    let minute = value.get(14..16)?.parse::<u32>().ok()?;
    let second = value.get(17..19)?.parse::<u32>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second);
    let millis = value
        .split_once('.')
        .and_then(|(_, rest)| rest.get(..3))
        .and_then(|fraction| fraction.parse::<i64>().ok())
        .unwrap_or(0);
    u64::try_from(seconds * 1_000 + millis).ok()
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day = i64::from(day);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buffer = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return Err(std::io::Error::other("request headers too large"));
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let header = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or("GET").to_string();
    let path = request_parts.next().unwrap_or("/").to_string();
    let mut content_length = 0usize;
    let mut origin = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "content-length" {
                content_length = value.parse::<usize>().unwrap_or(0);
            } else if name == "origin" {
                origin = Some(value.to_string());
            }
        }
    }
    if content_length > MAX_BODY_BYTES {
        return Err(std::io::Error::other("request body too large"));
    }
    let body_start = header_end + 4;
    let mut body = buffer[body_start..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);
    Ok(Some(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
        origin,
    }))
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_json_error(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let body = error_body(message);
    write_raw_response(stream, status, "application/json", &body, None, &[])
}

fn error_body(message: &str) -> String {
    json!({ "error": { "message": message, "type": "api_error" } }).to_string()
}

fn error_body_kind(message: &str, kind: &str) -> String {
    json!({
        "error": {
            "message": message,
            "type": "api_error",
            "kind": kind,
        }
    })
    .to_string()
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
    request: &Request,
    cors: &[String],
) -> std::io::Result<()> {
    let allowed = request
        .origin
        .as_deref()
        .filter(|origin| origin_allowed(origin, cors));
    write_raw_response(stream, status, content_type, body, allowed, cors)
}

fn write_raw_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
    origin: Option<&str>,
    cors: &[String],
) -> std::io::Result<()> {
    let reason = reason_phrase(status);
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n",
        body.len()
    );
    if let Some(origin) = origin {
        response.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
        response.push_str("Vary: Origin\r\n");
        response.push_str("Access-Control-Allow-Methods: GET, POST, PATCH, DELETE, OPTIONS\r\n");
        response.push_str("Access-Control-Allow-Headers: Content-Type, Authorization\r\n");
    }
    if !cors.is_empty() {
        response.push_str("Access-Control-Max-Age: 600\r\n");
    }
    response.push_str("\r\n");
    response.push_str(body);
    stream.write_all(response.as_bytes())
}

fn origin_allowed(origin: &str, cors: &[String]) -> bool {
    if cors
        .iter()
        .any(|allowed| allowed == "*" || allowed == origin)
    {
        return true;
    }
    let Some(rest) = origin.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or(rest);
    let host = host.rsplit_once(':').map(|(host, _)| host).unwrap_or(host);
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

fn openapi_document() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Clawcode API",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "paths": {
            "/global/health": { "get": { "summary": "Health check" } },
            "/session": {
                "get": { "summary": "List sessions" },
                "post": { "summary": "Create session" }
            },
            "/session/{sessionID}": {
                "get": { "summary": "Get session" },
                "delete": { "summary": "Delete session" }
            },
            "/session/{sessionID}/message": {
                "get": { "summary": "List messages" }
            },
            "/api/session": {
                "get": { "summary": "List sessions" },
                "post": { "summary": "Create session" }
            },
            "/api/session/{sessionID}": {
                "get": { "summary": "Get session" },
                "delete": { "summary": "Delete session" }
            },
            "/api/session/{sessionID}/message": {
                "get": { "summary": "List messages" }
            },
            "/api/session/{sessionID}/prompt": {
                "post": { "summary": "Admit a prompt and start generation" }
            },
            "/api/session/{sessionID}/interrupt": {
                "post": { "summary": "Interrupt the active generation" }
            },
            "/api/session/{sessionID}/fork": {
                "post": { "summary": "Fork session history before a message" }
            },
            "/api/session/{sessionID}/revert/stage": {
                "post": { "summary": "Stage a session revert" }
            },
            "/api/session/{sessionID}/revert": {
                "delete": { "summary": "Clear a staged session revert" }
            },
            "/api/session/{sessionID}/revert/commit": {
                "post": { "summary": "Commit a staged session revert" }
            },
            "/api/session/{sessionID}/permission": {
                "get": { "summary": "List session permission requests" },
                "post": { "summary": "Create permission request" }
            },
            "/api/session/{sessionID}/permission/{requestID}": {
                "get": { "summary": "Get permission request" }
            },
            "/api/session/{sessionID}/permission/{requestID}/reply": {
                "post": { "summary": "Reply to permission request" }
            },
            "/api/permission/request": {
                "get": { "summary": "List pending permission requests" }
            },
            "/api/permission/saved": {
                "get": { "summary": "List saved permissions" }
            },
            "/api/permission/saved/{id}": {
                "patch": { "summary": "Update saved permission" },
                "delete": { "summary": "Remove saved permission" }
            },
            "/api/event": {
                "get": { "summary": "Server-sent event stream" }
            }
        }
    })
}

fn web_page() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Clawcode</title>
<style>
:root { color-scheme: dark; }
body { margin: 0; font: 15px/1.5 ui-monospace, SFMono-Regular, Consolas, monospace; background: #141414; color: #e8e6e1; }
main { max-width: 56rem; margin: 0 auto; padding: 3rem 1.5rem; }
h1 { font-size: 1.25rem; font-weight: 600; margin: 0 0 0.5rem; }
p { color: #a8a49c; margin: 0 0 2rem; }
section { border-top: 1px solid #2e2c29; padding: 1.25rem 0; }
h2 { font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.08em; color: #8c8880; margin: 0 0 0.75rem; }
ul { list-style: none; margin: 0; padding: 0; }
li { padding: 0.5rem 0; border-bottom: 1px solid #242220; }
li:last-child { border-bottom: 0; }
a { color: #e8e6e1; }
small { color: #8c8880; }
.empty { color: #8c8880; }
</style>
</head>
<body>
<main>
<h1>Clawcode</h1>
<p>Local server. The terminal interface is still the primary surface.</p>
<section>
<h2>Sessions</h2>
<ul id="sessions"><li class="empty">Loading</li></ul>
</section>
<section>
<h2>API</h2>
<ul>
<li><a href="/doc">OpenAPI document</a></li>
<li><a href="/global/health">Health check</a></li>
</ul>
</section>
</main>
<script>
fetch('/session').then((response) => response.json()).then((sessions) => {
  const list = document.getElementById('sessions');
  if (!Array.isArray(sessions) || sessions.length === 0) {
    list.innerHTML = '<li class="empty">No sessions yet</li>';
    return;
  }
  list.innerHTML = sessions.map((session) => {
    const title = document.createElement('span');
    title.textContent = session.title;
    const meta = document.createElement('small');
    const row = document.createElement('li');
    meta.textContent = ' ' + session.id;
    row.append(title, meta);
    return row.outerHTML;
  }).join('');
}).catch(() => {
  document.getElementById('sessions').innerHTML = '<li class="empty">Could not load sessions</li>';
});
</script>
</body>
</html>
"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> ServerState {
        ServerState {
            bus: EventBus::new(),
            runtime: None,
            provider: "test-provider".to_string(),
            model: "test-model".to_string(),
            runtime_error: None,
            event_counter: AtomicU64::new(1),
            permission_counter: AtomicU64::new(1),
            pending_permissions: std::sync::Mutex::new(Vec::new()),
            directory: "D:\\work".to_string(),
        }
    }

    fn runtime_event(
        kind: &str,
        session_id: i64,
        generation_id: Option<i64>,
        payload: Value,
    ) -> RuntimeEvent {
        RuntimeEvent {
            seq: 0,
            session_id,
            generation_id,
            kind: kind.to_string(),
            payload_json: payload.to_string(),
        }
    }

    #[test]
    fn session_paths_accept_v1_v2_and_prefixed_ids() {
        assert_eq!(
            parse_session_path("/session/ses_7/message"),
            Some((7, vec!["message"]))
        );
        assert_eq!(
            parse_session_path("/api/session/7/prompt"),
            Some((7, vec!["prompt"]))
        );
        assert_eq!(parse_session_path("/api/session/nope"), None);
    }

    #[test]
    fn sse_frames_match_opencode_data_lines() {
        assert_eq!(
            sse_frame(&json!({ "type": "server.connected", "data": {} })),
            "data: {\"data\":{},\"type\":\"server.connected\"}\n\n"
        );
    }

    #[test]
    fn rfc3339_millis_parses_utc_timestamps() {
        assert_eq!(rfc3339_millis("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(rfc3339_millis("1970-01-02T00:00:01.250Z"), Some(86_401_250));
        assert_eq!(rfc3339_millis("not a timestamp"), None);
    }

    #[test]
    fn query_parameters_are_read_without_decoding_numbers() {
        assert_eq!(
            query_param("/api/event?sessionID=ses_7&limit=2", "sessionID"),
            Some("ses_7")
        );
        assert_eq!(query_param("/api/event", "sessionID"), None);
    }

    #[test]
    fn prompt_parser_accepts_text_and_rejects_unsupported_attachments() {
        let (text, delivery) = parse_prompt(r#"{"text":"hello","delivery":"queue"}"#).unwrap();
        assert_eq!(text, "hello");
        assert_eq!(delivery, "queue");
        assert!(parse_prompt(r#"{"text":"   "}"#).is_err());
        assert!(parse_prompt(r#"{"text":"hello","files":[{"uri":"file:///a"}]}"#).is_err());
        assert!(parse_prompt(r#"{"text":"hello","delivery":"steer"}"#).is_err());
    }

    #[test]
    fn fork_parser_accepts_optional_prefixed_message_ids() {
        assert_eq!(parse_before("").unwrap(), None);
        assert_eq!(parse_before("{}").unwrap(), None);
        assert_eq!(parse_before(r#"{"before":"msg_42"}"#).unwrap(), Some(42));
        assert!(parse_before(r#"{"before":"42"}"#).is_err());
        assert!(parse_before(r#"{"before":42}"#).is_err());
        assert!(parse_before("not json").is_err());
    }

    #[test]
    fn revert_parser_requires_message_id_and_boolean_files() {
        assert_eq!(parse_revert_stage(r#"{"messageID":"msg_7"}"#).unwrap(), 7);
        assert_eq!(
            parse_revert_stage(r#"{"messageID":"msg_7","files":false}"#).unwrap(),
            7
        );
        assert!(parse_revert_stage(r#"{"messageID":"7"}"#).is_err());
        assert!(parse_revert_stage(r#"{"messageID":"msg_7","files":"no"}"#).is_err());
        assert!(parse_revert_stage("{}").is_err());
    }

    #[test]
    fn public_revert_matches_the_protocol_shape() {
        assert_eq!(
            public_revert(5),
            json!({ "messageID": "msg_5", "files": [] })
        );
    }

    #[test]
    fn public_session_includes_fork_parent_boundary_and_revert() {
        let summary = SessionSummary {
            id: 11,
            title: "child".to_string(),
            workspace_id: 1,
            created_at: "1970-01-01T00:00:00.000Z".to_string(),
            updated_at: "1970-01-01T00:00:01.000Z".to_string(),
            pinned: false,
            archived: false,
            parent_id: Some(7),
            fork_boundary_type: Some(crate::persistence::ForkBoundaryType::Before),
            fork_boundary_message_id: Some(42),
            revert_message_id: Some(9),
        };
        let value = public_session_summary(&summary, "D:\\work");
        assert_eq!(value["parentID"], "ses_7");
        assert_eq!(value["fork"]["sessionID"], "ses_7");
        assert_eq!(value["fork"]["boundary"]["type"], "before");
        assert_eq!(value["fork"]["boundary"]["messageID"], "msg_42");
        assert_eq!(value["revert"]["messageID"], "msg_9");
    }

    #[test]
    fn permission_parser_validates_tool_sources_and_optional_fields() {
        let request = parse_new_permission(
            r#"{"id":"per_7","action":"bash","resources":["git status"],"save":["git *"],"metadata":{"path":"a"},"source":{"type":"tool","messageID":"msg_1","id":"call_1"}}"#,
        )
        .unwrap();
        assert_eq!(request.id.as_deref(), Some("per_7"));
        assert_eq!(request.action, "bash");
        assert_eq!(request.resources, vec!["git status".to_string()]);
        assert_eq!(request.save, Some(vec!["git *".to_string()]));
        assert_eq!(request.metadata.as_ref().unwrap()["path"], "a");
        assert!(request.source.is_some());

        assert!(parse_new_permission(r#"{"action":"bash"}"#).is_err());
        assert!(parse_new_permission(r#"{"action":"bash","resources":"no"}"#).is_err());
        assert!(parse_new_permission(r#"{"action":"bash","resources":[],"save":[1]}"#).is_err());
        assert!(
            parse_new_permission(r#"{"action":"bash","resources":[],"source":{"type":"http"}}"#)
                .is_err()
        );
        assert!(parse_new_permission(r#"{"id":"x","action":"bash","resources":[]}"#).is_err());
    }

    #[test]
    fn saved_permissions_use_glob_rules_and_empty_resources_allow() {
        let rules = vec![SavedPermission {
            id: 1,
            project_id: PROJECT_ID.to_string(),
            action: "bash".to_string(),
            resource: "git *".to_string(),
            created_at: "1970-01-01T00:00:00.000Z".to_string(),
            updated_at: "1970-01-01T00:00:00.000Z".to_string(),
        }];
        assert_eq!(
            permission_effect(&rules, "bash", &["git status".to_string()]),
            "allow"
        );
        assert_eq!(
            permission_effect(&rules, "bash", &["cargo test".to_string()]),
            "ask"
        );
        assert_eq!(permission_effect(&[], "bash", &[]), "allow");
        assert!(wildcard_match("src/main.rs", "src/*.rs"));
        assert!(wildcard_match("src\\main.rs", "src/*.rs"));
        assert!(wildcard_match("a1", "a?"));
        assert!(!wildcard_match("a12", "a?"));
        assert!(!wildcard_match("cargo test", "git *"));
    }

    #[test]
    fn permission_payloads_and_events_match_the_protocol_shape() {
        let state = test_state();
        state
            .pending_permissions
            .lock()
            .unwrap()
            .push(PermissionRequest {
                id: "per_1".to_string(),
                session_id: 3,
                action: "bash".to_string(),
                resources: vec!["git status".to_string()],
                save: Some(vec!["git *".to_string()]),
                metadata: Some(json!({ "path": "a" })),
                source: None,
                message: None,
            });
        let pending = pending_permission_requests(&state);
        assert_eq!(pending[0]["sessionID"], "ses_3");
        assert_eq!(pending[0]["resources"][0], "git status");
        assert_eq!(pending[0]["save"][0], "git *");

        let mut translator = EventTranslator::default();
        let events = translator.translate(
            &runtime_event(
                "permission.asked",
                3,
                None,
                json!({ "id": "per_1", "sessionID": "ses_3" }),
            ),
            &state,
        );
        assert_eq!(events[0]["type"], "permission.asked");
        assert_eq!(events[0]["data"]["id"], "per_1");

        assert_eq!(
            parse_permission_reply(r#"{"decision":"once"}"#).unwrap(),
            "once"
        );
        assert_eq!(
            parse_permission_reply(r#"{"decision":"reject","message":"no"}"#).unwrap(),
            "reject"
        );
        assert!(parse_permission_reply(r#"{"decision":"sometimes"}"#).is_err());
        assert!(parse_permission_reply(r#"{"decision":"once","message":1}"#).is_err());
    }

    #[test]
    fn public_saved_permission_uses_prefixed_id_and_millis() {
        let saved = SavedPermission {
            id: 4,
            project_id: PROJECT_ID.to_string(),
            action: "edit".to_string(),
            resource: "src/*".to_string(),
            created_at: "1970-01-01T00:00:01.000Z".to_string(),
            updated_at: "1970-01-01T00:00:02.000Z".to_string(),
        };
        assert_eq!(
            public_saved_permission(&saved),
            json!({
                "id": "psv_4",
                "projectID": PROJECT_ID,
                "action": "edit",
                "resource": "src/*",
                "time": { "created": 1000, "updated": 2000 },
            })
        );
    }

    #[test]
    fn text_deltas_start_and_end_one_assistant_message() {
        let state = test_state();
        let mut translator = EventTranslator::default();
        let started = translator.translate(
            &runtime_event("text_delta", 1, Some(2), json!({ "delta": "hel" })),
            &state,
        );
        assert_eq!(started.len(), 2);
        assert_eq!(started[0]["type"], "session.text.started");
        assert_eq!(started[0]["data"]["assistantMessageID"], "msg_s1_g2");
        assert_eq!(started[1]["type"], "session.text.delta");
        assert_eq!(started[1]["data"]["delta"], "hel");

        let ended = translator.translate(
            &runtime_event(
                "assistant_message",
                1,
                Some(2),
                json!({ "content": "hello" }),
            ),
            &state,
        );
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0]["type"], "session.text.ended");
        assert_eq!(ended[0]["data"]["text"], "hello");
    }

    #[test]
    fn tool_input_is_reassembled_before_settlement() {
        let state = test_state();
        let mut translator = EventTranslator::default();
        translator.translate(
            &runtime_event(
                "tool_call_start",
                1,
                Some(2),
                json!({ "id": "call_1", "name": "read" }),
            ),
            &state,
        );
        translator.translate(
            &runtime_event(
                "tool_call_delta",
                1,
                Some(2),
                json!({ "id": "call_1", "arguments": "{\"path\":" }),
            ),
            &state,
        );
        let ended = translator.translate(
            &runtime_event("tool_call_end", 1, Some(2), json!({ "id": "call_1" })),
            &state,
        );
        assert_eq!(ended[0]["type"], "session.tool.input.ended");
        assert_eq!(ended[0]["data"]["text"], "{\"path\":");

        let settled = translator.translate(
            &runtime_event(
                "tool_call_settled",
                1,
                Some(2),
                json!({
                    "call_id": "call_1",
                    "status": "completed",
                    "result": "file body",
                    "metadata": { "path": "a.txt" },
                }),
            ),
            &state,
        );
        assert_eq!(settled[0]["type"], "session.tool.success");
        assert_eq!(settled[0]["data"]["content"][0]["text"], "file body");
        assert_eq!(settled[0]["data"]["metadata"]["path"], "a.txt");
    }

    #[test]
    fn failed_generation_carries_the_runtime_error_and_idles() {
        let state = test_state();
        let mut translator = EventTranslator::default();
        translator.translate(
            &runtime_event(
                "error",
                4,
                Some(9),
                json!({ "message": "provider unavailable" }),
            ),
            &state,
        );
        let finished = translator.translate(
            &runtime_event(
                "generation_finished",
                4,
                Some(9),
                json!({ "status": "failed" }),
            ),
            &state,
        );
        assert_eq!(finished[0]["type"], "session.execution.failed");
        assert_eq!(
            finished[0]["data"]["error"]["message"],
            "provider unavailable"
        );
        assert_eq!(finished[1]["type"], "session.idle");
    }

    #[test]
    fn successful_generation_emits_execution_and_idle() {
        let state = test_state();
        let mut translator = EventTranslator::default();
        let finished = translator.translate(
            &runtime_event(
                "generation_finished",
                3,
                Some(5),
                json!({ "status": "completed" }),
            ),
            &state,
        );
        assert_eq!(finished[0]["type"], "session.execution.succeeded");
        assert_eq!(finished[1]["type"], "session.idle");
        assert_eq!(finished[0]["durable"]["aggregateID"], "ses_3");
    }
}
