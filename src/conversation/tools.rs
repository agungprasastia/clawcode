use crate::workspace::{Mode, Mutation, TransactionResult, Workspace, WorkspacePreview};
use std::path::{Path, PathBuf};

pub const TOOL_READ_MAX_BYTES: usize = 256 * 1024;
pub const TOOL_MUTATION_MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolRequest {
    Read { path: PathBuf, max_bytes: usize },
    Build { mutations: Vec<Mutation> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolStatus {
    Requested,
    Running,
    Completed,
    Failed(String),
    Cancelled,
    AwaitingApproval,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ToolResult {
    pub status: ToolStatus,
    pub read: Option<crate::workspace::ReadResult>,
    pub transaction: Option<TransactionResult>,
    pub preview: Option<WorkspacePreview>,
}

pub struct ToolLifecycle<'a, F: crate::workspace::FileSystem> {
    workspace: &'a Workspace<F>,
    mode: Mode,
    cancelled: bool,
    pending: Option<Vec<Mutation>>,
    diff: Option<TransactionResult>,
    preview: Option<WorkspacePreview>,
    awaiting_approval: bool,
}

impl<'a, F: crate::workspace::FileSystem> ToolLifecycle<'a, F> {
    pub fn new(workspace: &'a Workspace<F>, mode: Mode) -> Self {
        Self {
            workspace,
            mode,
            cancelled: false,
            pending: None,
            diff: None,
            preview: None,
            awaiting_approval: false,
        }
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    pub fn request(&mut self, request: ToolRequest) -> ToolResult {
        if self.cancelled {
            return self.cancelled_result();
        }
        match request {
            ToolRequest::Read { path, max_bytes } => match self
                .workspace
                .read(path, max_bytes.min(TOOL_READ_MAX_BYTES))
            {
                Ok(read) => ToolResult {
                    status: ToolStatus::Completed,
                    read: Some(read),
                    transaction: None,
                    preview: None,
                },
                Err(error) => self.failed(error.to_string()),
            },
            ToolRequest::Build { mutations } => {
                if mutations.iter().any(|mutation| matches!(mutation, Mutation::Write { bytes, .. } if bytes.len() > TOOL_MUTATION_MAX_BYTES)) {
                    return self.failed("workspace mutation exceeds tool byte limit".into());
                }
                self.pending = Some(mutations);
                self.diff = None;
                self.preview = None;
                self.awaiting_approval = false;
                ToolResult {
                    status: ToolStatus::Requested,
                    read: None,
                    transaction: None,
                    preview: None,
                }
            }
        }
    }
    pub fn review(&mut self) -> ToolResult {
        if self.cancelled {
            return self.cancelled_result();
        }
        let Some(mutations) = self.pending.clone() else {
            return self.failed("no pending build".into());
        };
        match self.workspace.preview(self.mode, &mutations) {
            Ok(preview) => {
                if preview
                    .decisions
                    .contains(&crate::workspace::PolicyDecision::Denied)
                {
                    self.clear_pending();
                    return self.failed("workspace mutation denied by policy".into());
                }
                let requires_approval = preview
                    .decisions
                    .contains(&crate::workspace::PolicyDecision::ApprovalRequired);
                let transaction = TransactionResult {
                    snapshot_ids: Vec::new(),
                    diffs: preview.diffs.clone(),
                };
                self.diff = Some(transaction.clone());
                self.preview = Some(preview.clone());
                if !requires_approval {
                    return match self.workspace.build(self.mode, mutations, true) {
                        Ok(applied) => {
                            self.clear_pending();
                            ToolResult {
                                status: ToolStatus::Completed,
                                read: None,
                                transaction: Some(applied),
                                preview: Some(preview),
                            }
                        }
                        Err(error) => {
                            self.clear_pending();
                            self.failed(error.to_string())
                        }
                    };
                }
                self.awaiting_approval = true;
                ToolResult {
                    status: ToolStatus::AwaitingApproval,
                    read: None,
                    transaction: Some(transaction),
                    preview: Some(preview),
                }
            }
            Err(error) => {
                self.clear_pending();
                self.failed(error.to_string())
            }
        }
    }
    pub fn approve(&mut self, approved: bool) -> ToolResult {
        if self.cancelled {
            return self.cancelled_result();
        }
        let Some(mutations) = self.pending.take() else {
            return self.failed("no pending build".into());
        };
        if !self.awaiting_approval {
            self.clear_pending();
            return self.failed("build requires review before approval".into());
        }
        if !approved {
            self.diff = None;
            self.preview = None;
            return ToolResult {
                status: ToolStatus::Completed,
                read: None,
                transaction: None,
                preview: None,
            };
        }
        match self.workspace.build(self.mode, mutations, true) {
            Ok(transaction) => {
                self.diff = None;
                self.preview = None;
                ToolResult {
                    status: ToolStatus::Completed,
                    read: None,
                    transaction: Some(transaction),
                    preview: None,
                }
            }
            Err(error) => {
                self.clear_pending();
                self.failed(error.to_string())
            }
        }
    }
    pub fn diff(&self) -> Option<&TransactionResult> {
        self.diff.as_ref()
    }
    fn cancelled_result(&mut self) -> ToolResult {
        self.clear_pending();
        ToolResult {
            status: ToolStatus::Cancelled,
            read: None,
            transaction: None,
            preview: None,
        }
    }
    fn clear_pending(&mut self) {
        self.pending = None;
        self.diff = None;
        self.preview = None;
        self.awaiting_approval = false;
    }
    fn failed(&mut self, message: String) -> ToolResult {
        self.clear_pending();
        ToolResult {
            status: ToolStatus::Failed(message),
            read: None,
            transaction: None,
            preview: None,
        }
    }
}

pub fn coding_tools_schemas() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read contents of a file in the workspace. Supports pagination by line offset and limit.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file relative to the workspace root"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "1-based line number to start reading from (default: 1)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of lines to return (default: 2000)"
                        }
                    },
                    "required": ["path"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Create a new file or completely overwrite an existing file in the workspace. Only available in BUILD mode.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file relative to the workspace root"
                        },
                        "content": {
                            "type": "string",
                            "description": "The complete text content to write"
                        }
                    },
                    "required": ["path", "content"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Perform an exact literal substring replacement in an existing file. Only available in BUILD mode.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file relative to the workspace root"
                        },
                        "old_string": {
                            "type": "string",
                            "description": "The exact literal text to replace (must match exactly once)"
                        },
                        "new_string": {
                            "type": "string",
                            "description": "The new replacement text"
                        }
                    },
                    "required": ["path", "old_string", "new_string"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_dir",
                "description": "List directory contents in the workspace.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Directory path relative to workspace root (default: '.')"
                        }
                    }
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "glob_search",
                "description": "Find files matching a glob pattern (e.g. '**/*.rs', '*.toml').",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "The search pattern"
                        },
                        "path": {
                            "type": "string",
                            "description": "Base directory to search in (default: '.')"
                        }
                    },
                    "required": ["pattern"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "grep_search",
                "description": "Search file contents for a case-insensitive query string.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The text pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "Base directory to search in (default: '.')"
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "bash",
                "description": "Execute a shell command in the workspace root. Mutating commands require BUILD mode.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command line string to run"
                        }
                    },
                    "required": ["command"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "question",
                "description": "Ask the user one or more clarifying questions when requirements are ambiguous or key choices are needed. The user will select options or provide free text.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "The clarifying question to ask the user"
                        },
                        "options": {
                            "type": "array",
                            "items": {
                                "type": "string"
                            },
                            "description": "Optional list of predefined choices for the user to select from"
                        }
                    },
                    "required": ["question"]
                }
            }
        }),
    ]
}

