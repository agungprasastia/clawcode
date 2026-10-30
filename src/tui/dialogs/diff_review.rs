use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::Theme;
use crate::tui::diff::{DiffOp, compute_diff, compute_side_by_side_diff};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffViewMode {
    #[default]
    Unified,
    Split,
}

impl DiffViewMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Unified => Self::Split,
            Self::Split => Self::Unified,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unified => "Unified",
            Self::Split => "Split",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFileDiff {
    pub path: String,
    pub status: char,
    pub before: String,
    pub after: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReviewDialogState {
    pub files: Vec<SessionFileDiff>,
    pub selected: usize,
    pub view_mode: DiffViewMode,
    pub diff_scroll: u16,
    pub file_scroll: usize,
}

impl DiffReviewDialogState {
    pub fn new(files: Vec<SessionFileDiff>) -> Self {
        Self {
            files,
            selected: 0,
            view_mode: DiffViewMode::Unified,
            diff_scroll: 0,
            file_scroll: 0,
        }
    }

    pub fn selected_file(&self) -> Option<&SessionFileDiff> {
        self.files.get(self.selected)
    }

    pub fn next(&mut self) {
        if self.files.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.files.len();
        self.diff_scroll = 0;
    }

    pub fn previous(&mut self) {
        if self.files.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.files.len() - 1
        } else {
            self.selected - 1
        };
        self.diff_scroll = 0;
    }

    pub fn toggle_view_mode(&mut self) {
        self.view_mode = self.view_mode.toggle();
        self.diff_scroll = 0;
    }

    pub fn scroll_up(&mut self, delta: u16) {
        self.diff_scroll = self.diff_scroll.saturating_sub(delta);
    }

    pub fn scroll_down(&mut self, delta: u16) {
        self.diff_scroll = self.diff_scroll.saturating_add(delta);
    }
}

