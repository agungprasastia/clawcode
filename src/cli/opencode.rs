//! OpenCode-compatible command dispatch.
//!
//! The command names, flags, help layout, and success/error text are kept in
//! `spec.rs`; this module owns the behaviour behind each command.

use crate::cli::spec::{
    Invocation, ParseOutcome, completion_script, parse, render_help, root_spec,
};
use crate::config::{Config, ConfigLoader, jsonc};
use crate::persistence::{Db, SessionSummary};
use crate::runtime::client::RuntimeClient;
use crate::runtime::{EventBus, RuntimeEvent};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug)]
pub struct CliError(pub String);

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<String> for CliError {
    fn from(error: String) -> Self {
        Self(error)
    }
}

impl From<rusqlite::Error> for CliError {
    fn from(error: rusqlite::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<crate::config::ConfigDiagnostic> for CliError {
    fn from(error: crate::config::ConfigDiagnostic) -> Self {
        Self(error.to_string())
    }
}

pub fn run(args: Vec<String>) -> Result<(), CliError> {
    let root = root_spec();
    match parse(&root, &args).map_err(|error| CliError(error.0))? {
        ParseOutcome::Help(spec, path) => {
            print!("{}", render_help(&spec, &path));
            Ok(())
        }
        ParseOutcome::Version => {
            println!("clawcode v{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ParseOutcome::Completions(shell) => {
            print!("{}", completion_script(&shell, &root));
            Ok(())
        }
        ParseOutcome::Invocation(invocation) => dispatch(invocation),
    }
}

fn dispatch(invocation: Invocation) -> Result<(), CliError> {
    let command: Vec<&str> = invocation.subcommand().iter().map(String::as_str).collect();
    match command.as_slice() {
        [] => launch_default(&invocation),
        ["upgrade"] => handle_upgrade(&invocation),
        ["uninstall"] => handle_uninstall(&invocation),
        ["acp"] => handle_acp(&invocation),
        ["api"] => handle_api(&invocation),
        ["debug", "agents"] => debug_agents(),
        ["debug", "config"] => debug_config(),
        ["debug", "paths"] => debug_paths(&invocation),
        ["auth", "list"] => auth_list(&invocation),
        ["auth", "login"] => auth_login(&invocation),
        ["auth", "logout"] => auth_logout(&invocation),
        ["auth", "export"] => auth_export(&invocation),
        ["auth", "import"] => auth_import(&invocation),
        ["auth", "switch"] => auth_switch(&invocation),
        ["mcp", "list"] => mcp_list(),
        ["mcp", "add"] => mcp_add(&invocation),
        ["mcp", "auth"] => mcp_auth(&invocation),
        ["mcp", "logout"] => mcp_logout(&invocation),
        ["plugin", "list"] => plugin_list(&invocation),
        ["plugin", "add"] => plugin_add(&invocation),
        ["plugin", "check"] => plugin_check(&invocation),
        ["plugin", "update"] => plugin_update(&invocation),
        ["plugin", "remove"] => plugin_remove(&invocation),
        ["models"] => models(&invocation),
        ["stats"] => stats(&invocation),
        ["mini"] => launch_mini(&invocation),
        ["run"] => run_prompt(&invocation),
        ["session", "list"] => session_list(&invocation),
        ["session", "delete"] => session_delete(&invocation),
        ["session", "export"] => session_export(&invocation),
        ["session", "import"] => session_import(&invocation),
        ["service", "status"] => service_status(),
        ["service", "start" | "restart" | "stop"] => Err(CliError(
            "The background server is not available in this build.".into(),
        )),
        ["service", "get"] => service_get(&invocation),
        ["service", "set"] => service_set(&invocation),
        ["service", "unset"] => service_unset(&invocation),
        ["reload"] => reload(),
        ["pair"] => pair(&invocation),
        ["serve"] => crate::cli::server::serve(&invocation),
        _ => Err(CliError(format!(
            "Unrecognized command: {}",
            invocation.path.join(" ")
        ))),
    }
}

fn launch_default(invocation: &Invocation) -> Result<(), CliError> {
    if let Some(directory) = invocation.argument("directory") {
        std::env::set_current_dir(directory).map_err(|error| {
            CliError(format!(
                "Error: {}: no such file or directory, chdir '{}' -> '{}'",
                error,
                std::env::current_dir()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| ".".into()),
                directory
            ))
        })?;
    }
    export_initial_context(invocation);
    crate::tui::run().map_err(|error| CliError(error.to_string()))
}

fn launch_mini(invocation: &Invocation) -> Result<(), CliError> {
    export_initial_context(invocation);
    crate::tui::run().map_err(|error| CliError(error.to_string()))
}

fn export_initial_context(invocation: &Invocation) {
    // The TUI reads these once at startup. Keeping the hand-off in the
    // environment avoids a second argument parser in the render thread.
    for (flag, key) in [
        ("prompt", "CLAWCODE_INITIAL_PROMPT"),
        ("session", "CLAWCODE_INITIAL_SESSION"),
        ("model", "CLAWCODE_INITIAL_MODEL"),
        ("agent", "CLAWCODE_INITIAL_AGENT"),
    ] {
        if let Some(value) = invocation.flag(flag) {
            // SAFETY: the process is single-threaded at this point on the
            // command startup path, before the TUI or runtime spawns workers.
            unsafe { std::env::set_var(key, value) };
        }
    }
}

fn print_json(value: &Value) -> Result<(), CliError> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| CliError(error.to_string()))?
    );
    Ok(())
}

