use crate::workspace::{Mode, Mutation, TransactionResult, Workspace, WorkspacePreview};
use sha2::{Digest, Sha256};
use std::io::Read;
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LineSelector {
    Range(usize, usize),
    From(usize),
    OffsetCount(usize, usize),
    Last(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedReadPath<'a> {
    pub path: &'a str,
    pub is_raw: bool,
    pub selector: Option<LineSelector>,
}

pub fn parse_line_selector(tag: &str) -> Option<LineSelector> {
    let tag = tag.trim();
    if let Some(num_str) = tag.strip_prefix('-') {
        let num_str = num_str.trim();
        if !num_str.is_empty() && num_str.chars().all(|c| c.is_ascii_digit()) {
            return num_str.parse::<usize>().ok().map(LineSelector::Last);
        }
        return None;
    }
    if let Some((start_str, count_str)) = tag.split_once('+') {
        let (start_str, count_str) = (start_str.trim(), count_str.trim());
        if !start_str.is_empty()
            && start_str.chars().all(|c| c.is_ascii_digit())
            && !count_str.is_empty()
            && count_str.chars().all(|c| c.is_ascii_digit())
        {
            let start = start_str.parse::<usize>().ok()?;
            let count = count_str.parse::<usize>().ok()?;
            return Some(LineSelector::OffsetCount(start, count));
        }
        return None;
    }
    if let Some(num_str) = tag.strip_suffix('-') {
        let num_str = num_str.trim();
        if !num_str.is_empty() && num_str.chars().all(|c| c.is_ascii_digit()) {
            return num_str.parse::<usize>().ok().map(LineSelector::From);
        }
        return None;
    }
    if let Some((start_str, end_str)) = tag.split_once('-') {
        let (start_str, end_str) = (start_str.trim(), end_str.trim());
        if !start_str.is_empty()
            && start_str.chars().all(|c| c.is_ascii_digit())
            && !end_str.is_empty()
            && end_str.chars().all(|c| c.is_ascii_digit())
        {
            let start = start_str.parse::<usize>().ok()?;
            let end = end_str.parse::<usize>().ok()?;
            return Some(LineSelector::Range(start, end));
        }
        return None;
    }
    if !tag.is_empty() && tag.chars().all(|c| c.is_ascii_digit()) {
        return tag.parse::<usize>().ok().map(LineSelector::From);
    }
    None
}

pub fn parse_read_path(input: &str) -> ParsedReadPath<'_> {
    let mut remaining = input;
    let mut is_raw = false;
    let mut selector = None;

    loop {
        let min_idx = if remaining.len() >= 2
            && remaining.as_bytes()[0].is_ascii_alphabetic()
            && remaining.as_bytes()[1] == b':'
        {
            2
        } else {
            0
        };

        let Some(colon_offset) = remaining[min_idx..].rfind(':') else {
            break;
        };
        let colon_pos = min_idx + colon_offset;
        let tag = &remaining[colon_pos + 1..];

        if tag.trim().eq_ignore_ascii_case("raw") {
            is_raw = true;
            remaining = &remaining[..colon_pos];
        } else if let Some(sel) = parse_line_selector(tag) {
            if selector.is_none() {
                selector = Some(sel);
            }
            remaining = &remaining[..colon_pos];
        } else {
            break;
        }
    }

    ParsedReadPath {
        path: remaining,
        is_raw,
        selector,
    }
}

