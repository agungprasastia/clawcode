use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::persistence::SessionStatus;
use crate::tui::{App, Theme};

/// Overlay panel listing all sessions grouped by workspace. Pinned sessions
/// first within each group; running sessions show a spinner glyph.
pub fn render_sessions_panel(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    const SPINNER: [&str; 7] = ["·", "✻", "✽", "✶", "✳", "✢", "*"];

    let sessions = app.session_listings();
    let group_count = sessions
        .iter()
        .map(|session| session.workspace_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    // Borders + one header line per workspace + one line per session.
    let height = (2 + group_count + sessions.len()).min(area.height as usize);
    let width = area.width.min(70);

    let rows: Vec<Line> = sessions
        .iter()
        .scan(None::<i64>, |last_workspace, session| {
            let new_group = *last_workspace != Some(session.workspace_id);
            *last_workspace = Some(session.workspace_id);
            Some((new_group, session))
        })
        .flat_map(|(new_group, session)| {
            let glyph = if session.status == SessionStatus::Running {
                SPINNER[(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() / 120)
                    .unwrap_or(0) as usize)
                    % SPINNER.len()]
            } else {
                " "
            };
            let mut lines = Vec::new();
            if new_group {
                lines.push(Line::from(Span::styled(
                    format!("  ws#{}", session.workspace_id),
                    Style::new().fg(theme.quiet),
                )));
            }
            let title = if session.pinned { "📌 " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(title, Style::new().fg(theme.amber)),
                Span::raw(&session.title),
                if session.status == SessionStatus::Running {
                    Span::styled(format!("  {glyph}"), Style::new().fg(theme.teal))
                } else {
                    Span::raw("")
                },
            ]));
            lines
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.panel))
        .title(Span::styled(
            " Sessions ",
            Style::new().fg(theme.ink).bold(),
        ));

    let paragraph = Paragraph::new(rows)
        .block(block)
        .style(Style::new().bg(theme.bg_element));
    let panel_area = Rect::new(
        area.width.saturating_sub(width) / 2,
        1,
        width,
        height.min(area.height.saturating_sub(2) as usize) as u16,
    );
    frame.render_widget(Clear, panel_area);
    frame.render_widget(paragraph, panel_area);
}
