use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::App;
use super::app::{
    StreamPart, ToolRow, ToolRowState, canonical_tool_name, exploration_category,
    tool_completed_icon, tool_headline, tool_is_background, tool_pending_label,
    tool_uses_generic_detail,
};
use super::render::{
    identity_label, input_card_height, mode_label, render_command_popup, render_hints_row,
    render_input_card, render_pending_prompts_strip, status_label,
};
use super::theme::Theme;

pub fn render_chat(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, mode_color: Color) {
    app.set_last_quick_actions_area(None);
    if area.width == 0 || area.height == 0 {
        return;
    }
    let has_gap = area.height >= 16 || !app.pending_prompts().is_empty();
    let input_height = input_card_height(app, area.width).min(area.height.saturating_sub(9).max(5));
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
            Constraint::Length(input_height),
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
        let mut ctx = ChatRenderContext {
            lines: &mut ordered,
            app,
            theme,
            mode_color,
            width: chunks[1].width,
            tool_row_lines: &mut tool_row_lines,
        };
        append_stream_parts(&mut ctx, app.stream_parts());
        ordered
    } else {
        let mut ordered = format_transcript_lines_with_mode(
            app.transcript(),
            theme,
            mode_color,
            Some(chunks[1].width),
            app.tui_settings().markdown,
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
            if app.tui_settings().tps {
                let dur_secs = metrics.duration.as_secs_f64();
                let tps = if dur_secs > 0.05 {
                    format!("{:.0}t/s", (usage.output_tokens as f64) / dur_secs)
                } else {
                    format!("{}t", usage.output_tokens)
                };
                meta_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
                meta_spans.push(Span::styled(tps, Style::default().fg(theme.dim)));
            }
            match app.tui_settings().turn_tokens {
                crate::config::TurnTokensMode::Off => {}
                crate::config::TurnTokensMode::Compact => {
                    let total = usage.input_tokens.saturating_add(usage.output_tokens);
                    meta_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
                    meta_spans.push(Span::styled(
                        format!("{total}tok"),
                        Style::default().fg(theme.dim),
                    ));
                }
                crate::config::TurnTokensMode::Verbose => {
                    meta_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
                    meta_spans.push(Span::styled(
                        format!("in:{} out:{}", usage.input_tokens, usage.output_tokens),
                        Style::default().fg(theme.dim),
                    ));
                }
            }
        }

        let dur_str = if metrics.duration.as_secs() > 0 {
            format!("{:.1}s", metrics.duration.as_secs_f64())
        } else {
            format!("{}ms", metrics.duration.as_millis())
        };
        meta_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
        meta_spans.push(Span::styled(dur_str, Style::default().fg(theme.dim)));

        if matches!(
            app.conversation_status(),
            super::ConversationStatus::Cancelled
        ) {
            meta_spans.push(Span::styled(" · ", Style::default().fg(theme.dim)));
            meta_spans.push(Span::styled(
                "interrupted",
                Style::default().fg(theme.warning),
            ));
        }

        lines.push(Line::from(String::new()));
        lines.push(Line::from(meta_spans));
    } else if matches!(
        app.conversation_status(),
        super::ConversationStatus::Cancelled
    ) && !lines.is_empty()
    {
        let model = if !app.selected_model().is_empty() {
            app.selected_model()
        } else {
            "default"
        };
        lines.push(Line::from(String::new()));
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(
                "▣ ",
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                mode_label(app),
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" · {model} · "), Style::default().fg(theme.dim)),
            Span::styled("interrupted", Style::default().fg(theme.warning)),
        ]));
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

    if app.tui_settings().scrollbar && total_visual_lines > visible_height && visible_height > 0 {
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

    app.set_jump_to_latest_area(None);
    if scroll_offset > 0 && transcript_area.width >= 26 && transcript_area.height >= 2 {
        let badge_text = if transcript_area.width >= 60 {
            format!(" Jump to latest ↓ · {scroll_offset} lines up (End to bottom) ")
        } else {
            format!(" Jump to latest ↓ ({scroll_offset}) ")
        };
        let badge_width = UnicodeWidthStr::width(badge_text.as_str()) as u16;
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
            app.set_jump_to_latest_area(Some(badge_area));
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

    if !app.pending_prompts().is_empty() {
        render_pending_prompts_strip(frame, chunks[2], app, theme, mode_color);
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

struct ChatRenderContext<'a> {
    lines: &'a mut Vec<Line<'static>>,
    app: &'a App,
    theme: &'a Theme,
    mode_color: Color,
    width: u16,
    tool_row_lines: &'a mut Vec<(String, usize)>,
}

/// OpenCode derives a compact reasoning title from a leading `**bold**`
/// paragraph. The title is only recognised when it ends at a paragraph
/// boundary, so ordinary bold text inside the reasoning is not mistaken for
/// a title.
fn reasoning_title(text: &str) -> Option<&str> {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("**")?;
    let end = rest.find("**")?;
    let title = rest[..end].trim();
    let suffix = &rest[end + 2..];
    if title.is_empty() || title.contains(['\n', '\r']) {
        return None;
    }
    if !suffix.is_empty() && !suffix.starts_with("\n\n") && !suffix.starts_with("\r\n\r\n") {
        return None;
    }
    Some(title)
}

fn reasoning_label(text: &str, done: bool) -> String {
    let base = if done { "Thought" } else { "Thinking" };
    match reasoning_title(text) {
        Some(title) => format!("{base}: {}", truncate_tool_text(title, 60)),
        None => base.to_string(),
    }
}

fn append_reasoning_body_lines(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    width: u16,
    theme: &Theme,
) {
    let body_width = (width as usize).saturating_sub(10).max(20);
    for raw_line in text.trim().lines() {
        if raw_line.trim().is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "      │ ",
                Style::default().fg(theme.dim),
            )]));
        } else {
            for wrapped in wrap_line_to_width(raw_line, body_width) {
                lines.push(Line::from(vec![
                    Span::styled("      │ ", Style::default().fg(theme.dim)),
                    Span::styled(wrapped, Style::default().fg(theme.quiet)),
                ]));
            }
        }
    }
}

fn parse_code_fence(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start();
    let fence_char = trimmed.chars().next()?;
    if fence_char != '`' && fence_char != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == fence_char).count();
    if count < 3 {
        return None;
    }
    let info = trimmed[count * fence_char.len_utf8()..].trim();
    if fence_char == '`' && info.contains('`') {
        return None;
    }
    Some((fence_char, count, info))
}

fn is_nested_markdown_lang(lang: &str) -> bool {
    let token = lang
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        token.as_str(),
        "markdown" | "md" | "mdx" | "rmd" | "quarto" | "qmd"
    )
}

fn has_later_closing_fence(
    raw_lines: &[&str],
    start_idx: usize,
    fence_char: char,
    min_count: usize,
) -> bool {
    for candidate in &raw_lines[start_idx..] {
        if candidate.starts_with("> ") || *candidate == "[earlier transcript compacted]" {
            break;
        }
        if let Some((c, count, info)) = parse_code_fence(candidate)
            && c == fence_char
            && count >= min_count
            && info.is_empty()
        {
            return true;
        }
    }
    false
}

fn tool_row_is_terminal(row: &ToolRow) -> bool {
    matches!(row.state, ToolRowState::Completed | ToolRowState::Failed)
}

fn exploration_run(app: &App, parts: &[StreamPart], start: usize) -> Option<(Vec<String>, bool)> {
    let mut ids = Vec::new();
    for part in &parts[start..] {
        let StreamPart::Tool(call_id) = part else {
            break;
        };
        let Some(row) = app.tool_rows().iter().find(|row| row.call_id == *call_id) else {
            break;
        };
        if exploration_category(&row.name).is_none() {
            break;
        }
        ids.push(call_id.clone());
    }
    if ids.len() < 2 {
        return None;
    }
    let complete = ids.iter().all(|call_id| {
        app.tool_rows()
            .iter()
            .find(|row| row.call_id == *call_id)
            .is_some_and(tool_row_is_terminal)
    });
    Some((ids, complete))
}