pub fn coding_tools_schemas() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read contents of a file in the workspace. Supports pagination by line offset and limit. Supports inline selectors: path:50-200, path:50+30, path:-40 (last 40 lines), path:raw.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file relative to the workspace root. Supports inline selectors: path:50-200, path:50+30, path:-40 (last 40 lines), path:raw."
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
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "update_plan",
                "description": "Update the structured multi-step execution plan checklist. Always call this before starting a multi-step task and as milestones are reached. Allowed status: 'pending', 'in_progress', 'completed'.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "explanation": {
                            "type": "string",
                            "description": "Optional explanation for why the plan is being updated"
                        },
                        "plan": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "step": {
                                        "type": "string",
                                        "description": "Description of the plan step"
                                    },
                                    "status": {
                                        "type": "string",
                                        "description": "Status of the step: pending, in_progress, completed"
                                    }
                                },
                                "required": ["step", "status"]
                            },
                            "description": "The list of plan steps"
                        }
                    },
                    "required": ["plan"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "webfetch",
                "description": "Fetch web page content by URL and return readable text or HTML/markdown.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to fetch (must begin with http:// or https://)"
                        }
                    },
                    "required": ["url"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "websearch",
                "description": "Search the web for up-to-date documentation, API references, library examples, or error solutions.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query keywords"
                        }
                    },
                    "required": ["query"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "skill",
                "description": "Load a specialized domain skill or instruction rulebook by name from the skills/ directory (e.g. 'best-practices', 'testing', 'refactor').",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "The name of the skill to load"
                        }
                    },
                    "required": ["name"]
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

            let parsed = parse_read_path(path_str);
            let clean_path = parsed.path;
            let is_raw = parsed.is_raw;

            let explicit_offset = args.get("offset").and_then(|v| v.as_u64()).map(|v| v as usize);
            let explicit_limit = args.get("limit").and_then(|v| v.as_u64()).map(|v| v as usize);

            let read_res = workspace
                .read(clean_path, TOOL_READ_MAX_BYTES)
                .map_err(|e| format!("Failed to read '{clean_path}': {e}"))?;

            let digest = Sha256::digest(&read_res.bytes);
            let hash = format!("{:02X}{:02X}", digest[0], digest[1]);

            let text = String::from_utf8_lossy(&read_res.bytes);
            let lines: Vec<&str> = text.lines().collect();
            let total_lines = lines.len();

            if total_lines == 0 {
                if is_raw {
                    return Ok(String::new());
                } else {
                    return Ok(format!("[{clean_path}#{hash}] (lines 0..0 of 0)\n"));
                }
            }

            let (start, end) = match &parsed.selector {
                Some(LineSelector::Range(s, e)) => {
                    let start = (*s).max(1);
                    let end = (*e).min(total_lines).max(start);
                    (start, end)
                }
                Some(LineSelector::From(s)) => {
                    let start = (*s).max(1);
                    (start, total_lines)
                }
                Some(LineSelector::OffsetCount(s, count)) => {
                    let start = (*s).max(1);
                    if *count == 0 {
                        (start, start.saturating_sub(1))
                    } else {
                        let end = (start + count.saturating_sub(1)).min(total_lines);
                        (start, end)
                    }
                }
                Some(LineSelector::Last(count)) => {
                    if *count == 0 {
                        (total_lines + 1, total_lines)
                    } else {
                        let start = total_lines.saturating_sub(*count).saturating_add(1).max(1);
                        (start, total_lines)
                    }
                }
                None => {
                    let start = explicit_offset.unwrap_or(1).max(1);
                    let limit = explicit_limit.unwrap_or(if is_raw { usize::MAX } else { 2000 });
                    let end = (start + limit.saturating_sub(1)).min(total_lines);
                    (start, end)
                }
            };

            if start > total_lines || start > end {
                if is_raw {
                    return Ok(String::new());
                } else {
                    return Ok(format!(
                        "[File '{clean_path}' has {total_lines} lines; offset {start} exceeds line count]"
                    ));
                }
            }

            let end = end.min(total_lines);

            if is_raw {
                let mut out = String::new();
                for line in &lines[start - 1..end] {
                    out.push_str(line);
                    out.push('\n');
                }
                Ok(out)
            } else {
                let mut out = format!("[{clean_path}#{hash}] (lines {start}..{end} of {total_lines})\n");
                for (i, line) in lines[start - 1..end].iter().enumerate() {
                    let line_no = start + i;
                    out.push_str(&format!("  {line_no}: {line}\n"));
                }
                Ok(out)
            }
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
        "update_plan" => {
            let plan_val = args
                .get("plan")
                .ok_or_else(|| "Missing required argument 'plan'".to_string())?;

            let plan_array = plan_val
                .as_array()
                .ok_or_else(|| "Argument 'plan' must be an array of objects".to_string())?;

            if plan_array.is_empty() {
                return Err("Plan must contain at least one step".to_string());
            }

            let mut completed = 0;
            let mut in_progress = 0;
            let mut pending = 0;

            for (idx, item) in plan_array.iter().enumerate() {
                let obj = item.as_object().ok_or_else(|| {
                    format!("Plan step at index {idx} must be an object with 'step' and 'status'")
                })?;

                let step = obj
                    .get("step")
                    .or_else(|| obj.get("content"))
                    .or_else(|| obj.get("title"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        format!("Plan step at index {idx} is missing or has empty 'step'")
                    })?;

                let raw_status = obj
                    .get("status")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        format!("Plan step at index {idx} is missing 'status'")
                    })?;

                let norm_status = match raw_status.trim().to_ascii_lowercase().as_str() {
                    "todo" | "open" | "pending" | "not_started" | "not-started" => "pending",
                    "in_progress" | "in-progress" | "in progress" | "doing" | "active" => "in_progress",
                    "done" | "completed" | "complete" => "completed",
                    other => {
                        return Err(format!(
                            "Invalid status '{other}' for step '{step}'. Allowed status: 'pending', 'in_progress', 'completed'"
                        ));
                    }
                };

                match norm_status {
                    "completed" => completed += 1,
                    "in_progress" => in_progress += 1,
                    _ => pending += 1,
                }
            }

            let total = plan_array.len();
            Ok(format!(
                "Plan updated: {total} steps ({completed} completed, {in_progress} in progress, {pending} pending)"
            ))
        }
        "webfetch" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'url'".to_string())?;

            execute_webfetch(url)
        }
        "websearch" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'query'".to_string())?;

            execute_websearch(query)
        }
        "skill" => {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'name'".to_string())?;

            execute_skill(workspace, name)
        }
        _ => Err(format!("Unknown tool: '{name}'")),
    }
}