pub fn execute_tool(
    workspace: &Workspace<crate::workspace::RealFileSystem>,
    mode: Mode,
    name: &str,
    arguments_json: &str,
) -> Result<String, String> {
    let args: serde_json::Value = serde_json::from_str(arguments_json)
        .map_err(|e| format!("Invalid JSON arguments for tool '{name}': {e}"))?;

    match name {
        "read_file" => {
            let path_str = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'path'".to_string())?;
            let offset = args
                .get("offset")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as usize;
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(2000) as usize;

            let read_res = workspace
                .read(path_str, TOOL_READ_MAX_BYTES)
                .map_err(|e| format!("Failed to read '{path_str}': {e}"))?;
            let text = String::from_utf8_lossy(&read_res.bytes);
            let lines: Vec<&str> = text.lines().collect();
            let total_lines = lines.len();
            let start = offset.max(1);
            if start > total_lines {
                return Ok(format!(
                    "[File '{path_str}' has {total_lines} lines; offset {start} exceeds line count]"
                ));
            }
            let end = (start + limit.saturating_sub(1)).min(total_lines);
            let mut out = String::new();
            for (i, line) in lines[start - 1..end].iter().enumerate() {
                let line_no = start + i;
                out.push_str(&format!("{line_no:5} | {line}\n"));
            }
            if total_lines > end {
                out.push_str(&format!(
                    "[Lines {start}..{end} of {total_lines} shown. Use offset={} to read more]\n",
                    end + 1
                ));
            }
            Ok(out)
        }
        "write_file" => {
            if mode == Mode::Plan {
                return Err(
                    "Forbidden: 'write_file' is blocked in PLAN mode. Switch to BUILD mode (press Tab) to create or overwrite files."
                        .to_string(),
                );
            }
            let path_str = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'path'".to_string())?;
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'content'".to_string())?;

            let mutation = Mutation::Write {
                path: PathBuf::from(path_str),
                bytes: content.as_bytes().to_vec(),
            };
            workspace
                .build(mode, vec![mutation], true)
                .map_err(|e| format!("Failed to write '{path_str}': {e}"))?;
            Ok(format!(
                "Successfully wrote {} bytes to '{}'",
                content.len(),
                path_str
            ))
        }
        "edit_file" => {
            if mode == Mode::Plan {
                return Err(
                    "Forbidden: 'edit_file' is blocked in PLAN mode. Switch to BUILD mode (press Tab) to modify files."
                        .to_string(),
                );
            }
            let path_str = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'path'".to_string())?;
            let old_string = args
                .get("old_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'old_string'".to_string())?;
            let new_string = args
                .get("new_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'new_string'".to_string())?;

            if old_string == new_string {
                return Err("old_string and new_string are identical; no edit applied".to_string());
            }

            let read_res = workspace
                .read(path_str, TOOL_READ_MAX_BYTES)
                .map_err(|e| format!("Failed to read '{path_str}' for edit: {e}"))?;
            let content = String::from_utf8(read_res.bytes)
                .map_err(|_| format!("File '{path_str}' is not valid UTF-8 text"))?;

            let count = content.matches(old_string).count();
            if count == 0 {
                return Err(format!(
                    "old_string not found in '{path_str}'. Verify exact line breaks, whitespace, and surrounding context."
                ));
            }
            if count > 1 {
                return Err(format!(
                    "old_string matches {count} locations in '{path_str}'. Provide more surrounding lines to uniquely locate the target edit."
                ));
            }

            let updated = content.replacen(old_string, new_string, 1);
            let mutation = Mutation::Write {
                path: PathBuf::from(path_str),
                bytes: updated.into_bytes(),
            };
            workspace
                .build(mode, vec![mutation], true)
                .map_err(|e| format!("Failed to apply edit to '{path_str}': {e}"))?;

            let old_lines = old_string.lines().count();
            let new_lines = new_string.lines().count();
            Ok(format!(
                "Successfully edited '{}' (-{old_lines} lines, +{new_lines} lines)",
                path_str
            ))
        }
        "list_dir" => {
            let subpath = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let target_dir = if subpath == "." || subpath.is_empty() {
                workspace.root_path().to_path_buf()
            } else {
                workspace.root_path().join(subpath)
            };

            let entries = std::fs::read_dir(&target_dir)
                .map_err(|e| format!("Failed to read directory '{}': {e}", target_dir.display()))?;

            let mut dirs = Vec::new();
            let mut files = Vec::new();

            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') && name != ".claude" && name != ".clawcode" {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        dirs.push(format!("{name}/"));
                    } else {
                        let size = meta.len();
                        files.push(format!("{name} ({size} B)"));
                    }
                }
            }

            dirs.sort();
            files.sort();
            let mut result = Vec::new();
            result.extend(dirs);
            result.extend(files);

            if result.is_empty() {
                Ok("[Directory is empty]".to_string())
            } else {
                Ok(result.join("\n"))
            }
        }
        "glob_search" => {
            let pattern = args
                .get("pattern")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'pattern'".to_string())?;
            let subpath = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let target_dir = if subpath == "." || subpath.is_empty() {
                workspace.root_path().to_path_buf()
            } else {
                workspace.root_path().join(subpath)
            };

            let mut matched = Vec::new();
            walk_dir_glob(&target_dir, workspace.root_path(), pattern, &mut matched, 100);

            if matched.is_empty() {
                Ok(format!("No files matched pattern '{pattern}'"))
            } else {
                matched.sort();
                Ok(matched.join("\n"))
            }
        }
        "grep_search" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'query'".to_string())?;
            let subpath = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let target_dir = if subpath == "." || subpath.is_empty() {
                workspace.root_path().to_path_buf()
            } else {
                workspace.root_path().join(subpath)
            };

            let mut matches = Vec::new();
            walk_dir_grep(&target_dir, workspace.root_path(), query, &mut matches, 100);

            if matches.is_empty() {
                Ok(format!("No matches found for query '{query}'"))
            } else {
                Ok(matches.join("\n"))
            }
        }
        "bash" => {
            let command_str = args
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'command'".to_string())?;

            if mode == Mode::Plan {
                let decision = workspace.validate_shell(mode, ".", command_str).unwrap_or(
                    crate::workspace::PolicyDecision::ApprovalRequired,
                );
                if matches!(decision, crate::workspace::PolicyDecision::Denied) {
                    return Err(
                        "Command execution denied in PLAN mode. Switch to BUILD mode (Tab) to execute modifying commands."
                            .to_string(),
                    );
                }
            }

            let output = if cfg!(windows) {
                std::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", command_str])
                    .current_dir(workspace.root_path())
                    .output()
            } else {
                std::process::Command::new("sh")
                    .args(["-c", command_str])
                    .current_dir(workspace.root_path())
                    .output()
            };

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let status = out.status.code().unwrap_or(-1);
                    let mut res = String::new();
                    if !stdout.is_empty() {
                        res.push_str(&stdout);
                    }
                    if !stderr.is_empty() {
                        if !res.is_empty() {
                            res.push('\n');
                        }
                        res.push_str("STDERR:\n");
                        res.push_str(&stderr);
                    }
                    if status != 0 {
                        res.push_str(&format!("\n[Process exited with code {status}]"));
                    }
                    if res.is_empty() {
                        res = "[Command finished with no output]".to_string();
                    }
                    Ok(res)
                }
                Err(e) => Err(format!("Failed to execute command: {e}")),
            }
        }
        "question" => {
            let question = args
                .get("question")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'question'".to_string())?;
            let options = args.get("options").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            });
            let mut out = format!("Question asked: {question}");
            if let Some(opts) = options {
                if !opts.is_empty() {
                    out.push_str("\nOptions:\n");
                    for (i, opt) in opts.iter().enumerate() {
                        out.push_str(&format!(" {}. {}\n", i + 1, opt));
                    }
                }
            }
            Ok(out)
        }
        _ => Err(format!("Unknown tool: '{name}'")),
    }
}

