//! Command-line surface shared by help rendering and argument parsing.
//!
//! OpenCode's CLI is built on `effect/cli`; its help output has a stable
//! section order and alignment. This module keeps the same shape so scripts
//! and documentation that target OpenCode keep working.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParamKind {
    Argument,
    Flag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueKind {
    Bool,
    String,
    Integer,
    Literal(&'static [&'static str]),
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: ParamKind,
    pub aliases: Vec<&'static str>,
    pub value: ValueKind,
    pub required: bool,
    pub variadic: bool,
    pub hidden: bool,
    pub repeatable: bool,
}

impl Param {
    pub const fn argument(name: &'static str, description: &'static str, required: bool) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Argument,
            aliases: Vec::new(),
            value: ValueKind::String,
            required,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub const fn variadic_argument(
        name: &'static str,
        description: &'static str,
        required: bool,
    ) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Argument,
            aliases: Vec::new(),
            value: ValueKind::String,
            required,
            variadic: true,
            hidden: false,
            repeatable: false,
        }
    }

    pub const fn literal_argument(
        name: &'static str,
        values: &'static [&'static str],
        description: &'static str,
        required: bool,
    ) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Argument,
            aliases: Vec::new(),
            value: ValueKind::Literal(values),
            required,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub const fn flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: Vec::new(),
            value: ValueKind::Bool,
            required: false,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub fn flag_with_alias(
        name: &'static str,
        alias: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: vec![alias],
            value: ValueKind::Bool,
            required: false,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub const fn string_flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: Vec::new(),
            value: ValueKind::String,
            required: false,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub fn string_flag_alias(
        name: &'static str,
        alias: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: vec![alias],
            value: ValueKind::String,
            required: false,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub fn integer_flag_alias(
        name: &'static str,
        alias: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: vec![alias],
            value: ValueKind::Integer,
            required: false,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub fn enum_flag_alias(
        name: &'static str,
        alias: &'static str,
        values: &'static [&'static str],
        description: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: vec![alias],
            value: ValueKind::Literal(values),
            required: false,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub const fn integer_flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: Vec::new(),
            value: ValueKind::Integer,
            required: false,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub const fn enum_flag(
        name: &'static str,
        values: &'static [&'static str],
        description: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: Vec::new(),
            value: ValueKind::Literal(values),
            required: false,
            variadic: false,
            hidden: false,
            repeatable: false,
        }
    }

    pub const fn repeated_string_flag(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: ParamKind::Flag,
            aliases: Vec::new(),
            value: ValueKind::String,
            required: false,
            variadic: false,
            hidden: false,
            repeatable: true,
        }
    }

    pub const fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub name: &'static str,
    pub description: &'static str,
    pub aliases: Vec<&'static str>,
    pub params: Vec<Param>,
    pub commands: Vec<Spec>,
}

impl Spec {
    pub fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            aliases: Vec::new(),
            params: Vec::new(),
            commands: Vec::new(),
        }
    }

    pub fn alias(mut self, alias: &'static str) -> Self {
        self.aliases.push(alias);
        self
    }

    pub fn params(mut self, params: impl IntoIterator<Item = Param>) -> Self {
        self.params.extend(params);
        self
    }

    pub fn commands(mut self, commands: impl IntoIterator<Item = Spec>) -> Self {
        self.commands.extend(commands);
        self
    }
}

#[derive(Clone, Debug)]
pub struct Invocation {
    pub path: Vec<String>,
    pub flags: BTreeMap<&'static str, Vec<String>>,
    pub arguments: BTreeMap<&'static str, Vec<String>>,
}

