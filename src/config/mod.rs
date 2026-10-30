//! JSONC (JSON with comments and trailing commas) config: parser with exact
//! spans, typed secret references, schema migration, and merge. No warning
//! suppressions; everything here is used by tests or public API.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub mod compat;
pub mod jsonc;
pub mod secret;

pub use secret::SecretRef;

/// Current config schema version. Older versions migrate to this.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CustomModelModalities {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CustomModelConfig {
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub modalities: Option<CustomModelModalities>,
    pub variants: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThinkingDisplay {
    #[default]
    Hide,
    Show,
}

impl ThinkingDisplay {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hide => "hide",
            Self::Show => "show",
        }
    }

    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "hide" | "collapsed" | "false" | "off" => Some(Self::Hide),
            "show" | "expanded" | "true" | "on" => Some(Self::Show),
            _ => None,
        }
    }

    pub const fn toggle(self) -> Self {
        match self {
            Self::Hide => Self::Show,
            Self::Show => Self::Hide,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToolGrouping {
    #[default]
    Auto,
    None,
}

impl ToolGrouping {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
        }
    }

    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" | "true" | "on" => Some(Self::Auto),
            "none" | "false" | "off" => Some(Self::None),
            _ => None,
        }
    }

    pub const fn toggle(self) -> Self {
        match self {
            Self::Auto => Self::None,
            Self::None => Self::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TranscriptVerbosity {
    Low,
    #[default]
    Medium,
    High,
}

impl TranscriptVerbosity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" | "normal" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub const fn cycle(self, delta: isize) -> Self {
        const ALL: [TranscriptVerbosity; 3] = [
            TranscriptVerbosity::Low,
            TranscriptVerbosity::Medium,
            TranscriptVerbosity::High,
        ];
        let idx = match self {
            Self::Low => 0isize,
            Self::Medium => 1,
            Self::High => 2,
        };
        let next = (idx + delta).rem_euclid(3) as usize;
        ALL[next]
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkdownMode {
    #[default]
    Rendered,
    Source,
}

impl MarkdownMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rendered => "rendered",
            Self::Source => "source",
        }
    }

    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rendered" | "true" | "on" => Some(Self::Rendered),
            "source" | "raw" | "false" | "off" => Some(Self::Source),
            _ => None,
        }
    }

    pub const fn toggle(self) -> Self {
        match self {
            Self::Rendered => Self::Source,
            Self::Source => Self::Rendered,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TurnTokensMode {
    #[default]
    Off,
    Compact,
    Verbose,
}

impl TurnTokensMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Compact => "compact",
            Self::Verbose => "verbose",
        }
    }

    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "none" => Some(Self::Off),
            "compact" | "true" | "on" => Some(Self::Compact),
            "verbose" => Some(Self::Verbose),
            _ => None,
        }
    }

    pub const fn cycle(self, delta: isize) -> Self {
        const ALL: [TurnTokensMode; 3] = [
            TurnTokensMode::Off,
            TurnTokensMode::Compact,
            TurnTokensMode::Verbose,
        ];
        let idx = match self {
            Self::Off => 0isize,
            Self::Compact => 1,
            Self::Verbose => 2,
        };
        let next = (idx + delta).rem_euclid(3) as usize;
        ALL[next]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiSettingsConfig {
    pub animations: bool,
    pub thinking: ThinkingDisplay,
    pub grouping: ToolGrouping,
    pub verbosity: TranscriptVerbosity,
    pub markdown: MarkdownMode,
    pub tps: bool,
    pub scrollbar: bool,
    pub scroll_speed: u16,
    pub scroll_acceleration: bool,
    pub mouse: bool,
    pub leader_timeout_ms: u64,
    pub turn_tokens: TurnTokensMode,
}

impl Default for TuiSettingsConfig {
    fn default() -> Self {
        Self {
            animations: true,
            thinking: ThinkingDisplay::Hide,
            grouping: ToolGrouping::Auto,
            verbosity: TranscriptVerbosity::Medium,
            markdown: MarkdownMode::Rendered,
            tps: true,
            scrollbar: true,
            scroll_speed: 3,
            scroll_acceleration: false,
            mouse: true,
            leader_timeout_ms: 2000,
            turn_tokens: TurnTokensMode::Off,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct TuiSettingsPatch {
    animations: Option<bool>,
    thinking: Option<ThinkingDisplay>,
    grouping: Option<ToolGrouping>,
    verbosity: Option<TranscriptVerbosity>,
    markdown: Option<MarkdownMode>,
    tps: Option<bool>,
    scrollbar: Option<bool>,
    scroll_speed: Option<u16>,
    scroll_acceleration: Option<bool>,
    mouse: Option<bool>,
    leader_timeout_ms: Option<u64>,
    turn_tokens: Option<TurnTokensMode>,
}

impl TuiSettingsPatch {
    fn merge_from(&mut self, other: &Self) {
        if other.animations.is_some() {
            self.animations = other.animations;
        }
        if other.thinking.is_some() {
            self.thinking = other.thinking;
        }
        if other.grouping.is_some() {
            self.grouping = other.grouping;
        }
        if other.verbosity.is_some() {
            self.verbosity = other.verbosity;
        }
        if other.markdown.is_some() {
            self.markdown = other.markdown;
        }
        if other.tps.is_some() {
            self.tps = other.tps;
        }
        if other.scrollbar.is_some() {
            self.scrollbar = other.scrollbar;
        }
        if other.scroll_speed.is_some() {
            self.scroll_speed = other.scroll_speed;
        }
        if other.scroll_acceleration.is_some() {
            self.scroll_acceleration = other.scroll_acceleration;
        }
        if other.mouse.is_some() {
            self.mouse = other.mouse;
        }
        if other.leader_timeout_ms.is_some() {
            self.leader_timeout_ms = other.leader_timeout_ms;
        }
        if other.turn_tokens.is_some() {
            self.turn_tokens = other.turn_tokens;
        }
    }

    fn resolve(&self) -> TuiSettingsConfig {
        let defaults = TuiSettingsConfig::default();
        TuiSettingsConfig {
            animations: self.animations.unwrap_or(defaults.animations),
            thinking: self.thinking.unwrap_or(defaults.thinking),
            grouping: self.grouping.unwrap_or(defaults.grouping),
            verbosity: self.verbosity.unwrap_or(defaults.verbosity),
            markdown: self.markdown.unwrap_or(defaults.markdown),
            tps: self.tps.unwrap_or(defaults.tps),
            scrollbar: self.scrollbar.unwrap_or(defaults.scrollbar),
            scroll_speed: self.scroll_speed.unwrap_or(defaults.scroll_speed),
            scroll_acceleration: self
                .scroll_acceleration
                .unwrap_or(defaults.scroll_acceleration),
            mouse: self.mouse.unwrap_or(defaults.mouse),
            leader_timeout_ms: self.leader_timeout_ms.unwrap_or(defaults.leader_timeout_ms),
            turn_tokens: self.turn_tokens.unwrap_or(defaults.turn_tokens),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CustomProviderConfig {
    pub name: Option<String>,
    pub npm: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub models: BTreeMap<String, CustomModelConfig>,
}

impl CustomProviderConfig {
    pub fn resolved_api_key(&self) -> Option<String> {
        let value = self.api_key.as_deref()?.trim();
        if value.is_empty() {
            return None;
        }
        if let Some(rest) = value
            .strip_prefix("{env:")
            .and_then(|v| v.strip_suffix('}'))
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return std::env::var(rest)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
        }
        if let Some(rest) = value
            .strip_prefix("env:")
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return std::env::var(rest)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
        }
        Some(value.to_string())
    }

    pub fn to_model_infos(&self) -> Vec<crate::provider::ModelInfo> {
        self.models
            .iter()
            .map(|(id, m)| crate::provider::ModelInfo {
                id: id.clone(),
                context_window: m.context_window.unwrap_or(4096),
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigAgent {
    pub model: Option<String>,
    pub temperature: Option<u32>,
}

/// One MCP server from `mcp`/`mcp.servers`. Local servers are spawned over
/// stdio; remote servers use streamable HTTP.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct McpServerConfig {
    pub enabled: bool,
    pub command: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub oauth: bool,
}

impl McpServerConfig {
    pub fn is_remote(&self) -> bool {
        self.url.is_some()
    }
}

/// Manual Debug: SecretRef already redacts values; keep Config debug safe.
#[derive(Clone, Debug, Eq)]
pub struct Config {
    pub schema_version: u32,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<SecretRef>,
    pub providers: BTreeMap<String, CustomProviderConfig>,
    pub agents: BTreeMap<String, ConfigAgent>,
    pub mcp: BTreeMap<String, McpServerConfig>,
    pub tui: TuiSettingsConfig,
    tui_patch: TuiSettingsPatch,
}

impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.model == other.model
            && self.endpoint == other.endpoint
            && self.api_key == other.api_key
            && self.providers == other.providers
            && self.agents == other.agents
            && self.mcp == other.mcp
            && self.tui == other.tui
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            model: None,
            endpoint: None,
            api_key: None,
            providers: BTreeMap::new(),
            agents: BTreeMap::new(),
            mcp: BTreeMap::new(),
            tui: TuiSettingsConfig::default(),
            tui_patch: TuiSettingsPatch::default(),
        }
    }
}

impl Config {
    pub fn resolved_endpoint(&self, provider_id: Option<&str>) -> Option<&str> {
        if let Some(ep) = self.endpoint.as_deref() {
            return Some(ep);
        }
        provider_id
            .and_then(|id| self.providers.get(id))
            .and_then(|p| p.base_url.as_deref())
    }

    pub fn initial_provider_and_model(&self) -> (String, String) {
        if let Some(ref m) = self.model {
            for p in self.providers.keys() {
                if let Some(rest) = m.strip_prefix(&format!("{p}/")) {
                    return (p.clone(), rest.to_string());
                }
            }
            if let Some((p, _)) = self
                .providers
                .iter()
                .find(|(_, cfg)| cfg.models.contains_key(m))
            {
                return (p.clone(), m.clone());
            }
            if self.providers.len() == 1 {
                let p = self.providers.keys().next().unwrap().clone();
                return (p, m.clone());
            }
            if let Some((p, model_name)) = m.split_once('/') {
                return (p.to_string(), model_name.to_string());
            }
            if let Some(p) = self.providers.keys().next() {
                return (p.clone(), m.clone());
            }
            return (String::new(), m.clone());
        }

        if let Some((p, cfg)) = self.providers.iter().next() {
            let model = cfg.models.keys().next().cloned().unwrap_or_default();
            return (p.clone(), model);
        }

        (String::new(), String::new())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigDiagnostic {
    Parse {
        path: String,
        line: usize,
        column: usize,
        message: String,
    },
    UnknownField {
        path: String,
        field: String,
        line: usize,
    },
    PlaintextSecret {
        path: String,
        field: String,
    },
    MissingSecret {
        name: String,
    },
}

impl fmt::Display for ConfigDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse {
                path,
                line,
                column,
                message,
            } => write!(f, "{path}:{line}:{column}: {message}"),
            Self::UnknownField { path, field, line } => {
                write!(f, "{path}:{line}: unknown field `{field}`")
            }
            Self::PlaintextSecret { path, field } => write!(
                f,
                "{path}: plaintext secret `{field}` is not allowed; use env: or credential: reference"
            ),
            Self::MissingSecret { name } => write!(f, "secret not found: env `{name}`"),
        }
    }
}

impl std::error::Error for ConfigDiagnostic {}

const KNOWN_FIELDS: [&str; 22] = [
    "$schema",
    "schema_version",
    "model",
    "endpoint",
    "api_key",
    "provider",
    "providers",
    "agent",
    "agents",
    "mcp",
    "plugin",
    "plugins",
    "theme",
    "keymap",
    "keybinds",
    "tui",
    "animations",
    "session",
    "scroll",
    "mouse",
    "leader",
    "debug",
];

fn parse_secret(
    path: &str,
    field: &str,
    value: &jsonc::Value,
) -> Result<Option<SecretRef>, ConfigDiagnostic> {
    match value {
        jsonc::Value::String { value, .. } => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else if let Some(rest) = trimmed.strip_prefix("env:") {
                Ok(Some(SecretRef::Env(rest.to_string())))
            } else if let Some(rest) = trimmed
                .strip_prefix("{env:")
                .and_then(|s| s.strip_suffix('}'))
            {
                Ok(Some(SecretRef::Env(rest.trim().to_string())))
            } else if let Some(rest) = trimmed.strip_prefix("credential:") {
                Ok(Some(SecretRef::Credential(rest.to_string())))
            } else {
                Err(ConfigDiagnostic::PlaintextSecret {
                    path: path.to_string(),
                    field: field.to_string(),
                })
            }
        }
        _ => Err(ConfigDiagnostic::PlaintextSecret {
            path: path.to_string(),
            field: field.to_string(),
        }),
    }
}

fn parse_models(
    path: &str,
    val: &jsonc::Value,
) -> Result<BTreeMap<String, CustomModelConfig>, ConfigDiagnostic> {
    let obj = val.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
        path: path.to_string(),
        line: val.line(),
        column: 1,
        message: "models must be an object".into(),
    })?;

    let mut map = BTreeMap::new();
    for (model_id, mval) in obj {
        let mobj = mval.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
            path: path.to_string(),
            line: mval.line(),
            column: 1,
            message: format!("model `{model_id}` must be an object"),
        })?;

        let mut model_config = CustomModelConfig::default();

        for (k, v) in mobj {
            match k.as_str() {
                "name" => {
                    model_config.name = v.as_str().map(str::to_string);
                }
                "context_window" | "contextWindow" => {
                    model_config.context_window = v.as_u64();
                }
                "max_output_tokens" | "maxOutputTokens" => {
                    model_config.max_output_tokens = v.as_u64();
                }
                "limit" => {
                    if let Some(limits) = v.as_object() {
                        for (lk, lv) in limits {
                            match lk.as_str() {
                                "context" => {
                                    model_config.context_window = lv.as_u64();
                                }
                                "output" => {
                                    model_config.max_output_tokens = lv.as_u64();
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "modalities" => {
                    if let Some(mods) = v.as_object() {
                        let mut modalities = CustomModelModalities::default();
                        for (mk, mv) in mods {
                            let list: Vec<String> = mv
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(jsonc::Value::as_str)
                                        .map(str::to_string)
                                        .collect()
                                })
                                .unwrap_or_default();
                            match mk.as_str() {
                                "input" => modalities.input = list,
                                "output" => modalities.output = list,
                                _ => {}
                            }
                        }
                        model_config.modalities = Some(modalities);
                    }
                }
                "variants" => {
                    if let Some(arr) = v.as_array() {
                        model_config.variants = arr
                            .iter()
                            .filter_map(jsonc::Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect();
                    } else if let Some(vobj) = v.as_object() {
                        model_config.variants = vobj
                            .iter()
                            .map(|(vk, _)| vk.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                _ => {}
            }
        }
        map.insert(model_id.clone(), model_config);
    }
    Ok(map)
}

fn parse_providers(
    path: &str,
    val: &jsonc::Value,
) -> Result<BTreeMap<String, CustomProviderConfig>, ConfigDiagnostic> {
    let obj = val.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
        path: path.to_string(),
        line: val.line(),
        column: 1,
        message: "provider must be an object".into(),
    })?;

    let mut map = BTreeMap::new();
    for (name, pval) in obj {
        let pobj = pval.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
            path: path.to_string(),
            line: pval.line(),
            column: 1,
            message: format!("provider `{name}` must be an object"),
        })?;

        let mut provider_config = CustomProviderConfig::default();
        let mut base_url = None;
        let mut api_key = None;

        if let Some(opt_obj) = pobj
            .iter()
            .find(|(k, _)| k == "options")
            .and_then(|(_, v)| v.as_object())
        {
            for (ok, ov) in opt_obj {
                match ok.as_str() {
                    "baseURL" | "baseUrl" | "base_url" => {
                        base_url = ov.as_str().map(str::to_string);
                    }
                    "apiKey" | "api_key" => {
                        api_key = ov
                            .as_str()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty());
                    }
                    _ => {}
                }
            }
        }

        for (k, v) in pobj {
            match k.as_str() {
                "name" => {
                    provider_config.name = v.as_str().map(str::to_string);
                }
                "npm" => {
                    provider_config.npm = v.as_str().map(str::to_string);
                }
                "baseURL" | "baseUrl" | "base_url" => {
                    if base_url.is_none() {
                        base_url = v.as_str().map(str::to_string);
                    }
                }
                "apiKey" | "api_key" => {
                    if api_key.is_none() {
                        api_key = v
                            .as_str()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty());
                    }
                }
                "models" => {
                    provider_config.models = parse_models(path, v)?;
                }
                _ => {}
            }
        }
        provider_config.base_url = base_url;
        provider_config.api_key = api_key;
        map.insert(name.clone(), provider_config);
    }
    Ok(map)
}

