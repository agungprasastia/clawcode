use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
};

use super::App;
use super::render::{
    identity_label, mode_label, render_command_popup, render_hints_row, render_input_card,
    status_label,
};
use super::theme::Theme;
use super::wave_spinner::WaveSpinner;

pub fn render_chat(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: &Theme,
    mode_color: Color,
) {
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
            "update_plan" => "Updating Plan",
            "webfetch" => "Fetching",
            "websearch" => "Searching",
            "skill" => "Loading skill",
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
            WaveSpinner::WIDTH
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
            WaveSpinner::WIDTH
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

pub(crate) fn visual_line_count(lines: &[Line], width: u16) -> usize {
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

fn parse_diff_badge(s: &str) -> Option<(&str, &str, &str)> {
    let open_idx = s.rfind("(+")?;
    let close_idx = s[open_idx..].find(')')? + open_idx;
    let badge_content = &s[open_idx + 2..close_idx];
    let (add_part, rem_part) = badge_content.split_once(" -")?;
    if !add_part.chars().all(|c| c.is_ascii_digit()) || !rem_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let before = s[..open_idx].trim_end();
    Some((before, add_part, rem_part))
}
fn format_inline_code(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    if !text.contains('`') {
        return vec![Span::styled(text.to_string(), Style::default().fg(theme.ink))];
    }

    let mut spans = Vec::new();
    let mut remainder = text;

    while let Some(start_idx) = remainder.find('`') {
        let before = &remainder[..start_idx];
        if let Some(end_idx) = remainder[start_idx + 1..].find('`') {
            if !before.is_empty() {
                spans.push(Span::styled(before.to_string(), Style::default().fg(theme.ink)));
            }
            let code_content = &remainder[start_idx + 1..start_idx + 1 + end_idx];
            spans.push(Span::styled(
                code_content.to_string(),
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ));
            remainder = &remainder[start_idx + 1 + end_idx + 1..];
        } else {
            break;
        }
    }

    if !remainder.is_empty() {
        spans.push(Span::styled(remainder.to_string(), Style::default().fg(theme.ink)));
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), Style::default().fg(theme.ink)));
    }

    spans
}


