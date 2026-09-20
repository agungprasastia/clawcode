use std::path::PathBuf;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::platform::git::{self, GitFileChange};
use crate::tui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDialogState {
    pub repo_path: PathBuf,
    pub changes: Vec<GitFileChange>,
    pub selected: usize,
    pub diff: String,
    pub diff_scroll: u16,
    pub file_scroll: usize,
    pub commit_mode: bool,
    pub commit_message: String,
    pub status_message: Option<String>,
}

impl GitDialogState {
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        let path = repo_path.into();
        let changes = git::get_status(&path).unwrap_or_default();
        let mut state = Self {
            repo_path: path,
            changes,
            selected: 0,
            diff: String::new(),
            diff_scroll: 0,
            file_scroll: 0,
            commit_mode: false,
            commit_message: String::new(),
            status_message: None,
        };
        state.reload_diff();
        state
    }

    pub fn with_changes(repo_path: impl Into<PathBuf>, changes: Vec<GitFileChange>) -> Self {
        Self {
            repo_path: repo_path.into(),
            changes,
            selected: 0,
            diff: String::new(),
            diff_scroll: 0,
            file_scroll: 0,
            commit_mode: false,
            commit_message: String::new(),
            status_message: None,
        }
    }

    pub fn refresh(&mut self) {
        if let Ok(changes) = git::get_status(&self.repo_path) {
            self.changes = changes;
            if self.selected >= self.changes.len() {
                self.selected = self.changes.len().saturating_sub(1);
            }
            self.reload_diff();
        }
    }

    pub fn reload_diff(&mut self) {
        self.diff_scroll = 0;
        if let Some(change) = self.selected_change() {
            match git::get_diff(&self.repo_path, &change.path, change.staged) {
                Ok(diff) if !diff.is_empty() => {
                    self.diff = diff;
                }
                Ok(_) => {
                    if change.status_code == "?" {
                        let full_path = self.repo_path.join(&change.path);
                        if let Ok(content) = std::fs::read_to_string(&full_path) {
                            let mut preview = format!("(untracked: {})\n", change.path);
                            for line in content.lines().take(200) {
                                preview.push_str("+ ");
                                preview.push_str(line);
                                preview.push('\n');
                            }
                            self.diff = preview;
                        } else {
                            self.diff =
                                format!("(untracked: {})\nPress Space to stage.", change.path);
                        }
                    } else {
                        self.diff = "(no diff or binary file)".to_string();
                    }
                }
                Err(err) => {
                    self.diff = format!("diff error: {err}");
                }
            }
        } else {
            self.diff.clear();
        }
    }

    pub fn selected_change(&self) -> Option<&GitFileChange> {
        self.changes.get(self.selected)
    }

    pub fn next(&mut self) {
        if self.changes.is_empty() {
            return;
        }
        if self.selected + 1 < self.changes.len() {
            self.selected += 1;
        } else {
            self.selected = 0;
        }
        self.reload_diff();
    }

    pub fn previous(&mut self) {
        if self.changes.is_empty() {
            return;
        }
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.changes.len().saturating_sub(1);
        }
        self.reload_diff();
    }

    pub fn scroll_diff_up(&mut self, delta: u16) {
        self.diff_scroll = self.diff_scroll.saturating_sub(delta);
    }

    pub fn scroll_diff_down(&mut self, delta: u16) {
        let total_lines = self.diff.lines().count() as u16;
        let max_scroll = total_lines.saturating_sub(1);
        self.diff_scroll = (self.diff_scroll + delta).min(max_scroll);
    }

    pub fn toggle_stage(&mut self) {
        if let Some(change) = self.selected_change().cloned() {
            let res = if change.staged {
                git::unstage_file(&self.repo_path, &change.path)
            } else {
                git::stage_file(&self.repo_path, &change.path)
            };
            match res {
                Ok(()) => {
                    self.status_message = None;
                    self.refresh();
                }
                Err(e) => {
                    self.status_message = Some(format!("Error: {e}"));
                }
            }
        }
    }

    pub fn enter_commit_mode(&mut self) {
        self.commit_mode = true;
        self.status_message = None;
    }

    pub fn exit_commit_mode(&mut self) {
        self.commit_mode = false;
        self.status_message = None;
    }

    pub fn push_commit_char(&mut self, ch: char) {
        self.commit_message.push(ch);
    }

    pub fn pop_commit_char(&mut self) {
        self.commit_message.pop();
    }

    pub fn push_commit_str(&mut self, text: &str) {
        self.commit_message.push_str(text);
    }

    pub fn commit(&mut self) -> Result<String, String> {
        let msg = self.commit_message.trim();
        if msg.is_empty() {
            let err = "commit message cannot be empty".to_string();
            self.status_message = Some(err.clone());
            return Err(err);
        }
        match git::commit(&self.repo_path, msg) {
            Ok(output) => {
                self.commit_message.clear();
                self.commit_mode = false;
                self.status_message = Some(output.clone());
                self.refresh();
                Ok(output)
            }
            Err(err) => {
                self.status_message = Some(format!("commit failed: {err}"));
                Err(err)
            }
        }
    }
}