pub fn render_diff_review_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &DiffReviewDialogState,
    theme: &Theme,
) {
    let width = area.width.clamp(60, 106).min(area.width);
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
            " Session Diff Review • Workspace Changes ",
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

    let total_add: usize = dialog.files.iter().map(|f| f.additions).sum();
    let total_del: usize = dialog.files.iter().map(|f| f.deletions).sum();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let header_line = Line::from(vec![
        Span::styled(
            "SESSION DIFF",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" • {} file(s)  ", dialog.files.len()),
            Style::default().fg(theme.ink),
        ),
        Span::styled(
            format!("+{total_add}"),
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / ", Style::default().fg(theme.dim)),
        Span::styled(
            format!("-{total_del}"),
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  •  View: ", Style::default().fg(theme.dim)),
        Span::styled(
            format!("[{}]", dialog.view_mode.label()),
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(header_line), chunks[0]);

    let divider = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider.as_str(),
            Style::default().fg(theme.dim),
        ))),
        chunks[1],
    );

    let content_area = chunks[2];
    let file_list_width = 32.min(content_area.width.saturating_sub(14));
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(file_list_width), Constraint::Min(12)])
        .split(content_area);

    let files_area = panes[0];
    let diff_area = panes[1];

    if dialog.files.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  No modified files in session.",
                Style::default().fg(theme.dim),
            ))),
            files_area,
        );
    } else {
        let visible_files = files_area.height as usize;
        let selected = dialog.selected.min(dialog.files.len() - 1);
        let start = if selected >= visible_files {
            selected + 1 - visible_files
        } else {
            0
        };
        let mut file_lines = Vec::new();
        for (offset, file) in dialog
            .files
            .iter()
            .skip(start)
            .take(visible_files)
            .enumerate()
        {
            let idx = start + offset;
            let is_selected = idx == selected;
            let indicator = if is_selected { "▸ " } else { "  " };
            let status_color = match file.status {
                'A' => theme.success,
                'D' => theme.error,
                _ => theme.amber,
            };
            let max_path = (file_list_width as usize).saturating_sub(14).max(6);
            let display_path: String = if file.path.chars().count() > max_path {
                let mut s: String = file.path.chars().take(max_path.saturating_sub(1)).collect();
                s.push('…');
                s
            } else {
                file.path.clone()
            };

            file_lines.push(Line::from(vec![
                Span::styled(
                    indicator,
                    Style::default().fg(if is_selected { theme.amber } else { theme.dim }),
                ),
                Span::styled(
                    format!("{} ", file.status),
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<width$} ", display_path, width = max_path),
                    if is_selected {
                        Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.ink)
                    },
                ),
                Span::styled(
                    format!("+{}", file.additions),
                    Style::default().fg(theme.success),
                ),
                Span::styled(" ", Style::default()),
                Span::styled(
                    format!("-{}", file.deletions),
                    Style::default().fg(theme.error),
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(file_lines), files_area);
    }

    if let Some(file) = dialog.selected_file() {
        let mut rendered_lines = Vec::new();
        rendered_lines.push(Line::from(vec![
            Span::styled(" │ ", Style::default().fg(theme.dim)),
            Span::styled(
                format!(
                    "{} ({})",
                    file.path,
                    dialog.view_mode.label().to_lowercase()
                ),
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        match dialog.view_mode {
            DiffViewMode::Unified => {
                let diff = compute_diff(&file.before, &file.after, 2000);
                if diff.lines.is_empty() {
                    rendered_lines.push(Line::from(vec![
                        Span::styled(" │ ", Style::default().fg(theme.dim)),
                        Span::styled("(no line changes)", Style::default().fg(theme.dim)),
                    ]));
                } else {
                    for line in diff.lines {
                        let (prefix, style) = match line.op {
                            DiffOp::Add => ("+ ", Style::default().fg(theme.success)),
                            DiffOp::Remove => ("- ", Style::default().fg(theme.error)),
                            DiffOp::Same => ("  ", Style::default().fg(theme.ink)),
                        };
                        rendered_lines.push(Line::from(vec![
                            Span::styled(" │ ", Style::default().fg(theme.dim)),
                            Span::styled(format!("{prefix}{}", line.text), style),
                        ]));
                    }
                }
            }
            DiffViewMode::Split => {
                let rows = compute_side_by_side_diff(&file.before, &file.after, 1, 500);
                let col_w = (diff_area.width as usize).saturating_sub(7) / 2;
                let col_w = col_w.max(10);
                if rows.is_empty() {
                    rendered_lines.push(Line::from(vec![
                        Span::styled(" │ ", Style::default().fg(theme.dim)),
                        Span::styled("(no line changes)", Style::default().fg(theme.dim)),
                    ]));
                } else {
                    for row in rows {
                        let left_cell =
                            format_split_cell(row.left_num, row.left_sign, &row.left_text, col_w);
                        let right_cell = format_split_cell(
                            row.right_num,
                            row.right_sign,
                            &row.right_text,
                            col_w,
                        );
                        let left_style = match row.left_sign {
                            Some('-') => Style::default().fg(theme.error),
                            _ => Style::default().fg(theme.ink),
                        };
                        let right_style = match row.right_sign {
                            Some('+') => Style::default().fg(theme.success),
                            _ => Style::default().fg(theme.ink),
                        };
                        rendered_lines.push(Line::from(vec![
                            Span::styled(" │ ", Style::default().fg(theme.dim)),
                            Span::styled(left_cell, left_style),
                            Span::styled(" │ ", Style::default().fg(theme.dim)),
                            Span::styled(right_cell, right_style),
                        ]));
                    }
                }
            }
        }

        let max_scroll = rendered_lines.len().saturating_sub(1) as u16;
        let scroll = dialog.diff_scroll.min(max_scroll);
        frame.render_widget(
            Paragraph::new(rendered_lines).scroll((scroll, 0)),
            diff_area,
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider.as_str(),
            Style::default().fg(theme.dim),
        ))),
        chunks[3],
    );

    let footer = Line::from(vec![
        Span::styled(
            "Up/Down",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" file  ", Style::default().fg(theme.dim)),
        Span::styled(
            "j/k",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" scroll  ", Style::default().fg(theme.dim)),
        Span::styled(
            "v/Tab",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" unified/split  ", Style::default().fg(theme.dim)),
        Span::styled(
            "u",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" undo turn  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Esc",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" close", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[4]);
}

fn format_split_cell(
    num: Option<usize>,
    sign: Option<char>,
    text: &str,
    col_width: usize,
) -> String {
    let num_str = match num {
        Some(n) => format!("{n:>3}"),
        None => "   ".to_string(),
    };
    let sign_ch = sign.unwrap_or(' ');
    let prefix = format!("{num_str} {sign_ch} ");
    let max_text = col_width.saturating_sub(prefix.len());
    let mut cell: String = text.chars().take(max_text).collect();
    let count = cell.chars().count();
    if count < max_text {
        cell.push_str(&" ".repeat(max_text - count));
    }
    format!("{prefix}{cell}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_review_state_cycles_files_and_toggles_view_mode() {
        let files = vec![
            SessionFileDiff {
                path: "src/main.rs".to_string(),
                status: 'M',
                before: "fn main() {}\n".to_string(),
                after: "fn main() {\n    println!(\"hi\");\n}\n".to_string(),
                additions: 2,
                deletions: 1,
            },
            SessionFileDiff {
                path: "src/lib.rs".to_string(),
                status: 'A',
                before: String::new(),
                after: "pub mod cli;\n".to_string(),
                additions: 1,
                deletions: 0,
            },
        ];
        let mut dialog = DiffReviewDialogState::new(files);
        assert_eq!(dialog.view_mode, DiffViewMode::Unified);
        assert_eq!(dialog.selected_file().unwrap().path, "src/main.rs");

        dialog.next();
        assert_eq!(dialog.selected_file().unwrap().path, "src/lib.rs");

        dialog.toggle_view_mode();
        assert_eq!(dialog.view_mode, DiffViewMode::Split);
    }
}