pub fn execute_webfetch(url: &str) -> Result<String, String> {
    let trimmed_url = url.trim();
    if !trimmed_url.starts_with("http://") && !trimmed_url.starts_with("https://") {
        return Err(format!(
            "Invalid URL '{url}': URL must begin with 'http://' or 'https://'"
        ));
    }

    let resp = ureq::get(trimmed_url)
        .timeout(std::time::Duration::from_secs(12))
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .call()
        .map_err(|e| format!("Webfetch failed for '{trimmed_url}': {e}"))?;

    let content_type = resp.header("content-type").unwrap_or("").to_lowercase();
    let mut body = String::new();
    resp.into_reader()
        .take(5 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    let is_html = content_type.contains("text/html")
        || content_type.contains("application/xhtml+xml")
        || (content_type.is_empty()
            && (body.contains("<html") || body.contains("<body") || body.contains("<!DOCTYPE")));

    let text = if is_html {
        clean_html_to_text(&body)
    } else {
        normalize_text(&body, 50_000)
    };

    if text.trim().is_empty() {
        Ok("(Empty page content)".to_string())
    } else {
        Ok(text)
    }
}

pub fn clean_html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len().min(50_000));
    let mut chars = html.chars().peekable();
    let mut skip_until_tag: Option<String> = None;

    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag_content = String::new();
            while let Some(&next_ch) = chars.peek() {
                chars.next();
                if next_ch == '>' {
                    break;
                }
                tag_content.push(next_ch);
            }

            let trimmed_tag = tag_content.trim();
            if trimmed_tag.starts_with("!--") {
                if !trimmed_tag.ends_with("--") {
                    while let Some(c) = chars.next() {
                        if c == '-' && chars.peek() == Some(&'-') {
                            chars.next();
                            if chars.peek() == Some(&'>') {
                                chars.next();
                                break;
                            }
                        }
                    }
                }
                continue;
            }

            let is_closing = trimmed_tag.starts_with('/');
            let tag_name = if is_closing {
                trimmed_tag[1..].split_whitespace().next().unwrap_or("")
            } else {
                trimmed_tag.split_whitespace().next().unwrap_or("")
            }
            .to_ascii_lowercase();

            if let Some(skip) = &skip_until_tag {
                if is_closing && tag_name == *skip {
                    skip_until_tag = None;
                }
                continue;
            }

            if !is_closing
                && matches!(
                    tag_name.as_str(),
                    "script" | "style" | "noscript" | "svg" | "head" | "iframe"
                )
            {
                if !trimmed_tag.ends_with('/') {
                    skip_until_tag = Some(tag_name);
                }
                continue;
            }

            if is_closing {
                if is_block_element(&tag_name) {
                    ensure_newline(&mut out);
                }
            } else {
                if is_block_element(&tag_name) {
                    ensure_newline(&mut out);
                }
                if tag_name == "li" {
                    out.push_str("- ");
                } else if tag_name == "br" {
                    out.push('\n');
                }
            }
            continue;
        }

        if skip_until_tag.is_some() {
            continue;
        }

        if ch == '&' {
            let mut entity = String::new();
            let mut found_semi = false;
            while let Some(&next_ch) = chars.peek() {
                if next_ch == ';' {
                    chars.next();
                    found_semi = true;
                    break;
                }
                if next_ch.is_alphanumeric() || next_ch == '#' {
                    entity.push(next_ch);
                    chars.next();
                } else {
                    break;
                }
            }
            if found_semi {
                let decoded = decode_entity(&entity);
                out.push(decoded);
            } else {
                out.push('&');
                out.push_str(&entity);
            }
            continue;
        }

        if ch == '\r' {
            continue;
        }
        if ch == '\t' || ch == ' ' {
            if !out.ends_with(' ') && !out.ends_with('\n') {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }

    normalize_text(&out, 50_000)
}

