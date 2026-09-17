use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use super::chat::render_chat;

use super::App;
use super::dialogs::{
    render_agents_dialog, render_models_dialog, render_model_suggestions_popup,
    render_permission_dialog, render_question_dialog, render_sessions_dialog, render_sessions_panel,
    render_status_dialog, render_themes_dialog, render_which_key,
};
use super::home::render_home;
use super::theme::Theme;


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

    if let Some(dialog) = app.permission_dialog() {
        render_permission_dialog(frame, area, dialog, &theme);
    } else if let Some(dialog) = app.question_dialog() {
        render_question_dialog(frame, area, dialog, &theme);
    } else if app.which_key().visible {
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


pub(crate) fn render_input_card(
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

pub(crate) fn render_hints_row(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let mode_color = match app.mode() {
        super::ConversationMode::Plan => theme.amber,
        super::ConversationMode::Build => theme.teal,
    };
    let is_working = app.metrics().is_none()
        && (matches!(app.conversation_status(), super::ConversationStatus::Active)
            || app.is_streaming_active()
            || app.is_typing()
            || app.is_reasoning()
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
            let action_str = if tool.desc == "preparing arguments..." {
                format!("Preparing {}...", tool.name)
            } else if tool.desc.is_empty() {
                tool.name.clone()
            } else {
                format!("{}: {}", tool.name, tool.desc)
            };
            vec![
                Span::styled("⬡ ", Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
                Span::styled(action_str, Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" · {:.1}s", elapsed), Style::default().fg(theme.dim)),
            ]
        } else if app.is_reasoning() {
            let elapsed = app.reasoning_elapsed_seconds().unwrap_or(0.0);
            vec![
                Span::styled("💭 ", Style::default().fg(theme.amber)),
                Span::styled("Thinking", Style::default().fg(theme.amber).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" ({:.1}s)", elapsed), Style::default().fg(theme.dim)),
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

pub(crate) fn mode_label(app: &App) -> &'static str {
    match app.mode() {
        super::ConversationMode::Plan => "PLAN",
        super::ConversationMode::Build => "BUILD",
    }
}

pub(crate) fn status_label(app: &App) -> String {
    if let Some(tool) = app.active_tool() {
        if tool.desc == "preparing arguments..." {
            format!("PREPARING: {}", tool.name.to_ascii_uppercase())
        } else {
            format!("TOOL: {}", tool.name.to_ascii_uppercase())
        }
    } else if app.is_reasoning() {
        "THINKING".to_string()
    } else if app.metrics().is_none() && (app.is_typing() || app.is_streaming_active()) {
        "STREAMING".to_string()
    } else {
        format!("{:?}", app.conversation_status()).to_ascii_uppercase()
    }
}

pub(crate) fn identity_label(app: &App) -> String {
    match (app.selected_provider(), app.selected_model()) {
        ("", "") => "provider not connected".into(),
        (provider, "") => provider.to_owned(),
        (provider, model) => format!("{provider} / {model}"),
    }
}

pub(crate) fn render_command_popup(frame: &mut Frame<'_>, input_area: Rect, app: &App, theme: &Theme) {
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
