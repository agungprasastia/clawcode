use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WhichKeyState {
    pub visible: bool,
}

impl WhichKeyState {
    pub fn new() -> Self {
        Self { visible: false }
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }
}

pub fn render_which_key(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let width = area.width.clamp(36, 72);
    let height = 14.min(area.height.saturating_sub(2));

    let popup_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.amber))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            " Keyboard Shortcuts (Cheatsheet) ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, popup_area);

    let inner = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(2),
    };

    if inner.width < 10 || inner.height < 4 {
        return;
    }

    let shortcuts: [(&str, &str, &str, &str); 7] = [
        ("Tab", "Toggle Plan/Build", "a", "Open Agents dialog"),
        ("Ctrl+X", "Toggle Shortcuts", "t", "Open Themes dialog"),
        ("Ctrl+C", "Cancel Turn", "m", "Open Models dialog"),
        ("Ctrl+L", "Clear Screen", "s", "System Status dialog"),
        ("Esc", "Dismiss Dialog / Panel", "p", "Switch to Plan mode"),
        ("↑ / ↓", "History & Selection", "b", "Switch to Build mode"),
        (
            "/connect",
            "Connect AI provider",
            "/sessions",
            "List saved sessions",
        ),
    ];

    let col_w = (inner.width as usize).saturating_sub(2) / 2;
    let mut lines = Vec::new();

    for (k1, d1, k2, d2) in shortcuts {
        let left_key_w = 8.min(col_w.saturating_sub(1));
        let left_desc_w = col_w.saturating_sub(left_key_w + 3);
        let right_key_w = 8.min(col_w.saturating_sub(1));

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:<width$}", k1, width = left_key_w),
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<width$}", d1, width = left_desc_w),
                Style::default().fg(theme.ink),
            ),
            Span::styled(" │ ", Style::default().fg(theme.dim)),
            Span::styled(
                format!("{:<width$}", k2, width = right_key_w),
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ),
            Span::styled(d2, Style::default().fg(theme.ink)),
        ]));
    }

    // Add footer
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Press ", Style::default().fg(theme.dim)),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" or ", Style::default().fg(theme.dim)),
        Span::styled(
            "Ctrl+X",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to dismiss, or press highlighted keys directly.",
            Style::default().fg(theme.dim),
        ),
    ]));

    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_which_key_state_show_hide_toggle() {
        let mut state = WhichKeyState::new();
        assert!(!state.visible);

        state.show();
        assert!(state.visible);

        state.hide();
        assert!(!state.visible);

        state.toggle();
        assert!(state.visible);

        state.toggle();
        assert!(!state.visible);
    }
}