fn ensure_newline(s: &mut String) {
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
}

fn decode_entity(entity: &str) -> char {
    match entity {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "copy" => '©',
        "reg" => '®',
        _ => {
            if entity.starts_with("#x") || entity.starts_with("#X") {
                u32::from_str_radix(&entity[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or('?')
            } else if entity.starts_with('#') {
                entity[1..]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or('?')
            } else {
                ' '
            }
        }
    }
}

fn is_block_element(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "li"
            | "ul"
            | "ol"
            | "tr"
            | "table"
            | "blockquote"
            | "pre"
            | "article"
            | "section"
            | "header"
            | "footer"
            | "nav"
            | "aside"
            | "main"
            | "hr"
    )
}

fn normalize_text(text: &str, max_chars: usize) -> String {
    let mut result = String::with_capacity(text.len());
    let mut consecutive_newlines = 0;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if consecutive_newlines < 2 {
                result.push('\n');
                consecutive_newlines += 1;
            }
        } else {
            result.push_str(trimmed);
            result.push('\n');
            consecutive_newlines = 0;
        }
    }

    let trimmed = result.trim();
    if trimmed.chars().count() > max_chars {
        let truncated: String = trimmed.chars().take(max_chars).collect();
        format!("{truncated}\n\n[Content truncated at {max_chars} characters]")
    } else {
        trimmed.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub fn execute_websearch(query: &str) -> Result<String, String> {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        return Err("Search query cannot be empty".to_string());
    }

    let html_endpoint = std::env::var("CLAWCODE_DDG_SEARCH_URL")
        .unwrap_or_else(|_| "https://html.duckduckgo.com/html/?q=".to_string());
    let api_endpoint = std::env::var("CLAWCODE_DDG_API_URL")
        .unwrap_or_else(|_| "https://api.duckduckgo.com/?format=json&no_html=1&q=".to_string());

    let mut last_net_err = None;

    // 1. Try DuckDuckGo HTML search
    let search_url = format!("{html_endpoint}{}", url_encode(trimmed_query));
    let html_res = ureq::get(&search_url)
        .timeout(std::time::Duration::from_secs(12))
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .call();

    match html_res {
        Ok(resp) => {
            let mut body = String::new();
            if resp
                .into_reader()
                .take(2 * 1024 * 1024)
                .read_to_string(&mut body)
                .is_ok()
            {
                if !body.contains("anomaly-modal") && !body.contains("anomaly.js") {
                    let items = parse_ddg_html(&body);
                    if !items.is_empty() {
                        return Ok(format_search_results(trimmed_query, &items));
                    }
                }
            }
        }
        Err(e) => {
            last_net_err = Some(e.to_string());
        }
    }

    // 2. Fallback to DuckDuckGo Instant Answer API
    let api_url = format!("{api_endpoint}{}", url_encode(trimmed_query));
    let api_res = ureq::get(&api_url)
        .timeout(std::time::Duration::from_secs(12))
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .call();

    match api_res {
        Ok(resp) => {
            let mut body = String::new();
            if resp
                .into_reader()
                .take(2 * 1024 * 1024)
                .read_to_string(&mut body)
                .is_ok()
            {
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&body) {
                    let items = parse_instant_answer_json(&json_val);
                    if !items.is_empty() {
                        return Ok(format_search_results(trimmed_query, &items));
                    }
                }
            }
        }
        Err(e) => {
            if last_net_err.is_none() {
                last_net_err = Some(e.to_string());
            }
        }
    }

    if let Some(err) = last_net_err {
        return Err(format!(
            "Web search failed: network unreachable or request error: {err}"
        ));
    }

    Ok(format!("No search results found for query \"{trimmed_query}\"."))
}

