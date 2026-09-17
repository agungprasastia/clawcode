use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::provider::ModelInfo;
use crate::tui::{App, Theme};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelsDialogState {
    pub items: Vec<ModelInfo>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
}

impl ModelsDialogState {
    pub fn new(items: Vec<ModelInfo>, active_model: &str) -> Self {
        let selected = items.iter().position(|m| m.id == active_model).unwrap_or(0);
        Self {
            items,
            selected,
            filter: String::new(),
            scroll_offset: 0,
        }
    }

    pub fn filtered_items(&self) -> Vec<&ModelInfo> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let q = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|m| m.id.to_lowercase().contains(&q))
                .collect()
        }
    }

    pub fn selected_model(&self) -> Option<&ModelInfo> {
        let filtered = self.filtered_items();
        filtered.get(self.selected).copied()
    }

    pub fn next(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    pub fn previous(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = if self.selected == 0 {
                count - 1
            } else {
                self.selected - 1
            };
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

pub fn format_context_window(ctx: u64) -> String {
    if ctx == 0 {
        String::new()
    } else if ctx >= 1_000_000 {
        format!("{}M", ctx / 1_000_000)
    } else if ctx >= 1_000 {
        format!("{}k", ctx / 1_000)
    } else {
        format!("{ctx}")
    }
}

pub fn render_model_suggestions_popup(
    frame: &mut Frame<'_>,
    input_area: Rect,
    app: &App,
    theme: &Theme,
) {
    let suggestions = app.matching_model_suggestions();
    if suggestions.is_empty() {
        return;
    }

    let available_space = input_area.y as usize;
    if available_space < 3 {
        return;
    }
    let max_visible = 8.min(available_space - 2);
    let visible_count = suggestions.len().min(max_visible);
    if visible_count == 0 {
        return;
    }
    let popup_height = (visible_count as u16) + 2;

    let popup_y = input_area.y.saturating_sub(popup_height);
    let popup_width = input_area.width.min(70);
    let popup_x = input_area.x;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    frame.render_widget(Clear, popup_area);

    let selected_idx = app.selected_suggestion_index();
    let scroll_offset = if selected_idx >= visible_count {
        selected_idx + 1 - visible_count
    } else {
        0
    };

    let active_model = app.selected_model();

    let items: Vec<Line> = suggestions
        .iter()
        .skip(scroll_offset)
        .take(visible_count)
        .enumerate()
        .map(|(rel_idx, model_id)| {
            let actual_idx = scroll_offset + rel_idx;
            let is_selected = actual_idx == selected_idx;
            let is_active = model_id.as_str() == active_model;

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

            let badge = if is_active { " [active]" } else { "" };
            let badge_style = Style::default().fg(theme.success);

            let row_style = if is_selected {
                Style::default().bg(theme.panel)
            } else {
                Style::default()
            };

            Line::from(vec![
                Span::styled(cursor, cursor_style),
                Span::styled(active_dot, dot_style),
                Span::styled(model_id.clone(), name_style),
                Span::styled(badge, badge_style),
            ])
            .style(row_style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.amber))
        .style(Style::default().bg(theme.bg_element))
        .title(Span::styled(
            " Select Model (↑/↓ navigate, Tab/Enter select) ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Paragraph::new(items).block(block), popup_area);
}

pub fn render_models_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &ModelsDialogState,
    active_model: &str,
    theme: &Theme,
) {
    let width = area.width.clamp(36, 74);
    let filtered = dialog.filtered_items();
    let max_items_visible = 12.min(area.height.saturating_sub(8) as usize).max(3);
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
        format!(" Select Model ({total_count}) ")
    } else {
        format!(" Select Model ({filtered_count}/{total_count}) ")
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
            Span::styled("Search: ", Style::default().fg(theme.dim)),
            Span::styled(
                "type to filter...",
                Style::default()
                    .fg(theme.quiet)
                    .add_modifier(Modifier::ITALIC),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "Search: ",
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
    let selected_idx = dialog.selected;
    let scroll_offset = if selected_idx >= list_height {
        selected_idx + 1 - list_height
    } else {
        0
    };

    let item_lines: Vec<Line> = if filtered.is_empty() {
        vec![Line::from(Span::styled(
            format!("  No models matching \"{}\"", dialog.filter),
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
            .map(|(rel_idx, model)| {
                let actual_idx = scroll_offset + rel_idx;
                let is_selected = actual_idx == selected_idx;
                let is_active = model.id.as_str() == active_model;

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

                let ctx_str = if model.context_window > 0 {
                    format_context_window(model.context_window)
                } else {
                    String::new()
                };

                let badge = if is_active {
                    if !ctx_str.is_empty() {
                        format!("{ctx_str} [active]")
                    } else {
                        "[active]".to_string()
                    }
                } else {
                    ctx_str
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
                let badge_len = badge.len();
                let available_for_name = row_width.saturating_sub(prefix_len + badge_len + 2);

                let display_name = if model.id.len() > available_for_name && available_for_name > 4
                {
                    format!("{}…", &model.id[..available_for_name - 1])
                } else {
                    model.id.clone()
                };

                let pad_len = row_width.saturating_sub(prefix_len + display_name.len() + badge_len);

                let line_style = if is_selected {
                    Style::default().bg(theme.bg_element)
                } else {
                    Style::default()
                };

                Line::from(vec![
                    Span::styled(cursor, cursor_style),
                    Span::styled(active_dot, dot_style),
                    Span::styled(display_name, name_style),
                    Span::raw(" ".repeat(pad_len)),
                    Span::styled(badge, badge_style),
                ])
                .style(line_style)
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

    fn sample_models() -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "anthropic/claude-3-5-sonnet".to_string(),
                context_window: 200_000,
            },
            ModelInfo {
                id: "openai/gpt-4o".to_string(),
                context_window: 128_000,
            },
            ModelInfo {
                id: "google/gemini-pro".to_string(),
                context_window: 1_000_000,
            },
        ]
    }

    #[test]
    fn test_models_dialog_state_new() {
        let models = sample_models();
        let state = ModelsDialogState::new(models.clone(), "openai/gpt-4o");
        assert_eq!(state.selected, 1);
        assert!(state.filter.is_empty());
        assert_eq!(state.scroll_offset, 0);
        assert_eq!(state.items.len(), 3);

        let fallback = ModelsDialogState::new(models, "nonexistent");
        assert_eq!(fallback.selected, 0);

        let empty = ModelsDialogState::new(Vec::new(), "any");
        assert_eq!(empty.selected, 0);
        assert!(empty.items.is_empty());
    }

    #[test]
    fn test_models_dialog_state_push_pop_char() {
        let mut state = ModelsDialogState::new(sample_models(), "openai/gpt-4o");
        state.selected = 1;
        state.scroll_offset = 2;

        state.push_char('g');
        state.push_char('p');
        assert_eq!(state.filter, "gp");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "g");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "");
    }

    #[test]
    fn test_models_dialog_state_navigation() {
        let mut state = ModelsDialogState::new(sample_models(), "anthropic/claude-3-5-sonnet");
        assert_eq!(state.selected, 0);

        state.next();
        assert_eq!(state.selected, 1);

        state.next();
        assert_eq!(state.selected, 2);

        state.next();
        assert_eq!(state.selected, 0);

        state.previous();
        assert_eq!(state.selected, 2);

        state.previous();
        assert_eq!(state.selected, 1);

        let mut empty = ModelsDialogState::new(Vec::new(), "none");
        empty.next();
        assert_eq!(empty.selected, 0);
        empty.previous();
        assert_eq!(empty.selected, 0);
    }

    #[test]
    fn test_models_dialog_state_filtered_items() {
        let mut state = ModelsDialogState::new(sample_models(), "anthropic/claude-3-5-sonnet");
        assert_eq!(state.filtered_items().len(), 3);

        state.push_char('C');
        state.push_char('L');
        state.push_char('A');
        state.push_char('U');
        state.push_char('D');
        state.push_char('E');
        let filtered = state.filtered_items();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "anthropic/claude-3-5-sonnet");

        state.filter = "nomatch_xyz".to_string();
        assert!(state.filtered_items().is_empty());
    }

    #[test]
    fn test_models_dialog_state_selected_model() {
        let mut state = ModelsDialogState::new(sample_models(), "openai/gpt-4o");
        assert_eq!(
            state.selected_model().map(|m| m.id.as_str()),
            Some("openai/gpt-4o")
        );

        state.filter = "nonexistent".to_string();
        assert_eq!(state.selected_model(), None);
    }

    #[test]
    fn test_format_context_window() {
        assert_eq!(format_context_window(0), "");
        assert_eq!(format_context_window(500), "500");
        assert_eq!(format_context_window(1_000), "1k");
        assert_eq!(format_context_window(128_000), "128k");
        assert_eq!(format_context_window(1_000_000), "1M");
        assert_eq!(format_context_window(2_000_000), "2M");
    }
}
