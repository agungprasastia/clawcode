pub mod dialogs;
pub mod input;
pub mod layout_cache;
pub mod runtime_bridge;
pub mod session_sync;
pub mod streaming;
pub mod suggestions;
pub mod tabs;
pub mod tool_rows;
pub mod util;

#[cfg(test)]
mod tests;

use ratatui::layout::Rect;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc;

use crate::cli::{self, CommandOutput, ConversationMode as CommandMode};
use crate::conversation::ConversationEvent;
use crate::notify::{BestEffortNotifier, Notification, NotificationKind, Notifier};
use crate::provider::{FinishReason, TurnMetrics};
use crate::runtime::RuntimeEvent;
use crate::runtime::client::RuntimeClient;

use super::dialogs::{
    AgentsDialogState, GitDialogState, MAX_STASH_ENTRIES, McpDialogState, ModelsDialogState,
    PaletteDialogState, PendingPromptItem, PermissionDecision, PermissionDialogState,
    QuestionDialogState, QueuedPromptsDialogState, SessionRenameDialogState, SessionsDialogState,
    SettingsDialogState, SkillsDialogState, StashDialogState, StashEntry, StatusDialogState,
    ThemesDialogState, VariantsDialogState, WhichKeyState, bound_stash_content,
};
use super::home::HomeState;

