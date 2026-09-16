use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};

use crate::persistence::SessionStatus;

use super::App;

const LOGO: [&str; 6] = [
    " ██████╗██╗      █████╗ ██╗    ██╗ ██████╗ ██████╗ ██████╗ ███████╗",
    "██╔════╝██║     ██╔══██╗██║    ██║██╔════╝██╔═══██╗██╔══██╗██╔════╝",
    "██║     ██║     ███████║██║ █╗ ██║██║     ██║   ██║██║  ██║█████╗  ",
    "██║     ██║     ██╔══██║██║███╗██║██║     ██║   ██║██║  ██║██╔══╝  ",
    "╚██████╗███████╗██║  ██║╚███╔███╔╝╚██████╗╚██████╔╝██████╔╝███████╗",
    " ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝  ╚═════╝ ╚═════╝ ╚═════╝ ╚══════╝",
];

struct Theme {
    bg_element: Color,
    ink: Color,
    quiet: Color,
    dim: Color,
    amber: Color,
    teal: Color,
    panel: Color,
    success: Color,
    error: Color,
    warning: Color,
}

impl Theme {
    const fn new() -> Self {
        Self {
            bg_element: Color::Rgb(24, 28, 30),
            ink: Color::Rgb(240, 238, 233),
            quiet: Color::Rgb(140, 148, 144),
            dim: Color::Rgb(85, 95, 92),
            amber: Color::Rgb(232, 181, 84),
            teal: Color::Rgb(102, 190, 174),
            panel: Color::Rgb(48, 56, 60),
            success: Color::Rgb(74, 222, 128),
            error: Color::Rgb(248, 113, 113),
            warning: Color::Rgb(251, 191, 36),
        }
    }
}

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let theme = Theme::new();
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mode_color = match app.mode() {
        super::ConversationMode::Plan => theme.amber,
        super::ConversationMode::Build => theme.teal,
    };

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let workspace_area = main_chunks[0];
    let status_bar_area = main_chunks[1];

    if app.transcript().is_empty() {
        render_home(frame, workspace_area, app, &theme, mode_color);
    } else {
        render_chat(frame, workspace_area, app, &theme, mode_color);
    }

    render_status_bar(frame, status_bar_area, app, &theme);

    if !app.session_listings().is_empty() {
        render_sessions_panel(frame, area, app, &theme);
    }
}

/// Overlay panel listing all sessions grouped by workspace. Pinned sessions
/// first within each group; running sessions show a spinner glyph.
fn render_sessions_panel(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
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
fn render_home(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, mode_color: Color) {
    if area.height < 14 || area.width < 50 {
        render_compact_home(frame, area, app, theme, mode_color);
        return;
    }

    let input_height = 5;
    let hints_height = 1;

    // Match Crabcode dock layout: top canvas stretches, input card & hints dock at bottom
    let home_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(input_height),
            Constraint::Length(hints_height),
            Constraint::Length(1),
        ])
        .split(area);

    let top_canvas = home_chunks[0];
    let input_area = home_chunks[1];
    let hints_area = home_chunks[2];

    let show_big_logo = top_canvas.height >= 13 && area.width >= 70;
    let show_cards = top_canvas.height >= 16 && area.width >= 70;

    let hero_height = if show_big_logo { 8 } else { 3 };
    let cards_height = if show_cards { 4 } else { 0 };
    let gap_height = if show_cards { 1 } else { 0 };
    let content_height = hero_height + gap_height + cards_height;

    // Center logo and quick action cards vertically within the top canvas
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(content_height),
            Constraint::Min(0),
        ])
        .split(top_canvas);

    let hero_cards_canvas = v_chunks[1];
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(hero_height),
            Constraint::Length(gap_height),
            Constraint::Length(cards_height),
        ])
        .split(hero_cards_canvas);

    let hero_area = inner_chunks[0];
    let cards_area = inner_chunks[2];

    let content_width = if area.width >= 106 {
        100
    } else {
        area.width.saturating_sub(4)
    };
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(content_width),
            Constraint::Min(0),
        ])
        .split(area);
    let centered_x = h_chunks[1].x;
    let centered_w = h_chunks[1].width;

    render_hero(frame, hero_area, show_big_logo, theme);
    if show_cards {
        let centered_cards = Rect {
            x: centered_x,
            y: cards_area.y,
            width: centered_w,
            height: cards_area.height,
        };
        render_quick_actions(frame, centered_cards, theme);
    }

    let centered_input = Rect {
        x: centered_x,
        y: input_area.y,
        width: centered_w,
        height: input_area.height,
    };
    render_input_card(frame, centered_input, app, theme, mode_color);
    render_command_popup(frame, centered_input, app, theme);

    let centered_hints = Rect {
        x: centered_x,
        y: hints_area.y,
        width: centered_w,
        height: hints_area.height,
    };
    render_hints_row(frame, centered_hints, app, theme);
}

