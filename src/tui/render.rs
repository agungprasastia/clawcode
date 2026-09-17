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

use super::App;
use super::dialogs::{
    render_agents_dialog, render_models_dialog, render_model_suggestions_popup,
    render_sessions_dialog, render_sessions_panel, render_status_dialog, render_themes_dialog,
    render_which_key,
};
use super::theme::{Theme, darken_color};

const LOGO: [&str; 6] = [
    " ██████╗██╗      █████╗ ██╗    ██╗ ██████╗ ██████╗ ██████╗ ███████╗",
    "██╔════╝██║     ██╔══██╗██║    ██║██╔════╝██╔═══██╗██╔══██╗██╔════╝",
    "██║     ██║     ███████║██║ █╗ ██║██║     ██║   ██║██║  ██║█████╗  ",
    "██║     ██║     ██╔══██║██║███╗██║██║     ██║   ██║██║  ██║██╔══╝  ",
    "╚██████╗███████╗██║  ██║╚███╔███╔╝╚██████╗╚██████╔╝██████╔╝███████╗",
    " ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝  ╚═════╝ ╚═════╝ ╚═════╝ ╚══════╝",
];

const MASCOT_FRAMES: [[&str; 3]; 2] = [
    ["   ▃▃▛████▜▃▃", "█▟▟▜████████▛▙▙█", "   ▞ ▘    ▝ ▚"],
    ["   ▃▃▛████▜▃▃", "█▙▟▜████████▛▙▟█", "   ▞ ▘    ▝ ▚"],
];

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let theme = app.theme().to_theme();
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

    if app.which_key().visible {
        render_which_key(frame, area, &theme);
    } else if let Some(dialog) = app.status_dialog() {
        render_status_dialog(frame, area, dialog, &theme);
    } else if let Some(dialog) = app.sessions_dialog() {
        render_sessions_dialog(frame, area, dialog, app.active_session_id(), &theme);
    } else if let Some(dialog) = app.agents_dialog() {
        render_agents_dialog(frame, area, dialog, app.mode(), &theme);
    } else if let Some(dialog) = app.themes_dialog() {
        render_themes_dialog(frame, area, dialog, app.theme(), &theme);
    } else if let Some(dialog) = app.models_dialog() {
        render_models_dialog(frame, area, dialog, app.selected_model(), &theme);
    } else if !app.session_listings().is_empty() {
        render_sessions_panel(frame, area, app, &theme);
    }
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

    let show_big_logo = top_canvas.height >= 11 && area.width >= 70;
    let show_cards = top_canvas.height >= 15 && area.width >= 70;

    let hero_height = if show_big_logo { 10 } else { 3 };
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

    render_hero(
        frame,
        hero_area,
        show_big_logo,
        app.home_state().frame(),
        theme,
    );
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
        Span::raw(" "),
        Span::styled("clawcode v0.1.0", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(brand_line), chunks[0]);

    render_input_card(frame, chunks[2], app, theme, mode_color);
    render_command_popup(frame, chunks[2], app, theme);
    render_hints_row(frame, chunks[3], app, theme);
}