fn exploration_group_label(app: &App, ids: &[String], complete: bool) -> String {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for call_id in ids {
        let Some(row) = app.tool_rows().iter().find(|row| row.call_id == *call_id) else {
            continue;
        };
        let Some(category) = exploration_category(&row.name) else {
            continue;
        };
        if let Some((_, count)) = counts.iter_mut().find(|(name, _)| *name == category) {
            *count += 1;
        } else {
            counts.push((category, 1));
        }
    }
    let parts = counts
        .into_iter()
        .map(|(category, count)| {
            let noun = match category {
                "search" if count != 1 => "searches",
                "fetch" if count != 1 => "fetches",
                _ if count == 1 => category,
                _ => match category {
                    "read" => "reads",
                    "search" => "searches",
                    "fetch" => "fetches",
                    _ => "items",
                },
            };
            format!("{count} {noun}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{}: {parts}",
        if complete { "Explored" } else { "Exploring" }
    )
}

fn render_exploration_group(ctx: &mut ChatRenderContext<'_>, ids: &[String], complete: bool) {
    let Some(first) = ids.first() else {
        return;
    };
    let group_id = format!("group:{first}");
    let expanded = ctx.app.is_tool_expanded(&group_id);
    let label = exploration_group_label(ctx.app, ids, complete);
    let marker = if complete {
        "→ ".to_string()
    } else {
        format!("{} ", ctx.app.compact_spinner_frame())
    };
    let line_index = ctx.lines.len();
    ctx.lines.push(Line::from(vec![
        Span::raw("   "),
        Span::styled(
            marker,
            Style::default()
                .fg(if complete {
                    ctx.theme.quiet
                } else {
                    ctx.theme.amber
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            label,
            Style::default().fg(if complete {
                ctx.theme.quiet
            } else {
                ctx.theme.ink
            }),
        ),
    ]));
    ctx.tool_row_lines.push((group_id, line_index));
    if expanded {
        for call_id in ids {
            if let Some(row) = ctx
                .app
                .tool_rows()
                .iter()
                .find(|row| row.call_id == *call_id)
            {
                render_tool_card_or_row(ctx, row);
            }
        }
    }
}

fn activity_run(app: &App, parts: &[StreamPart], start: usize) -> Option<(usize, String, bool)> {
    let mut len = 0usize;
    let mut tool_count = 0usize;
    let mut thought_count = 0usize;
    let mut active = false;
    for part in &parts[start..] {
        match part {
            StreamPart::Tool(call_id) => {
                let Some(row) = app.tool_rows().iter().find(|row| row.call_id == *call_id) else {
                    break;
                };
                if row.state == ToolRowState::Failed {
                    break;
                }
                if !tool_row_is_terminal(row) {
                    active = true;
                }
                tool_count += 1;
                len += 1;
            }
            StreamPart::Reasoning(_) => {
                active = true;
                thought_count += 1;
                len += 1;
            }
            StreamPart::ReasoningDone { .. } => {
                thought_count += 1;
                len += 1;
            }
            StreamPart::User(_) | StreamPart::Text(_) => break,
        }
    }
    if len == 0 {
        return None;
    }
    let mut segments = Vec::new();
    if tool_count > 0 {
        segments.push(format!(
            "{tool_count} {}",
            if tool_count == 1 { "tool" } else { "tools" }
        ));
    }
    if thought_count > 0 {
        segments.push(format!(
            "{thought_count} {}",
            if thought_count == 1 {
                "thought"
            } else {
                "thoughts"
            }
        ));
    }
    Some((len, format!("Activity: {}", segments.join(", ")), !active))
}

fn append_stream_parts(ctx: &mut ChatRenderContext<'_>, parts: &[StreamPart]) {
    append_stream_parts_inner(ctx, parts, true);
}

fn append_stream_parts_inner(
    ctx: &mut ChatRenderContext<'_>,
    parts: &[StreamPart],
    allow_activity_fold: bool,
) {
    let mut index = 0;
    while index < parts.len() {
        if allow_activity_fold
            && ctx.app.tui_settings().verbosity == crate::config::TranscriptVerbosity::Low
            && let Some((len, label, complete)) = activity_run(ctx.app, parts, index)
        {
            let activity_id = format!("activity:{index}");
            let expanded = ctx.app.is_tool_expanded(&activity_id);
            let marker = if !complete {
                format!("{} ", ctx.app.compact_spinner_frame())
            } else if expanded {
                "- ".to_string()
            } else {
                "+ ".to_string()
            };
            let line_index = ctx.lines.len();
            ctx.lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    format!("{marker}{label}"),
                    Style::default().fg(if complete {
                        ctx.theme.quiet
                    } else {
                        ctx.theme.amber
                    }),
                ),
            ]));
            ctx.tool_row_lines.push((activity_id, line_index));
            if expanded {
                append_stream_parts_inner(ctx, &parts[index..index + len], false);
            }
            ctx.lines.push(Line::from(""));
            index += len;
            continue;
        }
        let part = &parts[index];
        match part {
            StreamPart::User(prompt) => {
                if !ctx.lines.is_empty() {
                    ctx.lines.push(Line::from(""));
                }
                let card_width = if ctx.width > 2 {
                    (ctx.width.saturating_sub(2) as usize).max(20)
                } else {
                    ctx.width as usize
                };
                let p_lines: Vec<&str> = prompt.lines().collect();
                if p_lines.is_empty() {
                    let content_line = Line::from(vec![
                        Span::styled(
                            "▌ ",
                            Style::default()
                                .fg(ctx.mode_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            String::new(),
                            Style::default()
                                .fg(ctx.theme.ink)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]);
                    ctx.lines.push(pad_card_line(
                        content_line,
                        card_width,
                        ctx.theme.bg_element,
                    ));
                } else {
                    let content_width = card_width.saturating_sub(2);
                    let mut is_first = true;
                    for pline in &p_lines {
                        let wrapped = wrap_line_to_width(pline, content_width);
                        for wline in wrapped {
                            let prefix = if is_first { "▌ " } else { "  " };
                            is_first = false;
                            let content_line = Line::from(vec![
                                Span::styled(
                                    prefix,
                                    Style::default()
                                        .fg(ctx.mode_color)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    wline,
                                    Style::default()
                                        .fg(ctx.theme.ink)
                                        .add_modifier(Modifier::BOLD),
                                ),
                            ]);
                            ctx.lines.push(pad_card_line(
                                content_line,
                                card_width,
                                ctx.theme.bg_element,
                            ));
                        }
                    }
                }
                ctx.lines.push(Line::from(""));
            }
            StreamPart::Text(text) => {
                if !ctx.lines.is_empty()
                    && !ctx
                        .lines
                        .last()
                        .map(|l| {
                            l.spans.is_empty()
                                || (l.spans.len() == 1 && l.spans[0].content.is_empty())
                        })
                        .unwrap_or(false)
                {
                    ctx.lines.push(Line::from(""));
                }
                let mut rendered = format_transcript_lines_with_mode(
                    text,
                    ctx.theme,
                    ctx.mode_color,
                    Some(ctx.width),
                    ctx.app.tui_settings().markdown,
                );
                for line in &mut rendered {
                    indent_assistant_line(line);
                }
                let has_rendered = !rendered.is_empty();
                ctx.lines.extend(rendered);
                if has_rendered {
                    ctx.lines.push(Line::from(""));
                }
            }
            StreamPart::Reasoning(text) => {
                ctx.lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        format!(
                            "{} {}",
                            ctx.app.compact_spinner_frame(),
                            reasoning_label(text, false)
                        ),
                        Style::default()
                            .fg(ctx.theme.amber)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                if ctx.app.is_thought_expanded() {
                    append_reasoning_body_lines(ctx.lines, text, ctx.width, ctx.theme);
                }
            }
            StreamPart::ReasoningDone { text, duration } => {
                let dur_str = format!("{:.1}s", duration.as_secs_f64().max(0.1));
                let line_count = ctx.lines.len();
                ctx.tool_row_lines
                    .push(("__thought__".to_string(), line_count));
                let label = reasoning_label(text, true);
                if ctx.app.is_thought_expanded() {
                    ctx.lines.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled(
                            format!("- {label} · {dur_str}"),
                            Style::default().fg(ctx.theme.amber),
                        ),
                    ]));
                    append_reasoning_body_lines(ctx.lines, text, ctx.width, ctx.theme);
                } else {
                    ctx.lines.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled(
                            format!("+ {label} · {dur_str}"),
                            Style::default().fg(ctx.theme.amber),
                        ),
                    ]));
                }
                ctx.lines.push(Line::from(""));
            }
            StreamPart::Tool(call_id) => {
                if ctx.app.tui_settings().grouping == crate::config::ToolGrouping::Auto
                    && let Some((ids, complete)) = exploration_run(ctx.app, parts, index)
                {
                    let len = ids.len();
                    render_exploration_group(ctx, &ids, complete);
                    index += len;
                    continue;
                }
                let app = ctx.app;
                let Some(row) = app.tool_rows().iter().find(|row| row.call_id == *call_id) else {
                    index += 1;
                    continue;
                };
                render_tool_card_or_row(ctx, row);
            }
        }
        index += 1;
    }
}

