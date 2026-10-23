use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPromptItem {
    pub id: i64,
    pub content: String,
    pub delivery: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPromptsDialogState {
    pub items: Vec<PendingPromptItem>,
    pub selected: usize,
    pub scroll_offset: usize,
}

impl QueuedPromptsDialogState {
    pub fn new(items: Vec<PendingPromptItem>) -> Self {
        Self {
            items,
            selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn selected_item(&self) -> Option<&PendingPromptItem> {
        self.items.get(self.selected)
    }

    pub fn next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        } else {
            self.selected = 0;
        }
    }

    pub fn previous(&mut self) {
        if !self.items.is_empty() {
            self.selected = if self.selected == 0 || self.selected >= self.items.len() {
                self.items.len() - 1
            } else {
                self.selected - 1
            };
        } else {
            self.selected = 0;
        }
    }

    pub fn remove_selected(&mut self) -> Option<PendingPromptItem> {
        if self.selected >= self.items.len() {
            return None;
        }
        let item = self.items.remove(self.selected);
        if self.selected >= self.items.len() && !self.items.is_empty() {
            self.selected = self.items.len() - 1;
        } else if self.items.is_empty() {
            self.selected = 0;
        }
        Some(item)
    }

    pub fn mark_selected_steer(&mut self) -> Option<i64> {
        let item = self.items.get_mut(self.selected)?;
        item.delivery = "steer".to_string();
        Some(item.id)
    }
}

pub fn format_single_line_preview(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let normalized = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if UnicodeWidthStr::width(normalized.as_str()) <= max_width {
        return normalized;
    }
    let mut out = String::new();
    let mut width = 0usize;
    let budget = max_width.saturating_sub(1);
    for ch in normalized.chars() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_w > budget {
            break;
        }
        out.push(ch);
        width += ch_w;
    }
    out.push('…');
    out
}

pub fn render_queued_prompts_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &QueuedPromptsDialogState,
    theme: &Theme,
) {
    let width = area.width.clamp(38, 76).min(area.width);
    let max_items_visible = 10.min(area.height.saturating_sub(7) as usize).max(2);
    let visible_items_count = dialog.items.len().min(max_items_visible).max(1);
    let height = (visible_items_count as u16 + 5).min(area.height.saturating_sub(2));

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let title = format!(" Queued Prompts ({}) ", dialog.items.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.teal))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            title,
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, dialog_area);

    let inner = Rect {
        x: dialog_area.x + 2,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(4),
        height: dialog_area.height.saturating_sub(2),
    };
    if inner.width < 12 || inner.height < 3 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let list_height = chunks[0].height as usize;
    let selected_idx = if dialog.items.is_empty() {
        0
    } else {
        dialog.selected.min(dialog.items.len() - 1)
    };
    let scroll_offset = if selected_idx >= list_height {
        (selected_idx + 1).saturating_sub(list_height)
    } else {
        0
    };

    let lines: Vec<Line> = if dialog.items.is_empty() {
        vec![Line::from(Span::styled(
            "  No queued prompts. Submit during an active turn to queue one.",
            Style::default()
                .fg(theme.ink)
                .add_modifier(Modifier::ITALIC),
        ))]
    } else {
        dialog
            .items
            .iter()
            .skip(scroll_offset)
            .take(list_height)
            .enumerate()
            .map(|(rel_idx, item)| {
                let actual_idx = scroll_offset + rel_idx;
                let is_selected = actual_idx == selected_idx;
                let cursor = if is_selected { " › " } else { "   " };
                let is_steer = item.delivery == "steer";
                let badge = if is_steer { "[STEER] " } else { "[QUEUE] " };
                let badge_style = if is_steer {
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.teal)
                };
                let row_width = chunks[0].width as usize;
                let prefix_len = UnicodeWidthStr::width(cursor) + UnicodeWidthStr::width(badge);
                let preview =
                    format_single_line_preview(&item.content, row_width.saturating_sub(prefix_len));
                let text_style = if is_selected {
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.ink)
                };
                let line_style = if is_selected {
                    Style::default().bg(theme.bg_element)
                } else {
                    Style::default()
                };
                Line::from(vec![
                    Span::styled(
                        cursor,
                        Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(badge, badge_style),
                    Span::styled(preview, text_style),
                ])
                .style(line_style)
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines), chunks[0]);

    let divider = "─".repeat(chunks[1].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(theme.quiet),
        ))),
        chunks[1],
    );

    let footer = Line::from(vec![
        Span::styled(
            "↑/↓ ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("navigate  ", Style::default().fg(theme.ink)),
        Span::styled(
            "Enter ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("steer  ", Style::default().fg(theme.ink)),
        Span::styled(
            "u ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("undo  ", Style::default().fg(theme.ink)),
        Span::styled(
            "d ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("discard  ", Style::default().fg(theme.ink)),
        Span::styled(
            "Esc ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("close", Style::default().fg(theme.ink)),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[2]);
}