pub fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 3);
    for b in input.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

pub fn percent_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = s.as_bytes().iter().copied();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(h1), Some(h2)) = (h1, h2) {
                if let (Some(d1), Some(d2)) = (
                    (h1 as char).to_digit(16),
                    (h2 as char).to_digit(16),
                ) {
                    bytes.push(((d1 << 4) | d2) as u8);
                    continue;
                }
            }
            bytes.push(b'%');
        } else if b == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}

pub fn decode_ddg_url(raw_url: &str) -> String {
    if let Some(idx) = raw_url.find("uddg=") {
        let after = &raw_url[idx + 5..];
        let end_idx = after.find('&').unwrap_or(after.len());
        let encoded_url = &after[..end_idx];
        percent_decode(encoded_url)
    } else if raw_url.starts_with("//") {
        format!("https:{raw_url}")
    } else {
        raw_url.to_string()
    }
}

pub fn parse_ddg_html(html: &str) -> Vec<SearchItem> {
    let mut items = Vec::new();

    let parts: Vec<&str> = if html.contains("result__body") {
        html.split("result__body").collect()
    } else if html.contains("web-result") {
        html.split("web-result").collect()
    } else if html.contains("<div class=\"result ") {
        html.split("<div class=\"result ").collect()
    } else {
        Vec::new()
    };

    for part in parts.iter().skip(1) {
        let (url, title) = extract_title_and_url_from_block(part);
        let snippet = extract_snippet_from_block(part);

        if !title.is_empty() && !url.is_empty() {
            items.push(SearchItem {
                title,
                url: decode_ddg_url(&url),
                snippet,
            });
        }
    }

    if items.is_empty() && (html.contains("result-link") || html.contains("result__snippet")) {
        let parts: Vec<&str> = html.split("<tr").collect();
        let mut cur_title = String::new();
        let mut cur_url = String::new();

        for part in parts {
            if part.contains("result-link") {
                if let Some((u, t)) = extract_link_and_text(part, "result-link") {
                    cur_url = decode_ddg_url(&u);
                    cur_title = t;
                }
            } else if (part.contains("result-snippet") || part.contains("result__snippet"))
                && !cur_title.is_empty()
            {
                let snippet = clean_html_to_text(part);
                items.push(SearchItem {
                    title: std::mem::take(&mut cur_title),
                    url: std::mem::take(&mut cur_url),
                    snippet,
                });
            }
        }
    }

    items
}

fn extract_title_and_url_from_block(block: &str) -> (String, String) {
    let search_slice = if let Some(idx) = block.find("result__title") {
        &block[idx..]
    } else if let Some(idx) = block.find("result__url") {
        &block[idx..]
    } else {
        block
    };

    if let Some(a_idx) = search_slice.find("<a") {
        let after_a = &search_slice[a_idx..];
        if let Some(href) = extract_attr_val(after_a, "href") {
            if let Some(end_tag) = after_a.find('>') {
                let after_tag = &after_a[end_tag + 1..];
                if let Some(close_a) = after_tag.find("</a>") {
                    let raw_title = &after_tag[..close_a];
                    let clean_title = clean_html_to_text(raw_title);
                    return (href, clean_title);
                }
            }
        }
    }
    (String::new(), String::new())
}

fn extract_snippet_from_block(block: &str) -> String {
    if let Some(idx) = block.find("result__snippet") {
        let after = &block[idx..];
        if let Some(start_tag) = after.find('>') {
            let after_start = &after[start_tag + 1..];
            let end_idx = after_start
                .find("</a")
                .or_else(|| after_start.find("</div"))
                .unwrap_or(after_start.len());
            return clean_html_to_text(&after_start[..end_idx]);
        }
    }
    String::new()
}

