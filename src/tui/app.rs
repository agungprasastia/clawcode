use std::collections::VecDeque;

use crate::cli::{self, CommandOutput, ConversationMode as CommandMode};
use crate::conversation::ConversationEvent;
use crate::notify::{BestEffortNotifier, Notification, NotificationKind, Notifier};
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveToolInfo {
    pub name: String,
    pub desc: String,
    pub started_at: std::time::Instant,
}

pub fn tool_target_and_verbs(name: &str, args: Option<&serde_json::Value>) -> (&'static str, &'static str, String) {
    let desc = match (name, args) {
        ("read_file" | "write_file" | "edit_file", Some(a)) => {
            a.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        ("bash", Some(a)) => {
            a.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string()
        }
        ("question", Some(a)) => {
            a.get("question").and_then(|q| q.as_str()).unwrap_or("").to_string()
        }
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
        ("glob_search", Some(a)) => {
            a.get("pattern").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        ("grep_search", Some(a)) => {
            a.get("query").and_then(|q| q.as_str()).unwrap_or("").to_string()
        }
        ("list_dir", Some(a)) => {
            a.get("path").and_then(|p| p.as_str()).unwrap_or(".").to_string()
        }
        ("webfetch", Some(a)) => {
            a.get("url").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        ("websearch", Some(a)) => {
            a.get("query").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
        ("skill", Some(a)) => {
            a.get("name").and_then(|p| p.as_str()).unwrap_or("").to_string()
        }
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
        "read_file" => ("Read", "Reading", desc),
        "write_file" => ("Write", "Writing", desc),
        "edit_file" => ("Edit", "Editing", desc),
        "list_dir" => ("List", "Listing", desc),
        "glob_search" => ("Glob", "Running glob_search", desc),
        "grep_search" => ("Grep", "Running grep_search", desc),
        "bash" => ("Ran", "Running", desc),
        "question" => ("Ask", "Asking", desc),
        "update_plan" => ("Updated Plan", "Updating Plan", desc),
        "webfetch" => ("Fetched", "Fetching", desc),
        "websearch" => ("Searched", "Searching", desc),
        "skill" => ("Loaded skill", "Loading skill", desc),
        _ => ("Tool", "Running", desc),
    }
}

pub fn format_tool_success_detail(name: &str, output: &str) -> String {
    match name {
        "read_file" => {
            let lines = output.lines().count();
            if lines == 1 {
                "1 line".to_string()
            } else {
                format!("{lines} lines")
            }
        }
        "grep_search" => {
            let lines = output.lines().count();
            if lines == 0 || output.trim().is_empty() {
                "0 matches".to_string()
            } else if lines == 1 {
                "1 line".to_string()
            } else {
                format!("{lines} lines")
            }
        }
        "glob_search" => "succeeded".to_string(),
        "list_dir" => {
            let count = output.lines().count();
            if count == 1 {
                "1 entry".to_string()
            } else {
                format!("{count} entries")
            }
        }
        "write_file" => {
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
        "edit_file" => "succeeded".to_string(),
        "bash" => {
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
pub fn is_sensitive_command(cmd: &str) -> bool {
    let lower = cmd.trim().to_lowercase();
    const PATTERNS: &[&str] = &[
        "rm ", "rm\t", "git reset", "git clean", "mkfs", "dd ", "dd\t", "kill ", "kill\t",
        "chmod ", "chmod\t", "chown ", "chown\t",
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
    reasoning_buffer: String,
    reasoning_start: Option<std::time::Instant>,
    /// Snapshot of latest task plan: list of (step, status).
    pub current_plan: Vec<(String, String)>,
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
    pub loading: bool,
    pub current_plan: Vec<(String, String)>,
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
            reasoning_buffer: String::new(),
            reasoning_start: None,
            current_plan: Vec::new(),
        }
    }

    pub const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024;
    pub const TRUNCATION_MARKER: &str = "[earlier transcript truncated]\n";

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
                            PermissionDecision::AllowOnce => "Permission granted (once)".to_string(),
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
                    if let Some(dialog) = &mut self.question_dialog {
                        if dialog.typing_custom || dialog.selected_option == dialog.options.len() {
                            dialog.push_char(c);
                        }
                    }
                }
                UiEvent::Input(Input::Backspace) => {
                    if let Some(dialog) = &mut self.question_dialog {
                        if dialog.typing_custom || dialog.selected_option == dialog.options.len() {
                            dialog.pop_char();
                        }
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
                        self.theme = chosen;
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
                            self.selected_suggestion = 0;
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
                        self.diagnostic = format!("switched to session #{} ({})", chosen.id, chosen.title);
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
                        self.model = bounded(chosen_id.clone(), MAX_IDENTITY_BYTES);
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

        match event {
            UiEvent::Input(Input::Quit) => {
                if self.session_listings.is_empty() {
                    self.running = false;
                } else {
                    self.session_listings.clear();
                }
            }
            UiEvent::Input(Input::Cancel) => {
                self.cancellation_pending = true;
                self.active_tool = None;
                if let Some(runtime) = self.runtime.as_ref()
                    && let Some(session_id) = self.active_session_id
                {
                    let _ = runtime.cancel_generation(session_id);
                }
            }
            UiEvent::Input(Input::Clear) => {
                self.transcript.clear();
                self.chat_scroll = 0;
                self.status = ConversationStatus::Idle;
                self.active_tool = None;
                self.diagnostic = "screen cleared".to_string();
            }
            UiEvent::Input(Input::Character(character)) => {
                self.history_index = None;
                self.prompt.push(character);
                self.selected_suggestion = 0;
            }
            UiEvent::Input(Input::Backspace) => {
                self.history_index = None;
                self.prompt.pop();
                self.selected_suggestion = 0;
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
                self.scroll_to_top();
            }
            UiEvent::Input(Input::End) => {
                self.scroll_to_bottom();
            }
            UiEvent::Input(Input::Left) | UiEvent::Input(Input::Right) => {}
            UiEvent::Input(Input::Submit) => self.submit_prompt(),
            UiEvent::Input(Input::WhichKey) => {
                self.which_key.toggle();
            }
            UiEvent::Input(Input::ToggleMode) => {
                if !self.matching_suggestions().is_empty() || self.prompt.starts_with("/model ") {
                    self.autocomplete_selected_command();
                } else {
                    self.toggle_mode();
                }
            }
            UiEvent::Resize { .. } => {}
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
                self.typewriter.start_stream();
            }
            ConversationEvent::TextDelta(delta) => {
                if self.status == ConversationStatus::Active {
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

    /// Whether text is actively streaming/typing out.
    pub fn is_typing(&self) -> bool {
        self.typewriter.is_typing() || matches!(self.status, ConversationStatus::Active)
    }

    /// Whether the typewriter buffer has pending characters, active status, or a tool is running.
    pub fn is_streaming_active(&self) -> bool {
        matches!(self.status, ConversationStatus::Active)
            || self.typewriter.is_typing()
            || self.typewriter.pending_status().is_some()
            || self.is_reasoning()
            || self.active_tool.is_some()
    }

    pub fn reasoning_buffer(&self) -> &str {
        &self.reasoning_buffer
    }

    pub fn is_reasoning(&self) -> bool {
        !self.reasoning_buffer.is_empty() && self.active_tool.is_none()
    }

    pub fn reasoning_elapsed_seconds(&self) -> Option<f64> {
        self.reasoning_start.map(|t| t.elapsed().as_secs_f64())
    }

    pub fn flush_reasoning(&mut self) {
        if self.reasoning_buffer.trim().is_empty() {
            self.reasoning_buffer.clear();
            self.reasoning_start = None;
            return;
        }
        let duration = self
            .reasoning_start
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        let dur_str = format!("{:.1}s", duration.max(0.1));
        let reasoning = self.reasoning_buffer.trim();
        let raw_lines: Vec<&str> = reasoning.lines().collect();

        let mut snippet = String::new();
        if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
            snippet.push('\n');
        }
        if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
            snippet.push('\n');
        }
        snippet.push_str(&format!("💭 Thought for {dur_str}\n"));
        if raw_lines.len() <= 10 {
            for l in raw_lines {
                snippet.push_str(&format!("  │ {l}\n"));
            }
        } else {
            for l in &raw_lines[..5] {
                snippet.push_str(&format!("  │ {l}\n"));
            }
            snippet.push_str("  │ ...\n");
        }
        snippet.push('\n');
        self.transcript.push_str(&snippet);
        self.truncate_transcript();
        self.reasoning_buffer.clear();
        self.reasoning_start = None;
    }

    pub fn active_tool(&self) -> Option<&ActiveToolInfo> {
        self.active_tool.as_ref()
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
        self.selected_suggestion
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
        let query = self.prompt[7..].trim().to_lowercase();
        self.available_models
            .iter()
            .map(|m| m.id.clone())
            .filter(|id| query.is_empty() || id.to_lowercase().contains(&query))
            .collect()
    }

    pub fn suggestion_count(&self) -> usize {
        if self.prompt.starts_with("/model ") {
            self.matching_model_suggestions().len()
        } else {
            self.matching_suggestions().len()
        }
    }

    pub fn next_suggestion(&mut self) {
        let count = self.suggestion_count();
        if count > 0 {
            self.selected_suggestion = (self.selected_suggestion + 1) % count;
        }
    }

    pub fn previous_suggestion(&mut self) {
        let count = self.suggestion_count();
        if count > 0 {
            self.selected_suggestion = if self.selected_suggestion == 0 {
                count - 1
            } else {
                self.selected_suggestion - 1
            };
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
                self.prompt = self.prompt_history[last_idx].clone();
            }
            Some(idx) => {
                if idx > 0 {
                    let next_idx = idx - 1;
                    self.history_index = Some(next_idx);
                    self.prompt = self.prompt_history[next_idx].clone();
                }
            }
        }
    }

    pub fn navigate_history_down(&mut self) {
        if let Some(idx) = self.history_index {
            if idx + 1 < self.prompt_history.len() {
                let next_idx = idx + 1;
                self.history_index = Some(next_idx);
                self.prompt = self.prompt_history[next_idx].clone();
            } else {
                self.history_index = None;
                self.prompt = std::mem::take(&mut self.draft_prompt);
            }
        }
    }

    pub fn autocomplete_selected_command(&mut self) -> bool {
        if self.prompt.starts_with("/model ") {
            let model_suggestions = self.matching_model_suggestions();
            if let Some(first) = model_suggestions.get(self.selected_suggestion) {
                self.prompt = format!("/model {first}");
                self.selected_suggestion = 0;
                return true;
            }
        }
        let suggestions = self.matching_suggestions();
        if let Some(suggestion) = suggestions.get(self.selected_suggestion) {
            self.prompt = suggestion.template.to_string();
            self.selected_suggestion = 0;
            true
        } else {
            false
        }
    }

    fn submit_prompt(&mut self) {
        let suggestions = self.matching_suggestions();
        let selected_suggestion = if !suggestions.is_empty() {
            suggestions.get(self.selected_suggestion).copied()
        } else {
            None
        };
        let was_suggestion_focused = self.selected_suggestion > 0;
        self.selected_suggestion = 0;
        let input = std::mem::take(&mut self.prompt);
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return;
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
                if let Some(kind) = crate::tui::ThemeKind::from_name(name) {
                    self.theme = kind;
                    self.diagnostic = format!("theme switched to: {}", kind.name());
                } else {
                    self.diagnostic = format!("unknown theme: {name} (run /themes to list)");
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
                self.transcript.clear();
                self.status = ConversationStatus::Idle;
                self.active_tool = None;
                self.diagnostic = "screen cleared".to_string();
                return;
            }
            if trimmed == "/compact" {
                if self.transcript.len() > 1024 {
                    let keep_bytes = 1024.min(self.transcript.len());
                    let split_idx = self.transcript.len() - keep_bytes;
                    let mut boundary = split_idx;
                    while boundary < self.transcript.len()
                        && !self.transcript.is_char_boundary(boundary)
                    {
                        boundary += 1;
                    }
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
                    self.diagnostic =
                        format!("transcript copied ({} bytes)", self.transcript.len());
                } else {
                    self.diagnostic = format!("copied status: {} ({})", self.model, self.provider);
                }
                return;
            }
            if let Some(stripped) = trimmed.strip_prefix("/model ") {
                let arg = stripped.trim();
                if arg.is_empty() {
                    let model_suggestions = self.matching_model_suggestions();
                    if let Some(first) = model_suggestions.get(self.selected_suggestion)
                        && was_suggestion_focused
                    {
                        let chosen = first.clone();
                        self.model = bounded(chosen.clone(), MAX_IDENTITY_BYTES);
                        self.diagnostic = format!("model switched to: {chosen}");
                        return;
                    }
                }
            }
            let command = match cli::parse_command(trimmed) {
                Ok(command) => command,
                Err(error) => {
                    if let Some(suggestion) = selected_suggestion
                        && (trimmed.len() > 1 || was_suggestion_focused)
                    {
                        if suggestion.template.ends_with(' ') {
                            self.prompt = suggestion.template.to_string();
                            self.diagnostic = format!("usage: {}<title>", suggestion.template);
                            return;
                        } else if let Ok(command) = cli::parse_command(suggestion.template) {
                            command
                        } else {
                            self.diagnostic = error;
                            return;
                        }
                    } else {
                        self.diagnostic = error;
                        return;
                    }
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
        self.flush_reasoning();
        self.flush_typewriter();
        self.session_listings.clear();
        if self.active_session_id.is_none() {
            if let Some(runtime) = self.runtime.as_ref() {
                if let Ok(id) = runtime.create_session(1, prompt) {
                    self.switch_session(id);
                }
            } else if let Ok(session) = self.command_service.create_session(prompt) {
                self.switch_session(session.id);
            }
        }
        if let Some(session_id) = self.active_session_id {
            let _ = self
                .command_service
                .append_message(session_id, "user", prompt);
        }

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

        if let Some(runtime) = self.runtime.as_ref()
            && let Some(session_id) = self.active_session_id
        {
            let agent_mode = match self.mode {
                ConversationMode::Plan => "plan",
                ConversationMode::Build => "build",
            };
            if let Err(error) =
                runtime.start_generation(session_id, agent_mode, &provider, &model, prompt)
            {
                self.diagnostic = format!("generation failed to start: {error}");
                self.status = ConversationStatus::Error;
            }
        } else if !is_connected {
            self.diagnostic = "provider not connected; use /connect".into();
        }
    }

    /// Drain pending runtime events into the live transcript view.
    /// Unbounded growth is impossible: the bus prunes closed/full receivers,
    /// and this drains to exhaustion each call.
    pub fn poll_runtime(&mut self) -> bool {
        let Some(receiver) = self.runtime_events.take() else {
            return false;
        };
        let mut had_events = false;
        while let Ok(event) = receiver.try_recv() {
            had_events = true;
            match event.kind.as_str() {
                "reasoning_delta" => {
                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(delta) = payload.get("delta").and_then(|v| v.as_str())
                    {
                        if self.reasoning_start.is_none() {
                            self.reasoning_start = Some(std::time::Instant::now());
                        }
                        self.reasoning_buffer.push_str(delta);
                    }
                }
                "tool_call_start" => {
                    self.flush_reasoning();
                    self.flush_typewriter();
                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json) {
                        let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                        self.active_tool = Some(ActiveToolInfo {
                            name: name.to_string(),
                            desc: "preparing arguments...".to_string(),
                            started_at: std::time::Instant::now(),
                        });
                    }
                }
                "text_delta" => {
                    self.flush_reasoning();
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(delta) = payload.get("delta").and_then(|v| v.as_str())
                    {
                        self.typewriter.push_delta(delta);
                    }
                }
                "tool_executing" => {
                    self.flush_reasoning();
                    self.flush_typewriter();
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    {
                        let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                        let args = payload.get("arguments");
                        let (_verb, _active_verb, desc) = tool_target_and_verbs(name, args);
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
                    self.flush_reasoning();
                    self.flush_typewriter();
                    let prev_active = self.active_tool.take();
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    {
                        let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                        let success = payload.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
                        let output = payload.get("output").and_then(|v| v.as_str()).unwrap_or("");
                        let args = payload.get("arguments");

                        let (verb, _active_verb, mut target) = tool_target_and_verbs(name, args);
                        if target.is_empty() {
                            if let Some(prev) = prev_active {
                                target = prev.desc;
                            }
                        }

                        let detail = if !success {
                            let err_line = output.lines().next().unwrap_or("error").trim();
                            let cleaned = err_line.strip_prefix(&format!("Error executing {name}: ")).unwrap_or(err_line);
                            format!("failed: {cleaned}")
                        } else {
                            format_tool_success_detail(name, output)
                        };
                        if name == "update_plan" {
                            let parsed_args_holder: Option<serde_json::Value> = match args {
                                Some(serde_json::Value::String(s)) => serde_json::from_str(s).ok(),
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
                                            let clean_step = if let Some((num, rest)) = step.split_once(". ") {
                                                if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
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
                                                "todo" | "open" | "pending" | "not_started" | "not-started" => "pending",
                                                "in_progress" | "in-progress" | "in progress" | "doing" | "active" => "in_progress",
                                                "done" | "completed" | "complete" => "completed",
                                                other => other,
                                            };
                                            Some((clean_step.to_string(), norm_status.to_string()))
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
                            if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
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
                        } else if name == "bash" {
                            let exit_code: i32 = if let Some(idx) = output.rfind("[Process exited with code ") {
                                let after = &output[idx + "[Process exited with code ".len()..];
                                after.split(']').next().and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(if success { 0 } else { 1 })
                            } else if success {
                                0
                            } else {
                                1
                            };

                            let header = format!("⬢ Ran {target} (exit {exit_code})");
                            let branch = format!("  └ {detail}");

                            let mut cleaned_lines: Vec<&str> = Vec::new();
                            for l in output.lines() {
                                let t = l.trim();
                                if t.starts_with("[Process exited with code ") && t.ends_with(']') {
                                    continue;
                                }
                                if t == "[Command finished with no output]" {
                                    continue;
                                }
                                cleaned_lines.push(l);
                            }

                            let mut snippet = String::new();
                            if !self.transcript.is_empty() && !self.transcript.ends_with('\n') {
                                snippet.push('\n');
                            }
                            if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
                                snippet.push('\n');
                            }
                            snippet.push_str(&header);
                            snippet.push('\n');

                            if !cleaned_lines.is_empty() {
                                snippet.push_str("  ┌── Output ────────────────────────────────────────\n");
                                let max_lines = 25;
                                if cleaned_lines.len() <= max_lines {
                                    for l in &cleaned_lines {
                                        snippet.push_str(&format!("  │ {l}\n"));
                                    }
                                } else {
                                    for l in &cleaned_lines[..max_lines] {
                                        snippet.push_str(&format!("  │ {l}\n"));
                                    }
                                    snippet.push_str(&format!("  │ ... ({} more lines)\n", cleaned_lines.len() - max_lines));
                                }
                                snippet.push_str("  └───\n");
                            }
                            snippet.push_str(&branch);
                            snippet.push_str("\n\n");
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
                                if let Some((_num, title)) = crate::tui::chat::split_numbered_result(trimmed) {
                                    if let Some(t) = current_title.take() {
                                        results.push((t, current_url.take().unwrap_or_default()));
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
                            if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
                                snippet.push('\n');
                            }
                            snippet.push_str(&header);
                            snippet.push('\n');

                            if !results.is_empty() {
                                snippet.push_str("  ┌── Results ──────────────────────────────────────\n");
                                for (i, (title, url)) in results.iter().take(5).enumerate() {
                                    snippet.push_str(&format!("  │ {}. {}\n", i + 1, title));
                                    if !url.is_empty() {
                                        snippet.push_str(&format!("  │    URL: {}\n", url));
                                    }
                                }
                                if results.len() > 5 {
                                    snippet.push_str(&format!("  │ ... ({} more results)\n", results.len() - 5));
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
                        } else if success && name == "edit_file" {
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
                                old_str,
                                new_str,
                                start_line,
                                20,
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
                            if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
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
                        } else if name == "write_file" {
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
                            if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
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
                            if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
                                snippet.push('\n');
                            }
                            snippet.push_str(&header);
                            snippet.push('\n');
                            snippet.push_str(&branch);
                            snippet.push_str("\n\n");
                            self.transcript.push_str(&snippet);
                            self.truncate_transcript();
                        }
                    }
                }
                "generation_finished" => {
                    self.flush_reasoning();
                    self.active_tool = None;
                    if let Some(session_id) = self.active_session_id
                        && event.session_id == session_id
                        && let Ok(payload) =
                            serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(status) = payload.get("status").and_then(|v| v.as_str())
                    {
                        let target_status = match status {
                            "cancelled" => ConversationStatus::Cancelled,
                            "failed" => ConversationStatus::Error,
                            _ => ConversationStatus::Finished(FinishReason::Stop),
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
                    self.active_tool = None;
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(message) = payload.get("message").and_then(|v| v.as_str())
                    {
                        self.diagnostic = bounded(message.to_string(), MAX_DIAGNOSTIC_BYTES);
                        self.status = ConversationStatus::Error;
                    }
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
    }

    /// Restore the target session's state into the live view fields.
    fn restore_into_view(&mut self, session_id: i64) {
        let state = self.sessions.entry(session_id).or_default();
        self.transcript = std::mem::take(&mut state.transcript);
        self.prompt = std::mem::take(&mut state.input_draft);
        self.chat_scroll = state.scroll;
        self.status = state.status;
        self.current_plan = state.current_plan.clone();
        self.metrics = None;
        self.diagnostic.clear();
        self.selected_suggestion = 0;

        if self.transcript.is_empty() {
            if let Ok(messages) = self.command_service.session_messages(session_id) {
                for msg in messages {
                    if !self.transcript.is_empty() && !self.transcript.ends_with("\n\n") {
                        if self.transcript.ends_with('\n') {
                            self.transcript.push('\n');
                        } else {
                            self.transcript.push_str("\n\n");
                        }
                    }
                    if msg.role == "user" {
                        self.transcript.push_str(&format!("> {}\n\n", msg.content.trim()));
                    } else if msg.role == "assistant" {
                        self.transcript.push_str(&format!("{}\n\n", msg.content.trim()));
                    } else {
                        self.transcript.push_str(&format!("[{}]: {}\n\n", msg.role, msg.content.trim()));
                    }
                }
                self.truncate_transcript();
                self.scroll_to_bottom();
            }
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
                let model_to_set = if !self.provider.is_empty()
                    && model.starts_with(&format!("{}/", self.provider))
                {
                    model[self.provider.len() + 1..].to_string()
                } else {
                    model
                };
                self.model = bounded(model_to_set.clone(), MAX_IDENTITY_BYTES);
                self.diagnostic = format!("model switched to: {model_to_set}");
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
                self.sessions_dialog = Some(SessionsDialogState::new(sessions, self.active_session_id));
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

        let retained_bytes = Self::MAX_TRANSCRIPT_BYTES - Self::TRUNCATION_MARKER.len();
        let start = ceil_char_boundary(
            &self.transcript,
            self.transcript.len().saturating_sub(retained_bytes),
        );
        self.transcript
            .replace_range(..start, Self::TRUNCATION_MARKER);
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
        assert!(capacity >= 3, "UI event queue capacity must be at least 3");
        Self {
            capacity,
            quit: false,
            cancel: false,
            events: VecDeque::with_capacity(capacity - 3),
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
                if self.events.len() == self.capacity - 3 {
                    self.events.pop_front();
                }
                self.events.push_back(event);
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

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(value: &str, mut index: usize) -> usize {
    while !value.is_char_boundary(index) {
        index += 1;
    }
    index
}