fn parse_agents(
    path: &str,
    val: &jsonc::Value,
) -> Result<BTreeMap<String, ConfigAgent>, ConfigDiagnostic> {
    let obj = val.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
        path: path.to_string(),
        line: val.line(),
        column: 1,
        message: "agent must be an object".into(),
    })?;

    let mut map = BTreeMap::new();
    for (name, aval) in obj {
        let aobj = aval.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
            path: path.to_string(),
            line: aval.line(),
            column: 1,
            message: format!("agent `{name}` must be an object"),
        })?;

        let mut agent = ConfigAgent::default();
        for (k, v) in aobj {
            match k.as_str() {
                "model" => agent.model = v.as_str().map(str::to_string),
                "temperature" => {
                    agent.temperature = v.as_u64().and_then(|n| u32::try_from(n).ok());
                }
                _ => {}
            }
        }
        map.insert(name.clone(), agent);
    }
    Ok(map)
}

fn parse_string_map(
    path: &str,
    val: &jsonc::Value,
    field: &str,
) -> Result<BTreeMap<String, String>, ConfigDiagnostic> {
    let obj = val.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
        path: path.to_string(),
        line: val.line(),
        column: 1,
        message: format!("{field} must be an object"),
    })?;
    let mut map = BTreeMap::new();
    for (key, value) in obj {
        let value = value.as_str().ok_or_else(|| ConfigDiagnostic::Parse {
            path: path.to_string(),
            line: value.line(),
            column: 1,
            message: format!("{field}.{key} must be a string"),
        })?;
        map.insert(key.clone(), value.to_string());
    }
    Ok(map)
}