fn walk_dir_glob(
    dir: &Path,
    root: &Path,
    pattern: &str,
    results: &mut Vec<String>,
    max_items: usize,
) {
    if results.len() >= max_items {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue; };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "target" || name_str == "node_modules" {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            walk_dir_glob(&path, root, pattern, results, max_items);
        } else if file_type.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if simple_pattern_match(pattern, &rel_str) {
                results.push(rel_str);
                if results.len() >= max_items {
                    return;
                }
            }
        }
    }
}

fn walk_dir_grep(
    dir: &Path,
    root: &Path,
    query: &str,
    results: &mut Vec<String>,
    max_items: usize,
) {
    if results.len() >= max_items {
        return;
    }
    let query_lower = query.to_lowercase();
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue; };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "target" || name_str == "node_modules" {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            walk_dir_grep(&path, root, query, results, max_items);
        } else if file_type.is_file() {
            let Ok(bytes) = std::fs::read(&path) else { continue; };
            if bytes.contains(&0) || bytes.len() > 1024 * 1024 {
                // skip binary or large files
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel_str = rel.to_string_lossy().replace('\\', "/");

            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&query_lower) {
                    results.push(format!("{}:{}: {}", rel_str, i + 1, line.trim()));
                    if results.len() >= max_items {
                        return;
                    }
                }
            }
        }
    }
}