pub(crate) fn load_config() -> Result<Config, CliError> {
    ConfigLoader.load().map_err(CliError::from)
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn config_home() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"));
    base.join("clawcode")
}

pub(crate) fn data_dir() -> PathBuf {
    crate::tui::default_data_dir()
}

fn cache_dir() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        return PathBuf::from(local).join("clawcode").join("cache");
    }
    data_dir().join("cache")
}

fn state_dir() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        return PathBuf::from(local).join("clawcode").join("state");
    }
    data_dir().join("state")
}

fn tmp_dir() -> PathBuf {
    std::env::temp_dir().join("clawcode")
}

fn paths() -> Vec<(&'static str, PathBuf)> {
    let data = data_dir();
    vec![
        ("home", home_dir()),
        ("data", data.clone()),
        ("cache", cache_dir()),
        ("config", config_home()),
        ("state", state_dir()),
        ("tmp", tmp_dir()),
        ("bin", cache_dir().join("bin")),
        ("log", data.join("log")),
        ("repos", data.join("repos")),
        ("db", data.join("clawcode.db")),
    ]
}

fn debug_paths(invocation: &Invocation) -> Result<(), CliError> {
    let entries = paths();
    if let Some(name) = invocation.argument("name") {
        let path = entries
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, path)| path)
            .ok_or_else(|| CliError(format!("Unknown path: {name}")))?;
        println!("{}", path.display());
        return Ok(());
    }
    for (name, path) in entries {
        println!("{name:<10} {}", path.display());
    }
    Ok(())
}

fn global_config_candidates() -> Vec<PathBuf> {
    let base = config_home()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home_dir().join(".config"));
    vec![
        base.join("clawcode").join("config.jsonc"),
        base.join("clawcode").join("clawcode.jsonc"),
        base.join("clawcode").join("clawcode.json"),
        base.join("opencode").join("opencode.jsonc"),
        base.join("opencode").join("opencode.json"),
    ]
}

fn project_config_candidates() -> Vec<PathBuf> {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    vec![
        root.join(".clawcode").join("config.jsonc"),
        root.join(".clawcode").join("clawcode.jsonc"),
        root.join(".clawcode").join("clawcode.json"),
        root.join("clawcode.jsonc"),
        root.join("clawcode.json"),
        root.join(".opencode").join("opencode.jsonc"),
        root.join(".opencode").join("opencode.json"),
        root.join("opencode.jsonc"),
        root.join("opencode.json"),
    ]
}

fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.is_file())
}

fn debug_config() -> Result<(), CliError> {
    let global = first_existing(global_config_candidates());
    let project = first_existing(project_config_candidates());
    let mut documents = Vec::new();
    for path in global.into_iter().chain(project) {
        let content = fs::read_to_string(&path)?;
        let info = match jsonc::parse(&content) {
            Ok(value) => jsonc_to_json(&value),
            Err((line, column, message)) => {
                return Err(CliError(format!(
                    "{}:{line}:{column}: {message}",
                    path.display()
                )));
            }
        };
        documents.push(json!({
            "type": "document",
            "path": path.display().to_string(),
            "info": info,
        }));
    }
    let mut directories = Vec::new();
    for path in [config_home(), config_home().join("..").join("opencode")] {
        if path.is_dir() {
            directories.push(json!({
                "type": "directory",
                "path": path.display().to_string(),
            }));
        }
    }
    documents.extend(directories);
    print_json(&Value::Array(documents))
}

fn jsonc_to_json(value: &jsonc::Value) -> Value {
    match value {
        jsonc::Value::Null { .. } => Value::Null,
        jsonc::Value::Bool { value, .. } => Value::Bool(*value),
        jsonc::Value::Number { value, .. } => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        jsonc::Value::String { value, .. } => Value::String(value.clone()),
        jsonc::Value::Array { items, .. } => {
            Value::Array(items.iter().map(jsonc_to_json).collect())
        }
        jsonc::Value::Object { members, .. } => {
            let mut map = Map::new();
            for (key, value) in members {
                map.insert(key.clone(), jsonc_to_json(value));
            }
            Value::Object(map)
        }
    }
}

