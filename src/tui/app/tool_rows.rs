use ratatui::layout::Rect;

use super::util::bounded;
use super::{
    App, MAX_IDENTITY_BYTES, MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_OUTPUT_BYTES, MAX_TOOL_ROWS,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ToolRowState {
    Pending,
    Running,
    Completed,
    Failed,
}

pub(crate) struct ToolRowUpdate<'a> {
    pub call_id: &'a str,
    pub name: &'a str,
    pub state: ToolRowState,
    pub desc: String,
    pub arguments: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRow {
    pub call_id: String,
    pub name: String,
    pub desc: String,
    pub arguments: String,
    pub output: String,
    pub state: ToolRowState,
    pub arguments_complete: bool,
    pub metadata: Option<serde_json::Value>,
    pub started_at: std::time::Instant,
    pub expandable: bool,
}

impl ToolRow {
    pub fn compute_expandable(&self) -> bool {
        if matches!(self.name.as_str(), "bash" | "sh") {
            self.output.lines().count() > 10
        } else if matches!(self.name.as_str(), "edit_file" | "edit") {
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(&self.arguments)
                && let Some(old_str) = args
                    .get("old_string")
                    .or_else(|| args.get("old_str"))
                    .and_then(|v| v.as_str())
                && let Some(new_str) = args
                    .get("new_string")
                    .or_else(|| args.get("new_str"))
                    .and_then(|v| v.as_str())
            {
                let diff = crate::tui::diff::compute_diff(old_str, new_str, 20);
                diff.lines.len() > 10
            } else {
                false
            }
        } else if matches!(self.name.as_str(), "patch" | "apply_patch") {
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(&self.arguments)
                && let Some(patch_str) = args.get("patch").and_then(|v| v.as_str())
            {
                patch_str
                    .lines()
                    .filter(|l| l.starts_with(['+', '-', ' ']))
                    .count()
                    > 10
            } else {
                false
            }
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveToolInfo {
    pub name: String,
    pub desc: String,
    pub started_at: std::time::Instant,
}

pub(crate) fn tool_names_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    matches!(
        (a, b),
        ("grep_search" | "grep", "grep_search" | "grep")
            | ("glob_search" | "glob", "glob_search" | "glob")
            | ("read_file" | "read", "read_file" | "read")
            | ("write_file" | "write", "write_file" | "write")
            | (
                "edit_file" | "edit" | "patch" | "apply_patch",
                "edit_file" | "edit" | "patch" | "apply_patch"
            )
            | ("bash" | "sh", "bash" | "sh")
    )
}

pub fn tool_target_and_verbs(
    name: &str,
    args: Option<&serde_json::Value>,
) -> (&'static str, &'static str, String) {
    let parsed_args_holder: Option<serde_json::Value> = match args {
        Some(serde_json::Value::String(s)) => serde_json::from_str(s).ok(),
        _ => None,
    };
    let effective_args = parsed_args_holder.as_ref().or(args);
    let desc = match (name, effective_args) {
        (
            "read_file" | "read" | "write_file" | "write" | "edit_file" | "edit" | "patch"
            | "apply_patch",
            Some(a),
        ) => a
            .get("path")
            .or_else(|| a.get("file_path"))
            .or_else(|| a.get("filePath"))
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        ("bash" | "sh", Some(a)) => a
            .get("command")
            .or_else(|| a.get("cmd"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        ("question", Some(a)) => a
            .get("question")
            .and_then(|q| q.as_str())
            .unwrap_or("")
            .to_string(),
        ("update_plan", Some(a)) => {
            if let Some(exp) = a
                .get("explanation")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                exp.to_string()
            } else if let Some(plan) = a.get("plan").and_then(|v| v.as_array()) {
                format!("{} steps", plan.len())
            } else {
                String::new()
            }
        }
        ("glob_search" | "glob", Some(a)) => a
            .get("pattern")
            .or_else(|| a.get("query"))
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        ("grep_search" | "grep", Some(a)) => a
            .get("query")
            .or_else(|| a.get("pattern"))
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        ("list_dir", Some(a)) => a
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or(".")
            .to_string(),
        ("webfetch", Some(a)) => a
            .get("url")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        ("websearch", Some(a)) => a
            .get("query")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        ("skill", Some(a)) => a
            .get("name")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
        ("task", Some(a)) => {
            let agent = a
                .get("subagent_type")
                .or_else(|| a.get("agent"))
                .and_then(|v| v.as_str())
                .unwrap_or("subagent");
            let description = a
                .get("description")
                .or_else(|| a.get("prompt"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if description.is_empty() {
                agent.to_string()
            } else {
                format!("{agent}: {description}")
            }
        }
        ("execute", Some(a)) => a
            .get("command")
            .or_else(|| a.get("tool"))
            .and_then(|v| v.as_str())
            .unwrap_or("execute")
            .to_string(),
        (_, Some(a)) => {
            if let Some(s) = a
                .get("path")
                .or_else(|| a.get("command"))
                .or_else(|| a.get("query"))
                .or_else(|| a.get("pattern"))
                .and_then(|v| v.as_str())
            {
                s.to_string()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    };

    match name {
        "read_file" | "read" => ("Read", "Reading", desc),
        "write_file" | "write" => ("Write", "Writing", desc),
        "edit_file" | "edit" => ("Edit", "Editing", desc),
        "patch" | "apply_patch" => ("Applied patch", "Applying patch", desc),
        "list_dir" => ("List", "Listing", desc),
        "glob_search" | "glob" => ("Glob", "Running glob_search", desc),
        "grep_search" | "grep" => ("Grep", "Running grep_search", desc),
        "bash" | "sh" => ("Ran", "Running", desc),
        "question" => ("Ask", "Asking", desc),
        "update_plan" => ("Updated Plan", "Updating Plan", desc),
        "task" => ("Task", "Delegating", desc),
        "execute" => ("Execute", "Executing", desc),
        "webfetch" => ("Fetched", "Fetching", desc),
        "websearch" => ("Searched", "Searching", desc),
        "skill" => ("Loaded skill", "Loading skill", desc),
        _ => ("Tool", "Running", desc),
    }
}

pub fn format_tool_success_detail(name: &str, output: &str) -> String {
    match name {
        "read_file" | "read" => {
            let lines = output.lines().count();
            if lines == 1 {
                "1 line".to_string()
            } else {
                format!("{lines} lines")
            }
        }
        "grep_search" | "grep" => {
            let lines = output.lines().count();
            if lines == 0 || output.trim().is_empty() {
                "0 matches".to_string()
            } else if lines == 1 {
                "1 line".to_string()
            } else {
                format!("{lines} lines")
            }
        }
        "glob_search" | "glob" => "succeeded".to_string(),
        "list_dir" => {
            let count = output.lines().count();
            if count == 1 {
                "1 entry".to_string()
            } else {
                format!("{count} entries")
            }
        }
        "write_file" | "write" => {
            let trimmed = output.trim();
            if trimmed.starts_with("Successfully wrote") {
                trimmed.to_string()
            } else {
                let count = output.lines().count();
                if count <= 1 {
                    if !trimmed.is_empty() && trimmed.len() < 80 {
                        trimmed.to_string()
                    } else {
                        "succeeded".to_string()
                    }
                } else {
                    format!("{count} lines")
                }
            }
        }
        "edit_file" | "edit" | "patch" | "apply_patch" => "succeeded".to_string(),
        "bash" | "sh" => {
            let count = output.lines().count();
            if output.trim().is_empty() {
                "succeeded".to_string()
            } else if count == 1 && output.trim().len() < 60 {
                output.trim().to_string()
            } else {
                format!("{count} lines")
            }
        }
        "question" => "answered".to_string(),
        "update_plan" => {
            if !output.trim().is_empty() {
                output.trim().to_string()
            } else {
                "Plan updated".to_string()
            }
        }
        "webfetch" => {
            let count = output.lines().count();
            if count == 1 {
                "1 line".to_string()
            } else {
                format!("{count} lines")
            }
        }
        "websearch" => {
            let count = output
                .lines()
                .filter(|l| {
                    let trimmed = l.trim_start();
                    trimmed.chars().next().is_some_and(|c| c.is_ascii_digit())
                        && trimmed.contains(". ")
                })
                .count();
            if count == 0 {
                if output.contains("0 results")
                    || output.contains("No search results")
                    || output.contains("No results")
                {
                    "0 results".to_string()
                } else {
                    "succeeded".to_string()
                }
            } else if count == 1 {
                "1 result".to_string()
            } else {
                format!("{count} results")
            }
        }
        "skill" => "skill loaded successfully".to_string(),
        _ => {
            let count = output.lines().count();
            if count <= 1 && output.trim().len() < 60 && !output.trim().is_empty() {
                output.trim().to_string()
            } else if count > 1 {
                format!("{count} lines")
            } else {
                "succeeded".to_string()
            }
        }
    }
}

impl App {
    pub fn active_tool(&self) -> Option<&ActiveToolInfo> {
        self.active_tool.as_ref()
    }

    pub fn tool_rows(&self) -> &[ToolRow] {
        &self.tool_rows
    }

    pub fn set_tool_rows_for_test(&mut self, rows: Vec<ToolRow>) {
        self.tool_rows = rows;
    }

    pub fn is_tool_expanded(&self, call_id: &str) -> bool {
        self.expanded_tool_rows.contains(call_id)
    }

    pub fn toggle_tool_expanded(&mut self, call_id: &str) {
        if !self.expanded_tool_rows.insert(call_id.to_string()) {
            self.expanded_tool_rows.remove(call_id);
        }
    }

    pub fn is_thought_expanded(&self) -> bool {
        self.thought_expanded
    }

    pub fn toggle_thought_expanded(&mut self) {
        self.thought_expanded = !self.thought_expanded;
    }

    pub fn set_tool_row_clicks(&self, clicks: Vec<(String, Rect)>) {
        *self.last_tool_row_clicks.borrow_mut() = clicks;
    }

    pub(crate) fn tool_row_at(&self, x: u16, y: u16) -> Option<String> {
        self.last_tool_row_clicks
            .borrow()
            .iter()
            .find(|(_, area)| {
                x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
            })
            .map(|(id, _)| id.clone())
    }

    pub(crate) fn upsert_tool_row(&mut self, update: ToolRowUpdate<'_>) {
        let ToolRowUpdate {
            call_id,
            name,
            state,
            desc,
            arguments,
            metadata,
        } = update;
        if let Some(row) = self.tool_rows.iter_mut().find(|row| row.call_id == call_id) {
            row.name = name.to_string();
            row.state = state;
            if !desc.is_empty() {
                row.desc = bounded(desc, MAX_TOOL_ARGUMENT_BYTES);
            }
            if !arguments.is_empty() {
                row.arguments = bounded(arguments, MAX_TOOL_ARGUMENT_BYTES);
            }
            if metadata.is_some() {
                row.metadata = metadata;
            }
            row.expandable = row.compute_expandable();
        } else {
            let mut row = ToolRow {
                call_id: bounded(call_id.to_string(), MAX_IDENTITY_BYTES),
                name: bounded(name.to_string(), MAX_IDENTITY_BYTES),
                desc: bounded(desc, MAX_TOOL_ARGUMENT_BYTES),
                arguments: bounded(arguments, MAX_TOOL_ARGUMENT_BYTES),
                output: String::new(),
                state,
                arguments_complete: false,
                metadata,
                started_at: std::time::Instant::now(),
                expandable: false,
            };
            row.expandable = row.compute_expandable();
            self.tool_rows.push(row);
            if self.tool_rows.len() > MAX_TOOL_ROWS {
                self.tool_rows.remove(0);
            }
        }
        self.refresh_active_tool();
    }

    pub(crate) fn normalized_tool_call_id(call_id: &str) -> String {
        bounded(call_id.to_string(), MAX_IDENTITY_BYTES)
    }

    pub(crate) fn complete_tool_row(
        &mut self,
        call_id: &str,
        name: &str,
        success: bool,
        output: &str,
    ) -> bool {
        let found_index = if !call_id.is_empty() {
            let norm_id = Self::normalized_tool_call_id(call_id);
            self.tool_rows
                .iter()
                .position(|r| r.call_id == norm_id)
                .or_else(|| {
                    self.tool_rows.iter().rposition(|r| {
                        tool_names_match(&r.name, name)
                            && matches!(r.state, ToolRowState::Pending | ToolRowState::Running)
                    })
                })
        } else {
            self.tool_rows
                .iter()
                .rposition(|r| {
                    tool_names_match(&r.name, name)
                        && matches!(r.state, ToolRowState::Pending | ToolRowState::Running)
                })
                .or_else(|| {
                    self.tool_rows.iter().rposition(|r| {
                        matches!(r.state, ToolRowState::Pending | ToolRowState::Running)
                    })
                })
        };
        let Some(idx) = found_index else {
            return false;
        };
        let row = &mut self.tool_rows[idx];
        row.state = if success {
            ToolRowState::Completed
        } else {
            ToolRowState::Failed
        };
        row.output = bounded(output.to_string(), MAX_TOOL_OUTPUT_BYTES);
        row.expandable = row.compute_expandable();
        self.refresh_active_tool();
        true
    }

    pub(crate) fn refresh_active_tool(&mut self) {
        self.active_tool = self
            .tool_rows
            .iter()
            .rev()
            .find(|row| matches!(row.state, ToolRowState::Pending | ToolRowState::Running))
            .map(|row| ActiveToolInfo {
                name: row.name.clone(),
                desc: if row.desc.is_empty() {
                    "preparing arguments...".to_string()
                } else {
                    row.desc.clone()
                },
                started_at: row.started_at,
            });
    }
}
