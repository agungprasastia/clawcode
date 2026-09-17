use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::{Theme, ThemeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemesDialogState {
    pub items: Vec<ThemeKind>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
}

impl ThemesDialogState {
    pub fn new(active_theme: ThemeKind) -> Self {
        let items = ThemeKind::ALL.to_vec();
        let selected = items.iter().position(|t| *t == active_theme).unwrap_or(0);
        Self {
            items,
            selected,
            filter: String::new(),
            scroll_offset: 0,
        }
    }

    pub fn filtered_items(&self) -> Vec<ThemeKind> {
        if self.filter.is_empty() {
            self.items.clone()
        } else {
            let q = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|t| {
                    t.name().to_lowercase().contains(&q)
                        || t.description().to_lowercase().contains(&q)
                        || t.id().to_lowercase().contains(&q)
                })
                .copied()
                .collect()
        }
    }

    pub fn selected_theme(&self) -> Option<ThemeKind> {
        let filtered = self.filtered_items();
        if filtered.is_empty() {
            return None;
        }
        let idx = self.selected.min(filtered.len() - 1);
        filtered.get(idx).copied()
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

    pub fn push_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn pop_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

pub fn render_themes_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &ThemesDialogState,
    active_theme: ThemeKind,
    theme: &Theme,
) {
    let width = area.width.clamp(36, 74).min(area.width);
    let filtered = dialog.filtered_items();
    let max_items_visible = 10.min(area.height.saturating_sub(8) as usize).max(3);
    let visible_items_count = filtered.len().min(max_items_visible).max(1);
    let height = (visible_items_count as u16 + 6).min(area.height.saturating_sub(2));

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let total_count = dialog.items.len();
    let filtered_count = filtered.len();
    let title_text = if dialog.filter.is_empty() {
        format!(" Color Theme ({total_count}) ")
    } else {
        format!(" Color Theme ({filtered_count}/{total_count}) ")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.amber))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            title_text,
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

    if inner.width < 10 || inner.height < 4 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // search input
            Constraint::Length(1), // top divider
            Constraint::Min(1),    // items
            Constraint::Length(1), // bottom divider
            Constraint::Length(1), // footer hints
        ])
        .split(inner);

    let search_line = if dialog.filter.is_empty() {
        Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(theme.dim)),
            Span::styled(
                "type to filter themes...",
                Style::default()
                    .fg(theme.quiet)
                    .add_modifier(Modifier::ITALIC),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "Filter: ",
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &dialog.filter,
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme.amber)),
        ])
    };
    frame.render_widget(Paragraph::new(search_line), chunks[0]);

    let divider_str = "─".repeat(chunks[1].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider_str,
            Style::default().fg(theme.quiet),
        ))),
        chunks[1],
    );

    let list_height = chunks[2].height as usize;
    let selected_idx = if filtered.is_empty() {
        0
    } else {
        dialog.selected.min(filtered.len() - 1)
    };
    let scroll_offset = if selected_idx >= list_height {
        (selected_idx + 1).saturating_sub(list_height)
    } else {
        0
    };

    let item_lines: Vec<Line> = if filtered.is_empty() {
        vec![Line::from(Span::styled(
            format!("  No themes matching \"{}\"", dialog.filter),
            Style::default()
                .fg(theme.dim)
                .add_modifier(Modifier::ITALIC),
        ))]
    } else {
        filtered
            .iter()
            .skip(scroll_offset)
            .take(list_height)
            .enumerate()
            .map(|(rel_idx, &theme_kind)| {
                let actual_idx = scroll_offset + rel_idx;
                let is_selected = actual_idx == selected_idx;
                let is_active = theme_kind == active_theme;

                let cursor = if is_selected { " › " } else { "   " };
                let active_dot = if is_active { "● " } else { "  " };

                let cursor_style = if is_selected {
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let dot_style = if is_active {
                    Style::default()
                        .fg(theme.success)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.dim)
                };

                let name_style = if is_selected {
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
                } else if is_active {
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.ink)
                };

                let preview_theme = theme_kind.to_theme();
                let swatches = vec![
                    Span::styled("█", Style::default().fg(preview_theme.bg_element)),
                    Span::styled("█", Style::default().fg(preview_theme.panel)),
                    Span::styled("█", Style::default().fg(preview_theme.amber)),
                    Span::styled("█", Style::default().fg(preview_theme.teal)),
                    Span::styled("█", Style::default().fg(preview_theme.ink)),
                    Span::raw(" "),
                ];

                let badge = if is_active {
                    format!("{} [active]", theme_kind.description())
                } else {
                    theme_kind.description().to_string()
                };

                let badge_style = if is_active {
                    Style::default()
                        .fg(theme.success)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.quiet)
                };

                let row_width = chunks[2].width as usize;
                let prefix_len = cursor.len() + active_dot.len();
                let swatch_len = 6;
                let badge_len = badge.len();
                let available_for_name =
                    row_width.saturating_sub(prefix_len + swatch_len + badge_len + 2);

                let display_name =
                    if theme_kind.name().len() > available_for_name && available_for_name > 4 {
                        format!("{}…", &theme_kind.name()[..available_for_name - 1])
                    } else {
                        theme_kind.name().to_string()
                    };

                let pad_len = row_width
                    .saturating_sub(prefix_len + swatch_len + display_name.len() + badge_len);

                let line_style = if is_selected {
                    Style::default().bg(theme.bg_element)
                } else {
                    Style::default()
                };

                let mut spans = vec![
                    Span::styled(cursor, cursor_style),
                    Span::styled(active_dot, dot_style),
                ];
                spans.extend(swatches);
                spans.push(Span::styled(display_name, name_style));
                spans.push(Span::raw(" ".repeat(pad_len)));
                spans.push(Span::styled(badge, badge_style));

                Line::from(spans).style(line_style)
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(item_lines), chunks[2]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider_str,
            Style::default().fg(theme.quiet),
        ))),
        chunks[3],
    );

    let footer_line = Line::from(vec![
        Span::styled(
            "↑/↓ ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("navigate  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Enter ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("select  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Esc ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("close  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Type ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("filter", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(footer_line), chunks[4]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_themes_dialog_state_new() {
        let state = ThemesDialogState::new(ThemeKind::ClawcodeDark);
        assert_eq!(state.items, ThemeKind::ALL);
        assert_eq!(state.selected, 0);
        assert!(state.filter.is_empty());
        assert_eq!(state.scroll_offset, 0);

        if ThemeKind::ALL.len() > 1 {
            let second_theme = ThemeKind::ALL[1];
            let state2 = ThemesDialogState::new(second_theme);
            assert_eq!(state2.selected, 1);
        }
    }

    #[test]
    fn test_themes_dialog_state_push_pop_char() {
        let mut state = ThemesDialogState::new(ThemeKind::ClawcodeDark);
        state.selected = 2;
        state.scroll_offset = 1;

        state.push_char('c');
        state.push_char('l');
        assert_eq!(state.filter, "cl");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "c");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_themes_dialog_state_navigation() {
        let mut state = ThemesDialogState::new(ThemeKind::ClawcodeDark);
        let count = state.filtered_items().len();
        assert!(count > 1);

        state.next();
        assert_eq!(state.selected, 1);

        state.previous();
        assert_eq!(state.selected, 0);

        state.previous();
        assert_eq!(state.selected, count - 1);

        state.next();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_themes_dialog_state_filtered_items() {
        let mut state = ThemesDialogState::new(ThemeKind::ClawcodeDark);
        assert_eq!(state.filtered_items().len(), ThemeKind::ALL.len());

        state.push_char('c');
        state.push_char('l');
        state.push_char('a');
        state.push_char('w');
        let filtered = state.filtered_items();
        assert!(!filtered.is_empty());
        assert!(
            filtered
                .iter()
                .all(|t| t.id().contains("claw") || t.name().to_lowercase().contains("claw"))
        );

        state.filter = "non_existent_theme_query_xyz".to_string();
        assert!(state.filtered_items().is_empty());
    }

    #[test]
    fn test_themes_dialog_state_selected_theme() {
        let mut state = ThemesDialogState::new(ThemeKind::ClawcodeDark);
        assert_eq!(state.selected_theme(), Some(ThemeKind::ClawcodeDark));

        state.filter = "non_existent_theme_query_xyz".to_string();
        assert_eq!(state.selected_theme(), None);
    }
}