fn parse_mcp_servers(
    path: &str,
    val: &jsonc::Value,
) -> Result<BTreeMap<String, McpServerConfig>, ConfigDiagnostic> {
    let obj = val.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
        path: path.to_string(),
        line: val.line(),
        column: 1,
        message: "mcp must be an object".into(),
    })?;
    let entries = obj
        .iter()
        .find(|(key, _)| key == "servers")
        .map(|(_, servers)| {
            servers.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
                path: path.to_string(),
                line: servers.line(),
                column: 1,
                message: "mcp.servers must be an object".into(),
            })
        })
        .transpose()?
        .unwrap_or(obj);

    let mut map = BTreeMap::new();
    for (name, value) in entries {
        let server = value.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
            path: path.to_string(),
            line: value.line(),
            column: 1,
            message: format!("mcp server `{name}` must be an object"),
        })?;
        let mut config = McpServerConfig {
            enabled: true,
            oauth: true,
            ..McpServerConfig::default()
        };
        for (key, value) in server {
            match key.as_str() {
                "enabled" => config.enabled = value.as_bool().unwrap_or(true),
                "disabled" => {
                    if value.as_bool() == Some(true) {
                        config.enabled = false;
                    }
                }
                "command" => {
                    config.command = if let Some(command) = value.as_str() {
                        vec![command.to_string()]
                    } else if let Some(items) = value.as_array() {
                        items
                            .iter()
                            .map(|item| {
                                item.as_str().map(str::to_string).ok_or_else(|| {
                                    ConfigDiagnostic::Parse {
                                        path: path.to_string(),
                                        line: item.line(),
                                        column: 1,
                                        message: format!(
                                            "mcp server `{name}` command entries must be strings"
                                        ),
                                    }
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    } else {
                        return Err(ConfigDiagnostic::Parse {
                            path: path.to_string(),
                            line: value.line(),
                            column: 1,
                            message: format!("mcp server `{name}` command must be a string array"),
                        });
                    };
                }
                "environment" | "env" => {
                    config.environment =
                        parse_string_map(path, value, &format!("mcp server `{name}` environment"))?;
                }
                "url" => config.url = value.as_str().map(str::to_string),
                "headers" => {
                    config.headers =
                        parse_string_map(path, value, &format!("mcp server `{name}` headers"))?;
                }
                "oauth" => config.oauth = value.as_bool().unwrap_or(true),
                _ => {}
            }
        }
        if config.enabled && config.command.is_empty() && config.url.is_none() {
            return Err(ConfigDiagnostic::Parse {
                path: path.to_string(),
                line: value.line(),
                column: 1,
                message: format!("mcp server `{name}` needs a command or url"),
            });
        }
        map.insert(name.clone(), config);
    }
    Ok(map)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn xdg_config_home() -> PathBuf {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(val) if !val.is_empty() => PathBuf::from(val),
        _ => home_dir()
            .map(|h| h.join(".config"))
            .unwrap_or_else(|| PathBuf::from(".")),
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigLoader;

impl ConfigLoader {
    pub fn parse_str(
        &self,
        path: impl Into<String>,
        source: &str,
    ) -> Result<Config, ConfigDiagnostic> {
        let path = path.into();
        let value =
            jsonc::parse(source).map_err(|(line, column, message)| ConfigDiagnostic::Parse {
                path: path.clone(),
                line,
                column,
                message,
            })?;

        let object = value.as_object().ok_or_else(|| ConfigDiagnostic::Parse {
            path: path.clone(),
            line: 1,
            column: 1,
            message: "config must be an object".into(),
        })?;

        let schema_version_val = object
            .iter()
            .find(|(k, _)| k == "schema_version")
            .map(|(_, v)| v);

        let schema_version = match schema_version_val {
            Some(v) => v.as_u64().ok_or_else(|| ConfigDiagnostic::Parse {
                path: path.clone(),
                line: v.line(),
                column: 1,
                message: "missing or invalid schema_version".into(),
            })?,
            None => {
                let has_opencode_keys = object.iter().any(|(k, _)| {
                    matches!(
                        k.as_str(),
                        "$schema"
                            | "provider"
                            | "providers"
                            | "agent"
                            | "agents"
                            | "tui"
                            | "animations"
                            | "session"
                            | "scroll"
                            | "mouse"
                            | "leader"
                            | "debug"
                    )
                });
                if has_opencode_keys {
                    u64::from(CURRENT_SCHEMA_VERSION)
                } else {
                    return Err(ConfigDiagnostic::Parse {
                        path: path.clone(),
                        line: 1,
                        column: 1,
                        message: "missing or invalid schema_version".into(),
                    });
                }
            }
        };

        if schema_version > u64::from(CURRENT_SCHEMA_VERSION) {
            return Err(ConfigDiagnostic::Parse {
                path,
                line: 1,
                column: 1,
                message: format!(
                    "unsupported schema_version {schema_version}; current is {CURRENT_SCHEMA_VERSION}"
                ),
            });
        }

        // Migration hook: versions below current convert here, chaining to
        // CURRENT_SCHEMA_VERSION. v1 is current and passes through unchanged.
        let migrated = migrate(schema_version as u32, value);
        let object = migrated
            .as_object()
            .ok_or_else(|| ConfigDiagnostic::Parse {
                path: path.clone(),
                line: 1,
                column: 1,
                message: "config must be an object".into(),
            })?;

        for (key, val) in object {
            if !KNOWN_FIELDS.contains(&key.as_str()) {
                return Err(ConfigDiagnostic::UnknownField {
                    path: path.clone(),
                    field: key.clone(),
                    line: val.line(),
                });
            }
        }

        let api_key = match object
            .iter()
            .find(|(k, _)| k == "api_key" || k == "apiKey")
            .map(|(_, v)| v)
        {
            Some(v) => parse_secret(&path, "api_key", v)?,
            None => None,
        };

        let get_non_empty_str = |name: &str| {
            object
                .iter()
                .find(|(k, _)| k == name)
                .and_then(|(_, v)| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };

        let mut providers = BTreeMap::new();
        for (k, v) in object {
            if k == "provider" || k == "providers" {
                let parsed = parse_providers(&path, v)?;
                providers.extend(parsed);
            }
        }

        let mut agents = BTreeMap::new();
        for (k, v) in object {
            if k == "agent" || k == "agents" {
                let parsed = parse_agents(&path, v)?;
                agents.extend(parsed);
            }
        }

        let mut mcp = BTreeMap::new();
        for (k, v) in object {
            if k == "mcp" {
                let parsed = parse_mcp_servers(&path, v)?;
                mcp.extend(parsed);
            }
        }

        let mut tui_patch = parse_tui_patch_from_object(object);
        for (k, v) in object {
            if k == "tui"
                && let Some(tui_obj) = v.as_object()
            {
                let nested = parse_tui_patch_from_object(tui_obj);
                tui_patch.merge_from(&nested);
            }
        }
        let tui = tui_patch.resolve();

        Ok(Config {
            schema_version: CURRENT_SCHEMA_VERSION,
            model: get_non_empty_str("model"),
            endpoint: get_non_empty_str("endpoint"),
            api_key,
            providers,
            agents,
            mcp,
            tui,
            tui_patch,
        })
    }

    pub fn active_config_path(&self) -> PathBuf {
        let xdg = xdg_config_home();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        self.active_config_path_with_roots(&xdg, &cwd)
    }

    pub fn active_config_path_with_roots(&self, xdg_config: &Path, project_root: &Path) -> PathBuf {
        let project_candidates = [
            project_root.join(".clawcode").join("config.jsonc"),
            project_root.join(".clawcode").join("clawcode.jsonc"),
            project_root.join(".clawcode").join("clawcode.json"),
            project_root.join("clawcode.jsonc"),
            project_root.join("clawcode.json"),
            project_root.join(".opencode").join("opencode.jsonc"),
            project_root.join(".opencode").join("opencode.json"),
            project_root.join("opencode.jsonc"),
            project_root.join("opencode.json"),
        ];
        for candidate in &project_candidates {
            if candidate.is_file() {
                return candidate.clone();
            }
        }
        let global_candidates = [
            xdg_config.join("clawcode").join("config.jsonc"),
            xdg_config.join("clawcode").join("clawcode.jsonc"),
            xdg_config.join("clawcode").join("clawcode.json"),
            xdg_config.join("opencode").join("opencode.jsonc"),
            xdg_config.join("opencode").join("opencode.json"),
        ];
        for candidate in &global_candidates {
            if candidate.is_file() {
                return candidate.clone();
            }
        }
        xdg_config.join("clawcode").join("clawcode.jsonc")
    }

    pub fn load(&self) -> Result<Config, ConfigDiagnostic> {
        let xdg = xdg_config_home();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        self.load_with_paths(&xdg, &cwd)
    }

    pub fn load_with_paths(
        &self,
        xdg_config: &Path,
        project_root: &Path,
    ) -> Result<Config, ConfigDiagnostic> {
        let global = self.find_and_load_global(xdg_config)?;
        let project = self.find_and_load_project(project_root)?;

        match (global, project) {
            (Some(g), Some(p)) => Config::merge(g, p),
            (Some(g), None) => Ok(g),
            (None, Some(p)) => Ok(p),
            (None, None) => Ok(Config::default()),
        }
    }

    fn find_and_load_global(&self, xdg_config: &Path) -> Result<Option<Config>, ConfigDiagnostic> {
        let candidates = [
            xdg_config.join("clawcode").join("config.jsonc"),
            xdg_config.join("clawcode").join("clawcode.jsonc"),
            xdg_config.join("clawcode").join("clawcode.json"),
            xdg_config.join("opencode").join("opencode.jsonc"),
            xdg_config.join("opencode").join("opencode.json"),
            xdg_config.join("crabcode").join("crabcode.jsonc"),
            xdg_config.join("crabcode").join("crabcode.json"),
        ];

        for path in &candidates {
            if path.is_file() {
                let content =
                    std::fs::read_to_string(path).map_err(|e| ConfigDiagnostic::Parse {
                        path: path.display().to_string(),
                        line: 1,
                        column: 1,
                        message: format!("cannot read config file: {e}"),
                    })?;
                let config = self.parse_str(path.display().to_string(), &content)?;
                return Ok(Some(config));
            }
        }
        Ok(None)
    }

    fn find_and_load_project(
        &self,
        project_root: &Path,
    ) -> Result<Option<Config>, ConfigDiagnostic> {
        let candidates = [
            project_root.join(".clawcode").join("config.jsonc"),
            project_root.join(".clawcode").join("clawcode.jsonc"),
            project_root.join(".clawcode").join("clawcode.json"),
            project_root.join("clawcode.jsonc"),
            project_root.join("clawcode.json"),
            project_root.join(".opencode").join("opencode.jsonc"),
            project_root.join(".opencode").join("opencode.json"),
            project_root.join("opencode.jsonc"),
            project_root.join("opencode.json"),
            project_root.join(".crabcode").join("crabcode.jsonc"),
            project_root.join("crabcode.jsonc"),
        ];

        for path in &candidates {
            if path.is_file() {
                let content =
                    std::fs::read_to_string(path).map_err(|e| ConfigDiagnostic::Parse {
                        path: path.display().to_string(),
                        line: 1,
                        column: 1,
                        message: format!("cannot read config file: {e}"),
                    })?;
                let config = self.parse_str(path.display().to_string(), &content)?;
                return Ok(Some(config));
            }
        }
        Ok(None)
    }
}

fn migrate(from: u32, value: jsonc::Value) -> jsonc::Value {
    // Future versions chain conversions: 1 => 2 => ... => CURRENT.
    let _ = from;
    value
}

impl Config {
    pub fn merge(global: Self, project: Self) -> Result<Self, ConfigDiagnostic> {
        if global.schema_version != project.schema_version {
            return Err(ConfigDiagnostic::Parse {
                path: "<merged>".into(),
                line: 1,
                column: 1,
                message: "schema_version mismatch".into(),
            });
        }
        let mut providers = global.providers;
        providers.extend(project.providers);
        let mut agents = global.agents;
        agents.extend(project.agents);
        let mut mcp = global.mcp;
        mcp.extend(project.mcp);

        let mut tui_patch = global.tui_patch;
        tui_patch.merge_from(&project.tui_patch);
        let tui = tui_patch.resolve();

        Ok(Self {
            schema_version: project.schema_version,
            model: project.model.or(global.model),
            endpoint: project.endpoint.or(global.endpoint),
            api_key: project.api_key.or(global.api_key),
            providers,
            agents,
            mcp,
            tui,
            tui_patch,
        })
    }

    pub fn save_tui_settings(path: &Path, settings: &TuiSettingsConfig) -> Result<(), String> {
        use crate::workspace::{FileSystem, RealFileSystem, temporary_sibling};

        let fs = RealFileSystem;
        let tui_block = format_tui_settings_json(settings);
        let updated_source = if fs.exists(path) {
            let bytes = fs
                .read(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let existing = String::from_utf8(bytes)
                .map_err(|e| format!("invalid UTF-8 in {}: {e}", path.display()))?;
            ConfigLoader
                .parse_str(path.display().to_string(), &existing)
                .map_err(|e| e.to_string())?;
            upsert_top_level_tui_block(&existing, &tui_block)?
        } else {
            format!(
                "{{\n  \"schema_version\": {CURRENT_SCHEMA_VERSION},\n  \"tui\": {tui_block}\n}}\n"
            )
        };

        ConfigLoader
            .parse_str(path.display().to_string(), &updated_source)
            .map_err(|e| e.to_string())?;

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs.create_dir_all(parent)
                .map_err(|e| format!("cannot create config directory {}: {e}", parent.display()))?;
        }

        let temp_path = temporary_sibling(path, "settings")
            .map_err(|e| format!("cannot allocate temp file for {}: {e}", path.display()))?;
        fs.write_new(&temp_path, updated_source.as_bytes())
            .map_err(|e| format!("cannot write temp config {}: {e}", temp_path.display()))?;
        if let Err(err) = fs.replace(&temp_path, path) {
            let _ = fs.remove_file(&temp_path);
            return Err(format!("cannot replace config {}: {err}", path.display()));
        }
        Ok(())
    }
}

fn parse_tui_patch_from_object(object: &[(String, jsonc::Value)]) -> TuiSettingsPatch {
    let mut patch = TuiSettingsPatch::default();
    for (k, v) in object {
        match k.as_str() {
            "animations" => {
                if let Some(b) = v.as_bool() {
                    patch.animations = Some(b);
                }
            }
            "mouse" => {
                if let Some(b) = v.as_bool() {
                    patch.mouse = Some(b);
                }
            }
            "session" => {
                if let Some(sobj) = v.as_object() {
                    for (sk, sv) in sobj {
                        match sk.as_str() {
                            "thinking" => {
                                if let Some(s) = sv.as_str() {
                                    patch.thinking = ThinkingDisplay::from_str_opt(s);
                                } else if let Some(b) = sv.as_bool() {
                                    patch.thinking = Some(if b {
                                        ThinkingDisplay::Show
                                    } else {
                                        ThinkingDisplay::Hide
                                    });
                                }
                            }
                            "grouping" => {
                                if let Some(s) = sv.as_str() {
                                    patch.grouping = ToolGrouping::from_str_opt(s);
                                } else if let Some(b) = sv.as_bool() {
                                    patch.grouping = Some(if b {
                                        ToolGrouping::Auto
                                    } else {
                                        ToolGrouping::None
                                    });
                                }
                            }
                            "verbosity" => {
                                if let Some(s) = sv.as_str() {
                                    patch.verbosity = TranscriptVerbosity::from_str_opt(s);
                                }
                            }
                            "markdown" => {
                                if let Some(s) = sv.as_str() {
                                    patch.markdown = MarkdownMode::from_str_opt(s);
                                } else if let Some(b) = sv.as_bool() {
                                    patch.markdown = Some(if b {
                                        MarkdownMode::Rendered
                                    } else {
                                        MarkdownMode::Source
                                    });
                                }
                            }
                            "tps" => {
                                if let Some(b) = sv.as_bool() {
                                    patch.tps = Some(b);
                                }
                            }
                            "scrollbar" => {
                                if let Some(b) = sv.as_bool() {
                                    patch.scrollbar = Some(b);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "scroll" => {
                if let Some(sc_obj) = v.as_object() {
                    for (sk, sv) in sc_obj {
                        match sk.as_str() {
                            "speed" => {
                                if let Some(n) = sv.as_u64() {
                                    patch.scroll_speed = Some(n.clamp(1, 10) as u16);
                                } else if let jsonc::Value::Number { value, .. } = sv
                                    && value.is_finite()
                                    && *value > 0.0
                                {
                                    patch.scroll_speed =
                                        Some((value.round() as u64).clamp(1, 10) as u16);
                                }
                            }
                            "acceleration" => {
                                if let Some(b) = sv.as_bool() {
                                    patch.scroll_acceleration = Some(b);
                                } else if let Some(aobj) = sv.as_object()
                                    && let Some((_, enabled)) =
                                        aobj.iter().find(|(ak, _)| ak == "enabled")
                                    && let Some(b) = enabled.as_bool()
                                {
                                    patch.scroll_acceleration = Some(b);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "leader" => {
                if let Some(lobj) = v.as_object() {
                    for (lk, lv) in lobj {
                        if lk == "timeout"
                            && let Some(ms) = lv.as_u64()
                        {
                            patch.leader_timeout_ms = Some(ms.clamp(250, 10_000));
                        }
                    }
                }
            }
            "debug" => {
                if let Some(dobj) = v.as_object() {
                    for (dk, dv) in dobj {
                        if dk == "turn_tokens" {
                            if let Some(b) = dv.as_bool() {
                                patch.turn_tokens = Some(if b {
                                    TurnTokensMode::Compact
                                } else {
                                    TurnTokensMode::Off
                                });
                            } else if let Some(s) = dv.as_str() {
                                patch.turn_tokens = TurnTokensMode::from_str_opt(s);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    patch
}

fn format_tui_settings_json(s: &TuiSettingsConfig) -> String {
    format!(
        "{{\n    \"animations\": {},\n    \"mouse\": {},\n    \"session\": {{\n      \"thinking\": \"{}\",\n      \"grouping\": \"{}\",\n      \"verbosity\": \"{}\",\n      \"markdown\": \"{}\",\n      \"tps\": {},\n      \"scrollbar\": {}\n    }},\n    \"scroll\": {{\n      \"speed\": {},\n      \"acceleration\": {}\n    }},\n    \"leader\": {{\n      \"timeout\": {}\n    }},\n    \"debug\": {{\n      \"turn_tokens\": \"{}\"\n    }}\n  }}",
        s.animations,
        s.mouse,
        s.thinking.as_str(),
        s.grouping.as_str(),
        s.verbosity.as_str(),
        s.markdown.as_str(),
        s.tps,
        s.scrollbar,
        s.scroll_speed,
        s.scroll_acceleration,
        s.leader_timeout_ms,
        s.turn_tokens.as_str(),
    )
}

fn skip_jsonc_ws_and_comments(bytes: &[u8], pos: &mut usize) {
    loop {
        match bytes.get(*pos).copied() {
            Some(b' ' | b'\t' | b'\r' | b'\n') => *pos += 1,
            Some(b'/') if bytes.get(*pos + 1) == Some(&b'/') => {
                *pos += 2;
                while let Some(b) = bytes.get(*pos).copied() {
                    if b == b'\n' {
                        break;
                    }
                    *pos += 1;
                }
            }
            Some(b'/') if bytes.get(*pos + 1) == Some(&b'*') => {
                *pos += 2;
                while *pos + 1 < bytes.len() {
                    if bytes[*pos] == b'*' && bytes[*pos + 1] == b'/' {
                        *pos += 2;
                        break;
                    }
                    *pos += 1;
                }
            }
            _ => break,
        }
    }
}

fn scan_json_raw_string(bytes: &[u8], pos: &mut usize) -> Result<String, String> {
    if bytes.get(*pos) != Some(&b'"') {
        return Err("expected string key in JSONC object".into());
    }
    let start = *pos;
    *pos += 1;
    while let Some(b) = bytes.get(*pos).copied() {
        *pos += 1;
        if b == b'\\' {
            *pos += 1;
        } else if b == b'"' {
            let slice = std::str::from_utf8(&bytes[start..*pos])
                .map_err(|_| "invalid UTF-8 in JSONC key".to_string())?;
            return match jsonc::parse(slice) {
                Ok(jsonc::Value::String { value, .. }) => Ok(value),
                _ => Err("invalid JSONC string literal".into()),
            };
        }
    }
    Err("unterminated JSONC string".into())
}

fn scan_jsonc_value(bytes: &[u8], pos: &mut usize) -> Result<(), String> {
    skip_jsonc_ws_and_comments(bytes, pos);
    match bytes.get(*pos).copied() {
        Some(b'"') => {
            let _ = scan_json_raw_string(bytes, pos)?;
            Ok(())
        }
        Some(b'{') => {
            *pos += 1;
            loop {
                skip_jsonc_ws_and_comments(bytes, pos);
                if bytes.get(*pos) == Some(&b'}') {
                    *pos += 1;
                    return Ok(());
                }
                let _ = scan_json_raw_string(bytes, pos)?;
                skip_jsonc_ws_and_comments(bytes, pos);
                if bytes.get(*pos) != Some(&b':') {
                    return Err("expected `:` in JSONC object".into());
                }
                *pos += 1;
                scan_jsonc_value(bytes, pos)?;
                skip_jsonc_ws_and_comments(bytes, pos);
                if bytes.get(*pos) == Some(&b',') {
                    *pos += 1;
                }
            }
        }
        Some(b'[') => {
            *pos += 1;
            loop {
                skip_jsonc_ws_and_comments(bytes, pos);
                if bytes.get(*pos) == Some(&b']') {
                    *pos += 1;
                    return Ok(());
                }
                scan_jsonc_value(bytes, pos)?;
                skip_jsonc_ws_and_comments(bytes, pos);
                if bytes.get(*pos) == Some(&b',') {
                    *pos += 1;
                }
            }
        }
        Some(_) => {
            while let Some(b) = bytes.get(*pos).copied() {
                if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b']' | b'/') {
                    break;
                }
                *pos += 1;
            }
            Ok(())
        }
        None => Err("unexpected EOF in JSONC value".into()),
    }
}

fn upsert_top_level_tui_block(source: &str, tui_block: &str) -> Result<String, String> {
    let bytes = source.as_bytes();
    let mut pos = 0usize;
    skip_jsonc_ws_and_comments(bytes, &mut pos);
    if bytes.get(pos) != Some(&b'{') {
        return Err("config root must be a JSON object".into());
    }
    pos += 1;

    let mut members: Vec<(String, usize, usize)> = Vec::new();
    let mut trailing_comma_after_last = false;
    let close_brace;

    loop {
        skip_jsonc_ws_and_comments(bytes, &mut pos);
        if bytes.get(pos) == Some(&b'}') {
            close_brace = pos;
            break;
        }
        let key = scan_json_raw_string(bytes, &mut pos)?;
        skip_jsonc_ws_and_comments(bytes, &mut pos);
        if bytes.get(pos) != Some(&b':') {
            return Err("expected `:` after top-level key".into());
        }
        pos += 1;
        skip_jsonc_ws_and_comments(bytes, &mut pos);
        let val_start = pos;
        scan_jsonc_value(bytes, &mut pos)?;
        let val_end = pos;
        members.push((key, val_start, val_end));

        skip_jsonc_ws_and_comments(bytes, &mut pos);
        if bytes.get(pos) == Some(&b',') {
            trailing_comma_after_last = true;
            pos += 1;
        } else {
            trailing_comma_after_last = false;
        }
    }

    if let Some((_, val_start, val_end)) = members.iter().rev().find(|(k, _, _)| k == "tui") {
        let mut out = String::with_capacity(source.len() + tui_block.len());
        out.push_str(&source[..*val_start]);
        out.push_str(tui_block);
        out.push_str(&source[*val_end..]);
        return Ok(out);
    }

    if members.is_empty() {
        let mut out = String::with_capacity(source.len() + tui_block.len() + 48);
        out.push_str(&source[..close_brace]);
        out.push_str(&format!(
            "\n  \"schema_version\": {CURRENT_SCHEMA_VERSION},\n  \"tui\": {tui_block}\n"
        ));
        out.push_str(&source[close_brace..]);
        return Ok(out);
    }

    let last_val_end = members.last().map(|(_, _, end)| *end).unwrap_or(0);
    let needs_leading_newline = close_brace > 0 && bytes[close_brace - 1] != b'\n';
    let prefix_nl = if needs_leading_newline { "\n" } else { "" };

    let mut out = String::with_capacity(source.len() + tui_block.len() + 32);
    if trailing_comma_after_last {
        out.push_str(&source[..close_brace]);
        out.push_str(&format!("{prefix_nl}  \"tui\": {tui_block},\n"));
        out.push_str(&source[close_brace..]);
    } else {
        out.push_str(&source[..last_val_end]);
        out.push(',');
        out.push_str(&source[last_val_end..close_brace]);
        out.push_str(&format!("{prefix_nl}  \"tui\": {tui_block}\n"));
        out.push_str(&source[close_brace..]);
    }
    Ok(out)
}
