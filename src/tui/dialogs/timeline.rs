use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::queued_prompts::format_single_line_preview;
use crate::tui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineTurnItem {
    pub turn_number: usize,
    pub user_message_id: Option<i64>,
    pub next_user_message_id: Option<i64>,
    pub stream_part_index: usize,
    pub prompt: String,
    pub assistant_preview: String,
    pub tool_count: usize,
    pub file_changes: usize,
    pub fork_children: Vec<(i64, String)>,
    pub reverted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineDialogState {
    pub session_id: Option<i64>,
    pub session_title: String,
    pub parent_session: Option<(i64, String)>,
    pub items: Vec<TimelineTurnItem>,
    pub selected: usize,
    pub filter: String,
    pub search_active: bool,
    pub scroll_offset: usize,
}

impl TimelineDialogState {
    pub fn new(
        session_id: Option<i64>,
        session_title: impl Into<String>,
        parent_session: Option<(i64, String)>,
        items: Vec<TimelineTurnItem>,
    ) -> Self {
        let selected = items.len().saturating_sub(1);
        Self {
            session_id,
            session_title: session_title.into(),
            parent_session,
            items,
            selected,
            filter: String::new(),
            search_active: false,
            scroll_offset: 0,
        }
    }

    pub fn filtered_items(&self) -> Vec<&TimelineTurnItem> {
        if self.filter.is_empty() {
            return self.items.iter().collect();
        }
        let query = self.filter.to_lowercase();
        self.items
            .iter()
            .filter(|item| {
                item.prompt.to_lowercase().contains(&query)
                    || item.assistant_preview.to_lowercase().contains(&query)
                    || format!("#{}", item.turn_number).contains(&query)
            })
            .collect()
    }

    pub fn selected_item(&self) -> Option<&TimelineTurnItem> {
        let filtered = self.filtered_items();
        if filtered.is_empty() {
            return None;
        }
        filtered.get(self.selected.min(filtered.len() - 1)).copied()
    }

    pub fn next(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        } else {
            self.selected = 0;
        }
    }

    pub fn previous(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = if self.selected == 0 || self.selected >= count {
                count - 1
            } else {
                self.selected - 1
            };
        } else {
            self.selected = 0;
        }
    }

    pub fn push_char(&mut self, character: char) {
        self.search_active = true;
        self.filter.push(character);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn pop_char(&mut self) {
        self.filter.pop();
        if self.filter.is_empty() {
            self.search_active = false;
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

pub fn render_timeline_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &TimelineDialogState,
    theme: &Theme,
) {
    let width = area.width.clamp(52, 92).min(area.width);
    let filtered = dialog.filtered_items();
    let max_items_visible = 10.min(area.height.saturating_sub(9) as usize).max(3);
    let visible_count = filtered.len().min(max_items_visible).max(1);
    let parent_rows = u16::from(dialog.parent_session.is_some());
    let height = (visible_count as u16 + 7 + parent_rows).min(area.height.saturating_sub(2));

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
        .border_style(Style::default().fg(theme.amber))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            " Session Timeline • Turns & Branches ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, dialog_area);

    let inner = Rect {
        x: dialog_area.x + 2,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(4),
        height: dialog_area.height.saturating_sub(2),
    };

    if inner.width < 16 || inner.height < 5 {
        return;
    }

    let mut constraints = vec![Constraint::Length(1)];
    if dialog.parent_session.is_some() {
        constraints.push(Constraint::Length(1));
    }
    constraints.extend([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut chunk_idx = 0;
    let search_line = if dialog.filter.is_empty() && !dialog.search_active {
        Line::from(vec![
            Span::styled(" > ", Style::default().fg(theme.amber)),
            Span::styled(
                format!(
                    "{} ({} turns) - press / to filter",
                    dialog.session_title,
                    dialog.items.len()
                ),
                Style::default().fg(theme.dim),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme.amber)),
            Span::styled(
                &dialog.filter,
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme.amber)),
        ])
    };
    frame.render_widget(Paragraph::new(search_line), chunks[chunk_idx]);
    chunk_idx += 1;

    if let Some((parent_id, parent_title)) = &dialog.parent_session {
        let lineage_line = Line::from(vec![
            Span::styled(" ⑂ ", Style::default().fg(theme.teal)),
            Span::styled(
                format!("Branched from session #{parent_id} ({parent_title})"),
                Style::default().fg(theme.quiet),
            ),
        ]);
        frame.render_widget(Paragraph::new(lineage_line), chunks[chunk_idx]);
        chunk_idx += 1;
    }

    let divider = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider.as_str(),
            Style::default().fg(theme.dim),
        ))),
        chunks[chunk_idx],
    );
    chunk_idx += 1;

    let list_area = chunks[chunk_idx];
    chunk_idx += 1;

    if filtered.is_empty() {
        let msg = if dialog.items.is_empty() {
            "  No turns recorded in this session yet."
        } else {
            "  No turns match filter."
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                msg,
                Style::default().fg(theme.dim),
            ))),
            list_area,
        );
    } else {
        let max_rows = list_area.height as usize;
        let selected = dialog.selected.min(filtered.len() - 1);
        let start = if selected >= max_rows {
            selected + 1 - max_rows
        } else {
            0
        };
        let mut lines = Vec::new();
        let preview_budget = (inner.width as usize).saturating_sub(28).max(12);

        for (offset, item) in filtered.iter().skip(start).take(max_rows).enumerate() {
            let idx = start + offset;
            let is_selected = idx == selected;
            let indicator = if is_selected { "▸ " } else { "  " };
            let turn_badge = format!("#{:<2} ", item.turn_number);
            let prompt_preview = format_single_line_preview(&item.prompt, preview_budget);

            let mut spans = vec![
                Span::styled(
                    indicator,
                    Style::default()
                        .fg(if is_selected { theme.amber } else { theme.dim })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    turn_badge,
                    Style::default()
                        .fg(if is_selected { theme.amber } else { theme.teal })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    prompt_preview,
                    if is_selected {
                        Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.ink)
                    },
                ),
            ];

            if item.tool_count > 0 {
                spans.push(Span::styled(
                    format!("  · {} tool(s)", item.tool_count),
                    Style::default().fg(theme.teal),
                ));
            }
            if item.file_changes > 0 {
                spans.push(Span::styled(
                    format!(" · {} file(s)", item.file_changes),
                    Style::default().fg(theme.amber),
                ));
            }
            if !item.fork_children.is_empty() {
                let ids = item
                    .fork_children
                    .iter()
                    .map(|(id, _)| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(",");
                spans.push(Span::styled(
                    format!("  ⑂ {ids}"),
                    Style::default()
                        .fg(theme.success)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if item.reverted {
                spans.push(Span::styled(
                    "  [reverted]",
                    Style::default().fg(theme.warning),
                ));
            }

            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), list_area);
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider.as_str(),
            Style::default().fg(theme.dim),
        ))),
        chunks[chunk_idx],
    );
    chunk_idx += 1;

    let footer = Line::from(vec![
        Span::styled(
            "Enter",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" jump  ", Style::default().fg(theme.dim)),
        Span::styled(
            "f",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" fork branch  ", Style::default().fg(theme.dim)),
        Span::styled(
            "r",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" revert here  ", Style::default().fg(theme.dim)),
        Span::styled(
            "/",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" filter  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" close", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[chunk_idx]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_items() -> Vec<TimelineTurnItem> {
        vec![
            TimelineTurnItem {
                turn_number: 1,
                user_message_id: Some(10),
                next_user_message_id: Some(20),
                stream_part_index: 0,
                prompt: "Create initial main.rs".to_string(),
                assistant_preview: "Created src/main.rs".to_string(),
                tool_count: 1,
                file_changes: 1,
                fork_children: vec![(2, "Branch A".to_string())],
                reverted: false,
            },
            TimelineTurnItem {
                turn_number: 2,
                user_message_id: Some(20),
                next_user_message_id: None,
                stream_part_index: 3,
                prompt: "Add CLI parser tests".to_string(),
                assistant_preview: "Added unit tests".to_string(),
                tool_count: 2,
                file_changes: 1,
                fork_children: Vec::new(),
                reverted: false,
            },
        ]
    }

    #[test]
    fn timeline_state_selects_latest_and_filters() {
        let mut state = TimelineDialogState::new(
            Some(1),
            "Demo session",
            Some((0, "Root".to_string())),
            sample_items(),
        );
        assert_eq!(state.selected, 1);
        assert_eq!(state.selected_item().unwrap().turn_number, 2);

        state.previous();
        assert_eq!(state.selected_item().unwrap().turn_number, 1);

        state.push_char('p');
        state.push_char('a');
        state.push_char('r');
        assert_eq!(state.filtered_items().len(), 1);
        assert_eq!(state.selected_item().unwrap().turn_number, 2);
    }
}
