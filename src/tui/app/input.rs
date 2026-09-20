use super::App;
use super::util::{bounded, ceil_char_boundary, find_user_turn_boundary, split_provider_model};
use super::{ConversationMode, ConversationStatus, Input, MAX_IDENTITY_BYTES, StreamPart};
use crate::cli::{self, CommandOutput, ConversationMode as CommandMode};
use crate::conversation::ConversationEvent;
use crate::platform::SystemClipboard;
use crate::tui::dialogs::{AgentsDialogState, ThemesDialogState};

impl App {
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

    pub(crate) fn submit_prompt(&mut self) {
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
                self.stream_parts.clear();
                self.tool_rows.clear();
                self.expanded_tool_rows.clear();
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
            self.push_stream_part(StreamPart::User(prompt.to_string()));
            self.stream_base_len = Some(self.transcript.len());
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
            self.push_stream_part(StreamPart::User(prompt.to_string()));
            self.stream_base_len = Some(self.transcript.len());
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

    pub(crate) fn handle_input_key(&mut self, input: Input) {
        match input {
            Input::Quit => {
                if self.session_listings.is_empty() {
                    self.running = false;
                } else {
                    self.session_listings.clear();
                }
            }
            Input::Cancel => {
                if let Some(generation_id) = self.active_generation_id {
                    self.ignored_generation_ids.insert(generation_id);
                }
                self.request_runtime_cancel();
                self.reset_turn_view(false);
                self.cancellation_pending = true;
                self.status = ConversationStatus::Cancelled;
            }
            Input::Clear => {
                self.request_runtime_cancel();
                self.reset_turn_view(true);
                self.status = ConversationStatus::Idle;
                self.diagnostic = "screen cleared".to_string();
            }
            Input::Character(character) => {
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
            Input::Backspace => {
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
            Input::Paste => {
                if let Ok(text) = SystemClipboard.get_text() {
                    self.insert_str_at_cursor(&text);
                }
            }
            Input::Up => {
                if self.suggestion_count() > 0 {
                    self.previous_suggestion();
                } else {
                    self.navigate_history_up();
                }
            }
            Input::Down => {
                if self.suggestion_count() > 0 {
                    self.next_suggestion();
                } else {
                    self.navigate_history_down();
                }
            }
            Input::ScrollUp => {
                self.scroll_up(3);
            }
            Input::ScrollDown => {
                self.scroll_down(3);
            }
            Input::PageUp => {
                self.scroll_up(15);
            }
            Input::PageDown => {
                self.scroll_down(15);
            }
            Input::Home => {
                self.cursor_position = 0;
                self.scroll_to_top();
            }
            Input::End => {
                self.cursor_position = self.prompt.chars().count();
                self.scroll_to_bottom();
            }
            Input::Left => {
                self.cursor_position = self.cursor_position.saturating_sub(1);
            }
            Input::Right => {
                self.cursor_position = (self.cursor_position + 1).min(self.prompt.chars().count());
            }
            Input::Submit => self.submit_prompt(),
            Input::WhichKey => {
                self.which_key.toggle();
            }
            Input::ToggleMode => {
                if !self.matching_suggestions().is_empty()
                    || self.prompt.starts_with("/model ")
                    || self.prompt.starts_with("/theme ")
                {
                    self.autocomplete_selected_command();
                } else {
                    self.toggle_mode();
                }
            }
            Input::Click { column, row } => {
                self.handle_mouse_click(column, row);
            }
        }
    }
}
