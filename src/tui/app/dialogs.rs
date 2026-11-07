use super::session_sync::RevertedTurnSnapshot;
use super::tool_rows::ToolRow;
use super::util::is_sensitive_command;
use super::{App, ConversationMode, ConversationStatus, Input, StreamPart, UiEvent};
use crate::tui::dialogs::{
    AgentsDialogState, DiffReviewDialogState, GitDialogState, McpDialogItem, McpDialogState,
    McpDialogStatus, ModelsDialogState, PaletteAction, PaletteDialogState, PermissionDecision,
    PermissionDialogState, QuestionDialogState, QueuedPromptsDialogState, SessionFileDiff,
    SessionRenameDialogState, SessionsDialogState, SettingsDialogState, SkillsDialogState,
    StashDialogState, StatusDialogState, ThemesDialogState, TimelineDialogState, TimelineTurnItem,
    VariantsDialogState, WhichKeyState, format_single_line_preview,
};
use crate::tui::terminal_pane::TerminalPaneState;

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

    pub fn queued_prompts_dialog(&self) -> Option<&QueuedPromptsDialogState> {
        self.queued_prompts_dialog.as_ref()
    }

    pub fn queued_prompts_dialog_mut(&mut self) -> Option<&mut QueuedPromptsDialogState> {
        self.queued_prompts_dialog.as_mut()
    }

    pub fn open_queued_prompts_dialog(&mut self) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        if let (Some(runtime), Some(session_id)) = (self.runtime.as_ref(), self.active_session_id)
            && let Ok(pending) = runtime.pending_inputs(session_id)
        {
            self.pending_prompts = pending
                .into_iter()
                .map(|input| crate::tui::dialogs::PendingPromptItem {
                    id: input.id,
                    content: input.content,
                    delivery: input.delivery.as_str().to_string(),
                })
                .collect();
        }
        let count = self.pending_prompts.len();
        self.queued_prompts_dialog =
            Some(QueuedPromptsDialogState::new(self.pending_prompts.clone()));
        self.diagnostic = format!("Queued prompts ({count}) - Enter steer, u undo, d discard");
    }

    pub fn close_queued_prompts_dialog(&mut self) {
        self.queued_prompts_dialog = None;
    }

    pub fn stash_dialog(&self) -> Option<&StashDialogState> {
        self.stash_dialog.as_ref()
    }

    pub fn stash_dialog_mut(&mut self) -> Option<&mut StashDialogState> {
        self.stash_dialog.as_mut()
    }

    pub fn open_stash_dialog(&mut self) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        let count = self.stash_entries.len();
        self.stash_dialog = Some(StashDialogState::new(self.stash_entries.clone()));
        self.diagnostic = format!("Prompt stash ({count}) - Enter restore, Ctrl+D delete");
    }

    pub fn close_stash_dialog(&mut self) {
        self.stash_dialog = None;
    }

    pub fn variants_dialog(&self) -> Option<&VariantsDialogState> {
        self.variants_dialog.as_ref()
    }

    pub fn variants_dialog_mut(&mut self) -> Option<&mut VariantsDialogState> {
        self.variants_dialog.as_mut()
    }

    pub fn open_variants_dialog(&mut self) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        let variants = self.available_variants();
        if variants.is_empty() {
            self.diagnostic = "No model variants available.".to_string();
            return;
        }
        let active = self.active_variant().map(ToString::to_string);
        self.variants_dialog = Some(VariantsDialogState::new(variants, active.as_deref()));
        self.diagnostic = "Select model variant".to_string();
    }

    pub fn close_variants_dialog(&mut self) {
        self.variants_dialog = None;
    }

    pub fn timeline_dialog(&self) -> Option<&TimelineDialogState> {
        self.timeline_dialog.as_ref()
    }

    pub fn timeline_dialog_mut(&mut self) -> Option<&mut TimelineDialogState> {
        self.timeline_dialog.as_mut()
    }

    pub fn close_timeline_dialog(&mut self) {
        self.timeline_dialog = None;
    }

    pub fn open_timeline_dialog(&mut self) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        let (title, parent_session, items) = self.build_timeline_items();
        let count = items.len();
        self.timeline_dialog = Some(TimelineDialogState::new(
            self.active_session_id,
            title,
            parent_session,
            items,
        ));
        self.diagnostic =
            format!("Session timeline ({count} turn(s)) - Enter jump, f fork, r revert");
    }

    fn build_timeline_items(&self) -> (String, Option<(i64, String)>, Vec<TimelineTurnItem>) {
        let summaries = self
            .runtime
            .as_ref()
            .and_then(|r| r.session_summaries(false, 200).ok())
            .filter(|s| !s.is_empty())
            .or_else(|| self.command_service.session_summaries(false, 200).ok())
            .unwrap_or_default();

        let session_title = self
            .active_session_id
            .and_then(|id| {
                self.session_titles.get(&id).cloned().or_else(|| {
                    summaries
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.title.clone())
                })
            })
            .unwrap_or_else(|| "Current session".to_string());

        let parent_session = self.active_session_id.and_then(|id| {
            let summary = summaries.iter().find(|s| s.id == id)?;
            let parent_id = summary.parent_id?;
            let parent_title = summaries
                .iter()
                .find(|s| s.id == parent_id)
                .map(|s| s.title.clone())
                .unwrap_or_else(|| format!("Session #{parent_id}"));
            Some((parent_id, parent_title))
        });

        let db_messages = self
            .active_session_id
            .and_then(|id| {
                self.runtime
                    .as_ref()
                    .and_then(|r| r.session_messages(id).ok())
                    .filter(|m| !m.is_empty())
                    .or_else(|| self.command_service.session_messages(id).ok())
            })
            .unwrap_or_default();
        let db_user_msgs: Vec<&crate::persistence::Message> =
            db_messages.iter().filter(|m| m.role == "user").collect();

        let user_indices: Vec<usize> = self
            .stream_parts
            .iter()
            .enumerate()
            .filter_map(|(idx, part)| matches!(part, StreamPart::User(_)).then_some(idx))
            .collect();

        let mut items = Vec::new();
        let total_user_turns = user_indices.len();

        for (turn_idx, &part_idx) in user_indices.iter().enumerate() {
            let next_part_idx = user_indices
                .get(turn_idx + 1)
                .copied()
                .unwrap_or(self.stream_parts.len());
            let prompt = match &self.stream_parts[part_idx] {
                StreamPart::User(text) => format_single_line_preview(text, 72),
                _ => String::new(),
            };
            let mut assistant_chunks = Vec::new();
            let mut tool_count = 0usize;
            let mut file_changes = 0usize;
            for part in &self.stream_parts[part_idx + 1..next_part_idx] {
                match part {
                    StreamPart::Text(text) if !text.trim().is_empty() => {
                        assistant_chunks.push(text.trim());
                    }
                    StreamPart::Tool(call_id) => {
                        tool_count += 1;
                        if let Some(row) = self.tool_rows.iter().find(|r| r.call_id == *call_id)
                            && (row
                                .metadata
                                .as_ref()
                                .and_then(|m| m.get("filediff"))
                                .is_some()
                                || matches!(
                                    row.name.as_str(),
                                    "write_file" | "edit_file" | "apply_patch"
                                ))
                        {
                            file_changes += 1;
                        }
                    }
                    _ => {}
                }
            }
            let assistant_preview = if assistant_chunks.is_empty() {
                String::new()
            } else {
                format_single_line_preview(&assistant_chunks.join(" "), 72)
            };
            let user_message_id = db_user_msgs.get(turn_idx).map(|m| m.id);
            let next_user_message_id = db_user_msgs.get(turn_idx + 1).map(|m| m.id);
            let mut fork_children = Vec::new();
            if let Some(active_id) = self.active_session_id {
                for summary in &summaries {
                    if summary.parent_id == Some(active_id) {
                        let matches_fork_point =
                            match (next_user_message_id, summary.fork_boundary_message_id) {
                                (Some(next_id), Some(fork_id)) => next_id == fork_id,
                                (None, None) => turn_idx + 1 == total_user_turns,
                                _ => false,
                            };
                        if matches_fork_point {
                            fork_children.push((summary.id, summary.title.clone()));
                        }
                    }
                }
            }
            items.push(TimelineTurnItem {
                turn_number: turn_idx + 1,
                user_message_id,
                next_user_message_id,
                stream_part_index: part_idx,
                prompt,
                assistant_preview,
                tool_count,
                file_changes,
                fork_children,
                reverted: false,
            });
        }

        for snap in self.reverted_turns.iter().rev() {
            let file_changes = snap
                .tool_rows
                .iter()
                .filter(|row| {
                    row.metadata
                        .as_ref()
                        .and_then(|m| m.get("filediff"))
                        .is_some()
                        || matches!(
                            row.name.as_str(),
                            "write_file" | "edit_file" | "apply_patch"
                        )
                })
                .count();
            items.push(TimelineTurnItem {
                turn_number: snap.turn_number,
                user_message_id: snap.user_message_id,
                next_user_message_id: None,
                stream_part_index: self.stream_parts.len(),
                prompt: format_single_line_preview(&snap.prompt, 72),
                assistant_preview: format_single_line_preview(&snap.assistant_text, 72),
                tool_count: snap.tool_rows.len(),
                file_changes,
                fork_children: Vec::new(),
                reverted: true,
            });
        }

        (session_title, parent_session, items)
    }

    pub fn jump_to_turn(&mut self, stream_part_index: usize, turn_number: usize) {
        if stream_part_index < self.stream_parts.len() {
            let mut lines_below = 0u16;
            for part in &self.stream_parts[stream_part_index + 1..] {
                lines_below = lines_below.saturating_add(match part {
                    StreamPart::Text(text) => (text.lines().count().max(1) as u16) + 1,
                    StreamPart::Reasoning(_) | StreamPart::ReasoningDone { .. } => 2,
                    StreamPart::Tool(_) => 1,
                    StreamPart::User(text) => (text.lines().count().max(1) as u16) + 2,
                });
            }
            self.chat_scroll = lines_below;
        } else {
            self.scroll_to_bottom();
        }
        self.timeline_dialog = None;
        self.diagnostic = format!("jumped to turn #{turn_number}");
    }

    pub fn fork_session_at_turn(
        &mut self,
        turn_number: usize,
        next_user_message_id: Option<i64>,
    ) -> Option<i64> {
        let base_session_id = if let Some(id) = self.active_session_id {
            id
        } else {
            let created = self
                .runtime
                .as_ref()
                .and_then(|r| r.create_session(1, "Session").ok())
                .or_else(|| {
                    self.command_service
                        .create_session("Session")
                        .ok()
                        .map(|s| s.id)
                })?;
            self.open_session_in_tab_with_title(created, Some("Session"));
            created
        };

        let base_title = self
            .session_titles
            .get(&base_session_id)
            .cloned()
            .unwrap_or_else(|| format!("Session #{base_session_id}"));
        let fork_title = format!("{base_title} (fork #{turn_number})");

        let forked_id = self
            .runtime
            .as_ref()
            .and_then(|r| {
                r.fork_session_before(base_session_id, next_user_message_id)
                    .ok()
                    .map(|s| s.id)
            })
            .or_else(|| {
                self.command_service
                    .fork_session_before(base_session_id, next_user_message_id)
                    .ok()
                    .map(|s| s.id)
            })
            .or_else(|| {
                self.runtime
                    .as_ref()
                    .and_then(|r| r.create_session(1, &fork_title).ok())
                    .or_else(|| {
                        self.command_service
                            .create_session(&fork_title)
                            .ok()
                            .map(|s| s.id)
                    })
            });

        let Some(new_id) = forked_id else {
            self.diagnostic = "failed to fork session".to_string();
            return None;
        };
        let _ = self.command_service.rename_session(new_id, &fork_title);

        let user_indices: Vec<usize> = self
            .stream_parts
            .iter()
            .enumerate()
            .filter_map(|(idx, part)| matches!(part, StreamPart::User(_)).then_some(idx))
            .collect();
        let cutoff_idx = user_indices
            .get(turn_number)
            .copied()
            .unwrap_or(self.stream_parts.len());
        let forked_parts = self.stream_parts[..cutoff_idx].to_vec();
        let kept_call_ids: std::collections::HashSet<&str> = forked_parts
            .iter()
            .filter_map(|p| match p {
                StreamPart::Tool(id) => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let forked_tools: Vec<ToolRow> = self
            .tool_rows
            .iter()
            .filter(|r| kept_call_ids.contains(r.call_id.as_str()))
            .cloned()
            .collect();
        let forked_transcript = Self::rebuild_transcript_from_parts(&forked_parts);

        self.timeline_dialog = None;
        self.open_session_in_tab_with_title(new_id, Some(&fork_title));
        if self.stream_parts.is_empty() && !forked_parts.is_empty() {
            self.stream_parts = forked_parts;
            self.tool_rows = forked_tools;
            self.transcript = forked_transcript;
        }
        self.diagnostic = format!("forked session #{new_id} at turn #{turn_number}");
        Some(new_id)
    }

    fn rebuild_transcript_from_parts(parts: &[StreamPart]) -> String {
        let mut out = String::new();
        for part in parts {
            match part {
                StreamPart::User(prompt) => {
                    if !out.is_empty() && !out.ends_with("\n\n") {
                        out.push_str("\n\n");
                    }
                    out.push_str(&format!("> {}\n\n", prompt.trim()));
                }
                StreamPart::Text(text) => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        if !out.is_empty() && !out.ends_with("\n\n") {
                            out.push_str("\n\n");
                        }
                        out.push_str(&format!("{trimmed}\n\n"));
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn undo_last_turn(&mut self) -> bool {
        if matches!(self.status, ConversationStatus::Active) || self.active_generation_id.is_some()
        {
            if let Some(generation_id) = self.active_generation_id {
                self.ignored_generation_ids.insert(generation_id);
            }
            self.request_runtime_cancel();
            self.reset_turn_view(false);
            self.status = ConversationStatus::Idle;
        }

        let user_indices: Vec<usize> = self
            .stream_parts
            .iter()
            .enumerate()
            .filter_map(|(idx, part)| matches!(part, StreamPart::User(_)).then_some(idx))
            .collect();
        let Some(&last_user_idx) = user_indices.last() else {
            self.diagnostic = "nothing to undo in this session".to_string();
            return false;
        };
        let turn_number = user_indices.len();

        let removed_parts = self.stream_parts.split_off(last_user_idx);
        let mut prompt = String::new();
        let mut assistant_chunks = Vec::new();
        let mut removed_call_ids = Vec::new();
        for part in &removed_parts {
            match part {
                StreamPart::User(text) if prompt.is_empty() => prompt = text.clone(),
                StreamPart::Text(text) if !text.trim().is_empty() => {
                    assistant_chunks.push(text.trim().to_string());
                }
                StreamPart::Tool(call_id) => removed_call_ids.push(call_id.clone()),
                _ => {}
            }
        }
        let assistant_text = assistant_chunks.join("\n\n");

        let mut removed_tools = Vec::new();
        let mut remaining_tools = Vec::with_capacity(self.tool_rows.len());
        for row in self.tool_rows.drain(..) {
            if removed_call_ids.iter().any(|id| id == &row.call_id) {
                removed_tools.push(row);
            } else {
                remaining_tools.push(row);
            }
        }
        self.tool_rows = remaining_tools;

        let workspace_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut restored_call_ids = std::collections::HashSet::new();
        for row in removed_tools.iter().rev() {
            if let Some(filediff) = row.metadata.as_ref().and_then(|m| m.get("filediff")) {
                let _ = crate::conversation::tools::restore_filediff_snapshot(
                    &workspace_dir,
                    filediff,
                    true,
                );
                restored_call_ids.insert(row.call_id.clone());
            }
        }

        let mut user_message_id = None;
        if let Some(session_id) = self.active_session_id {
            let db_messages = self
                .runtime
                .as_ref()
                .and_then(|r| r.session_messages(session_id).ok())
                .filter(|m| !m.is_empty())
                .or_else(|| self.command_service.session_messages(session_id).ok())
                .unwrap_or_default();
            if let Some(last_user_msg) = db_messages.iter().rev().find(|m| m.role == "user") {
                user_message_id = Some(last_user_msg.id);
                let db_calls = self
                    .runtime
                    .as_ref()
                    .and_then(|r| r.session_tool_calls(session_id).ok())
                    .filter(|c| !c.is_empty())
                    .or_else(|| self.command_service.session_tool_calls(session_id).ok())
                    .unwrap_or_default();
                for call in db_calls
                    .iter()
                    .filter(|c| c.assistant_message_id >= last_user_msg.id)
                    .rev()
                {
                    if !restored_call_ids.contains(&call.call_id)
                        && let Some(meta_str) = call.metadata_json.as_deref()
                        && let Ok(meta) = serde_json::from_str::<serde_json::Value>(meta_str)
                        && let Some(filediff) = meta.get("filediff")
                    {
                        let _ = crate::conversation::tools::restore_filediff_snapshot(
                            &workspace_dir,
                            filediff,
                            true,
                        );
                    }
                }
                if let Some(runtime) = self.runtime.as_ref() {
                    let _ = runtime.stage_revert(session_id, last_user_msg.id);
                    let _ = runtime.commit_revert(session_id);
                } else {
                    let _ = self
                        .command_service
                        .stage_revert(session_id, last_user_msg.id);
                    let _ = self.command_service.commit_revert(session_id);
                }
            }
        }

        let new_transcript = Self::rebuild_transcript_from_parts(&self.stream_parts);
        let transcript_tail = self
            .transcript
            .strip_prefix(&new_transcript)
            .unwrap_or("")
            .to_string();
        self.transcript = new_transcript;
        self.prompt = prompt.clone();
        self.cursor_position = self.prompt.chars().count();
        self.reverted_turns.push(RevertedTurnSnapshot {
            turn_number,
            prompt,
            assistant_text,
            stream_parts: removed_parts,
            tool_rows: removed_tools,
            transcript_tail,
            user_message_id,
        });
        if self.timeline_dialog.is_some() {
            let (title, parent_session, items) = self.build_timeline_items();
            self.timeline_dialog = Some(TimelineDialogState::new(
                self.active_session_id,
                title,
                parent_session,
                items,
            ));
        }
        self.diagnostic = format!("undone turn #{turn_number} (prompt restored; /redo to restore)");
        true
    }

    pub fn revert_session_to_turn(&mut self, target_turn_number: usize) -> bool {
        let mut current_turns = self
            .stream_parts
            .iter()
            .filter(|part| matches!(part, StreamPart::User(_)))
            .count();
        if current_turns < target_turn_number || target_turn_number == 0 {
            self.diagnostic = format!("turn #{target_turn_number} is already reverted");
            return false;
        }
        let mut undone = 0usize;
        while current_turns >= target_turn_number {
            if !self.undo_last_turn() {
                break;
            }
            undone += 1;
            current_turns = self
                .stream_parts
                .iter()
                .filter(|part| matches!(part, StreamPart::User(_)))
                .count();
        }
        if undone > 0 {
            self.diagnostic =
                format!("reverted {undone} turn(s) back to turn #{target_turn_number}");
            true
        } else {
            false
        }
    }

    pub fn redo_last_turn(&mut self) -> bool {
        let Some(snapshot) = self.reverted_turns.pop() else {
            self.diagnostic = "nothing to redo".to_string();
            return false;
        };

        let workspace_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        for row in &snapshot.tool_rows {
            if let Some(filediff) = row.metadata.as_ref().and_then(|m| m.get("filediff")) {
                let _ = crate::conversation::tools::restore_filediff_snapshot(
                    &workspace_dir,
                    filediff,
                    false,
                );
            }
        }

        if self.prompt == snapshot.prompt {
            self.prompt.clear();
            self.cursor_position = 0;
        }
        self.stream_parts.extend(snapshot.stream_parts);
        self.tool_rows.extend(snapshot.tool_rows);
        if !snapshot.transcript_tail.is_empty() {
            self.transcript.push_str(&snapshot.transcript_tail);
        } else {
            self.transcript = Self::rebuild_transcript_from_parts(&self.stream_parts);
        }
        self.truncate_transcript();

        if let Some(session_id) = self.active_session_id {
            if let Some(runtime) = self.runtime.as_ref() {
                let _ = runtime.clear_revert(session_id);
                let _ = runtime.append_message(session_id, "user", &snapshot.prompt);
                if !snapshot.assistant_text.is_empty() {
                    let _ =
                        runtime.append_message(session_id, "assistant", &snapshot.assistant_text);
                }
            } else {
                let _ = self.command_service.clear_revert(session_id);
                let _ = self
                    .command_service
                    .append_message(session_id, "user", &snapshot.prompt);
                if !snapshot.assistant_text.is_empty() {
                    let _ = self.command_service.append_message(
                        session_id,
                        "assistant",
                        &snapshot.assistant_text,
                    );
                }
            }
        }

        if self.timeline_dialog.is_some() {
            let (title, parent_session, items) = self.build_timeline_items();
            self.timeline_dialog = Some(TimelineDialogState::new(
                self.active_session_id,
                title,
                parent_session,
                items,
            ));
        }
        self.diagnostic = format!("redone turn #{}", snapshot.turn_number);
        true
    }

    pub fn diff_review_dialog(&self) -> Option<&DiffReviewDialogState> {
        self.diff_review_dialog.as_ref()
    }

    pub fn diff_review_dialog_mut(&mut self) -> Option<&mut DiffReviewDialogState> {
        self.diff_review_dialog.as_mut()
    }

    pub fn close_diff_review_dialog(&mut self) {
        self.diff_review_dialog = None;
    }

    pub fn open_diff_review_dialog(&mut self) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        let files = self.collect_session_file_diffs();
        let count = files.len();
        self.diff_review_dialog = Some(DiffReviewDialogState::new(files));
        self.diagnostic =
            format!("Session diff ({count} file(s)) - Tab/v toggle view, Esc to close");
    }

    pub(crate) fn collect_session_file_diffs(&self) -> Vec<SessionFileDiff> {
        struct CoalescedDiff {
            path: String,
            first_before_exists: bool,
            first_before: String,
            latest_after_exists: bool,
            latest_after: String,
        }

        let mut coalesced: Vec<CoalescedDiff> = Vec::new();
        let apply_filediff = |coalesced: &mut Vec<CoalescedDiff>, filediff: &serde_json::Value| {
            let Some(path) = filediff
                .get("path")
                .or_else(|| filediff.get("file"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
            else {
                return;
            };
            let before_exists = filediff
                .get("before_exists")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let before = filediff
                .get("before")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let after_exists = filediff
                .get("after_exists")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let after = filediff
                .get("after")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(existing) = coalesced.iter_mut().find(|item| item.path == path) {
                existing.latest_after_exists = after_exists;
                existing.latest_after = after;
            } else {
                coalesced.push(CoalescedDiff {
                    path,
                    first_before_exists: before_exists,
                    first_before: before,
                    latest_after_exists: after_exists,
                    latest_after: after,
                });
            }
        };

        for row in &self.tool_rows {
            if let Some(filediff) = row.metadata.as_ref().and_then(|m| m.get("filediff")) {
                apply_filediff(&mut coalesced, filediff);
            }
        }

        if coalesced.is_empty()
            && let Some(session_id) = self.active_session_id
        {
            let db_calls = self
                .runtime
                .as_ref()
                .and_then(|r| r.session_tool_calls(session_id).ok())
                .filter(|c| !c.is_empty())
                .or_else(|| self.command_service.session_tool_calls(session_id).ok())
                .unwrap_or_default();
            for call in db_calls {
                if let Some(meta_str) = call.metadata_json.as_deref()
                    && let Ok(meta) = serde_json::from_str::<serde_json::Value>(meta_str)
                    && let Some(filediff) = meta.get("filediff")
                {
                    apply_filediff(&mut coalesced, filediff);
                }
            }
        }

        coalesced
            .into_iter()
            .map(|entry| {
                let status = match (entry.first_before_exists, entry.latest_after_exists) {
                    (false, true) => 'A',
                    (true, false) => 'D',
                    _ => 'M',
                };
                let diff =
                    crate::tui::diff::compute_diff(&entry.first_before, &entry.latest_after, 0);
                SessionFileDiff {
                    path: entry.path,
                    status,
                    before: entry.first_before,
                    after: entry.latest_after,
                    additions: diff.added,
                    deletions: diff.removed,
                }
            })
            .collect()
    }

    pub fn terminal_pane(&self) -> &TerminalPaneState {
        &self.terminal_pane
    }

    pub fn terminal_pane_mut(&mut self) -> &mut TerminalPaneState {
        &mut self.terminal_pane
    }

    pub fn toggle_terminal_pane(&mut self) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        if self.terminal_pane.toggle() {
            self.diagnostic =
                "Terminal pane opened (Esc or Ctrl+X Down to return to composer)".to_string();
        } else {
            self.diagnostic = "Terminal pane closed".to_string();
        }
    }

    pub fn focus_terminal_pane(&mut self, focused: bool) {
        self.which_key.hide();
        self.leader_pending = false;
        self.leader_started_at = None;
        self.terminal_pane.set_focused(focused);
        self.diagnostic = if self.terminal_pane.is_focused() {
            "Terminal pane focused (Esc to return to composer)".to_string()
        } else {
            "Terminal pane unfocused (composer active)".to_string()
        };
    }

    pub(crate) fn steer_first_queued_prompt(&mut self) -> bool {
        let target_id = self
            .pending_prompts
            .iter()
            .find(|item| item.delivery != "steer")
            .or_else(|| self.pending_prompts.first())
            .map(|item| item.id);
        let Some(input_id) = target_id else {
            return false;
        };
        self.steer_queued_prompt(input_id)
    }

    pub(crate) fn steer_queued_prompt(&mut self, input_id: i64) -> bool {
        if let (Some(runtime), Some(session_id)) = (self.runtime.as_ref(), self.active_session_id) {
            let _ = runtime.steer_input(session_id, input_id);
        }
        let mut updated = false;
        if let Some(pos) = self.pending_prompts.iter().position(|p| p.id == input_id) {
            let mut item = self.pending_prompts.remove(pos);
            item.delivery = "steer".to_string();
            let insert_at = self
                .pending_prompts
                .iter()
                .position(|p| p.delivery != "steer")
                .unwrap_or(self.pending_prompts.len());
            self.pending_prompts.insert(insert_at, item);
            updated = true;
        }
        if let Some(dialog) = &mut self.queued_prompts_dialog {
            dialog.items = self.pending_prompts.clone();
        }
        if updated {
            self.diagnostic = "queued prompt promoted to steer".to_string();
        }
        updated
    }

    pub(crate) fn discard_queued_prompt(
        &mut self,
        input_id: i64,
        restore_to_composer: bool,
    ) -> bool {
        let mut removed_content = None;
        if let (Some(runtime), Some(session_id)) = (self.runtime.as_ref(), self.active_session_id)
            && let Ok(Some(content)) = runtime.discard_input(session_id, input_id)
        {
            removed_content = Some(content);
        }
        if let Some(pos) = self.pending_prompts.iter().position(|p| p.id == input_id) {
            let item = self.pending_prompts.remove(pos);
            if removed_content.is_none() {
                removed_content = Some(item.content);
            }
        }
        if let Some(dialog) = &mut self.queued_prompts_dialog {
            dialog.items = self.pending_prompts.clone();
            if dialog.selected >= dialog.items.len() && !dialog.items.is_empty() {
                dialog.selected = dialog.items.len() - 1;
            } else if dialog.items.is_empty() {
                dialog.selected = 0;
            }
        }
        if let Some(content) = removed_content {
            if restore_to_composer {
                if self.prompt.is_empty() {
                    self.prompt = content;
                } else {
                    self.prompt = format!("{}\n{}", self.prompt, content);
                }
                self.cursor_position = self.prompt.chars().count();
                self.diagnostic = "queued prompt restored to input".to_string();
            } else {
                self.diagnostic = "queued prompt discarded".to_string();
            }
            true
        } else {
            false
        }
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
        if self.recent_models.len() >= 2 {
            let current_key = self.current_model_key();
            let current = self
                .recent_models
                .iter()
                .position(|id| id == &current_key || id == &self.model);
            let len = self.recent_models.len();
            let next = match (current, reverse) {
                (Some(index), false) => (index + 1) % len,
                (Some(index), true) => (index + len - 1) % len,
                (None, false) => 0,
                (None, true) => len - 1,
            };
            if let Some(chosen) = self.recent_models.get(next).cloned() {
                self.apply_provider_and_model_without_reordering(&chosen);
                self.diagnostic = format!("model switched to: {chosen}");
            }
            return;
        }
        if self.available_models.is_empty() {
            self.diagnostic = "no models available".to_string();
            return;
        }
        let current_id = self.current_model_key();
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
            self.apply_provider_and_model_without_reordering(&chosen);
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
                let active_key = self.current_model_key();
                self.models_dialog = Some(ModelsDialogState::with_preferences(
                    models,
                    &active_key,
                    self.favorite_models.clone(),
                    self.recent_models.clone(),
                ));
                self.diagnostic = "Select model".to_string();
            }
            PaletteAction::SwitchVariant => self.open_variants_dialog(),
            PaletteAction::SessionTimeline => self.open_timeline_dialog(),
            PaletteAction::UndoLastTurn => {
                self.undo_last_turn();
            }
            PaletteAction::RedoLastTurn => {
                self.redo_last_turn();
            }
            PaletteAction::ViewSessionDiff => self.open_diff_review_dialog(),
            PaletteAction::ToggleTerminalPane => self.toggle_terminal_pane(),
            PaletteAction::QueuedPrompts => self.open_queued_prompts_dialog(),
            PaletteAction::PromptStash => {
                if !self.prompt.trim().is_empty() {
                    self.stash_current_prompt();
                } else {
                    self.open_stash_dialog();
                }
            }
            PaletteAction::PromptStashList => self.open_stash_dialog(),
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
                let active_key = self.current_model_key();
                self.models_dialog = Some(ModelsDialogState::with_preferences(
                    models,
                    &active_key,
                    self.favorite_models.clone(),
                    self.recent_models.clone(),
                ));
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
            Input::Character('g') => self.open_timeline_dialog(),
            Input::Character('b') => self.toggle_sessions_panel(),
            Input::Character('t') => self.toggle_terminal_pane(),
            Input::Character('d') => self.open_diff_review_dialog(),
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
                self.redo_last_turn();
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
            Input::Up => self.focus_terminal_pane(true),
            Input::Down => self.focus_terminal_pane(false),
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
            || self.queued_prompts_dialog.is_some()
            || self.stash_dialog.is_some()
            || self.variants_dialog.is_some()
            || self.timeline_dialog.is_some()
            || self.diff_review_dialog.is_some()
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

        if self.timeline_dialog.is_some() {
            let mut close = false;
            let mut jump_target: Option<(usize, usize, bool)> = None;
            let mut fork_target: Option<(usize, Option<i64>)> = None;
            let mut revert_target: Option<(usize, bool)> = None;
            if let Some(dialog) = &mut self.timeline_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => {
                        if dialog.search_active || !dialog.filter.is_empty() {
                            dialog.filter.clear();
                            dialog.search_active = false;
                        } else {
                            close = true;
                        }
                    }
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
                    UiEvent::Input(Input::Backspace) => dialog.pop_char(),
                    UiEvent::Input(Input::Submit) => {
                        if let Some(item) = dialog.selected_item() {
                            jump_target =
                                Some((item.stream_part_index, item.turn_number, item.reverted));
                        }
                    }
                    UiEvent::Input(Input::Character('/'))
                        if !dialog.search_active && dialog.filter.is_empty() =>
                    {
                        dialog.search_active = true;
                    }
                    UiEvent::Input(Input::Character('f' | 'F'))
                        if !dialog.search_active && dialog.filter.is_empty() =>
                    {
                        if let Some(item) = dialog.selected_item() {
                            fork_target = Some((item.turn_number, item.next_user_message_id));
                        }
                    }
                    UiEvent::Input(Input::Character('r' | 'R' | 'u' | 'U'))
                        if !dialog.search_active && dialog.filter.is_empty() =>
                    {
                        if let Some(item) = dialog.selected_item() {
                            revert_target = Some((item.turn_number, item.reverted));
                        }
                    }
                    UiEvent::Input(Input::Character(character)) => dialog.push_char(*character),
                    UiEvent::Paste(text) => {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            dialog.push_char(character);
                        }
                    }
                    _ => {}
                }
            }
            if close {
                self.timeline_dialog = None;
                self.diagnostic = "timeline closed".to_string();
            } else if let Some((part_idx, turn_number, reverted)) = jump_target {
                if reverted {
                    self.redo_last_turn();
                } else {
                    self.jump_to_turn(part_idx, turn_number);
                }
            } else if let Some((turn_number, next_user_id)) = fork_target {
                self.fork_session_at_turn(turn_number, next_user_id);
            } else if let Some((turn_number, reverted)) = revert_target {
                if reverted {
                    self.redo_last_turn();
                } else {
                    self.revert_session_to_turn(turn_number);
                }
            }
            return true;
        }

        if self.diff_review_dialog.is_some() {
            let mut close = false;
            let mut undo_requested = false;
            if let Some(dialog) = &mut self.diff_review_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                    UiEvent::Input(Input::Up | Input::Palette) => dialog.previous(),
                    UiEvent::Input(Input::Down) => dialog.next(),
                    UiEvent::Input(Input::ScrollUp | Input::Character('k' | 'K')) => {
                        dialog.scroll_up(2)
                    }
                    UiEvent::Input(Input::ScrollDown | Input::Character('j' | 'J')) => {
                        dialog.scroll_down(2)
                    }
                    UiEvent::Input(Input::PageUp) => dialog.scroll_up(8),
                    UiEvent::Input(Input::PageDown) => dialog.scroll_down(8),
                    UiEvent::Input(
                        Input::ToggleMode | Input::Character('v' | 'V') | Input::Submit,
                    ) => dialog.toggle_view_mode(),
                    UiEvent::Input(Input::Character('u' | 'U')) => undo_requested = true,
                    _ => {}
                }
            }
            if close {
                self.diff_review_dialog = None;
                self.diagnostic = "diff review closed".to_string();
            } else if undo_requested {
                self.undo_last_turn();
                let updated_files = self.collect_session_file_diffs();
                if let Some(dialog) = &mut self.diff_review_dialog {
                    dialog.files = updated_files;
                    if dialog.selected >= dialog.files.len() && !dialog.files.is_empty() {
                        dialog.selected = dialog.files.len() - 1;
                    } else if dialog.files.is_empty() {
                        dialog.selected = 0;
                    }
                    dialog.diff_scroll = 0;
                }
            }
            return true;
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

        if self.queued_prompts_dialog.is_some() {
            let mut close = false;
            let mut steer_id = None;
            let mut undo_id = None;
            let mut discard_id = None;
            if let Some(dialog) = &mut self.queued_prompts_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                    UiEvent::Input(
                        Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                    ) => dialog.previous(),
                    UiEvent::Input(
                        Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                    ) => dialog.next(),
                    UiEvent::Input(Input::Submit) => {
                        steer_id = dialog.selected_item().map(|item| item.id);
                    }
                    UiEvent::Input(Input::Character('u' | 'U') | Input::Backspace) => {
                        undo_id = dialog.selected_item().map(|item| item.id);
                    }
                    UiEvent::Input(
                        Input::Character('d' | 'D') | Input::Delete | Input::DeleteOrExit,
                    ) => {
                        discard_id = dialog.selected_item().map(|item| item.id);
                    }
                    _ => {}
                }
            }
            if close {
                self.queued_prompts_dialog = None;
                self.diagnostic = "queued prompts closed".to_string();
            } else if let Some(id) = steer_id {
                self.steer_queued_prompt(id);
            } else if let Some(id) = undo_id {
                self.discard_queued_prompt(id, true);
                if self.pending_prompts.is_empty() {
                    self.queued_prompts_dialog = None;
                }
            } else if let Some(id) = discard_id {
                self.discard_queued_prompt(id, false);
                if self.pending_prompts.is_empty() {
                    self.queued_prompts_dialog = None;
                }
            }
            return true;
        }

        if self.stash_dialog.is_some() {
            let mut close = false;
            let mut restore_idx = None;
            let mut delete_pressed = false;
            let mut deleted_idx = None;
            if let Some(dialog) = &mut self.stash_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                    UiEvent::Input(
                        Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                    ) => dialog.previous(),
                    UiEvent::Input(
                        Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                    ) => dialog.next(),
                    UiEvent::Input(Input::DeleteOrExit | Input::Delete) => {
                        delete_pressed = true;
                        deleted_idx = dialog.request_or_confirm_delete();
                    }
                    UiEvent::Input(Input::Submit) => {
                        restore_idx = dialog.selected_index();
                    }
                    UiEvent::Input(Input::Backspace) => dialog.pop_char(),
                    UiEvent::Input(Input::Character(c)) => dialog.push_char(*c),
                    UiEvent::Paste(text) => {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            dialog.push_char(character);
                        }
                    }
                    _ => {}
                }
            }
            if close {
                self.stash_dialog = None;
                self.diagnostic = "stash closed".to_string();
            } else if let Some(idx) = restore_idx {
                self.stash_dialog = None;
                self.pop_stash_to_prompt(idx);
            } else if delete_pressed {
                if let Some(orig_idx) = deleted_idx {
                    if orig_idx < self.stash_entries.len() {
                        self.stash_entries.remove(orig_idx);
                        self.persist_kv_json("prompt.stash", &self.stash_entries);
                    }
                    self.diagnostic = "stash entry deleted".to_string();
                } else {
                    self.diagnostic = "press Ctrl+D again to delete stash entry".to_string();
                }
            }
            return true;
        }

        if self.variants_dialog.is_some() {
            let mut close = false;
            let mut chosen: Option<Option<String>> = None;
            if let Some(dialog) = &mut self.variants_dialog {
                match event {
                    UiEvent::Input(Input::Quit | Input::Cancel) => close = true,
                    UiEvent::Input(
                        Input::Up | Input::ScrollUp | Input::PageUp | Input::Palette,
                    ) => dialog.previous(),
                    UiEvent::Input(
                        Input::Down | Input::ScrollDown | Input::PageDown | Input::ToggleMode,
                    ) => dialog.next(),
                    UiEvent::Input(Input::Submit) => {
                        chosen = dialog
                            .selected_variant()
                            .map(|opt| opt.map(ToString::to_string));
                    }
                    _ => {}
                }
            }
            if close {
                self.variants_dialog = None;
                self.diagnostic = "variant picker closed".to_string();
            } else if let Some(variant) = chosen {
                self.variants_dialog = None;
                self.set_active_variant(variant);
            }
            return true;
        }

        if self.models_dialog.is_some() {
            let mut chosen = None;
            let mut fav_toggled: Option<(String, bool)> = None;
            let mut provider_cycled: Option<Option<String>> = None;
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
                    UiEvent::Input(Input::End) => {
                        d.selected = d.filtered_items().len().saturating_sub(1);
                    }
                    UiEvent::Input(Input::PinSession) => {
                        fav_toggled = d.toggle_selected_favorite();
                    }
                    UiEvent::Input(Input::Home) => {
                        provider_cycled = Some(d.cycle_provider_filter());
                    }
                    UiEvent::Input(Input::Submit) => chosen = d.selected_model().cloned(),
                    _ => {}
                }
            }
            if let Some((id, is_fav)) = fav_toggled {
                self.toggle_favorite_model(&id);
                self.diagnostic = if is_fav {
                    format!("model favorited: {id}")
                } else {
                    format!("model unfavorited: {id}")
                };
            } else if let Some(provider_opt) = provider_cycled {
                self.diagnostic = match provider_opt {
                    Some(p) => format!("provider filter: {p}"),
                    None => "provider filter: all".to_string(),
                };
            } else if matches!(event, UiEvent::Input(Input::Submit)) {
                self.models_dialog = None;
                if let Some(m) = chosen {
                    let chosen_id = m.id;
                    self.select_provider_and_model(&chosen_id);
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