impl Invocation {
    pub fn subcommand(&self) -> &[String] {
        self.path.get(1..).unwrap_or_default()
    }

    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .get(name)
            .and_then(|values| values.last())
            .map(String::as_str)
    }

    pub fn flag_values(&self, name: &str) -> &[String] {
        self.flags.get(name).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn flag_present(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    pub fn argument(&self, name: &str) -> Option<&str> {
        self.arguments
            .get(name)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    pub fn argument_values(&self, name: &str) -> &[String] {
        self.arguments
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub enum ParseOutcome {
    Invocation(Invocation),
    Help(Box<Spec>, Vec<String>),
    Version,
    Completions(String),
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

pub const LOG_LEVELS: &[&str] = &[
    "all", "trace", "debug", "info", "warn", "warning", "error", "fatal", "none",
];

pub fn global_params() -> Vec<Param> {
    vec![
        Param::flag_with_alias("help", "h", "Show help information"),
        Param::flag_with_alias("version", "v", "Show version information"),
        Param::flag("wizard", "Start wizard mode for a command"),
        Param::enum_flag(
            "completions",
            &["bash", "zsh", "fish", "sh"],
            "Print shell completion script (choices: bash, zsh, fish, sh)",
        ),
        Param::enum_flag(
            "log-level",
            LOG_LEVELS,
            "Sets the minimum log level (choices: all, trace, debug, info, warn, warning, error, fatal, none)",
        ),
        Param::flag(
            "print-logs",
            "Print logs to stderr (server logs require --standalone)",
        ),
    ]
}

pub fn root_spec() -> Spec {
    Spec::new("clawcode", "Clawcode command line interface")
        .params(
            server_params()
                .into_iter()
                .chain(permission_params())
                .chain([
                    Param::argument("directory", "Directory to start Clawcode in", false),
                    Param::flag_with_alias("continue", "c", "Continue the last session"),
                    Param::string_flag_alias(
                        "session",
                        "s",
                        "Session ID to continue, or to create if it does not exist",
                    ),
                    Param::string_flag("prompt", "Prompt to use"),
                ]),
        )
        .commands([
            Spec::new(
                "upgrade",
                "Upgrade Clawcode to the latest or a specific version",
            )
            .alias("update")
            .params([
                Param::argument(
                    "target",
                    "Version to upgrade to (with or without a leading v)",
                    false,
                ),
                Param::enum_flag_alias(
                    "method",
                    "m",
                    &["curl", "npm", "pnpm", "bun", "yarn", "vp", "brew", "winget"],
                    "Installation method to use",
                ),
            ]),
            Spec::new(
                "uninstall",
                "Uninstall Clawcode, keeping session data, configuration, and state",
            )
            .params([
                Param::flag(
                    "dry-run",
                    "Show what would be removed without removing",
                ),
                Param::flag_with_alias("force", "f", "Skip confirmation prompts"),
            ]),
            Spec::new("acp", "Start an Agent Client Protocol server").params([Param::flag(
                "login",
                "Run auth login instead of starting the server",
            )]),
            Spec::new("api", "Make a request to the running server").params(
                server_params()
                    .into_iter()
                    .chain([
                        Param::variadic_argument(
                            "request",
                            "OpenAPI operation ID, or an HTTP method followed by a path",
                            true,
                        ),
                        Param::string_flag_alias("data", "d", "Request body"),
                        Param::string_flag_alias(
                            "header",
                            "H",
                            "Request header in name:value form",
                        )
                        .repeatable_if(),
                        Param::string_flag(
                            "param",
                            "OpenAPI path or query parameter",
                        ),
                    ]),
            ),
            Spec::new("debug", "Debugging and troubleshooting tools").commands([
                Spec::new("agents", "List all agents"),
                Spec::new("config", "List configuration sources"),
                Spec::new("paths", "Show global paths (data, config, cache, state)").params([
                    Param::literal_argument(
                        "name",
                        &[
                            "db", "home", "data", "config", "cache", "state", "tmp", "bin", "log",
                            "repos",
                        ],
                        "Print only one path: db, home, data, config, cache, state, tmp, bin, log, repos",
                        false,
                    ),
                ]),
            ]),
            Spec::new("auth", "manage integrations and credentials").commands([
                Spec::new("list", "List integrations and credentials").params(
                    server_params().into_iter().chain([Param::enum_flag(
                        "format",
                        &["default", "json"],
                        "Output format",
                    )]),
                ),
                Spec::new("login", "Connect an integration").params(
                    server_params()
                        .into_iter()
                        .chain([
                            Param::argument(
                                "target",
                                "Integration ID, name, or well-known provider URL",
                                false,
                            ),
                            Param::string_flag("method", "Authentication method ID"),
                            Param::string_flag(
                                "answer",
                                "Provider form answer (key=value; repeat for multiple fields)",
                            )
                            .repeatable_if(),
                        ]),
                ),
                Spec::new("logout", "Log out of a saved account").params(
                    server_params()
                        .into_iter()
                        .chain([
                            Param::argument("target", "Integration ID or name", false),
                            Param::argument(
                                "credential",
                                "Credential ID or label (opens an account picker when omitted)",
                                false,
                            ),
                        ]),
                ),
                Spec::new(
                    "export",
                    "Print stored credentials, including secrets, as JSON",
                )
                .params(server_params().into_iter().chain([Param::argument(
                    "target",
                    "Integration ID or name (exports every integration when omitted)",
                    false,
                )])),
                Spec::new(
                    "import",
                    "Import credentials exported by auth export",
                )
                .params(server_params().into_iter().chain([Param::argument(
                    "file",
                    "JSON file to import (reads stdin when omitted)",
                    false,
                )])),
                Spec::new("switch", "Switch the active account for an integration").params(
                    server_params()
                        .into_iter()
                        .chain([
                            Param::argument("target", "Integration ID or name", false),
                            Param::argument(
                                "credential",
                                "Credential ID or label (opens an account picker when omitted)",
                                false,
                            ),
                        ]),
                ),
            ]),
            Spec::new("mcp", "Manage MCP (Model Context Protocol) servers").commands([
                Spec::new("list", "List configured MCP servers and their status"),
                Spec::new("add", "Add an MCP server to your configuration").params([
                    Param::argument("name", "Name of the MCP server", true),
                    Param::variadic_argument(
                        "command",
                        "Command and arguments for a local server, passed after --",
                        false,
                    ),
                    Param::string_flag("url", "URL for a remote MCP server"),
                    Param::string_flag("header", "HTTP header for a remote server, as name=value"),
                    Param::string_flag(
                        "env",
                        "Environment variable for a local server, as name=value",
                    ),
                    Param::flag(
                        "global",
                        "Write to the global config instead of the project config",
                    ),
                ]),
                Spec::new(
                    "auth",
                    "Authenticate with an OAuth-capable remote MCP server",
                )
                .params([Param::argument("name", "Name of the MCP server", false)]),
                Spec::new("logout", "Remove stored OAuth credentials for an MCP server").params(
                    [Param::argument("name", "Name of the MCP server", true)],
                ),
            ]),
            Spec::new("plugin", "Manage plugins").commands([
                Spec::new("list", "List plugins").params([Param::flag(
                    "builtin",
                    "Include built-in server plugins",
                )]),
                Spec::new(
                    "add",
                    "Install a plugin and add it to the global configuration",
                )
                .params([Param::argument(
                    "package",
                    "npm registry or Git package specifier",
                    true,
                )]),
                Spec::new("check", "Check package plugins for updates").params([
                    Param::argument("target", "Configured package target", false),
                ]),
                Spec::new("update", "Update package plugins").params([Param::argument(
                    "target",
                    "Configured package target; omit to update all outdated plugins",
                    false,
                )]),
                Spec::new("remove", "Remove a plugin from global configuration").params([
                    Param::argument("package", "configured package specifier", true),
                ]),
            ]),
            Spec::new("models", "List all available models").params(server_params()),
            Spec::new("stats", "Show shareable usage statistics").params(
                server_params()
                    .into_iter()
                    .chain([
                        Param::integer_flag("days", "Show the last N days; 0 means today"),
                        Param::integer_flag("year", "Show a calendar year"),
                        Param::flag("all", "Show lifetime statistics"),
                        Param::string_flag(
                            "project",
                            "Filter by project ID, or use \".\" for the current project",
                        ),
                        Param::flag("models", "Show model usage"),
                        Param::flag("tools", "Show tool reliability"),
                        Param::flag("cost", "Show cost and token details"),
                        Param::flag("full", "Show every detailed section"),
                        Param::integer_flag("limit", "Number of rows in detailed sections"),
                        Param::flag("json", "Output statistics as JSON"),
                    ]),
            ),
            Spec::new("mini", "Start the minimal interactive interface").params(
                server_params()
                    .into_iter()
                    .chain(interactive_params())
                    .chain([
                        Param::string_flag("prompt", "Prompt to use"),
                        Param::flag("demo", "Show demo data").hidden(),
                    ]),
            ),
            Spec::new("run", "Run Clawcode with a message").params(
                server_params()
                    .into_iter()
                    .chain(interactive_params())
                    .chain([
                        Param::variadic_argument("message", "Message to send", false),
                        Param::enum_flag("format", &["default", "json"], "Output format"),
                        Param::string_flag_alias(
                            "file",
                            "f",
                            "File to attach to the message",
                        )
                        .repeatable_if(),
                        Param::string_flag("title", "Session title"),
                        Param::flag("thinking", "Show thinking blocks"),
                    ])
                    .chain(permission_params()),
            ),
            Spec::new("session", "Manage sessions").commands([
                Spec::new(
                    "list",
                    "List top-level sessions in the current project, newest first",
                )
                .params(
                    server_params()
                        .into_iter()
                        .chain([
                            Param::integer_flag_alias(
                                "max-count",
                                "n",
                                "Limit to N most recent sessions (default: 100)",
                            ),
                            Param::enum_flag(
                                "format",
                                &["table", "json"],
                                "Output format",
                            ),
                        ]),
                ),
                Spec::new("delete", "Delete a session and its child sessions").params(
                    server_params().into_iter().chain([Param::argument(
                        "sessionID",
                        "Session ID to delete",
                        true,
                    )]),
                ),
                Spec::new("export", "Export session data as JSON").params(
                    server_params()
                        .into_iter()
                        .chain([
                            Param::argument("session", "Session ID to export", false),
                            Param::flag("sanitize", "Redact sensitive transcript and file data"),
                        ]),
                ),
                Spec::new("import", "Import session data from a JSON file or URL").params(
                    server_params()
                        .into_iter()
                        .chain([
                            Param::argument("file", "JSON file or URL to import", true),
                            Param::string_flag(
                                "directory",
                                "Directory in which to import the session",
                            ),
                        ]),
                ),
            ]),
            Spec::new("service", "Manage the background server").commands([
                Spec::new("start", "Start the background server"),
                Spec::new("restart", "Restart the background server"),
                Spec::new("status", "Show background server status"),
                Spec::new("stop", "Stop the background server"),
                Spec::new("get", "Get service configuration").params([
                    Param::argument("key", "Service setting or env", false),
                    Param::argument("name", "Environment variable name", false),
                ]),
                Spec::new("set", "Set service configuration").params([
                    Param::argument("key", "Service setting or env", true),
                    Param::argument("value", "Setting value or environment variable name", true),
                    Param::argument("env-value", "Environment variable value", false),
                ]),
                Spec::new("unset", "Unset service configuration").params([
                    Param::argument("key", "Service setting or env", true),
                    Param::argument("name", "Environment variable name", false),
                ]),
            ]),
            Spec::new("reload", "Reload configuration").params(server_params()),
            Spec::new("pair", "Print one-time links to connect a browser or app").params([
                Param::string_flag(
                    "url",
                    "Use an external HTTP(S) server URL in pairing links",
                ),
                Param::flag(
                    "remote",
                    "Pair through the OpenTunnel remote address, enabling remote access if needed (exposes the server to the internet)",
                ),
                Param::flag_with_alias(
                    "yes",
                    "y",
                    "Accept exposing the server to the internet when --remote enables remote access for the first time (required without an interactive terminal)",
                ),
            ]),
            Spec::new("serve", "Start the v2 API and web server").params([
                Param::string_flag("hostname", "Hostname to bind"),
                Param::integer_flag("port", "Port to bind"),
                Param::string_flag(
                    "cors",
                    "Additional allowed CORS origin (repeat for multiple origins)",
                )
                .repeatable_if(),
                Param::flag("service", "Run as the background service"),
                Param::flag("stdio", "Use stdio transport"),
            ]),
        ])
}

fn permission_params() -> Vec<Param> {
    vec![
        Param::flag(
            "auto",
            "Auto-approve permissions that are not explicitly denied",
        ),
        Param::flag("yolo", "Skip permission checks").hidden(),
        Param::flag("dangerously-skip-permissions", "Skip permission checks").hidden(),
    ]
}

fn interactive_params() -> Vec<Param> {
    vec![
        Param::flag_with_alias("continue", "c", "Continue the last session"),
        Param::string_flag_alias(
            "session",
            "s",
            "Session ID to continue, or to create if it does not exist",
        ),
        Param::flag("fork", "Fork the session before continuing"),
        Param::string_flag_alias("model", "m", "Model to use in the format provider/model"),
        Param::string_flag("agent", "Agent to use"),
    ]
}

fn server_params() -> Vec<Param> {
    vec![
        Param::flag(
            "standalone",
            "Run with a private server instead of the background service",
        ),
        Param::string_flag(
            "server",
            "Connect to a server URL instead of the background service",
        ),
    ]
}

trait RepeatableIf {
    fn repeatable_if(self) -> Self;
}

impl RepeatableIf for Param {
    fn repeatable_if(mut self) -> Self {
        self.repeatable = true;
        self
    }
}

/// Parse `args` against `root`. Help and version short-circuit like OpenCode.
pub fn parse(root: &Spec, args: &[String]) -> Result<ParseOutcome, ParseError> {
    let globals = global_params();
    let mut current = root.clone();
    let mut path = vec![root.name.to_string()];
    let mut positionals: Vec<String> = Vec::new();
    let mut flags: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut index = 0;
    let mut only_positionals = false;

    while index < args.len() {
        let token = &args[index];
        if only_positionals {
            positionals.push(token.clone());
            index += 1;
            continue;
        }
        if token == "--" {
            only_positionals = true;
            index += 1;
            continue;
        }
        if token.starts_with("--") || (token.starts_with('-') && token.len() > 1) {
            let (raw_name, inline_value) = match token.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (token.as_str(), None),
            };
            let name = raw_name.trim_start_matches('-');
            let param = current
                .params
                .iter()
                .chain(globals.iter())
                .find(|param| {
                    param.kind == ParamKind::Flag
                        && (param.name == name || param.aliases.contains(&name))
                })
                .cloned()
                .ok_or_else(|| {
                    ParseError(format!(
                        "Unrecognized flag: {} in command {}",
                        raw_name,
                        path.join(" ")
                    ))
                })?;

            if param.name == "help" {
                return Ok(ParseOutcome::Help(Box::new(current), path));
            }
            if param.name == "version" {
                return Ok(ParseOutcome::Version);
            }

            let value = match param.value {
                ValueKind::Bool => {
                    if inline_value.is_some() {
                        return Err(ParseError(format!(
                            "Flag {} does not take a value",
                            raw_name
                        )));
                    }
                    "true".to_string()
                }
                ValueKind::String => match inline_value {
                    Some(value) => value,
                    None => {
                        index += 1;
                        args.get(index).cloned().ok_or_else(|| {
                            ParseError(format!("Expected a value for flag {}", raw_name))
                        })?
                    }
                },
                ValueKind::Integer => {
                    let value = match inline_value {
                        Some(value) => value,
                        None => {
                            index += 1;
                            args.get(index).cloned().ok_or_else(|| {
                                ParseError(format!("Expected a value for flag {}", raw_name))
                            })?
                        }
                    };
                    if value.parse::<i64>().is_err() {
                        return Err(ParseError(format!(
                            "Expected an integer for flag {}, got {value}",
                            raw_name
                        )));
                    }
                    value
                }
                ValueKind::Literal(allowed) => {
                    let value = match inline_value {
                        Some(value) => value,
                        None => {
                            index += 1;
                            args.get(index).cloned().ok_or_else(|| {
                                ParseError(format!("Expected a value for flag {}", raw_name))
                            })?
                        }
                    };
                    if !allowed.contains(&value.as_str()) {
                        return Err(ParseError(format!(
                            "Invalid value for flag {}: {value}",
                            raw_name
                        )));
                    }
                    value
                }
            };

            if param.name == "completions" {
                return Ok(ParseOutcome::Completions(value));
            }

            if !param.repeatable && flags.contains_key(param.name) {
                return Err(ParseError(format!(
                    "Flag {} may only be used once",
                    raw_name
                )));
            }
            flags.entry(param.name).or_default().push(value);
            index += 1;
            continue;
        }

        if let Some(command) = current
            .commands
            .iter()
            .find(|command| command.name == token || command.aliases.contains(&token.as_str()))
            .cloned()
        {
            if !positionals.is_empty() && current.name != root.name {
                return Err(ParseError(format!(
                    "Unexpected argument `{}` before subcommand `{}`",
                    positionals.join(" "),
                    command.name
                )));
            }
            if current.name == root.name && !positionals.is_empty() {
                return Err(ParseError(format!(
                    "Unexpected argument `{}` before subcommand `{}`",
                    positionals.join(" "),
                    command.name
                )));
            }
            path.push(command.name.to_string());
            current = command;
            index += 1;
            continue;
        }

        // An unknown first positional is a directory in OpenCode. Deeper
        // levels reject it so typos do not silently launch the wrong command.
        if current.name == root.name
            && current
                .commands
                .iter()
                .all(|command| command.name != *token)
            && positionals.is_empty()
        {
            positionals.push(token.clone());
            index += 1;
            continue;
        }
        if current.commands.is_empty() {
            positionals.push(token.clone());
            index += 1;
            continue;
        }

        return Err(ParseError(format!(
            "Unrecognized subcommand: {token} in command {}",
            path.join(" ")
        )));
    }

    if !current.commands.is_empty() {
        let root_directory = current.name == root.name && positionals.len() == 1;
        if !root_directory {
            return Ok(ParseOutcome::Help(Box::new(current), path));
        }
    }

    let mut arguments: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut remaining = positionals.into_iter();
    for param in current
        .params
        .iter()
        .filter(|param| param.kind == ParamKind::Argument)
    {
        if param.variadic {
            let values: Vec<String> = remaining.by_ref().collect();
            if param.required && values.is_empty() {
                return Err(ParseError(format!(
                    "Missing required argument: {}",
                    param.name
                )));
            }
            arguments.insert(param.name, values);
            continue;
        }
        match remaining.next() {
            Some(value) => {
                if let ValueKind::Literal(allowed) = param.value
                    && !allowed.contains(&value.as_str())
                {
                    return Err(ParseError(format!(
                        "Invalid value for argument {}: {value}",
                        param.name
                    )));
                }
                arguments.insert(param.name, vec![value]);
            }
            None if param.required => {
                return Err(ParseError(format!(
                    "Missing required argument: {}",
                    param.name
                )));
            }
            None => {}
        }
    }
    if let Some(extra) = remaining.next() {
        return Err(ParseError(format!("Unexpected argument: {extra}")));
    }

    for param in current.params.iter().filter(|param| param.required) {
        if param.kind == ParamKind::Flag && !flags.contains_key(param.name) {
            return Err(ParseError(format!(
                "Missing required flag: --{}",
                param.name
            )));
        }
    }

    Ok(ParseOutcome::Invocation(Invocation {
        path,
        flags,
        arguments,
    }))
}

pub fn render_help(spec: &Spec, path: &[String]) -> String {
    let mut output = String::new();
    output.push_str("DESCRIPTION\n  ");
    output.push_str(spec.description);
    output.push_str("\n\nUSAGE\n  ");
    output.push_str(&usage(spec, path));
    output.push('\n');

    let arguments: Vec<&Param> = spec
        .params
        .iter()
        .filter(|param| param.kind == ParamKind::Argument && !param.hidden)
        .collect();
    if !arguments.is_empty() {
        output.push_str("\nARGUMENTS\n");
        render_rows(
            &mut output,
            arguments.iter().map(|param| {
                let label = argument_label(param);
                let mut description = param.description.to_string();
                if !param.required {
                    description.push_str(" (optional)");
                }
                (label, description)
            }),
        );
    }

    let flags: Vec<&Param> = spec
        .params
        .iter()
        .filter(|param| param.kind == ParamKind::Flag && !param.hidden)
        .collect();
    if !flags.is_empty() {
        output.push_str("\nFLAGS\n");
        render_rows(
            &mut output,
            flags.iter().map(|param| {
                let mut description = param.description.to_string();
                if let ValueKind::Literal(values) = param.value
                    && !description.contains("choices:")
                {
                    description.push_str(&format!(" (choices: {})", values.join(", ")));
                }
                (flag_label(param, false), description)
            }),
        );
    }

    output.push_str("\nGLOBAL FLAGS\n");
    render_rows(
        &mut output,
        global_params()
            .into_iter()
            .filter(|param| !param.hidden)
            .map(|param| (flag_label(&param, true), param.description.to_string())),
    );

    if !spec.commands.is_empty() {
        output.push_str("\nSUBCOMMANDS\n");
        render_rows(
            &mut output,
            spec.commands
                .iter()
                .filter(|command| !command.name.starts_with('_'))
                .map(|command| {
                    let mut label = command.name.to_string();
                    for alias in &command.aliases {
                        label.push_str(", ");
                        label.push_str(alias);
                    }
                    (label, command.description.to_string())
                }),
        );
    }

    output
}

fn usage(spec: &Spec, path: &[String]) -> String {
    let mut usage = path.join(" ");
    if !spec.commands.is_empty() {
        usage.push_str(" <subcommand>");
    }
    usage.push_str(" [flags]");
    for param in &spec.params {
        if param.kind != ParamKind::Argument {
            continue;
        }
        let name = if param.variadic {
            format!("{}...", param.name)
        } else {
            param.name.to_string()
        };
        if param.required {
            usage.push_str(&format!(" <{name}>"));
        } else {
            usage.push_str(&format!(" [<{name}>]"));
        }
    }
    usage
}

fn argument_label(param: &Param) -> String {
    let mut label = param.name.to_string();
    if param.variadic {
        label.push_str("...");
    }
    match param.value {
        ValueKind::Literal(_) => label.push_str(" choice"),
        _ => label.push_str(" string"),
    }
    label
}

fn flag_label(param: &Param, show_literal_values: bool) -> String {
    let mut label = format!("--{}", param.name);
    for alias in &param.aliases {
        label.push_str(", -");
        label.push_str(alias);
    }
    match param.value {
        ValueKind::Bool => {}
        ValueKind::String => label.push_str(" string"),
        ValueKind::Integer => label.push_str(" integer"),
        ValueKind::Literal(values) if show_literal_values => {
            label.push_str(" <");
            label.push_str(&values.join("|"));
            label.push('>');
        }
        ValueKind::Literal(_) => label.push_str(" choice"),
    }
    label
}

fn render_rows(output: &mut String, rows: impl IntoIterator<Item = (String, String)>) {
    let rows: Vec<(String, String)> = rows.into_iter().collect();
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    for (label, description) in rows {
        output.push_str("  ");
        output.push_str(&label);
        for _ in label.chars().count()..width + 4 {
            output.push(' ');
        }
        output.push_str(&description);
        output.push('\n');
    }
}

pub fn completion_script(shell: &str, root: &Spec) -> String {
    let commands: Vec<&str> = root.commands.iter().map(|command| command.name).collect();
    match shell {
        "bash" | "sh" => format!(
            "###-begin-clawcode-completions-###\n\
             _clawcode_completions() {{\n\
             \x20 local cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n\
             \x20 COMPREPLY=( $(compgen -W \"{} help version\" -- \"$cur\") )\n\
             }}\n\
             complete -F _clawcode_completions clawcode\n\
             ###-end-clawcode-completions-###\n",
            commands.join(" ")
        ),
        "zsh" => format!(
            "#compdef clawcode\n\
             _clawcode() {{\n\
             \x20 local -a commands\n\
             \x20 commands=({})\n\
             \x20 _describe 'command' commands\n\
             }}\n\
             compdef _clawcode clawcode\n",
            commands
                .iter()
                .map(|command| format!("'{command}'"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "fish" => format!("complete -c clawcode -f -a \"{}\"\n", commands.join(" ")),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_commands_and_flags() {
        let root = root_spec();
        let args = vec![
            "session".to_string(),
            "list".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--max-count".to_string(),
            "5".to_string(),
        ];
        let ParseOutcome::Invocation(invocation) = parse(&root, &args).unwrap() else {
            panic!("expected invocation");
        };
        assert_eq!(invocation.subcommand(), ["session", "list"]);
        assert_eq!(invocation.flag("max-count"), Some("5"));
        assert_eq!(invocation.flag("format"), Some("json"));
    }

    #[test]
    fn help_short_circuits_at_current_level() {
        let root = root_spec();
        let args = vec!["session".to_string(), "--help".to_string()];
        let ParseOutcome::Help(spec, path) = parse(&root, &args).unwrap() else {
            panic!("expected help");
        };
        assert_eq!(spec.name, "session");
        assert_eq!(path, ["clawcode", "session"]);
    }

    #[test]
    fn rejects_unknown_flags() {
        let root = root_spec();
        let args = vec!["models".to_string(), "--bogus".to_string()];
        let error = parse(&root, &args).unwrap_err();
        assert!(error.0.contains("Unrecognized flag: --bogus"));
    }

    #[test]
    fn renders_opencode_section_order() {
        let root = root_spec();
        let help = render_help(&root, &["clawcode".to_string()]);
        let description = help.find("DESCRIPTION").unwrap();
        let usage = help.find("USAGE").unwrap();
        let arguments = help.find("ARGUMENTS").unwrap();
        let flags = help.find("FLAGS").unwrap();
        let global = help.find("GLOBAL FLAGS").unwrap();
        let subcommands = help.find("SUBCOMMANDS").unwrap();
        assert!(description < usage);
        assert!(usage < arguments);
        assert!(arguments < flags);
        assert!(flags < global);
        assert!(global < subcommands);
    }
}
