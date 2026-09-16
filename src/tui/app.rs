use std::collections::VecDeque;

use crate::cli::{self, CommandOutput, ConversationMode as CommandMode};
use crate::conversation::ConversationEvent;
use crate::notify::{BestEffortNotifier, Notification, NotificationKind, Notifier};
use crate::provider::FinishReason;
use crate::provider::TurnMetrics;

const MAX_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const MAX_IDENTITY_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ConversationMode {
    Plan,
    Build,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ConversationStatus {
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
        name: "/models",
        description: "List discovered AI models",
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UiEvent {
    Input(Input),
    Resize { width: u16, height: u16 },
    StreamDelta(String),
}

pub struct App {
    running: bool,
    cancellation_pending: bool,
    prompt: String,
    transcript: String,
    mode: ConversationMode,
    status: ConversationStatus,
    provider: String,
    model: String,
    diagnostic: String,
    metrics: Option<TurnMetrics>,
    command_service: crate::cli::CommandService<cli::CliDiscovery>,
    selected_suggestion: usize,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            running: true,
            cancellation_pending: false,
            prompt: String::new(),
            transcript: String::new(),
            mode: ConversationMode::Plan,
            status: ConversationStatus::Idle,
            provider: String::new(),
            model: String::new(),
            diagnostic: String::new(),
            metrics: None,
            command_service: cli::runtime_service().expect("in-memory runtime database"),
            selected_suggestion: 0,
        }
    }

    pub const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024;
    pub const TRUNCATION_MARKER: &str = "[earlier transcript truncated]\n";

    pub fn apply(&mut self, event: UiEvent) {
        match event {
            UiEvent::Input(Input::Quit) => self.running = false,
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
                if !self.matching_suggestions().is_empty() {
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

        let quitting = event == UiEvent::Input(Input::Quit);
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

    pub fn is_running(&self) -> bool {
        self.running
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

    pub fn next_suggestion(&mut self) {
        let count = self.matching_suggestions().len();
        if count > 0 {
            self.selected_suggestion = (self.selected_suggestion + 1) % count;
        }
    }

    pub fn previous_suggestion(&mut self) {
        let count = self.matching_suggestions().len();
        if count > 0 {
            self.selected_suggestion = if self.selected_suggestion == 0 {
                count - 1
            } else {
                self.selected_suggestion - 1
            };
        }
    }

    pub fn autocomplete_selected_command(&mut self) -> bool {
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
        self.selected_suggestion = 0;
        let input = std::mem::take(&mut self.prompt);
        let command = match cli::parse_command(&input) {
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
                self.diagnostic = format!("session created: {}", session.title)
            }
            Ok(CommandOutput::Sessions(sessions)) => {
                self.diagnostic = format!("{} session(s)", sessions.len())
            }
            Ok(CommandOutput::Connected(provider)) => {
                self.apply_command_output(CommandOutput::Connected(provider));
            }
            Ok(CommandOutput::Models(models)) => {
                self.diagnostic = format!("{} model(s)", models.len())
            }
            Ok(CommandOutput::Help(text)) => self.diagnostic = text,
            Ok(CommandOutput::RefreshStarted) => self.diagnostic = "model refresh started".into(),
            Err(error) => self.diagnostic = error.to_string(),
        }
        if let Some(error) = self.command_service.take_diagnostic() {
            self.diagnostic = error;
        }
    }

    pub fn apply_command_output(&mut self, output: CommandOutput) {
        if let CommandOutput::Connected(provider) = output {
            self.provider = bounded(provider.to_string(), MAX_IDENTITY_BYTES);
            self.diagnostic = "provider connected".into();
        }
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