fn extract_link_and_text(slice: &str, marker: &str) -> Option<(String, String)> {
    if let Some(m_idx) = slice.find(marker) {
        let before_or_at = &slice[..m_idx];
        let a_start = before_or_at.rfind("<a")?;
        let after_a = &slice[a_start..];
        let href = extract_attr_val(after_a, "href")?;
        let close_open_tag = after_a.find('>')?;
        let after_open = &after_a[close_open_tag + 1..];
        let close_a = after_open.find("</a>")?;
        let text = clean_html_to_text(&after_open[..close_a]);
        return Some((href, text));
    }
    None
}

fn extract_attr_val(slice: &str, attr_name: &str) -> Option<String> {
    let target = format!("{attr_name}=\"");
    if let Some(idx) = slice.find(&target) {
        let after = &slice[idx + target.len()..];
        if let Some(end_quote) = after.find('"') {
            return Some(after[..end_quote].to_string());
        }
    }
    let target_single = format!("{attr_name}='");
    if let Some(idx) = slice.find(&target_single) {
        let after = &slice[idx + target_single.len()..];
        if let Some(end_quote) = after.find('\'') {
            return Some(after[..end_quote].to_string());
        }
    }
    None
}

pub fn parse_instant_answer_json(json_val: &serde_json::Value) -> Vec<SearchItem> {
    let mut results = Vec::new();

    let heading = json_val.get("Heading").and_then(|v| v.as_str()).unwrap_or("");
    let abstract_text = json_val
        .get("AbstractText")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let abstract_url = json_val
        .get("AbstractURL")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !abstract_text.is_empty() || !abstract_url.is_empty() {
        results.push(SearchItem {
            title: if heading.is_empty() {
                "Instant Answer".to_string()
            } else {
                heading.to_string()
            },
            url: abstract_url.to_string(),
            snippet: abstract_text.to_string(),
        });
    }

    if let Some(topics) = json_val.get("RelatedTopics").and_then(|v| v.as_array()) {
        for topic in topics {
            if let Some(sub_topics) = topic.get("Topics").and_then(|v| v.as_array()) {
                for sub in sub_topics {
                    if let Some(item) = extract_topic_item(sub) {
                        results.push(item);
                    }
                }
            } else if let Some(item) = extract_topic_item(topic) {
                results.push(item);
            }
        }
    }

    results
}

fn extract_topic_item(topic: &serde_json::Value) -> Option<SearchItem> {
    let text = topic.get("Text").and_then(|v| v.as_str())?;
    let url = topic.get("FirstURL").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() && url.is_empty() {
        return None;
    }
    let (title, snippet) = if let Some((t, s)) = text.split_once(" - ") {
        (t.to_string(), s.to_string())
    } else {
        (text.to_string(), String::new())
    };
    Some(SearchItem {
        title,
        url: url.to_string(),
        snippet,
    })
}

pub fn format_search_results(query: &str, items: &[SearchItem]) -> String {
    if items.is_empty() {
        return format!("No search results found for query \"{query}\".");
    }
    let mut out = format!(
        "Search results for \"{query}\" ({} results):\n\n",
        items.len()
    );
    for (i, item) in items.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, item.title));
        if !item.url.is_empty() {
            out.push_str(&format!("   URL: {}\n", item.url));
        }
        if !item.snippet.is_empty() {
            out.push_str(&format!("   {}\n", item.snippet));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

pub fn execute_skill(
    workspace: &Workspace<crate::workspace::RealFileSystem>,
    name: &str,
) -> Result<String, String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err("Missing required argument 'name'".to_string());
    }

    let ws_root = workspace.root_path();
    let candidates = [
        ws_root.join("skills").join(trimmed_name).join("SKILL.md"),
        ws_root.join("skills").join(format!("{trimmed_name}.md")),
        ws_root
            .join("crabcode")
            .join("skills")
            .join(trimmed_name)
            .join("SKILL.md"),
        PathBuf::from("skills").join(trimmed_name).join("SKILL.md"),
        PathBuf::from("skills").join(format!("{trimmed_name}.md")),
        PathBuf::from("crabcode")
            .join("skills")
            .join(trimmed_name)
            .join("SKILL.md"),
    ];

    for candidate in &candidates {
        if candidate.is_file() {
            if let Ok(content) = std::fs::read_to_string(candidate) {
                return Ok(format!(
                    "<skill_content name=\"{trimmed_name}\">\n{}\n</skill_content>",
                    content.trim()
                ));
            }
        }
    }

    let available = list_available_skills(ws_root);
    if available.is_empty() {
        Err(format!(
            "Skill \"{trimmed_name}\" not found. No skills are currently available."
        ))
    } else {
        Err(format!(
            "Skill \"{trimmed_name}\" not found. Available skills: {}",
            available.join(", ")
        ))
    }
}