fn shell_command(row: &ToolRow) -> String {
    if row.desc == "preparing arguments..." {
        return tool_pending_label(&row.name).to_string();
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

/// Pre-wrap a text line into chunks that each fit within `max_width` display columns.
/// Breaks on word boundaries when possible, hard-breaks otherwise.
fn wrap_line_to_width(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;
    for word in WordSplitter::new(text) {
        let w = UnicodeWidthStr::width(word.as_str());
        if current_width + w <= max_width {
            current.push_str(&word);
            current_width += w;
        } else if w > max_width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            let trimmed = word.strip_prefix(' ').unwrap_or(&word);
            for ch in trimmed.chars() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if current_width + cw > max_width && !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                current.push(ch);
                current_width += cw;
            }
        } else {
            lines.push(std::mem::take(&mut current));
            let trimmed = word.strip_prefix(' ').unwrap_or(&word);
            current.push_str(trimmed);
            current_width = UnicodeWidthStr::width(trimmed);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Iterator that yields words with their trailing/leading whitespace preserved.
/// Splits on word boundaries (space transitions) for wrap purposes.
struct WordSplitter<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> WordSplitter<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, pos: 0 }
    }
}

impl<'a> Iterator for WordSplitter<'a> {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        if self.pos >= self.text.len() {
            return None;
        }
        let rest = &self.text[self.pos..];
        let chars: Vec<char> = rest.chars().collect();
        let mut end = 0;
        // Consume leading spaces
        while end < chars.len() && chars[end] == ' ' {
            end += 1;
        }
        // Consume non-space chars
        while end < chars.len() && chars[end] != ' ' {
            end += 1;
        }
        if end == 0 {
            return None;
        }
        let word: String = chars[..end].iter().collect();
        self.pos += word.len();
        Some(word)
    }
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

