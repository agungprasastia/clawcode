use super::util::{bounded, is_sensitive_command, split_provider_model};
use super::{App, ConversationMode, ConversationStatus, Input, MAX_IDENTITY_BYTES, UiEvent};
use crate::tui::dialogs::{
    AgentsDialogState, GitDialogState, ModelsDialogState, PermissionDecision,
    PermissionDialogState, QuestionDialogState, SessionsDialogState, SkillsDialogState,
    StatusDialogState, ThemesDialogState, WhichKeyState,
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

    pub fn git_dialog(&self) -> Option<&GitDialogState> {
        self.git_dialog.as_ref()
    }

    pub fn git_dialog_mut(&mut self) -> Option<&mut GitDialogState> {
        self.git_dialog.as_mut()
    }

    pub fn open_git_dialog(&mut self) {
        let repo_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        self.git_dialog = Some(GitDialogState::new(repo_path));
        self.diagnostic = "Git status & staging (Esc to close)".to_string();
    }

    pub fn close_git_dialog(&mut self) {
        self.git_dialog = None;
    }

    pub fn skills_dialog(&self) -> Option<&SkillsDialogState> {
        self.skills_dialog.as_ref()
    }

    pub fn skills_dialog_mut(&mut self) -> Option<&mut SkillsDialogState> {
        self.skills_dialog.as_mut()
    }

    pub fn open_skills_dialog(&mut self) {
        let repo_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let store = crate::workspace::skills::SkillStore::load(&repo_path);
        let items: Vec<crate::workspace::skills::SkillItem> =
            store.all().into_iter().cloned().collect();
        self.skills_dialog = Some(SkillsDialogState::new(items));
        self.diagnostic = "Skills library (Esc to close)".to_string();
    }

    pub fn close_skills_dialog(&mut self) {
        self.skills_dialog = None;
    }

    pub(crate) fn handle_dialog_input(&mut self, event: &UiEvent) -> bool {
        let has_dialog = self.permission_dialog.is_some()
            || self.question_dialog.is_some()
            || self.which_key.visible
            || self.status_dialog.is_some()
            || self.sessions_dialog.is_some()
            || self.agents_dialog.is_some()
            || self.themes_dialog.is_some()
            || self.models_dialog.is_some()
            || self.git_dialog.is_some()
            || self.skills_dialog.is_some();
        if !has_dialog {
            return false;
        }
        match event {
            UiEvent::Resize { .. } => return true,
            UiEvent::StreamDelta(delta) => {
                self.transcript.push_str(delta);
                self.truncate_transcript();
                return true;
            }
            _ => {}
        }

        if self.permission_dialog.is_some() {
            match event {
                UiEvent::Input(Input::Left | Input::Up | Input::Character('h' | 'k')) => {
                    if let Some(dialog) = &mut self.permission_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(
                    Input::Right | Input::Down | Input::ToggleMode | Input::Character('l' | 'j'),
                ) => {
                    if let Some(dialog) = &mut self.permission_dialog {
                        dialog.next();
                    }
                }
                UiEvent::Input(Input::Quit | Input::Cancel) => {
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
                UiEvent::Input(Input::Up | Input::ScrollUp) => {
                    if let Some(dialog) = &mut self.question_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(Input::Down | Input::ScrollDown | Input::ToggleMode) => {
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
            self.which_key.hide();
            match event {
                UiEvent::Input(Input::Character('a')) => {
                    let current = match self.mode {
                        ConversationMode::Plan => "plan",
                        ConversationMode::Build => "build",
                    };
                    self.agents_dialog = Some(AgentsDialogState::new(current));
                }
                UiEvent::Input(Input::Character('t')) => {
                    self.themes_dialog = Some(ThemesDialogState::new(self.theme));
                }
                UiEvent::Input(Input::Character('m')) => {
                    let models = self.available_models.clone();
                    self.models_dialog = Some(ModelsDialogState::new(models, &self.model));
                }
                UiEvent::Input(Input::Character('p')) => self.set_mode(ConversationMode::Plan),
                UiEvent::Input(Input::Character('b')) => self.set_mode(ConversationMode::Build),
                UiEvent::Input(Input::Character('s')) => self.open_status_dialog(),
                UiEvent::Input(Input::Character('r')) => self.open_sessions_dialog(),
                UiEvent::Input(Input::Character('c')) => {
                    self.transcript.clear();
                    self.status = ConversationStatus::Idle;
                    self.diagnostic = "screen cleared".to_string();
                }
                UiEvent::Input(Input::ToggleMode) => self.toggle_mode(),
                _ => {}
            }
            return true;
        }

        if self.status_dialog.is_some() {
            if matches!(
                event,
                UiEvent::Input(Input::Quit | Input::Cancel | Input::Submit | Input::Clear)
            ) {
                self.status_dialog = None;
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
                UiEvent::Input(Input::Up | Input::ScrollUp | Input::PageUp) => {
                    if let Some(dialog) = &mut self.sessions_dialog {
                        dialog.previous();
                    }
                }
                UiEvent::Input(
                    Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                ) => {
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
                _ => {}
            }
            return true;
        }

        if self.agents_dialog.is_some() {
            let mut chosen = None;
            if let Some(d) = &mut self.agents_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => self.agents_dialog = None,
                    UiEvent::Input(Input::Character(c)) => d.push_char(*c),
                    UiEvent::Input(Input::Backspace) => d.pop_char(),
                    UiEvent::Input(Input::Up | Input::ScrollUp | Input::PageUp) => d.previous(),
                    UiEvent::Input(
                        Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                    ) => d.next(),
                    UiEvent::Input(Input::Submit) => chosen = d.selected_agent().cloned(),
                    _ => {}
                }
            }
            if matches!(event, UiEvent::Input(Input::Submit)) {
                self.agents_dialog = None;
                if let Some(agent) = chosen {
                    self.set_mode(agent.mode);
                    self.diagnostic = format!("agent selected: {}", agent.name);
                }
            }
            return true;
        }

        if self.themes_dialog.is_some() {
            let mut chosen = None;
            if let Some(d) = &mut self.themes_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => self.themes_dialog = None,
                    UiEvent::Input(Input::Character(c)) => d.push_char(*c),
                    UiEvent::Input(Input::Backspace) => d.pop_char(),
                    UiEvent::Input(Input::Up | Input::ScrollUp | Input::PageUp) => d.previous(),
                    UiEvent::Input(
                        Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                    ) => d.next(),
                    UiEvent::Input(Input::Submit) => chosen = d.selected_theme(),
                    _ => {}
                }
            }
            if matches!(event, UiEvent::Input(Input::Submit)) {
                self.themes_dialog = None;
                if let Some(theme) = chosen {
                    self.set_theme(theme);
                    self.diagnostic = format!("theme switched to: {}", theme.name());
                }
            }
            return true;
        }

        if self.models_dialog.is_some() {
            let mut chosen = None;
            if let Some(d) = &mut self.models_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => self.models_dialog = None,
                    UiEvent::Input(Input::Character(c)) => d.push_char(*c),
                    UiEvent::Input(Input::Backspace) => d.pop_char(),
                    UiEvent::Input(Input::Up | Input::ScrollUp | Input::PageUp) => d.previous(),
                    UiEvent::Input(
                        Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                    ) => d.next(),
                    UiEvent::Input(Input::Submit) => chosen = d.selected_model().cloned(),
                    _ => {}
                }
            }
            if matches!(event, UiEvent::Input(Input::Submit)) {
                self.models_dialog = None;
                if let Some(m) = chosen {
                    let chosen_id = m.id;
                    if let Some((provider, model)) = split_provider_model(&chosen_id) {
                        self.provider = bounded(provider.to_string(), MAX_IDENTITY_BYTES);
                        self.model = bounded(model.to_string(), MAX_IDENTITY_BYTES);
                    } else {
                        self.model = bounded(chosen_id.clone(), MAX_IDENTITY_BYTES);
                    }
                    self.diagnostic = format!("model switched to: {chosen_id}");
                }
            }
            return true;
        }

        if self.git_dialog.is_some() {
            let mut close = false;
            let mut status_update = None;
            if let Some(dialog) = &mut self.git_dialog {
                if dialog.commit_mode {
                    match event {
                        UiEvent::Input(Input::Quit | Input::Cancel) => dialog.exit_commit_mode(),
                        UiEvent::Input(Input::Submit) => match dialog.commit() {
                            Ok(msg) => status_update = Some(format!("git: {msg}")),
                            Err(err) => status_update = Some(format!("git error: {err}")),
                        },
                        UiEvent::Input(Input::Character(ch)) => dialog.push_commit_char(*ch),
                        UiEvent::Input(Input::Backspace) => dialog.pop_commit_char(),
                        UiEvent::Paste(text) => dialog.push_commit_str(text),
                        _ => {}
                    }
                } else {
                    match event {
                        UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                        UiEvent::Input(Input::Up | Input::ScrollUp) => dialog.previous(),
                        UiEvent::Input(Input::Down | Input::ScrollDown) => dialog.next(),
                        UiEvent::Input(Input::PageUp) => dialog.scroll_diff_up(5),
                        UiEvent::Input(Input::PageDown) => dialog.scroll_diff_down(5),
                        UiEvent::Input(Input::Character(' ')) => dialog.toggle_stage(),
                        UiEvent::Input(Input::Character('c' | 'C')) => dialog.enter_commit_mode(),
                        _ => {}
                    }
                }
            }
            if close {
                self.close_git_dialog();
            }
            if let Some(status) = status_update {
                self.diagnostic = status;
            }
            return true;
        }

        if self.skills_dialog.is_some() {
            let mut chosen_skill = None;
            if let Some(dialog) = &mut self.skills_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => self.skills_dialog = None,
                    UiEvent::Input(Input::Character(c)) => dialog.push_char(*c),
                    UiEvent::Input(Input::Backspace) => dialog.pop_char(),
                    UiEvent::Input(Input::Up | Input::ScrollUp) => dialog.previous(),
                    UiEvent::Input(Input::Down | Input::ScrollDown) => dialog.next(),
                    UiEvent::Input(Input::PageUp) => dialog.scroll_preview_up(5),
                    UiEvent::Input(Input::PageDown) => dialog.scroll_preview_down(5),
                    UiEvent::Input(Input::Submit) => {
                        if let Some(skill) = dialog.selected_skill() {
                            chosen_skill = Some(skill.name.clone());
                        }
                    }
                    _ => {}
                }
            }
            if matches!(event, UiEvent::Input(Input::Submit)) {
                self.skills_dialog = None;
                if let Some(name) = chosen_skill {
                    self.prompt = format!("/skill {name}");
                    self.cursor_position = self.prompt.len();
                    self.diagnostic = format!("skill selected: {name}");
                }
            }
            return true;
        }

        false
    }
}