fn simple_pattern_match(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim_start_matches("**/").replace('\\', "/");
    if pat == "*" || pat == "**/*" {
        return true;
    }
    if let Some(ext) = pat.strip_prefix("*.") {
        return path.ends_with(&format!(".{ext}"));
    }
    if let Some(prefix) = pat.strip_suffix('*') {
        return path.starts_with(prefix) || path.contains(prefix);
    }
    path.contains(&pat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schemas_validity() {
        let schemas = coding_tools_schemas();
        assert_eq!(schemas.len(), 8);
        let names: Vec<_> = schemas
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"list_dir"));
        assert!(names.contains(&"glob_search"));
        assert!(names.contains(&"grep_search"));
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"question"));
    }

    #[test]
    fn test_simple_pattern_match() {
        assert!(simple_pattern_match("*.rs", "src/main.rs"));
        assert!(simple_pattern_match("*.toml", "Cargo.toml"));
        assert!(simple_pattern_match("main*", "src/main.rs"));
        assert!(!simple_pattern_match("*.py", "src/main.rs"));
    }

    #[test]
    fn test_execute_tool_workflow() {
        let temp_dir = std::env::temp_dir().join(format!(
            "clawcode-execute-tool-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let workspace = Workspace::open(&temp_dir).unwrap();

        // 1. In Plan mode, write_file must be rejected
        let plan_write = execute_tool(
            &workspace,
            Mode::Plan,
            "write_file",
            r#"{"path": "test.txt", "content": "hello world"}"#,
        );
        assert!(plan_write.is_err());
        assert!(plan_write.unwrap_err().contains("PLAN mode"));

        // 2. In Build mode, write_file must succeed
        let build_write = execute_tool(
            &workspace,
            Mode::Build,
            "write_file",
            r#"{"path": "test.txt", "content": "hello world\nline 2"}"#,
        );
        assert!(build_write.is_ok());

        // 3. read_file must show line numbers
        let read_res = execute_tool(
            &workspace,
            Mode::Plan,
            "read_file",
            r#"{"path": "test.txt"}"#,
        );
        assert!(read_res.is_ok());
        let read_str = read_res.unwrap();
        assert!(read_str.contains("1 | hello world"));
        assert!(read_str.contains("2 | line 2"));

        // 4. edit_file must replace text in Build mode
        let edit_res = execute_tool(
            &workspace,
            Mode::Build,
            "edit_file",
            r#"{"path": "test.txt", "old_string": "hello world", "new_string": "hello universe"}"#,
        );
        assert!(edit_res.is_ok());

        let read_after = execute_tool(
            &workspace,
            Mode::Plan,
            "read_file",
            r#"{"path": "test.txt"}"#,
        )
        .unwrap();
        assert!(read_after.contains("hello universe"));

        // 5. list_dir
        let list_res = execute_tool(&workspace, Mode::Plan, "list_dir", r#"{}"#).unwrap();
        assert!(list_res.contains("test.txt"));

        // 6. grep_search
        let grep_res = execute_tool(
            &workspace,
            Mode::Plan,
            "grep_search",
            r#"{"query": "universe"}"#,
        )
        .unwrap();
        assert!(grep_res.contains("test.txt:1: hello universe"));

        // 7. question tool
        let question_res = execute_tool(
            &workspace,
            Mode::Plan,
            "question",
            r#"{"question": "Choose database", "options": ["postgres", "sqlite"]}"#,
        )
        .unwrap();
        assert!(question_res.contains("Question asked: Choose database"));
        assert!(question_res.contains("1. postgres"));
        assert!(question_res.contains("2. sqlite"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