pub fn render_git_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &GitDialogState,
    theme: &Theme,
) {
    let width = area.width.clamp(60, 100).min(area.width);
    let height = area.height.clamp(16, 32).min(area.height.saturating_sub(2));

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.teal))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            " Git Panel • Changes & Diff ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, dialog_area);

    let inner = Rect {
        x: dialog_area.x + 2,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(4),
        height: dialog_area.height.saturating_sub(2),
    };

    if inner.width < 20 || inner.height < 6 {
        return;
    }

    let staged_count = dialog.changes.iter().filter(|c| c.staged).count();
    let unstaged_count = dialog.changes.iter().filter(|c| !c.staged).count();

    let bottom_height = if dialog.commit_mode { 2 } else { 1 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),             // Header
            Constraint::Length(1),             // Divider
            Constraint::Min(4),                // Content
            Constraint::Length(1),             // Divider
            Constraint::Length(bottom_height), // Footer
        ])
        .split(inner);

    let header_line = Line::from(vec![
        Span::styled(
            "CLAWCODE GIT",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                " • {} staged, {} unstaged ({} total)",
                staged_count,
                unstaged_count,
                dialog.changes.len()
            ),
            Style::default().fg(theme.dim),
        ),
    ]);
    frame.render_widget(Paragraph::new(header_line), chunks[0]);

    let divider_char = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider_char,
            Style::default().fg(theme.dim),
        ))),
        chunks[1],
    );

    let content_area = chunks[2];
    let file_list_width = 34.min(content_area.width.saturating_sub(10));
    let main_panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(file_list_width), Constraint::Min(10)])
        .split(content_area);

    let files_area = main_panes[0];
    let diff_area = main_panes[1];

    // Render file list
    let visible_files = files_area.height as usize;
    let scroll_offset = if dialog.selected >= dialog.file_scroll + visible_files {
        dialog
            .selected
            .saturating_sub(visible_files)
            .saturating_add(1)
    } else if dialog.selected < dialog.file_scroll {
        dialog.selected
    } else {
        dialog.file_scroll
    };

    let mut file_lines = Vec::new();
    if dialog.changes.is_empty() {
        file_lines.push(Line::from(Span::styled(
            " No changes present",
            Style::default().fg(theme.dim),
        )));
    } else {
        for (i, change) in dialog
            .changes
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_files)
        {
            let is_selected = i == dialog.selected;
            let (badge, badge_color) = if change.staged {
                ("[S]", theme.success)
            } else if change.status_code == "?" {
                ("[?]", theme.dim)
            } else {
                ("[U]", theme.warning)
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let prefix_style = if is_selected {
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.dim)
            };

            let badge_style = Style::default()
                .fg(badge_color)
                .add_modifier(Modifier::BOLD);

            let max_name_len = (files_area.width as usize).saturating_sub(8);
            let display_name = if change.path.len() > max_name_len && max_name_len > 3 {
                format!(
                    "…{}",
                    &change.path[change.path.len() - (max_name_len - 1)..]
                )
            } else {
                change.path.clone()
            };

            let name_style = if is_selected {
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.quiet)
            };

            file_lines.push(Line::from(vec![
                Span::styled(prefix, prefix_style),
                Span::styled(badge, badge_style),
                Span::raw(" "),
                Span::styled(display_name, name_style),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(file_lines), files_area);

    // Render diff preview pane
    let diff_title = if let Some(change) = dialog.selected_change() {
        let tag = if change.staged { "staged" } else { "working" };
        format!(" Diff: {} ({}) ", change.path, tag)
    } else {
        " Diff Preview ".to_string()
    };

    let diff_block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(theme.dim))
        .title(Span::styled(diff_title, Style::default().fg(theme.dim)));

    let diff_inner = diff_block.inner(diff_area);
    frame.render_widget(diff_block, diff_area);

    let mut diff_lines = Vec::new();
    if dialog.diff.is_empty() {
        diff_lines.push(Line::from(Span::styled(
            "No diff available",
            Style::default().fg(theme.dim),
        )));
    } else {
        for line in dialog.diff.lines() {
            let style = if line.starts_with('+') && !line.starts_with("+++") {
                Style::default().fg(theme.success)
            } else if line.starts_with('-') && !line.starts_with("---") {
                Style::default().fg(theme.error)
            } else if line.starts_with("@@") {
                Style::default().fg(theme.teal)
            } else if line.starts_with("diff ") || line.starts_with("index ") {
                Style::default().fg(theme.dim)
            } else {
                Style::default().fg(theme.quiet)
            };
            diff_lines.push(Line::from(Span::styled(line.to_string(), style)));
        }
    }

    let diff_widget = Paragraph::new(diff_lines).scroll((dialog.diff_scroll, 0));
    frame.render_widget(diff_widget, diff_inner);

    // Divider
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider_char,
            Style::default().fg(theme.dim),
        ))),
        chunks[3],
    );

    // Footer
    if dialog.commit_mode {
        let commit_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(chunks[4]);

        let input_line = Line::from(vec![
            Span::styled(
                "Commit message: ",
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&dialog.commit_message, Style::default().fg(theme.ink)),
            Span::styled("▋", Style::default().fg(theme.amber)),
        ]);
        frame.render_widget(Paragraph::new(input_line), commit_chunks[0]);

        let hint_line = Line::from(vec![
            Span::styled(
                "Enter",
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Commit  ", Style::default().fg(theme.dim)),
            Span::styled(
                "Esc",
                Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Cancel commit mode", Style::default().fg(theme.dim)),
        ]);
        frame.render_widget(Paragraph::new(hint_line), commit_chunks[1]);
    } else {
        let footer_line = Line::from(vec![
            Span::styled(
                "↑/↓",
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Select  ", Style::default().fg(theme.dim)),
            Span::styled(
                "Space",
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Stage/Unstage  ", Style::default().fg(theme.dim)),
            Span::styled(
                "c",
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Commit  ", Style::default().fg(theme.dim)),
            Span::styled(
                "PgUp/PgDn",
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Scroll diff  ", Style::default().fg(theme.dim)),
            Span::styled(
                "Esc",
                Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Close", Style::default().fg(theme.dim)),
        ]);
        frame.render_widget(Paragraph::new(footer_line), chunks[4]);
    }
}
