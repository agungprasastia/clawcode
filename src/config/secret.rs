//! Typed secret references. Plaintext secrets cannot be represented; Debug
//! never exposes resolved values.

use crate::config::ConfigDiagnostic;

/// Reference to a secret stored outside config. Not resolved at parse time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecretRef {
    /// Environment variable name.
    Env(String),
    /// OS credential store entry id.
    Credential(String),
}

impl SecretRef {
    /// Resolve to the actual secret value. Only called when needed.
    pub fn resolve(&self) -> Result<String, ConfigDiagnostic> {
        match self {
            Self::Env(name) => std::env::var(name)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ConfigDiagnostic::MissingSecret { name: name.clone() }),
            // OS credential-store lookup lands with the Task 10 OS adapters.
            // Until then the reference is accepted and stored without
            // resolution; failing here would break config-only usage.
            Self::Credential(_) => Err(ConfigDiagnostic::MissingSecret {
                name: "credential-store (not yet available)".into(),
            }),
        }
    }
}
