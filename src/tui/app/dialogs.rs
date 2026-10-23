use super::util::{bounded, is_sensitive_command, split_provider_model};
use super::{
    App, ConversationMode, ConversationStatus, Input, MAX_IDENTITY_BYTES, StreamPart, UiEvent,
};
use crate::tui::dialogs::{
    AgentsDialogState, GitDialogState, McpDialogItem, McpDialogState, McpDialogStatus,
    ModelsDialogState, PaletteAction, PaletteDialogState, PermissionDecision,
    PermissionDialogState, QuestionDialogState, SessionRenameDialogState, SessionsDialogState,
    SettingsDialogState, SkillsDialogState, StatusDialogState, ThemesDialogState, WhichKeyState,
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
            self.diagnostic = format!("{} session(s) - press Esc to close", sessions.len());
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

    pub fn palette_dialog(&self) -> Option<&PaletteDialogState> {
        self.palette_dialog.as_ref()
    }

    pub fn palette_dialog_mut(&mut self) -> Option<&mut PaletteDialogState> {
        self.palette_dialog.as_mut()
    }

    pub fn session_rename_dialog(&self) -> Option<&SessionRenameDialogState> {
        self.session_rename_dialog.as_ref()
    }

    pub fn open_palette(&mut self) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        self.palette_dialog = Some(PaletteDialogState::new());
        self.diagnostic = "Commands - type to filter, Enter to run".to_string();
    }

    pub fn close_palette(&mut self) {
        self.palette_dialog = None;
    }

    pub fn settings_dialog(&self) -> Option<&SettingsDialogState> {
        self.settings_dialog.as_ref()
    }

    pub fn settings_dialog_mut(&mut self) -> Option<&mut SettingsDialogState> {
        self.settings_dialog.as_mut()
    }

    pub fn open_settings_dialog(&mut self) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        self.settings_dialog = Some(SettingsDialogState::new());
        self.diagnostic = "Settings - Left/Right/Enter to change, Esc to close".to_string();
    }

    pub fn close_settings_dialog(&mut self) {
        self.settings_dialog = None;
    }

    pub fn open_session_rename_dialog(&mut self) {
        let Some(session_id) = self.active_session_id else {
            self.diagnostic = "no active session to rename".to_string();
            return;
        };
        self.open_session_rename_dialog_for(session_id);
    }

    pub fn open_session_rename_dialog_for(&mut self, session_id: i64) {
        let title = self
            .command_service
            .list_sessions()
            .ok()
            .and_then(|sessions| {
                sessions
                    .into_iter()
                    .find(|session| session.id == session_id)
                    .map(|session| session.title)
            })
            .or_else(|| {
                self.session_listings
                    .iter()
                    .find(|session| session.id == session_id)
                    .map(|session| session.title.clone())
            })
            .or_else(|| {
                self.sessions_dialog.as_ref().and_then(|dialog| {
                    dialog
                        .items
                        .iter()
                        .find(|session| session.id == session_id)
                        .map(|session| session.title.clone())
                })
            })
            .unwrap_or_else(|| format!("Session #{session_id}"));
        self.session_rename_dialog = Some(SessionRenameDialogState::new(session_id, title));
        self.diagnostic = "Rename session (Enter to save, Esc to cancel)".to_string();
    }

    pub fn rename_active_session(&mut self, title: &str) {
        let Some(session_id) = self.active_session_id else {
            self.diagnostic = "no active session to rename".to_string();
            return;
        };
        self.rename_session(session_id, title);
    }

    pub fn rename_session(&mut self, session_id: i64, title: &str) {
        let title = title.trim();
        if title.is_empty() {
            self.diagnostic = "session title cannot be empty".to_string();
            return;
        }
        match self.command_service.rename_session(session_id, title) {
            Ok(()) => {
                self.update_session_title(session_id, title);
                self.diagnostic = format!("session renamed: {title}");
            }
            Err(error) => self.diagnostic = format!("rename failed: {error}"),
        }
    }

    pub fn toggle_active_session_pinned(&mut self) {
        let Some(session_id) = self.active_session_id else {
            self.diagnostic = "no active session to pin".to_string();
            return;
        };
        let pinned = self
            .session_listings
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.pinned)
            .or_else(|| {
                self.sessions_dialog
                    .as_ref()
                    .and_then(|dialog| dialog.items.iter().find(|session| session.id == session_id))
                    .map(|session| session.pinned)
            })
            .or_else(|| {
                self.command_service
                    .list_sessions()
                    .ok()
                    .and_then(|sessions| {
                        sessions
                            .into_iter()
                            .find(|session| session.id == session_id)
                    })
                    .map(|session| session.pinned)
            })
            .unwrap_or(false);
        let next = !pinned;
        match self.command_service.set_session_pinned(session_id, next) {
            Ok(()) => {
                self.update_session_pinned(session_id, next);
                self.diagnostic = if next {
                    format!("session pinned: #{session_id}")
                } else {
                    format!("session unpinned: #{session_id}")
                };
            }
            Err(error) => self.diagnostic = format!("pin failed: {error}"),
        }
    }

    fn update_session_title(&mut self, session_id: i64, title: &str) {
        self.session_titles.insert(session_id, title.to_string());
        if let Some(session) = self
            .session_listings
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.title = title.to_string();
        }
        if let Some(dialog) = self.sessions_dialog.as_mut()
            && let Some(session) = dialog
                .items
                .iter_mut()
                .find(|session| session.id == session_id)
        {
            session.title = title.to_string();
        }
    }

    fn update_session_pinned(&mut self, session_id: i64, pinned: bool) {
        if let Some(session) = self
            .session_listings
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.pinned = pinned;
        }
        if let Some(dialog) = self.sessions_dialog.as_mut()
            && let Some(session) = dialog
                .items
                .iter_mut()
                .find(|session| session.id == session_id)
        {
            session.pinned = pinned;
        }
    }

    pub fn delete_session_by_id(&mut self, session_id: i64) -> bool {
        let title = self
            .session_listings
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.title.clone())
            .or_else(|| {
                self.sessions_dialog.as_ref().and_then(|dialog| {
                    dialog
                        .items
                        .iter()
                        .find(|session| session.id == session_id)
                        .map(|session| session.title.clone())
                })
            })
            .unwrap_or_else(|| format!("#{session_id}"));
        match self.command_service.delete_session(session_id) {
            Ok(()) => {
                self.session_listings
                    .retain(|session| session.id != session_id);
                if let Some(dialog) = self.sessions_dialog.as_mut() {
                    dialog.remove_item(session_id);
                }
                self.sessions.remove(&session_id);
                self.remove_session_tab(session_id);
                self.ignored_generation_ids.clear();
                if self.active_session_id == Some(session_id) {
                    self.request_runtime_cancel();
                    self.active_session_id = None;
                    self.reset_turn_view(true);
                    self.status = ConversationStatus::Idle;
                }
                self.diagnostic = format!("session deleted: {title}");
                true
            }
            Err(error) => {
                self.diagnostic = format!("delete failed: {error}");
                false
            }
        }
    }

    pub fn new_session(&mut self) {
        let created = self
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.create_session(1, "New session").ok())
            .or_else(|| {
                self.command_service
                    .create_session("New session")
                    .ok()
                    .map(|session| session.id)
            });
        match created {
            Some(session_id) => {
                self.open_session_in_tab_with_title(session_id, Some("New session"));
                self.status = ConversationStatus::Idle;
                self.diagnostic = format!("new session: #{session_id}");
            }
            None => {
                self.diagnostic = "failed to create session".to_string();
            }
        }
    }

    pub fn cycle_session(&mut self, forward: bool) {
        self.cycle_session_tab(forward);
    }

    pub fn select_session(&mut self, index: usize) {
        self.select_session_tab(index);
    }

    pub fn cycle_model(&mut self, reverse: bool) {
        if self.available_models.is_empty() {
            self.diagnostic = "no models available".to_string();
            return;
        }
        let current_id = if self.model.contains('/') || self.provider.is_empty() {
            self.model.clone()
        } else {
            format!("{}/{}", self.provider, self.model)
        };
        let current = self
            .available_models
            .iter()
            .position(|model| model.id == current_id)
            .or_else(|| {
                self.available_models
                    .iter()
                    .position(|model| model.id == self.model)
            });
        let next = match (current, reverse) {
            (Some(index), false) => (index + 1) % self.available_models.len(),
            (Some(index), true) => {
                (index + self.available_models.len() - 1) % self.available_models.len()
            }
            (None, false) => 0,
            (None, true) => self.available_models.len() - 1,
        };
        if let Some(model) = self.available_models.get(next) {
            let chosen = model.id.clone();
            if let Some((provider, model_name)) = split_provider_model(&chosen) {
                self.provider = bounded(provider.to_string(), MAX_IDENTITY_BYTES);
                self.model = bounded(model_name.to_string(), MAX_IDENTITY_BYTES);
            } else {
                self.model = bounded(chosen.clone(), MAX_IDENTITY_BYTES);
            }
            self.diagnostic = format!("model switched to: {chosen}");
        }
    }

    pub fn toggle_sessions_panel(&mut self) {
        if self.session_listings.is_empty() {
            match self.command_service.list_sessions() {
                Ok(sessions) => {
                    self.diagnostic = format!("{} session(s)", sessions.len());
                    self.session_listings = sessions;
                }
                Err(error) => self.diagnostic = format!("session list failed: {error}"),
            }
        } else {
            self.session_listings.clear();
            self.diagnostic = "session sidebar hidden".to_string();
        }
    }

    pub fn execute_palette_action(&mut self, action: PaletteAction) {
        self.palette_dialog = None;
        match action {
            PaletteAction::SwitchSession | PaletteAction::OpenSessionOrProject => {
                self.open_sessions_dialog()
            }
            PaletteAction::NewSession => self.new_session(),
            PaletteAction::ClearSession => {
                self.request_runtime_cancel();
                self.reset_turn_view(true);
                self.status = ConversationStatus::Idle;
                self.diagnostic = "session cleared".to_string();
            }
            PaletteAction::SwitchModel => {
                let models = self.available_models.clone();
                self.models_dialog = Some(ModelsDialogState::new(models, &self.model));
                self.diagnostic = "Select model".to_string();
            }
            PaletteAction::SwitchAgent => {
                let current = match self.mode {
                    ConversationMode::Plan => "plan",
                    ConversationMode::Build => "build",
                };
                self.agents_dialog = Some(AgentsDialogState::new(current));
                self.diagnostic = "Select agent".to_string();
            }
            PaletteAction::McpServers => self.open_mcp_dialog(),
            PaletteAction::ConnectIntegration => {
                match self.command_service.execute(crate::cli::Command::Connect) {
                    Ok(output) => self.apply_command_output(output),
                    Err(error) => self.diagnostic = error.to_string(),
                }
            }
            PaletteAction::ReleaseHighlights => {
                self.diagnostic = format!(
                    "clawcode v{} - release notes are not bundled with this build",
                    env!("CARGO_PKG_VERSION")
                );
            }
            PaletteAction::Settings => self.open_settings_dialog(),
            PaletteAction::ToggleThinking => {
                self.tui_settings.thinking = self.tui_settings.thinking.toggle();
                self.persist_tui_settings();
                self.diagnostic =
                    format!("thinking display: {}", self.tui_settings.thinking.as_str());
            }
            PaletteAction::ViewStatus | PaletteAction::DebugInfo => self.open_status_dialog(),
            PaletteAction::Update => {
                self.diagnostic = "Run `clawcode upgrade` in a shell to update.".to_string();
            }
            PaletteAction::PairDevice => {
                self.diagnostic = "Run `clawcode pair` in a shell to pair a device.".to_string();
            }
            PaletteAction::RestartService => {
                self.diagnostic =
                    "Run `clawcode service restart` in a shell to restart the service.".to_string();
            }
            PaletteAction::ReloadConfiguration => match crate::config::ConfigLoader.load() {
                Ok(_) => {
                    self.diagnostic =
                        "Configuration is valid; restart clawcode to apply changes.".to_string();
                }
                Err(error) => self.diagnostic = format!("configuration error: {error}"),
            },
            PaletteAction::SwitchTheme => {
                self.themes_dialog = Some(ThemesDialogState::new(self.theme));
                self.diagnostic = "Select theme".to_string();
            }
            PaletteAction::Help => {
                self.which_key.show();
                self.leader_pending = false;
                self.diagnostic = "Keyboard shortcuts".to_string();
            }
            PaletteAction::OpenDocs => {
                self.diagnostic =
                    "Documentation: see README.md and docs/ in the clawcode repository."
                        .to_string();
            }
            PaletteAction::Exit => self.running = false,
        }
    }

    pub(crate) fn handle_leader_input(&mut self, input: Input) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        match input {
            Input::Quit | Input::ClearOrExit => self.running = false,
            Input::Cancel | Input::Leader => {}
            Input::Palette => self.open_palette(),
            Input::OpenMenu => self.open_sessions_dialog(),
            Input::RenameSession => self.open_session_rename_dialog(),
            Input::PinSession => self.toggle_active_session_pinned(),
            Input::CycleModel => self.cycle_model(false),
            Input::CycleModelReverse => self.cycle_model(true),
            Input::SessionNext => self.cycle_session(true),
            Input::SessionPrevious => self.cycle_session(false),
            Input::ReopenTab => self.reopen_closed_session_tab(),
            Input::SelectSession(index) => self.select_session(index),
            Input::Character('q') => self.running = false,
            Input::Character('n') => self.new_session(),
            Input::Character('l') => self.open_sessions_dialog(),
            Input::Character('m') => {
                let models = self.available_models.clone();
                self.models_dialog = Some(ModelsDialogState::new(models, &self.model));
                self.diagnostic = "Select model".to_string();
            }
            Input::Character('a') => {
                let current = match self.mode {
                    ConversationMode::Plan => "plan",
                    ConversationMode::Build => "build",
                };
                self.agents_dialog = Some(AgentsDialogState::new(current));
                self.diagnostic = "Select agent".to_string();
            }
            Input::Character('s') => self.open_status_dialog(),
            Input::Character('c') => self.compact_session(),
            Input::Character('g') => {
                self.diagnostic = "Session timeline is not available in this build.".to_string();
            }
            Input::Character('b') => self.toggle_sessions_panel(),
            Input::Character('t') => {
                self.diagnostic = "Terminal pane is not available in this build.".to_string();
            }
            Input::Character('e') => {
                self.diagnostic = "External editor is not available in this build.".to_string();
            }
            Input::Character('w') => self.close_active_session_tab(),
            Input::Character('x') => {
                self.copy_transcript();
            }
            Input::Character('y') => self.copy_transcript(),
            Input::Character('u') => self.jump_to_last_user_message(),
            Input::Character(',') => self.open_settings_dialog(),
            Input::Character('r') => {
                self.diagnostic = "Session redo is not available in this build.".to_string();
            }
            Input::Character('i') => {
                self.diagnostic = "No image attachments in this session.".to_string();
            }
            Input::Character(character @ '1'..='9') => {
                self.select_session(character.to_digit(10).unwrap_or(1) as usize - 1);
            }
            Input::Character('0') => self.select_session(9),
            Input::Submit => self.submit_prompt(),
            Input::Left => {
                self.diagnostic = "Session pane focus is not available in this build.".to_string();
            }
            Input::Right => {
                self.diagnostic = "Right pane focus is not available in this build.".to_string();
            }
            Input::Up => {
                self.diagnostic = "Terminal pane is not available in this build.".to_string();
            }
            Input::Down => {
                self.diagnostic = "Terminal pane is not available in this build.".to_string();
            }
            _ => self.diagnostic = "Unknown leader shortcut".to_string(),
        }
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

    pub fn mcp_dialog(&self) -> Option<&McpDialogState> {
        self.mcp_dialog.as_ref()
    }

    pub fn mcp_dialog_mut(&mut self) -> Option<&mut McpDialogState> {
        self.mcp_dialog.as_mut()
    }

    pub fn open_mcp_dialog(&mut self) {
        let items = self
            .mcp_servers
            .iter()
            .map(|(name, config)| {
                let status = self.mcp_status_for(name, config);
                McpDialogItem::new(name.clone(), status)
            })
            .collect::<Vec<_>>();
        let count = items.len();
        self.mcp_dialog = Some(McpDialogState::new(items));
        self.diagnostic = format!("MCP servers ({count}) - Esc to close");

        let registry = self.mcp_registry.clone();
        let connect = self
            .mcp_servers
            .keys()
            .filter(|name| {
                !self.mcp_probing.contains(*name)
                    && registry.as_ref().is_some_and(|registry| {
                        registry.is_enabled(name)
                            && !registry.is_connected(name)
                            && registry.status(name).is_none()
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        for name in connect {
            self.start_mcp_probe(&name);
        }
    }

    fn mcp_status_for(
        &self,
        name: &str,
        config: &crate::config::McpServerConfig,
    ) -> McpDialogStatus {
        let Some(registry) = &self.mcp_registry else {
            return if config.enabled {
                McpDialogStatus::Unavailable("MCP runtime is not attached.".to_string())
            } else {
                McpDialogStatus::Disabled
            };
        };
        if self.mcp_probing.contains(name) {
            return McpDialogStatus::Pending;
        }
        if registry.is_connected(name) {
            return McpDialogStatus::Connected {
                tool_count: registry.tool_count(name),
            };
        }
        if let Some(error) = registry.status(name) {
            return McpDialogStatus::Failed(error);
        }
        if registry.is_enabled(name) {
            McpDialogStatus::Pending
        } else {
            McpDialogStatus::Disabled
        }
    }

    pub(crate) fn start_mcp_startup_probes(&mut self) {
        let Some(registry) = self.mcp_registry.clone() else {
            return;
        };
        for name in registry.names() {
            if registry.is_enabled(&name)
                && !registry.is_connected(&name)
                && !self.mcp_probing.contains(&name)
            {
                self.start_mcp_probe(&name);
            }
        }
    }

    fn start_mcp_probe(&mut self, name: &str) {
        if self.mcp_probing.contains(name) {
            if let Some(dialog) = &mut self.mcp_dialog {
                dialog.set_status(name, McpDialogStatus::Pending);
            }
            return;
        }
        let Some(registry) = self.mcp_registry.clone() else {
            if let Some(dialog) = &mut self.mcp_dialog {
                dialog.set_status(
                    name,
                    McpDialogStatus::Unavailable("MCP runtime is not attached.".to_string()),
                );
            }
            return;
        };
        self.mcp_probing.insert(name.to_string());
        if let Some(dialog) = &mut self.mcp_dialog {
            dialog.set_status(name, McpDialogStatus::Pending);
        }

        let sender = self.mcp_probe_tx.clone();
        let thread_name = name.to_string();
        let result_name = name.to_string();
        let spawned = std::thread::Builder::new()
            .name(format!("mcp-probe-{name}"))
            .spawn(move || {
                let result = registry.probe(&thread_name);
                let _ = sender.send(super::McpProbeOutcome {
                    name: result_name,
                    result,
                });
            });
        if let Err(error) = spawned {
            self.mcp_probing.remove(name);
            let message = format!("failed to start MCP probe: {error}");
            if let Some(dialog) = &mut self.mcp_dialog {
                dialog.set_status(name, McpDialogStatus::Failed(message.clone()));
            }
            self.diagnostic = message;
        }
    }

    pub(crate) fn poll_mcp_probes(&mut self) -> bool {
        let mut changed = false;
        while let Ok(outcome) = self.mcp_probe_rx.try_recv() {
            changed = true;
            self.mcp_probing.remove(&outcome.name);
            let status = match outcome.result {
                Ok(tool_count) => McpDialogStatus::Connected { tool_count },
                Err(error) => McpDialogStatus::Failed(error),
            };
            if let Some(dialog) = &mut self.mcp_dialog {
                dialog.set_status(&outcome.name, status);
            }
        }
        changed
    }

    fn toggle_selected_mcp_server(&mut self) {
        let Some(item) = self
            .mcp_dialog
            .as_ref()
            .and_then(|dialog| dialog.selected_item().cloned())
        else {
            return;
        };
        match &item.status {
            McpDialogStatus::Pending => {
                self.diagnostic = format!("MCP server {} is still connecting", item.name);
            }
            McpDialogStatus::Connected { .. } => {
                if let Some(registry) = &self.mcp_registry {
                    registry.disconnect(&item.name);
                }
                if let Some(dialog) = &mut self.mcp_dialog {
                    dialog.set_status(&item.name, McpDialogStatus::Disabled);
                }
                self.diagnostic = format!("MCP server disconnected: {}", item.name);
            }
            McpDialogStatus::Disabled
            | McpDialogStatus::Failed(_)
            | McpDialogStatus::Unavailable(_) => {
                if self.mcp_registry.is_none() {
                    self.diagnostic = "MCP runtime is not attached".to_string();
                    return;
                }
                self.start_mcp_probe(&item.name);
                self.diagnostic = format!("connecting to MCP server: {}", item.name);
            }
        }
    }

    fn copy_mcp_error(&mut self) {
        let Some(text) = self
            .mcp_dialog
            .as_ref()
            .and_then(|dialog| dialog.detail_copy_text())
        else {
            return;
        };
        match crate::platform::SystemClipboard.set_text(&text) {
            Ok(()) => self.diagnostic = "MCP error copied to clipboard".to_string(),
            Err(error) => self.diagnostic = format!("failed to copy MCP error: {error}"),
        }
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
            || self.skills_dialog.is_some()
            || self.mcp_dialog.is_some()
            || self.settings_dialog.is_some()
            || self.palette_dialog.is_some()
            || self.session_rename_dialog.is_some();
        if !has_dialog {
            return false;
        }
        match event {
            UiEvent::Resize { .. } => return true,
            UiEvent::StreamDelta(delta) => {
                self.push_stream_part(StreamPart::Text(delta.clone()));
                self.transcript.push_str(delta);
                self.truncate_transcript();
                return true;
            }
            _ => {}
        }

        if self.settings_dialog.is_some() {
            let mut close = false;
            let mut cycle_delta: Option<isize> = None;
            let mut selected_key = None;
            if let Some(dialog) = &mut self.settings_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                    UiEvent::Input(Input::Character(character)) => dialog.push_char(*character),
                    UiEvent::Input(Input::Backspace) => dialog.pop_char(),
                    UiEvent::Input(
                        Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                    ) => dialog.previous(),
                    UiEvent::Input(
                        Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                    ) => dialog.next(),
                    UiEvent::Input(Input::Home) => dialog.selected = 0,
                    UiEvent::Input(Input::End) => {
                        dialog.selected = dialog.filtered_items().len().saturating_sub(1);
                    }
                    UiEvent::Input(Input::Left) => {
                        selected_key = dialog.selected_key();
                        cycle_delta = Some(-1);
                    }
                    UiEvent::Input(Input::Right | Input::Submit) => {
                        selected_key = dialog.selected_key();
                        cycle_delta = Some(1);
                    }
                    UiEvent::Paste(text) => {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            dialog.push_char(character);
                        }
                    }
                    _ => {}
                }
            }
            if close {
                self.settings_dialog = None;
                self.diagnostic = "settings closed".to_string();
            } else if let (Some(key), Some(delta)) = (selected_key, cycle_delta) {
                key.cycle(&mut self.tui_settings, delta);
                let label = key.value_label(&self.tui_settings);
                self.diagnostic = format!("{}: {label}", key.title());
                self.persist_tui_settings();
            }
            return true;
        }

        if self.permission_dialog.is_some() {
            match event {
                UiEvent::Input(
                    Input::Left | Input::Up | Input::Palette | Input::Character('h' | 'k'),
                ) => {
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
                UiEvent::Input(Input::Up | Input::ScrollUp | Input::Palette) => {
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

        if self.palette_dialog.is_some() {
            let mut chosen = None;
            let mut close = false;
            if let Some(dialog) = &mut self.palette_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                    UiEvent::Input(Input::Character(character)) => dialog.push_char(*character),
                    UiEvent::Input(Input::Backspace) => dialog.pop_char(),
                    UiEvent::Input(
                        Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                    ) => {
                        dialog.previous();
                    }
                    UiEvent::Input(
                        Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                    ) => dialog.next(),
                    UiEvent::Input(Input::Home) => dialog.selected = 0,
                    UiEvent::Input(Input::End) => {
                        dialog.selected = dialog.filtered_items().len().saturating_sub(1);
                    }
                    UiEvent::Input(Input::Submit) => {
                        chosen = dialog.selected_item().map(|item| item.action);
                    }
                    UiEvent::Paste(text) => {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            dialog.push_char(character);
                        }
                    }
                    _ => {}
                }
            }
            if close {
                self.palette_dialog = None;
                self.diagnostic = "commands closed".to_string();
            } else if let Some(action) = chosen {
                self.execute_palette_action(action);
            }
            return true;
        }

        if self.session_rename_dialog.is_some() {
            let mut submit = false;
            let mut close = false;
            if let Some(dialog) = &mut self.session_rename_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                    UiEvent::Input(Input::Character(character)) => dialog.push_char(*character),
                    UiEvent::Input(Input::Backspace) => dialog.pop_char(),
                    UiEvent::Input(Input::Delete | Input::DeleteOrExit | Input::DeleteLine) => {
                        dialog.delete_forward();
                    }
                    UiEvent::Input(Input::DeleteWord | Input::WordLeft | Input::Home) => {
                        dialog.move_home();
                    }
                    UiEvent::Input(Input::DeleteWordForward | Input::WordRight | Input::End) => {
                        dialog.move_end();
                    }
                    UiEvent::Input(Input::Left) => dialog.move_left(),
                    UiEvent::Input(Input::Right) => dialog.move_right(),
                    UiEvent::Input(Input::Submit) => submit = true,
                    UiEvent::Paste(text) => {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            dialog.push_char(character);
                        }
                    }
                    _ => {}
                }
            }
            if submit {
                let (session_id, title) = self
                    .session_rename_dialog
                    .as_ref()
                    .map(|dialog| (dialog.session_id, dialog.trimmed_title().to_string()))
                    .unwrap_or((-1, String::new()));
                self.session_rename_dialog = None;
                if session_id >= 0 {
                    self.rename_session(session_id, &title);
                }
            } else if close {
                self.session_rename_dialog = None;
                self.diagnostic = "rename cancelled".to_string();
            }
            return true;
        }

        if self.which_key.visible {
            self.which_key.hide();
            if let UiEvent::Input(input) = event {
                if matches!(input, Input::Quit) {
                    self.running = false;
                } else {
                    self.handle_leader_input(*input);
                }
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
                UiEvent::Input(Input::RenameSession) => {
                    let selected = self
                        .sessions_dialog
                        .as_ref()
                        .and_then(|dialog| dialog.selected_session().cloned());
                    if let Some(session) = selected {
                        self.open_session_rename_dialog_for(session.id);
                    }
                    return true;
                }
                UiEvent::Input(Input::DeleteOrExit) => {
                    let selected = self
                        .sessions_dialog
                        .as_ref()
                        .and_then(|dialog| dialog.selected_session().cloned());
                    if let Some(session) = selected {
                        self.delete_session_by_id(session.id);
                    }
                    return true;
                }
                UiEvent::Input(Input::PinSession) => {
                    let selected = self
                        .sessions_dialog
                        .as_ref()
                        .and_then(|dialog| dialog.selected_session().cloned());
                    if let Some(session) = selected {
                        let next = !session.pinned;
                        match self.command_service.set_session_pinned(session.id, next) {
                            Ok(()) => {
                                self.update_session_pinned(session.id, next);
                                self.diagnostic = if next {
                                    format!("session pinned: {}", session.title)
                                } else {
                                    format!("session unpinned: {}", session.title)
                                };
                            }
                            Err(error) => self.diagnostic = format!("pin failed: {error}"),
                        }
                    }
                    return true;
                }
                UiEvent::Input(Input::Character(character)) => {
                    let starts_slash = self
                        .sessions_dialog
                        .as_ref()
                        .is_some_and(|dialog| dialog.filter.is_empty() && *character == '/');
                    if starts_slash {
                        self.sessions_dialog = None;
                        self.session_listings.clear();
                        self.history_index = None;
                        self.prompt.push('/');
                        self.cursor_position = 1;
                        self.selected_suggestion = 0;
                        return true;
                    }
                    let delete_candidate = self.sessions_dialog.as_ref().and_then(|dialog| {
                        if dialog.filter.is_empty() && *character == 'd' {
                            dialog.selected_session().cloned()
                        } else {
                            None
                        }
                    });
                    if let Some(session) = delete_candidate {
                        self.delete_session_by_id(session.id);
                        return true;
                    }
                    if let Some(dialog) = &mut self.sessions_dialog {
                        dialog.push_char(*character);
                    }
                }
                UiEvent::Input(Input::Backspace) => {
                    if let Some(dialog) = &mut self.sessions_dialog {
                        dialog.pop_char();
                    }
                }
                UiEvent::Input(Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette) => {
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
                        self.open_session_in_tab_with_title(chosen.id, Some(&chosen.title));
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
                    UiEvent::Input(
                        Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                    ) => d.previous(),
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
                    UiEvent::Input(
                        Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                    ) => d.previous(),
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
                    UiEvent::Input(
                        Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                    ) => d.previous(),
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
                        UiEvent::Input(Input::Up | Input::ScrollUp | Input::Palette) => {
                            dialog.previous()
                        }
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
                    UiEvent::Input(Input::Up | Input::ScrollUp | Input::Palette) => {
                        dialog.previous()
                    }
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

        if self.mcp_dialog.is_some() {
            let mut close = false;
            let mut back = false;
            let mut copy = false;
            let mut toggle = false;
            let mut open_error = false;
            if let Some(dialog) = &mut self.mcp_dialog {
                if dialog.detail.is_some() {
                    match event {
                        UiEvent::Input(Input::Quit) => close = true,
                        UiEvent::Input(Input::Cancel) => back = true,
                        UiEvent::Input(Input::Character('c' | 'C')) => copy = true,
                        UiEvent::Input(Input::Up | Input::ScrollUp | Input::Palette) => {
                            dialog.scroll_detail_up(1)
                        }
                        UiEvent::Input(Input::Down | Input::ScrollDown | Input::ToggleMode) => {
                            dialog.scroll_detail_down(1)
                        }
                        UiEvent::Input(Input::PageUp) => dialog.scroll_detail_up(5),
                        UiEvent::Input(Input::PageDown) => dialog.scroll_detail_down(5),
                        _ => {}
                    }
                } else {
                    match event {
                        UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                        UiEvent::Input(Input::Character(' ')) => toggle = true,
                        UiEvent::Input(Input::Character(character)) => dialog.push_char(*character),
                        UiEvent::Input(Input::Backspace) => dialog.pop_char(),
                        UiEvent::Input(
                            Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                        ) => dialog.previous(),
                        UiEvent::Input(
                            Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                        ) => dialog.next(),
                        UiEvent::Input(Input::Submit) => open_error = true,
                        UiEvent::Paste(text) => {
                            for character in
                                text.chars().filter(|character| !character.is_control())
                            {
                                dialog.push_char(character);
                            }
                        }
                        _ => {}
                    }
                }
            }
            if close {
                self.mcp_dialog = None;
                self.diagnostic = "MCP servers closed".to_string();
            } else if back {
                if let Some(dialog) = &mut self.mcp_dialog {
                    dialog.close_detail();
                }
                self.diagnostic = "MCP servers".to_string();
            } else if copy {
                self.copy_mcp_error();
            } else if toggle {
                self.toggle_selected_mcp_server();
            } else if open_error {
                let opened = self
                    .mcp_dialog
                    .as_mut()
                    .is_some_and(|dialog| dialog.open_selected_error());
                self.diagnostic = if opened {
                    "MCP error details (Esc to go back)".to_string()
                } else {
                    "No error details for this MCP server".to_string()
                };
            }
            return true;
        }

        false
    }
}
