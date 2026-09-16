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

/// Manual Debug: SecretRef already redacts values; keep Config debug safe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub schema_version: u32,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<SecretRef>,
    pub providers: BTreeMap<String, CustomProviderConfig>,
    pub agents: BTreeMap<String, ConfigAgent>,
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

const KNOWN_FIELDS: [&str; 14] = [
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
                    k == "$schema"
                        || k == "provider"
                        || k == "providers"
                        || k == "agent"
                        || k == "agents"
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

        Ok(Config {
            schema_version: CURRENT_SCHEMA_VERSION,
            model: get_non_empty_str("model"),
            endpoint: get_non_empty_str("endpoint"),
            api_key,
            providers,
            agents,
        })
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

        Ok(Self {
            schema_version: project.schema_version,
            model: project.model.or(global.model),
            endpoint: project.endpoint.or(global.endpoint),
            api_key: project.api_key.or(global.api_key),
            providers,
            agents,
        })
    }
}