pub fn list_available_skills(ws_root: &Path) -> Vec<String> {
    let mut skills = Vec::new();
    let check_dirs = [
        ws_root.join("skills"),
        ws_root.join("crabcode").join("skills"),
        PathBuf::from("skills"),
        PathBuf::from("crabcode").join("skills"),
    ];

    for dir in &check_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.join("SKILL.md").is_file() {
                        if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                            skills.push(dir_name.to_string());
                        }
                    }
                } else if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if ext.eq_ignore_ascii_case("md") {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                if !stem.eq_ignore_ascii_case("SKILL")
                                    && !stem.eq_ignore_ascii_case("README")
                                {
                                    skills.push(stem.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    skills.sort();
    skills.dedup();
    skills
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
        assert_eq!(schemas.len(), 12);
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
        assert!(names.contains(&"update_plan"));
        assert!(names.contains(&"webfetch"));
        assert!(names.contains(&"websearch"));
        assert!(names.contains(&"skill"));
    }

    #[test]
    fn test_clean_html_to_text() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Ignored</title>
                <style>body { color: red; }</style>
                <script>alert("hello");</script>
            </head>
            <body>
                <h1>Welcome &amp; Hello</h1>
                <p>Paragraph with <a href="https://example.com">link</a> and &quot;quotes&quot;.</p>
                <ul>
                    <li>Item 1</li>
                    <li>Item 2</li>
                </ul>
            </body>
            </html>
        "#;
        let text = clean_html_to_text(html);
        assert!(!text.contains("alert"));
        assert!(!text.contains("color: red"));
        assert!(text.contains("Welcome & Hello"));
        assert!(text.contains("Paragraph with link and \"quotes\"."));
        assert!(text.contains("- Item 1"));
        assert!(text.contains("- Item 2"));
    }

    #[test]
    fn test_url_encode_and_decode() {
        let query = "rust ureq client+server";
        let encoded = url_encode(query);
        assert_eq!(encoded, "rust+ureq+client%2Bserver");

        let ddg_link = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fcrates.io%2Fcrates%2Fureq&rut=123";
        let decoded = decode_ddg_url(ddg_link);
        assert_eq!(decoded, "https://crates.io/crates/ureq");
    }

    #[test]
    fn test_parse_instant_answer_json() {
        let json_val = serde_json::json!({
            "Heading": "Rust (programming language)",
            "AbstractText": "Rust is a multi-paradigm, general-purpose programming language.",
            "AbstractURL": "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "RelatedTopics": [
                {
                    "FirstURL": "https://duckduckgo.com/Cargo",
                    "Text": "Cargo - The Rust package manager."
                }
            ]
        });
        let items = parse_instant_answer_json(&json_val);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Rust (programming language)");
        assert_eq!(items[0].url, "https://en.wikipedia.org/wiki/Rust_(programming_language)");
        assert_eq!(items[1].title, "Cargo");
        assert_eq!(items[1].snippet, "The Rust package manager.");
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
        assert!(read_str.contains("1: hello world"));
        assert!(read_str.contains("2: line 2"));
        assert!(read_str.contains("[test.txt#"));

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

    #[test]
    fn test_parse_read_path_selectors() {
        let p1 = parse_read_path("file.rs:2-4");
        assert_eq!(p1.path, "file.rs");
        assert_eq!(p1.selector, Some(LineSelector::Range(2, 4)));
        assert!(!p1.is_raw);

        let p2 = parse_read_path("file.rs:3+2");
        assert_eq!(p2.path, "file.rs");
        assert_eq!(p2.selector, Some(LineSelector::OffsetCount(3, 2)));
        assert!(!p2.is_raw);

        let p3 = parse_read_path("file.rs:-2");
        assert_eq!(p3.path, "file.rs");
        assert_eq!(p3.selector, Some(LineSelector::Last(2)));
        assert!(!p3.is_raw);

        let p4 = parse_read_path("file.rs:raw");
        assert_eq!(p4.path, "file.rs");
        assert_eq!(p4.selector, None);
        assert!(p4.is_raw);

        let p5 = parse_read_path("file.rs:50");
        assert_eq!(p5.path, "file.rs");
        assert_eq!(p5.selector, Some(LineSelector::From(50)));

        let p6 = parse_read_path("file.rs:50-");
        assert_eq!(p6.path, "file.rs");
        assert_eq!(p6.selector, Some(LineSelector::From(50)));

        let p7 = parse_read_path(r"C:\test\file.rs:10-20");
        assert_eq!(p7.path, r"C:\test\file.rs");
        assert_eq!(p7.selector, Some(LineSelector::Range(10, 20)));
        assert!(!p7.is_raw);

        let p8 = parse_read_path(r"C:\test\file.rs");
        assert_eq!(p8.path, r"C:\test\file.rs");
        assert_eq!(p8.selector, None);

        let p9 = parse_read_path("file.rs:2-4:raw");
        assert_eq!(p9.path, "file.rs");
        assert_eq!(p9.selector, Some(LineSelector::Range(2, 4)));
        assert!(p9.is_raw);
    }

    #[test]
    fn test_execute_tool_read_file_selectors() {
        let temp_dir = std::env::temp_dir().join(format!(
            "clawcode-read-sel-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let workspace = Workspace::open(&temp_dir).unwrap();

        let content = "line 1\nline 2\nline 3\nline 4\nline 5";
        let args = serde_json::json!({ "path": "lines.txt", "content": content }).to_string();
        let _ = execute_tool(
            &workspace,
            Mode::Build,
            "write_file",
            &args,
        )
        .unwrap();

        // 1. path:2-4
        let res_range = execute_tool(
            &workspace,
            Mode::Plan,
            "read_file",
            r#"{"path": "lines.txt:2-4"}"#,
        )
        .unwrap();
        assert!(res_range.contains("[lines.txt#"));
        assert!(res_range.contains("(lines 2..4 of 5)"));
        assert!(res_range.contains("  2: line 2"));
        assert!(res_range.contains("  3: line 3"));
        assert!(res_range.contains("  4: line 4"));
        assert!(!res_range.contains("line 1"));
        assert!(!res_range.contains("line 5"));

        // 2. path:3+2
        let res_plus = execute_tool(
            &workspace,
            Mode::Plan,
            "read_file",
            r#"{"path": "lines.txt:3+2"}"#,
        )
        .unwrap();
        assert!(res_plus.contains("(lines 3..4 of 5)"));
        assert!(res_plus.contains("  3: line 3"));
        assert!(res_plus.contains("  4: line 4"));
        assert!(!res_plus.contains("line 2"));
        assert!(!res_plus.contains("line 5"));

        // 3. path:-2 (last 2 lines)
        let res_last = execute_tool(
            &workspace,
            Mode::Plan,
            "read_file",
            r#"{"path": "lines.txt:-2"}"#,
        )
        .unwrap();
        assert!(res_last.contains("(lines 4..5 of 5)"));
        assert!(res_last.contains("  4: line 4"));
        assert!(res_last.contains("  5: line 5"));
        assert!(!res_last.contains("line 3"));

        // 4. path:raw
        let res_raw = execute_tool(
            &workspace,
            Mode::Plan,
            "read_file",
            r#"{"path": "lines.txt:raw"}"#,
        )
        .unwrap();
        assert_eq!(res_raw, "line 1\nline 2\nline 3\nline 4\nline 5\n");
        assert!(!res_raw.contains('['));
        assert!(!res_raw.contains(':'));

        // 5. Normal path + offset/limit
        let res_offset_limit = execute_tool(
            &workspace,
            Mode::Plan,
            "read_file",
            r#"{"path": "lines.txt", "offset": 2, "limit": 2}"#,
        )
        .unwrap();
        assert!(res_offset_limit.contains("(lines 2..3 of 5)"));
        assert!(res_offset_limit.contains("  2: line 2"));
        assert!(res_offset_limit.contains("  3: line 3"));
        assert!(!res_offset_limit.contains("line 1"));
        assert!(!res_offset_limit.contains("line 4"));

        // 6. Windows drive letter test
        let parsed_win = parse_read_path(r"C:\test\file.rs:10-20");
        assert_eq!(parsed_win.path, r"C:\test\file.rs");
        assert_eq!(parsed_win.selector, Some(LineSelector::Range(10, 20)));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