fn render_shell_card(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    let lines = &mut *ctx.lines;
    let app = ctx.app;
    let theme = ctx.theme;
    let mode_color = ctx.mode_color;
    let width = ctx.width;
    let tool_row_lines = &mut *ctx.tool_row_lines;
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
        let spinner_frame = format!("{} ", app.compact_spinner_frame());
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

        // Foreground shells publish chunks while the process is alive.
        let output = strip_ansi_codes(&row.output);
        let raw_lines: Vec<&str> = output.lines().collect();
        if !raw_lines.is_empty() {
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
                    Span::styled((*line_text).to_string(), Style::default().fg(theme.ink)),
                ]);
                lines.push(pad_card_line(out_line, card_width, theme.bg_element));
                tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            }
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
        }

        if row.state == ToolRowState::Running
            && row.started_at.elapsed() >= std::time::Duration::from_secs(3)
        {
            let bg_hint = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Ctrl+B to run in background",
                    Style::default().fg(theme.dim),
                ),
            ]);
            lines.push(pad_card_line(bg_hint, card_width, theme.bg_element));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
        }

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
            let mut cmd_spans = vec![
                Span::raw("  "),
                Span::styled(
                    "$ ",
                    Style::default()
                        .fg(marker_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(cmd, Style::default().fg(text_color)),
            ];
            if tool_is_background(row) {
                cmd_spans.push(Span::raw(" "));
                cmd_spans.push(Span::styled(
                    " Background ",
                    Style::default()
                        .fg(mode_color)
                        .bg(theme.bg_element)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            let cmd_line = Line::from(cmd_spans);
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
            let mut cmd_spans = vec![
                Span::raw("  "),
                Span::styled(
                    "$ ",
                    Style::default()
                        .fg(marker_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(cmd, Style::default().fg(text_color)),
            ];
            if tool_is_background(row) {
                cmd_spans.push(Span::raw(" "));
                cmd_spans.push(Span::styled(
                    " Background ",
                    Style::default()
                        .fg(mode_color)
                        .bg(theme.bg_element)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            let cmd_line = Line::from(cmd_spans);
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

fn render_diff_card(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    let lines = &mut *ctx.lines;
    let app = ctx.app;
    let theme = ctx.theme;
    let mode_color = ctx.mode_color;
    let width = ctx.width;
    let tool_row_lines = &mut *ctx.tool_row_lines;
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
        let spinner_frame = format!("{} ", app.compact_spinner_frame());
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
                    format!("{} ", tool_completed_icon(&row.name, row.state)),
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
                    format!("{} ", tool_completed_icon(&row.name, row.state)),
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

fn render_tool_card_or_row(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    match canonical_tool_name(&row.name) {
        "bash" => render_shell_card(ctx, row),
        "edit" | "patch" => render_diff_card(ctx, row),
        "write" => render_write_card(ctx, row),
        "update_plan" | "todowrite" => render_todos_card(ctx, row),
        "question" => render_question_card(ctx, row),
        "task" => render_subagent_row(ctx, row),
        "execute" => render_execute_row(ctx, row),
        "read" => {
            render_compact_tool(ctx, row);
            append_loaded_files_detail(ctx, row);
        }
        _ => render_compact_tool(ctx, row),
    }
}

fn card_width(width: u16) -> usize {
    (width as usize).saturating_sub(2).max(20)
}

fn ensure_card_separator(lines: &mut Vec<Line<'static>>) {
    if !lines.is_empty()
        && !lines
            .last()
            .map(|line| {
                line.spans.is_empty() || (line.spans.len() == 1 && line.spans[0].content.is_empty())
            })
            .unwrap_or(false)
    {
        lines.push(Line::from(""));
    }
}

fn push_card_line(
    ctx: &mut ChatRenderContext<'_>,
    row: &ToolRow,
    line: Line<'static>,
    width: usize,
) {
    ctx.lines
        .push(pad_card_line(line, width, ctx.theme.bg_element));
    ctx.tool_row_lines
        .push((row.call_id.clone(), ctx.lines.len() - 1));
}

fn push_card_blank(ctx: &mut ChatRenderContext<'_>, row: &ToolRow, width: usize) {
    push_card_line(
        ctx,
        row,
        empty_card_line(width, ctx.theme.bg_element),
        width,
    );
}

fn tool_content(row: &ToolRow) -> String {
    serde_json::from_str::<serde_json::Value>(&row.arguments)
        .ok()
        .and_then(|args| {
            args.get("content")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_default()
}

fn tool_path(row: &ToolRow) -> String {
    serde_json::from_str::<serde_json::Value>(&row.arguments)
        .ok()
        .and_then(|args| {
            args.get("path")
                .or_else(|| args.get("file_path"))
                .or_else(|| args.get("filePath"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| row.desc.clone())
}

fn render_write_card(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    let content = tool_content(row);
    let content_lines: Vec<&str> = content.lines().collect();
    let width = card_width(ctx.width);
    if row.state == ToolRowState::Completed && !content_lines.is_empty() {
        ensure_card_separator(ctx.lines);
        push_card_blank(ctx, row, width);
        push_card_line(
            ctx,
            row,
            Line::from(Span::styled(
                format!("  # Wrote {}", tool_path(row)),
                Style::default()
                    .fg(ctx.theme.quiet)
                    .add_modifier(Modifier::BOLD),
            )),
            width,
        );
        let expanded = ctx.app.is_tool_expanded(&row.call_id);
        let total = content_lines.len();
        let max_preview = 10;
        let visible = if expanded || total <= max_preview {
            total
        } else {
            max_preview
        };
        for line_text in content_lines.iter().take(visible) {
            push_card_line(
                ctx,
                row,
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled((*line_text).to_string(), Style::default().fg(ctx.theme.ink)),
                ]),
                width,
            );
        }
        if total > max_preview {
            let remaining = total - max_preview;
            let hint = if expanded {
                "↳ click to collapse".to_string()
            } else if remaining == 1 {
                "↳ click to expand (1 more line)".to_string()
            } else {
                format!("↳ click to expand ({remaining} more lines)")
            };
            push_card_line(
                ctx,
                row,
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(hint, Style::default().fg(ctx.theme.dim)),
                ]),
                width,
            );
        }
        push_card_blank(ctx, row, width);
        ctx.lines.push(Line::from(""));
    }
    render_compact_tool(ctx, row);
}

fn render_todos_card(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    if row.state == ToolRowState::Completed
        && let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments)
        && let Some(plan) = args.get("plan").and_then(|value| value.as_array())
        && !plan.is_empty()
    {
        let width = card_width(ctx.width);
        ensure_card_separator(ctx.lines);
        push_card_blank(ctx, row, width);
        push_card_line(
            ctx,
            row,
            Line::from(Span::styled(
                "  # Todos",
                Style::default()
                    .fg(ctx.theme.quiet)
                    .add_modifier(Modifier::BOLD),
            )),
            width,
        );
        for item in plan {
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
            let (icon, color) = match status {
                "completed" | "complete" | "done" => ("✔", ctx.theme.success),
                "in_progress" | "in-progress" | "running" => ("•", ctx.theme.amber),
                _ => ("□", ctx.theme.dim),
            };
            push_card_line(
                ctx,
                row,
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(icon.to_string(), Style::default().fg(color)),
                    Span::raw(" "),
                    Span::styled(step.to_string(), Style::default().fg(ctx.theme.ink)),
                ]),
                width,
            );
        }
        push_card_blank(ctx, row, width);
        ctx.lines.push(Line::from(""));
    }
    render_compact_tool(ctx, row);
}

fn question_items(row: &ToolRow) -> Vec<String> {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments) else {
        return Vec::new();
    };
    if let Some(items) = args.get("questions").and_then(|value| value.as_array()) {
        return items
            .iter()
            .filter_map(|item| {
                item.get("question")
                    .or_else(|| item.get("text"))
                    .and_then(|value| value.as_str())
                    .or_else(|| item.as_str())
                    .map(ToString::to_string)
            })
            .collect();
    }
    args.get("question")
        .and_then(|value| value.as_str())
        .map(|question| vec![question.to_string()])
        .unwrap_or_default()
}

fn question_answers(row: &ToolRow) -> Vec<String> {
    let mut answers = Vec::new();
    if let Some(metadata_answers) = row
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("answers"))
        .and_then(|value| value.as_array())
    {
        for answer in metadata_answers {
            match answer {
                serde_json::Value::Array(values) => answers.push(
                    values
                        .iter()
                        .filter_map(|value| value.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                serde_json::Value::String(value) => answers.push(value.clone()),
                _ => answers.push(answer.to_string()),
            }
        }
    }
    if answers.is_empty()
        && let Some(answer) = row.output.lines().find_map(|line| {
            line.strip_prefix("Answer:")
                .map(str::trim)
                .filter(|answer| !answer.is_empty())
        })
    {
        answers.push(answer.to_string());
    }
    answers
}

fn render_question_card(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    let questions = question_items(row);
    if row.state == ToolRowState::Completed && !questions.is_empty() {
        let answers = question_answers(row);
        let width = card_width(ctx.width);
        ensure_card_separator(ctx.lines);
        push_card_blank(ctx, row, width);
        push_card_line(
            ctx,
            row,
            Line::from(Span::styled(
                "  # Questions",
                Style::default()
                    .fg(ctx.theme.quiet)
                    .add_modifier(Modifier::BOLD),
            )),
            width,
        );
        for (index, question) in questions.iter().enumerate() {
            push_card_line(
                ctx,
                row,
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(question.clone(), Style::default().fg(ctx.theme.quiet)),
                ]),
                width,
            );
            if let Some(answer) = answers.get(index) {
                push_card_line(
                    ctx,
                    row,
                    Line::from(vec![
                        Span::raw("    "),
                        Span::styled(answer.clone(), Style::default().fg(ctx.theme.ink)),
                    ]),
                    width,
                );
            }
        }
        push_card_blank(ctx, row, width);
        ctx.lines.push(Line::from(""));
    }
    render_compact_tool(ctx, row);
}

fn render_subagent_row(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    render_compact_tool(ctx, row);
    let session_id = row
        .metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .get("sessionID")
                .or_else(|| metadata.get("sessionId"))
        })
        .and_then(|value| value.as_str());
    if let Some(session_id) = session_id {
        ctx.lines.push(Line::from(vec![
            Span::raw("      ↳ "),
            Span::styled(
                format!("child session {session_id}"),
                Style::default().fg(ctx.theme.dim),
            ),
        ]));
        ctx.tool_row_lines
            .push((row.call_id.clone(), ctx.lines.len() - 1));
    }
}

fn execute_child_summary(call: &serde_json::Value) -> String {
    if let Some(input) = call.get("input") {
        if let Some(summary) = input.as_str() {
            return summary.to_string();
        }
        if let Some(object) = input.as_object()
            && let Some((key, value)) = object.iter().next()
        {
            let value = match value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            return format!("{key}: {value}");
        }
    }
    String::new()
}

fn render_execute_row(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    render_compact_tool(ctx, row);
    let Ok(args) = serde_json::from_str::<serde_json::Value>(&row.arguments) else {
        return;
    };
    if let Some(calls) = args.get("toolCalls").and_then(|value| value.as_array()) {
        for call in calls.iter().take(16) {
            let Some(tool) = call.get("tool").and_then(|value| value.as_str()) else {
                continue;
            };
            let status = call
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("running");
            let (marker, color) = match status {
                "completed" | "complete" | "success" => ("✓", ctx.theme.success),
                "error" | "failed" => ("✗", ctx.theme.error),
                _ => ("│", ctx.theme.quiet),
            };
            let summary = execute_child_summary(call);
            let mut spans = vec![
                Span::raw("      "),
                Span::styled(format!("{marker} "), Style::default().fg(color)),
                Span::styled(tool.to_string(), Style::default().fg(ctx.theme.ink)),
            ];
            if !summary.is_empty() {
                spans.push(Span::styled(
                    format!("  {summary}"),
                    Style::default().fg(ctx.theme.dim),
                ));
            }
            ctx.lines.push(Line::from(spans));
            ctx.tool_row_lines
                .push((row.call_id.clone(), ctx.lines.len() - 1));
        }
    }
    let failed = row
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("error"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        || row.state == ToolRowState::Failed;
    if failed {
        let output = strip_ansi_codes(&row.output);
        let output_lines: Vec<&str> = output.trim().lines().collect();
        let start = output_lines.len().saturating_sub(4);
        for (index, line) in output_lines.iter().skip(start).enumerate() {
            let prefix = if index == 0 { "      ↳ " } else { "        " };
            ctx.lines.push(Line::from(vec![
                Span::styled(prefix.to_string(), Style::default().fg(ctx.theme.quiet)),
                Span::styled((*line).to_string(), Style::default().fg(ctx.theme.error)),
            ]));
            ctx.tool_row_lines
                .push((row.call_id.clone(), ctx.lines.len() - 1));
        }
    }
}

fn render_compact_tool(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    let line_index = ctx.lines.len();
    ctx.lines.push(compact_tool_line(
        row,
        ctx.app,
        ctx.theme,
        ctx.mode_color,
        ctx.width,
    ));
    ctx.tool_row_lines.push((row.call_id.clone(), line_index));
    let expanded = ctx.app.is_tool_expanded(&row.call_id);
    if tool_uses_generic_detail(&row.name) {
        if expanded {
            append_generic_tool_detail(ctx.lines, row, ctx.theme, ctx.tool_row_lines);
        }
    } else if expanded && row.state == ToolRowState::Failed {
        append_tool_output_detail(ctx.lines, row, ctx.theme, ctx.tool_row_lines);
    }
}

fn append_generic_tool_detail(
    lines: &mut Vec<Line<'static>>,
    row: &ToolRow,
    theme: &Theme,
    tool_row_lines: &mut Vec<(String, usize)>,
) {
    let mut rendered = false;
    if let Ok(serde_json::Value::Object(input)) =
        serde_json::from_str::<serde_json::Value>(&row.arguments)
    {
        for (key, value) in input {
            let value = match value {
                serde_json::Value::String(text) => text,
                other => other.to_string(),
            };
            let value = strip_ansi_codes(&value).replace('\n', " ");
            lines.push(Line::from(vec![
                Span::styled("      : ", Style::default().fg(theme.quiet)),
                Span::styled(format!("{key}: "), Style::default().fg(theme.dim)),
                Span::styled(value, Style::default().fg(theme.ink)),
            ]));
            tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
            rendered = true;
        }
    } else if !row.arguments.trim().is_empty() {
        let arguments = strip_ansi_codes(&row.arguments).replace('\n', " ");
        lines.push(Line::from(vec![
            Span::styled("      : ", Style::default().fg(theme.quiet)),
            Span::styled(arguments, Style::default().fg(theme.ink)),
        ]));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
        rendered = true;
    }

    let output = strip_ansi_codes(&row.output);
    let output_lines: Vec<&str> = output.trim().lines().collect();
    for (index, line) in output_lines.iter().take(20).enumerate() {
        let prefix = if index == 0 {
            "      output: "
        } else {
            "              "
        };
        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), Style::default().fg(theme.quiet)),
            Span::styled((*line).to_string(), Style::default().fg(theme.ink)),
        ]));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
        rendered = true;
    }
    if output_lines.len() > 20 {
        lines.push(Line::from(Span::styled(
            format!("              … {} more lines", output_lines.len() - 20),
            Style::default().fg(theme.dim),
        )));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
    }

    if !rendered {
        lines.push(Line::from(Span::styled(
            "      (no input or output)",
            Style::default().fg(theme.dim),
        )));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
    }
}

fn append_tool_output_detail(
    lines: &mut Vec<Line<'static>>,
    row: &ToolRow,
    theme: &Theme,
    tool_row_lines: &mut Vec<(String, usize)>,
) {
    let output = strip_ansi_codes(&row.output);
    let output_lines: Vec<&str> = output.trim().lines().collect();
    if output_lines.is_empty() {
        return;
    }
    for (index, line) in output_lines.iter().take(20).enumerate() {
        let prefix = if index == 0 {
            "      failed: "
        } else {
            "              "
        };
        lines.push(Line::from(vec![
            Span::styled(prefix.to_string(), Style::default().fg(theme.quiet)),
            Span::styled((*line).to_string(), Style::default().fg(theme.ink)),
        ]));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
    }
    if output_lines.len() > 20 {
        lines.push(Line::from(Span::styled(
            format!("              … {} more lines", output_lines.len() - 20),
            Style::default().fg(theme.dim),
        )));
        tool_row_lines.push((row.call_id.clone(), lines.len() - 1));
    }
}

fn append_loaded_files_detail(ctx: &mut ChatRenderContext<'_>, row: &ToolRow) {
    let Some(loaded) = row
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("loaded"))
        .and_then(|value| value.as_array())
    else {
        return;
    };
    for path in loaded.iter().filter_map(|value| value.as_str()).take(16) {
        ctx.lines.push(Line::from(vec![
            Span::raw("      "),
            Span::styled("↳ Loaded ", Style::default().fg(ctx.theme.quiet)),
            Span::styled(path.to_string(), Style::default().fg(ctx.theme.ink)),
        ]));
        ctx.tool_row_lines
            .push((row.call_id.clone(), ctx.lines.len() - 1));
    }
}