fn render_compact_home(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: &Theme,
    mode_color: Color,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(area);

    let brand_line = Line::from(vec![
        Span::styled(
            " CLAWCODE ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("[{}]", mode_label(app)),
            Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(status_label(app), Style::default().fg(theme.quiet)),
    ]);
    frame.render_widget(Paragraph::new(brand_line), chunks[0]);

    render_input_card(frame, chunks[2], app, theme, mode_color);
    render_command_popup(frame, chunks[2], app, theme);
    render_hints_row(frame, chunks[3], app, theme);
}

fn render_hero(frame: &mut Frame<'_>, area: Rect, show_big_logo: bool, theme: &Theme) {
    if show_big_logo {
        let hero_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);

        let logo_lines: Vec<Line> = LOGO
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let color = if i == 5 { theme.quiet } else { theme.amber };
                Line::from(Span::styled(
                    *l,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(logo_lines).alignment(Alignment::Center),
            hero_chunks[0],
        );

        let subtitle = Line::from(vec![
            Span::styled(
                "CLAWCODE",
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  ", Style::default().fg(theme.panel)),
            Span::styled(
                "AUTONOMOUS AGENT WORKBENCH",
                Style::default().fg(theme.quiet),
            ),
            Span::styled("  ·  ", Style::default().fg(theme.panel)),
            Span::styled("v0.1.0", Style::default().fg(theme.dim)),
        ]);
        frame.render_widget(
            Paragraph::new(subtitle).alignment(Alignment::Center),
            hero_chunks[2],
        );
    } else {
        let logo_lines = vec![
            Line::from(vec![
                Span::styled(
                    " CLAWCODE ",
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "· AUTONOMOUS AGENT WORKBENCH",
                    Style::default().fg(theme.quiet),
                ),
            ]),
            Line::from(Span::styled(
                "Local architecture & verified execution",
                Style::default().fg(theme.dim),
            )),
        ];
        frame.render_widget(
            Paragraph::new(logo_lines).alignment(Alignment::Center),
            area,
        );
    }
}

fn render_quick_actions(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .spacing(1)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(area);

    let actions = [
        ("/plan", "Plan & Analyze", "Read-only plan", theme.amber),
        ("/build", "Execute & Edit", "Verified edits", theme.teal),
        ("/models", "Models & LLMs", "Providers & IDs", theme.ink),
        ("/help", "Manual & Keys", "Commands & tips", theme.quiet),
    ];

    for (i, &(cmd, title, desc, col)) in actions.iter().enumerate() {
        if i >= cards.len() {
            break;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.panel))
            .style(Style::default().bg(theme.bg_element))
            .title(Span::styled(
                format!(" {cmd} "),
                Style::default().fg(col).add_modifier(Modifier::BOLD),
            ));

        let content = vec![
            Line::from(Span::styled(
                format!(" {title}"),
                Style::default().fg(theme.ink),
            )),
            Line::from(Span::styled(
                format!(" {desc}"),
                Style::default().fg(theme.dim),
            )),
        ];

        frame.render_widget(Paragraph::new(content).block(block), cards[i]);
    }
}

fn render_input_card(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: &Theme,
    mode_color: Color,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let border_set = border::Set {
        vertical_left: "┃",
        ..border::PLAIN
    };

    let border = Block::default()
        .borders(Borders::LEFT)
        .border_set(border_set)
        .border_style(Style::default().fg(mode_color));

    let inner_area = border.inner(area);
    if inner_area.height == 0 || inner_area.width == 0 {
        frame.render_widget(border, area);
        return;
    }

    // Fill background of input card excluding the cap row at bottom (if height > 1)
    let bg_height = if inner_area.height > 1 {
        inner_area.height.saturating_sub(1)
    } else {
        inner_area.height
    };
    let bg_area = Rect {
        x: inner_area.x,
        y: inner_area.y,
        width: inner_area.width,
        height: bg_height,
    };
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.bg_element)),
        bg_area,
    );

    // Render left border `┃`
    frame.render_widget(border, area);

    // Two spaces padding from `┃` on the left
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner_area);

    let content_area = h_chunks[1];

    let prompt_text = if app.prompt().is_empty() {
        Line::from(Span::styled(
            app.placeholder(),
            Style::default().fg(theme.dim),
        ))
    } else {
        Line::from(Span::styled(
            app.prompt(),
            Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
        ))
    };

    let provider_text = if app.selected_provider().is_empty() {
        "disconnected"
    } else {
        app.selected_provider()
    };
    let model_text = if app.selected_model().is_empty() {
        "default"
    } else {
        app.selected_model()
    };

    let mut meta_spans = vec![
        Span::styled(
            format!("[{}]", mode_label(app)),
            Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(model_text, Style::default().fg(theme.ink)),
        Span::raw("  "),
        Span::styled(provider_text, Style::default().fg(theme.quiet)),
    ];

    if !matches!(app.conversation_status(), super::ConversationStatus::Idle) {
        let status_color = match app.conversation_status() {
            super::ConversationStatus::Active => theme.success,
            super::ConversationStatus::Error => theme.error,
            super::ConversationStatus::Cancelled => theme.warning,
            _ => theme.dim,
        };
        meta_spans.push(Span::raw("  ·  "));
        meta_spans.push(Span::styled("● ", Style::default().fg(status_color)));
        meta_spans.push(Span::styled(
            status_label(app),
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        ));
    }

    if area.height <= 2 {
        let cursor_x =
            content_area.x + (app.prompt().len() as u16).min(content_area.width.saturating_sub(1));
        frame.set_cursor_position((cursor_x, inner_area.y));

        if inner_area.height == 1 {
            frame.render_widget(Paragraph::new(prompt_text), content_area);
        } else {
            let mini_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1)])
                .split(inner_area);
            frame.render_widget(
                Paragraph::new(prompt_text),
                Rect {
                    x: content_area.x,
                    y: mini_chunks[0].y,
                    width: content_area.width,
                    height: 1,
                },
            );
            frame.render_widget(
                Paragraph::new(Line::from(meta_spans)),
                Rect {
                    x: content_area.x,
                    y: mini_chunks[1].y,
                    width: content_area.width,
                    height: 1,
                },
            );
        }
        return;
    }

    if area.height <= 4 {
        let mini_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(inner_area);

        let cursor_x =
            content_area.x + (app.prompt().len() as u16).min(content_area.width.saturating_sub(1));
        frame.set_cursor_position((cursor_x, mini_chunks[0].y));

        frame.render_widget(
            Paragraph::new(prompt_text),
            Rect {
                x: content_area.x,
                y: mini_chunks[0].y,
                width: content_area.width,
                height: 1,
            },
        );
        frame.render_widget(
            Paragraph::new(Line::from(meta_spans)),
            Rect {
                x: content_area.x,
                y: mini_chunks[1].y,
                width: content_area.width,
                height: 1,
            },
        );

        let cap_fill_width = area.width.saturating_sub(1) as usize;
        let cap_row = Paragraph::new(Line::from(vec![
            Span::styled("╹", Style::default().fg(mode_color)),
            Span::styled(
                "▀".repeat(cap_fill_width),
                Style::default().fg(theme.bg_element),
            ),
        ]));
        frame.render_widget(cap_row, Rect::new(area.x, mini_chunks[2].y, area.width, 1));
        return;
    }

    // Standard 5-row input card layout matching Crabcode
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Row 0: Top padding
            Constraint::Length(1), // Row 1: Prompt / placeholder
            Constraint::Length(1), // Row 2: Separator padding
            Constraint::Length(1), // Row 3: Metadata chips ([PLAN] model provider)
            Constraint::Length(1), // Row 4: Cap row (╹▀▀▀...)
        ])
        .split(inner_area);

    // Hardware terminal cursor positioning
    let cursor_x =
        content_area.x + (app.prompt().len() as u16).min(content_area.width.saturating_sub(1));
    let cursor_y = v_chunks[1].y;
    frame.set_cursor_position((cursor_x, cursor_y));

    // Row 1: Prompt or placeholder
    frame.render_widget(
        Paragraph::new(prompt_text),
        Rect {
            x: content_area.x,
            y: v_chunks[1].y,
            width: content_area.width,
            height: 1,
        },
    );

    // Row 3: Metadata chips
    frame.render_widget(
        Paragraph::new(Line::from(meta_spans)),
        Rect {
            x: content_area.x,
            y: v_chunks[3].y,
            width: content_area.width,
            height: 1,
        },
    );

    // Row 4: Bottom cap row
    let cap_fill_width = area.width.saturating_sub(1) as usize;
    let cap_row = Paragraph::new(Line::from(vec![
        Span::styled("╹", Style::default().fg(mode_color)),
        Span::styled(
            "▀".repeat(cap_fill_width),
            Style::default().fg(theme.bg_element),
        ),
    ]));
    frame.render_widget(cap_row, Rect::new(area.x, v_chunks[4].y, area.width, 1));
}

