use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusDialogState {
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub theme: String,
    pub branch: String,
    pub cwd: String,
    pub transcript_bytes: usize,
    pub status: String,
}

impl StatusDialogState {
    pub fn new(
        mode: &str,
        provider: &str,
        model: &str,
        theme: &str,
        branch: &str,
        cwd: &str,
    ) -> Self {
        Self {
            mode: mode.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            theme: theme.to_string(),
            branch: branch.to_string(),
            cwd: cwd.to_string(),
            transcript_bytes: 0,
            status: String::new(),
        }
    }

    pub fn with_details(mut self, transcript_bytes: usize, status: &str) -> Self {
        self.transcript_bytes = transcript_bytes;
        self.status = status.to_string();
        self
    }
}

pub fn render_status_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &StatusDialogState,
    theme: &Theme,
) {
    let width = area.width.clamp(40, 72).min(area.width);
    let height = 13.min(area.height.saturating_sub(2));

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
            " System Status & Diagnostics ",
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

    if inner.width < 12 || inner.height < 6 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Subtitle
            Constraint::Length(1), // Divider
            Constraint::Min(4),    // Key-value pairs
            Constraint::Length(1), // Divider
            Constraint::Length(1), // Footer
        ])
        .split(inner);

    let header_line = Line::from(vec![
        Span::styled(
            "CLAWCODE",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " • Autonomous Agent Workbench v0.1.0",
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

    let label_w = 14;
    let rows = [
        ("Agent Mode", dialog.mode.as_str(), theme.teal),
        ("Active Model", dialog.model.as_str(), theme.amber),
        ("AI Provider", dialog.provider.as_str(), theme.teal),
        ("Color Theme", dialog.theme.as_str(), theme.warning),
        ("Git Branch", dialog.branch.as_str(), theme.success),
        ("Working Dir", dialog.cwd.as_str(), theme.ink),
    ];

    let mut lines = Vec::new();
    for (label, val, val_color) in rows {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:<width$}", label, width = label_w),
                Style::default().fg(theme.dim),
            ),
            Span::styled(" : ", Style::default().fg(theme.dim)),
            Span::styled(
                val,
                Style::default().fg(val_color).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), chunks[2]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider_char,
            Style::default().fg(theme.dim),
        ))),
        chunks[3],
    );

    let footer_line = Line::from(vec![
        Span::styled(
            " Esc / Enter ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Close status dialog", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(footer_line), chunks[4]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_dialog_state_new() {
        let state = StatusDialogState::new(
            "Plan",
            "openai",
            "gpt-4o",
            "dark",
            "main",
            "/workspace",
        );
        assert_eq!(state.mode, "Plan");
        assert_eq!(state.provider, "openai");
        assert_eq!(state.model, "gpt-4o");
        assert_eq!(state.theme, "dark");
        assert_eq!(state.branch, "main");
        assert_eq!(state.cwd, "/workspace");
        assert_eq!(state.transcript_bytes, 0);
        assert_eq!(state.status, "");
    }

    #[test]
    fn test_status_dialog_state_with_details() {
        let state = StatusDialogState::new(
            "Build",
            "anthropic",
            "claude-3-5-sonnet",
            "light",
            "feature",
            "/repo",
        )
        .with_details(1024, "ready");

        assert_eq!(state.transcript_bytes, 1024);
        assert_eq!(state.status, "ready");
        assert_eq!(state.mode, "Build");
        assert_eq!(state.provider, "anthropic");
    }
}