pub(crate) fn format_transcript_lines(
    transcript: &str,
    theme: &Theme,
    mode_color: Color,
) -> Vec<Line<'static>> {
    let raw_lines: Vec<&str> = transcript.lines().collect();
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(raw_lines.len());
    let mut in_code_block = false;

    for (i, line) in raw_lines.iter().enumerate() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                in_code_block = false;
                lines.push(Line::from(vec![
                    Span::styled("└───", Style::default().fg(theme.dim)),
                ]));
            } else {
                in_code_block = true;
                let lang = line.trim_start().trim_start_matches('`').trim();
                if lang.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("┌───", Style::default().fg(theme.dim)),
                        Span::styled("─────────────────────────────────────", Style::default().fg(theme.dim)),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("┌─── ", Style::default().fg(theme.dim)),
                        Span::styled(lang.to_string(), Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)),
                        Span::styled(" ─────────────────────────────", Style::default().fg(theme.dim)),
                    ]));
                }
            }
            continue;
        }

        if in_code_block {
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(theme.dim)),
                Span::styled(line.to_string(), Style::default().fg(theme.ink)),
            ]));
            continue;
        }
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
            let is_failed = {
                let mut failed = false;
                for next_line in raw_lines.iter().skip(i + 1).take(25) {
                    let trimmed = next_line.trim_start();
                    if next_line.starts_with("⬢ ") || next_line.is_empty() {
                        break;
                    }
                    if trimmed.starts_with("└ failed:") || trimmed.starts_with("failed:") {
                        failed = true;
                        break;
                    }
                }
                failed
            };

            let marker_color = if is_failed { theme.error } else { theme.success };
            let mut spans = vec![
                Span::styled("⬢ ", Style::default().fg(marker_color).add_modifier(Modifier::BOLD)),
            ];

            let rest_trimmed = rest.trim();
            if let Some((before, add_part, rem_part)) = parse_diff_badge(rest_trimmed) {
                if let Some((verb, target)) = before.split_once(' ') {
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
                        before.to_string(),
                        Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                    ));
                }
                spans.push(Span::raw(" "));
                spans.push(Span::styled("(", Style::default().fg(theme.dim)));
                spans.push(Span::styled(
                    format!("+{add_part}"),
                    Style::default().fg(theme.success).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    format!("-{rem_part}"),
                    Style::default().fg(theme.error).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(")", Style::default().fg(theme.dim)));
            } else if let Some(target) = rest_trimmed.strip_prefix("Updated Plan") {
                spans.push(Span::styled(
                    "Updated Plan".to_string(),
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                ));
                let target_trimmed = target.trim();
                if !target_trimmed.is_empty() {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        target_trimmed.to_string(),
                        Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                    ));
                }
            } else if let Some((verb, target)) = rest_trimmed.split_once(' ') {
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
        } else if line.trim_start().starts_with("│ ") || line.trim_start() == "│" {
            let trimmed = line.trim_start();
            let indent_len = line.len() - trimmed.len();
            let indent = &line[..indent_len];
            let rest = if trimmed == "│" { "" } else { &trimmed["│ ".len()..] };
            let mut spans = vec![
                Span::styled(format!("{indent}│ "), Style::default().fg(theme.dim)),
            ];

            if let Some(item) = rest.strip_prefix("✔ ") {
                spans.push(Span::styled("✔ ", Style::default().fg(theme.dim)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.dim)));
            } else if let Some(item) = rest.strip_prefix("[✔] ") {
                spans.push(Span::styled("[✔] ", Style::default().fg(theme.dim)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.dim)));
            } else if let Some(item) = rest.strip_prefix("[✔]") {
                spans.push(Span::styled("[✔]", Style::default().fg(theme.dim)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.dim)));
            } else if let Some(item) = rest.strip_prefix("• ") {
                spans.push(Span::styled("• ", Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
            } else if let Some(item) = rest.strip_prefix("[•] ") {
                spans.push(Span::styled("[•] ", Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
            } else if let Some(item) = rest.strip_prefix("[•]") {
                spans.push(Span::styled("[•]", Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
            } else if let Some(item) = rest.strip_prefix("□ ") {
                spans.push(Span::styled("□ ", Style::default().fg(theme.quiet)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.quiet)));
            } else if let Some(item) = rest.strip_prefix("[ ] ") {
                spans.push(Span::styled("[ ] ", Style::default().fg(theme.quiet)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.quiet)));
            } else if let Some(item) = rest.strip_prefix("[ ]") {
                spans.push(Span::styled("[ ]", Style::default().fg(theme.quiet)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.quiet)));
            } else if rest.contains("✔ ") || rest.contains("[✔]") {
                spans.push(Span::styled(rest.to_string(), Style::default().fg(theme.dim)));
            } else if rest.contains("• ") || rest.contains("[•]") {
                spans.push(Span::styled(rest.to_string(), Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
            } else if rest.contains("□ ") || rest.contains("[ ]") {
                spans.push(Span::styled(rest.to_string(), Style::default().fg(theme.quiet)));
            } else {
                spans.push(Span::styled(rest.to_string(), Style::default().fg(theme.quiet)));
            }
            lines.push(Line::from(spans));
        } else if line.trim_start().starts_with("✔ ")
            || line.trim_start().starts_with("[✔]")
            || line.trim_start().starts_with("[•]")
            || line.trim_start().starts_with("□ ")
            || line.trim_start().starts_with("[ ]")
        {
            let trimmed = line.trim_start();
            let indent_len = line.len() - trimmed.len();
            let indent = &line[..indent_len];
            let mut spans = if indent.is_empty() {
                Vec::new()
            } else {
                vec![Span::raw(indent.to_string())]
            };
            if let Some(item) = trimmed.strip_prefix("✔ ") {
                spans.push(Span::styled("✔ ", Style::default().fg(theme.dim)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.dim)));
            } else if let Some(item) = trimmed.strip_prefix("[✔] ") {
                spans.push(Span::styled("[✔] ", Style::default().fg(theme.dim)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.dim)));
            } else if let Some(item) = trimmed.strip_prefix("[✔]") {
                spans.push(Span::styled("[✔]", Style::default().fg(theme.dim)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.dim)));
            } else if let Some(item) = trimmed.strip_prefix("[•] ") {
                spans.push(Span::styled("[•] ", Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
            } else if let Some(item) = trimmed.strip_prefix("[•]") {
                spans.push(Span::styled("[•]", Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)));
            } else if let Some(item) = trimmed.strip_prefix("□ ") {
                spans.push(Span::styled("□ ", Style::default().fg(theme.quiet)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.quiet)));
            } else if let Some(item) = trimmed.strip_prefix("[ ] ") {
                spans.push(Span::styled("[ ] ", Style::default().fg(theme.quiet)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.quiet)));
            } else if let Some(item) = trimmed.strip_prefix("[ ]") {
                spans.push(Span::styled("[ ]", Style::default().fg(theme.quiet)));
                spans.push(Span::styled(item.to_string(), Style::default().fg(theme.quiet)));
            } else {
                spans.push(Span::styled(trimmed.to_string(), Style::default().fg(theme.quiet)));
            }
            lines.push(Line::from(spans));
        } else if (line.starts_with(' ') || line.starts_with('\t'))
            && (line.trim_start().starts_with("- ") || line.trim_start() == "-")
        {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let removed_text = trimmed.strip_prefix("- ").unwrap_or("");
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{indent}- "),
                    Style::default().fg(theme.error).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    removed_text.to_string(),
                    Style::default().fg(theme.error),
                ),
            ]));
        } else if (line.starts_with(' ') || line.starts_with('\t'))
            && (line.trim_start().starts_with("+ ") || line.trim_start() == "+")
        {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let added_text = trimmed.strip_prefix("+ ").unwrap_or("");
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{indent}+ "),
                    Style::default().fg(theme.success).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    added_text.to_string(),
                    Style::default().fg(theme.success),
                ),
            ]));
        } else if (line.starts_with(' ') || line.starts_with('\t'))
            && line.trim_start().starts_with('⋯')
        {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.dim),
            )));
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
        } else if line.starts_with("### ") {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            )));
        } else if line.starts_with("## ") {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            )));
        } else if line.starts_with("# ") {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.amber).add_modifier(Modifier::BOLD),
            )));
        } else if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            let mut spans = vec![
                Span::styled("• ", Style::default().fg(theme.amber)),
            ];
            spans.extend(format_inline_code(rest, theme));
            lines.push(Line::from(spans));
        } else if (line.starts_with(' ') || line.starts_with('\t')) && line.trim_start().starts_with("* ") {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let rest = &trimmed["* ".len()..];
            let mut spans = vec![
                Span::raw(indent.to_string()),
                Span::styled("• ", Style::default().fg(theme.amber)),
            ];
            spans.extend(format_inline_code(rest, theme));
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(format_inline_code(line, theme)));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::ThemeKind;
    use ratatui::style::Color;

    #[test]
    fn test_visual_line_count_zero_width() {
        let lines = vec![Line::raw("hello"), Line::raw("world")];
        assert_eq!(visual_line_count(&lines, 0), 2);
    }

    #[test]
    fn test_visual_line_count_empty_line() {
        let lines = vec![Line::raw("")];
        assert_eq!(visual_line_count(&lines, 80), 1);
        let multi_empty = vec![Line::raw(""), Line::raw("")];
        assert_eq!(visual_line_count(&multi_empty, 80), 2);
    }

    #[test]
    fn test_visual_line_count_short_line() {
        let lines = vec![Line::raw("short text")];
        assert_eq!(visual_line_count(&lines, 80), 1);
    }

    #[test]
    fn test_visual_line_count_wrapped_lines() {
        let lines = vec![Line::raw("0123456789")];
        assert_eq!(visual_line_count(&lines, 4), 3);

        let lines = vec![Line::raw("0123456789"), Line::raw("abc")];
        assert_eq!(visual_line_count(&lines, 5), 3);
    }

    #[test]
    fn test_format_transcript_user_prompt() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;
        let lines = format_transcript_lines("> Hello Clawcode", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 2);
        assert_eq!(lines[0].spans[0].content, "┃ ");
        assert_eq!(lines[0].spans[1].content, "Hello Clawcode");
    }

    #[test]
    fn test_format_transcript_crabcode_success_and_failure() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "⬢ read_file src/main.rs\n└ done";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content, "⬢ ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.success));
        assert_eq!(lines[0].spans[1].content, "read_file");
        assert_eq!(lines[0].spans[3].content, "src/main.rs");

        let transcript = "⬢ edit_file src/main.rs\n└ failed: file not found";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content, "⬢ ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.error));
    }

    #[test]
    fn test_format_transcript_branch_lines() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let lines = format_transcript_lines("  └ completed successfully", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "  └ ");
        assert_eq!(lines[0].spans[1].content, "completed successfully");

        let lines = format_transcript_lines("  └ failed: permission denied", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "  └ ");
        assert_eq!(lines[0].spans[1].content, "failed: ");
        assert_eq!(lines[0].spans[2].content, "permission denied");
    }

    #[test]
    fn test_format_transcript_active_tool() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;
        let lines = format_transcript_lines("⬡ Reading src/tui/render.rs...", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "⬡ ");
        assert_eq!(lines[0].spans[0].style.fg, Some(mode_color));
        assert_eq!(lines[0].spans[1].content, "Reading src/tui/render.rs...");
    }

    #[test]
    fn test_format_transcript_historical_tool() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let lines = format_transcript_lines("⚙ [bash: cargo test]", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "⚙ ");
        assert_eq!(lines[0].spans[1].content, "bash");
        assert_eq!(lines[0].spans[3].content, "cargo test");

        let lines = format_transcript_lines("⚙ [cargo_check]", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "⚙ ");
        assert_eq!(lines[0].spans[1].content, "cargo_check");
    }

    #[test]
    fn test_format_transcript_success_and_failure_markers() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let lines = format_transcript_lines("✓ Build succeeded", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "  └ ");
        assert_eq!(lines[0].spans[1].content, "Build succeeded");

        let lines = format_transcript_lines("✗ Test failed", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "  └ failed: ");
        assert_eq!(lines[0].spans[1].content, "Test failed");
    }

    #[test]
    fn test_format_transcript_plain_text() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;
        let lines = format_transcript_lines("Plain response text from assistant", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "Plain response text from assistant");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.ink));
    }

    #[test]
    fn test_format_transcript_diff_lines() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "    - old removed line\n    + new added line";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 2);

        assert_eq!(lines[0].spans[0].content, "    - ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.error));
        assert_eq!(lines[0].spans[1].content, "old removed line");
        assert_eq!(lines[0].spans[1].style.fg, Some(theme.error));

        assert_eq!(lines[1].spans[0].content, "    + ");
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.success));
        assert_eq!(lines[1].spans[1].content, "new added line");
        assert_eq!(lines[1].spans[1].style.fg, Some(theme.success));
    }

    #[test]
    fn test_format_transcript_edit_diff_badge() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "⬢ Edit src/main.rs (+3 -1)\n    - old\n    + new\n  └ succeeded";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 4);

        assert_eq!(lines[0].spans[0].content, "⬢ ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.success));

        let add_span = lines[0].spans.iter().find(|s| s.content == "+3").expect("+3 badge span");
        assert_eq!(add_span.style.fg, Some(theme.success));

        let rem_span = lines[0].spans.iter().find(|s| s.content == "-1").expect("-1 badge span");
        assert_eq!(rem_span.style.fg, Some(theme.error));

        assert_eq!(lines[1].spans[0].style.fg, Some(theme.error));
        assert_eq!(lines[2].spans[0].style.fg, Some(theme.success));
        assert_eq!(lines[3].spans[0].content, "  └ ");
    }

    #[test]
    fn test_format_transcript_markdown_code_block() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "```rust\nfn main() {}\n```";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 3);

        // Header
        assert_eq!(lines[0].spans[0].content, "┌─── ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[0].spans[1].content, "rust");
        assert_eq!(lines[0].spans[1].style.fg, Some(theme.teal));

        // Content
        assert_eq!(lines[1].spans[0].content, "│ ");
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[1].spans[1].content, "fn main() {}");
        assert_eq!(lines[1].spans[1].style.fg, Some(theme.ink));

        // Footer
        assert_eq!(lines[2].spans[0].content, "└───");
        assert_eq!(lines[2].spans[0].style.fg, Some(theme.dim));
    }

    #[test]
    fn test_format_transcript_markdown_headings() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "# Heading 1\n## Heading 2\n### Heading 3";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 3);

        assert_eq!(lines[0].spans[0].content, "# Heading 1");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.amber));

        assert_eq!(lines[1].spans[0].content, "## Heading 2");
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.teal));

        assert_eq!(lines[2].spans[0].content, "### Heading 3");
        assert_eq!(lines[2].spans[0].style.fg, Some(theme.ink));
    }

    #[test]
    fn test_format_transcript_inline_code() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "Run `hello_world` now";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 3);

        assert_eq!(lines[0].spans[0].content, "Run ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.ink));

        assert_eq!(lines[0].spans[1].content, "hello_world");
        assert_eq!(lines[0].spans[1].style.fg, Some(theme.teal));

        assert_eq!(lines[0].spans[2].content, " now");
        assert_eq!(lines[0].spans[2].style.fg, Some(theme.ink));
    }

    #[test]
    fn test_format_transcript_bullet_list() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "- First item\n* Second item with `code`";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 2);

        assert_eq!(lines[0].spans[0].content, "• ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.amber));
        assert_eq!(lines[0].spans[1].content, "First item");
        assert_eq!(lines[0].spans[1].style.fg, Some(theme.ink));

        assert_eq!(lines[1].spans[0].content, "• ");
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.amber));
        assert_eq!(lines[1].spans[1].content, "Second item with ");
        assert_eq!(lines[1].spans[2].content, "code");
        assert_eq!(lines[1].spans[2].style.fg, Some(theme.teal));
    }

    #[test]
    fn test_format_transcript_visual_checklist_plan() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "\
⬢ Updated Plan
  │ ✔ 1. Selesai langkah pertama
  │ • 2. Sedang menjalankan langkah kedua
  │ □ 3. Langkah ketiga pending
  └ Plan updated: 3 steps";

        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 5);

        // Header: ⬢ Updated Plan
        assert_eq!(lines[0].spans[0].content, "⬢ ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.success));
        assert_eq!(lines[0].spans[1].content, "Updated Plan");

        // Line 1: completed item with ✔
        assert_eq!(lines[1].spans[0].content, "  │ ");
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[1].spans[1].content, "✔ ");
        assert_eq!(lines[1].spans[1].style.fg, Some(theme.dim));
        assert_eq!(lines[1].spans[2].content, "1. Selesai langkah pertama");
        assert_eq!(lines[1].spans[2].style.fg, Some(theme.dim));

        // Line 2: in_progress item with •
        assert_eq!(lines[2].spans[0].content, "  │ ");
        assert_eq!(lines[2].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[2].spans[1].content, "• ");
        assert_eq!(lines[2].spans[1].style.fg, Some(theme.teal));
        assert!(lines[2].spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(lines[2].spans[2].content, "2. Sedang menjalankan langkah kedua");
        assert_eq!(lines[2].spans[2].style.fg, Some(theme.teal));
        assert!(lines[2].spans[2].style.add_modifier.contains(Modifier::BOLD));

        // Line 3: pending item with □
        assert_eq!(lines[3].spans[0].content, "  │ ");
        assert_eq!(lines[3].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[3].spans[1].content, "□ ");
        assert_eq!(lines[3].spans[1].style.fg, Some(theme.quiet));
        assert_eq!(lines[3].spans[2].content, "3. Langkah ketiga pending");
        assert_eq!(lines[3].spans[2].style.fg, Some(theme.quiet));

        // Line 4: branch footer
        assert_eq!(lines[4].spans[0].content, "  └ ");
        assert_eq!(lines[4].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[4].spans[1].content, "Plan updated: 3 steps");
        assert_eq!(lines[4].spans[1].style.fg, Some(theme.quiet));
    }

    #[test]
    fn test_format_transcript_checklist_bracket_markers() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "  │ [✔] 1. Task A\n  │ [•] 2. Task B\n  │ [ ] 3. Task C";

        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 3);

        assert_eq!(lines[0].spans[0].content, "  │ ");
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[0].spans[1].content, "[✔] ");
        assert_eq!(lines[0].spans[1].style.fg, Some(theme.dim));

        assert_eq!(lines[1].spans[0].content, "  │ ");
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[1].spans[1].content, "[•] ");
        assert_eq!(lines[1].spans[1].style.fg, Some(theme.teal));
        assert!(lines[1].spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(lines[2].spans[0].content, "  │ ");
        assert_eq!(lines[2].spans[0].style.fg, Some(theme.dim));
        assert_eq!(lines[2].spans[1].content, "[ ] ");
        assert_eq!(lines[2].spans[1].style.fg, Some(theme.quiet));
    }
}