fn render_hints_row(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let left_spans = if matches!(app.conversation_status(), super::ConversationStatus::Error) {
        vec![
            Span::styled("✖ ", Style::default().fg(theme.error)),
            Span::styled(
                if app.diagnostic().is_empty() {
                    "error"
                } else {
                    app.diagnostic()
                },
                Style::default().fg(theme.error),
            ),
        ]
    } else if !app.diagnostic().is_empty() {
        vec![
            Span::styled("! ", Style::default().fg(theme.warning)),
            Span::styled(app.diagnostic(), Style::default().fg(theme.warning)),
        ]
    } else {
        vec![
            Span::styled("● ", Style::default().fg(theme.success)),
            Span::styled("ready", Style::default().fg(theme.quiet)),
        ]
    };

    let (right_spans, right_len) = if !app.matching_suggestions().is_empty() {
        (
            vec![
                Span::styled(
                    "Tab",
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" complete   ", Style::default().fg(theme.dim)),
                Span::styled("↑/↓", Style::default().fg(theme.ink)),
                Span::styled(" select   ", Style::default().fg(theme.dim)),
                Span::styled("Enter", Style::default().fg(theme.ink)),
                Span::styled(" run   ", Style::default().fg(theme.dim)),
                Span::styled("Ctrl+C", Style::default().fg(theme.ink)),
                Span::styled(" cancel", Style::default().fg(theme.dim)),
            ],
            54,
        )
    } else {
        (
            vec![
                Span::styled("Enter", Style::default().fg(theme.ink)),
                Span::styled(" send   ", Style::default().fg(theme.dim)),
                Span::styled("Tab", Style::default().fg(theme.ink)),
                Span::styled(" mode   ", Style::default().fg(theme.dim)),
                Span::styled("/help", Style::default().fg(theme.ink)),
                Span::styled(" commands   ", Style::default().fg(theme.dim)),
                Span::styled("Ctrl+C", Style::default().fg(theme.ink)),
                Span::styled(" cancel", Style::default().fg(theme.dim)),
            ],
            54,
        )
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(2),
            Constraint::Length(right_len),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(Line::from(left_spans)), chunks[0]);
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        chunks[2],
    );
}