pub use session_sync::ClientSessionState;
pub use streaming::StreamPart;
pub use suggestions::{AVAILABLE_COMMANDS, CommandSuggestion};
pub use tool_rows::{
    ActiveToolInfo, ToolRow, ToolRowState, canonical_tool_name, exploration_category,
    format_tool_success_detail, tool_completed_icon, tool_headline, tool_is_background,
    tool_pending_label, tool_target_and_verbs, tool_uses_generic_detail,
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
    ClearOrExit,
    DeleteOrExit,
    Character(char),
    Paste,
    Backspace,
    Delete,
    DeleteWord,
    DeleteWordForward,
    DeleteToLineStart,
    DeleteToLineEnd,
    DeleteLine,
    Submit,
    Newline,
    Undo,
    Redo,
    ToggleMode,
    CycleAgent,
    Up,
    Down,
    Left,
    BackgroundOrLeft,
    Right,
    WordLeft,
    WordRight,
    ScrollUp,
    ScrollDown,
    HalfPageUp,
    HalfPageDown,
    PageUp,
    PageDown,
    FirstMessage,
    LastMessage,
    JumpLastUserMessage,
    Home,
    End,
    WhichKey,
    Leader,
    Palette,
    OpenMenu,
    RenameSession,
    PinSession,
    CycleModel,
    CycleModelReverse,
    CycleVariant,
    SessionNext,
    SessionPrevious,
    ReopenTab,
    SelectSession(usize),
    Click { column: u16, row: u16 },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum InputEditKind {
    Typing,
    Deleting,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UiEvent {
    Input(Input),
    Resize { width: u16, height: u16 },
    StreamDelta(String),
    Paste(String),
    MouseClick { x: u16, y: u16 },
    MouseScroll { up: bool },
}

pub(crate) struct McpProbeOutcome {
    pub name: String,
    pub result: Result<usize, String>,
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
    /// Session ids currently open as tabs, in display order.
    open_session_tabs: Vec<i64>,
    /// Most recently closed tabs, newest last.
    recently_closed_tabs: Vec<i64>,
    /// Cached tab titles so rendering does not query SQLite every frame.
    session_titles: std::collections::HashMap<i64, String>,
    permission_dialog: Option<PermissionDialogState>,
    last_permission_decision: Option<PermissionDecision>,
    question_dialog: Option<QuestionDialogState>,
    last_question_answer: Option<String>,
    sessions_dialog: Option<SessionsDialogState>,
    models_dialog: Option<ModelsDialogState>,
    agents_dialog: Option<AgentsDialogState>,
    themes_dialog: Option<ThemesDialogState>,
    status_dialog: Option<StatusDialogState>,
    git_dialog: Option<GitDialogState>,
    skills_dialog: Option<SkillsDialogState>,
    mcp_dialog: Option<McpDialogState>,
    settings_dialog: Option<SettingsDialogState>,
    queued_prompts_dialog: Option<QueuedPromptsDialogState>,
    stash_dialog: Option<StashDialogState>,
    variants_dialog: Option<VariantsDialogState>,
    pending_prompts: Vec<PendingPromptItem>,
    next_local_input_id: i64,
    favorite_models: Vec<String>,
    recent_models: Vec<String>,
    model_variants: BTreeMap<String, String>,
    stash_entries: Vec<StashEntry>,
    config: crate::config::Config,
    tui_settings: crate::config::TuiSettingsConfig,
    config_path: Option<std::path::PathBuf>,
    mcp_registry: Option<Arc<crate::mcp::McpRegistry>>,
    mcp_servers: BTreeMap<String, crate::config::McpServerConfig>,
    mcp_probe_tx: mpsc::Sender<McpProbeOutcome>,
    mcp_probe_rx: mpsc::Receiver<McpProbeOutcome>,
    mcp_probing: HashSet<String>,
    /// Active theme color scheme.
    theme: crate::tui::ThemeKind,
    /// Quick keyboard shortcuts popup status.
    which_key: WhichKeyState,
    /// True while Ctrl+X leader mode is waiting for a chord.
    leader_pending: bool,
    leader_started_at: Option<std::time::Instant>,
    last_scroll_at: Option<std::time::Instant>,
    scroll_streak: u16,
    /// Command palette overlay.
    palette_dialog: Option<PaletteDialogState>,
    /// Inline editor for renaming the active session.
    session_rename_dialog: Option<SessionRenameDialogState>,
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
    input_undo: Vec<(String, usize)>,
    input_redo: Vec<(String, usize)>,
    last_input_edit: Option<InputEditKind>,
    active_tool: Option<ActiveToolInfo>,
    tool_rows: Vec<ToolRow>,
    active_generation_id: Option<i64>,
    ignored_generation_ids: HashSet<i64>,
    pending_finish_reason: Option<FinishReason>,
    text_stream_active: bool,
    reasoning_buffer: String,
    reasoning_start: Option<std::time::Instant>,
    reasoning_active: bool,
    stream_parts: Vec<StreamPart>,
    stream_base_len: Option<usize>,
    expanded_tool_rows: HashSet<String>,
    collapsed_tool_rows: HashSet<String>,
    thought_expanded: bool,
    last_tool_row_clicks: RefCell<Vec<(String, Rect)>>,
    pub current_plan: Vec<(String, String)>,
    last_popup_area: std::cell::Cell<Option<Rect>>,
    last_quick_actions_area: std::cell::Cell<Option<[Rect; 4]>>,
    last_jump_to_latest_area: std::cell::Cell<Option<Rect>>,
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
        let mcp_servers = config.mcp.clone();
        let tui_settings = config.tui.clone();
        let mut typewriter = crate::tui::TypewriterState::new();
        typewriter.set_animations_enabled(tui_settings.animations);
        let thought_expanded = tui_settings.thinking == crate::config::ThinkingDisplay::Show;
        let (mcp_probe_tx, mcp_probe_rx) = mpsc::channel();
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
            open_session_tabs: Vec::new(),
            recently_closed_tabs: Vec::new(),
            session_titles: std::collections::HashMap::new(),
            permission_dialog: None,
            last_permission_decision: None,
            question_dialog: None,
            last_question_answer: None,
            sessions_dialog: None,
            models_dialog: None,
            agents_dialog: None,
            themes_dialog: None,
            status_dialog: None,
            git_dialog: None,
            skills_dialog: None,
            mcp_dialog: None,
            settings_dialog: None,
            queued_prompts_dialog: None,
            stash_dialog: None,
            variants_dialog: None,
            pending_prompts: Vec::new(),
            next_local_input_id: 1,
            favorite_models: Vec::new(),
            recent_models: Vec::new(),
            model_variants: BTreeMap::new(),
            stash_entries: Vec::new(),
            config,
            tui_settings,
            config_path: None,
            mcp_registry: None,
            mcp_servers,
            mcp_probe_tx,
            mcp_probe_rx,
            mcp_probing: HashSet::new(),
            theme: crate::tui::ThemeKind::default(),
            which_key: WhichKeyState::default(),
            leader_pending: false,
            leader_started_at: None,
            last_scroll_at: None,
            scroll_streak: 0,
            palette_dialog: None,
            session_rename_dialog: None,
            available_models,
            git_branch: crate::platform::get_current_branch(),
            home_state: HomeState::new(),
            last_animation_tick: std::time::Instant::now(),
            chat_scroll: 0,
            loaded_until_seq: -1,
            typewriter,
            wave_spinner: crate::tui::WaveSpinner::new(ratatui::style::Color::Rgb(224, 159, 63)),
            runtime: None,
            runtime_events: None,
            prompt_history: Vec::new(),
            history_index: None,
            draft_prompt: String::new(),
            input_undo: Vec::new(),
            input_redo: Vec::new(),
            last_input_edit: None,
            active_tool: None,
            tool_rows: Vec::new(),
            active_generation_id: None,
            ignored_generation_ids: HashSet::new(),
            pending_finish_reason: None,
            text_stream_active: false,
            reasoning_buffer: String::new(),
            reasoning_start: None,
            reasoning_active: false,
            stream_parts: Vec::new(),
            stream_base_len: None,
            expanded_tool_rows: HashSet::new(),
            collapsed_tool_rows: HashSet::new(),
            thought_expanded,
            last_tool_row_clicks: RefCell::new(Vec::new()),
            current_plan: Vec::new(),
            last_popup_area: std::cell::Cell::new(None),
            last_quick_actions_area: std::cell::Cell::new(None),
            last_jump_to_latest_area: std::cell::Cell::new(None),
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
            self.last_jump_to_latest_area.set(None);
            return;
        }

        if matches!(
            event,
            UiEvent::MouseClick { .. } | UiEvent::MouseScroll { .. }
        ) && !self.tui_settings.mouse
        {
            return;
        }

        let event = match event {
            UiEvent::MouseScroll { up: true } => UiEvent::Input(Input::ScrollUp),
            UiEvent::MouseScroll { up: false } => UiEvent::Input(Input::ScrollDown),
            other => other,
        };

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
            UiEvent::Resize { .. } | UiEvent::MouseClick { .. } | UiEvent::MouseScroll { .. } => {}
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

        let cancelling = event == UiEvent::Input(Input::Cancel);
        self.apply(event);
        if !self.running || cancelling {
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

    pub fn tui_settings(&self) -> &crate::config::TuiSettingsConfig {
        &self.tui_settings
    }

    pub fn with_config_settings(
        &mut self,
        config: crate::config::Config,
        config_path: Option<std::path::PathBuf>,
    ) {
        self.tui_settings = config.tui.clone();
        self.config = config;
        self.config_path = config_path;
        self.apply_tui_settings_side_effects();
    }

    pub(crate) fn apply_tui_settings_side_effects(&mut self) {
        self.typewriter
            .set_animations_enabled(self.tui_settings.animations);
        self.thought_expanded = self.tui_settings.thinking == crate::config::ThinkingDisplay::Show;
    }

    pub(crate) fn persist_tui_settings(&mut self) {
        self.config.tui = self.tui_settings.clone();
        self.apply_tui_settings_side_effects();
        if let Some(path) = self.config_path.as_deref()
            && let Err(error) = crate::config::Config::save_tui_settings(path, &self.tui_settings)
        {
            self.diagnostic = format!("failed to save settings: {error}");
        }
    }

    pub fn compact_spinner_frame(&self) -> &'static str {
        if self.tui_settings.animations {
            self.wave_spinner.compact_frame()
        } else {
            "⋯"
        }
    }

    /// Advances periodic UI animations (mascot blinking, wave spinner, typewriter pacing).
    /// Returns true if a visual frame changed and redraw is needed.
    pub fn tick(&mut self) -> bool {
        const ANIMATION_INTERVAL: std::time::Duration = std::time::Duration::from_millis(40);
        let mut redraw = false;
        if self.leader_pending
            && let Some(started) = self.leader_started_at
            && started.elapsed()
                >= std::time::Duration::from_millis(self.tui_settings.leader_timeout_ms)
        {
            self.leader_pending = false;
            self.leader_started_at = None;
            self.which_key.hide();
            self.diagnostic = "leader chord timed out".to_string();
            redraw = true;
        }
        if self.last_animation_tick.elapsed() >= ANIMATION_INTERVAL {
            self.last_animation_tick = std::time::Instant::now();
            if self.tui_settings.animations && self.transcript.is_empty() {
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
                self.select_provider_and_model(&model);
                self.diagnostic = format!("model switched to: {display_id}");
            }
            CommandOutput::Models(models) => {
                self.available_models = models.clone();
                let active_key = self.current_model_key();
                self.models_dialog = Some(ModelsDialogState::with_preferences(
                    models.clone(),
                    &active_key,
                    self.favorite_models.clone(),
                    self.recent_models.clone(),
                ));
                self.diagnostic = format!("{} model(s)", models.len());
            }
            CommandOutput::Sessions(sessions) => {
                self.diagnostic = format!("{} session(s) - press Esc to close", sessions.len());
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

    pub fn pending_prompts(&self) -> &[PendingPromptItem] {
        &self.pending_prompts
    }

    pub fn favorite_models(&self) -> &[String] {
        &self.favorite_models
    }

    pub fn recent_models(&self) -> &[String] {
        &self.recent_models
    }

    pub fn stash_entries(&self) -> &[StashEntry] {
        &self.stash_entries
    }

    pub fn set_available_models_for_test(&mut self, models: Vec<crate::provider::ModelInfo>) {
        self.available_models = models;
    }

    pub fn current_model_key(&self) -> String {
        if self.provider.is_empty() || self.model.contains('/') {
            self.model.clone()
        } else {
            format!("{}/{}", self.provider, self.model)
        }
    }

    pub fn active_variant(&self) -> Option<&str> {
        let key = self.current_model_key();
        self.model_variants
            .get(&key)
            .or_else(|| self.model_variants.get(&self.model))
            .map(String::as_str)
            .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("default"))
    }

    pub fn available_variants(&self) -> Vec<String> {
        crate::provider::available_variants_for_model(
            &self.provider,
            &self.model,
            Some(&self.config),
        )
        .into_iter()
        .filter(|v| !v.eq_ignore_ascii_case("default"))
        .collect()
    }

    pub fn set_active_variant(&mut self, variant: Option<String>) {
        let key = self.current_model_key();
        let cleaned = variant
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("default"));
        match &cleaned {
            Some(v) => {
                self.model_variants.insert(key, v.clone());
                self.diagnostic = format!("model variant: {v}");
            }
            None => {
                self.model_variants.remove(&key);
                self.model_variants.remove(&self.model);
                self.diagnostic = "model variant: default".to_string();
            }
        }
        self.persist_kv_json("model.variants", &self.model_variants);
    }

    pub fn cycle_variant(&mut self) {
        let variants = self.available_variants();
        if variants.is_empty() {
            self.diagnostic = "No model variants available.".to_string();
            return;
        }
        let next = match self.active_variant() {
            None => Some(variants[0].clone()),
            Some(current) => match variants
                .iter()
                .position(|v| v.eq_ignore_ascii_case(current))
            {
                Some(idx) if idx + 1 < variants.len() => Some(variants[idx + 1].clone()),
                _ => None,
            },
        };
        self.set_active_variant(next);
    }

    pub(crate) fn apply_provider_and_model_without_reordering(&mut self, chosen: &str) {
        if let Some((provider, m)) = split_provider_model(chosen) {
            self.provider = bounded(provider.to_string(), MAX_IDENTITY_BYTES);
            self.model = bounded(m.to_string(), MAX_IDENTITY_BYTES);
        } else {
            let model_to_set = if !self.provider.is_empty() {
                let prefix = format!("{}/", self.provider);
                chosen.strip_prefix(&prefix).unwrap_or(chosen).to_string()
            } else {
                chosen.to_string()
            };
            self.model = bounded(model_to_set, MAX_IDENTITY_BYTES);
        }
    }

    pub(crate) fn select_provider_and_model(&mut self, chosen: &str) {
        self.apply_provider_and_model_without_reordering(chosen);
        let full_key = self.current_model_key();
        if !full_key.is_empty() {
            self.recent_models.retain(|item| item != &full_key);
            self.recent_models.insert(0, full_key);
            if self.recent_models.len() > 10 {
                self.recent_models.truncate(10);
            }
            self.persist_kv_json("model.recent", &self.recent_models);
        }
    }

    pub fn toggle_favorite_model(&mut self, id: &str) -> bool {
        let added = if let Some(pos) = self.favorite_models.iter().position(|f| f == id) {
            self.favorite_models.remove(pos);
            false
        } else {
            self.favorite_models.push(id.to_string());
            true
        };
        self.persist_kv_json("model.favorites", &self.favorite_models);
        added
    }

    pub fn stash_current_prompt(&mut self) -> bool {
        if self.prompt.trim().is_empty() {
            self.diagnostic = "nothing in input to stash".to_string();
            return false;
        }
        let content = bound_stash_content(&self.prompt);
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.stash_entries.insert(
            0,
            StashEntry {
                content,
                created_at,
            },
        );
        if self.stash_entries.len() > MAX_STASH_ENTRIES {
            self.stash_entries.truncate(MAX_STASH_ENTRIES);
        }
        self.prompt.clear();
        self.cursor_position = 0;
        self.history_index = None;
        self.selected_suggestion = 0;
        self.persist_kv_json("prompt.stash", &self.stash_entries);
        self.diagnostic = format!("prompt stashed ({} saved)", self.stash_entries.len());
        true
    }

    pub fn pop_stash_to_prompt(&mut self, index: usize) -> bool {
        if index >= self.stash_entries.len() {
            self.diagnostic = "stash is empty".to_string();
            return false;
        }
        let entry = self.stash_entries.remove(index);
        self.prompt = entry.content;
        self.cursor_position = self.prompt.chars().count();
        self.history_index = None;
        self.selected_suggestion = 0;
        self.persist_kv_json("prompt.stash", &self.stash_entries);
        self.diagnostic = "stash restored to input".to_string();
        true
    }

    pub(crate) fn persist_kv_json<T: serde::Serialize>(&self, key: &str, value: &T) {
        if let Some(runtime) = self.runtime.as_ref()
            && let Ok(json) = serde_json::to_string(value)
        {
            let _ = runtime.set_kv(key, &json);
        }
    }

    pub(crate) fn hydrate_kv_state(&mut self) {
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };
        if let Ok(Some(raw)) = runtime.get_kv("model.favorites")
            && let Ok(parsed) = serde_json::from_str::<Vec<String>>(&raw)
        {
            self.favorite_models = parsed;
        }
        if let Ok(Some(raw)) = runtime.get_kv("model.recent")
            && let Ok(parsed) = serde_json::from_str::<Vec<String>>(&raw)
        {
            self.recent_models = parsed;
            if self.recent_models.len() > 10 {
                self.recent_models.truncate(10);
            }
        }
        if let Ok(Some(raw)) = runtime.get_kv("model.variants")
            && let Ok(parsed) = serde_json::from_str::<BTreeMap<String, String>>(&raw)
        {
            self.model_variants = parsed;
        }
        if let Ok(Some(raw)) = runtime.get_kv("prompt.stash")
            && let Ok(parsed) = serde_json::from_str::<Vec<StashEntry>>(&raw)
        {
            self.stash_entries = parsed
                .into_iter()
                .take(MAX_STASH_ENTRIES)
                .map(|mut entry| {
                    entry.content = bound_stash_content(&entry.content);
                    entry
                })
                .collect();
        }
    }

    pub fn has_running_shell(&self) -> bool {
        self.tool_rows.iter().any(|row| {
            canonical_tool_name(&row.name) == "bash" && row.state == ToolRowState::Running
        })
    }

    pub fn background_running_tools(&mut self) -> bool {
        if !self.has_running_shell() {
            return false;
        }
        let has_runtime = if let (Some(runtime), Some(session_id)) =
            (self.runtime.as_ref(), self.active_session_id)
        {
            let _ = runtime.background_tools(session_id);
            true
        } else {
            false
        };
        for row in &mut self.tool_rows {
            if canonical_tool_name(&row.name) == "bash" && row.state == ToolRowState::Running {
                let mut meta = row
                    .metadata
                    .take()
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default();
                meta.insert("background".to_string(), serde_json::Value::Bool(true));
                meta.insert(
                    "status".to_string(),
                    serde_json::Value::String("running".to_string()),
                );
                row.metadata = Some(serde_json::Value::Object(meta));
                if !has_runtime {
                    row.state = ToolRowState::Completed;
                    if row.output.trim().is_empty() {
                        row.output = "[running in background]".to_string();
                    }
                }
            }
        }
        if !has_runtime {
            self.refresh_active_tool();
        }
        self.diagnostic = "shell moved to background".to_string();
        true
    }
}
