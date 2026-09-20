use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::{App, Theme};

pub fn render_model_suggestions_popup(
    frame: &mut Frame<'_>,
    input_area: Rect,
    app: &App,
    theme: &Theme,
) {
    let suggestions = app.matching_model_suggestions();
    if suggestions.is_empty() {
        app.set_last_popup_area(None);
        return;
    }

    if input_area.width < 10 {
        app.set_last_popup_area(None);
        return;
    }
    let available_space = input_area.y as usize;
    if available_space < 3 {
        app.set_last_popup_area(None);
        return;
    }
    let max_visible = 8.min(available_space.saturating_sub(2));
    let visible_count = suggestions.len().min(max_visible);
    if visible_count == 0 {
        app.set_last_popup_area(None);
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
    app.set_last_popup_area(Some(popup_area));

    frame.render_widget(Clear, popup_area);

    let selected_idx = app.selected_suggestion_index();
    let scroll_offset = if selected_idx >= visible_count {
        (selected_idx + 1).saturating_sub(visible_count)
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
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)
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
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)
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
        .border_style(Style::default().fg(theme.teal))
        .style(Style::default().bg(theme.bg_element))
        .title(Span::styled(
            " Select Model (↑/↓ navigate, Tab/Enter select) ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Paragraph::new(items).block(block), popup_area);
}
