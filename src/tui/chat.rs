use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use super::App;
use super::app::{StreamPart, ToolRow, ToolRowState};
use super::render::{
    identity_label, mode_label, render_command_popup, render_hints_row, render_input_card,
    status_label,
};
use super::theme::Theme;

pub fn render_chat(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, mode_color: Color) {
    app.set_last_quick_actions_area(None);
    if area.width == 0 || area.height == 0 {
        return;
    }
    let has_gap = area.height >= 16;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(4),
            if has_gap {
                Constraint::Length(1)
            } else {
                Constraint::Length(0)
            },
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

    let is_working = matches!(app.conversation_status(), super::ConversationStatus::Active)
        || app.is_streaming_active();
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

    let conversation_block = Block::default().borders(Borders::NONE);

    let mut tool_row_lines = Vec::new();
    let mut lines = if !app.stream_parts().is_empty() {
        let mut ordered = Vec::new();
        append_stream_parts(
            &mut ordered,
            app.stream_parts(),
            app,
            theme,
            mode_color,
            chunks[1].width,
            &mut tool_row_lines,
        );
        ordered
    } else {
        let mut ordered = format_transcript_lines_with_width(
            app.transcript(),
            theme,
            mode_color,
            Some(chunks[1].width),
        );
        for line in &mut ordered {
            indent_assistant_line(line);
        }
        ordered
    };

    if app.is_typing() {
        let is_empty_or_prompt = lines.is_empty()
            || lines
                .last()
                .map(|l| {
                    l.spans.is_empty()
                        || l.spans.iter().all(|s| s.content.trim().is_empty())
                        || l.spans.first().is_some_and(|s| {
                            s.content.as_ref() == "▌"
                                || s.content.as_ref() == "▌ "
                                || s.content.as_ref() == "┃ "
                        })
                })
                .unwrap_or(false);

        if is_empty_or_prompt {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled("▋", Style::default().fg(mode_color)),
            ]));
        } else if let Some(last_line) = lines.last_mut() {
            last_line
                .spans
                .push(Span::styled("▋", Style::default().fg(mode_color)));
        }
    }

    if let Some(metrics) = app.metrics() {
        let mut meta_spans = vec![
            Span::raw("   "),
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
        meta_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
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
            meta_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
            meta_spans.push(Span::styled(tps, Style::default().fg(theme.dim)));
        }

        let dur_str = if metrics.duration.as_secs() > 0 {
            format!("{:.1}s", metrics.duration.as_secs_f64())
        } else {
            format!("{}ms", metrics.duration.as_millis())
        };
        meta_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
        meta_spans.push(Span::styled(dur_str, Style::default().fg(theme.dim)));

        lines.push(Line::from(String::new()));
        lines.push(Line::from(meta_spans));
    }

    let transcript_area = chunks[1];

    let content_width = transcript_area.width;
    let total_visual_lines = visual_line_count(&lines, content_width).min(u16::MAX as usize) as u16;
    let visible_height = transcript_area.height;
    let max_scroll = total_visual_lines.saturating_sub(visible_height);
    let scroll_offset = app.chat_scroll().min(max_scroll);
    let scroll_y = max_scroll.saturating_sub(scroll_offset);

    let mut prefix_visual_lines = Vec::with_capacity(lines.len() + 1);
    prefix_visual_lines.push(0u16);
    let mut running = 0u16;
    for line in &lines {
        running = running.saturating_add(
            visual_line_count(std::slice::from_ref(line), content_width).min(u16::MAX as usize)
                as u16,
        );
        prefix_visual_lines.push(running);
    }

    let clicks = tool_row_lines
        .into_iter()
        .filter_map(|(call_id, line_index)| {
            let before = prefix_visual_lines[line_index.min(lines.len())];
            let y = transcript_area
                .y
                .saturating_add(before)
                .saturating_sub(scroll_y);
            (y >= transcript_area.y && y < transcript_area.y.saturating_add(transcript_area.height))
                .then_some((
                    call_id,
                    Rect {
                        x: transcript_area.x,
                        y,
                        width: transcript_area.width,
                        height: 1,
                    },
                ))
        })
        .collect();
    app.set_tool_row_clicks(clicks);
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
            .track_symbol(None)
            .thumb_style(Style::default().fg(theme.dim))
            .thumb_symbol("┃");
        frame.render_stateful_widget(scrollbar, transcript_area, &mut scrollbar_state);
    }

    if scroll_offset > 0 && transcript_area.width >= 30 && transcript_area.height >= 2 {
        let badge_text = format!(" ↓ {scroll_offset} lines up (End to bottom) ");
        let badge_width = badge_text.len() as u16;
        if transcript_area.width > badge_width + 2 {
            let offset_x = transcript_area
                .width
                .saturating_sub(badge_width)
                .saturating_sub(2);
            let offset_y = transcript_area.height.saturating_sub(1);
            let badge_area = Rect {
                x: transcript_area.x.saturating_add(offset_x),
                y: transcript_area.y.saturating_add(offset_y),
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

    render_input_card(frame, chunks[3], app, theme, mode_color);
    render_command_popup(frame, chunks[3], app, theme);
    render_hints_row(frame, chunks[4], app, theme);
}

fn indent_assistant_line(line: &mut Line<'static>) {
    if line.spans.is_empty() {
        return;
    }
    if line
        .spans
        .first()
        .is_some_and(|s| s.content.as_ref() == "▌" || s.content.as_ref() == "▌ ")
    {
        return;
    }
    if line.style.bg.is_some() {
        return;
    }
    if line
        .spans
        .first()
        .is_some_and(|s| s.content.starts_with("   "))
    {
        return;
    }
    line.spans.insert(0, Span::raw("   "));
}

#[allow(clippy::too_many_arguments)]
fn append_stream_parts(
    lines: &mut Vec<Line<'static>>,
    parts: &[StreamPart],
    app: &App,
    theme: &Theme,
    mode_color: Color,
    width: u16,
    tool_row_lines: &mut Vec<(String, usize)>,
) {
    for part in parts {
        match part {
            StreamPart::User(prompt) => {
                let card_width = if width > 2 {
                    (width.saturating_sub(2) as usize).max(20)
                } else {
                    width as usize
                };
                let p_lines: Vec<&str> = prompt.lines().collect();
                if p_lines.is_empty() {
                    let content_line = Line::from(vec![
                        Span::styled(
                            "▌ ",
                            Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            String::new(),
                            Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                        ),
                    ]);
                    lines.push(pad_card_line(content_line, card_width, theme.bg_element));
                } else {
                    for (idx, pline) in p_lines.iter().enumerate() {
                        let prefix = if idx == 0 { "▌ " } else { "  " };
                        let content_line = Line::from(vec![
                            Span::styled(
                                prefix,
                                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                (*pline).to_string(),
                                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                            ),
                        ]);
                        lines.push(pad_card_line(content_line, card_width, theme.bg_element));
                    }
                }
                lines.push(Line::from(""));
            }
            StreamPart::Text(text) => {
                let mut rendered =
                    format_transcript_lines_with_width(text, theme, mode_color, Some(width));
                for line in &mut rendered {
                    indent_assistant_line(line);
                }
                lines.extend(rendered);
            }
            StreamPart::Reasoning(text) => {
                let dur_str = format!(
                    "{:.1}s",
                    app.reasoning_elapsed_seconds().unwrap_or(0.1).max(0.1)
                );
                if app.is_reasoning() {
                    lines.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled(
                            format!("{} Thinking", app.wave_spinner().compact_frame()),
                            Style::default()
                                .fg(theme.amber)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                } else {
                    tool_row_lines.push(("__thought__".to_string(), lines.len()));
                    if app.is_thought_expanded() {
                        lines.push(Line::from(vec![
                            Span::raw("   "),
                            Span::styled(
                                format!("- Thought for {dur_str}"),
                                Style::default().fg(theme.amber),
                            ),
                        ]));
                        for l in text.lines() {
                            lines.push(Line::from(vec![
                                Span::raw("      │ "),
                                Span::styled(l.to_string(), Style::default().fg(theme.quiet)),
                            ]));
                        }
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("   "),
                            Span::styled(
                                format!("+ Thought for {dur_str}"),
                                Style::default().fg(theme.amber),
                            ),
                        ]));
                    }
                    lines.push(Line::from(""));
                }
            }
            StreamPart::Tool(call_id) => {
                let Some(row) = app.tool_rows().iter().find(|row| row.call_id == *call_id) else {
                    continue;
                };
                render_tool_card_or_row(lines, row, app, theme, mode_color, width, tool_row_lines);
            }
        }
    }
}