fn debug_agents() -> Result<(), CliError> {
    let config = load_config()?;
    let mut agents = vec![
        json!({
            "id": "build",
            "name": "Build",
            "request": { "settings": {}, "headers": {}, "body": {} },
            "description": "The default agent. Executes tools based on configured permissions.",
            "mode": "primary",
            "hidden": false,
            "permissions": [
                { "action": "*", "resource": "*", "effect": "allow" }
            ],
        }),
        json!({
            "id": "explore",
            "name": "Explore",
            "request": { "settings": {}, "headers": {}, "body": {} },
            "description": "Fast agent for exploring the codebase.",
            "mode": "subagent",
            "hidden": false,
            "permissions": [
                { "action": "read", "resource": "*", "effect": "allow" }
            ],
        }),
        json!({
            "id": "general",
            "name": "General",
            "request": { "settings": {}, "headers": {}, "body": {} },
            "description": "General-purpose agent for multi-step work.",
            "mode": "subagent",
            "hidden": false,
            "permissions": [
                { "action": "*", "resource": "*", "effect": "allow" }
            ],
        }),
        json!({
            "id": "plan",
            "name": "Plan",
            "request": { "settings": {}, "headers": {}, "body": {} },
            "description": "Read-only agent for planning and analysis.",
            "mode": "primary",
            "hidden": false,
            "permissions": [
                { "action": "*", "resource": "*", "effect": "deny" },
                { "action": "read", "resource": "*", "effect": "allow" }
            ],
        }),
        json!({
            "id": "compaction",
            "name": "Compaction",
            "mode": "primary",
            "hidden": true,
        }),
        json!({
            "id": "summary",
            "name": "Summary",
            "mode": "primary",
            "hidden": true,
        }),
        json!({
            "id": "title",
            "name": "Title",
            "mode": "primary",
            "hidden": true,
        }),
    ];
    for (name, agent) in &config.agents {
        let mut value = json!({
            "id": name,
            "name": name,
            "mode": "primary",
            "hidden": false,
            "request": { "settings": {}, "headers": {}, "body": {} },
        });
        if let Some(model) = &agent.model {
            value["model"] = Value::String(model.clone());
        }
        if let Some(temperature) = agent.temperature {
            value["temperature"] = json!(temperature);
        }
        agents.push(value);
    }
    print_json(&Value::Array(agents))
}

fn models(invocation: &Invocation) -> Result<(), CliError> {
    let config = load_config()?;
    let mut models = BTreeSet::new();
    for (provider, provider_config) in &config.providers {
        for model in provider_config.models.keys() {
            models.insert(format!("{provider}/{model}"));
        }
    }
    if let Some(selected) = &config.model {
        models.insert(selected.clone());
    }
    if models.is_empty() {
        return Err(CliError(
            "No models available. Add a provider and model to your configuration.".into(),
        ));
    }
    let _ = invocation;
    for model in models {
        println!("{model}");
    }
    Ok(())
}

fn session_summaries(db: &Db, limit: usize) -> Result<Vec<SessionSummary>, CliError> {
    db.session_summaries(false, limit).map_err(CliError::from)
}

fn session_list(invocation: &Invocation) -> Result<(), CliError> {
    let limit = invocation
        .flag("max-count")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| CliError("--max-count expects a positive integer".into()))?
        .unwrap_or(100)
        .max(1);
    let format = invocation.flag("format").unwrap_or("table");
    let db = open_db()?;
    let sessions = session_summaries(&db, limit)?;
    if format == "json" {
        let values: Vec<Value> = sessions
            .iter()
            .map(|session| {
                json!({
                    "id": session_id(session.id),
                    "title": session.title,
                    "updated": timestamp_to_millis(&session.updated_at),
                    "created": timestamp_to_millis(&session.created_at),
                    "projectId": "local",
                    "directory": std::env::current_dir()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default(),
                    "pinned": session.pinned,
                })
            })
            .collect();
        return print_json(&Value::Array(values));
    }
    for session in sessions {
        println!(
            "{}\t{}\t{}",
            session_id(session.id),
            session.title,
            format_timestamp(&session.updated_at)
        );
    }
    Ok(())
}

fn session_delete(invocation: &Invocation) -> Result<(), CliError> {
    let raw = invocation
        .argument("sessionID")
        .ok_or_else(|| CliError("Missing session ID".into()))?;
    let id = parse_session_id(raw)?;
    let db = open_db()?;
    if db.session(id)?.is_none() {
        return Err(CliError(format!("Session not found: {raw}")));
    }
    db.delete_session(id)?;
    println!("Deleted session {raw}");
    Ok(())
}

fn session_export(invocation: &Invocation) -> Result<(), CliError> {
    let db = open_db()?;
    let selected = match invocation.argument("session") {
        Some(raw) => parse_session_id(raw)?,
        None => session_summaries(&db, 1)?
            .first()
            .map(|summary| summary.id)
            .ok_or_else(|| CliError("No sessions found".into()))?,
    };
    let session = db
        .session(selected)?
        .ok_or_else(|| CliError(format!("Session not found: {selected}")))?;
    let summaries = session_summaries(&db, 1_000)?;
    let summary = summaries.iter().find(|summary| summary.id == selected);
    let sanitize = invocation.flag_present("sanitize");
    let mut messages = Vec::new();
    for (index, message) in db.messages(selected)?.into_iter().enumerate() {
        let text = if sanitize {
            "[redacted]".to_string()
        } else {
            message.content
        };
        messages.push(json!({
            "id": format!("msg_{index}"),
            "role": message.role,
            "type": message.role,
            "text": text,
        }));
    }
    let info = json!({
        "id": session_id(session.id),
        "title": session.title,
        "time": {
            "created": summary.map(|value| timestamp_to_millis(&value.created_at)).unwrap_or(0),
            "updated": summary.map(|value| timestamp_to_millis(&value.updated_at)).unwrap_or(0),
        },
        "location": {
            "directory": std::env::current_dir()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        },
    });
    print_json(&json!({ "info": info, "messages": messages }))
}