fn compact_tool_line(
    row: &ToolRow,
    app: &App,
    theme: &Theme,
    mode_color: Color,
    width: u16,
) -> Line<'static> {
    let active = matches!(row.state, ToolRowState::Pending | ToolRowState::Running);
    let canonical = canonical_tool_name(&row.name);
    let marker_text = if active {
        app.compact_spinner_frame().to_string()
    } else {
        tool_completed_icon(&row.name, row.state).to_string()
    };
    let marker_style = if active {
        if row.state == ToolRowState::Running {
            Style::default().fg(mode_color)
        } else {
            Style::default().fg(theme.dim)
        }
    } else if row.state == ToolRowState::Failed {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.quiet)
            .add_modifier(Modifier::BOLD)
    };

    let text = if active {
        if tool_uses_generic_detail(&row.name) || canonical == "execute" {
            canonical.to_string()
        } else {
            tool_pending_label(&row.name).to_string()
        }
    } else {
        tool_headline(row)
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
    let failed_suffix = if row.state == ToolRowState::Failed && !err_msg.is_empty() {
        format!(" · failed: {err_msg}")
    } else {
        String::new()
    };
    let background = tool_is_background(row);
    let badge = " Background ";
    let reserve =
        5 + if background {
            text_cell_width(badge) + 1
        } else {
            0
        } + text_cell_width(&failed_suffix);
    let max_text_width = (width as usize).saturating_sub(reserve);
    let text = if width > 0 && text_cell_width(&text) > max_text_width && max_text_width >= 8 {
        truncate_tool_text(&text, max_text_width)
    } else {
        text
    };

    let mut spans = vec![
        Span::raw("   "),
        Span::styled(format!("{marker_text} "), marker_style),
        Span::styled(
            text,
            Style::default().fg(if active { theme.quiet } else { theme.ink }),
        ),
    ];
    if background {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            badge,
            Style::default()
                .fg(mode_color)
                .bg(theme.bg_element)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if !failed_suffix.is_empty() {
        spans.push(Span::styled(
            failed_suffix,
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
        return tool_pending_label(&row.name).to_string();
    }
    if canonical_tool_name(&row.name) == "bash" {
        return shell_command(row);
    }
    tool_headline(row)
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
fn format_plain_bold(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    if !text.contains("**") {
        return vec![Span::styled(
            text.to_string(),
            Style::default().fg(theme.ink),
        )];
    }
    let mut spans = Vec::new();
    let mut remainder = text;
    while let Some(start_idx) = remainder.find("**") {
        let after_open = &remainder[start_idx + 2..];
        if let Some(end_idx) = after_open.find("**") {
            let bold_content = &after_open[..end_idx];
            if !bold_content.is_empty() {
                let before = &remainder[..start_idx];
                if !before.is_empty() {
                    spans.push(Span::styled(
                        before.to_string(),
                        Style::default().fg(theme.ink),
                    ));
                }
                spans.push(Span::styled(
                    bold_content.to_string(),
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
                ));
                remainder = &after_open[end_idx + 2..];
                continue;
            }
        }
        break;
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
        return format_plain_bold(text, theme);
    }

    let mut spans = Vec::new();
    let mut remainder = text;

    while let Some(start_idx) = remainder.find('`') {
        let before = &remainder[..start_idx];
        if let Some(end_idx) = remainder[start_idx + 1..].find('`') {
            if !before.is_empty() {
                spans.extend(format_plain_bold(before, theme));
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
        spans.extend(format_plain_bold(remainder, theme));
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
    format_transcript_lines_with_mode(
        transcript,
        theme,
        mode_color,
        width,
        crate::config::MarkdownMode::Rendered,
    )
}

pub fn format_transcript_lines_with_mode(
    transcript: &str,
    theme: &Theme,
    mode_color: Color,
    width: Option<u16>,
    markdown_mode: crate::config::MarkdownMode,
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
    if markdown_mode == crate::config::MarkdownMode::Source {
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(raw_lines.len());
        for line in &raw_lines {
            if *line == "[earlier transcript compacted]" {
                lines.push(Line::from(Span::styled(
                    (*line).to_string(),
                    Style::default().fg(theme.dim),
                )));
            } else if let Some(prompt) = line.strip_prefix("> ") {
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
            } else {
                lines.push(Line::from(Span::styled(
                    (*line).to_string(),
                    Style::default().fg(theme.ink),
                )));
            }
        }
        return lines;
    }
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(raw_lines.len());
    let mut in_code_block = false;
    let mut code_fence_char = '`';
    let mut code_fence_len = 3usize;
    let mut code_block_is_markdown = false;
    let mut nested_fence_depth = 0usize;
    let mut compaction_marker_seen = false;
    let mut orphan_fence = false;
    for (i, line) in raw_lines.iter().enumerate() {
        if *line == "[earlier transcript compacted]" {
            compaction_marker_seen = true;
            orphan_fence = false;
            in_code_block = false;
            nested_fence_depth = 0;
            lines.push(Line::from(Span::styled(
                (*line).to_string(),
                Style::default().fg(theme.dim),
            )));
            continue;
        }
        if let Some((fence_char, fence_len, info)) = parse_code_fence(line) {
            if !in_code_block {
                in_code_block = true;
                code_fence_char = fence_char;
                code_fence_len = fence_len;
                code_block_is_markdown = is_nested_markdown_lang(info);
                nested_fence_depth = 0;
                orphan_fence = compaction_marker_seen
                    && info.is_empty()
                    && i > 0
                    && !raw_lines[i - 1].trim().is_empty();
                if !info.is_empty() {
                    let header_line = Line::from(vec![
                        Span::raw("  "),
                        Span::styled(info.to_string(), Style::default().fg(theme.dim)),
                    ]);
                    lines.push(pad_card_line(header_line, card_width, theme.bg_element));
                }
                continue;
            }

            if !info.is_empty() {
                if fence_char == code_fence_char {
                    nested_fence_depth = nested_fence_depth.saturating_add(1);
                }
            } else if fence_char == code_fence_char && fence_len >= code_fence_len {
                if nested_fence_depth > 0 {
                    nested_fence_depth -= 1;
                } else if code_block_is_markdown
                    && fence_len == code_fence_len
                    && has_later_closing_fence(&raw_lines, i + 1, code_fence_char, code_fence_len)
                {
                    nested_fence_depth = 1;
                } else {
                    in_code_block = false;
                    orphan_fence = false;
                    code_block_is_markdown = false;
                    nested_fence_depth = 0;
                    lines.push(Line::from(""));
                    continue;
                }
            }
        }

        if orphan_fence
            && line.strip_prefix("> ").is_some()
            && (i == 0
                || raw_lines[i - 1].trim().is_empty()
                || parse_code_fence(raw_lines[i - 1]).is_some()
                || raw_lines[i - 1] == "[earlier transcript compacted]")
        {
            in_code_block = false;
            orphan_fence = false;
            code_block_is_markdown = false;
            nested_fence_depth = 0;
        }
        if in_code_block {
            if width.is_some() {
                let code_content_width = card_width.saturating_sub(2).max(16);
                for wrapped in wrap_line_to_width(line, code_content_width) {
                    let code_line = Line::from(vec![
                        Span::raw("  "),
                        Span::styled(wrapped, Style::default().fg(theme.ink)),
                    ]);
                    lines.push(pad_card_line(code_line, card_width, theme.bg_element));
                }
            } else {
                let code_line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(line.to_string(), Style::default().fg(theme.ink)),
                ]);
                lines.push(pad_card_line(code_line, card_width, theme.bg_element));
            }
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
        let mut ctx = ChatRenderContext {
            lines: &mut lines,
            app: &app,
            theme: &theme,
            mode_color: Color::Cyan,
            width: 60,
            tool_row_lines: &mut tool_lines,
        };
        render_shell_card(&mut ctx, &row);
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
        let mut ctx1 = ChatRenderContext {
            lines: &mut lines1,
            app: &app,
            theme: &theme,
            mode_color: Color::Cyan,
            width: 60,
            tool_row_lines: &mut tool_lines,
        };
        render_shell_card(&mut ctx1, &row1);
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
        let mut ctx2 = ChatRenderContext {
            lines: &mut lines2,
            app: &app,
            theme: &theme,
            mode_color: Color::Cyan,
            width: 60,
            tool_row_lines: &mut tool_lines,
        };
        render_shell_card(&mut ctx2, &row2);
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
        let mut ctx = ChatRenderContext {
            lines: &mut lines,
            app: &app,
            theme: &theme,
            mode_color: Color::Cyan,
            width: 70,
            tool_row_lines: &mut tool_lines,
        };
        render_diff_card(&mut ctx, &row);
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
        let mut ctx = ChatRenderContext {
            lines: &mut lines,
            app: &app,
            theme: &theme,
            mode_color,
            width: 80,
            tool_row_lines: &mut tool_lines,
        };
        append_stream_parts(&mut ctx, &parts);
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
        let mut ctx = ChatRenderContext {
            lines: &mut lines,
            app: &app,
            theme: &theme,
            mode_color: Color::Cyan,
            width: 60,
            tool_row_lines: &mut tool_lines,
        };
        render_shell_card(&mut ctx, &row);
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
    fn test_generic_tool_detail_expands_input_and_output() {
        let mut app = App::default();
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mode_color = Color::Cyan;
        let row = ToolRow {
            call_id: "mcp-1".to_string(),
            name: "mcp_lookup".to_string(),
            desc: "needle".to_string(),
            arguments: serde_json::json!({ "query": "needle", "limit": 2 }).to_string(),
            output: "first result\nsecond result".to_string(),
            state: ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: true,
        };
        assert!(row.compute_expandable());
        app.set_tool_rows_for_test(vec![row]);
        let parts = vec![StreamPart::Tool("mcp-1".to_string())];

        let mut lines = Vec::new();
        let mut tool_lines = Vec::new();
        let mut ctx = ChatRenderContext {
            lines: &mut lines,
            app: &app,
            theme: &theme,
            mode_color,
            width: 80,
            tool_row_lines: &mut tool_lines,
        };
        append_stream_parts(&mut ctx, &parts);
        assert!(!lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("output:"))
        }));

        app.toggle_tool_expanded("mcp-1");
        let mut expanded = Vec::new();
        let mut expanded_tool_lines = Vec::new();
        let mut ctx = ChatRenderContext {
            lines: &mut expanded,
            app: &app,
            theme: &theme,
            mode_color,
            width: 80,
            tool_row_lines: &mut expanded_tool_lines,
        };
        append_stream_parts(&mut ctx, &parts);
        let text = expanded
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("query: needle"), "{text}");
        assert!(text.contains("limit: 2"), "{text}");
        assert!(text.contains("output: first result"), "{text}");
        assert!(text.contains("second result"), "{text}");
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
        let parts = vec![StreamPart::ReasoningDone {
            text: "Line one\nLine two".to_string(),
            duration: std::time::Duration::from_millis(1200),
        }];

        // Collapsed by default
        let mut lines = Vec::new();
        let mut tool_lines = Vec::new();
        let mut ctx = ChatRenderContext {
            lines: &mut lines,
            app: &app,
            theme: &theme,
            mode_color,
            width: 80,
            tool_row_lines: &mut tool_lines,
        };
        append_stream_parts(&mut ctx, &parts);
        assert!(tool_lines.iter().any(|(id, _)| id == "__thought__"));
        let thought_header = lines.iter().find(|l| {
            l.spans
                .iter()
                .any(|s| s.content.starts_with("+ Thought · "))
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
        let mut ctx_exp = ChatRenderContext {
            lines: &mut lines_exp,
            app: &app,
            theme: &theme,
            mode_color,
            width: 80,
            tool_row_lines: &mut tool_lines_exp,
        };
        append_stream_parts(&mut ctx_exp, &parts);
        let expanded_header = lines_exp.iter().find(|l| {
            l.spans
                .iter()
                .any(|s| s.content.starts_with("- Thought · "))
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
    fn test_reasoning_label_uses_opencode_title_rule() {
        assert_eq!(
            reasoning_label("**Surveying the repository**\n\nbody", false),
            "Thinking: Surveying the repository"
        );
        assert_eq!(
            reasoning_label("**Surveying the repository**\n\nbody", true),
            "Thought: Surveying the repository"
        );
        assert_eq!(reasoning_label("plain thought", false), "Thinking");
        assert_eq!(reasoning_label("plain thought", true), "Thought");
        // Bold text that is not its own paragraph is not a title.
        assert_eq!(
            reasoning_label("**bold** in the middle of a sentence", true),
            "Thought"
        );
    }

    #[test]
    fn test_append_stream_parts_active_reasoning_label() {
        let app = App::default();
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let parts = vec![StreamPart::Reasoning(
            "**Surveying the repository**\n\nbody".to_string(),
        )];
        let mut lines = Vec::new();
        let mut tool_lines = Vec::new();
        let mut ctx = ChatRenderContext {
            lines: &mut lines,
            app: &app,
            theme: &theme,
            mode_color: Color::Cyan,
            width: 80,
            tool_row_lines: &mut tool_lines,
        };
        append_stream_parts(&mut ctx, &parts);
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("Thinking: Surveying the repository"))
        }));
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

    fn test_tool_row(
        call_id: &str,
        name: &str,
        state: ToolRowState,
        arguments: serde_json::Value,
    ) -> ToolRow {
        ToolRow {
            call_id: call_id.to_string(),
            name: name.to_string(),
            desc: String::new(),
            arguments: arguments.to_string(),
            output: String::new(),
            state,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn render_parts(app: &App, parts: &[StreamPart], width: u16) -> (String, Vec<(String, usize)>) {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let mut lines = Vec::new();
        let mut tool_lines = Vec::new();
        let mut ctx = ChatRenderContext {
            lines: &mut lines,
            app,
            theme: &theme,
            mode_color: Color::Cyan,
            width,
            tool_row_lines: &mut tool_lines,
        };
        append_stream_parts(&mut ctx, parts);
        let text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        (text, tool_lines)
    }

    #[test]
    fn exploration_groups_collapse_and_expand_by_category() {
        let mut app = App::default();
        let rows = vec![
            test_tool_row(
                "read-1",
                "read_file",
                ToolRowState::Completed,
                serde_json::json!({ "path": "src/main.rs" }),
            ),
            test_tool_row(
                "grep-1",
                "grep",
                ToolRowState::Completed,
                serde_json::json!({ "query": "needle", "path": "." }),
            ),
            test_tool_row(
                "glob-1",
                "glob",
                ToolRowState::Completed,
                serde_json::json!({ "pattern": "*.rs", "path": "src" }),
            ),
            test_tool_row(
                "fetch-1",
                "webfetch",
                ToolRowState::Completed,
                serde_json::json!({ "url": "https://example.com" }),
            ),
        ];
        app.set_tool_rows_for_test(rows);
        let parts = vec![
            StreamPart::Tool("read-1".to_string()),
            StreamPart::Tool("grep-1".to_string()),
            StreamPart::Tool("glob-1".to_string()),
            StreamPart::Tool("fetch-1".to_string()),
        ];

        let (collapsed, tool_lines) = render_parts(&app, &parts, 100);
        assert!(
            collapsed.contains("Explored: 1 read, 2 searches, 1 fetch"),
            "{collapsed}"
        );
        assert!(!collapsed.contains("Read src/main.rs"), "{collapsed}");
        assert!(
            tool_lines.iter().any(|(id, _)| id == "group:read-1"),
            "group header must be clickable: {tool_lines:?}"
        );

        let (single, _) = render_parts(&app, &[StreamPart::Tool("read-1".to_string())], 100);
        assert!(single.contains("→ Read src/main.rs"), "{single}");
        assert!(!single.contains("Explored:"), "{single}");

        app.toggle_tool_expanded("group:read-1");
        let (expanded, _) = render_parts(&app, &parts, 100);
        assert!(expanded.contains("→ Read src/main.rs"), "{expanded}");
        assert!(expanded.contains("✱ Grep \"needle\" in ."), "{expanded}");
        assert!(expanded.contains("✱ Glob \"*.rs\" in src"), "{expanded}");
        assert!(
            expanded.contains("% WebFetch https://example.com"),
            "{expanded}"
        );

        let mut pending_rows = app.tool_rows().to_vec();
        pending_rows[0].state = ToolRowState::Running;
        app.set_tool_rows_for_test(pending_rows);
        let (pending, _) = render_parts(&app, &parts, 100);
        assert!(
            pending.contains("Exploring: 1 read, 2 searches, 1 fetch"),
            "{pending}"
        );
    }

    #[test]
    fn specialized_tools_render_opencode_icons_and_labels() {
        let app = App::default();
        let theme = ThemeKind::ClawcodeDark.to_theme();

        let mut glob = test_tool_row(
            "glob",
            "glob_search",
            ToolRowState::Completed,
            serde_json::json!({ "pattern": "*.rs", "path": "src" }),
        );
        glob.metadata = Some(serde_json::json!({ "count": 3 }));
        let mut grep = test_tool_row(
            "grep",
            "grep_search",
            ToolRowState::Completed,
            serde_json::json!({ "query": "needle", "path": "." }),
        );
        grep.metadata = Some(serde_json::json!({ "matches": 1 }));
        let mut websearch = test_tool_row(
            "websearch",
            "websearch",
            ToolRowState::Completed,
            serde_json::json!({ "query": "rust" }),
        );
        websearch.metadata = Some(serde_json::json!({ "provider": "Brave" }));
        let mut empty_glob = test_tool_row(
            "glob-empty",
            "glob",
            ToolRowState::Completed,
            serde_json::json!({ "pattern": "*.none" }),
        );
        empty_glob.metadata = Some(serde_json::json!({ "count": 0 }));
        let mut failed_todos = test_tool_row(
            "todos-failed",
            "update_plan",
            ToolRowState::Failed,
            serde_json::json!({ "plan": [] }),
        );
        failed_todos.output = "plan update failed".to_string();

        let rows = vec![
            test_tool_row(
                "read",
                "read",
                ToolRowState::Completed,
                serde_json::json!({ "path": "src/main.rs" }),
            ),
            glob,
            grep,
            test_tool_row(
                "fetch",
                "webfetch",
                ToolRowState::Completed,
                serde_json::json!({ "url": "https://example.com" }),
            ),
            websearch,
            test_tool_row(
                "write",
                "write",
                ToolRowState::Completed,
                serde_json::json!({ "path": "notes.md" }),
            ),
            test_tool_row(
                "edit",
                "edit",
                ToolRowState::Completed,
                serde_json::json!({ "path": "src/main.rs" }),
            ),
            test_tool_row(
                "patch",
                "patch",
                ToolRowState::Completed,
                serde_json::json!({ "path": "src/lib.rs" }),
            ),
            test_tool_row(
                "skill",
                "skill",
                ToolRowState::Completed,
                serde_json::json!({ "name": "review" }),
            ),
            test_tool_row(
                "question",
                "question",
                ToolRowState::Completed,
                serde_json::json!({ "questions": [{ "question": "a" }, { "question": "b" }] }),
            ),
            test_tool_row(
                "todos",
                "update_plan",
                ToolRowState::Completed,
                serde_json::json!({ "plan": [] }),
            ),
            test_tool_row(
                "task",
                "task",
                ToolRowState::Completed,
                serde_json::json!({ "agent": "scout", "description": "inspect parser" }),
            ),
            test_tool_row(
                "execute",
                "execute",
                ToolRowState::Completed,
                serde_json::json!({}),
            ),
            test_tool_row(
                "generic",
                "mcp_lookup",
                ToolRowState::Completed,
                serde_json::json!({ "query": "needle" }),
            ),
            empty_glob,
            failed_todos,
        ];
        let labels = rows
            .iter()
            .map(|row| line_text(&compact_tool_line(row, &app, &theme, Color::Cyan, 120)))
            .collect::<Vec<_>>();

        assert_eq!(labels[0], "   → Read src/main.rs");
        assert_eq!(labels[1], "   ✱ Glob \"*.rs\" in src (3 matches)");
        assert_eq!(labels[2], "   ✱ Grep \"needle\" in . (1 match)");
        assert_eq!(labels[3], "   % WebFetch https://example.com");
        assert_eq!(labels[4], "   ◈ Web Search via Brave \"rust\"");
        assert_eq!(labels[5], "   ← Write notes.md");
        assert_eq!(labels[6], "   ← Edit src/main.rs");
        assert_eq!(labels[7], "   ✓ Patching src/lib.rs");
        assert_eq!(labels[8], "   → Skill \"review\"");
        assert_eq!(labels[9], "   → Asked 2 questions");
        assert_eq!(labels[10], "   ⚙ Cleared todos");
        assert_eq!(labels[11], "   ✓ Scout Subagent — inspect parser");
        assert_eq!(labels[12], "   ✓ execute");
        assert_eq!(labels[13], "   ✓ mcp_lookup needle");
        assert_eq!(labels[14], "   ✱ Glob \"*.none\"");
        assert_eq!(
            labels[15],
            "   ✗ Todo update failed · failed: plan update failed"
        );
    }

    #[test]
    fn specialized_cards_render_write_todos_and_question_content() {
        let mut app = App::default();
        let write_content = (1..=12)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let write = test_tool_row(
            "write",
            "write_file",
            ToolRowState::Completed,
            serde_json::json!({ "path": "notes.md", "content": write_content }),
        );
        let todos = test_tool_row(
            "todos",
            "update_plan",
            ToolRowState::Completed,
            serde_json::json!({
                "plan": [
                    { "step": "done", "status": "completed" },
                    { "step": "later", "status": "pending" }
                ]
            }),
        );
        let mut question = test_tool_row(
            "question",
            "question",
            ToolRowState::Completed,
            serde_json::json!({ "questions": [{ "question": "Which database?" }] }),
        );
        question.metadata = Some(serde_json::json!({ "answers": [["sqlite"]] }));
        app.set_tool_rows_for_test(vec![write, todos, question]);

        let parts = vec![
            StreamPart::Tool("write".to_string()),
            StreamPart::Tool("todos".to_string()),
            StreamPart::Tool("question".to_string()),
        ];
        let (text, _) = render_parts(&app, &parts, 80);
        assert!(text.contains("# Wrote notes.md"), "{text}");
        assert!(text.contains("line 10"), "{text}");
        assert!(text.contains("↳ click to expand (2 more lines)"), "{text}");
        assert!(!text.contains("line 11"), "{text}");
        assert!(text.contains("# Todos"), "{text}");
        assert!(text.contains("✔ done"), "{text}");
        assert!(text.contains("□ later"), "{text}");
        assert!(text.contains("⚙ Cleared todos"), "{text}");
        assert!(text.contains("# Questions"), "{text}");
        assert!(text.contains("Which database?"), "{text}");
        assert!(text.contains("sqlite"), "{text}");
        assert!(text.contains("→ Asked 1 question"), "{text}");

        app.toggle_tool_expanded("write");
        let (expanded, _) = render_parts(&app, &parts, 80);
        assert!(expanded.contains("line 11"), "{expanded}");
        assert!(expanded.contains("↳ click to collapse"), "{expanded}");

        let mut read = test_tool_row(
            "read-loaded",
            "read_file",
            ToolRowState::Completed,
            serde_json::json!({ "path": "src/main.rs" }),
        );
        read.metadata = Some(serde_json::json!({
            "loaded": ["src/one.rs", "src/two.rs"]
        }));
        app.set_tool_rows_for_test(vec![read]);
        let (read_text, _) = render_parts(&app, &[StreamPart::Tool("read-loaded".to_string())], 80);
        assert!(read_text.contains("↳ Loaded src/one.rs"), "{read_text}");
        assert!(read_text.contains("↳ Loaded src/two.rs"), "{read_text}");
    }

    #[test]
    fn shell_card_streams_live_output_and_marks_background_runs() {
        let mut app = App::default();
        let mut running = test_tool_row(
            "shell-running",
            "bash",
            ToolRowState::Running,
            serde_json::json!({ "command": "cargo run" }),
        );
        app.set_tool_rows_for_test(vec![running.clone()]);
        app.append_tool_progress("shell-running", "Compiling...\n");
        app.append_tool_progress("shell-running", "Linking...\n");
        running.output = app.tool_rows()[0].output.clone();
        app.set_tool_rows_for_test(vec![running]);
        let (text, _) = render_parts(&app, &[StreamPart::Tool("shell-running".to_string())], 80);
        assert!(text.contains("cargo run"), "{text}");
        assert!(text.contains("Compiling..."), "{text}");
        assert!(text.contains("Linking..."), "{text}");

        let mut background = test_tool_row(
            "shell-background",
            "bash",
            ToolRowState::Completed,
            serde_json::json!({ "command": "cargo run" }),
        );
        background.output = "Background command started (pid 42).".to_string();
        background.metadata = Some(serde_json::json!({
            "status": "running",
            "background": true,
            "pid": 42,
        }));
        app.set_tool_rows_for_test(vec![background]);
        let (text, _) = render_parts(
            &app,
            &[StreamPart::Tool("shell-background".to_string())],
            80,
        );
        assert!(text.contains("$ cargo run"), "{text}");
        assert!(text.contains("Background"), "{text}");
    }

    #[test]
    fn replayed_durable_tool_events_do_not_duplicate_parts_or_regress_state() {
        let mut app = App::default();
        app.set_stream_parts_for_test(
            vec![
                StreamPart::User("inspect files".to_string()),
                StreamPart::Text("Partial answer".to_string()),
                StreamPart::Tool("call-1".to_string()),
            ],
            Some(0),
        );
        let mut row = test_tool_row(
            "call-1",
            "glob",
            ToolRowState::Running,
            serde_json::json!({ "pattern": "*.rs", "path": "src" }),
        );
        row.output = "partial".to_string();
        app.set_tool_rows_for_test(vec![row]);

        let event = |seq, kind: &str, payload: serde_json::Value| crate::runtime::RuntimeEvent {
            session_id: 1,
            generation_id: Some(1),
            seq,
            kind: kind.to_string(),
            payload_json: payload.to_string(),
        };
        app.apply_replayed_event(&event(
            1,
            "tool_call_created",
            serde_json::json!({
                "call_id": "call-1",
                "tool_name": "glob",
                "arguments": { "pattern": "*.rs", "path": "src" }
            }),
        ));
        app.apply_replayed_event(&event(
            2,
            "tool_call_started",
            serde_json::json!({
                "call_id": "call-1",
                "tool_name": "glob",
                "arguments": { "pattern": "*.rs", "path": "src" }
            }),
        ));
        app.apply_replayed_event(&event(
            3,
            "tool_call_settled",
            serde_json::json!({
                "call_id": "call-1",
                "tool_name": "glob",
                "status": "completed",
                "result": "src/main.rs\nsrc/lib.rs",
                "metadata": { "count": 2 }
            }),
        ));
        app.apply_replayed_event(&event(
            4,
            "assistant_message_updated",
            serde_json::json!({ "content": "Final answer" }),
        ));

        assert_eq!(app.stream_parts().len(), 3);
        assert_eq!(
            app.stream_parts()
                .iter()
                .filter(|part| matches!(part, StreamPart::Tool(_)))
                .count(),
            1
        );
        assert!(matches!(
            app.stream_parts().last(),
            Some(StreamPart::Tool(id)) if id == "call-1"
        ));
        let text = app
            .stream_parts()
            .iter()
            .find_map(|part| match part {
                StreamPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .unwrap();
        assert_eq!(text, "Final answer");

        let row = &app.tool_rows()[0];
        assert_eq!(row.state, ToolRowState::Completed);
        assert_eq!(row.output, "src/main.rs\nsrc/lib.rs");
        assert_eq!(
            row.metadata.as_ref().and_then(|meta| meta.get("count")),
            Some(&serde_json::json!(2))
        );
    }

    #[test]
    fn nested_fenced_code_blocks_inside_markdown_stay_inside_card() {
        let theme = ThemeKind::ClawcodeDark.to_theme();
        let transcript = "Isi draf:\n\n```markdown\n# Clawcode\n\n## Kompilasi\n\n```\ncargo build --release --locked\n```\n\nBiner hasil kompilasi: `target/release/clawcode`.\n\n## Penggunaan\n\n```bash\ncargo run\n```\n```\n\nSelesai.";
        let lines = format_transcript_lines_with_width(transcript, &theme, Color::Cyan, Some(80));

        let prose_inside_md = lines
            .iter()
            .find(|line| line_text(line).contains("Biner hasil kompilasi"))
            .expect("must contain markdown prose line");
        let cmd_inside_md = lines
            .iter()
            .find(|line| line_text(line).contains("cargo build --release --locked"))
            .expect("must contain command line");
        let trailing_text = lines
            .iter()
            .find(|line| line_text(line).contains("Selesai."))
            .expect("must contain trailing text after outer fence");

        assert_eq!(prose_inside_md.style.bg, Some(theme.bg_element));
        assert_eq!(cmd_inside_md.style.bg, Some(theme.bg_element));
        assert_eq!(trailing_text.style.bg, None);
    }

    #[test]
    fn reasoning_long_lines_wrap_with_indent_bar_and_trim_trailing_blank() {
        let mut app = App::default();
        app.toggle_thought_expanded();
        let long_thought = "The input is a greeting and does not directly relate to editing the README file in the repository.\n\nSecond paragraph that is also quite long so it wraps across multiple rows cleanly.\n\n";
        let parts = vec![StreamPart::ReasoningDone {
            text: long_thought.to_string(),
            duration: std::time::Duration::from_millis(1500),
        }];
        let (rendered, _) = render_parts(&app, &parts, 50);
        let content_lines: Vec<&str> = rendered
            .lines()
            .skip(1)
            .filter(|line| !line.trim().is_empty())
            .collect();
        assert!(content_lines.len() >= 4, "{rendered}");
        for line in &content_lines {
            assert!(
                line.starts_with("      │ "),
                "wrapped reasoning line missing bar prefix: {line:?}"
            );
            assert!(
                UnicodeWidthStr::width(*line) <= 50,
                "wrapped reasoning line exceeds width: {line:?}"
            );
        }
        assert!(
            content_lines.last().unwrap().trim() != "│",
            "trailing empty thought line should be trimmed: {rendered}"
        );
    }

    #[test]
    fn exploration_group_has_blank_line_before_following_assistant_text() {
        let mut app = App::default();
        app.set_tool_rows_for_test(vec![
            test_tool_row(
                "r1",
                "read_file",
                ToolRowState::Completed,
                serde_json::json!({ "path": "README.md" }),
            ),
            test_tool_row(
                "r2",
                "read_file",
                ToolRowState::Completed,
                serde_json::json!({ "path": "Cargo.toml" }),
            ),
        ]);
        let parts = vec![
            StreamPart::Tool("r1".to_string()),
            StreamPart::Tool("r2".to_string()),
            StreamPart::Text("Draft selesai.".to_string()),
        ];
        let (rendered, _) = render_parts(&app, &parts, 80);
        assert!(
            rendered.contains("Explored: 2 reads\n\n   Draft selesai."),
            "expected blank line between exploration group and assistant text, got:\n{rendered}"
        );
    }
}
