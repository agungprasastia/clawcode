//! Strict, data-only loader for OpenCode-style project customization.

use super::{ConfigDiagnostic, jsonc};
use crate::workspace::{Operation, PolicyDecision, Risk};
use std::path::{Path, PathBuf};

const MAX_TEXT: usize = 16 * 1024;
const MAX_FILE: usize = 256 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agent {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub model: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub prompt: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    Allow,
    Ask,
    Deny,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionRule {
    pub operation: String,
    pub decision: Permission,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Theme {
    pub name: String,
    pub tokens: Vec<(String, String)>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompatibilityConfig {
    pub agents: Vec<Agent>,
    pub commands: Vec<Command>,
    pub permissions: Vec<PermissionRule>,
    pub themes: Vec<Theme>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CompatibilityLoader;

impl CompatibilityLoader {
    pub fn load_merged(
        &self,
        global_root: &Path,
        project_root: &Path,
    ) -> Result<CompatibilityConfig, ConfigDiagnostic> {
        let global = self.load_project(global_root)?;
        let project = self.load_project(project_root)?;
        Ok(merge(global, project))
    }

    pub fn parse_agent(
        &self,
        path: impl Into<String>,
        source: &str,
    ) -> Result<Agent, ConfigDiagnostic> {
        let path = path.into();
        let value = parse(path.clone(), source)?;
        let object = object(&value)?;
        let name = required_text(&path, object, "name", MAX_TEXT)?;
        let description = required_text(&path, object, "description", MAX_TEXT)?;
        let prompt = required_text(&path, object, "prompt", MAX_TEXT)?;
        let model = optional_text(&path, object, "model", MAX_TEXT)?;
        reject_unknown(&path, object, &["name", "description", "prompt", "model"])?;
        Ok(Agent {
            name,
            description,
            prompt,
            model,
        })
    }
    pub fn load_project(
        &self,
        project_root: &Path,
    ) -> Result<CompatibilityConfig, ConfigDiagnostic> {
        let root = compatibility_root(project_root);
        let mut config = CompatibilityConfig::default();
        self.load_directory(
            &root.join("agents"),
            |path, source| self.parse_agent(path, source),
            &mut config.agents,
        )?;
        self.load_directory(
            &root.join("commands"),
            |path, source| self.parse_command(path, source),
            &mut config.commands,
        )?;
        self.load_directory(
            &root.join("themes"),
            |path, source| self.parse_theme(path, source),
            &mut config.themes,
        )?;
        let permissions = root.join("permissions.jsonc");
        if permissions.is_file() {
            config.permissions = self
                .parse_permissions(permissions.display().to_string(), &load_file(&permissions)?)?;
        }
        Ok(config)
    }

    pub fn parse_command(
        &self,
        path: impl Into<String>,
        source: &str,
    ) -> Result<Command, ConfigDiagnostic> {
        let path = path.into();
        let value = parse(path.clone(), source)?;
        let object = object(&value)?;
        let name = required_text(&path, object, "name", MAX_TEXT)?;
        let description = required_text(&path, object, "description", MAX_TEXT)?;
        let prompt = required_text(&path, object, "prompt", MAX_TEXT)?;
        reject_unknown(&path, object, &["name", "description", "prompt"])?;
        Ok(Command {
            name,
            description,
            prompt,
        })
    }

    pub fn parse_permissions(
        &self,
        path: impl Into<String>,
        source: &str,
    ) -> Result<Vec<PermissionRule>, ConfigDiagnostic> {
        let path = path.into();
        let value = parse(path.clone(), source)?;
        let items = match value {
            jsonc::Value::Array { items, .. } => items,
            _ => return Err(parse_error(path, "permissions must be an array")),
        };
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            let object = object(&item)?;
            let operation = required_text(&path, object, "operation", MAX_TEXT)?;
            let decision = match required_text(&path, object, "decision", 32)?.as_str() {
                "allow" => Permission::Allow,
                "ask" => Permission::Ask,
                "deny" => Permission::Deny,
                _ => {
                    return Err(parse_error(
                        path.clone(),
                        "decision must be allow, ask, or deny",
                    ));
                }
            };
            reject_unknown(&path, object, &["operation", "decision"])?;
            result.push(PermissionRule {
                operation,
                decision,
            });
        }
        Ok(result)
    }

    pub fn parse_theme(
        &self,
        path: impl Into<String>,
        source: &str,
    ) -> Result<Theme, ConfigDiagnostic> {
        let path = path.into();
        let value = parse(path.clone(), source)?;
        let object = object(&value);
        let object = object?;
        let name = required_text(&path, object, "name", MAX_TEXT)?;
        let tokens_value = member(object, "tokens")
            .ok_or_else(|| parse_error(path.clone(), "missing field `tokens`"))?;
        let token_object = match tokens_value {
            jsonc::Value::Object { members, .. } => members,
            _ => return Err(parse_error(path.clone(), "tokens must be an object")),
        };
        let mut tokens = Vec::with_capacity(token_object.len());
        for (key, value) in token_object {
            let text = value
                .as_str()
                .ok_or_else(|| parse_error(path.clone(), "theme token must be string"))?;
            tokens.push((key.clone(), bounded(&path, text, MAX_TEXT)?));
        }
        reject_unknown(&path, object, &["name", "tokens"])?;
        Ok(Theme { name, tokens })
    }

    fn load_directory<T, F>(
        &self,
        directory: &Path,
        mut parse_one: F,
        output: &mut Vec<T>,
    ) -> Result<(), ConfigDiagnostic>
    where
        F: FnMut(String, &str) -> Result<T, ConfigDiagnostic>,
        T: NamedCompat,
    {
        if !directory.is_dir() {
            return Ok(());
        }
        let mut entries = std::fs::read_dir(directory)
            .map_err(|error| parse_error(directory.display().to_string(), error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| parse_error(directory.display().to_string(), error.to_string()))?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonc") {
                continue;
            }
            let value = parse_one(path.display().to_string(), &load_file(&path)?)?;
            if output
                .iter()
                .any(|item| item.compat_name() == value.compat_name())
            {
                return Err(parse_error(
                    path.display().to_string(),
                    "duplicate compatibility name",
                ));
            }
            output.push(value);
        }
        Ok(())
    }
}

impl PermissionRule {
    pub fn policy_decision(&self, mode: crate::workspace::Mode) -> Option<PolicyDecision> {
        let operation = match self.operation.as_str() {
            "write" => Operation::Write,
            "sensitive_write" => Operation::SensitiveWrite,
            "delete" => Operation::Delete,
            "shell" => Operation::Shell(Risk::Safe),
            "shell:destructive" => Operation::Shell(Risk::Destructive),
            "shell:dependency_install" => Operation::Shell(Risk::DependencyInstall),
            "shell:network_mutation" => Operation::Shell(Risk::NetworkMutation),
            "shell:privilege_escalation" => Operation::Shell(Risk::PrivilegeEscalation),
            _ => return None,
        };
        let base = crate::workspace::Policy::evaluate(mode, operation);
        Some(match self.decision {
            Permission::Deny => PolicyDecision::Denied,
            Permission::Ask if base != PolicyDecision::Denied => PolicyDecision::ApprovalRequired,
            Permission::Allow if base != PolicyDecision::Denied => base,
            _ => base,
        })
    }
}

fn merge(global: CompatibilityConfig, project: CompatibilityConfig) -> CompatibilityConfig {
    fn merge_named<T: NamedCompat>(mut global: Vec<T>, project: Vec<T>) -> Vec<T> {
        for item in project {
            global.retain(|existing| existing.compat_name() != item.compat_name());
            global.push(item);
        }
        global
    }
    CompatibilityConfig {
        agents: merge_named(global.agents, project.agents),
        commands: merge_named(global.commands, project.commands),
        permissions: if project.permissions.is_empty() {
            global.permissions
        } else {
            project.permissions
        },
        themes: merge_named(global.themes, project.themes),
    }
}

trait NamedCompat {
    fn compat_name(&self) -> &str;
}
impl NamedCompat for Agent {
    fn compat_name(&self) -> &str {
        &self.name
    }
}
impl NamedCompat for Command {
    fn compat_name(&self) -> &str {
        &self.name
    }
}
impl NamedCompat for Theme {
    fn compat_name(&self) -> &str {
        &self.name
    }
}

fn parse(path: String, source: &str) -> Result<jsonc::Value, ConfigDiagnostic> {
    jsonc::parse(source).map_err(|(line, column, message)| ConfigDiagnostic::Parse {
        path,
        line,
        column,
        message,
    })
}
fn parse_error(path: impl Into<String>, message: impl Into<String>) -> ConfigDiagnostic {
    ConfigDiagnostic::Parse {
        path: path.into(),
        line: 1,
        column: 1,
        message: message.into(),
    }
}
fn object(value: &jsonc::Value) -> Result<&Vec<(String, jsonc::Value)>, ConfigDiagnostic> {
    value
        .as_object()
        .ok_or_else(|| parse_error("compat", "value must be an object"))
}
fn member<'a>(object: &'a [(String, jsonc::Value)], name: &str) -> Option<&'a jsonc::Value> {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}
fn required_text(
    path: &str,
    object: &[(String, jsonc::Value)],
    name: &str,
    max: usize,
) -> Result<String, ConfigDiagnostic> {
    member(object, name)
        .and_then(jsonc::Value::as_str)
        .ok_or_else(|| parse_error(path, format!("missing or invalid field `{name}`")))
        .and_then(|text| bounded(path, text, max))
}
fn optional_text(
    path: &str,
    object: &[(String, jsonc::Value)],
    name: &str,
    max: usize,
) -> Result<Option<String>, ConfigDiagnostic> {
    member(object, name)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| parse_error(path, format!("invalid field `{name}`")))
                .and_then(|text| bounded(path, text, max))
        })
        .transpose()
}
fn bounded(path: &str, text: &str, max: usize) -> Result<String, ConfigDiagnostic> {
    if text.len() > max {
        Err(parse_error(path, "text exceeds configured limit"))
    } else {
        Ok(text.to_string())
    }
}
fn reject_unknown(
    path: &str,
    object: &[(String, jsonc::Value)],
    known: &[&str],
) -> Result<(), ConfigDiagnostic> {
    if let Some((field, value)) = object
        .iter()
        .find(|(field, _)| !known.contains(&field.as_str()))
    {
        return Err(ConfigDiagnostic::UnknownField {
            path: path.into(),
            field: field.clone(),
            line: value.line(),
        });
    }
    Ok(())
}

pub fn load_file(path: &Path) -> Result<String, ConfigDiagnostic> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| parse_error(path.display().to_string(), error.to_string()))?;
    if metadata.len() > MAX_FILE as u64 {
        return Err(parse_error(
            path.display().to_string(),
            "file exceeds configured limit",
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|error| parse_error(path.display().to_string(), error.to_string()))
}

pub fn compatibility_root(project_root: &Path) -> PathBuf {
    project_root.join(".opencode")
}
