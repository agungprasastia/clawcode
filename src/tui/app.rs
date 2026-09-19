use ratatui::layout::Rect;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};

use crate::cli::{self, CommandOutput, ConversationMode as CommandMode};
use crate::conversation::ConversationEvent;
use crate::notify::{BestEffortNotifier, Notification, NotificationKind, Notifier};
use crate::platform::SystemClipboard;
use crate::provider::FinishReason;
use crate::provider::TurnMetrics;
use crate::runtime::{EventBus, RuntimeEvent, client::RuntimeClient};

use super::dialogs::{
    AgentsDialogState, ModelsDialogState, PermissionDecision, PermissionDialogState,
    QuestionDialogState, SessionsDialogState, StatusDialogState, ThemesDialogState, WhichKeyState,
};
use super::home::HomeState;
const MAX_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const MAX_IDENTITY_BYTES: usize = 256;
const MAX_TOOL_ROWS: usize = 64;
const MAX_TOOL_ARGUMENT_BYTES: usize = 4 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ConversationMode {
    Plan,
    Build,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum ConversationStatus {
    #[default]
    Idle,
    Active,
    Finished(FinishReason),
    Error,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Input {
    Quit,
    Cancel,
    Clear,
    Character(char),
    Paste,
    Backspace,
    Submit,
    ToggleMode,
    Up,
    Down,
    Left,
    Right,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    Home,
    End,
    WhichKey,
    Click { column: u16, row: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSuggestion {
    pub name: &'static str,
    pub description: &'static str,
    pub template: &'static str,
}

pub const AVAILABLE_COMMANDS: &[CommandSuggestion] = &[
    CommandSuggestion {
        name: "/plan",
        description: "Switch to read-only Plan mode",
        template: "/plan",
    },
    CommandSuggestion {
        name: "/build",
        description: "Switch to Build mode with edits enabled",
        template: "/build",
    },
    CommandSuggestion {
        name: "/model",
        description: "Select active model",
        template: "/model",
    },
    CommandSuggestion {
        name: "/models",
        description: "Interactive model picker",
        template: "/models",
    },
    CommandSuggestion {
        name: "/models refresh",
        description: "Refresh provider model list",
        template: "/models refresh",
    },
    CommandSuggestion {
        name: "/agents",
        description: "Interactive agent picker & mode switcher",
        template: "/agents",
    },
    CommandSuggestion {
        name: "/themes",
        description: "Interactive theme selector & color palette",
        template: "/themes",
    },
    CommandSuggestion {
        name: "/theme",
        description: "Switch color theme (e.g. /theme catppuccin)",
        template: "/theme ",
    },
    CommandSuggestion {
        name: "/keys",
        description: "Keyboard shortcuts cheatsheet (Ctrl+X)",
        template: "/keys",
    },
    CommandSuggestion {
        name: "/status",
        description: "Show session status & diagnostics",
        template: "/status",
    },
    CommandSuggestion {
        name: "/connect",
        description: "Connect configured AI provider",
        template: "/connect",
    },
    CommandSuggestion {
        name: "/clear",
        description: "Clear conversation & return to home",
        template: "/clear",
    },
    CommandSuggestion {
        name: "/compact",
        description: "Compact session context",
        template: "/compact",
    },
    CommandSuggestion {
        name: "/copy",
        description: "Copy transcript or session status",
        template: "/copy",
    },
    CommandSuggestion {
        name: "/sessions",
        description: "List saved chat sessions",
        template: "/sessions",
    },
    CommandSuggestion {
        name: "/new",
        description: "Create a new conversation session",
        template: "/new ",
    },
    CommandSuggestion {
        name: "/help",
        description: "Show manual, commands & shortcuts",
        template: "/help",
    },
    CommandSuggestion {
        name: "/exit",
        description: "Quit Clawcode workbench",
        template: "/exit",
    },
];

pub const PLACEHOLDER_SUGGESTIONS: &[&str] = &[
    "Fix a TODO in the codebase",
    "What is the tech stack of this project?",
    "Write unit tests for this module",
    "Refactor this function for better performance",
    "Add error handling to this code",
    "Explain how this code works",
    "Find and fix a bug in this module",
    "Add documentation to this function",
    "Create a new feature for X",
    "Optimize this database query",
    "Add type hints to this code",
    "Implement caching for this endpoint",
];

pub fn get_random_placeholder() -> String {
    let index = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_millis() as usize) % PLACEHOLDER_SUGGESTIONS.len())
        .unwrap_or(0);
    format!("Ask anything... \"{}\"", PLACEHOLDER_SUGGESTIONS[index])
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UiEvent {
    Input(Input),
    Resize { width: u16, height: u16 },
    StreamDelta(String),
    Paste(String),
    MouseClick { x: u16, y: u16 },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ToolRowState {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamPart {
    Text(String),
    Reasoning(String),
    Tool(String),
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
pub fn split_provider_model(id: &str) -> Option<(&str, &str)> {
    if let Some((p, m)) = id.split_once('/')
        && matches!(
            p,
            "openai"
                | "anthropic"
                | "ollama"
                | "9router"
                | "groq"
                | "deepseek"
                | "gemini"
                | "openrouter"
                | "together"
        )
    {
        return Some((p, m));
    }
    None
}

pub fn is_sensitive_command(cmd: &str) -> bool {
    let lower = cmd.trim().to_lowercase();
    const PATTERNS: &[&str] = &[
        "rm ",
        "rm\t",
        "git reset",
        "git clean",
        "mkfs",
        "dd ",
        "dd\t",
        "kill ",
        "kill\t",
        "chmod ",
        "chmod\t",
        "chown ",
        "chown\t",
    ];
    if lower == "rm" || lower == "dd" || lower == "kill" || lower == "mkfs" {
        return true;
    }
    PATTERNS.iter().any(|&p| lower.contains(p))
}

pub struct App {
    running: bool,
    cancellation_pending: bool,
    prompt: String,
    cursor_position: usize,
    placeholder: String,
    transcript: String,
    mode: ConversationMode,
    status: ConversationStatus,
    provider: String,
    model: String,
    diagnostic: String,
    metrics: Option<TurnMetrics>,
    command_service: crate::cli::CommandService<cli::CliDiscovery>,
    selected_suggestion: usize,
    active_session_id: Option<i64>,
    /// Per-session view state. The fields above are the live window onto the
    /// active session; switching stashes them here and restores the target.
    sessions: std::collections::HashMap<i64, ClientSessionState>,
    /// Snapshot shown by the /sessions panel.
    session_listings: Vec<crate::persistence::Session>,
    /// Active interactive permission dialog, if security approval is required.
    permission_dialog: Option<PermissionDialogState>,
    /// Last decision recorded from the permission dialog.
    last_permission_decision: Option<PermissionDecision>,
    /// Active interactive question dialog, if agent asks a clarifying question.
    question_dialog: Option<QuestionDialogState>,
    /// Last user answer to a question dialog.
    last_question_answer: Option<String>,
    /// Active interactive sessions selection dialog, if opened.
    sessions_dialog: Option<SessionsDialogState>,
    /// Active interactive model selection dialog, if opened.
    models_dialog: Option<ModelsDialogState>,
    /// Active interactive agent mode selection dialog, if opened.
    agents_dialog: Option<AgentsDialogState>,
    /// Active interactive theme selection dialog, if opened.
    themes_dialog: Option<ThemesDialogState>,
    /// Active interactive system status dialog, if opened.
    status_dialog: Option<StatusDialogState>,
    /// Active theme color scheme.
    theme: crate::tui::ThemeKind,
    /// Quick keyboard shortcuts popup status.
    which_key: WhichKeyState,
    /// Cached list of discovered models for suggestions and selection.
    available_models: Vec<crate::provider::ModelInfo>,
    /// Cached git branch of the workspace
    git_branch: Option<String>,
    /// Animated mascot state for the home landing empty state.
    home_state: HomeState,
    /// Timestamp of last animation tick.
    last_animation_tick: std::time::Instant,
    /// Vertical scroll offset from the bottom of chat transcript (0 = auto-follow bottom).
    chat_scroll: u16,
    /// Smooth typewriter buffer and stream metrics.
    typewriter: crate::tui::TypewriterState,
    /// Animated wave spinner for streaming indicator.
    wave_spinner: crate::tui::WaveSpinner,
    /// Optional runtime client wiring prompt submissions to generation
    /// threads. Inactive until a provider-backed runtime is attached.
    runtime: Option<RuntimeClient>,
    /// Live events from the runtime for the active session.
    runtime_events: Option<std::sync::mpsc::Receiver<RuntimeEvent>>,
    /// Submitted prompt history for Up/Down arrow recall in the input card.
    prompt_history: Vec<String>,
    /// Active index into prompt history during navigation (None = typing new prompt).
    history_index: Option<usize>,
    /// Saved draft when user was typing and started navigating history with Up arrow.
    draft_prompt: String,
    active_tool: Option<ActiveToolInfo>,
    tool_rows: Vec<ToolRow>,
    active_generation_id: Option<i64>,
    ignored_generation_ids: HashSet<i64>,
    pending_finish_reason: Option<FinishReason>,
    text_stream_active: bool,
    reasoning_buffer: String,
    reasoning_start: Option<std::time::Instant>,
    reasoning_duration: Option<std::time::Duration>,
    reasoning_active: bool,
    stream_parts: Vec<StreamPart>,
    stream_base_len: Option<usize>,
    expanded_tool_rows: HashSet<String>,
    thought_expanded: bool,
    last_tool_row_clicks: RefCell<Vec<(String, Rect)>>,
    pub current_plan: Vec<(String, String)>,
    last_popup_area: std::cell::Cell<Option<Rect>>,
    last_quick_actions_area: std::cell::Cell<Option<[Rect; 4]>>,
    terminal_size: std::cell::Cell<(u16, u16)>,
}

/// View state isolated per session: switching sessions must not reset the
/// draft, transcript, or scroll position of either side.
#[derive(Debug, Default, Clone)]
pub struct ClientSessionState {
    pub transcript: String,
    pub input_draft: String,
    pub scroll: u16,
    pub loaded_until_seq: i64,
    pub status: ConversationStatus,
    pub active_generation_id: Option<i64>,
    pub text_stream_active: bool,
    pub loading: bool,
    pub current_plan: Vec<(String, String)>,
    pub tool_rows: Vec<ToolRow>,
    pub stream_parts: Vec<StreamPart>,
    pub stream_base_len: Option<usize>,
    pub expanded_tool_rows: HashSet<String>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let config = crate::config::ConfigLoader.load().unwrap_or_default();
        let (initial_p, initial_m) = config.initial_provider_and_model();
        let mut command_service =
            cli::runtime_service_with_config(&config).expect("in-memory runtime database");
        let available_models = command_service.models().unwrap_or_default();
        Self {
            running: true,
            cancellation_pending: false,
            prompt: String::new(),
            cursor_position: 0,
            placeholder: get_random_placeholder(),
            transcript: String::new(),
            mode: ConversationMode::Plan,
            status: ConversationStatus::Idle,
            provider: initial_p,
            model: initial_m,
            diagnostic: String::new(),
            metrics: None,
            command_service,
            selected_suggestion: 0,
            active_session_id: None,
            sessions: std::collections::HashMap::new(),
            session_listings: Vec::new(),
            permission_dialog: None,
            last_permission_decision: None,
            question_dialog: None,
            last_question_answer: None,
            sessions_dialog: None,
            models_dialog: None,
            agents_dialog: None,
            themes_dialog: None,
            status_dialog: None,
            theme: crate::tui::ThemeKind::default(),
            which_key: WhichKeyState::default(),
            available_models,
            git_branch: crate::platform::get_current_branch(),
            home_state: HomeState::new(),
            last_animation_tick: std::time::Instant::now(),
            chat_scroll: 0,
            typewriter: crate::tui::TypewriterState::new(),
            wave_spinner: crate::tui::WaveSpinner::new(ratatui::style::Color::Rgb(224, 159, 63)),
            runtime: None,
            runtime_events: None,
            prompt_history: Vec::new(),
            history_index: None,
            draft_prompt: String::new(),
            active_tool: None,
            tool_rows: Vec::new(),
            active_generation_id: None,
            ignored_generation_ids: HashSet::new(),
            pending_finish_reason: None,
            text_stream_active: false,
            reasoning_buffer: String::new(),
            reasoning_start: None,
            reasoning_duration: None,
            reasoning_active: false,
            stream_parts: Vec::new(),
            stream_base_len: None,
            expanded_tool_rows: HashSet::new(),
            thought_expanded: false,
            last_tool_row_clicks: RefCell::new(Vec::new()),
            current_plan: Vec::new(),
            last_popup_area: std::cell::Cell::new(None),
            last_quick_actions_area: std::cell::Cell::new(None),
            terminal_size: std::cell::Cell::new((100, 30)),
        }
    }

    pub const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024;
    pub const TRUNCATION_MARKER: &str = "[earlier transcript truncated]\n";

    fn request_runtime_cancel(&self) {
        if let Some(runtime) = self.runtime.as_ref()
            && let Some(session_id) = self.active_session_id
        {
            let _ = runtime.cancel_generation(session_id);
        }
    }

    fn reset_turn_view(&mut self, clear_transcript: bool) {
        if clear_transcript {
            self.transcript.clear();
        }
        self.typewriter.reset();
        self.text_stream_active = false;
        self.reasoning_buffer.clear();
        self.reasoning_start = None;
        self.reasoning_duration = None;
        self.reasoning_active = false;
        self.active_tool = None;
        self.tool_rows.clear();
        if let Some(generation_id) = self.active_generation_id.take() {
            self.ignored_generation_ids.insert(generation_id);
        }
        self.stream_parts.clear();
        self.stream_base_len = None;
        self.expanded_tool_rows.clear();
        self.thought_expanded = false;
        self.current_plan.clear();
        self.chat_scroll = 0;
    }

    pub fn apply(&mut self, event: UiEvent) {
        if self.permission_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Left)
                | UiEvent::Input(Input::Up)
                | UiEvent::Input(Input::Character('h'))
                | UiEvent::Input(Input::Character('k')) => {
                    if let Some(dialog) = &mut self.permission_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(Input::Right)
                | UiEvent::Input(Input::Down)
                | UiEvent::Input(Input::ToggleMode)
                | UiEvent::Input(Input::Character('l'))
                | UiEvent::Input(Input::Character('j')) => {
                    if let Some(dialog) = &mut self.permission_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.last_permission_decision = Some(PermissionDecision::Deny);
                    self.diagnostic = "Permission denied".to_string();
                    self.permission_dialog = None;
                }
                UiEvent::Input(Input::Submit) => {
                    if let Some(dialog) = &self.permission_dialog {
                        let decision = dialog.selected();
                        self.last_permission_decision = Some(decision);
                        self.diagnostic = match decision {
                            PermissionDecision::Deny => "Permission denied".to_string(),
                            PermissionDecision::AllowOnce => {
                                "Permission granted (once)".to_string()
                            }
                            PermissionDecision::AllowAlways => {
                                "Permission granted (always)".to_string()
                            }
                        };
                    }
                    self.permission_dialog = None;
                }
                _ => {}
            }
            return;
        }
        if self.question_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Up) | UiEvent::Input(Input::ScrollUp) => {
                    if let Some(dialog) = &mut self.question_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(Input::Down)
                | UiEvent::Input(Input::ScrollDown)
                | UiEvent::Input(Input::ToggleMode) => {
                    if let Some(dialog) = &mut self.question_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Input(Input::Character(c)) => {
                    if let Some(dialog) = &mut self.question_dialog
                        && (dialog.typing_custom || dialog.selected_option == dialog.options.len())
                    {
                        dialog.push_char(c);
                    }
                }
                UiEvent::Input(Input::Backspace) => {
                    if let Some(dialog) = &mut self.question_dialog
                        && (dialog.typing_custom || dialog.selected_option == dialog.options.len())
                    {
                        dialog.pop_char();
                    }
                }
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.diagnostic = "Question dismissed".to_string();
                    self.question_dialog = None;
                }
                UiEvent::Input(Input::Submit) => {
                    if let Some(dialog) = &self.question_dialog {
                        let answer = dialog.selected_answer();
                        self.diagnostic = format!("Answer selected: {answer}");
                        self.last_question_answer = Some(answer);
                    }
                    self.question_dialog = None;
                }
                _ => {}
            }
            return;
        }

        if self.which_key.visible {
            match event {
                UiEvent::Input(Input::WhichKey)
                | UiEvent::Input(Input::Quit)
                | UiEvent::Input(Input::Cancel) => {
                    self.which_key.hide();
                }
                UiEvent::Input(Input::Character('a')) => {
                    self.which_key.hide();
                    let current = match self.mode {
                        ConversationMode::Plan => "plan",
                        ConversationMode::Build => "build",
                    };
                    self.agents_dialog = Some(AgentsDialogState::new(current));
                }
                UiEvent::Input(Input::Character('t')) => {
                    self.which_key.hide();
                    self.themes_dialog = Some(ThemesDialogState::new(self.theme));
                }
                UiEvent::Input(Input::Character('m')) => {
                    self.which_key.hide();
                    let models = self.available_models.clone();
                    self.models_dialog = Some(ModelsDialogState::new(models, &self.model));
                }
                UiEvent::Input(Input::Character('p')) => {
                    self.which_key.hide();
                    self.set_mode(ConversationMode::Plan);
                }
                UiEvent::Input(Input::Character('b')) => {
                    self.which_key.hide();
                    self.set_mode(ConversationMode::Build);
                }
                UiEvent::Input(Input::Character('s')) => {
                    self.which_key.hide();
                    self.open_status_dialog();
                }
                UiEvent::Input(Input::Character('r')) => {
                    self.which_key.hide();
                    self.open_sessions_dialog();
                }
                UiEvent::Input(Input::Character('c')) => {
                    self.which_key.hide();
                    self.transcript.clear();
                    self.status = ConversationStatus::Idle;
                    self.diagnostic = "screen cleared".to_string();
                }
                UiEvent::Input(Input::ToggleMode) => {
                    self.which_key.hide();
                    self.toggle_mode();
                }
                _ => {
                    self.which_key.hide();
                }
            }
            return;
        }

        if self.agents_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.agents_dialog = None;
                }
                UiEvent::Input(Input::Character(character)) => {
                    if let Some(dialog) = &mut self.agents_dialog {
                        dialog.push_char(character);
                    }
                }
                UiEvent::Input(Input::Backspace) => {
                    if let Some(dialog) = &mut self.agents_dialog {
                        dialog.pop_char();
                    }
                }
                UiEvent::Input(Input::Up)
                | UiEvent::Input(Input::ScrollUp)
                | UiEvent::Input(Input::PageUp) => {
                    if let Some(dialog) = &mut self.agents_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(Input::Down)
                | UiEvent::Input(Input::ScrollDown)
                | UiEvent::Input(Input::PageDown) => {
                    if let Some(dialog) = &mut self.agents_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Input(Input::Submit) => {
                    let chosen_agent = self
                        .agents_dialog
                        .as_ref()
                        .and_then(|d| d.selected_agent().cloned());
                    self.agents_dialog = None;
                    if let Some(chosen) = chosen_agent {
                        self.set_mode(chosen.mode);
                        self.diagnostic = format!("agent selected: {}", chosen.name);
                    }
                }
                UiEvent::Input(Input::ToggleMode) => {
                    if let Some(dialog) = &mut self.agents_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Resize { .. } => {}
                UiEvent::StreamDelta(delta) => {
                    self.transcript.push_str(&delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return;
        }

        if self.themes_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.themes_dialog = None;
                }
                UiEvent::Input(Input::Character(character)) => {
                    if let Some(dialog) = &mut self.themes_dialog {
                        dialog.push_char(character);
                    }
                }
                UiEvent::Input(Input::Backspace) => {
                    if let Some(dialog) = &mut self.themes_dialog {
                        dialog.pop_char();
                    }
                }
                UiEvent::Input(Input::Up)
                | UiEvent::Input(Input::ScrollUp)
                | UiEvent::Input(Input::PageUp) => {
                    if let Some(dialog) = &mut self.themes_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(Input::Down)
                | UiEvent::Input(Input::ScrollDown)
                | UiEvent::Input(Input::PageDown) => {
                    if let Some(dialog) = &mut self.themes_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Input(Input::Submit) => {
                    if let Some(dialog) = &self.themes_dialog
                        && let Some(chosen) = dialog.selected_theme()
                    {
                        self.set_theme(chosen);
                        self.diagnostic = format!("theme switched to: {}", chosen.name());
                    }
                    self.themes_dialog = None;
                }
                UiEvent::Input(Input::ToggleMode) => {
                    if let Some(dialog) = &mut self.themes_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Resize { .. } => {}
                UiEvent::StreamDelta(delta) => {
                    self.transcript.push_str(&delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return;
        }

        if self.sessions_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.sessions_dialog = None;
                    self.session_listings.clear();
                }
                UiEvent::Input(Input::Character(character)) => {
                    if let Some(dialog) = &mut self.sessions_dialog {
                        if dialog.filter.is_empty() && character == '/' {
                            self.sessions_dialog = None;
                            self.session_listings.clear();
                            self.history_index = None;
                            self.prompt.push('/');
                            self.cursor_position = 1;
                            self.selected_suggestion = 0;
                            return;
                        }
                        if dialog.filter.is_empty() && character == 'd' {
                            if let Some(session) = dialog.selected_session().cloned() {
                                let title = session.title.clone();
                                let id = session.id;
                                let _ = self.command_service.delete_session(id);
                                dialog.remove_item(id);
                                self.session_listings.retain(|s| s.id != id);
                                self.diagnostic = format!("session deleted: {title}");
                            }
                            return;
                        }
                        dialog.push_char(character);
                    }
                }
                UiEvent::Input(Input::Backspace) => {
                    if let Some(dialog) = &mut self.sessions_dialog {
                        dialog.pop_char();
                    }
                }
                UiEvent::Input(Input::Up)
                | UiEvent::Input(Input::ScrollUp)
                | UiEvent::Input(Input::PageUp) => {
                    if let Some(dialog) = &mut self.sessions_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(Input::Down)
                | UiEvent::Input(Input::ScrollDown)
                | UiEvent::Input(Input::PageDown) => {
                    if let Some(dialog) = &mut self.sessions_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Input(Input::Submit) => {
                    let chosen_session = self
                        .sessions_dialog
                        .as_ref()
                        .and_then(|d| d.selected_session().cloned());
                    if let Some(chosen) = chosen_session {
                        self.switch_session(chosen.id);
                        self.diagnostic =
                            format!("switched to session #{} ({})", chosen.id, chosen.title);
                    }
                    self.sessions_dialog = None;
                    self.session_listings.clear();
                }
                UiEvent::Input(Input::ToggleMode) => {
                    if let Some(dialog) = &mut self.sessions_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Resize { .. } => {}
                UiEvent::StreamDelta(delta) => {
                    self.transcript.push_str(&delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return;
        }

        if self.models_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.models_dialog = None;
                }
                UiEvent::Input(Input::Character(character)) => {
                    if let Some(dialog) = &mut self.models_dialog {
                        dialog.push_char(character);
                    }
                }
                UiEvent::Input(Input::Backspace) => {
                    if let Some(dialog) = &mut self.models_dialog {
                        dialog.pop_char();
                    }
                }
                UiEvent::Input(Input::Up)
                | UiEvent::Input(Input::ScrollUp)
                | UiEvent::Input(Input::PageUp) => {
                    if let Some(dialog) = &mut self.models_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(Input::Down)
                | UiEvent::Input(Input::ScrollDown)
                | UiEvent::Input(Input::PageDown) => {
                    if let Some(dialog) = &mut self.models_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Input(Input::Submit) => {
                    if let Some(dialog) = &self.models_dialog
                        && let Some(chosen) = dialog.selected_model()
                    {
                        let chosen_id = chosen.id.clone();
                        if let Some((provider, model)) = split_provider_model(&chosen_id) {
                            self.provider = bounded(provider.to_string(), MAX_IDENTITY_BYTES);
                            self.model = bounded(model.to_string(), MAX_IDENTITY_BYTES);
                        } else {
                            self.model = bounded(chosen_id.clone(), MAX_IDENTITY_BYTES);
                        }
                        self.diagnostic = format!("model switched to: {chosen_id}");
                    }
                    self.models_dialog = None;
                }
                UiEvent::Input(Input::ToggleMode) => {
                    if let Some(dialog) = &mut self.models_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Resize { .. } => {}
                UiEvent::StreamDelta(delta) => {
                    self.transcript.push_str(&delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return;
        }

        if self.status_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit)
                | UiEvent::Input(Input::Cancel)
                | UiEvent::Input(Input::Submit)
                | UiEvent::Input(Input::Clear) => {
                    self.status_dialog = None;
                }
                UiEvent::Resize { .. } => {}
                UiEvent::StreamDelta(delta) => {
                    self.transcript.push_str(&delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return;
        }

        if let UiEvent::MouseClick { x, y } | UiEvent::Input(Input::Click { column: x, row: y }) =
            event
        {
            self.handle_mouse_click(x, y);
            return;
        }

        match event {
            UiEvent::Input(Input::Quit) => {
                if self.session_listings.is_empty() {
                    self.running = false;
                } else {
                    self.session_listings.clear();
                }
            }
            UiEvent::Input(Input::Cancel) => {
                if let Some(generation_id) = self.active_generation_id {
                    self.ignored_generation_ids.insert(generation_id);
                }
                self.request_runtime_cancel();
                self.reset_turn_view(false);
                self.cancellation_pending = true;
                self.status = ConversationStatus::Active;
            }
            UiEvent::Input(Input::Clear) => {
                self.request_runtime_cancel();
                self.reset_turn_view(true);
                self.status = ConversationStatus::Idle;
                self.diagnostic = "screen cleared".to_string();
            }
            UiEvent::Input(Input::Character(character)) => {
                self.history_index = None;
                let byte_idx = self
                    .prompt
                    .char_indices()
                    .nth(self.cursor_position)
                    .map(|(i, _)| i)
                    .unwrap_or(self.prompt.len());
                self.prompt.insert(byte_idx, character);
                self.cursor_position += 1;
                self.selected_suggestion = 0;
            }
            UiEvent::Input(Input::Backspace) => {
                self.history_index = None;
                if self.cursor_position > 0 {
                    let prev_pos = self.cursor_position - 1;
                    if let Some((byte_idx, _)) = self.prompt.char_indices().nth(prev_pos) {
                        self.prompt.remove(byte_idx);
                        self.cursor_position = prev_pos;
                    }
                }
                self.selected_suggestion = 0;
            }
            UiEvent::Input(Input::Paste) => {
                if let Ok(text) = SystemClipboard.get_text() {
                    self.insert_str_at_cursor(&text);
                }
            }
            UiEvent::Paste(text) => {
                self.insert_str_at_cursor(&text);
            }
            UiEvent::Input(Input::Up) => {
                if self.suggestion_count() > 0 {
                    self.previous_suggestion();
                } else {
                    self.navigate_history_up();
                }
            }
            UiEvent::Input(Input::Down) => {
                if self.suggestion_count() > 0 {
                    self.next_suggestion();
                } else {
                    self.navigate_history_down();
                }
            }
            UiEvent::Input(Input::ScrollUp) => {
                self.scroll_up(3);
            }
            UiEvent::Input(Input::ScrollDown) => {
                self.scroll_down(3);
            }
            UiEvent::Input(Input::PageUp) => {
                self.scroll_up(15);
            }
            UiEvent::Input(Input::PageDown) => {
                self.scroll_down(15);
            }
            UiEvent::Input(Input::Home) => {
                self.cursor_position = 0;
                self.scroll_to_top();
            }
            UiEvent::Input(Input::End) => {
                self.cursor_position = self.prompt.chars().count();
                self.scroll_to_bottom();
            }
            UiEvent::Input(Input::Left) => {
                self.cursor_position = self.cursor_position.saturating_sub(1);
            }
            UiEvent::Input(Input::Right) => {
                self.cursor_position = (self.cursor_position + 1).min(self.prompt.chars().count());
            }
            UiEvent::Input(Input::Submit) => self.submit_prompt(),
            UiEvent::Input(Input::WhichKey) => {
                self.which_key.toggle();
            }
            UiEvent::Input(Input::ToggleMode) => {
                if !self.matching_suggestions().is_empty()
                    || self.prompt.starts_with("/model ")
                    || self.prompt.starts_with("/theme ")
                {
                    self.autocomplete_selected_command();
                } else {
                    self.toggle_mode();
                }
            }
            UiEvent::Resize { width, height } => {
                self.terminal_size.set((width, height));
                self.last_popup_area.set(None);
                self.last_quick_actions_area.set(None);
            }
            UiEvent::MouseClick { .. } => {}
            UiEvent::Input(Input::Click { .. }) => {}
            UiEvent::StreamDelta(delta) => {
                self.transcript.push_str(&delta);
                self.truncate_transcript();
            }
        }
    }
    pub fn apply_pending(&mut self, events: &mut UiEventQueue) -> bool {
        let Some(event) = events.pop_priority() else {
            let pending = events.drain();
            if pending.is_empty() {
                return false;
            }
            self.apply_batch(pending);
            return true;
        };

        let quitting = event == UiEvent::Input(Input::Quit)
            && self.session_listings.is_empty()
            && self.permission_dialog.is_none()
            && self.question_dialog.is_none()
            && self.sessions_dialog.is_none()
            && self.models_dialog.is_none()
            && self.agents_dialog.is_none()
            && self.themes_dialog.is_none()
            && self.status_dialog.is_none()
            && !self.which_key.visible;
        self.apply(event);
        if quitting {
            events.clear();
        } else {
            let pending = events.drain();
            self.apply_batch(pending);
        }
        true
    }

    pub fn apply_batch(&mut self, events: impl IntoIterator<Item = UiEvent>) {
        for event in events {
            self.apply(event);
        }
    }

    pub fn apply_conversation(&mut self, event: ConversationEvent) {
        if self.status == ConversationStatus::Cancelled {
            return;
        }
        match event {
            ConversationEvent::PromptSubmitted {
                provider, model, ..
            } => {
                self.provider = bounded(provider, MAX_IDENTITY_BYTES);
                self.model = bounded(model, MAX_IDENTITY_BYTES);
                self.metrics = None;
                self.status = ConversationStatus::Active;
                self.diagnostic.clear();
                self.stream_parts.clear();
                self.stream_base_len = Some(self.transcript.len());
                self.expanded_tool_rows.clear();
                self.typewriter.start_stream();
            }
            ConversationEvent::TextDelta(delta) => {
                if self.status == ConversationStatus::Active {
                    self.push_stream_part(StreamPart::Text(delta.clone()));
                    self.typewriter.push_delta(&delta);
                }
            }
            ConversationEvent::Finished(reason) => {
                if self.status == ConversationStatus::Active {
                    self.flush_typewriter();
                    self.status = ConversationStatus::Finished(reason);
                    if let Some(session_id) = self.active_session_id {
                        let _ = self.command_service.append_message(
                            session_id,
                            "assistant",
                            &self.transcript,
                        );
                    }
                    self.notify(
                        NotificationKind::Success,
                        "Turn complete",
                        "Conversation finished",
                    );
                }
            }
            ConversationEvent::Error(message) => {
                if self.status == ConversationStatus::Active {
                    self.flush_typewriter();
                    self.status = ConversationStatus::Error;
                    self.diagnostic = bounded(message, MAX_DIAGNOSTIC_BYTES);
                    self.notify(
                        NotificationKind::Error,
                        "Turn failed",
                        &self.diagnostic.clone(),
                    );
                }
            }
            ConversationEvent::Cancelled => {
                self.status = ConversationStatus::Cancelled;
                self.notify(
                    NotificationKind::Cancelled,
                    "Turn cancelled",
                    "Conversation cancelled",
                );
            }
            ConversationEvent::MutationRequested(operation) => {
                if self.mode == ConversationMode::Plan {
                    self.status = ConversationStatus::Rejected;
                    self.diagnostic = bounded(
                        format!("PLAN mode rejects mutation: {operation}"),
                        MAX_DIAGNOSTIC_BYTES,
                    );
                }
            }
            ConversationEvent::Usage(_) => {}
            ConversationEvent::Metrics(mut metrics) => {
                if matches!(
                    self.status,
                    ConversationStatus::Finished(
                        FinishReason::Stop | FinishReason::Length | FinishReason::ToolCall
                    )
                ) && metrics.finish_reason
                    == Some(match self.status {
                        ConversationStatus::Finished(reason) => reason,
                        _ => unreachable!(),
                    })
                {
                    metrics.provider = bounded(metrics.provider, MAX_IDENTITY_BYTES);
                    metrics.model = bounded(metrics.model, MAX_IDENTITY_BYTES);
                    self.metrics = Some(metrics);
                }
            }
        }
    }

    fn notify(&self, kind: NotificationKind, title: &str, body: &str) {
        let Ok(notification) = Notification::new(kind, title, body) else {
            return;
        };
        std::thread::spawn(move || {
            let report = BestEffortNotifier.notify(&notification);
            for failure in report.failures {
                tracing::debug!(%failure, "notification backend failed");
            }
        });
    }

    pub fn set_mode(&mut self, mode: ConversationMode) {
        self.mode = mode;
        let theme_bundle = self.theme.to_theme();
        let mode_color = match mode {
            ConversationMode::Plan => theme_bundle.amber,
            ConversationMode::Build => theme_bundle.teal,
        };
        self.wave_spinner.set_color(mode_color);
        let command_mode = match mode {
            ConversationMode::Plan => CommandMode::Plan,
            ConversationMode::Build => CommandMode::Build,
        };
        let _ = self
            .command_service
            .execute(crate::cli::Command::Mode(command_mode));
    }

    pub fn toggle_mode(&mut self) {
        let next_mode = match self.mode {
            ConversationMode::Plan => ConversationMode::Build,
            ConversationMode::Build => ConversationMode::Plan,
        };
        self.set_mode(next_mode);
    }
    pub fn mode(&self) -> ConversationMode {
        self.mode
    }
    pub fn conversation_status(&self) -> ConversationStatus {
        self.status
    }
    pub fn selected_provider(&self) -> &str {
        &self.provider
    }
    pub fn selected_model(&self) -> &str {
        &self.model
    }
    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }

    pub fn metrics(&self) -> Option<&TurnMetrics> {
        self.metrics.as_ref()
    }

    pub fn set_metrics(&mut self, metrics: Option<TurnMetrics>) {
        self.metrics = metrics;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn git_branch(&self) -> Option<&str> {
        self.git_branch.as_deref()
    }

    pub fn set_git_branch(&mut self, branch: Option<String>) {
        self.git_branch = branch;
    }

    pub fn home_state(&self) -> &HomeState {
        &self.home_state
    }

    pub fn home_state_mut(&mut self) -> &mut HomeState {
        &mut self.home_state
    }
    /// Whether assistant text is visible or may resume during active generation.
    pub fn is_typing(&self) -> bool {
        self.typewriter.is_typing()
            || (self.text_stream_active
                && matches!(self.status, ConversationStatus::Active)
                && !self.is_reasoning()
                && !self
                    .tool_rows
                    .iter()
                    .any(|row| matches!(row.state, ToolRowState::Pending | ToolRowState::Running)))
    }

    /// Whether generation, text, reasoning, or any tool row is active.
    pub fn is_streaming_active(&self) -> bool {
        matches!(self.status, ConversationStatus::Active)
            || self.typewriter.is_typing()
            || self.typewriter.pending_status().is_some()
            || self.is_reasoning()
            || self
                .tool_rows
                .iter()
                .any(|row| matches!(row.state, ToolRowState::Pending | ToolRowState::Running))
    }

    pub fn reasoning_buffer(&self) -> &str {
        &self.reasoning_buffer
    }

    pub fn is_reasoning(&self) -> bool {
        self.reasoning_active && self.active_tool.is_none()
    }

    pub fn reasoning_elapsed_seconds(&self) -> Option<f64> {
        self.reasoning_duration
            .map(|d| d.as_secs_f64())
            .or_else(|| self.reasoning_start.map(|t| t.elapsed().as_secs_f64()))
    }

    fn record_reasoning_duration(&mut self) {
        if let Some(start) = self.reasoning_start
            && self.reasoning_duration.is_none()
        {
            self.reasoning_duration = Some(start.elapsed());
        }
    }
    pub fn flush_reasoning(&mut self) {
        if self.reasoning_buffer.trim().is_empty() {
            self.reasoning_buffer.clear();
            self.reasoning_start = None;
            self.reasoning_duration = None;
            self.reasoning_active = false;
            return;
        }
        let duration = self
            .reasoning_duration
            .or_else(|| self.reasoning_start.map(|t| t.elapsed()))
            .unwrap_or_default()
            .as_secs_f64();
        let dur_str = format!("{:.1}s", duration.max(0.1));

        let mut snippet = String::new();
        if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
            snippet.push('\n');
        }
        if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
            snippet.push('\n');
        }
        snippet.push_str(&format!("Thought for {dur_str}\n\n"));
        self.transcript.push_str(&snippet);
        self.truncate_transcript();
        self.reasoning_buffer.clear();
        self.reasoning_start = None;
        self.reasoning_duration = None;
        self.reasoning_active = false;
    }

    pub fn active_tool(&self) -> Option<&ActiveToolInfo> {
        self.active_tool.as_ref()
    }
    pub fn tool_rows(&self) -> &[ToolRow] {
        &self.tool_rows
    }
    pub fn stream_parts(&self) -> &[StreamPart] {
        &self.stream_parts
    }

    pub fn stream_base_len(&self) -> Option<usize> {
        self.stream_base_len
    }

    fn ensure_stream_parts(&mut self) {
        if self.stream_base_len.is_none() {
            self.stream_base_len = Some(self.transcript.len());
        }
    }

    fn push_stream_part(&mut self, part: StreamPart) {
        self.ensure_stream_parts();
        match part {
            StreamPart::Reasoning(delta) => {
                if let Some(StreamPart::Reasoning(existing)) = self
                    .stream_parts
                    .iter_mut()
                    .find(|p| matches!(p, StreamPart::Reasoning(_)))
                {
                    existing.push_str(&delta);
                    if existing.len() > Self::MAX_TRANSCRIPT_BYTES {
                        existing
                            .truncate(floor_char_boundary(existing, Self::MAX_TRANSCRIPT_BYTES));
                    }
                } else {
                    self.stream_parts.push(StreamPart::Reasoning(delta));
                }
            }
            StreamPart::Text(delta) => {
                if let Some(StreamPart::Text(existing)) = self.stream_parts.last_mut() {
                    existing.push_str(&delta);
                    if existing.len() > Self::MAX_TRANSCRIPT_BYTES {
                        existing
                            .truncate(floor_char_boundary(existing, Self::MAX_TRANSCRIPT_BYTES));
                    }
                } else {
                    self.stream_parts.push(StreamPart::Text(delta));
                }
            }
            StreamPart::Tool(call_id) => {
                if !self
                    .stream_parts
                    .iter()
                    .any(|part| matches!(part, StreamPart::Tool(id) if id == &call_id))
                {
                    self.stream_parts.push(StreamPart::Tool(call_id.clone()));
                }
                if let Some(row) = self.tool_rows.iter_mut().find(|r| r.call_id == call_id) {
                    row.expandable = row.compute_expandable();
                }
            }
        }
        if self.stream_parts.len() > MAX_TOOL_ROWS * 2 {
            self.stream_parts
                .drain(..self.stream_parts.len() - MAX_TOOL_ROWS * 2);
        }
    }

    fn push_tool_part(&mut self, call_id: String) {
        self.ensure_stream_parts();
        if !self
            .stream_parts
            .iter()
            .any(|part| matches!(part, StreamPart::Tool(id) if id == &call_id))
        {
            self.stream_parts.push(StreamPart::Tool(call_id.clone()));
        }
        if let Some(row) = self.tool_rows.iter_mut().find(|r| r.call_id == call_id) {
            row.expandable = row.compute_expandable();
        }
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

    fn tool_row_at(&self, x: u16, y: u16) -> Option<String> {
        self.last_tool_row_clicks
            .borrow()
            .iter()
            .find(|(_, area)| {
                x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
            })
            .map(|(id, _)| id.clone())
    }
    fn upsert_tool_row(
        &mut self,
        call_id: &str,
        name: &str,
        state: ToolRowState,
        desc: String,
        arguments: String,
        metadata: Option<serde_json::Value>,
    ) {
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

    fn normalized_tool_call_id(call_id: &str) -> String {
        bounded(call_id.to_string(), MAX_IDENTITY_BYTES)
    }

    fn complete_tool_row(
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

    fn refresh_active_tool(&mut self) {
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

    pub fn wave_spinner(&self) -> &crate::tui::WaveSpinner {
        &self.wave_spinner
    }

    pub fn wave_spinner_mut(&mut self) -> &mut crate::tui::WaveSpinner {
        &mut self.wave_spinner
    }

    pub fn typewriter(&self) -> &crate::tui::TypewriterState {
        &self.typewriter
    }

    pub fn typewriter_mut(&mut self) -> &mut crate::tui::TypewriterState {
        &mut self.typewriter
    }

    pub fn tokens_per_second(&self) -> Option<f64> {
        self.typewriter.tokens_per_second()
    }

    pub fn streaming_elapsed_seconds(&self) -> Option<f64> {
        self.typewriter.elapsed_seconds()
    }

    /// Flush all pending typewriter characters directly into the transcript.
    pub fn flush_typewriter(&mut self) {
        let remaining = self.typewriter.flush();
        if !remaining.is_empty() {
            self.transcript.push_str(&remaining);
            self.truncate_transcript();
        }
        if let Some(target) = self.typewriter.take_pending_status() {
            self.status = target;
        }
    }

    /// Advances periodic UI animations (mascot blinking, wave spinner, typewriter pacing).
    /// Returns true if a visual frame changed and redraw is needed.
    pub fn tick(&mut self) -> bool {
        const ANIMATION_INTERVAL: std::time::Duration = std::time::Duration::from_millis(40);
        let mut redraw = false;
        if self.last_animation_tick.elapsed() >= ANIMATION_INTERVAL {
            self.last_animation_tick = std::time::Instant::now();
            if self.transcript.is_empty() {
                let old_frame = self.home_state.frame();
                self.home_state.tick();
                if old_frame != self.home_state.frame() {
                    redraw = true;
                }
            }
            if self.is_typing()
                || self.typewriter.is_active()
                || self.active_tool.is_some()
                || matches!(self.status, ConversationStatus::Active)
            {
                if let Some(chunk) = self.typewriter.drain_step() {
                    self.transcript.push_str(&chunk);
                    self.truncate_transcript();
                }
                if !self.typewriter.is_typing()
                    && let Some(target) = self.typewriter.take_pending_status()
                {
                    self.typewriter.reset();
                    self.status = target;
                }
                redraw = true;
            }
        }
        redraw
    }

    pub fn chat_scroll(&self) -> u16 {
        self.chat_scroll
    }

    pub fn scroll_up(&mut self, lines: u16) {
        self.chat_scroll = self.chat_scroll.saturating_add(lines);
    }

    pub fn scroll_down(&mut self, lines: u16) {
        self.chat_scroll = self.chat_scroll.saturating_sub(lines);
    }

    pub fn scroll_to_top(&mut self) {
        self.chat_scroll = u16::MAX;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.chat_scroll = 0;
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn set_placeholder(&mut self, placeholder: String) {
        self.placeholder = placeholder;
    }

    pub fn rotate_placeholder(&mut self) {
        self.placeholder = get_random_placeholder();
    }

    /// Consumes a UI cancellation request. Provider cancellation is not wired yet.
    pub fn take_cancellation(&mut self) -> bool {
        std::mem::take(&mut self.cancellation_pending)
    }
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    pub fn insert_str_at_cursor(&mut self, text: &str) {
        let byte_pos = self
            .prompt
            .char_indices()
            .nth(self.cursor_position)
            .map(|(i, _)| i)
            .unwrap_or(self.prompt.len());
        self.prompt.insert_str(byte_pos, text);
        self.cursor_position += text.chars().count();
        self.history_index = None;
        self.selected_suggestion = 0;
    }
    pub fn matching_suggestions(&self) -> Vec<&'static CommandSuggestion> {
        if !self.prompt.starts_with('/') {
            return Vec::new();
        }
        let query = self.prompt.trim_start_matches('/');
        let mut exact_matches = Vec::new();
        let mut other_matches = Vec::new();
        for cmd in AVAILABLE_COMMANDS {
            let cmd_name = cmd.name.trim_start_matches('/');
            if cmd_name.starts_with(query) {
                exact_matches.push(cmd);
            } else if cmd.name.contains(query) {
                other_matches.push(cmd);
            }
        }
        exact_matches.extend(other_matches);
        exact_matches
    }

    pub fn selected_suggestion_index(&self) -> usize {
        let count = self.suggestion_count();
        if count == 0 {
            0
        } else {
            self.selected_suggestion.min(count - 1)
        }
    }

    pub fn set_terminal_size(&self, width: u16, height: u16) {
        self.terminal_size.set((width, height));
    }

    pub fn last_popup_area(&self) -> Option<Rect> {
        self.last_popup_area.get()
    }

    pub fn set_last_popup_area(&self, area: Option<Rect>) {
        self.last_popup_area.set(area);
    }

    pub fn last_quick_actions_area(&self) -> Option<[Rect; 4]> {
        self.last_quick_actions_area.get()
    }

    pub fn set_last_quick_actions_area(&self, areas: Option<[Rect; 4]>) {
        self.last_quick_actions_area.set(areas);
    }

    pub fn get_or_compute_popup_area(&self) -> Option<Rect> {
        if let Some(area) = self.last_popup_area.get() {
            return Some(area);
        }
        let count = self.suggestion_count();
        if count == 0 {
            return None;
        }
        let (width, height) = self.terminal_size.get();
        if width == 0 || height == 0 {
            return None;
        }
        let workspace_height = height.saturating_sub(1);
        let (input_x, input_y, input_w) = if self.transcript.is_empty() {
            let input_height = 5.min(workspace_height.saturating_sub(6));
            let input_y = workspace_height.saturating_sub(input_height + 2);
            let content_width = if width >= 106 {
                100
            } else {
                width.saturating_sub(4)
            };
            let input_x = width.saturating_sub(content_width) / 2;
            (input_x, input_y, content_width)
        } else {
            let input_y = workspace_height.saturating_sub(6);
            (0, input_y, width)
        };

        let available_space = input_y as usize;
        if available_space < 3 {
            return None;
        }
        let max_visible =
            if self.prompt.starts_with("/theme ") || self.prompt.starts_with("/model ") {
                8.min(available_space.saturating_sub(2))
            } else {
                6.min(available_space.saturating_sub(2))
            };
        let visible_count = count.min(max_visible);
        if visible_count == 0 {
            return None;
        }
        let popup_height = (visible_count as u16) + 2;
        let popup_y = input_y.saturating_sub(popup_height);
        let popup_width = if self.prompt.starts_with("/model ") {
            input_w.min(70)
        } else if self.prompt.starts_with("/theme ") {
            input_w.min(50)
        } else {
            input_w.min(64)
        };

        Some(Rect {
            x: input_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        })
    }

    pub fn get_or_compute_quick_actions_area(&self) -> Option<[Rect; 4]> {
        if let Some(cards) = self.last_quick_actions_area.get() {
            return Some(cards);
        }
        let (width, height) = self.terminal_size.get();
        crate::tui::home::compute_quick_actions_area_for_size(width, height)
    }

    pub fn handle_mouse_click(&mut self, x: u16, y: u16) {
        if let Some(call_id) = self.tool_row_at(x, y) {
            if call_id == "__thought__" {
                self.toggle_thought_expanded();
                return;
            }
            let expandable = self
                .tool_rows
                .iter()
                .find(|row| row.call_id == call_id)
                .is_some_and(|row| row.expandable);
            if expandable {
                self.toggle_tool_expanded(&call_id);
                self.diagnostic = if self.is_tool_expanded(&call_id) {
                    "tool output expanded".to_string()
                } else {
                    "tool output collapsed".to_string()
                };
            }
            return;
        }

        let has_command_suggestions = !self.matching_suggestions().is_empty();
        let has_model_suggestions =
            self.prompt.starts_with("/model ") && !self.matching_model_suggestions().is_empty();
        let has_theme_suggestions =
            self.prompt.starts_with("/theme ") && !self.matching_theme_suggestions().is_empty();

        if (has_command_suggestions || has_model_suggestions || has_theme_suggestions)
            && let Some(popup_area) = self.get_or_compute_popup_area()
            && x >= popup_area.x
            && x < popup_area.x + popup_area.width
            && y >= popup_area.y
            && y < popup_area.y + popup_area.height
        {
            let visible_count = (popup_area.height.saturating_sub(2)) as usize;
            if visible_count == 0 {
                return;
            }
            let rel_row = if y <= popup_area.y + 1 {
                0
            } else {
                ((y.saturating_sub(popup_area.y + 1)) as usize).min(visible_count.saturating_sub(1))
            };
            let selected_idx = self.selected_suggestion_index();
            let scroll_offset = if selected_idx >= visible_count {
                (selected_idx + 1).saturating_sub(visible_count)
            } else {
                0
            };
            let item_idx = scroll_offset + rel_row;

            if has_model_suggestions {
                let model_suggestions = self.matching_model_suggestions();
                if let Some(model_id) = model_suggestions.get(item_idx) {
                    self.prompt = format!("/model {model_id}");
                    self.submit_prompt();
                    return;
                }
            } else if has_theme_suggestions {
                let theme_suggestions = self.matching_theme_suggestions();
                if let Some(theme_name) = theme_suggestions.get(item_idx) {
                    self.prompt = format!("/theme {theme_name}");
                    self.submit_prompt();
                    return;
                }
            } else {
                let suggestions = self.matching_suggestions();
                if let Some(suggestion) = suggestions.get(item_idx) {
                    if suggestion.template.ends_with(' ') {
                        self.prompt = suggestion.template.to_string();
                        self.cursor_position = self.prompt.chars().count();
                        self.selected_suggestion = 0;
                    } else {
                        self.prompt = suggestion.template.to_string();
                        self.submit_prompt();
                    }
                    return;
                }
            }
            return;
        }

        if self.transcript.is_empty()
            && let Some(cards) = self.get_or_compute_quick_actions_area()
        {
            for (idx, card) in cards.iter().enumerate() {
                if x >= card.x && x < card.x + card.width && y >= card.y && y < card.y + card.height
                {
                    self.prompt.clear();
                    self.cursor_position = 0;
                    match idx {
                        0 => {
                            self.set_mode(ConversationMode::Plan);
                            self.diagnostic = "Switched to Plan mode (read-only)".to_string();
                        }
                        1 => {
                            self.set_mode(ConversationMode::Build);
                            self.diagnostic = "Switched to Build mode (edits enabled)".to_string();
                        }
                        2 => {
                            let command = cli::parse_command("/models").expect("valid command");
                            if let Ok(output) = self.command_service.execute(command) {
                                self.apply_command_output(output);
                            }
                        }
                        3 => {
                            self.which_key.show();
                            self.diagnostic =
                                "Shortcuts cheatsheet (Ctrl+X or Esc to dismiss)".to_string();
                        }
                        _ => {}
                    }
                    return;
                }
            }
        }
    }

    pub fn permission_dialog(&self) -> Option<&PermissionDialogState> {
        self.permission_dialog.as_ref()
    }

    pub fn permission_dialog_mut(&mut self) -> Option<&mut PermissionDialogState> {
        self.permission_dialog.as_mut()
    }

    pub fn last_permission_decision(&self) -> Option<PermissionDecision> {
        self.last_permission_decision
    }

    pub fn open_permission_dialog(
        &mut self,
        tool_name: impl Into<String>,
        action_desc: impl Into<String>,
        reason: impl Into<String>,
    ) {
        self.permission_dialog = Some(PermissionDialogState::with_prompt(
            tool_name,
            action_desc,
            reason,
        ));
    }

    pub fn close_permission_dialog(&mut self) {
        self.permission_dialog = None;
    }
    pub fn question_dialog(&self) -> Option<&QuestionDialogState> {
        self.question_dialog.as_ref()
    }

    pub fn question_dialog_mut(&mut self) -> Option<&mut QuestionDialogState> {
        self.question_dialog.as_mut()
    }

    pub fn last_question_answer(&self) -> Option<&str> {
        self.last_question_answer.as_deref()
    }

    pub fn open_question_dialog(&mut self, question: &str, options: Vec<String>) {
        self.question_dialog = Some(QuestionDialogState::new(question, options));
    }

    pub fn close_question_dialog(&mut self) {
        self.question_dialog = None;
    }

    pub fn is_sensitive_command(cmd: &str) -> bool {
        is_sensitive_command(cmd)
    }

    pub fn sessions_dialog(&self) -> Option<&SessionsDialogState> {
        self.sessions_dialog.as_ref()
    }

    pub fn sessions_dialog_mut(&mut self) -> Option<&mut SessionsDialogState> {
        self.sessions_dialog.as_mut()
    }

    pub fn open_sessions_dialog(&mut self) {
        if let Ok(sessions) = self.command_service.list_sessions() {
            self.diagnostic = format!("{} session(s) — press Esc to close", sessions.len());
            self.session_listings = sessions.clone();
            self.sessions_dialog = Some(SessionsDialogState::new(sessions, self.active_session_id));
        } else {
            self.diagnostic = "no sessions found or db unavailable".to_string();
        }
    }

    pub fn models_dialog(&self) -> Option<&ModelsDialogState> {
        self.models_dialog.as_ref()
    }

    pub fn models_dialog_mut(&mut self) -> Option<&mut ModelsDialogState> {
        self.models_dialog.as_mut()
    }

    pub fn agents_dialog(&self) -> Option<&AgentsDialogState> {
        self.agents_dialog.as_ref()
    }

    pub fn agents_dialog_mut(&mut self) -> Option<&mut AgentsDialogState> {
        self.agents_dialog.as_mut()
    }

    pub fn themes_dialog(&self) -> Option<&ThemesDialogState> {
        self.themes_dialog.as_ref()
    }

    pub fn themes_dialog_mut(&mut self) -> Option<&mut ThemesDialogState> {
        self.themes_dialog.as_mut()
    }

    pub fn status_dialog(&self) -> Option<&StatusDialogState> {
        self.status_dialog.as_ref()
    }

    pub fn status_dialog_mut(&mut self) -> Option<&mut StatusDialogState> {
        self.status_dialog.as_mut()
    }

    pub fn open_status_dialog(&mut self) {
        let mode = match self.mode {
            ConversationMode::Plan => "Plan (Read-only)",
            ConversationMode::Build => "Build (Edits enabled)",
        };
        let branch = self.git_branch.clone().unwrap_or_else(|| {
            crate::platform::get_current_branch().unwrap_or_else(|| "detached".into())
        });
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".into());
        let status_str = match self.status {
            ConversationStatus::Idle => "Idle",
            ConversationStatus::Active => "Active (Generating)",
            ConversationStatus::Finished(_) => "Finished",
            ConversationStatus::Cancelled => "Cancelled",
            ConversationStatus::Rejected => "Rejected",
            ConversationStatus::Error => "Error",
        };
        self.status_dialog = Some(
            StatusDialogState::new(
                mode,
                &self.provider,
                &self.model,
                self.theme.name(),
                &branch,
                &cwd,
            )
            .with_details(self.transcript.len(), status_str),
        );
        self.diagnostic = "System status".to_string();
    }

    pub fn theme(&self) -> crate::tui::ThemeKind {
        self.theme
    }

    pub fn set_theme(&mut self, theme: crate::tui::ThemeKind) {
        self.theme = theme;
        let theme_bundle = self.theme.to_theme();
        let mode_color = match self.mode {
            ConversationMode::Plan => theme_bundle.amber,
            ConversationMode::Build => theme_bundle.teal,
        };
        self.wave_spinner.set_color(mode_color);
    }

    pub fn which_key(&self) -> &WhichKeyState {
        &self.which_key
    }

    pub fn which_key_mut(&mut self) -> &mut WhichKeyState {
        &mut self.which_key
    }

    pub fn available_models(&self) -> &[crate::provider::ModelInfo] {
        &self.available_models
    }

    pub fn matching_model_suggestions(&self) -> Vec<String> {
        if !self.prompt.starts_with("/model ") {
            return Vec::new();
        }
        let query = self
            .prompt
            .strip_prefix("/model ")
            .unwrap_or("")
            .trim()
            .to_lowercase();
        self.available_models
            .iter()
            .map(|m| m.id.clone())
            .filter(|id| query.is_empty() || id.to_lowercase().contains(&query))
            .collect()
    }

    pub fn matching_theme_suggestions(&self) -> Vec<&'static str> {
        if !self.prompt.starts_with("/theme ") {
            return Vec::new();
        }
        let query = self
            .prompt
            .strip_prefix("/theme ")
            .unwrap_or("")
            .trim()
            .to_lowercase();
        crate::tui::ThemeKind::ALL
            .iter()
            .filter_map(|t| {
                if query.is_empty()
                    || t.name().to_lowercase().contains(&query)
                    || t.id().to_lowercase().contains(&query)
                {
                    Some(t.name())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn suggestion_count(&self) -> usize {
        if self.prompt.starts_with("/model ") {
            self.matching_model_suggestions().len()
        } else if self.prompt.starts_with("/theme ") {
            self.matching_theme_suggestions().len()
        } else {
            self.matching_suggestions().len()
        }
    }

    pub fn next_suggestion(&mut self) {
        let count = self.suggestion_count();
        if count > 0 {
            self.selected_suggestion = (self.selected_suggestion + 1) % count;
        } else {
            self.selected_suggestion = 0;
        }
    }

    pub fn previous_suggestion(&mut self) {
        let count = self.suggestion_count();
        if count > 0 {
            self.selected_suggestion =
                if self.selected_suggestion == 0 || self.selected_suggestion >= count {
                    count - 1
                } else {
                    self.selected_suggestion - 1
                };
        } else {
            self.selected_suggestion = 0;
        }
    }

    pub fn prompt_history(&self) -> &[String] {
        &self.prompt_history
    }

    pub fn navigate_history_up(&mut self) {
        if self.prompt_history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.draft_prompt = self.prompt.clone();
                let last_idx = self.prompt_history.len() - 1;
                self.history_index = Some(last_idx);
                if let Some(p) = self.prompt_history.get(last_idx) {
                    self.prompt = p.clone();
                    self.cursor_position = self.prompt.chars().count();
                }
            }
            Some(idx) => {
                let valid_idx = idx.min(self.prompt_history.len().saturating_sub(1));
                if valid_idx > 0 {
                    let next_idx = valid_idx - 1;
                    self.history_index = Some(next_idx);
                    if let Some(p) = self.prompt_history.get(next_idx) {
                        self.prompt = p.clone();
                        self.cursor_position = self.prompt.chars().count();
                    }
                } else if let Some(p) = self.prompt_history.first() {
                    self.history_index = Some(0);
                    self.prompt = p.clone();
                    self.cursor_position = self.prompt.chars().count();
                }
            }
        }
    }

    pub fn navigate_history_down(&mut self) {
        if let Some(idx) = self.history_index {
            if idx + 1 < self.prompt_history.len() {
                let next_idx = idx + 1;
                self.history_index = Some(next_idx);
                if let Some(p) = self.prompt_history.get(next_idx) {
                    self.prompt = p.clone();
                    self.cursor_position = self.prompt.chars().count();
                }
            } else {
                self.history_index = None;
                self.prompt = std::mem::take(&mut self.draft_prompt);
                self.cursor_position = self.prompt.chars().count();
            }
        }
    }

    pub fn autocomplete_selected_command(&mut self) -> bool {
        if self.prompt.starts_with("/model ") {
            let model_suggestions = self.matching_model_suggestions();
            let idx = self.selected_suggestion_index();
            if let Some(first) = model_suggestions
                .get(idx)
                .or_else(|| model_suggestions.first())
            {
                self.prompt = format!("/model {first}");
                self.cursor_position = self.prompt.chars().count();
                self.selected_suggestion = 0;
                return true;
            }
        }
        if self.prompt.starts_with("/theme ") {
            let theme_suggestions = self.matching_theme_suggestions();
            let idx = self.selected_suggestion_index();
            if let Some(first) = theme_suggestions
                .get(idx)
                .or_else(|| theme_suggestions.first())
            {
                self.prompt = format!("/theme {first}");
                self.cursor_position = self.prompt.chars().count();
                self.selected_suggestion = 0;
                return true;
            }
        }
        let suggestions = self.matching_suggestions();
        let idx = self.selected_suggestion_index();
        if let Some(suggestion) = suggestions
            .get(idx)
            .copied()
            .or_else(|| suggestions.first().copied())
        {
            self.prompt = suggestion.template.to_string();
            self.cursor_position = self.prompt.chars().count();
            self.selected_suggestion = 0;
            true
        } else {
            false
        }
    }

    fn submit_prompt(&mut self) {
        let suggestions = self.matching_suggestions();
        let model_suggestions = self.matching_model_suggestions();
        let theme_suggestions = self.matching_theme_suggestions();
        let selected_idx = self.selected_suggestion_index();
        let target_suggestion = if !suggestions.is_empty() {
            suggestions
                .get(selected_idx)
                .copied()
                .or_else(|| suggestions.first().copied())
        } else {
            None
        };
        self.selected_suggestion = 0;
        self.cursor_position = 0;
        let mut input = std::mem::take(&mut self.prompt);
        let trimmed_start = input.trim_start();
        if trimmed_start.is_empty() {
            return;
        }
        let is_slash = trimmed_start.starts_with('/');
        let mut trimmed = if is_slash {
            if trimmed_start.starts_with("/theme ") || trimmed_start.starts_with("/model ") {
                trimmed_start
            } else {
                trimmed_start.trim_end()
            }
        } else {
            input.trim()
        };

        if trimmed.starts_with('/') {
            // ponytail: click and autocomplete bounds use layout calculation; add dynamic drag-selection when full mouse text selection needed.
            if let Some(suggestion) = target_suggestion {
                if suggestion.template.ends_with(' ') {
                    if !trimmed.starts_with(suggestion.template) {
                        self.prompt = suggestion.template.to_string();
                        self.cursor_position = self.prompt.chars().count();
                        let usage_arg = if suggestion.name == "/theme" {
                            "<name>"
                        } else {
                            "<title>"
                        };
                        self.diagnostic = format!("usage: {}{usage_arg}", suggestion.template);
                        return;
                    }
                } else if trimmed != suggestion.template {
                    input = suggestion.template.to_string();
                    trimmed = &input;
                }
            }
        }

        self.chat_scroll = 0;
        if self.prompt_history.last().map(|s| s.as_str()) != Some(trimmed) {
            self.prompt_history.push(trimmed.to_string());
            if self.prompt_history.len() > 100 {
                self.prompt_history.remove(0);
            }
        }
        self.history_index = None;
        self.draft_prompt.clear();
        self.rotate_placeholder();

        if trimmed.starts_with('/') {
            if trimmed == "/agents" {
                let current_agent = match self.mode {
                    ConversationMode::Plan => "plan",
                    ConversationMode::Build => "build",
                };
                self.agents_dialog = Some(AgentsDialogState::new(current_agent));
                self.diagnostic = "Select agent mode".to_string();
                return;
            }
            if trimmed == "/themes" {
                self.themes_dialog = Some(ThemesDialogState::new(self.theme));
                self.diagnostic = "Select theme".to_string();
                return;
            }
            if let Some(stripped) = trimmed.strip_prefix("/theme ") {
                let name = stripped.trim();
                let chosen_name = if let Some(&suggestion) = theme_suggestions.get(selected_idx) {
                    Some(suggestion)
                } else if !name.is_empty()
                    && let Some(kind) = crate::tui::ThemeKind::from_name(name)
                {
                    Some(kind.name())
                } else {
                    theme_suggestions.first().copied()
                };
                if let Some(name) = chosen_name {
                    if let Some(kind) = crate::tui::ThemeKind::from_name(name) {
                        self.set_theme(kind);
                        self.diagnostic = format!("theme switched to: {}", kind.name());
                    } else {
                        self.diagnostic = format!("unknown theme: {name} (run /themes to list)");
                    }
                } else {
                    self.diagnostic = "unknown theme (run /themes to list)".to_string();
                }
                return;
            }
            if trimmed == "/keys" {
                self.which_key.show();
                self.diagnostic = "Shortcuts cheatsheet (Ctrl+X or Esc to dismiss)".to_string();
                return;
            }
            if trimmed == "/status" {
                self.open_status_dialog();
                return;
            }
            if trimmed == "/clear" || trimmed == "/home" {
                self.request_runtime_cancel();
                self.reset_turn_view(true);
                self.status = ConversationStatus::Idle;
                self.diagnostic = "screen cleared".to_string();
                return;
            }
            if trimmed == "/compact" {
                self.request_runtime_cancel();
                self.reset_turn_view(false);
                self.status = ConversationStatus::Idle;
                if self.transcript.len() > 1024 {
                    let split_idx = self.transcript.len() - 1024;
                    let boundary = find_user_turn_boundary(&self.transcript, split_idx)
                        .unwrap_or_else(|| ceil_char_boundary(&self.transcript, split_idx));
                    let tail = self.transcript[boundary..].to_string();
                    self.transcript = format!("[earlier transcript compacted]\n{tail}");
                    self.diagnostic = "session context compacted".to_string();
                } else {
                    self.diagnostic = "transcript is already compact".to_string();
                }
                return;
            }
            if trimmed == "/copy" {
                if !self.transcript.is_empty() {
                    match SystemClipboard.set_text(&self.transcript) {
                        Ok(()) => {
                            self.diagnostic = format!(
                                "transcript copied to clipboard ({} bytes)",
                                self.transcript.len()
                            );
                        }
                        Err(err) => {
                            self.diagnostic = format!("failed to copy transcript: {err}");
                        }
                    }
                } else {
                    let status = format!("{} ({})", self.model, self.provider);
                    match SystemClipboard.set_text(&status) {
                        Ok(()) => {
                            self.diagnostic =
                                format!("copied status: {} ({})", self.model, self.provider);
                        }
                        Err(err) => {
                            self.diagnostic = format!("failed to copy status: {err}");
                        }
                    }
                }
                return;
            }
            if let Some(stripped) = trimmed.strip_prefix("/model ") {
                let arg = stripped.trim();
                let chosen = if let Some(suggestion) = model_suggestions.get(selected_idx) {
                    Some(suggestion.clone())
                } else if !arg.is_empty() && self.available_models.iter().any(|m| m.id == arg) {
                    Some(arg.to_string())
                } else {
                    model_suggestions.first().cloned()
                };
                if let Some(chosen) = chosen {
                    if let Some((provider, model)) = split_provider_model(&chosen) {
                        self.provider = bounded(provider.to_string(), MAX_IDENTITY_BYTES);
                        self.model = bounded(model.to_string(), MAX_IDENTITY_BYTES);
                    } else {
                        self.model = bounded(chosen.clone(), MAX_IDENTITY_BYTES);
                    }
                    self.diagnostic = format!("model switched to: {chosen}");
                    return;
                }
            }
            let command = match cli::parse_command(trimmed) {
                Ok(command) => command,
                Err(error) => {
                    self.diagnostic = error;
                    return;
                }
            };
            match self.command_service.execute(command) {
                Ok(CommandOutput::Mode(mode)) => self.set_mode(match mode {
                    CommandMode::Plan => ConversationMode::Plan,
                    CommandMode::Build => ConversationMode::Build,
                }),
                Ok(CommandOutput::Exit) => self.running = false,
                Ok(CommandOutput::SessionCreated(session)) => {
                    self.switch_session(session.id);
                    self.transcript.clear();
                    self.active_tool = None;
                    self.tool_rows.clear();
                    self.active_generation_id = None;
                    self.typewriter.reset();
                    self.chat_scroll = 0;
                    self.diagnostic = format!("session created: {}", session.title);
                }
                Ok(CommandOutput::RefreshStarted) => {
                    self.diagnostic = "model refresh started".into();
                }
                Ok(output) => {
                    self.apply_command_output(output);
                }
                Err(error) => self.diagnostic = error.to_string(),
            }
            if let Some(error) = self.command_service.take_diagnostic() {
                self.diagnostic = error;
            }
            return;
        }

        self.submit_user_prompt(trimmed);
    }

    pub fn submit_user_prompt(&mut self, prompt: &str) {
        if self.status == ConversationStatus::Active || self.active_generation_id.is_some() {
            self.request_runtime_cancel();
        }
        self.reset_turn_view(false);
        self.status = ConversationStatus::Idle;
        self.session_listings.clear();
        if self.runtime.is_some() {
            if prompt.len() > crate::persistence::MAX_MESSAGE_BYTES {
                self.diagnostic = format!(
                    "message too large: {} bytes (max {})",
                    prompt.len(),
                    crate::persistence::MAX_MESSAGE_BYTES
                );
                return;
            }
            if self.active_session_id.is_none()
                && let Some(runtime) = self.runtime.as_ref()
                && let Ok(id) = runtime.create_session(1, prompt)
            {
                self.switch_session(id);
            }
            let Some(session_id) = self.active_session_id else {
                return;
            };

            let provider = if self.provider.is_empty() {
                "anthropic".to_string()
            } else {
                self.provider.clone()
            };
            let model = if self.model.is_empty() {
                "claude-3-7-sonnet".to_string()
            } else {
                self.model.clone()
            };
            let agent_mode = match self.mode {
                ConversationMode::Plan => "plan",
                ConversationMode::Build => "build",
            };

            if let Some(runtime) = self.runtime.as_ref()
                && let Err(error) =
                    runtime.start_generation(session_id, agent_mode, &provider, &model, prompt)
            {
                self.diagnostic = error;
                return;
            }

            self.text_stream_active = false;
            if self.transcript.is_empty() {
                self.transcript.push_str(&format!("> {prompt}\n\n"));
            } else {
                self.transcript.push_str(&format!("\n\n> {prompt}\n\n"));
            }
            self.truncate_transcript();

            self.apply_conversation(ConversationEvent::PromptSubmitted {
                prompt: prompt.to_string(),
                provider,
                model,
            });
        } else {
            if self.active_session_id.is_none()
                && let Ok(session) = self.command_service.create_session(prompt)
            {
                self.switch_session(session.id);
            }
            if let Some(session_id) = self.active_session_id {
                let _ = self
                    .command_service
                    .append_message(session_id, "user", prompt);
            }

            self.text_stream_active = false;
            if self.transcript.is_empty() {
                self.transcript.push_str(&format!("> {prompt}\n\n"));
            } else {
                self.transcript.push_str(&format!("\n\n> {prompt}\n\n"));
            }
            self.truncate_transcript();

            let provider = if self.provider.is_empty() {
                "anthropic".to_string()
            } else {
                self.provider.clone()
            };
            let model = if self.model.is_empty() {
                "claude-3-7-sonnet".to_string()
            } else {
                self.model.clone()
            };

            let is_connected = self.command_service.is_connected() || !self.provider.is_empty();

            self.apply_conversation(ConversationEvent::PromptSubmitted {
                prompt: prompt.to_string(),
                provider: provider.clone(),
                model: model.clone(),
            });

            if !is_connected {
                self.diagnostic = "provider not connected; use /connect".into();
            }
        }
    }

    /// Drain pending runtime events into the live transcript view.
    /// The bounded bus applies backpressure and prunes closed receivers;
    /// this drains pending events to exhaustion each call.
    pub fn poll_runtime(&mut self) -> bool {
        self.command_service.poll_refresh();
        if let Ok(models) = self.command_service.models()
            && !models.is_empty()
            && models != self.available_models
        {
            self.available_models = models;
        }
        let Some(receiver) = self.runtime_events.take() else {
            return false;
        };
        let mut had_events = false;
        while let Ok(event) = receiver.try_recv() {
            had_events = true;
            if let Some(session_id) = self.active_session_id
                && event.session_id != session_id
            {
                continue;
            }
            if event.kind == "generation_started" {
                let Some(generation_id) = event.generation_id else {
                    continue;
                };
                if self.status != ConversationStatus::Active
                    || self.ignored_generation_ids.contains(&generation_id)
                    || self.active_generation_id.is_some()
                {
                    continue;
                }
                self.active_generation_id = Some(generation_id);
                self.stream_parts.clear();
                self.stream_base_len = Some(self.transcript.len());
                self.reasoning_buffer.clear();
            } else {
                let ignored_generation = event.generation_id.is_some_and(|generation_id| {
                    self.ignored_generation_ids.contains(&generation_id)
                });
                let cancellation_completion = ignored_generation
                    && event.kind == "generation_finished"
                    && serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        .ok()
                        .and_then(|payload| {
                            payload
                                .get("status")
                                .and_then(|v| v.as_str())
                                .map(|status| status == "cancelled")
                        })
                        .unwrap_or(false);
                let is_session_event =
                    matches!(event.kind.as_str(), "prompt_admitted" | "prompt_promoted");
                if (ignored_generation && !cancellation_completion)
                    || (!is_session_event
                        && self
                            .active_generation_id
                            .is_some_and(|active_generation_id| {
                                event.generation_id != Some(active_generation_id)
                            }))
                {
                    continue;
                }
            }
            match event.kind.as_str() {
                "finish" => {
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(reason) = payload.get("reason").and_then(|v| v.as_str())
                    {
                        self.pending_finish_reason = parse_finish_reason(reason);
                    }
                }
                "reasoning_delta" => {
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(delta) = payload.get("delta").and_then(|v| v.as_str())
                    {
                        if self.reasoning_start.is_none() {
                            self.reasoning_start = Some(std::time::Instant::now());
                        }
                        self.reasoning_active = true;
                        self.push_stream_part(StreamPart::Reasoning(delta.to_string()));
                        self.reasoning_buffer.push_str(delta);
                    }
                }
                "tool_call_start" => {
                    self.text_stream_active = false;
                    self.reasoning_active = false;
                    self.record_reasoning_duration();
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    {
                        let name = payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        let call_id = payload
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string)
                            .unwrap_or_else(|| format!("{name}:{}", event.seq));
                        self.push_tool_part(call_id.clone());
                        self.upsert_tool_row(
                            &call_id,
                            name,
                            ToolRowState::Pending,
                            "preparing arguments...".to_string(),
                            String::new(),
                            payload.get("metadata").cloned(),
                        );
                    }
                }
                "tool_call_delta" => {
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    {
                        let call_id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let delta = payload
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if let Some(row) =
                            self.tool_rows.iter_mut().find(|row| row.call_id == call_id)
                        {
                            row.arguments = bounded(
                                format!("{}{}", row.arguments, delta),
                                MAX_TOOL_ARGUMENT_BYTES,
                            );
                            if let Ok(args) =
                                serde_json::from_str::<serde_json::Value>(&row.arguments)
                            {
                                let (_, _, desc) = tool_target_and_verbs(&row.name, Some(&args));
                                if !desc.is_empty() {
                                    row.desc = bounded(desc, MAX_TOOL_ARGUMENT_BYTES);
                                }
                            }
                            row.expandable = row.compute_expandable();
                        }
                        self.refresh_active_tool();
                    }
                }
                "tool_call_end" => {
                    let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    else {
                        continue;
                    };
                    let Some(call_id) = payload.get("id").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if let Some(row) = self.tool_rows.iter_mut().find(|row| row.call_id == call_id)
                    {
                        row.arguments_complete = true;
                        if let Some(metadata) = payload.get("metadata") {
                            row.metadata = Some(metadata.clone());
                        }
                        if row.desc == "preparing arguments..."
                            && let Ok(args) =
                                serde_json::from_str::<serde_json::Value>(&row.arguments)
                        {
                            let (_, _, desc) = tool_target_and_verbs(&row.name, Some(&args));
                            if !desc.is_empty() {
                                row.desc = bounded(desc, MAX_TOOL_ARGUMENT_BYTES);
                            }
                        }
                        row.expandable = row.compute_expandable();
                    }
                }
                "text_delta" => {
                    self.reasoning_active = false;
                    self.record_reasoning_duration();
                    self.flush_reasoning();
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(delta) = payload.get("delta").and_then(|v| v.as_str())
                    {
                        self.push_stream_part(StreamPart::Text(delta.to_string()));
                        self.typewriter.push_delta(delta);
                        self.text_stream_active = true;
                    }
                }
                "tool_executing" => {
                    self.text_stream_active = false;
                    self.reasoning_active = false;
                    self.record_reasoning_duration();
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    {
                        let name = payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        let args = payload.get("arguments");
                        let (_verb, _active_verb, mut desc) = tool_target_and_verbs(name, args);
                        if desc.is_empty()
                            && let Some(prev) = &self.active_tool
                            && prev.name == name
                            && !prev.desc.is_empty()
                        {
                            desc = prev.desc.clone();
                        }
                        if desc.is_empty() {
                            desc = "preparing arguments...".to_string();
                        }
                        let call_id = payload
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string)
                            .unwrap_or_else(|| format!("inline:{}:{}", name, event.seq));
                        self.push_tool_part(call_id.clone());
                        let arguments = args.map(serde_json::Value::to_string).unwrap_or_default();
                        self.upsert_tool_row(
                            &call_id,
                            name,
                            ToolRowState::Running,
                            desc.clone(),
                            arguments,
                            payload.get("metadata").cloned(),
                        );
                        self.active_tool = Some(ActiveToolInfo {
                            name: name.to_string(),
                            desc,
                            started_at: std::time::Instant::now(),
                        });
                        if name == "question" {
                            let parsed_args_obj = if let Some(serde_json::Value::String(s)) = args {
                                serde_json::from_str::<serde_json::Value>(s).ok()
                            } else {
                                None
                            };
                            let effective_args = parsed_args_obj.as_ref().or(args);
                            let q_str = effective_args
                                .and_then(|a| a.get("question"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let options = effective_args
                                .and_then(|a| a.get("options"))
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                                        .collect::<Vec<String>>()
                                })
                                .unwrap_or_default();
                            self.open_question_dialog(q_str, options);
                        }
                    }
                }
                "tool_executed" => {
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    {
                        let name = payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        let success = payload
                            .get("success")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                            && payload.get("output").is_some();
                        let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
                        let args = payload.get("arguments");

                        let (verb, _active_verb, target) = tool_target_and_verbs(name, args);

                        let detail = if !success {
                            let err_line = output.lines().next().unwrap_or("error").trim();
                            let cleaned = err_line
                                .strip_prefix(&format!("Error executing {name}: "))
                                .unwrap_or(err_line);
                            format!("failed: {cleaned}")
                        } else {
                            format_tool_success_detail(name, output)
                        };
                        let call_id = payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let existing_id = if !call_id.is_empty() {
                            if self.tool_rows.iter().any(|row| row.call_id == call_id) {
                                Some(call_id.to_string())
                            } else {
                                self.tool_rows
                                    .iter()
                                    .rev()
                                    .find(|row| {
                                        tool_names_match(&row.name, name)
                                            && matches!(
                                                row.state,
                                                ToolRowState::Pending | ToolRowState::Running
                                            )
                                    })
                                    .map(|row| row.call_id.clone())
                            }
                        } else {
                            self.tool_rows
                                .iter()
                                .rev()
                                .find(|row| {
                                    tool_names_match(&row.name, name)
                                        && matches!(
                                            row.state,
                                            ToolRowState::Pending | ToolRowState::Running
                                        )
                                })
                                .or_else(|| {
                                    self.tool_rows.iter().rev().find(|row| {
                                        matches!(
                                            row.state,
                                            ToolRowState::Pending | ToolRowState::Running
                                        )
                                    })
                                })
                                .map(|row| row.call_id.clone())
                        };
                        let part_id = existing_id.clone().unwrap_or_else(|| {
                            if !call_id.is_empty() {
                                call_id.to_string()
                            } else {
                                format!("inline:{}:{}", name, event.seq)
                            }
                        });
                        let had_row = existing_id.is_some()
                            || (!call_id.is_empty()
                                && self.tool_rows.iter().any(|row| row.call_id == call_id));
                        if !had_row {
                            if !call_id.is_empty() {
                                self.upsert_tool_row(
                                    &part_id,
                                    name,
                                    ToolRowState::Running,
                                    target.clone(),
                                    args.map(serde_json::Value::to_string).unwrap_or_default(),
                                    payload.get("metadata").cloned(),
                                );
                            }
                        } else if let Some(row) =
                            self.tool_rows.iter_mut().find(|row| row.call_id == part_id)
                        {
                            row.name = name.to_string();
                            if !target.is_empty()
                                && (row.desc == "preparing arguments..." || row.desc.is_empty())
                            {
                                row.desc = bounded(target.clone(), MAX_TOOL_ARGUMENT_BYTES);
                            }
                            if let Some(a) = args {
                                row.arguments = bounded(a.to_string(), MAX_TOOL_ARGUMENT_BYTES);
                            }
                            row.expandable = row.compute_expandable();
                        }
                        if name == "update_plan" && success {
                            let parsed_args_holder: Option<serde_json::Value> = match args {
                                Some(serde_json::Value::String(s)) => serde_json::from_str(s).ok(),
                                Some(v @ serde_json::Value::Object(_)) => Some(v.clone()),
                                _ => None,
                            };
                            let args_ref = parsed_args_holder.as_ref().or(args);
                            let plan_items: Vec<(String, String)> = args_ref
                                .and_then(|a| a.get("plan"))
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|item| {
                                            let obj = item.as_object()?;
                                            let step = obj
                                                .get("step")
                                                .or_else(|| obj.get("content"))
                                                .or_else(|| obj.get("title"))
                                                .and_then(|v| v.as_str())?
                                                .trim();
                                            if step.is_empty() {
                                                return None;
                                            }
                                            let clean_step =
                                                if let Some((num, rest)) = step.split_once(". ") {
                                                    if !num.is_empty()
                                                        && num.chars().all(|c| c.is_ascii_digit())
                                                    {
                                                        rest.trim()
                                                    } else {
                                                        step
                                                    }
                                                } else {
                                                    step
                                                };

                                            let status = obj
                                                .get("status")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("pending")
                                                .trim()
                                                .to_ascii_lowercase();
                                            let norm_status = match status.as_str() {
                                                "todo" | "open" | "pending" | "not_started"
                                                | "not-started" => "pending",
                                                "in_progress" | "in-progress" | "in progress"
                                                | "doing" | "active" => "in_progress",
                                                "done" | "completed" | "complete" => "completed",
                                                other => other,
                                            };
                                            Some((clean_step.to_string(), norm_status.to_string()))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            self.current_plan = plan_items;
                        }
                        let structured = self.complete_tool_row(&part_id, name, success, output);
                        if structured {
                            self.push_tool_part(part_id);
                        } else {
                            self.stream_parts.retain(
                                |part| !matches!(part, StreamPart::Tool(id) if id == &part_id),
                            );
                            self.tool_rows.retain(|row| row.call_id != part_id);
                            if name == "update_plan" {
                                let parsed_args_holder: Option<serde_json::Value> = match args {
                                    Some(serde_json::Value::String(s)) => {
                                        serde_json::from_str(s).ok()
                                    }
                                    Some(v @ serde_json::Value::Object(_)) => Some(v.clone()),
                                    _ => None,
                                };
                                let args_ref = parsed_args_holder.as_ref().or(args);

                                let explanation = args_ref
                                    .and_then(|a| a.get("explanation"))
                                    .and_then(|v| v.as_str())
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(ToString::to_string);

                                let plan_items: Vec<(String, String)> = args_ref
                                    .and_then(|a| a.get("plan"))
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|item| {
                                                let obj = item.as_object()?;
                                                let step = obj
                                                    .get("step")
                                                    .or_else(|| obj.get("content"))
                                                    .or_else(|| obj.get("title"))
                                                    .and_then(|v| v.as_str())?
                                                    .trim();
                                                if step.is_empty() {
                                                    return None;
                                                }
                                                let clean_step = if let Some((num, rest)) =
                                                    step.split_once(". ")
                                                {
                                                    if !num.is_empty()
                                                        && num.chars().all(|c| c.is_ascii_digit())
                                                    {
                                                        rest.trim()
                                                    } else {
                                                        step
                                                    }
                                                } else {
                                                    step
                                                };

                                                let status = obj
                                                    .get("status")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("pending")
                                                    .trim()
                                                    .to_ascii_lowercase();
                                                let norm_status = match status.as_str() {
                                                    "todo" | "open" | "pending" | "not_started"
                                                    | "not-started" => "pending",
                                                    "in_progress" | "in-progress"
                                                    | "in progress" | "doing" | "active" => {
                                                        "in_progress"
                                                    }
                                                    "done" | "completed" | "complete" => {
                                                        "completed"
                                                    }
                                                    other => other,
                                                };
                                                Some((
                                                    clean_step.to_string(),
                                                    norm_status.to_string(),
                                                ))
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();

                                if success {
                                    self.current_plan = plan_items.clone();
                                }

                                let header = "⬢ Updated Plan";
                                let branch = if !success {
                                    let err_line = output.lines().next().unwrap_or("error").trim();
                                    let cleaned = err_line
                                        .strip_prefix(&format!("Error executing {name}: "))
                                        .unwrap_or(err_line);
                                    format!("  └ failed: {cleaned}")
                                } else if !plan_items.is_empty() {
                                    format!("  └ Plan updated: {} steps", plan_items.len())
                                } else {
                                    format!("  └ {detail}")
                                };

                                let mut snippet = String::new();
                                if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
                                    snippet.push('\n');
                                }
                                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n")
                                {
                                    snippet.push('\n');
                                }
                                snippet.push_str(header);
                                snippet.push('\n');
                                if let Some(exp) = &explanation {
                                    snippet.push_str(&format!("  │ {exp}\n"));
                                }
                                for (i, (step, status)) in plan_items.iter().enumerate() {
                                    let marker = match status.as_str() {
                                        "completed" => "✔",
                                        "in_progress" => "•",
                                        _ => "□",
                                    };
                                    snippet.push_str(&format!("  │ {marker} {}. {step}\n", i + 1));
                                }
                                snippet.push_str(&branch);
                                snippet.push_str("\n\n");
                                self.transcript.push_str(&snippet);
                                self.truncate_transcript();
                            } else if name == "bash" || name == "sh" {
                                let mut cleaned_lines: Vec<&str> = Vec::new();
                                for l in output.lines() {
                                    let t = l.trim();
                                    if t.starts_with("[Process exited with code ")
                                        && t.ends_with(']')
                                    {
                                        continue;
                                    }
                                    if t == "[Command finished with no output]" {
                                        continue;
                                    }
                                    cleaned_lines.push(l);
                                }

                                let clean_cmd = target.strip_prefix("$ ").unwrap_or(&target);
                                let header = format!("$ {clean_cmd}");

                                let mut snippet = String::new();
                                if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
                                    snippet.push('\n');
                                }
                                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n")
                                {
                                    snippet.push('\n');
                                }
                                snippet.push_str(&header);
                                snippet.push('\n');

                                let max_lines = 25;
                                if cleaned_lines.len() <= max_lines {
                                    for l in &cleaned_lines {
                                        snippet.push_str(l);
                                        snippet.push('\n');
                                    }
                                } else {
                                    for l in &cleaned_lines[..max_lines] {
                                        snippet.push_str(l);
                                        snippet.push('\n');
                                    }
                                    snippet.push_str(&format!(
                                        "... ({} more lines)\n",
                                        cleaned_lines.len() - max_lines
                                    ));
                                }
                                if !success && cleaned_lines.is_empty() {
                                    snippet.push_str(&format!("failed: {detail}\n"));
                                }
                                snippet.push('\n');
                                self.transcript.push_str(&snippet);
                                self.truncate_transcript();
                            } else if name == "websearch" {
                                let header = if target.starts_with('"') {
                                    format!("⬢ Searched {target}")
                                } else {
                                    format!("⬢ Searched \"{target}\"")
                                };

                                let mut results = Vec::new();
                                let mut current_title: Option<String> = None;
                                let mut current_url: Option<String> = None;

                                for l in output.lines() {
                                    let trimmed = l.trim();
                                    if let Some((_num, title)) =
                                        crate::tui::chat::split_numbered_result(trimmed)
                                    {
                                        if let Some(t) = current_title.take() {
                                            results
                                                .push((t, current_url.take().unwrap_or_default()));
                                        }
                                        current_title = Some(title.to_string());
                                    } else if let Some(url) = trimmed.strip_prefix("URL: ") {
                                        current_url = Some(url.to_string());
                                    }
                                }
                                if let Some(t) = current_title {
                                    results.push((t, current_url.unwrap_or_default()));
                                }

                                let mut snippet = String::new();
                                if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
                                    snippet.push('\n');
                                }
                                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n")
                                {
                                    snippet.push('\n');
                                }
                                snippet.push_str(&header);
                                snippet.push('\n');

                                if !results.is_empty() {
                                    snippet.push_str(
                                        "  ┌── Results ──────────────────────────────────────\n",
                                    );
                                    for (i, (title, url)) in results.iter().take(5).enumerate() {
                                        snippet.push_str(&format!("  │ {}. {}\n", i + 1, title));
                                        if !url.is_empty() {
                                            snippet.push_str(&format!("  │    URL: {}\n", url));
                                        }
                                    }
                                    if results.len() > 5 {
                                        snippet.push_str(&format!(
                                            "  │ ... ({} more results)\n",
                                            results.len() - 5
                                        ));
                                    }
                                    snippet.push_str("  └───\n");
                                }

                                let branch = if !success {
                                    let err_line = output.lines().next().unwrap_or("error").trim();
                                    let cleaned = err_line
                                        .strip_prefix(&format!("Error executing {name}: "))
                                        .unwrap_or(err_line);
                                    format!("  └ failed: {cleaned}")
                                } else if results.is_empty() {
                                    "  └ 0 results found".to_string()
                                } else if results.len() == 1 {
                                    "  └ 1 result found".to_string()
                                } else {
                                    format!("  └ {} results found", results.len())
                                };

                                snippet.push_str(&branch);
                                snippet.push_str("\n\n");
                                self.transcript.push_str(&snippet);
                                self.truncate_transcript();
                            } else if success && matches!(name, "edit_file" | "edit") {
                                let old_str = args
                                    .and_then(|a| a.get("old_string"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let new_str = args
                                    .and_then(|a| a.get("new_string"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let start_line = output
                                    .split("at line ")
                                    .nth(1)
                                    .and_then(|s| s.split_whitespace().next())
                                    .and_then(|s| s.parse::<usize>().ok())
                                    .unwrap_or(1);
                                let diff = crate::tui::diff::compute_diff(old_str, new_str, 0);
                                let side_by_side = crate::tui::diff::compute_side_by_side_diff(
                                    old_str, new_str, start_line, 20,
                                );
                                let header = if target.is_empty() {
                                    format!("• Edit (+{} -{})", diff.added, diff.removed)
                                } else {
                                    format!("• Edit {target} (+{} -{})", diff.added, diff.removed)
                                };
                                let diff_lines_str =
                                    crate::tui::diff::format_side_by_side_diff(&side_by_side, 40);

                                let mut snippet = String::new();
                                if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
                                    snippet.push('\n');
                                }
                                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n")
                                {
                                    snippet.push('\n');
                                }
                                snippet.push_str(&header);
                                snippet.push('\n');
                                if !diff_lines_str.is_empty() {
                                    snippet.push_str(&diff_lines_str);
                                }
                                snippet.push('\n');
                                self.transcript.push_str(&snippet);
                                self.truncate_transcript();
                            } else if success && matches!(name, "patch" | "apply_patch") {
                                let patch_str = args
                                    .and_then(|a| a.get("patch"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let mut added = 0usize;
                                let mut removed = 0usize;
                                let mut diff_lines = Vec::new();
                                for l in patch_str.lines() {
                                    if l.starts_with('+') && !l.starts_with("+++") {
                                        added += 1;
                                        diff_lines.push(format!("    + {}", &l[1..]));
                                    } else if l.starts_with('-') && !l.starts_with("---") {
                                        removed += 1;
                                        diff_lines.push(format!("    - {}", &l[1..]));
                                    } else if l.starts_with(' ') {
                                        diff_lines.push(format!("    {}", l));
                                    }
                                }
                                let header = if target.is_empty() {
                                    format!("• Applied patch (+{added} -{removed})")
                                } else {
                                    format!("• Applied patch {target} (+{added} -{removed})")
                                };
                                let mut snippet = String::new();
                                if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
                                    snippet.push('\n');
                                }
                                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n")
                                {
                                    snippet.push('\n');
                                }
                                snippet.push_str(&header);
                                snippet.push('\n');
                                for dl in diff_lines.iter().take(25) {
                                    snippet.push_str(dl);
                                    snippet.push('\n');
                                }
                                snippet.push('\n');
                                self.transcript.push_str(&snippet);
                                self.truncate_transcript();
                            } else if matches!(name, "write_file" | "write") {
                                let line_count = args
                                    .and_then(|a| a.get("content"))
                                    .and_then(|v| v.as_str())
                                    .map(|c| c.lines().count())
                                    .unwrap_or_else(|| output.lines().count());
                                let header = if line_count > 0 {
                                    format!("• Write {target} ({line_count} lines)")
                                } else {
                                    format!("• Write {target}")
                                };
                                let branch = format!("  └ {detail}");
                                let mut snippet = String::new();
                                if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
                                    snippet.push('\n');
                                }
                                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n")
                                {
                                    snippet.push('\n');
                                }
                                snippet.push_str(&header);
                                snippet.push('\n');
                                snippet.push_str(&branch);
                                snippet.push_str("\n\n");
                                self.transcript.push_str(&snippet);
                                self.truncate_transcript();
                            } else {
                                let header = if target.is_empty() {
                                    format!("⬢ {verb}")
                                } else {
                                    format!("⬢ {verb} {target}")
                                };
                                let branch = format!("  └ {detail}");

                                let mut snippet = String::new();
                                if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
                                    snippet.push('\n');
                                }
                                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n")
                                {
                                    snippet.push('\n');
                                }
                                snippet.push_str(&header);
                                snippet.push('\n');
                                snippet.push_str(&branch);
                                self.transcript.push_str(&snippet);
                                self.truncate_transcript();
                            }
                        }
                        self.refresh_active_tool();
                    }
                }
                "generation_finished" => {
                    self.flush_reasoning();
                    for row in &mut self.tool_rows {
                        if matches!(row.state, ToolRowState::Pending | ToolRowState::Running) {
                            row.state = ToolRowState::Failed;
                            row.output = "generation ended before tool completion".to_string();
                        }
                    }
                    self.active_generation_id = None;
                    self.refresh_active_tool();
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(status) = payload.get("status").and_then(|v| v.as_str())
                    {
                        self.text_stream_active = false;
                        let finish_reason = payload
                            .get("finish_reason")
                            .and_then(|v| v.as_str())
                            .and_then(parse_finish_reason)
                            .or_else(|| self.pending_finish_reason.take());
                        let target_status = match status {
                            "cancelled" => ConversationStatus::Cancelled,
                            "failed" => ConversationStatus::Error,
                            _ => ConversationStatus::Finished(
                                finish_reason.unwrap_or(FinishReason::Stop),
                            ),
                        };
                        if self.typewriter.is_typing() {
                            self.typewriter.set_pending_status(target_status);
                        } else {
                            self.typewriter.reset();
                            self.status = target_status;
                        }
                    }
                }
                "error" => {
                    self.flush_reasoning();
                    self.flush_typewriter();
                    self.text_stream_active = false;
                    for row in &mut self.tool_rows {
                        if matches!(row.state, ToolRowState::Pending | ToolRowState::Running) {
                            row.state = ToolRowState::Failed;
                        }
                    }
                    self.refresh_active_tool();
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(message) = payload.get("message").and_then(|v| v.as_str())
                    {
                        self.diagnostic = bounded(message.to_string(), MAX_DIAGNOSTIC_BYTES);
                        self.status = ConversationStatus::Error;
                    }
                }
                "prompt_admitted" => {
                    tracing::debug!(
                        session_id = event.session_id,
                        seq = event.seq,
                        "prompt admitted"
                    );
                }
                "prompt_promoted" => {
                    tracing::debug!(
                        session_id = event.session_id,
                        seq = event.seq,
                        "prompt promoted"
                    );
                }
                _ => {}
            }
        }
        // Advance typewriter drain so headless loops and fast polls drain
        if self.typewriter.is_typing() || self.typewriter.pending_status().is_some() {
            if let Some(chunk) = self.typewriter.drain_step() {
                self.transcript.push_str(&chunk);
                self.truncate_transcript();
                had_events = true;
            }
            if !self.typewriter.is_typing()
                && let Some(target) = self.typewriter.take_pending_status()
            {
                self.typewriter.reset();
                self.status = target;
                had_events = true;
            }
        }
        self.runtime_events = Some(receiver);
        had_events
    }

    pub fn active_session_id(&self) -> Option<i64> {
        self.active_session_id
    }

    /// Stash the live view fields into the active session's state.
    fn stash_active(&mut self) {
        self.flush_reasoning();
        self.flush_typewriter();
        let Some(session_id) = self.active_session_id else {
            return;
        };
        let state = self.sessions.entry(session_id).or_default();
        state.transcript = std::mem::take(&mut self.transcript);
        state.input_draft = std::mem::take(&mut self.prompt);
        state.scroll = self.chat_scroll;
        state.status = self.status;
        state.current_plan = self.current_plan.clone();
        state.text_stream_active = self.text_stream_active;
        state.active_generation_id = self.active_generation_id;
        state.tool_rows = std::mem::take(&mut self.tool_rows);
        state.stream_parts = std::mem::take(&mut self.stream_parts);
        state.stream_base_len = self.stream_base_len;
        state.expanded_tool_rows = std::mem::take(&mut self.expanded_tool_rows);
    }

    /// Restore the target session's state into the live view fields.
    fn restore_into_view(&mut self, session_id: i64) {
        let state = self.sessions.entry(session_id).or_default();
        self.transcript = std::mem::take(&mut state.transcript);
        self.prompt = std::mem::take(&mut state.input_draft);
        self.cursor_position = self.prompt.chars().count();
        self.chat_scroll = state.scroll;
        self.status = state.status;
        self.current_plan = state.current_plan.clone();
        self.text_stream_active = state.text_stream_active;
        self.active_generation_id = state.active_generation_id;
        self.stream_parts = std::mem::take(&mut state.stream_parts);
        self.stream_base_len = state.stream_base_len;
        self.expanded_tool_rows = std::mem::take(&mut state.expanded_tool_rows);
        self.tool_rows = std::mem::take(&mut state.tool_rows);
        self.refresh_active_tool();
        self.metrics = None;
        self.diagnostic.clear();
        self.selected_suggestion = 0;

        if self.transcript.is_empty()
            && let Ok(messages) = self.command_service.session_messages(session_id)
        {
            for msg in messages {
                if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
                    if self.transcript.ends_with('\n') {
                        self.transcript.push('\n');
                    } else {
                        self.transcript.push_str("\n\n");
                    }
                }
                if msg.role == "user" {
                    self.transcript
                        .push_str(&format!("> {}\n\n", msg.content.trim()));
                } else if msg.role == "assistant" {
                    self.transcript
                        .push_str(&format!("{}\n\n", msg.content.trim()));
                } else {
                    self.transcript.push_str(&format!(
                        "[{}]: {}\n\n",
                        msg.role,
                        msg.content.trim()
                    ));
                }
            }
            self.truncate_transcript();
            self.scroll_to_bottom();
        }
    }

    /// Switch the live view to `session_id`, stashing the current one first.
    /// View state is per session: drafts, transcripts, and status survive
    /// the round trip.
    pub fn switch_session(&mut self, session_id: i64) {
        if self.active_session_id == Some(session_id) {
            return;
        }
        self.stash_active();
        self.active_session_id = Some(session_id);
        self.restore_into_view(session_id);
    }

    /// Per-session state for `session_id` (creating an empty entry on first
    /// access), independent of which session is live.
    pub fn session_state(&mut self, session_id: i64) -> &mut ClientSessionState {
        self.sessions.entry(session_id).or_default()
    }

    pub fn apply_command_output(&mut self, output: CommandOutput) {
        match output {
            CommandOutput::Connected(provider) => {
                self.provider = bounded(provider.to_string(), MAX_IDENTITY_BYTES);
                self.diagnostic = format!("provider connected: {provider}");
            }
            CommandOutput::ModelSelected(model) => {
                let display_id = model.clone();
                if let Some((provider, m)) = split_provider_model(&model) {
                    self.provider = bounded(provider.to_string(), MAX_IDENTITY_BYTES);
                    self.model = bounded(m.to_string(), MAX_IDENTITY_BYTES);
                } else {
                    let model_to_set = if !self.provider.is_empty() {
                        let prefix = format!("{}/", self.provider);
                        model.strip_prefix(&prefix).unwrap_or(&model).to_string()
                    } else {
                        model
                    };
                    self.model = bounded(model_to_set, MAX_IDENTITY_BYTES);
                }
                self.diagnostic = format!("model switched to: {display_id}");
            }
            CommandOutput::Models(models) => {
                self.available_models = models.clone();
                self.models_dialog = Some(ModelsDialogState::new(models.clone(), &self.model));
                self.diagnostic = format!("{} model(s)", models.len());
            }
            CommandOutput::Sessions(sessions) => {
                // Grouped per workspace for the /sessions panel; rendering
                // order matches the panel layout (workspace, then sessions).
                self.diagnostic = format!("{} session(s) — press Esc to close", sessions.len());
                self.session_listings = sessions.clone();
                self.sessions_dialog =
                    Some(SessionsDialogState::new(sessions, self.active_session_id));
            }
            CommandOutput::Help(text) => {
                if self.transcript.is_empty() {
                    self.transcript.push_str(&text);
                } else {
                    self.transcript.push_str(&format!("\n{text}"));
                }
                self.truncate_transcript();
                self.diagnostic = text;
            }
            _ => {}
        }
    }

    /// Snapshot of sessions for the /sessions panel, most recent first.
    pub fn session_listings(&self) -> &[crate::persistence::Session] {
        &self.session_listings
    }

    /// Attach a provider-backed runtime. Prompt submissions on the active
    /// session are then queued as generations and progress arrives as
    /// runtime events (drained by [`App::poll_runtime`]).
    pub fn attach_runtime(
        &mut self,
        db: crate::persistence::Db,
        writer: crate::persistence::WriterHandle,
        provider: Box<dyn crate::provider::Provider + Send + Sync>,
    ) {
        let bus = EventBus::new();
        let (_subscription_id, receiver) = bus.subscribe(None);
        let client = RuntimeClient::spawn(db, writer, provider, bus);
        self.runtime_events = Some(receiver);
        self.runtime = Some(client);
    }
    pub fn set_runtime_receiver(&mut self, receiver: std::sync::mpsc::Receiver<RuntimeEvent>) {
        self.runtime_events = Some(receiver);
    }

    pub fn set_command_service(
        &mut self,
        service: crate::cli::CommandService<crate::cli::CliDiscovery>,
    ) {
        self.command_service = service;
    }

    pub fn transcript(&self) -> &str {
        &self.transcript
    }
    pub fn current_plan(&self) -> &[(String, String)] {
        &self.current_plan
    }

    fn truncate_transcript(&mut self) {
        if self.transcript.len() <= Self::MAX_TRANSCRIPT_BYTES {
            return;
        }

        let retained_bytes =
            Self::MAX_TRANSCRIPT_BYTES.saturating_sub(Self::TRUNCATION_MARKER.len());
        let start = ceil_char_boundary(
            &self.transcript,
            self.transcript.len().saturating_sub(retained_bytes),
        );

        self.transcript
            .replace_range(..start, Self::TRUNCATION_MARKER);
    }
}
fn parse_finish_reason(value: &str) -> Option<FinishReason> {
    if value.eq_ignore_ascii_case("stop") {
        Some(FinishReason::Stop)
    } else if value.eq_ignore_ascii_case("length") || value.eq_ignore_ascii_case("max_tokens") {
        Some(FinishReason::Length)
    } else if value.eq_ignore_ascii_case("tool_call")
        || value.eq_ignore_ascii_case("tool_calls")
        || value.eq_ignore_ascii_case("tool_use")
    {
        Some(FinishReason::ToolCall)
    } else if value.eq_ignore_ascii_case("error") {
        Some(FinishReason::Error)
    } else {
        None
    }
}

fn bounded(mut value: String, limit: usize) -> String {
    let end = floor_char_boundary(&value, limit.min(value.len()));
    value.truncate(end);
    value
}

#[derive(Debug)]
pub struct UiEventQueue {
    capacity: usize,
    quit: bool,
    cancel: bool,
    events: VecDeque<UiEvent>,
    delta: String,
}

impl UiEventQueue {
    pub const MAX_COALESCED_STREAM_BYTES: usize = 64 * 1024;

    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(3);
        Self {
            capacity,
            quit: false,
            cancel: false,
            events: VecDeque::with_capacity(capacity.saturating_sub(3)),
            delta: String::new(),
        }
    }

    pub fn push(&mut self, event: UiEvent) {
        match event {
            UiEvent::Input(Input::Quit) => self.quit = true,
            UiEvent::Input(Input::Cancel) => self.cancel = true,
            UiEvent::StreamDelta(delta) => {
                let remaining = Self::MAX_COALESCED_STREAM_BYTES.saturating_sub(self.delta.len());
                let end = floor_char_boundary(&delta, remaining.min(delta.len()));
                self.delta.push_str(&delta[..end]);
            }
            event => {
                let max_events = self.capacity.saturating_sub(3);
                while self.events.len() >= max_events && !self.events.is_empty() {
                    self.events.pop_front();
                }
                if max_events > 0 {
                    self.events.push_back(event);
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        usize::from(self.quit)
            + usize::from(self.cancel)
            + self.events.len()
            + usize::from(!self.delta.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn pop_priority(&mut self) -> Option<UiEvent> {
        if std::mem::take(&mut self.quit) {
            Some(UiEvent::Input(Input::Quit))
        } else if std::mem::take(&mut self.cancel) {
            Some(UiEvent::Input(Input::Cancel))
        } else {
            None
        }
    }

    fn drain(&mut self) -> Vec<UiEvent> {
        let mut drained =
            Vec::with_capacity(self.events.len() + usize::from(!self.delta.is_empty()));
        drained.extend(self.events.drain(..));
        if !self.delta.is_empty() {
            drained.push(UiEvent::StreamDelta(std::mem::take(&mut self.delta)));
        }
        drained
    }

    fn clear(&mut self) {
        self.quit = false;
        self.cancel = false;
        self.events.clear();
        self.delta.clear();
    }
}

fn find_user_turn_boundary(value: &str, cutoff: usize) -> Option<usize> {
    let cutoff = floor_char_boundary(value, cutoff);
    let mut in_code_block = false;
    for line in value[..cutoff].split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        }
    }

    let tail = &value[cutoff..];
    let mut offset = 0;
    for line in tail.split_inclusive('\n') {
        let start = cutoff + offset;
        let at_line_start = start == 0 || value.as_bytes()[start - 1] == b'\n';
        if at_line_start && !in_code_block && line.starts_with("> ") {
            return Some(start);
        }
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        }
        offset += line.len();
    }
    None
}

pub(crate) fn floor_char_boundary(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}

fn ceil_char_boundary(value: &str, mut index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    while !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_boundary_helpers_multibyte_and_oob() {
        let text = "🦀 clawcode 🚀 日本語";
        assert_eq!(floor_char_boundary(text, 0), 0);
        assert_eq!(floor_char_boundary(text, 1), 0); // Inside 🦀
        assert_eq!(floor_char_boundary(text, 2), 0);
        assert_eq!(floor_char_boundary(text, 3), 0);
        assert_eq!(floor_char_boundary(text, 4), 4); // After 🦀
        assert_eq!(floor_char_boundary(text, text.len() + 100), text.len());

        assert_eq!(ceil_char_boundary(text, 0), 0);
        assert_eq!(ceil_char_boundary(text, 1), 4); // Rounds up to end of 🦀
        assert_eq!(ceil_char_boundary(text, 2), 4);
        assert_eq!(ceil_char_boundary(text, 3), 4);
        assert_eq!(ceil_char_boundary(text, 4), 4);
        assert_eq!(ceil_char_boundary(text, text.len()), text.len());
        assert_eq!(ceil_char_boundary(text, text.len() + 100), text.len());
    }
    #[test]
    fn test_compact_keeps_complete_utf8_user_turn() {
        let mut app = App {
            transcript: format!(
                "old\n{}\n```rust\n> literal\n```\n> recent prompt\nassistant",
                "x".repeat(1300)
            ),
            prompt: "/compact".to_string(),
            ..App::default()
        };

        app.submit_prompt();

        assert!(
            app.transcript()
                .starts_with("[earlier transcript compacted]\n> recent prompt\n")
        );
        assert!(app.transcript().is_char_boundary(app.transcript().len()));
    }

    #[test]
    fn test_bounded_string_utf8() {
        let text = "🦀 clawcode 🚀".to_string();
        let truncated = bounded(text, 2);
        assert_eq!(truncated, ""); // 2 bytes is inside 4-byte 🦀, clamped to 0

        let text2 = "🦀 clawcode 🚀".to_string();
        let truncated2 = bounded(text2, 4);
        assert_eq!(truncated2, "🦀");
    }

    #[test]
    fn test_ui_event_queue_safety() {
        let mut queue = UiEventQueue::new(0); // Should clamp to >= 3 without panicking
        assert!(queue.capacity() >= 3);

        for i in 0..100 {
            queue.push(UiEvent::Input(Input::Character(
                (b'a' + (i % 26) as u8) as char,
            )));
        }
        assert!(queue.len() <= queue.capacity());
    }

    #[test]
    fn test_prompt_history_navigation_bounds() {
        let mut app = App::default();
        // Empty history
        app.navigate_history_up();
        assert_eq!(app.prompt(), "");
        app.navigate_history_down();
        assert_eq!(app.prompt(), "");

        // Submit some prompts
        app.prompt = "first".to_string();
        app.submit_prompt();
        app.prompt = "second".to_string();
        app.submit_prompt();

        app.navigate_history_up();
        assert_eq!(app.prompt(), "second");
        app.navigate_history_up();
        assert_eq!(app.prompt(), "first");
        app.navigate_history_up(); // Should clamp, not panic
        assert_eq!(app.prompt(), "first");
        app.navigate_history_down();
        assert_eq!(app.prompt(), "second");
        app.navigate_history_down();
        assert_eq!(app.prompt(), "");
    }

    #[test]
    fn test_suggestion_navigation_bounds() {
        let mut app = App::default();
        assert_eq!(app.selected_suggestion_index(), 0);
        app.previous_suggestion();
        assert_eq!(app.selected_suggestion_index(), 0);
        app.next_suggestion();
        assert_eq!(app.selected_suggestion_index(), 0);

        app.prompt = "/m".to_string();
        let count = app.suggestion_count();
        if count > 0 {
            app.next_suggestion();
            assert!(app.selected_suggestion_index() < count);
            app.previous_suggestion();
            assert!(app.selected_suggestion_index() < count);
        }
    }

    #[test]
    fn test_truncate_transcript_multibyte() {
        let mut app = App::default();
        // Fill with Japanese text
        let chunk = "こんにちは世界！🦀\n";
        let mut s = String::new();
        while s.len() < App::MAX_TRANSCRIPT_BYTES + 1000 {
            s.push_str(chunk);
        }
        app.apply(UiEvent::StreamDelta(s));
        assert!(app.transcript().len() <= App::MAX_TRANSCRIPT_BYTES);
        assert!(app.transcript().starts_with(App::TRUNCATION_MARKER));
    }

    #[test]
    fn test_copy_command_writes_to_clipboard() {
        let mut app = App {
            transcript: "hello transcript".to_string(),
            prompt: "/copy".to_string(),
            ..App::default()
        };
        app.submit_prompt();
        assert!(
            app.diagnostic.contains("transcript copied to clipboard")
                || app.diagnostic.contains("failed to copy transcript")
        );

        let mut app_empty = App {
            prompt: "/copy".to_string(),
            ..App::default()
        };
        app_empty.submit_prompt();
        assert!(
            app_empty.diagnostic.contains("copied status")
                || app_empty.diagnostic.contains("failed to copy status")
        );
    }

    #[test]
    fn test_paste_input_inserts_at_cursor() {
        let mut app = App::default();
        app.apply(UiEvent::Paste("hello world".to_string()));
        assert_eq!(app.prompt(), "hello world");
        assert_eq!(app.cursor_position(), 11);

        app.apply(UiEvent::Input(Input::Left));
        app.apply(UiEvent::Input(Input::Left));
        app.apply(UiEvent::Paste("!".to_string()));
        assert_eq!(app.prompt(), "hello wor!ld");
    }

    #[test]
    fn test_theme_autocompletion_and_no_mode_toggle() {
        let mut app = App::default();
        let original_mode = app.mode();
        app.prompt = "/theme ".to_string();
        app.cursor_position = app.prompt.chars().count();
        let suggestions = app.matching_theme_suggestions();
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0], "Clawcode Dark");

        app.apply(UiEvent::Input(Input::ToggleMode));
        assert_eq!(app.mode(), original_mode);
        assert!(app.prompt().starts_with("/theme "));
        assert_eq!(app.prompt(), "/theme Clawcode Dark");
    }

    #[test]
    fn test_cursor_navigation_and_editing() {
        let mut app = App::default();
        for c in "hello".chars() {
            app.apply(UiEvent::Input(Input::Character(c)));
        }
        assert_eq!(app.prompt(), "hello");
        assert_eq!(app.cursor_position(), 5);

        app.apply(UiEvent::Input(Input::Left));
        assert_eq!(app.cursor_position(), 4);

        app.apply(UiEvent::Input(Input::Character('X')));
        assert_eq!(app.prompt(), "hellXo");
        assert_eq!(app.cursor_position(), 5);

        app.apply(UiEvent::Input(Input::Home));
        assert_eq!(app.cursor_position(), 0);

        app.apply(UiEvent::Input(Input::Right));
        assert_eq!(app.cursor_position(), 1);

        app.apply(UiEvent::Input(Input::Backspace));
        assert_eq!(app.prompt(), "ellXo");
        assert_eq!(app.cursor_position(), 0);

        app.apply(UiEvent::Input(Input::End));
        assert_eq!(app.cursor_position(), 5);
    }

    #[test]
    fn test_delete_session_in_sessions_dialog() {
        let mut app = App::default();
        let session = app
            .command_service
            .create_session("delete me session")
            .unwrap();
        let id = session.id;
        let title = session.title.clone();
        app.sessions_dialog = Some(SessionsDialogState::new(vec![session], Some(id)));
        assert_eq!(app.sessions_dialog.as_ref().unwrap().items.len(), 1);

        app.apply(UiEvent::Input(Input::Character('d')));
        assert_eq!(app.sessions_dialog.as_ref().unwrap().items.len(), 0);
        assert_eq!(app.diagnostic(), format!("session deleted: {title}"));
    }

    #[test]
    fn test_tool_row_expandable_and_click() {
        let mut app = App::default();
        app.upsert_tool_row(
            "bash-short",
            "bash",
            ToolRowState::Running,
            "echo 1".to_string(),
            String::new(),
            None,
        );
        app.complete_tool_row("bash-short", "bash", true, "line 1\nline 2");
        assert!(!app.tool_rows[0].expandable);

        app.upsert_tool_row(
            "bash-long",
            "bash",
            ToolRowState::Running,
            "echo many".to_string(),
            String::new(),
            None,
        );
        let long_output = (1..=12)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.complete_tool_row("bash-long", "bash", true, &long_output);
        assert!(app.tool_rows[1].expandable);

        app.set_tool_row_clicks(vec![
            (
                "bash-short".to_string(),
                ratatui::layout::Rect {
                    x: 0,
                    y: 10,
                    width: 50,
                    height: 1,
                },
            ),
            (
                "bash-long".to_string(),
                ratatui::layout::Rect {
                    x: 0,
                    y: 15,
                    width: 50,
                    height: 1,
                },
            ),
        ]);

        app.handle_mouse_click(5, 10);
        assert!(!app.is_tool_expanded("bash-short"));

        app.handle_mouse_click(5, 15);
        assert!(app.is_tool_expanded("bash-long"));
        assert_eq!(app.diagnostic(), "tool output expanded");

        app.handle_mouse_click(5, 15);
        assert!(!app.is_tool_expanded("bash-long"));
        assert_eq!(app.diagnostic(), "tool output collapsed");
    }
    #[test]
    fn cancel_blocks_stale_same_generation_events() {
        let mut app = App::default();
        let (tx, rx) = std::sync::mpsc::channel();
        app.set_runtime_receiver(rx);
        app.submit_user_prompt("first");
        let session_id = app.active_session_id().unwrap();

        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(7),
            seq: 1,
            kind: "generation_started".into(),
            payload_json: "{}".into(),
        })
        .unwrap();
        app.poll_runtime();
        assert_eq!(app.active_generation_id, Some(7));

        app.apply(UiEvent::Input(Input::Cancel));
        assert_eq!(app.conversation_status(), ConversationStatus::Active);

        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(7),
            seq: 2,
            kind: "generation_started".into(),
            payload_json: "{}".into(),
        })
        .unwrap();
        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(7),
            seq: 3,
            kind: "text_delta".into(),
            payload_json: serde_json::json!({"delta": "stale"}).to_string(),
        })
        .unwrap();
        app.poll_runtime();

        assert_eq!(app.conversation_status(), ConversationStatus::Active);
        assert_eq!(app.active_generation_id, None);
        assert!(app.stream_parts.is_empty());
        assert!(!app.transcript().contains("stale"));
    }

    #[test]
    fn generation_started_does_not_bypass_generation_mismatch_guard() {
        let mut app = App::default();
        let (tx, rx) = std::sync::mpsc::channel();
        app.set_runtime_receiver(rx);
        app.submit_user_prompt("first");
        let session_id = app.active_session_id().unwrap();

        for (seq, generation_id, kind, payload_json) in [
            (1, 7, "generation_started", "{}".to_string()),
            (
                2,
                7,
                "text_delta",
                serde_json::json!({"delta": "kept"}).to_string(),
            ),
            (3, 8, "generation_started", "{}".to_string()),
        ] {
            tx.send(RuntimeEvent {
                session_id,
                generation_id: Some(generation_id),
                seq,
                kind: kind.into(),
                payload_json,
            })
            .unwrap();
        }
        app.poll_runtime();

        assert_eq!(app.active_generation_id, Some(7));
        assert!(matches!(app.stream_parts.as_slice(), [StreamPart::Text(text)] if text == "kept"));
    }

    #[test]
    fn cancelled_generation_followed_by_prompt_starts_fresh_active_turn() {
        let mut app = App::default();
        let (tx, rx) = std::sync::mpsc::channel();
        app.set_runtime_receiver(rx);
        app.submit_user_prompt("first");
        let session_id = app.active_session_id().unwrap();
        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(7),
            seq: 1,
            kind: "generation_started".into(),
            payload_json: "{}".into(),
        })
        .unwrap();
        app.poll_runtime();
        app.apply(UiEvent::Input(Input::Cancel));

        app.submit_user_prompt("second");
        assert_eq!(app.conversation_status(), ConversationStatus::Active);

        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(7),
            seq: 2,
            kind: "text_delta".into(),
            payload_json: serde_json::json!({"delta": "stale"}).to_string(),
        })
        .unwrap();
        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(8),
            seq: 3,
            kind: "generation_started".into(),
            payload_json: "{}".into(),
        })
        .unwrap();
        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(8),
            seq: 4,
            kind: "text_delta".into(),
            payload_json: serde_json::json!({"delta": "fresh"}).to_string(),
        })
        .unwrap();
        app.poll_runtime();

        assert_eq!(app.conversation_status(), ConversationStatus::Active);
        assert_eq!(app.active_generation_id, Some(8));
        assert!(matches!(app.stream_parts.as_slice(), [StreamPart::Text(text)] if text == "fresh"));
    }

    #[test]
    fn clear_and_commands_reset_all_turn_view_state() {
        for command in ["/clear", "/home", "/compact"] {
            let mut app = App {
                transcript: "x".repeat(2048),
                prompt: command.to_string(),
                status: ConversationStatus::Active,
                active_generation_id: Some(7),
                text_stream_active: true,
                stream_parts: vec![StreamPart::Text("stale".into())],
                stream_base_len: Some(1),
                reasoning_buffer: "thinking".into(),
                reasoning_start: Some(std::time::Instant::now()),
                reasoning_duration: Some(std::time::Duration::from_secs(1)),
                reasoning_active: true,
                current_plan: vec![("step".into(), "pending".into())],
                chat_scroll: 9,
                thought_expanded: true,
                expanded_tool_rows: HashSet::from(["call".into()]),
                ..App::default()
            };
            app.upsert_tool_row(
                "call",
                "bash",
                ToolRowState::Running,
                "echo stale".into(),
                String::new(),
                None,
            );
            app.active_tool = Some(ActiveToolInfo {
                name: "bash".into(),
                desc: "echo stale".into(),
                started_at: std::time::Instant::now(),
            });

            app.submit_prompt();

            assert!(app.stream_parts.is_empty(), "{command}");
            assert_eq!(app.stream_base_len, None, "{command}");
            assert!(!app.typewriter.is_active(), "{command}");
            assert!(!app.text_stream_active, "{command}");
            assert!(
                app.reasoning_buffer.is_empty(),
                "{command}: {:?}",
                app.reasoning_buffer
            );
            assert!(app.reasoning_start.is_none(), "{command}");
            assert!(app.reasoning_duration.is_none(), "{command}");
            assert!(!app.reasoning_active, "{command}");
            assert!(app.active_tool.is_none(), "{command}");
            assert!(app.tool_rows.is_empty(), "{command}");
            assert!(app.expanded_tool_rows.is_empty(), "{command}");
            assert!(!app.thought_expanded, "{command}");
            assert!(app.current_plan.is_empty(), "{command}");
            assert_eq!(app.chat_scroll, 0, "{command}");
        }
    }

    #[test]
    fn fresh_prompt_after_cancel_accepts_prompt_submitted() {
        let mut app = App::default();
        app.submit_user_prompt("first");
        app.status = ConversationStatus::Cancelled;
        app.active_generation_id = Some(7);
        app.text_stream_active = true;
        app.stream_parts = vec![StreamPart::Text("stale".into())];
        app.stream_base_len = Some(1);
        app.reasoning_buffer = "thinking".into();
        app.reasoning_active = true;
        app.current_plan = vec![("step".into(), "pending".into())];
        app.thought_expanded = true;
        app.upsert_tool_row(
            "call",
            "bash",
            ToolRowState::Running,
            "echo stale".into(),
            String::new(),
            None,
        );

        app.submit_user_prompt("fresh");

        assert_eq!(app.conversation_status(), ConversationStatus::Active);
        assert_eq!(app.active_generation_id, None);
        assert!(app.stream_parts.is_empty());
        assert!(app.tool_rows.is_empty());
        assert!(app.current_plan.is_empty());
        assert!(!app.thought_expanded);
        assert!(!app.text_stream_active);
    }

    #[test]
    fn runtime_finish_reason_survives_generation_finished() {
        let mut app = App::default();
        let (tx, rx) = std::sync::mpsc::channel();
        app.set_runtime_receiver(rx);
        app.submit_user_prompt("finish with length");
        let session_id = app.active_session_id().unwrap();
        for (seq, kind, payload_json) in [
            (1, "generation_started", "{}".to_string()),
            (
                2,
                "finish",
                serde_json::json!({"reason": "Length"}).to_string(),
            ),
            (
                3,
                "generation_finished",
                serde_json::json!({"status": "completed", "finish_reason": "Length"}).to_string(),
            ),
        ] {
            tx.send(RuntimeEvent {
                session_id,
                generation_id: Some(1),
                seq,
                kind: kind.into(),
                payload_json,
            })
            .unwrap();
        }
        app.poll_runtime();

        assert_eq!(
            app.conversation_status(),
            ConversationStatus::Finished(FinishReason::Length)
        );
    }
}
