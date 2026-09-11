use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::App;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let [header, conversation, composer, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(frame.area());

    let quiet = Style::default().fg(Color::DarkGray);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("CLAW", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!(
                    "CODE  /  {:?}  /  {:?}  /  {}:{}",
                    app.mode(),
                    app.conversation_status(),
                    app.selected_provider(),
                    app.selected_model()
                ),
                quiet,
            ),
        ]))
        .block(Block::default().borders(Borders::BOTTOM)),
        header,
    );
    frame.render_widget(
        Paragraph::new(app.transcript())
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: false }),
        conversation,
    );
    frame.render_widget(
        Paragraph::new(app.prompt()).block(
            Block::default()
                .borders(Borders::TOP)
                .title(Span::styled(" WRITE ", quiet)),
        ),
        composer,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{}  ·  Esc / q quit  ·  Ctrl-C cancel",
            app.diagnostic()
        ))
        .style(quiet),
        footer,
    );
}