fn shell_command(row: &ToolRow) -> String {
    if row.desc == "preparing arguments..." {
        return "Preparing bash...".to_string();
    }
    if !row.desc.is_empty() {
        let clean = row.desc.strip_prefix("$ ").unwrap_or(&row.desc).trim();
        if !clean.is_empty() {
            return clean.to_string();
        }
    }
    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments)
        && let Some(cmd) = args.get("command").and_then(|v| v.as_str())
    {
        let clean = cmd.strip_prefix("$ ").unwrap_or(cmd).trim();
        if !clean.is_empty() {
            return clean.to_string();
        }
    }
    let clean = row
        .arguments
        .strip_prefix("$ ")
        .unwrap_or(&row.arguments)
        .trim();
    if !clean.is_empty() {
        clean.to_string()
    } else {
        "bash".to_string()
    }
}

fn pad_card_line(mut line: Line<'static>, card_width: usize, bg: Color) -> Line<'static> {
    let current_width = line.width();
    if current_width < card_width {
        line.spans.push(Span::styled(
            " ".repeat(card_width - current_width),
            Style::default().bg(bg),
        ));
    }
    line.style(Style::default().bg(bg))
}

fn empty_card_line(card_width: usize, bg: Color) -> Line<'static> {
    Line::from(Span::styled(
        " ".repeat(card_width),
        Style::default().bg(bg),
    ))
    .style(Style::default().bg(bg))
}
pub fn strip_ansi_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_csi = false;
    for ch in s.chars() {
        if ch == '\x1b' {
            in_csi = true;
        } else if in_csi {
            if ch.is_ascii_alphabetic() {
                in_csi = false;
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render_shell_card(
    lines: &mut Vec<Line<'static>>,
    row: &ToolRow,
    app: &App,
    theme: &Theme,
    mode_color: Color,
    width: u16,
    tool_row_lines: &mut Vec<(String, usize)>,
) {
    let card_width = (width as usize).saturating_sub(2).max(20);
    let cmd = shell_command(row);
    let is_running = matches!(row.state, ToolRowState::Pending | ToolRowState::Running);

    if !lines.is_empty()
        && !lines
            .last()
            .map(|l| l.spans.is_empty() || (l.spans.len() == 1 && l.spans[0].content.is_empty()))
            .unwrap_or(false)
    {
        lines.push(Line::from(""));
    }

    if is_running {
        // Line 1: empty padding line
        lines.push(empty_card_line(card_width, theme.bg_element));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

        // Line 2: command line
        let spinner_frame = format!("{} ", app.wave_spinner().compact_frame());
        let marker_color = if row.state == ToolRowState::Pending {
            theme.dim
        } else {
            mode_color
        };
        let cmd_line = Line::from(vec![
            Span::raw("  "),
            Span::styled(
                spinner_frame,
                Style::default()
                    .fg(marker_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(cmd, Style::default().fg(theme.ink)),
        ]);
        lines.push(pad_card_line(cmd_line, card_width, theme.bg_element));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

        // Line 3: empty padding line
        lines.push(empty_card_line(card_width, theme.bg_element));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

        // Followed by a blank line (normal background) before the next item.
        lines.push(Line::from(""));
    } else {
        let output_trimmed = row.output.trim();
        if output_trimmed.is_empty() {
            // Line 1: empty padding line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Line 2: command line
            let marker_color = if row.state == ToolRowState::Failed {
                theme.error
            } else {
                theme.amber
            };
            let text_color = if row.state == ToolRowState::Failed {
                theme.error
            } else {
                theme.ink
            };
            let cmd_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "$ ",
                    Style::default()
                        .fg(marker_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(cmd, Style::default().fg(text_color)),
            ]);
            lines.push(pad_card_line(cmd_line, card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Line 3: empty padding line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Followed by a blank line (normal background)
            lines.push(Line::from(""));
        } else {
            // Line 1: empty padding line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Line 2: command line
            let marker_color = if row.state == ToolRowState::Failed {
                theme.error
            } else {
                theme.amber
            };
            let text_color = if row.state == ToolRowState::Failed {
                theme.error
            } else {
                theme.ink
            };
            let cmd_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "$ ",
                    Style::default()
                        .fg(marker_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(cmd, Style::default().fg(text_color)),
            ]);
            lines.push(pad_card_line(cmd_line, card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Line 3..N: output lines
            let raw_lines: Vec<&str> = row.output.lines().collect();
            let total = raw_lines.len();
            let max_preview = 10;
            let is_expanded = app.is_tool_expanded(&row.call_id);
            let display_lines = if is_expanded || total <= max_preview {
                &raw_lines[..]
            } else {
                &raw_lines[..max_preview]
            };
            for line_text in display_lines {
                let out_line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(strip_ansi_codes(line_text), Style::default().fg(theme.ink)),
                ]);
                lines.push(pad_card_line(out_line, card_width, theme.bg_element));
                tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            }

            // If output > 10 lines:
            if total > max_preview {
                let remaining = total - max_preview;
                let hint_text = if is_expanded {
                    "↳ click to collapse".to_string()
                } else if remaining == 1 {
                    "↳ click to expand (1 more line)".to_string()
                } else {
                    format!("↳ click to expand ({} more lines)", remaining)
                };
                let hint_line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(hint_text, Style::default().fg(theme.dim)),
                ]);
                lines.push(pad_card_line(hint_line, card_width, theme.bg_element));
                tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            }

            // Final line: empty padding line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Followed by a blank line (normal background)
            lines.push(Line::from(""));
        }
    }
}

fn extract_diff_lines(row: &ToolRow) -> Vec<crate::tui::diff::DiffLine> {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments) else {
        return Vec::new();
    };
    if matches!(row.name.as_str(), "edit_file" | "edit") {
        let old_str = args
            .get("old_string")
            .or_else(|| args.get("old_str"))
            .and_then(|v| v.as_str());
        let new_str = args
            .get("new_string")
            .or_else(|| args.get("new_str"))
            .and_then(|v| v.as_str());
        if let (Some(old_str), Some(new_str)) = (old_str, new_str) {
            let diff = crate::tui::diff::compute_diff(old_str, new_str, usize::MAX);
            return diff.lines;
        }
    } else if matches!(row.name.as_str(), "patch" | "apply_patch")
        && let Some(patch_str) = args.get("patch").and_then(|v| v.as_str())
    {
        let mut diff_lines = Vec::new();
        for l in patch_str.lines() {
            if l.starts_with('+') && !l.starts_with("+++") {
                diff_lines.push(crate::tui::diff::DiffLine {
                    op: crate::tui::diff::DiffOp::Add,
                    text: l[1..].to_string(),
                });
            } else if l.starts_with('-') && !l.starts_with("---") {
                diff_lines.push(crate::tui::diff::DiffLine {
                    op: crate::tui::diff::DiffOp::Remove,
                    text: l[1..].to_string(),
                });
            } else if let Some(stripped) = l.strip_prefix(' ') {
                diff_lines.push(crate::tui::diff::DiffLine {
                    op: crate::tui::diff::DiffOp::Same,
                    text: stripped.to_string(),
                });
            }
        }
        return diff_lines;
    }
    Vec::new()
}

#[allow(clippy::too_many_arguments)]
fn render_diff_card(
    lines: &mut Vec<Line<'static>>,
    row: &ToolRow,
    app: &App,
    theme: &Theme,
    mode_color: Color,
    width: u16,
    tool_row_lines: &mut Vec<(String, usize)>,
) {
    let card_width = (width as usize).saturating_sub(2).max(20);
    let mut title = tool_row_detail(row);
    if row.state == ToolRowState::Failed {
        let first = row.output.lines().next().unwrap_or("error").trim();
        let err_msg = first
            .strip_prefix(&format!("Error executing {}: ", row.name))
            .unwrap_or(first);
        if !err_msg.is_empty() {
            title.push_str(" · failed: ");
            title.push_str(err_msg);
        }
    }
    let is_running = matches!(row.state, ToolRowState::Pending | ToolRowState::Running);

    if !lines.is_empty()
        && !lines
            .last()
            .map(|l| l.spans.is_empty() || (l.spans.len() == 1 && l.spans[0].content.is_empty()))
            .unwrap_or(false)
    {
        lines.push(Line::from(""));
    }

    if is_running {
        // Line 1: empty padding line
        lines.push(empty_card_line(card_width, theme.bg_element));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

        // Line 2: command line
        let spinner_frame = format!("{} ", app.wave_spinner().compact_frame());
        let marker_color = if row.state == ToolRowState::Pending {
            theme.dim
        } else {
            mode_color
        };
        let title_line = Line::from(vec![
            Span::raw("  "),
            Span::styled(
                spinner_frame,
                Style::default()
                    .fg(marker_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(title, Style::default().fg(theme.ink)),
        ]);
        lines.push(pad_card_line(title_line, card_width, theme.bg_element));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

        // Line 3: empty padding line
        lines.push(empty_card_line(card_width, theme.bg_element));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

        // Followed by a blank line (normal background) before the next item.
        lines.push(Line::from(""));
    } else {
        let diff_lines = extract_diff_lines(row);
        if diff_lines.is_empty() {
            // Line 1: empty padding line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Line 2: title line
            let marker_color = if row.state == ToolRowState::Failed {
                theme.error
            } else {
                theme.quiet
            };
            let title_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "• ",
                    Style::default()
                        .fg(marker_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    title,
                    Style::default().fg(if row.state == ToolRowState::Failed {
                        theme.error
                    } else {
                        theme.ink
                    }),
                ),
            ]);
            lines.push(pad_card_line(title_line, card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Line 3: empty padding line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Followed by a blank line (normal background)
            lines.push(Line::from(""));
        } else {
            // Line 1: empty padding line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Line 2: Title:   • Edit <path> (+A -R)
            let marker_color = if row.state == ToolRowState::Failed {
                theme.error
            } else {
                theme.quiet
            };
            let title_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "• ",
                    Style::default()
                        .fg(marker_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    title,
                    Style::default().fg(if row.state == ToolRowState::Failed {
                        theme.error
                    } else {
                        theme.ink
                    }),
                ),
            ]);
            lines.push(pad_card_line(title_line, card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Line 3: Blank gap line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Diff lines
            let total = diff_lines.len();
            let max_preview = 10;
            let is_expanded = app.is_tool_expanded(&row.call_id);
            let display_lines = if is_expanded || total <= max_preview {
                &diff_lines[..]
            } else {
                &diff_lines[..max_preview]
            };
            for dl in display_lines {
                let line = match dl.op {
                    crate::tui::diff::DiffOp::Remove => Line::from(vec![
                        Span::raw("     "),
                        Span::styled(format!("- {}", dl.text), Style::default().fg(theme.error)),
                    ]),
                    crate::tui::diff::DiffOp::Add => Line::from(vec![
                        Span::raw("     "),
                        Span::styled(format!("+ {}", dl.text), Style::default().fg(theme.success)),
                    ]),
                    crate::tui::diff::DiffOp::Same => Line::from(vec![
                        Span::raw("       "),
                        Span::styled(dl.text.clone(), Style::default().fg(theme.quiet)),
                    ]),
                };
                lines.push(pad_card_line(line, card_width, theme.bg_element));
                tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            }

            // If diff > 10 lines:
            if total > max_preview {
                let remaining = total - max_preview;
                let hint_text = if is_expanded {
                    "↳ click to collapse".to_string()
                } else if remaining == 1 {
                    "↳ click to expand (1 more line)".to_string()
                } else {
                    format!("↳ click to expand ({} more lines)", remaining)
                };
                let hint_line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(hint_text, Style::default().fg(theme.dim)),
                ]);
                lines.push(pad_card_line(hint_line, card_width, theme.bg_element));
                tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            }

            // Final line: empty padding line
            lines.push(empty_card_line(card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

            // Followed by a blank line (normal background)
            lines.push(Line::from(""));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_tool_card_or_row(
    lines: &mut Vec<Line<'static>>,
    row: &ToolRow,
    app: &App,
    theme: &Theme,
    mode_color: Color,
    width: u16,
    tool_row_lines: &mut Vec<(String, usize)>,
) {
    if matches!(row.name.as_str(), "bash" | "sh") {
        render_shell_card(lines, row, app, theme, mode_color, width, tool_row_lines);
    } else if matches!(
        row.name.as_str(),
        "edit_file" | "edit" | "patch" | "apply_patch"
    ) {
        render_diff_card(lines, row, app, theme, mode_color, width, tool_row_lines);
    } else {
        render_generic_tool_card(lines, row, app, theme, mode_color, width, tool_row_lines);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_generic_tool_card(
    lines: &mut Vec<Line<'static>>,
    row: &ToolRow,
    app: &App,
    theme: &Theme,
    mode_color: Color,
    width: u16,
    tool_row_lines: &mut Vec<(String, usize)>,
) {
    lines.push(compact_tool_line(row, app, theme, mode_color, width));
    tool_row_lines.push((row.call_id.clone(), lines.len() - 1));

    append_specialized_detail(lines, row, theme, tool_row_lines);
}

fn append_specialized_detail(
    lines: &mut Vec<Line<'static>>,
    row: &ToolRow,
    theme: &Theme,
    tool_row_lines: &mut Vec<(String, usize)>,
) {
    let terminal = row.state == ToolRowState::Completed || row.state == ToolRowState::Failed;
    if !terminal && !matches!(row.name.as_str(), "task" | "execute" | "update_plan") {
        return;
    }
    let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments) else {
        return;
    };
    match row.name.as_str() {
        "task" => {
            let agent = args
                .get("subagent_type")
                .or_else(|| args.get("agent"))
                .and_then(|value| value.as_str())
                .unwrap_or("subagent");
            let description = args
                .get("description")
                .or_else(|| args.get("prompt"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let suffix = if description.is_empty() {
                String::new()
            } else {
                format!(": {description}")
            };
            let line = Line::from(vec![
                Span::styled("      ↳ ", Style::default().fg(theme.quiet)),
                Span::styled(format!("{agent}{suffix}"), Style::default().fg(theme.quiet)),
            ]);
            lines.push(line);
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            if let Some(session_id) = row
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("sessionId"))
                .and_then(|value| value.as_str())
            {
                let session_line = Line::from(vec![
                    Span::styled("      ↳ ", Style::default().fg(theme.quiet)),
                    Span::styled(
                        format!("child session {session_id}"),
                        Style::default().fg(theme.dim),
                    ),
                ]);
                lines.push(session_line);
                tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            }
        }
        "execute" => {
            if let Some(calls) = args.get("toolCalls").and_then(|value| value.as_array()) {
                for call in calls.iter().take(16) {
                    let Some(tool) = call.get("tool").and_then(|value| value.as_str()) else {
                        continue;
                    };
                    let Some(status) = call.get("status").and_then(|value| value.as_str()) else {
                        let line = Line::from(vec![
                            Span::styled("      ↳ ", Style::default().fg(theme.quiet)),
                            Span::styled(tool.to_string(), Style::default().fg(theme.quiet)),
                        ]);
                        lines.push(line);
                        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
                        continue;
                    };
                    let (marker, color) = match status {
                        "completed" | "complete" => ("✓", theme.success),
                        "error" | "failed" => ("×", theme.error),
                        "running" | "pending" | "in_progress" | "in-progress" => ("⠋", theme.quiet),
                        _ => ("·", theme.quiet),
                    };
                    let line = Line::from(vec![
                        Span::styled("      ↳ ", Style::default().fg(theme.quiet)),
                        Span::styled(format!("{marker} {tool}"), Style::default().fg(color)),
                    ]);
                    lines.push(line);
                    tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
                }
            }
        }
        "update_plan" => {
            let Some(plan) = args.get("plan").and_then(|value| value.as_array()) else {
                return;
            };
            for item in plan.iter() {
                let Some(step) = item
                    .get("step")
                    .or_else(|| item.get("content"))
                    .or_else(|| item.get("title"))
                    .and_then(|value| value.as_str())
                else {
                    continue;
                };
                let status = item
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or("pending");
                let (status_icon, status_color) = match status {
                    "completed" | "complete" | "done" => ("✔ ", theme.success),
                    "in_progress" | "in-progress" | "running" => ("• ", theme.amber),
                    _ => ("□ ", theme.dim),
                };
                let line = Line::from(vec![
                    Span::styled("      │ ", Style::default().fg(theme.dim)),
                    Span::styled(status_icon, Style::default().fg(status_color)),
                    Span::styled(step.to_string(), Style::default().fg(theme.quiet)),
                ]);
                lines.push(line);
                tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            }
        }
        _ => {}
    }
}

fn compact_tool_line(
    row: &ToolRow,
    app: &App,
    theme: &Theme,
    mode_color: Color,
    width: u16,
) -> Line<'static> {
    let marker = match row.state {
        ToolRowState::Running => Span::styled(
            format!("{} ", app.wave_spinner().compact_frame()),
            Style::default().fg(mode_color),
        ),
        ToolRowState::Pending => Span::styled(
            format!("{} ", app.wave_spinner().compact_frame()),
            Style::default().fg(theme.dim),
        ),
        ToolRowState::Completed => Span::styled(
            "● ",
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        ),
        ToolRowState::Failed => Span::styled(
            "× ",
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let (verb, mut target, detail): (&str, String, Option<String>) =
        if row.desc == "preparing arguments..." {
            ("Preparing", format!("{}...", row.name), None)
        } else {
            let target_str = if row.desc.is_empty() {
                row.arguments.clone()
            } else {
                row.desc.clone()
            };
            match row.name.as_str() {
                "read_file" | "read" => {
                    let clean = target_str
                        .strip_prefix("Read ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Read", clean, None)
                }
                "write_file" | "write" => {
                    let clean = target_str
                        .strip_prefix("Write ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Write", clean, None)
                }
                "edit_file" | "edit" => {
                    let diff_detail = if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&row.arguments)
                        && let Some(old_str) = args
                            .get("old_string")
                            .or_else(|| args.get("old_str"))
                            .and_then(|v| v.as_str())
                        && let Some(new_str) = args
                            .get("new_string")
                            .or_else(|| args.get("new_str"))
                            .and_then(|v| v.as_str())
                    {
                        let diff = crate::tui::diff::compute_diff(old_str, new_str, 0);
                        Some(format!("+{} -{}", diff.added, diff.removed))
                    } else {
                        None
                    };
                    let clean = target_str
                        .strip_prefix("Edit ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Edit", clean, diff_detail)
                }
                "patch" | "apply_patch" => {
                    let patch_detail = if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&row.arguments)
                        && let Some(patch_str) = args.get("patch").and_then(|v| v.as_str())
                    {
                        let mut added = 0usize;
                        let mut removed = 0usize;
                        for l in patch_str.lines() {
                            if l.starts_with('+') && !l.starts_with("+++") {
                                added += 1;
                            } else if l.starts_with('-') && !l.starts_with("---") {
                                removed += 1;
                            }
                        }
                        Some(format!("+{added} -{removed}"))
                    } else {
                        None
                    };
                    let clean = target_str
                        .strip_prefix("Patch ")
                        .or_else(|| target_str.strip_prefix("Applied patch "))
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Patch", clean, patch_detail)
                }
                "list_dir" => {
                    let clean = target_str
                        .strip_prefix("List ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("List", clean, None)
                }
                "glob_search" | "glob" => {
                    let count_detail = if row.state == ToolRowState::Completed {
                        let count = row
                            .metadata
                            .as_ref()
                            .and_then(|m| m.get("count").or_else(|| m.get("matches")))
                            .and_then(|v| v.as_u64())
                            .map(|c| c as usize)
                            .unwrap_or_else(|| {
                                row.output.lines().filter(|l| !l.trim().is_empty()).count()
                            });
                        Some(format!(
                            "{} {}",
                            count,
                            if count == 1 { "match" } else { "matches" }
                        ))
                    } else {
                        None
                    };
                    let clean = target_str
                        .strip_prefix("Glob ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Glob", clean, count_detail)
                }
                "grep_search" | "grep" => {
                    let count_detail = if row.state == ToolRowState::Completed {
                        let count = row
                            .metadata
                            .as_ref()
                            .and_then(|m| m.get("count").or_else(|| m.get("matches")))
                            .and_then(|v| v.as_u64())
                            .map(|c| c as usize)
                            .unwrap_or_else(|| {
                                row.output.lines().filter(|l| !l.trim().is_empty()).count()
                            });
                        Some(format!(
                            "{} {}",
                            count,
                            if count == 1 { "match" } else { "matches" }
                        ))
                    } else {
                        None
                    };
                    let clean = target_str
                        .strip_prefix("Grep ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Grep", clean, count_detail)
                }
                "webfetch" | "fetch" => {
                    let clean = target_str
                        .strip_prefix("Fetch ")
                        .or_else(|| target_str.strip_prefix("Fetched "))
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Fetch", clean, None)
                }
                "websearch" | "search" => {
                    let clean = target_str
                        .strip_prefix("Search ")
                        .or_else(|| target_str.strip_prefix("Searched "))
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Search", clean, None)
                }
                "update_plan" => ("Updated Plan", String::new(), None),
                "task" => {
                    let clean = target_str
                        .strip_prefix("Task ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Task", clean, None)
                }
                "execute" => {
                    let clean = target_str
                        .strip_prefix("Execute ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Execute", clean, None)
                }
                "question" => {
                    let clean = target_str
                        .strip_prefix("Ask ")
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Ask", clean, None)
                }
                "skill" => {
                    let clean = target_str
                        .strip_prefix("Load skill ")
                        .or_else(|| target_str.strip_prefix("Loaded skill "))
                        .unwrap_or(&target_str)
                        .to_string();
                    ("Load skill", clean, None)
                }
                "bash" | "sh" => {
                    let clean = target_str.strip_prefix("$ ").unwrap_or(&target_str).trim();
                    if clean.is_empty() {
                        ("bash", String::new(), None)
                    } else {
                        ("", clean.to_string(), None)
                    }
                }
                _ => {
                    let clean = target_str
                        .strip_prefix("Run ")
                        .unwrap_or(&target_str)
                        .to_string();
                    if clean.is_empty() {
                        ("Run", row.name.clone(), None)
                    } else {
                        ("Run", clean, None)
                    }
                }
            }
        };

    let err_msg = if row.state == ToolRowState::Failed {
        let first = row.output.lines().next().unwrap_or("error").trim();
        let stripped = first
            .strip_prefix(&format!("Error executing {}: ", row.name))
            .unwrap_or(first);
        if stripped.is_empty() {
            first.to_string()
        } else {
            stripped.to_string()
        }
    } else {
        String::new()
    };

    let indent_w = 3;
    let marker_w = 2;
    let verb_w = if verb.is_empty() {
        0
    } else if target.is_empty() {
        text_cell_width(verb)
    } else {
        text_cell_width(verb) + 1
    };
    let detail_w = detail.as_ref().map(|d| text_cell_width(d) + 3).unwrap_or(0);
    let err_w = if row.state == ToolRowState::Failed && !err_msg.is_empty() {
        text_cell_width(&err_msg) + 11
    } else {
        0
    };

    let overhead = indent_w + marker_w + verb_w + detail_w + err_w;
    let max_target_width = (width as usize).saturating_sub(overhead + 2);
    if width > 0 && max_target_width >= 8 && text_cell_width(&target) > max_target_width {
        target = truncate_tool_text(&target, max_target_width);
    }

    let mut spans = vec![Span::raw("   "), marker];

    if !verb.is_empty() {
        let verb_text = if target.is_empty() {
            verb.to_string()
        } else {
            format!("{verb} ")
        };
        spans.push(Span::styled(
            verb_text,
            Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
        ));
    }

    if !target.is_empty() {
        spans.push(Span::styled(target, Style::default().fg(theme.ink)));
    }

    if let Some(detail) = detail {
        spans.push(Span::styled(
            format!(" ({detail})"),
            Style::default().fg(theme.quiet),
        ));
    }

    if row.state == ToolRowState::Failed && !err_msg.is_empty() {
        spans.push(Span::styled(
            format!(" · failed: {err_msg}"),
            Style::default().fg(theme.error),
        ));
    }

    Line::from(spans)
}

pub(crate) fn visual_line_count(lines: &[Line], width: u16) -> usize {
    if width == 0 {
        return lines.len();
    }
    let mut count: usize = 0;
    for line in lines {
        let w = line.width();
        let rows = if w == 0 {
            1
        } else {
            (w.saturating_add(width as usize - 1)) / width as usize
        };
        count = count.saturating_add(rows);
    }
    count
}

fn tool_row_detail(row: &ToolRow) -> String {
    if row.desc == "preparing arguments..." {
        return format!("Preparing {}...", row.name);
    }
    let target = if row.desc.is_empty() {
        row.arguments.clone()
    } else {
        row.desc.clone()
    };
    let label = match row.name.as_str() {
        "read_file" | "read" => "Read",
        "write_file" | "write" => "Write",
        "edit_file" | "edit" => "Edit",
        "patch" | "apply_patch" => "Patch",
        "list_dir" => "List",
        "glob_search" | "glob" => "Glob",
        "grep_search" | "grep" => "Grep",
        "webfetch" | "fetch" => "Fetch",
        "websearch" | "search" => "Search",
        "update_plan" => "Updated Plan",
        "task" => "Task",
        "execute" => "Execute",
        "question" => "Ask",
        "skill" => "Load skill",
        "bash" | "sh" => "",
        _ => "Run",
    };
    if matches!(row.name.as_str(), "edit_file" | "edit")
        && let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments)
        && let Some(old_str) = args
            .get("old_string")
            .or_else(|| args.get("old_str"))
            .and_then(|v| v.as_str())
        && let Some(new_str) = args
            .get("new_string")
            .or_else(|| args.get("new_str"))
            .and_then(|v| v.as_str())
    {
        let diff = crate::tui::diff::compute_diff(old_str, new_str, 0);
        let count_suffix = format!(" (+{} -{})", diff.added, diff.removed);
        let clean_target = target.strip_prefix("Edit ").unwrap_or(&target);
        if clean_target.is_empty() {
            format!("{label}{count_suffix}")
        } else {
            format!("{label} {clean_target}{count_suffix}")
        }
    } else if matches!(row.name.as_str(), "patch" | "apply_patch")
        && let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments)
        && let Some(patch_str) = args.get("patch").and_then(|v| v.as_str())
    {
        let mut added = 0usize;
        let mut removed = 0usize;
        for l in patch_str.lines() {
            if l.starts_with('+') && !l.starts_with("+++") {
                added += 1;
            } else if l.starts_with('-') && !l.starts_with("---") {
                removed += 1;
            }
        }
        let count_suffix = format!(" (+{added} -{removed})");
        let clean_target = target.strip_prefix("Patch ").unwrap_or(&target);
        if clean_target.is_empty() {
            format!("{label}{count_suffix}")
        } else {
            format!("{label} {clean_target}{count_suffix}")
        }
    } else if matches!(
        row.name.as_str(),
        "glob_search" | "glob" | "grep_search" | "grep"
    ) && row.state == ToolRowState::Completed
    {
        let count = row
            .metadata
            .as_ref()
            .and_then(|m| m.get("count").or_else(|| m.get("matches")))
            .and_then(|v| v.as_u64())
            .map(|c| c as usize)
            .unwrap_or_else(|| row.output.lines().filter(|l| !l.trim().is_empty()).count());
        let count_suffix = format!(
            " ({} {})",
            count,
            if count == 1 { "match" } else { "matches" }
        );
        let clean_target = target
            .strip_prefix("Glob ")
            .or_else(|| target.strip_prefix("Grep "))
            .unwrap_or(&target);
        if clean_target.is_empty() {
            format!("{label}{count_suffix}")
        } else {
            format!("{label} {clean_target}{count_suffix}")
        }
    } else if matches!(row.name.as_str(), "bash" | "sh") {
        let clean = target.strip_prefix("$ ").unwrap_or(&target).trim();
        if clean.is_empty() {
            "bash".to_string()
        } else {
            clean.to_string()
        }
    } else if target.is_empty() {
        if label.is_empty() {
            row.name.clone()
        } else {
            format!("{label} {}", row.name)
        }
    } else if label.is_empty() {
        target
    } else {
        format!("{label} {target}")
    }
}

fn text_cell_width(text: &str) -> usize {
    Span::raw(text).width()
}

fn truncate_tool_text(text: &str, max_width: usize) -> String {
    if text_cell_width(text) <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let ellipsis_width = text_cell_width(ellipsis);
    let mut result = String::new();
    let mut width = 0;
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        let ch_width = Span::raw(&*ch.encode_utf8(&mut buf)).width();
        if width + ch_width + ellipsis_width > max_width {
            break;
        }
        result.push(ch);
        width += ch_width;
    }
    result.push_str(ellipsis);
    result
}
fn format_inline_code(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    if let Some(open) = text.find('[')
        && let Some(label_end) = text[open + 1..].find("](")
    {
        let label_end = open + 1 + label_end;
        if let Some(url_end) = text[label_end + 2..].find(')') {
            let url_end = label_end + 2 + url_end;
            let mut spans = format_inline_code(&text[..open], theme);
            spans.push(Span::styled(
                text[open + 1..label_end].to_string(),
                Style::default()
                    .fg(theme.teal)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            spans.extend(format_inline_code(&text[url_end + 1..], theme));
            return spans;
        }
    }
    if !text.contains('`') {
        return vec![Span::styled(
            text.to_string(),
            Style::default().fg(theme.ink),
        )];
    }

    let mut spans = Vec::new();
    let mut remainder = text;

    while let Some(start_idx) = remainder.find('`') {
        let before = &remainder[..start_idx];
        if let Some(end_idx) = remainder[start_idx + 1..].find('`') {
            if !before.is_empty() {
                spans.push(Span::styled(
                    before.to_string(),
                    Style::default().fg(theme.ink),
                ));
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
        spans.push(Span::styled(
            remainder.to_string(),
            Style::default().fg(theme.ink),
        ));
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), Style::default().fg(theme.ink)));
    }

    spans
}

pub fn format_transcript_lines(
    transcript: &str,
    theme: &Theme,
    mode_color: Color,
) -> Vec<Line<'static>> {
    format_transcript_lines_with_width(transcript, theme, mode_color, None)
}

pub fn format_transcript_lines_with_width(
    transcript: &str,
    theme: &Theme,
    mode_color: Color,
    width: Option<u16>,
) -> Vec<Line<'static>> {
    let raw_lines: Vec<&str> = transcript.lines().collect();
    let transcript_width = raw_lines
        .iter()
        .map(|l| Span::raw(*l).width())
        .max()
        .unwrap_or(0);
    let card_width = width
        .map(|w| (w as usize).saturating_sub(2).max(20))
        .unwrap_or_else(|| transcript_width.max(20));
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(raw_lines.len());
    let mut in_code_block = false;
    let mut compaction_marker_seen = false;
    let mut orphan_fence = false;
    for (i, line) in raw_lines.iter().enumerate() {
        if *line == "[earlier transcript compacted]" {
            compaction_marker_seen = true;
            orphan_fence = false;
            in_code_block = false;
            lines.push(Line::from(Span::styled(
                (*line).to_string(),
                Style::default().fg(theme.dim),
            )));
            continue;
        }
        if line.trim_start().starts_with("```") {
            if in_code_block {
                in_code_block = false;
                orphan_fence = false;
                lines.push(Line::from(""));
            } else {
                in_code_block = true;
                let lang = line.trim_start().trim_start_matches('`').trim();
                orphan_fence = compaction_marker_seen
                    && lang.is_empty()
                    && i > 0
                    && !raw_lines[i - 1].trim().is_empty();
                if !lang.is_empty() {
                    let header_line = Line::from(vec![
                        Span::raw("  "),
                        Span::styled(lang.to_string(), Style::default().fg(theme.dim)),
                    ]);
                    lines.push(pad_card_line(header_line, card_width, theme.bg_element));
                }
            }
            continue;
        }

        if orphan_fence
            && line.strip_prefix("> ").is_some()
            && (i == 0
                || raw_lines[i - 1].trim().is_empty()
                || raw_lines[i - 1].trim_start().starts_with("```")
                || raw_lines[i - 1] == "[earlier transcript compacted]")
        {
            in_code_block = false;
            orphan_fence = false;
        }
        if in_code_block {
            let code_line = Line::from(vec![
                Span::raw("  "),
                Span::styled(line.to_string(), Style::default().fg(theme.ink)),
            ]);
            lines.push(pad_card_line(code_line, card_width, theme.bg_element));
            continue;
        }
        if line.trim().is_empty() {
            let is_last_empty = lines
                .last()
                .map(|l| {
                    l.spans.is_empty() || (l.spans.len() == 1 && l.spans[0].content.is_empty())
                })
                .unwrap_or(false);
            if !lines.is_empty() && !is_last_empty {
                lines.push(Line::from(""));
            }
            continue;
        }
        if let Some(prompt) = line.strip_prefix("> ") {
            let content_line = Line::from(vec![
                Span::styled(
                    "▌ ",
                    Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    prompt.to_string(),
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                ),
            ]);
            lines.push(pad_card_line(content_line, card_width, theme.bg_element));
            lines.push(Line::from(""));
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("+ Thought for ")
            .or_else(|| line.trim_start().strip_prefix("+ Thought for "))
        {
            let spans = vec![Span::styled(
                format!("+ Thought for {rest}"),
                Style::default().fg(theme.amber),
            )];
            lines.push(Line::from(spans));
        } else if let Some(rest) = line
            .strip_prefix("- Thought for ")
            .or_else(|| line.trim_start().strip_prefix("- Thought for "))
        {
            let spans = vec![Span::styled(
                format!("- Thought for {rest}"),
                Style::default().fg(theme.amber),
            )];
            lines.push(Line::from(spans));
        } else if let Some(rest) = line
            .strip_prefix("Thought for ")
            .or_else(|| line.strip_prefix("💭 Thought for "))
            .or_else(|| line.trim_start().strip_prefix("Thought for "))
            .or_else(|| line.trim_start().strip_prefix("💭 Thought for "))
        {
            let spans = vec![Span::styled(
                format!("Thought for {rest}"),
                Style::default().fg(theme.amber),
            )];
            lines.push(Line::from(spans));
        } else if let Some(rest) = line.strip_prefix("💭 ") {
            let spans = vec![Span::styled(
                rest.to_string(),
                Style::default().fg(theme.amber),
            )];
            lines.push(Line::from(spans));
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
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            )));
        } else if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            let mut spans = vec![Span::styled("• ", Style::default().fg(theme.amber))];
            spans.extend(format_inline_code(rest, theme));
            lines.push(Line::from(spans));
        } else if (line.starts_with(' ') || line.starts_with('\t'))
            && line.trim_start().starts_with("* ")
        {
            let trimmed = line.trim_start();
            let indent_len = line.len().saturating_sub(trimmed.len());
            let indent = &line[..indent_len];
            let rest = trimmed.strip_prefix("* ").unwrap_or("");
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
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content, "▌ ");
        assert_eq!(lines[0].spans[1].content, "Hello Clawcode");
        assert_eq!(lines[0].style.bg, Some(theme.bg_element));
        assert!(lines[1].spans.is_empty() || lines[1].spans[0].content.is_empty());
    }

    #[test]
    fn test_format_transcript_user_prompt_width_padding() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;
        let lines =
            format_transcript_lines_with_width("> Hello Clawcode", &theme, mode_color, Some(80));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].width(), 78);
        assert_eq!(lines[0].style.bg, Some(theme.bg_element));
    }

    #[test]
    fn test_pad_card_line_and_empty_card_line() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let empty = empty_card_line(50, theme.bg_element);
        assert_eq!(empty.width(), 50);
        assert_eq!(empty.style.bg, Some(theme.bg_element));

        let line = Line::from("test");
        let padded = pad_card_line(line, 50, theme.bg_element);
        assert_eq!(padded.width(), 50);
        assert_eq!(padded.style.bg, Some(theme.bg_element));
    }

    #[test]
    fn test_shell_card_width_padding() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mut lines = Vec::new();
        let mut tool_lines = Vec::new();
        let row = ToolRow {
            call_id: "test-call".to_string(),
            name: "bash".to_string(),
            arguments: "echo hi".to_string(),
            desc: "echo hi".to_string(),
            output: "hi\nthere".to_string(),
            state: ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        };
        let app = App::default();
        render_shell_card(
            &mut lines,
            &row,
            &app,
            &theme,
            Color::Cyan,
            60,
            &mut tool_lines,
        );
        let card_width = 58;
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0].width(), card_width);
        assert_eq!(lines[0].style.bg, Some(theme.bg_element));
        assert_eq!(lines[1].width(), card_width);
        assert_eq!(lines[1].style.bg, Some(theme.bg_element));
        assert_eq!(lines[2].width(), card_width);
        assert_eq!(lines[2].style.bg, Some(theme.bg_element));
        assert_eq!(lines[3].width(), card_width);
        assert_eq!(lines[3].style.bg, Some(theme.bg_element));
        assert_eq!(lines[4].width(), card_width);
        assert_eq!(lines[4].style.bg, Some(theme.bg_element));
        assert_eq!(lines[5].width(), 0);
        assert_eq!(lines[5].style.bg, None);
    }
    #[test]
    fn test_shell_card_expansion_hint_grammar() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let app = App::default();
        let mut tool_lines = Vec::new();

        // Exactly 11 lines (10 preview + 1 remaining) -> "(1 more line)"
        let mut lines1 = Vec::new();
        let row1 = ToolRow {
            call_id: "test-1".to_string(),
            name: "bash".to_string(),
            arguments: "echo test".to_string(),
            desc: "echo test".to_string(),
            output: (1..=11)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n"),
            state: ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: true,
        };
        render_shell_card(
            &mut lines1,
            &row1,
            &app,
            &theme,
            Color::Cyan,
            60,
            &mut tool_lines,
        );
        let hint_line1 = lines1.iter().find(|l| {
            l.spans
                .iter()
                .any(|s| s.content.contains("click to expand"))
        });
        assert!(hint_line1.is_some());
        assert!(
            hint_line1
                .unwrap()
                .spans
                .iter()
                .any(|s| s.content.contains("(1 more line)"))
        );

        // 12 lines (10 preview + 2 remaining) -> "(2 more lines)"
        let mut lines2 = Vec::new();
        let row2 = ToolRow {
            call_id: "test-2".to_string(),
            name: "bash".to_string(),
            arguments: "echo test".to_string(),
            desc: "echo test".to_string(),
            output: (1..=12)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n"),
            state: ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: true,
        };
        render_shell_card(
            &mut lines2,
            &row2,
            &app,
            &theme,
            Color::Cyan,
            60,
            &mut tool_lines,
        );
        let hint_line2 = lines2.iter().find(|l| {
            l.spans
                .iter()
                .any(|s| s.content.contains("click to expand"))
        });
        assert!(hint_line2.is_some());
        assert!(
            hint_line2
                .unwrap()
                .spans
                .iter()
                .any(|s| s.content.contains("(2 more lines)"))
        );
    }

    #[test]
    fn test_diff_card_width_padding() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mut lines = Vec::new();
        let mut tool_lines = Vec::new();
        let row = ToolRow {
            call_id: "diff-call".to_string(),
            name: "edit".to_string(),
            arguments: serde_json::json!({
                "path": "src/main.rs",
                "old_string": "line1\nline2",
                "new_string": "line1\nline_new",
            })
            .to_string(),
            desc: "edit src/main.rs".to_string(),
            output: "ok".to_string(),
            state: ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        };
        let app = App::default();
        render_diff_card(
            &mut lines,
            &row,
            &app,
            &theme,
            Color::Cyan,
            70,
            &mut tool_lines,
        );
        let card_width = 68;
        for line in &lines[..lines.len() - 1] {
            assert_eq!(line.width(), card_width);
            assert_eq!(line.style.bg, Some(theme.bg_element));
        }
        assert_eq!(lines.last().unwrap().width(), 0);
        assert_eq!(lines.last().unwrap().style.bg, None);
    }

    #[test]
    fn test_format_transcript_plain_text() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;
        let lines =
            format_transcript_lines("Plain response text from assistant", &theme, mode_color);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].spans[0].content,
            "Plain response text from assistant"
        );
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.ink));
    }

    #[test]
    fn test_format_transcript_markdown_code_block() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let transcript = "```rust\nfn main() {}\n```";
        let lines = format_transcript_lines(transcript, &theme, mode_color);
        assert_eq!(lines.len(), 3);

        // Header
        assert_eq!(lines[0].spans[0].content, "  ");
        assert_eq!(lines[0].spans[1].content, "rust");
        assert_eq!(lines[0].spans[1].style.fg, Some(theme.dim));
        assert_eq!(lines[0].style.bg, Some(theme.bg_element));

        // Content
        assert_eq!(lines[1].spans[0].content, "  ");
        assert_eq!(lines[1].spans[1].content, "fn main() {}");
        assert_eq!(lines[1].spans[1].style.fg, Some(theme.ink));
        assert_eq!(lines[1].style.bg, Some(theme.bg_element));

        // Footer
        assert!(lines[2].spans.is_empty() || lines[2].spans[0].content.is_empty());
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
    fn test_append_stream_parts_user_and_tools() {
        let mut app = App::default();
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;
        app.set_tool_rows_for_test(vec![ToolRow {
            call_id: "tool-1".to_string(),
            name: "bash".to_string(),
            desc: "ls -la".to_string(),
            arguments: serde_json::json!({ "command": "ls -la" }).to_string(),
            output: "total 0\n-rw-r--r-- 1 user 0 test.txt".to_string(),
            state: ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        }]);
        let parts = vec![
            StreamPart::User("Check directory".to_string()),
            StreamPart::Tool("tool-1".to_string()),
            StreamPart::Text("Directory checked.".to_string()),
        ];
        let mut lines = Vec::new();
        let mut tool_lines = Vec::new();
        append_stream_parts(
            &mut lines,
            &parts,
            &app,
            &theme,
            mode_color,
            80,
            &mut tool_lines,
        );
        assert!(
            lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content == "▌ "))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content == "Check directory"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content.contains("ls -la")))
        );
        assert!(lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.content.contains("Directory checked."))
        }));
    }
    #[test]
    fn truncate_tool_text_respects_terminal_cells() {
        let value = truncate_tool_text("界界界abc", 5);
        assert!(text_cell_width(&value) <= 5);
        assert!(value.ends_with('…'));
    }

    #[test]
    fn test_strip_ansi_codes() {
        assert_eq!(strip_ansi_codes("\x1b[34;40m.agents\x1b[0m"), ".agents");
        assert_eq!(strip_ansi_codes("plain text"), "plain text");
        assert_eq!(
            strip_ansi_codes("\x1b[1;32mhello\x1b[0m \x1b[31mworld\x1b[m"),
            "hello world"
        );
    }

    #[test]
    fn test_render_shell_card_strips_ansi() {
        let mut lines = Vec::new();
        let row = ToolRow {
            call_id: "test-shell".to_string(),
            name: "bash".to_string(),
            state: ToolRowState::Completed,
            desc: String::new(),
            arguments: serde_json::json!({ "command": "ls" }).to_string(),
            output: "\x1b[34;40m.agents\x1b[0m\nfile.txt".to_string(),
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        };
        let app = App::default();
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mut tool_lines = Vec::new();
        render_shell_card(
            &mut lines,
            &row,
            &app,
            &theme,
            Color::Cyan,
            60,
            &mut tool_lines,
        );
        let output_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains(".agents")));
        assert!(output_line.is_some());
        let span = output_line
            .unwrap()
            .spans
            .iter()
            .find(|s| s.content.contains(".agents"))
            .unwrap();
        assert_eq!(span.content, ".agents");
        assert!(!span.content.contains("\x1b"));
    }

    #[test]
    fn test_format_transcript_lines_thought_variants() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let lines_collapsed = format_transcript_lines("+ Thought for 1.2s", &theme, mode_color);
        assert_eq!(lines_collapsed.len(), 1);
        assert_eq!(lines_collapsed[0].spans[0].content, "+ Thought for 1.2s");
        assert_eq!(lines_collapsed[0].spans[0].style.fg, Some(theme.amber));

        let lines_expanded = format_transcript_lines("- Thought for 1.2s", &theme, mode_color);
        assert_eq!(lines_expanded.len(), 1);
        assert_eq!(lines_expanded[0].spans[0].content, "- Thought for 1.2s");
        assert_eq!(lines_expanded[0].spans[0].style.fg, Some(theme.amber));
    }

    #[test]
    fn test_append_stream_parts_thought_expanded_and_collapsed() {
        let mut app = App::default();
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;
        let parts = vec![StreamPart::Reasoning("Line one\nLine two".to_string())];

        // Collapsed by default
        let mut lines = Vec::new();
        let mut tool_lines = Vec::new();
        append_stream_parts(
            &mut lines,
            &parts,
            &app,
            &theme,
            mode_color,
            80,
            &mut tool_lines,
        );
        assert!(tool_lines.iter().any(|(id, _)| id == "__thought__"));
        let thought_header = lines.iter().find(|l| {
            l.spans
                .iter()
                .any(|s| s.content.starts_with("+ Thought for "))
        });
        assert!(thought_header.is_some());
        assert!(
            !lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content.contains("Line one")))
        );

        // Toggle to expanded
        app.toggle_thought_expanded();
        assert!(app.is_thought_expanded());
        let mut lines_exp = Vec::new();
        let mut tool_lines_exp = Vec::new();
        append_stream_parts(
            &mut lines_exp,
            &parts,
            &app,
            &theme,
            mode_color,
            80,
            &mut tool_lines_exp,
        );
        let expanded_header = lines_exp.iter().find(|l| {
            l.spans
                .iter()
                .any(|s| s.content.starts_with("- Thought for "))
        });
        assert!(expanded_header.is_some());
        assert!(
            lines_exp
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content == "Line one"))
        );
        assert!(
            lines_exp
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content == "Line two"))
        );
        assert!(
            lines_exp
                .iter()
                .any(|l| l.spans.iter().any(|s| s.content == "      │ "))
        );
    }

    #[test]
    fn test_compact_visual_bug_reproduction() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;

        let tail = "git status\n```\n> cioba shell lain nya\n\n$ git status -s\n  M README.md\n\n> tes lagi shell lain";
        let compacted = format!("[earlier transcript compacted]\n{tail}");
        let lines = format_transcript_lines_with_width(&compacted, &theme, mode_color, Some(80));

        let has_prompt_card = lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content == "▌ "));
        let has_literal_prompt = lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.content.contains("> cioba shell lain nya"))
        });
        let marker = &lines[0];

        assert!(
            has_prompt_card,
            "Prompt must render as card after compaction"
        );
        assert!(
            !has_literal_prompt,
            "Prompt must not render inside code card"
        );
        assert_eq!(marker.spans[0].style.fg, Some(theme.dim));
    }

    #[test]
    fn test_code_block_prompt_like_line_stays_code() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let lines = format_transcript_lines("```rust\n> literal\n```", &theme, Color::Cyan);

        assert!(
            lines
                .iter()
                .any(|line| { line.spans.iter().any(|span| span.content == "> literal") })
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.spans.iter().any(|span| span.content == "▌ "))
        );
    }
}
