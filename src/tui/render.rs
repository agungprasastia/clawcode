use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

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
}

fn render_home(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, mode_color: Color) {
    if area.height < 14 || area.width < 50 {
        render_compact_home(frame, area, app, theme, mode_color);
        return;
    }

    let show_big_logo = area.height >= 20 && area.width >= 70;
    let show_cards = area.height >= 22 && area.width >= 70;

    let hero_height = if show_big_logo { 8 } else { 3 };
    let cards_height = if show_cards { 4 } else { 0 };
    let input_height = 5;
    let hints_height = 1;

    let home_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(hero_height),
            Constraint::Length(if show_cards { 1 } else { 0 }),
            Constraint::Length(cards_height),
            Constraint::Length(1),
            Constraint::Length(input_height),
            Constraint::Length(hints_height),
            Constraint::Min(0),
        ])
        .split(area);

    let hero_area = home_chunks[1];
    let cards_area = home_chunks[3];
    let input_area = home_chunks[5];
    let hints_area = home_chunks[6];

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
            Constraint::Length(4),
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
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(mode_color))
        .style(Style::default().bg(theme.bg_element))
        .title(Span::styled(
            format!(" [{}] ", mode_label(app)),
            Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let input_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    let prompt_text = if app.prompt().is_empty() {
        Line::from(vec![
            Span::styled(" › ", Style::default().fg(mode_color)),
            Span::styled(
                "Ask Clawcode to inspect, plan, or build…",
                Style::default().fg(theme.dim),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" › ", Style::default().fg(mode_color)),
            Span::styled(
                app.prompt(),
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(mode_color)),
        ])
    };
    frame.render_widget(Paragraph::new(prompt_text), input_chunks[0]);

    if input_chunks.len() >= 3 && input_chunks[2].height > 0 {
        let status_color = match app.conversation_status() {
            super::ConversationStatus::Active => theme.success,
            super::ConversationStatus::Error => theme.error,
            super::ConversationStatus::Cancelled => theme.warning,
            _ => theme.dim,
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

        let meta_spans = vec![
            Span::styled("● ", Style::default().fg(status_color)),
            Span::styled(
                mode_label(app),
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(model_text, Style::default().fg(theme.ink)),
            Span::styled("  ", Style::default()),
            Span::styled(provider_text, Style::default().fg(theme.quiet)),
            Span::styled("  ·  ", Style::default().fg(theme.panel)),
            Span::styled(
                status_label(app),
                Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
            ),
        ];
        frame.render_widget(Paragraph::new(Line::from(meta_spans)), input_chunks[2]);
    }
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
                    Style::default().fg(theme.amber).add_modifier(Modifier::BOLD),
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

    let header_line2 = Line::from(vec![
        Span::styled("● ", Style::default().fg(theme.success)),
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

    frame.render_widget(
        Paragraph::new(Text::from(app.transcript()))
            .style(Style::default().fg(theme.ink))
            .wrap(Wrap { trim: false })
            .block(conversation_block),
        chunks[1],
    );

    render_input_card(frame, chunks[2], app, theme, mode_color);
    render_command_popup(frame, chunks[2], app, theme);
    render_hints_row(frame, chunks[3], app, theme);
}

fn render_status_bar(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let repo = repo_context();
    let cwd_display = if repo.cwd.len() > 30 {
        format!("...{}", &repo.cwd[repo.cwd.len() - 27..])
    } else {
        repo.cwd
    };

    let mut left_spans = vec![Span::styled(cwd_display, Style::default().fg(theme.dim))];

    if let Some(branch) = repo.branch {
        left_spans.push(Span::styled(
            format!(" ({branch})"),
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if let Some(metrics) = app.metrics() {
        left_spans.push(Span::styled("  ·  ", Style::default().fg(theme.panel)));
        left_spans.push(Span::styled(
            format!(
                "duration {:?}  usage {:?}  finish {:?}",
                metrics.duration(),
                metrics.usage(),
                metrics.finish_reason()
            ),
            Style::default().fg(theme.dim),
        ));
    }

    let right_spans = vec![
        Span::styled("clawcode ", Style::default().fg(theme.dim)),
        Span::styled(
            "v0.1.0",
            Style::default()
                .fg(theme.quiet)
                .add_modifier(Modifier::BOLD),
        ),
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

struct RepoContext {
    cwd: String,
    branch: Option<String>,
}

fn repo_context() -> RepoContext {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".into());
    let cwd_display =
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            let home_str = home.to_string_lossy();
            if cwd.starts_with(&*home_str) {
                let relative = &cwd[home_str.len()..];
                let relative = relative.trim_start_matches(|c| c == '/' || c == '\\');
                format!("~/{relative}")
            } else {
                cwd
            }
        } else {
            cwd
        };

    let branch = std::fs::read_to_string(".git/HEAD")
        .ok()
        .and_then(|content| {
            let line = content.lines().next()?.trim();
            if let Some(branch) = line.strip_prefix("ref: refs/heads/") {
                Some(branch.to_string())
            } else if line.len() >= 7 {
                Some(line[..7].to_string())
            } else {
                None
            }
        });

    RepoContext {
        cwd: cwd_display,
        branch,
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

    let max_visible = 6;
    let visible_count = suggestions.len().min(max_visible);
    let popup_height = (visible_count as u16) + 2;

    if input_area.y < popup_height {
        return;
    }
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
                        Style::default().fg(theme.amber).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<16}", item.name),
                        Style::default().fg(theme.amber).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(item.description, Style::default().fg(theme.ink)),
                ])
            } else {
                Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        format!("{:<16}", item.name),
                        Style::default().fg(theme.ink),
                    ),
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
            Style::default().fg(theme.amber).add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Paragraph::new(items).block(block), popup_area);
}