fn render_chat(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, mode_color: Color) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(area);

    let header_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(theme.panel));

    let header_line1 = Line::from(vec![
        Span::styled(
            " CLAWCODE ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("· ", Style::default().fg(theme.dim)),
        Span::styled(
            format!("[{}]", mode_label(app)),
            Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(identity_label(app), Style::default().fg(theme.ink)),
    ]);

    let status_color = match app.conversation_status() {
        super::ConversationStatus::Active => theme.success,
        super::ConversationStatus::Error => theme.error,
        super::ConversationStatus::Cancelled => theme.warning,
        _ => theme.dim,
    };

    let header_line2 = Line::from(vec![
        Span::styled("● ", Style::default().fg(status_color)),
        Span::styled(status_label(app), Style::default().fg(theme.quiet)),
        Span::styled(
            if app.diagnostic().is_empty() {
                String::new()
            } else {
                format!("  ·  {}", app.diagnostic())
            },
            Style::default().fg(theme.warning),
        ),
    ]);

    frame.render_widget(
        Paragraph::new(vec![header_line1, header_line2]).block(header_block),
        chunks[0],
    );

    let conversation_block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(theme.bg_element));

    let mut lines: Vec<Line> = Vec::new();
    for line in app.transcript().lines() {
        if let Some(prompt) = line.strip_prefix("> ") {
            lines.push(Line::from(vec![
                Span::styled(
                    "┃ ",
                    Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    prompt.to_string(),
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.ink),
            )));
        }
    }

    if let Some(metrics) = app.metrics() {
        let mut meta_spans = vec![
            Span::styled(
                "▣ ",
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                mode_label(app),
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ),
        ];

        let model = if !metrics.model().is_empty() {
            metrics.model()
        } else if !app.selected_model().is_empty() {
            app.selected_model()
        } else {
            "default"
        };
        meta_spans.push(Span::styled(" • ", Style::default().fg(theme.dim)));
        meta_spans.push(Span::styled(
            model.to_string(),
            Style::default().fg(theme.dim),
        ));

        if let Some(usage) = metrics.usage() {
            let dur_secs = metrics.duration.as_secs_f64();
            let tps = if dur_secs > 0.05 {
                format!("{:.0}t/s", (usage.output_tokens as f64) / dur_secs)
            } else {
                format!("{}t", usage.output_tokens)
            };
            meta_spans.push(Span::styled(" • ", Style::default().fg(theme.dim)));
            meta_spans.push(Span::styled(tps, Style::default().fg(theme.dim)));
        }

        let dur_str = if metrics.duration.as_secs() > 0 {
            format!("{:.1}s", metrics.duration.as_secs_f64())
        } else {
            format!("{}ms", metrics.duration.as_millis())
        };
        meta_spans.push(Span::styled(" • ", Style::default().fg(theme.dim)));
        meta_spans.push(Span::styled(dur_str, Style::default().fg(theme.dim)));

        lines.push(Line::from(String::new()));
        lines.push(Line::from(meta_spans));
    } else if matches!(app.conversation_status(), super::ConversationStatus::Active) {
        lines.push(Line::from(String::new()));
        lines.push(Line::from(vec![
            Span::styled(
                "▣ ",
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Streaming...", Style::default().fg(theme.dim)),
        ]));
    }

    let total_lines = lines.len() as u16;
    let visible_height = chunks[1].height;
    let scroll_y = total_lines.saturating_sub(visible_height);

    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(theme.ink))
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0))
            .block(conversation_block),
        chunks[1],
    );

    if total_lines > visible_height && visible_height > 0 {
        let mut scrollbar_state =
            ScrollbarState::new(total_lines as usize).position(scroll_y as usize);
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃");
        frame.render_stateful_widget(scrollbar, chunks[1], &mut scrollbar_state);
    }

    render_input_card(frame, chunks[2], app, theme, mode_color);
    render_command_popup(frame, chunks[2], app, theme);
    render_hints_row(frame, chunks[3], app, theme);
}

