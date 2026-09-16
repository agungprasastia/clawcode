use std::collections::VecDeque;

use crate::cli::{self, CommandOutput, ConversationMode as CommandMode};
use crate::conversation::ConversationEvent;
use crate::notify::{BestEffortNotifier, Notification, NotificationKind, Notifier};
use crate::provider::FinishReason;
use crate::provider::TurnMetrics;
use crate::runtime::{EventBus, RuntimeEvent, client::RuntimeClient};

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
    Character(char),
    Backspace,
    Submit,
    ToggleMode,
    Up,
    Down,
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
        name: "/connect",
        description: "Connect configured AI provider",
        template: "/connect",
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
pub struct ModelsDialogState {
    pub items: Vec<crate::provider::ModelInfo>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
}

impl ModelsDialogState {
    pub fn new(items: Vec<crate::provider::ModelInfo>, active_model: &str) -> Self {
        let selected = items.iter().position(|m| m.id == active_model).unwrap_or(0);
        Self {
            items,
            selected,
            filter: String::new(),
            scroll_offset: 0,
        }
    }

    pub fn filtered_items(&self) -> Vec<&crate::provider::ModelInfo> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let q = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|m| m.id.to_lowercase().contains(&q))
                .collect()
        }
    }

    pub fn selected_model(&self) -> Option<&crate::provider::ModelInfo> {
        let filtered = self.filtered_items();
        filtered.get(self.selected).copied()
    }

    pub fn next(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    pub fn previous(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = if self.selected == 0 {
                count - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn pop_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.scroll_offset = 0;
    }
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
    /// Active interactive model selection dialog, if opened.
    models_dialog: Option<ModelsDialogState>,
    /// Cached list of discovered models for suggestions and selection.
    available_models: Vec<crate::provider::ModelInfo>,
    /// Cached git branch of the workspace
    git_branch: Option<String>,
    /// Optional runtime client wiring prompt submissions to generation
    /// threads. Inactive until a provider-backed runtime is attached.
    runtime: Option<RuntimeClient>,
    /// Live events from the runtime for the active session.
    runtime_events: Option<std::sync::mpsc::Receiver<RuntimeEvent>>,
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
        let mut command_service = cli::runtime_service_with_config(&config).expect("in-memory runtime database");
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
            models_dialog: None,
            available_models,
            git_branch: crate::platform::get_current_branch(),
            runtime: None,
            runtime_events: None,
        }
    }

    pub const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024;
    pub const TRUNCATION_MARKER: &str = "[earlier transcript truncated]\n";

    pub fn apply(&mut self, event: UiEvent) {
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
                UiEvent::Input(Input::Up) => {
                    if let Some(dialog) = &mut self.models_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(Input::Down) => {
                    if let Some(dialog) = &mut self.models_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Input(Input::Submit) => {
                    if let Some(dialog) = &self.models_dialog {
                        if let Some(chosen) = dialog.selected_model() {
                            let chosen_id = chosen.id.clone();
                            self.model = bounded(chosen_id.clone(), MAX_IDENTITY_BYTES);
                            self.diagnostic = format!("model switched to: {chosen_id}");
                        }
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
            UiEvent::Input(Input::Cancel) => self.cancellation_pending = true,
            UiEvent::Input(Input::Character(character)) => {
                self.prompt.push(character);
                self.selected_suggestion = 0;
            }
            UiEvent::Input(Input::Backspace) => {
                self.prompt.pop();
                self.selected_suggestion = 0;
            }
            UiEvent::Input(Input::Up) => {
                self.previous_suggestion();
            }
            UiEvent::Input(Input::Down) => {
                self.next_suggestion();
            }
            UiEvent::Input(Input::Submit) => self.submit_prompt(),
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
            && self.models_dialog.is_none();
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
            }
            ConversationEvent::TextDelta(delta) => {
                if self.status == ConversationStatus::Active {
                    self.transcript.push_str(&delta);
                    self.truncate_transcript();
                }
            }
            ConversationEvent::Finished(reason) => {
                if self.status == ConversationStatus::Active {
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

    pub fn models_dialog(&self) -> Option<&ModelsDialogState> {
        self.models_dialog.as_ref()
    }

    pub fn models_dialog_mut(&mut self) -> Option<&mut ModelsDialogState> {
        self.models_dialog.as_mut()
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
        self.rotate_placeholder();

        if trimmed.starts_with('/') {
            if trimmed.starts_with("/model ") {
                let arg = trimmed[7..].trim();
                if arg.is_empty() {
                    let model_suggestions = self.matching_model_suggestions();
                    if let Some(first) = model_suggestions.get(self.selected_suggestion) {
                        if was_suggestion_focused {
                            let chosen = first.clone();
                            self.model = bounded(chosen.clone(), MAX_IDENTITY_BYTES);
                            self.diagnostic = format!("model switched to: {chosen}");
                            return;
                        }
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
    pub fn poll_runtime(&mut self) {
        let Some(receiver) = self.runtime_events.take() else {
            return;
        };
        while let Ok(event) = receiver.try_recv() {
            match event.kind.as_str() {
                "text_delta" => {
                    if let Ok(payload) =
                        serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(delta) = payload.get("delta").and_then(|v| v.as_str())
                    {
                        self.transcript.push_str(delta);
                        self.truncate_transcript();
                    }
                }
                "generation_finished" => {
                    if let Some(session_id) = self.active_session_id
                        && event.session_id == session_id
                        && let Ok(payload) =
                            serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        && let Some(status) = payload.get("status").and_then(|v| v.as_str())
                    {
                        self.status = match status {
                            "cancelled" => ConversationStatus::Cancelled,
                            "failed" => ConversationStatus::Error,
                            _ => ConversationStatus::Finished(FinishReason::Stop),
                        };
                    }
                }
                "error" => {
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
        self.runtime_events = Some(receiver);
    }

    pub fn active_session_id(&self) -> Option<i64> {
        self.active_session_id
    }

    /// Stash the live view fields into the active session's state.
    fn stash_active(&mut self) {
        let Some(session_id) = self.active_session_id else {
            return;
        };
        let state = self.sessions.entry(session_id).or_default();
        state.transcript = std::mem::take(&mut self.transcript);
        state.input_draft = std::mem::take(&mut self.prompt);
        state.status = self.status;
    }

    /// Restore the target session's state into the live view fields.
    fn restore_into_view(&mut self, session_id: i64) {
        let state = self.sessions.entry(session_id).or_default();
        self.transcript = std::mem::take(&mut state.transcript);
        self.prompt = std::mem::take(&mut state.input_draft);
        self.status = state.status;
        self.metrics = None;
        self.diagnostic.clear();
        self.selected_suggestion = 0;
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
                self.session_listings = sessions;
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

    pub fn transcript(&self) -> &str {
        &self.transcript
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
