use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{App, Theme};

pub(crate) fn render_session_tabs(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let tabs = app.open_session_tabs();
    if tabs.is_empty() || area.width == 0 {
        return;
    }
    let active = app.active_session_id();
    let mut spans = vec![Span::raw(" ")];
    let budget = area.width as usize;
    let mut used = 1usize;
    let mut shown = 0usize;

    for session_id in tabs {
        let title = app.session_tab_title(*session_id).unwrap_or("untitled");
        let is_active = active == Some(*session_id);
        let marker = if is_active { "▎" } else { " " };
        let label = truncate(&format!("{marker}{}. {}", shown + 1, title), 24);
        let width = Span::raw(&label).width();
        if used + width + 3 > budget {
            break;
        }
        spans.push(Span::styled(
            label,
            if is_active {
                Style::default()
                    .fg(theme.amber)
                    .bg(theme.panel)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.quiet)
            },
        ));
        spans.push(Span::styled(" │ ", Style::default().fg(theme.dim)));
        used += width + 3;
        shown += 1;
    }

    if shown < tabs.len() {
        spans.push(Span::styled(
            format!("+{} ", tabs.len() - shown),
            Style::default().fg(theme.dim),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.bg_element)),
        area,
    );
}

fn truncate(text: &str, max_width: usize) -> String {
    if Span::raw(text).width() <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = Span::raw(character.to_string()).width();
        if width + character_width > max_width.saturating_sub(1) {
            break;
        }
        result.push(character);
        width += character_width;
    }
    result.push('…');
    result
}
