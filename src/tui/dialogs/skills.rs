use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::tui::Theme;
use crate::workspace::skills::{SkillItem, SkillSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsDialogState {
    pub skills: Vec<SkillItem>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
    pub preview_scroll: u16,
}

impl SkillsDialogState {
    pub fn new(skills: Vec<SkillItem>) -> Self {
        Self {
            skills,
            selected: 0,
            filter: String::new(),
            scroll_offset: 0,
            preview_scroll: 0,
        }
    }

    pub fn filtered_skills(&self) -> Vec<&SkillItem> {
        if self.filter.is_empty() {
            self.skills.iter().collect()
        } else {
            let q = self.filter.to_lowercase();
            self.skills
                .iter()
                .filter(|s| {
                    s.name.to_lowercase().contains(&q)
                        || s.description.to_lowercase().contains(&q)
                        || s.instructions.to_lowercase().contains(&q)
                })
                .collect()
        }
    }

    pub fn selected_skill(&self) -> Option<&SkillItem> {
        let filtered = self.filtered_skills();
        if filtered.is_empty() {
            None
        } else {
            let idx = self.selected.min(filtered.len().saturating_sub(1));
            filtered.get(idx).copied()
        }
    }

    pub fn next(&mut self) {
        let count = self.filtered_skills().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        } else {
            self.selected = 0;
        }
        self.preview_scroll = 0;
    }

    pub fn previous(&mut self) {
        let count = self.filtered_skills().len();
        if count > 0 {
            self.selected = if self.selected == 0 || self.selected >= count {
                count - 1
            } else {
                self.selected - 1
            };
        } else {
            self.selected = 0;
        }
        self.preview_scroll = 0;
    }

    pub fn push_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.scroll_offset = 0;
        self.preview_scroll = 0;
    }

    pub fn pop_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.scroll_offset = 0;
        self.preview_scroll = 0;
    }

    pub fn scroll_preview_up(&mut self, lines: u16) {
        self.preview_scroll = self.preview_scroll.saturating_sub(lines);
    }

    pub fn scroll_preview_down(&mut self, lines: u16) {
        self.preview_scroll = self.preview_scroll.saturating_add(lines);
    }
}

pub fn render_skills_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &SkillsDialogState,
    theme: &Theme,
) {
    let width = (area.width.saturating_sub(4)).clamp(50, 96).min(area.width);
    let height = (area.height.saturating_sub(2))
        .clamp(16, 32)
        .min(area.height);

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let filtered = dialog.filtered_skills();
    let total_count = dialog.skills.len();
    let filtered_count = filtered.len();

    let title_text = if dialog.filter.is_empty() {
        format!(" Skills Library ({total_count}) ")
    } else {
        format!(" Skills Library ({filtered_count}/{total_count}) ")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.teal))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            title_text,
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

    let filter_line = Line::from(vec![
        Span::styled("Search: ", Style::default().fg(theme.dim)),
        Span::styled(
            &dialog.filter,
            Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(theme.teal)),
    ]);
    frame.render_widget(Paragraph::new(filter_line), chunks[0]);

    let divider = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider,
            Style::default().fg(theme.dim),
        ))),
        chunks[1],
    );

    let list_width = (chunks[2].width * 42 / 100)
        .clamp(24, 40)
        .min(chunks[2].width.saturating_sub(10));
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(list_width),
            Constraint::Length(1),
            Constraint::Min(10),
        ])
        .split(chunks[2]);

    let list_area = panes[0];
    let v_divider_area = panes[1];
    let preview_area = panes[2];

    let v_divider_lines: Vec<Line> = (0..chunks[2].height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(theme.dim))))
        .collect();
    frame.render_widget(Paragraph::new(v_divider_lines), v_divider_area);

    let visible_rows = (list_area.height as usize).max(1);
    let selected_idx = dialog.selected.min(filtered.len().saturating_sub(1));
    let scroll_offset = if selected_idx >= dialog.scroll_offset + visible_rows {
        selected_idx.saturating_sub(visible_rows).saturating_add(1)
    } else if selected_idx < dialog.scroll_offset {
        selected_idx
    } else {
        dialog.scroll_offset
    };

    let mut list_lines = Vec::new();
    if filtered.is_empty() {
        list_lines.push(Line::from(Span::styled(
            " No skills found",
            Style::default().fg(theme.dim),
        )));
    } else {
        for (i, skill) in filtered
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_rows)
        {
            let is_selected = i == selected_idx;
            let badge = format!("[{}] ", skill.source.label());

            let prefix = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.ink)
            };

            list_lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(badge, Style::default().fg(theme.dim)),
                Span::styled(&skill.name, style),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(list_lines), list_area);

    if let Some(selected_skill) = dialog.selected_skill() {
        let source_str = match selected_skill.source {
            SkillSource::Project => "Project skill",
            SkillSource::Global => "Global skill",
            SkillSource::Agents => "Agents skill",
            SkillSource::OpenCode => "OpenCode compat skill",
            SkillSource::Claude => "Claude skill",
        };

        let mut preview_lines = Vec::new();
        preview_lines.push(Line::from(vec![
            Span::styled(
                &selected_skill.name,
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  ({source_str})"), Style::default().fg(theme.dim)),
        ]));
        preview_lines.push(Line::from(Span::styled(
            selected_skill.location.display().to_string(),
            Style::default().fg(theme.dim),
        )));
        preview_lines.push(Line::default());

        if !selected_skill.description.is_empty() {
            preview_lines.push(Line::from(Span::styled(
                "Description:",
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            )));
            preview_lines.push(Line::from(Span::styled(
                &selected_skill.description,
                Style::default().fg(theme.ink),
            )));
            preview_lines.push(Line::default());
        }

        preview_lines.push(Line::from(Span::styled(
            "Instructions:",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        )));
        for line in selected_skill.instructions.lines() {
            preview_lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(theme.ink),
            )));
        }

        let preview_widget = Paragraph::new(preview_lines)
            .scroll((dialog.preview_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(preview_widget, preview_area);
    } else {
        let empty_msg = Paragraph::new(Line::from(Span::styled(
            "Select a skill to view details",
            Style::default().fg(theme.dim),
        )));
        frame.render_widget(empty_msg, preview_area);
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider,
            Style::default().fg(theme.dim),
        ))),
        chunks[3],
    );

    let help_line = Line::from(vec![
        Span::styled(
            "↑/↓",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Select  ", Style::default().fg(theme.dim)),
        Span::styled(
            "PgUp/PgDn",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Scroll  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Enter",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Choose  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Esc",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Close", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(help_line), chunks[4]);
}