fn render_status_bar(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let cwd_display = repo_cwd_display();

    let mut left_spans = vec![Span::styled(cwd_display, Style::default().fg(theme.dim))];

    let branch = app
        .git_branch()
        .map(|b| b.to_string())
        .or_else(crate::platform::get_current_branch);
    if let Some(branch) = branch {
        left_spans.push(Span::styled(
            format!(":{branch}"),
            Style::default().fg(theme.dim),
        ));
    }

    let right_spans = vec![
        Span::styled("clawcode ", Style::default().fg(theme.dim)),
        Span::styled("v0.1.0", Style::default().fg(theme.dim)),
    ];

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(16)])
        .split(area);

    frame.render_widget(Paragraph::new(Line::from(left_spans)), chunks[0]);
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        chunks[1],
    );
}

fn repo_cwd_display() -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".into());
    let cwd_with_tilde =
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            let home_str = home.to_string_lossy();
            if cwd.starts_with(&*home_str) {
                let relative = &cwd[home_str.len()..];
                let relative = relative.trim_start_matches(['/', '\\']);
                format!("~/{relative}")
            } else {
                cwd
            }
        } else {
            cwd
        };

    if cwd_with_tilde.len() > 30 {
        format!("...{}", &cwd_with_tilde[cwd_with_tilde.len() - 27..])
    } else {
        cwd_with_tilde
    }
}

