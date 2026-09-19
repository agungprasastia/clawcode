use super::util::{bounded, is_sensitive_command, split_provider_model};
use super::{App, ConversationMode, ConversationStatus, Input, MAX_IDENTITY_BYTES, UiEvent};
use crate::tui::dialogs::{
    AgentsDialogState, ModelsDialogState, PermissionDecision, PermissionDialogState,
    QuestionDialogState, SessionsDialogState, StatusDialogState, ThemesDialogState, WhichKeyState,
};

impl App {
    pub fn selected_provider(&self) -> &str {
        &self.provider
    }

    pub fn selected_model(&self) -> &str {
        &self.model
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

    pub fn session_listings(&self) -> &[crate::persistence::Session] {
        &self.session_listings
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

    pub fn which_key(&self) -> &WhichKeyState {
        &self.which_key
    }

    pub fn which_key_mut(&mut self) -> &mut WhichKeyState {
        &mut self.which_key
    }

    pub(crate) fn handle_dialog_input(&mut self, event: &UiEvent) -> bool {
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
            return true;
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
                        dialog.push_char(*c);
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
            return true;
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
            return true;
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
                    self.transcript.push_str(delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return true;
        }

        if self.sessions_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.sessions_dialog = None;
                    self.session_listings.clear();
                }
                UiEvent::Input(Input::Character(character)) => {
                    if let Some(dialog) = &mut self.sessions_dialog {
                        if dialog.filter.is_empty() && *character == '/' {
                            self.sessions_dialog = None;
                            self.session_listings.clear();
                            self.history_index = None;
                            self.prompt.push('/');
                            self.cursor_position = 1;
                            self.selected_suggestion = 0;
                            return true;
                        }
                        if dialog.filter.is_empty() && *character == 'd' {
                            if let Some(session) = dialog.selected_session().cloned() {
                                let title = session.title.clone();
                                let id = session.id;
                                let _ = self.command_service.delete_session(id);
                                dialog.remove_item(id);
                                self.session_listings.retain(|s| s.id != id);
                                self.diagnostic = format!("session deleted: {title}");
                            }
                            return true;
                        }
                        dialog.push_char(*character);
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
                    self.transcript.push_str(delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return true;
        }

        if self.agents_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.agents_dialog = None;
                }
                UiEvent::Input(Input::Character(character)) => {
                    if let Some(dialog) = &mut self.agents_dialog {
                        dialog.push_char(*character);
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
                    self.transcript.push_str(delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return true;
        }

        if self.themes_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.themes_dialog = None;
                }
                UiEvent::Input(Input::Character(character)) => {
                    if let Some(dialog) = &mut self.themes_dialog {
                        dialog.push_char(*character);
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
                    self.transcript.push_str(delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return true;
        }

        if self.models_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Quit) | UiEvent::Input(Input::Cancel) => {
                    self.models_dialog = None;
                }
                UiEvent::Input(Input::Character(character)) => {
                    if let Some(dialog) = &mut self.models_dialog {
                        dialog.push_char(*character);
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
                    self.transcript.push_str(delta);
                    self.truncate_transcript();
                }
                _ => {}
            }
            return true;
        }

        false
    }
}