fn session_import(invocation: &Invocation) -> Result<(), CliError> {
    let source = invocation
        .argument("file")
        .ok_or_else(|| CliError("Missing import file".into()))?;
    let text = read_source(source)?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| CliError(format!("Invalid session JSON: {error}")))?;
    let title = value
        .get("info")
        .and_then(|info| info.get("title"))
        .and_then(Value::as_str)
        .or_else(|| value.get("title").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| {
            Path::new(source)
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Imported session")
                .to_string()
        });
    if let Some(directory) = invocation.flag("directory")
        && !Path::new(directory).is_dir()
    {
        return Err(CliError(format!("Directory not found: {directory}")));
    }
    let db = open_db()?;
    let session = db.create_session(&title)?;
    if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        for message in messages {
            let role = message
                .get("role")
                .or_else(|| message.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("user");
            if role != "user" && role != "assistant" {
                continue;
            }
            let content = message_text(message);
            if content.trim().is_empty() {
                continue;
            }
            db.append_message(session.id, role, &content)?;
        }
    }
    println!("{}", session_id(session.id));
    Ok(())
}

fn message_text(message: &Value) -> String {
    if let Some(text) = message.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.as_str())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn open_db() -> Result<Db, CliError> {
    let data = data_dir();
    fs::create_dir_all(&data)?;
    Db::open(&data.join("clawcode.db")).map_err(CliError::from)
}

fn session_id(id: i64) -> String {
    format!("ses_{id}")
}

fn parse_session_id(raw: &str) -> Result<i64, CliError> {
    let body = raw.strip_prefix("ses_").unwrap_or(raw);
    body.parse::<i64>()
        .map_err(|_| CliError(format!("Session not found: {raw}")))
}

fn timestamp_to_millis(value: &str) -> i64 {
    // The database stores ISO-8601 UTC with milliseconds. Parsing the numeric
    // tail keeps this dependency-free while still matching the API shape.
    let Some((date, time)) = value.split_once('T') else {
        return 0;
    };
    let mut date_parts = date.split('-');
    let year = date_parts.next().and_then(|part| part.parse::<i64>().ok());
    let month = date_parts.next().and_then(|part| part.parse::<i64>().ok());
    let day = date_parts.next().and_then(|part| part.parse::<i64>().ok());
    let (Some(year), Some(month), Some(day)) = (year, month, day) else {
        return 0;
    };
    let time = time.trim_end_matches('Z');
    let mut time_parts = time.split(':');
    let hour = time_parts.next().and_then(|part| part.parse::<i64>().ok());
    let minute = time_parts.next().and_then(|part| part.parse::<i64>().ok());
    let second = time_parts
        .next()
        .and_then(|part| part.split('.').next())
        .and_then(|part| part.parse::<i64>().ok());
    let (Some(hour), Some(minute), Some(second)) = (hour, minute, second) else {
        return 0;
    };
    days_from_civil(year, month, day) * 86_400_000
        + hour * 3_600_000
        + minute * 60_000
        + second * 1_000
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn format_timestamp(value: &str) -> String {
    let Some((date, time)) = value.split_once('T') else {
        return value.to_string();
    };
    let mut date_parts = date.split('-');
    let year = date_parts.next().unwrap_or("");
    let month = date_parts.next().unwrap_or("");
    let day = date_parts.next().unwrap_or("");
    let time = time.trim_end_matches('Z');
    let mut time_parts = time.split(':');
    let hour = time_parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0);
    let minute = time_parts.next().unwrap_or("00");
    let second = time_parts
        .next()
        .and_then(|part| part.split('.').next())
        .unwrap_or("00");
    let suffix = if hour < 12 { "AM" } else { "PM" };
    let hour = match hour % 12 {
        0 => 12,
        value => value,
    };
    format!("{month}/{day}/{year}, {hour}:{minute}:{second} {suffix}")
}

