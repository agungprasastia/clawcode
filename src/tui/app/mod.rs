pub mod dialogs;
pub mod input;
pub mod layout_cache;
pub mod runtime_bridge;
pub mod session_sync;
pub mod streaming;
pub mod suggestions;
pub mod tool_rows;
pub mod util;

#[cfg(test)]
mod tests;

use ratatui::layout::Rect;
use std::cell::RefCell;
use std::collections::HashSet;

use crate::cli::{self, CommandOutput, ConversationMode as CommandMode};
use crate::conversation::ConversationEvent;
use crate::notify::{BestEffortNotifier, Notification, NotificationKind, Notifier};
use crate::provider::{FinishReason, TurnMetrics};
use crate::runtime::RuntimeEvent;
use crate::runtime::client::RuntimeClient;

use super::dialogs::{
    AgentsDialogState, ModelsDialogState, PermissionDecision, PermissionDialogState,
    QuestionDialogState, SessionsDialogState, StatusDialogState, ThemesDialogState, WhichKeyState,
};
use super::home::HomeState;

pub use session_sync::ClientSessionState;
pub use streaming::StreamPart;
pub use suggestions::{AVAILABLE_COMMANDS, CommandSuggestion};
pub use tool_rows::{
    ActiveToolInfo, ToolRow, ToolRowState, format_tool_success_detail, tool_target_and_verbs,
};
use util::bounded;
pub use util::{
    PLACEHOLDER_SUGGESTIONS, UiEventQueue, get_random_placeholder, is_sensitive_command,
    split_provider_model,
};

pub(crate) const MAX_DIAGNOSTIC_BYTES: usize = 4 * 1024;
pub(crate) const MAX_IDENTITY_BYTES: usize = 256;
pub(crate) const MAX_TOOL_ROWS: usize = 64;
pub(crate) const MAX_TOOL_ARGUMENT_BYTES: usize = 4 * 1024;
pub(crate) const MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024;

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

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UiEvent {
    Input(Input),
    Resize { width: u16, height: u16 },
    StreamDelta(String),
    Paste(String),
    MouseClick { x: u16, y: u16 },
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
    loaded_until_seq: i64,
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
            loaded_until_seq: -1,
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

    pub fn apply(&mut self, event: UiEvent) {
        if let UiEvent::Resize { width, height } = event {
            self.terminal_size.set((width, height));
            self.last_popup_area.set(None);
            self.last_quick_actions_area.set(None);
            return;
        }

        if self.handle_dialog_input(&event) {
            return;
        }

        if let UiEvent::MouseClick { x, y } | UiEvent::Input(Input::Click { column: x, row: y }) =
            event
        {
            self.handle_mouse_click(x, y);
            return;
        }

        match event {
            UiEvent::Input(input) => self.handle_input_key(input),
            UiEvent::Paste(text) => self.insert_str_at_cursor(&text),
            UiEvent::Resize { .. } | UiEvent::MouseClick { .. } => {}
            UiEvent::StreamDelta(delta) => {
                self.push_stream_part(StreamPart::Text(delta.clone()));
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
        let cancelling = event == UiEvent::Input(Input::Cancel);
        self.apply(event);
        if quitting || cancelling {
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
                    self.push_stream_part(StreamPart::Text(delta.clone()));
                    if let Some(flushed) = self.typewriter.push_delta(&delta) {
                        self.transcript.push_str(&flushed);
                        self.truncate_transcript();
                    }
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

    pub(crate) fn notify(&self, kind: NotificationKind, title: &str, body: &str) {
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

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn set_placeholder(&mut self, placeholder: String) {
        self.placeholder = placeholder;
    }

    pub fn rotate_placeholder(&mut self) {
        self.placeholder = get_random_placeholder();
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
}