fn mode_label(app: &App) -> &'static str {
    match app.mode() {
        super::ConversationMode::Plan => "PLAN",
        super::ConversationMode::Build => "BUILD",
    }
}

fn status_label(app: &App) -> String {
    format!("{:?}", app.conversation_status()).to_ascii_uppercase()
}

fn identity_label(app: &App) -> String {
    match (app.selected_provider(), app.selected_model()) {
        ("", "") => "provider not connected".into(),
        (provider, "") => provider.to_owned(),
        (provider, model) => format!("{provider} / {model}"),
    }
}

fn render_command_popup(frame: &mut Frame<'_>, input_area: Rect, app: &App, theme: &Theme) {
    let suggestions = app.matching_suggestions();
    if suggestions.is_empty() {
        return;
    }

    let available_space = input_area.y as usize;
    if available_space < 3 {
        return;
    }
    let max_visible = 6.min(available_space - 2);
    let visible_count = suggestions.len().min(max_visible);
    if visible_count == 0 {
        return;
    }
    let popup_height = (visible_count as u16) + 2;

    let popup_y = input_area.y.saturating_sub(popup_height);
    let popup_width = input_area.width.min(64);
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

    let items: Vec<Line> = suggestions
        .iter()
        .skip(scroll_offset)
        .take(visible_count)
        .enumerate()
        .map(|(rel_idx, item)| {
            let actual_idx = scroll_offset + rel_idx;
            let is_selected = actual_idx == selected_idx;
            if is_selected {
                Line::from(vec![
                    Span::styled(
                        " › ",
                        Style::default()
                            .fg(theme.amber)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<16}", item.name),
                        Style::default()
                            .fg(theme.amber)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(item.description, Style::default().fg(theme.ink)),
                ])
            } else {
                Line::from(vec![
                    Span::raw("   "),
                    Span::styled(format!("{:<16}", item.name), Style::default().fg(theme.ink)),
                    Span::styled(item.description, Style::default().fg(theme.dim)),
                ])
            }
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.amber))
        .style(Style::default().bg(theme.bg_element))
        .title(Span::styled(
            " Commands (↑/↓ navigate, Tab complete) ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Paragraph::new(items).block(block), popup_area);
}