fn auth_list(invocation: &Invocation) -> Result<(), CliError> {
    let config = load_config()?;
    let mut accounts = Vec::new();
    for (provider, provider_config) in &config.providers {
        let stored = provider_config.resolved_api_key().is_some();
        accounts.push(json!({
            "provider": provider,
            "account": "Default",
            "status": if stored { "configured" } else { "not configured" },
        }));
    }
    if invocation.flag("format") == Some("json") {
        return print_json(&Value::Array(accounts));
    }
    if accounts.is_empty() {
        println!("No integrations found");
        return Ok(());
    }
    for account in accounts {
        println!(
            "{}  {}  {}",
            account["provider"].as_str().unwrap_or(""),
            account["account"].as_str().unwrap_or(""),
            account["status"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

pub(crate) fn auth_login(invocation: &Invocation) -> Result<(), CliError> {
    let target = invocation.argument("target").unwrap_or("provider");
    Err(CliError(format!(
        "Interactive login for {target} is not available. Set api_key to env:NAME or credential:NAME in your configuration."
    )))
}

fn auth_logout(invocation: &Invocation) -> Result<(), CliError> {
    let target = invocation.argument("target").unwrap_or("credentials");
    Err(CliError(format!(
        "No stored credentials to remove for {target}."
    )))
}

fn auth_export(_invocation: &Invocation) -> Result<(), CliError> {
    println!("[]");
    Ok(())
}

fn auth_import(invocation: &Invocation) -> Result<(), CliError> {
    let source = invocation.argument("file");
    if let Some(source) = source {
        let text = fs::read_to_string(source)?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| CliError(format!("Invalid credential JSON: {error}")))?;
        if !value.is_array() && !value.is_object() {
            return Err(CliError(
                "Credential JSON must be an array or object".into(),
            ));
        }
    }
    Err(CliError(
        "Importing stored credentials is not available. Use environment-backed api_key entries."
            .into(),
    ))
}

fn auth_switch(_invocation: &Invocation) -> Result<(), CliError> {
    Err(CliError(
        "No alternate stored accounts are available.".into(),
    ))
}

fn mcp_list() -> Result<(), CliError> {
    let config = read_merged_config_json()?;
    let servers = config
        .get("mcp")
        .and_then(|mcp| mcp.get("servers").or(Some(mcp)))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if servers.is_empty() {
        println!("No MCP servers configured");
        return Ok(());
    }
    for (name, server) in servers {
        let disabled = server
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let status = if disabled { "disabled" } else { "configured" };
        println!("- {name}  {status}");
    }
    Ok(())
}

fn mcp_add(invocation: &Invocation) -> Result<(), CliError> {
    let name = invocation
        .argument("name")
        .ok_or_else(|| CliError("Missing MCP server name".into()))?;
    let command = invocation.argument_values("command");
    let url = invocation.flag("url");
    if command.is_empty() == url.is_none() {
        return Err(CliError(
            "Provide either --url or a command after --".into(),
        ));
    }
    let target = config_write_target(invocation.flag_present("global"))?;
    let mut root = read_config_json(&target)?;
    let root_object = root
        .as_object_mut()
        .ok_or_else(|| CliError("Configuration root must be an object".into()))?;
    let mcp = root_object
        .entry("mcp")
        .or_insert_with(|| json!({ "servers": {} }));
    let servers = mcp
        .as_object_mut()
        .ok_or_else(|| CliError("mcp must be an object".into()))?
        .entry("servers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| CliError("mcp.servers must be an object".into()))?;
    let mut server = if let Some(url) = url {
        let mut entries = Map::new();
        entries.insert("type".into(), Value::String("remote".into()));
        entries.insert("url".into(), Value::String(url.to_string()));
        let headers = parse_pairs(invocation.flag_values("header"))?;
        if !headers.is_empty() {
            entries.insert("headers".into(), Value::Object(headers));
        }
        Value::Object(entries)
    } else {
        let mut entries = Map::new();
        entries.insert("type".into(), Value::String("local".into()));
        entries.insert(
            "command".into(),
            Value::Array(
                command
                    .iter()
                    .map(|value| Value::String(value.clone()))
                    .collect(),
            ),
        );
        let env = parse_pairs(invocation.flag_values("env"))?;
        if !env.is_empty() {
            entries.insert("environment".into(), Value::Object(env));
        }
        Value::Object(entries)
    };
    server["disabled"] = Value::Bool(false);
    servers.insert(name.to_string(), server);
    write_config_json(&target, &root)?;
    println!("Added MCP server {name} to {}", target.display());
    Ok(())
}

fn mcp_auth(invocation: &Invocation) -> Result<(), CliError> {
    let name = invocation.argument("name").unwrap_or("server");
    Err(CliError(format!(
        "OAuth authentication for {name} is not available. Use a local server or a static header."
    )))
}

fn mcp_logout(invocation: &Invocation) -> Result<(), CliError> {
    let name = invocation.argument("name").unwrap_or("server");
    Err(CliError(format!(
        "No OAuth credentials are stored for {name}."
    )))
}

fn plugin_list(invocation: &Invocation) -> Result<(), CliError> {
    let config = read_merged_config_json()?;
    let plugins = configured_plugins(&config);
    if plugins.is_empty() {
        println!("No plugins found");
        return Ok(());
    }
    let _ = invocation;
    for plugin in plugins {
        println!("{plugin}");
    }
    Ok(())
}

fn plugin_add(invocation: &Invocation) -> Result<(), CliError> {
    let package = invocation
        .argument("package")
        .ok_or_else(|| CliError("Missing plugin package".into()))?;
    let target = config_write_target(true)?;
    let mut root = read_config_json(&target)?;
    let object = root
        .as_object_mut()
        .ok_or_else(|| CliError("Configuration root must be an object".into()))?;
    let plugins = object
        .entry("plugin")
        .or_insert_with(|| Value::Array(Vec::new()));
    let array = plugins
        .as_array_mut()
        .ok_or_else(|| CliError("plugin must be an array".into()))?;
    if !array.iter().any(|value| value.as_str() == Some(package)) {
        array.push(Value::String(package.to_string()));
    }
    write_config_json(&target, &root)?;
    println!("Added plugin {package} to {}", target.display());
    Ok(())
}

fn plugin_check(_invocation: &Invocation) -> Result<(), CliError> {
    println!("Plugin update checks are not available in this build.");
    Ok(())
}

fn plugin_update(_invocation: &Invocation) -> Result<(), CliError> {
    println!("Plugin updates are not available in this build.");
    Ok(())
}

fn plugin_remove(invocation: &Invocation) -> Result<(), CliError> {
    let package = invocation
        .argument("package")
        .ok_or_else(|| CliError("Missing plugin package".into()))?;
    let target = config_write_target(true)?;
    let mut root = read_config_json(&target)?;
    let mut removed = false;
    if let Some(array) = root.get_mut("plugin").and_then(Value::as_array_mut) {
        let before = array.len();
        array.retain(|value| value.as_str() != Some(package));
        removed = array.len() != before;
    }
    if !removed {
        return Err(CliError(format!("Plugin not found: {package}")));
    }
    write_config_json(&target, &root)?;
    println!("Removed plugin {package}");
    Ok(())
}

fn configured_plugins(config: &Value) -> Vec<String> {
    config
        .get("plugin")
        .or_else(|| config.get("plugins"))
        .and_then(Value::as_array)
        .map(|plugins| {
            plugins
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn stats(invocation: &Invocation) -> Result<(), CliError> {
    let db = open_db()?;
    let stats = db.stats()?;
    let total_tokens = stats.input_tokens.saturating_add(stats.output_tokens);
    let tool_total = stats.tool_successes.saturating_add(stats.tool_failures);
    let success = if tool_total == 0 {
        0.0
    } else {
        stats.tool_successes as f64 / tool_total as f64 * 100.0
    };
    if invocation.flag_present("json") {
        return print_json(&json!({
            "sessions": stats.sessions,
            "prompts": stats.prompts,
            "steps": stats.steps,
            "toolSuccess": stats.tool_successes,
            "toolFailures": stats.tool_failures,
            "inputTokens": stats.input_tokens,
            "outputTokens": stats.output_tokens,
            "totalTokens": total_tokens,
            "activeDays": stats.active_days,
        }));
    }
    if stats.sessions == 0 && stats.prompts == 0 {
        println!("No usage yet");
        return Ok(());
    }
    println!("clawcode stats");
    println!();
    println!(
        "{} session{}",
        stats.sessions,
        if stats.sessions == 1 { "" } else { "s" }
    );
    println!(
        "{} prompts · {} steps · {} tokens",
        stats.prompts,
        stats.steps,
        format_tokens(total_tokens)
    );
    println!(
        "{:.1}% tool success · {} active day{}",
        success,
        stats.active_days,
        if stats.active_days == 1 { "" } else { "s" }
    );
    Ok(())
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn run_prompt(invocation: &Invocation) -> Result<(), CliError> {
    let mut prompt = invocation.argument_values("message").join(" ");
    if prompt.trim().is_empty() {
        let mut stdin = String::new();
        io::stdin().read_to_string(&mut stdin)?;
        prompt = stdin.trim().to_string();
    }
    if prompt.is_empty() {
        return Err(CliError("No message provided".into()));
    }
    let config = load_config()?;
    let (provider, model) = select_model(&config, invocation.flag("model"))?;
    if provider.is_empty() || model.is_empty() {
        return Err(CliError(
            "No model configured. Add a provider and model to your configuration.".into(),
        ));
    }
    let agent = invocation.flag("agent").unwrap_or("build");
    let data = data_dir();
    fs::create_dir_all(&data)?;
    let db_path = data.join("clawcode.db");
    let runtime_db = Db::open(&db_path)?;
    let writer_db = Db::open(&db_path)?;
    let writer = crate::persistence::WriterHandle::spawn(writer_db);
    let bus = EventBus::new();
    let mcp_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mcp = crate::mcp::McpRegistry::new(&config.mcp, &mcp_cwd);
    let router = crate::adapters::ConfiguredRouter::new(config);
    let client =
        RuntimeClient::spawn_with_mcp(runtime_db, writer, Box::new(router), bus.clone(), mcp);
    let title = invocation
        .flag("title")
        .map(str::to_string)
        .unwrap_or_else(|| prompt.chars().take(80).collect());
    let session = client.create_session(1, &title)?;
    let (_, receiver) = bus.subscribe(Some(session));
    client.start_generation(session, agent, &provider, &model, &prompt)?;

    let json_output = invocation.flag("format") == Some("json");
    let mut text = String::new();
    let mut saw_delta = false;
    let mut error = None;
    loop {
        match receiver.recv_timeout(Duration::from_secs(600)) {
            Ok(event) => {
                if let Some(failure) =
                    handle_run_event(&event, &mut text, &mut saw_delta, json_output)
                {
                    error = Some(failure);
                    break;
                }
                if event.kind == "generation_finished" {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                error = Some("generation timed out".into());
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                error = Some("runtime stopped before the generation completed".into());
                break;
            }
        }
    }
    let _ = client.cancel_generation(session);
    let _ = client.shutdown();

    if let Some(error) = error {
        return Err(CliError(error));
    }
    if json_output {
        return print_json(&json!({
            "sessionID": session_id(session),
            "text": text,
            "provider": provider,
            "model": model,
        }));
    }
    if !saw_delta {
        println!("{text}");
    }
    Ok(())
}

fn handle_run_event(
    event: &RuntimeEvent,
    text: &mut String,
    saw_delta: &mut bool,
    json_output: bool,
) -> Option<String> {
    let payload: Value = serde_json::from_str(&event.payload_json).unwrap_or(Value::Null);
    match event.kind.as_str() {
        "text_delta" => {
            if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
                text.push_str(delta);
                *saw_delta = true;
                if !json_output {
                    print!("{delta}");
                    let _ = io::Write::flush(&mut io::stdout());
                }
            }
        }
        "assistant_message" if text.is_empty() => {
            if let Some(content) = payload.get("content").and_then(Value::as_str) {
                text.push_str(content);
            }
        }
        "error" => {
            let message = payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("stream error");
            return Some(message.to_string());
        }
        "generation_finished" => {
            let status = payload
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            if status != "completed" {
                return Some(format!("generation {status}"));
            }
        }
        _ => {}
    }
    None
}

pub(crate) fn select_model(
    config: &Config,
    selected: Option<&str>,
) -> Result<(String, String), CliError> {
    if let Some(selected) = selected {
        let body = selected.split('#').next().unwrap_or(selected);
        if let Some((provider, model)) = body.split_once('/') {
            return Ok((provider.to_string(), model.to_string()));
        }
        let (provider, _) = config.initial_provider_and_model();
        if provider.is_empty() {
            return Err(CliError(
                "Model must use the provider/model form when no provider is configured".into(),
            ));
        }
        return Ok((provider, body.to_string()));
    }
    let (provider, model) = config.initial_provider_and_model();
    Ok((provider, model))
}

fn handle_upgrade(invocation: &Invocation) -> Result<(), CliError> {
    let _ = invocation;
    println!(
        "clawcode v{} is installed. This build does not include an automatic updater.",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

fn handle_uninstall(invocation: &Invocation) -> Result<(), CliError> {
    let exe = std::env::current_exe()?;
    if invocation.flag_present("dry-run") {
        println!("Would remove {}", exe.display());
        return Ok(());
    }
    Err(CliError(format!(
        "Refusing to remove the running binary automatically. Remove {} manually.",
        exe.display()
    )))
}

fn handle_acp(invocation: &Invocation) -> Result<(), CliError> {
    if invocation.flag_present("login") {
        return auth_login(invocation);
    }
    crate::cli::acp::run(invocation)
}

fn handle_api(invocation: &Invocation) -> Result<(), CliError> {
    let request = invocation.argument_values("request");
    let (method, path) = match request {
        [method, path] if is_http_method(method) => (method.to_uppercase(), path.clone()),
        [path] => ("GET".to_string(), path.clone()),
        _ => {
            return Err(CliError(
                "Usage: clawcode api [METHOD] <path> [--data BODY]".into(),
            ));
        }
    };
    let base = invocation
        .flag("server")
        .map(str::to_string)
        .or_else(|| std::env::var("CLAWCODE_SERVER").ok())
        .unwrap_or_else(|| "http://127.0.0.1:4096".into());
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let mut response = ureq::request(&method, &url);
    for header in invocation.flag_values("header") {
        if let Some((name, value)) = header.split_once(':') {
            response = response.set(name.trim(), value.trim());
        } else {
            return Err(CliError(format!(
                "Invalid header (expected name:value): {header}"
            )));
        }
    }
    let result = match invocation.flag("data") {
        Some(data) => response.send_string(data),
        None => response.call(),
    };
    match result {
        Ok(response) => {
            println!("{}", response.status());
            let body = response
                .into_string()
                .map_err(|error| CliError(error.to_string()))?;
            print!("{body}");
            if !body.ends_with('\n') {
                println!();
            }
            Ok(())
        }
        Err(ureq::Error::Status(code, response)) => {
            println!("{code}");
            let body = response.into_string().unwrap_or_default();
            Err(CliError(body))
        }
        Err(error) => Err(CliError(error.to_string())),
    }
}

fn is_http_method(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    )
}

fn service_status() -> Result<(), CliError> {
    let file = service_file();
    if !file.is_file() {
        println!("stopped");
        return Ok(());
    }
    let value = read_config_json(&file)?;
    let pid = value.get("pid").and_then(Value::as_i64).unwrap_or(0);
    if pid > 0 {
        println!("running (pid {pid})");
    } else {
        println!("stopped");
    }
    Ok(())
}

fn service_file() -> PathBuf {
    data_dir().join("service.json")
}

fn service_get(invocation: &Invocation) -> Result<(), CliError> {
    let key = invocation
        .argument("key")
        .ok_or_else(|| CliError("Missing service key".into()))?;
    let value = read_config_json(&service_file()).unwrap_or_else(|_| json!({}));
    let found = nested_get(&value, key);
    match found {
        Some(value) => {
            println!("{}", value_to_display(value));
            Ok(())
        }
        None => Err(CliError(format!("Service setting not found: {key}"))),
    }
}

fn service_set(invocation: &Invocation) -> Result<(), CliError> {
    let key = invocation
        .argument("key")
        .ok_or_else(|| CliError("Missing service key".into()))?;
    let value = invocation
        .argument("value")
        .ok_or_else(|| CliError("Missing service value".into()))?;
    let mut root = read_config_json(&service_file()).unwrap_or_else(|_| json!({}));
    nested_set(&mut root, key, Value::String(value.to_string()))?;
    write_config_json(&service_file(), &root)?;
    println!("{key}={value}");
    Ok(())
}

fn service_unset(invocation: &Invocation) -> Result<(), CliError> {
    let key = invocation
        .argument("key")
        .ok_or_else(|| CliError("Missing service key".into()))?;
    let mut root = read_config_json(&service_file()).unwrap_or_else(|_| json!({}));
    if nested_remove(&mut root, key) {
        write_config_json(&service_file(), &root)?;
        println!("Unset {key}");
        Ok(())
    } else {
        Err(CliError(format!("Service setting not found: {key}")))
    }
}

fn nested_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn nested_set(root: &mut Value, key: &str, value: Value) -> Result<(), CliError> {
    let parts: Vec<&str> = key.split('.').collect();
    let Some((last, parents)) = parts.split_last() else {
        return Err(CliError("Service key cannot be empty".into()));
    };
    let mut current = root;
    for part in parents {
        if !current.is_object() {
            *current = json!({});
        }
        current = current
            .as_object_mut()
            .expect("object just inserted")
            .entry((*part).to_string())
            .or_insert_with(|| json!({}));
    }
    if !current.is_object() {
        *current = json!({});
    }
    current
        .as_object_mut()
        .expect("object just inserted")
        .insert((*last).to_string(), value);
    Ok(())
}

fn nested_remove(root: &mut Value, key: &str) -> bool {
    let parts: Vec<&str> = key.split('.').collect();
    let Some((last, parents)) = parts.split_last() else {
        return false;
    };
    let mut current = root;
    for part in parents {
        let Some(next) = current.get_mut(*part) else {
            return false;
        };
        current = next;
    }
    current
        .as_object_mut()
        .and_then(|object| object.remove(*last))
        .is_some()
}

fn value_to_display(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn reload() -> Result<(), CliError> {
    let config = load_config()?;
    let (provider, model) = config.initial_provider_and_model();
    if provider.is_empty() {
        println!("Configuration reloaded");
    } else {
        println!("Configuration reloaded: {provider}/{model}");
    }
    Ok(())
}

fn pair(invocation: &Invocation) -> Result<(), CliError> {
    if invocation.flag_present("remote") && !invocation.flag_present("yes") {
        return Err(CliError(
            "--remote exposes the server to the internet; pass --yes to confirm".into(),
        ));
    }
    Err(CliError(
        "The background server is not running. Start it with `clawcode serve` first.".into(),
    ))
}

fn read_merged_config_json() -> Result<Value, CliError> {
    let mut merged = json!({});
    for path in first_existing(global_config_candidates())
        .into_iter()
        .chain(first_existing(project_config_candidates()))
    {
        if !path.is_file() {
            continue;
        }
        let value = read_config_json(&path)?;
        merge_json(&mut merged, value);
    }
    Ok(merged)
}

fn merge_json(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                if let Some(existing) = target.get_mut(&key) {
                    merge_json(existing, value);
                } else {
                    target.insert(key, value);
                }
            }
        }
        (target, source) => *target = source,
    }
}

fn read_config_json(path: &Path) -> Result<Value, CliError> {
    let text = fs::read_to_string(path)?;
    match serde_json::from_str(&text) {
        Ok(value) => Ok(value),
        Err(_) => {
            let value = jsonc::parse(&text).map_err(|(line, column, message)| {
                CliError(format!("{}:{line}:{column}: {message}", path.display()))
            })?;
            Ok(jsonc_to_json(&value))
        }
    }
}

fn write_config_json(path: &Path, value: &Value) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| CliError(error.to_string()))?;
    fs::write(path, format!("{text}\n"))?;
    Ok(())
}

fn config_write_target(global: bool) -> Result<PathBuf, CliError> {
    if global {
        return Ok(first_existing(global_config_candidates())
            .unwrap_or_else(|| config_home().join("clawcode.jsonc")));
    }
    Ok(
        first_existing(project_config_candidates()).unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("clawcode.jsonc")
        }),
    )
}

fn parse_pairs(values: &[String]) -> Result<Map<String, Value>, CliError> {
    let mut map = Map::new();
    for value in values {
        let (key, value) = value
            .split_once('=')
            .ok_or_else(|| CliError(format!("Expected name=value, got {value}")))?;
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
    Ok(map)
}

fn read_source(source: &str) -> Result<String, CliError> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let response = ureq::get(source)
            .call()
            .map_err(|error| CliError(error.to_string()))?;
        return response
            .into_string()
            .map_err(|error| CliError(error.to_string()));
    }
    fs::read_to_string(source).map_err(CliError::from)
}
