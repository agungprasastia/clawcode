//! JSONC (JSON with comments and trailing commas) config: parser with exact
//! spans, typed secret references, schema migration, and merge. No warning
//! suppressions; everything here is used by tests or public API.

use std::fmt;

pub mod jsonc;
pub mod secret;

pub use secret::SecretRef;

/// Current config schema version. Older versions migrate to this.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Manual Debug: SecretRef already redacts values; keep Config debug safe.
#[derive(Debug)]
pub struct Config {
    pub schema_version: u32,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<SecretRef>,
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

const KNOWN_FIELDS: [&str; 4] = ["schema_version", "model", "endpoint", "api_key"];

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

        let schema_version = object
            .iter()
            .find(|(k, _)| k == "schema_version")
            .map(|(_, v)| v)
            .and_then(jsonc::Value::as_u64)
            .ok_or_else(|| ConfigDiagnostic::Parse {
                path: path.clone(),
                line: 1,
                column: 1,
                message: "missing or invalid schema_version".into(),
            })?;

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

        let api_key = match object.iter().find(|(k, _)| k == "api_key").map(|(_, v)| v) {
            Some(jsonc::Value::String { value, .. }) => {
                if let Some(rest) = value.strip_prefix("env:") {
                    Some(SecretRef::Env(rest.to_string()))
                } else if let Some(rest) = value.strip_prefix("credential:") {
                    Some(SecretRef::Credential(rest.to_string()))
                } else {
                    return Err(ConfigDiagnostic::PlaintextSecret {
                        path,
                        field: "api_key".into(),
                    });
                }
            }
            Some(_) => {
                return Err(ConfigDiagnostic::PlaintextSecret {
                    path,
                    field: "api_key".into(),
                });
            }
            None => None,
        };

        let get_str = |name: &str| {
            object
                .iter()
                .find(|(k, _)| k == name)
                .and_then(|(_, v)| v.as_str())
                .map(str::to_string)
        };

        Ok(Config {
            schema_version: CURRENT_SCHEMA_VERSION,
            model: get_str("model"),
            endpoint: get_str("endpoint"),
            api_key,
        })
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
        Ok(Self {
            schema_version: project.schema_version,
            model: project.model.or(global.model),
            endpoint: project.endpoint.or(global.endpoint),
            api_key: project.api_key.or(global.api_key),
        })
    }
}