fn render_hero(
    frame: &mut Frame<'_>,
    area: Rect,
    show_big_logo: bool,
    mascot_frame: usize,
    theme: &Theme,
) {
    if show_big_logo {
        let hero_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(6),
                Constraint::Length(1),
            ])
            .split(area);

        let mascot = MASCOT_FRAMES[mascot_frame % 2];
        let mascot_lines: Vec<Line> = mascot
            .iter()
            .map(|l| {
                Line::from(Span::styled(
                    *l,
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD),
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(mascot_lines).alignment(Alignment::Center),
            hero_chunks[0],
        );

        let logo_lines: Vec<Line> = LOGO
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let color = if i == 5 {
                    darken_color(theme.amber, 0.7)
                } else {
                    theme.amber
                };
                Line::from(Span::styled(
                    *l,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(logo_lines).alignment(Alignment::Center),
            hero_chunks[1],
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
            Span::styled("clawcode v0.1.0", Style::default().fg(theme.dim)),
        ]);
        frame.render_widget(
            Paragraph::new(subtitle).alignment(Alignment::Center),
            hero_chunks[2],
        );
    } else {
        let mascot = MASCOT_FRAMES[mascot_frame % 2];
        let logo_lines = vec![
            Line::from(Span::styled(
                mascot[1],
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            )),
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
                Span::styled(" · clawcode v0.1.0", Style::default().fg(theme.dim)),
            ]),
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

fn visual_line_count(lines: &[Line], width: u16) -> usize {
    if width == 0 {
        return lines.len();
    }
    let mut count = 0;
    for line in lines {
        let w = line.width();
        let rows = if w == 0 {
            1
        } else {
            (w + width as usize - 1) / width as usize
        };
        count += rows;
    }
    count
}

fn format_transcript_lines(
    transcript: &str,
    theme: &Theme,
    mode_color: Color,
) -> Vec<Line<'static>> {
    let raw_lines: Vec<&str> = transcript.lines().collect();
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(raw_lines.len());

    for (i, line) in raw_lines.iter().enumerate() {
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
        } else if let Some(rest) = line.strip_prefix("⬢ ") {
            let is_failed = raw_lines.get(i + 1).map(|nl| {
                let trimmed = nl.trim_start();
                trimmed.starts_with("└ failed:") || trimmed.starts_with("failed:")
            }).unwrap_or(false);

            let marker_color = if is_failed { theme.error } else { theme.success };
            let mut spans = vec![
                Span::styled("⬢ ", Style::default().fg(marker_color).add_modifier(Modifier::BOLD)),
            ];

            let rest_trimmed = rest.trim();
            if let Some((verb, target)) = rest_trimmed.split_once(' ') {
                spans.push(Span::styled(
                    verb.to_string(),
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    target.to_string(),
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(
                    rest_trimmed.to_string(),
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                ));
            }
            lines.push(Line::from(spans));
        } else if line.trim_start().starts_with("└ ") {
            let trimmed = line.trim_start();
            let rest = &trimmed["└ ".len()..];
            let mut spans = vec![
                Span::styled("  └ ", Style::default().fg(theme.dim)),
            ];
            let rest_trimmed = rest.trim();
            if let Some(err_detail) = rest_trimmed.strip_prefix("failed:") {
                spans.push(Span::styled("failed: ", Style::default().fg(theme.error).add_modifier(Modifier::BOLD)));
                spans.push(Span::styled(err_detail.trim().to_string(), Style::default().fg(theme.error)));
            } else {
                spans.push(Span::styled(rest_trimmed.to_string(), Style::default().fg(theme.quiet)));
            }
            lines.push(Line::from(spans));
        } else if let Some(rest) = line.strip_prefix("⬡ ") {
            let spans = vec![
                Span::styled("⬡ ", Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
                Span::styled(rest.to_string(), Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)),
            ];
            lines.push(Line::from(spans));
        } else if let Some(rest) = line.strip_prefix("⚙ [") {
            let inner = rest.strip_suffix(']').unwrap_or(rest);
            let mut spans = vec![
                Span::styled("⚙ ", Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
            ];
            if let Some((name, args)) = inner.split_once(':') {
                spans.push(Span::styled(name.trim().to_string(), Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(args.trim().to_string(), Style::default().fg(theme.ink)));
            } else {
                spans.push(Span::styled(inner.trim().to_string(), Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)));
            }
            lines.push(Line::from(spans));
        } else if let Some(rest) = line.strip_prefix("✓ ") {
            lines.push(Line::from(vec![
                Span::styled("  └ ", Style::default().fg(theme.dim)),
                Span::styled(rest.to_string(), Style::default().fg(theme.quiet)),
            ]));
        } else if let Some(rest) = line.strip_prefix("✗ ") {
            lines.push(Line::from(vec![
                Span::styled("  └ failed: ", Style::default().fg(theme.error).add_modifier(Modifier::BOLD)),
                Span::styled(rest.to_string(), Style::default().fg(theme.error)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.ink),
            )));
        }
    }

    lines
}

fn render_hints_row(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let mode_color = match app.mode() {
        super::ConversationMode::Plan => theme.amber,
        super::ConversationMode::Build => theme.teal,
    };
    let is_working = app.metrics().is_none()
        && (matches!(app.conversation_status(), super::ConversationStatus::Active)
            || app.is_streaming_active()
            || app.is_typing()
            || app.active_tool().is_some());

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
    } else if is_working {
        if let Some(tool) = app.active_tool() {
            let elapsed = tool.started_at.elapsed().as_secs_f64();
            let action_str = if tool.desc.is_empty() {
                tool.name.clone()
            } else {
                format!("{}: {}", tool.name, tool.desc)
            };
            vec![
                Span::styled("⬡ ", Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
                Span::styled(action_str, Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" · {:.1}s", elapsed), Style::default().fg(theme.dim)),
            ]
        } else {
            let elapsed_str = if let Some(elapsed) = app.streaming_elapsed_seconds() {
                format!(" · {:.1}s", elapsed)
            } else {
                String::new()
            };
            vec![
                Span::styled("● ", Style::default().fg(mode_color)),
                Span::styled("streaming", Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
                Span::styled(elapsed_str, Style::default().fg(theme.dim)),
            ]
        }
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

    let (right_spans, right_len) = if is_working {
        (
            vec![
                Span::styled("Esc", Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)),
                Span::styled(" cancel", Style::default().fg(theme.dim)),
            ],
            12,
        )
    } else if !app.matching_suggestions().is_empty() {
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

    let is_working = app.metrics().is_none()
        && (matches!(app.conversation_status(), super::ConversationStatus::Active)
            || app.is_streaming_active()
            || app.is_typing()
            || app.active_tool().is_some());
    let status_color = if is_working {
        mode_color
    } else {
        match app.conversation_status() {
            super::ConversationStatus::Active => theme.success,
            super::ConversationStatus::Error => theme.error,
            super::ConversationStatus::Cancelled => theme.warning,
            _ => theme.dim,
        }
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

    let mut lines = format_transcript_lines(app.transcript(), theme, mode_color);

    if app.is_typing() {
        let is_last_prompt = lines
            .last()
            .and_then(|l| l.spans.first())
            .map(|s| s.content.as_ref() == "┃ ")
            .unwrap_or(false);

        if is_last_prompt || app.transcript().ends_with('\n') || lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "▋",
                Style::default().fg(mode_color),
            )));
        } else if let Some(last_line) = lines.last_mut() {
            last_line.spans.push(Span::styled(
                "▋",
                Style::default().fg(mode_color),
            ));
        }
    }

    if let Some(active_tool) = app.active_tool() {
        let active_verb = match active_tool.name.as_str() {
            "read_file" => "Reading",
            "write_file" => "Writing",
            "edit_file" => "Editing",
            "list_dir" => "Listing",
            "glob_search" => "Running glob_search",
            "grep_search" => "Running grep_search",
            "bash" => "Running",
            _ => "Running",
        };
        let action_text = if active_tool.desc.is_empty() {
            format!("Running {}...", active_tool.name)
        } else if active_tool.name == "grep_search" || active_tool.name == "glob_search" {
            format!("Running {} {}...", active_tool.name, active_tool.desc)
        } else {
            format!("{} {}...", active_verb, active_tool.desc)
        };
        let mut active_spans = vec![
            Span::styled("⬡ ", Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
            Span::styled(action_text, Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)),
        ];
        let elapsed = active_tool.started_at.elapsed().as_secs_f64();
        if elapsed >= 0.5 {
            active_spans.push(Span::styled(
                format!("  {:.1}s", elapsed),
                Style::default().fg(theme.dim),
            ));
        }
        if !lines.is_empty() && !lines.last().map(|l| l.spans.is_empty()).unwrap_or(false) {
            lines.push(Line::from(String::new()));
        }
        lines.push(Line::from(active_spans));
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

    }

    let (transcript_area, status_bar_area) = if is_working && chunks[1].height >= 3 {
        let sub = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(chunks[1]);
        (sub[0], Some(sub[1]))
    } else {
        (chunks[1], None)
    };

    if status_bar_area.is_none() && is_working {
        let spinner_width = if transcript_area.width < 30 {
            1
        } else {
            crate::tui::WaveSpinner::WIDTH
        };
        let mut status_spans = app.wave_spinner().spans_for_width(spinner_width);
        if let Some(tps) = app.tokens_per_second() {
            status_spans.push(Span::raw(" "));
            status_spans.push(Span::styled(
                format!("{:.0}t/s", tps),
                Style::default().fg(theme.dim),
            ));
        }
        if let Some(elapsed) = app.streaming_elapsed_seconds() {
            status_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
            status_spans.push(Span::styled(
                format!("{:.1}s", elapsed),
                Style::default().fg(theme.dim),
            ));
        }
        status_spans.push(Span::raw("  "));
        status_spans.push(Span::styled(
            "esc to cancel",
            Style::default().fg(theme.dim),
        ));

        lines.push(Line::from(String::new()));
        lines.push(Line::from(status_spans));
    }

    if let Some(status_area) = status_bar_area {
        let spinner_width = if status_area.width < 30 {
            1
        } else {
            crate::tui::WaveSpinner::WIDTH
        };
        let mut status_spans = app.wave_spinner().spans_for_width(spinner_width);

        if let Some(tool) = app.active_tool() {
            status_spans.push(Span::raw(" "));
            status_spans.push(Span::styled(
                "⬡ ",
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ));
            let tool_desc = if tool.desc.is_empty() {
                tool.name.clone()
            } else {
                format!("{}: {}", tool.name, tool.desc)
            };
            let max_desc_len = (status_area.width as usize).saturating_sub(36).max(8);
            let display_desc = if tool_desc.len() > max_desc_len {
                format!("{}…", &tool_desc[..max_desc_len.saturating_sub(1)])
            } else {
                tool_desc
            };
            status_spans.push(Span::styled(
                display_desc,
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            ));
        } else if let Some(tps) = app.tokens_per_second() {
            status_spans.push(Span::raw(" "));
            status_spans.push(Span::styled(
                format!("{:.0}t/s", tps),
                Style::default().fg(theme.dim),
            ));
        }

        if let Some(elapsed) = app.streaming_elapsed_seconds() {
            status_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
            status_spans.push(Span::styled(
                format!("{:.1}s", elapsed),
                Style::default().fg(theme.dim),
            ));
        } else if let Some(tool) = app.active_tool() {
            let elapsed = tool.started_at.elapsed().as_secs_f64();
            status_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
            status_spans.push(Span::styled(
                format!("{:.1}s", elapsed),
                Style::default().fg(theme.dim),
            ));
        }

        status_spans.push(Span::raw("  "));
        status_spans.push(Span::styled(
            "esc to cancel",
            Style::default().fg(theme.dim),
        ));

        let status_widget = Paragraph::new(Line::from(status_spans))
            .style(Style::default().bg(theme.bg_element));
        frame.render_widget(status_widget, status_area);
    }

    let content_width = transcript_area.width;
    let total_visual_lines = visual_line_count(&lines, content_width) as u16;
    let visible_height = transcript_area.height;
    let max_scroll = total_visual_lines.saturating_sub(visible_height);
    let scroll_offset = app.chat_scroll().min(max_scroll);
    let scroll_y = max_scroll.saturating_sub(scroll_offset);

    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(theme.ink))
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0))
            .block(conversation_block),
        transcript_area,
    );

    if total_visual_lines > visible_height && visible_height > 0 {
        let mut scrollbar_state =
            ScrollbarState::new(total_visual_lines as usize).position(scroll_y as usize);
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃");
        frame.render_stateful_widget(scrollbar, transcript_area, &mut scrollbar_state);
    }

    if scroll_offset > 0 && transcript_area.width >= 30 && transcript_area.height >= 2 {
        let badge_text = format!(" ↓ {scroll_offset} lines up (End to bottom) ");
        let badge_width = badge_text.len() as u16;
        if transcript_area.width > badge_width + 2 {
            let badge_area = Rect {
                x: transcript_area.x + transcript_area.width - badge_width - 2,
                y: transcript_area.y + transcript_area.height - 1,
                width: badge_width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    badge_text,
                    Style::default()
                        .fg(theme.bg_element)
                        .bg(theme.amber)
                        .add_modifier(Modifier::BOLD),
                ))),
                badge_area,
            );
        }
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
    if let Some(tool) = app.active_tool() {
        format!("TOOL: {}", tool.name.to_ascii_uppercase())
    } else if app.metrics().is_none() && (app.is_typing() || app.is_streaming_active()) {
        "STREAMING".to_string()
    } else {
        format!("{:?}", app.conversation_status()).to_ascii_uppercase()
    }
}

fn identity_label(app: &App) -> String {
    match (app.selected_provider(), app.selected_model()) {
        ("", "") => "provider not connected".into(),
        (provider, "") => provider.to_owned(),
        (provider, model) => format!("{provider} / {model}"),
    }
}

fn render_command_popup(frame: &mut Frame<'_>, input_area: Rect, app: &App, theme: &Theme) {
    if app.prompt().starts_with("/model ") {
        render_model_suggestions_popup(frame, input_area, app, theme);
        return;
    }

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
