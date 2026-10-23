use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

use super::queued_prompts::format_single_line_preview;
use crate::tui::Theme;

pub const MAX_STASH_ENTRIES: usize = 50;
pub const MAX_STASH_ENTRY_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashEntry {
    pub content: String,
    pub created_at: u64,
}

pub fn bound_stash_content(text: &str) -> String {
    if text.len() <= MAX_STASH_ENTRY_BYTES {
        return text.to_string();
    }
    let mut end = MAX_STASH_ENTRY_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashDialogState {
    pub items: Vec<StashEntry>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
    confirm_delete: Option<usize>,
}

impl StashDialogState {
    pub fn new(items: Vec<StashEntry>) -> Self {
        Self {
            items,
            selected: 0,
            filter: String::new(),
            scroll_offset: 0,
            confirm_delete: None,
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            (0..self.items.len()).collect()
        } else {
            let q = self.filter.to_lowercase();
            self.items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.content.to_lowercase().contains(&q))
                .map(|(idx, _)| idx)
                .collect()
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        let indices = self.filtered_indices();
        indices.get(self.selected).copied()
    }

    pub fn selected_entry(&self) -> Option<&StashEntry> {
        self.selected_index().and_then(|idx| self.items.get(idx))
    }

    pub fn confirm_delete_index(&self) -> Option<usize> {
        self.confirm_delete
    }

    pub fn clear_confirm(&mut self) {
        self.confirm_delete = None;
    }

    /// First call arms deletion confirmation and returns `None`.
    /// Second consecutive call on the same entry removes it and returns `Some(original_index)`.
    pub fn request_or_confirm_delete(&mut self) -> Option<usize> {
        let orig_idx = self.selected_index()?;
        if self.confirm_delete == Some(orig_idx) {
            self.confirm_delete = None;
            self.items.remove(orig_idx);
            let remaining = self.filtered_indices().len();
            if self.selected >= remaining && remaining > 0 {
                self.selected = remaining - 1;
            } else if remaining == 0 {
                self.selected = 0;
            }
            Some(orig_idx)
        } else {
            self.confirm_delete = Some(orig_idx);
            None
        }
    }

    pub fn next(&mut self) {
        self.confirm_delete = None;
        let count = self.filtered_indices().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        } else {
            self.selected = 0;
        }
    }

    pub fn previous(&mut self) {
        self.confirm_delete = None;
        let count = self.filtered_indices().len();
        if count > 0 {
            self.selected = if self.selected == 0 || self.selected >= count {
                count - 1
            } else {
                self.selected - 1
            };
        } else {
            self.selected = 0;
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.confirm_delete = None;
        self.filter.push(c);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn pop_char(&mut self) {
        self.confirm_delete = None;
        self.filter.pop();
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

pub fn render_stash_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &StashDialogState,
    theme: &Theme,
) {
    let width = area.width.clamp(38, 76).min(area.width);
    let filtered = dialog.filtered_indices();
    let max_items_visible = 10.min(area.height.saturating_sub(8) as usize).max(2);
    let visible_items_count = filtered.len().min(max_items_visible).max(1);
    let height = (visible_items_count as u16 + 6).min(area.height.saturating_sub(2));

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let title = if dialog.filter.is_empty() {
        format!(" Prompt Stash ({}) ", dialog.items.len())
    } else {
        format!(" Prompt Stash ({}/{}) ", filtered.len(), dialog.items.len())
    };
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
    if inner.width < 12 || inner.height < 4 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let search_line = if dialog.filter.is_empty() {
        Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(theme.teal)),
            Span::styled(
                "type to filter stashed drafts...",
                Style::default()
                    .fg(theme.ink)
                    .add_modifier(Modifier::ITALIC),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "Filter: ",
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &dialog.filter,
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            ),
            Span::styled("█", Style::default().fg(theme.teal)),
        ])
    };
    frame.render_widget(Paragraph::new(search_line), chunks[0]);

    let divider = "─".repeat(chunks[1].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider,
            Style::default().fg(theme.quiet),
        ))),
        chunks[1],
    );

    let list_height = chunks[2].height as usize;
    let selected_idx = if filtered.is_empty() {
        0
    } else {
        dialog.selected.min(filtered.len() - 1)
    };
    let scroll_offset = if selected_idx >= list_height {
        (selected_idx + 1).saturating_sub(list_height)
    } else {
        0
    };

    let item_lines: Vec<Line> = if filtered.is_empty() {
        let empty_msg = if dialog.items.is_empty() {
            "  No stashed drafts. Use /stash push <text> to save one.".to_string()
        } else {
            format!("  No stashed drafts matching \"{}\".", dialog.filter)
        };
        vec![Line::from(Span::styled(
            empty_msg,
            Style::default()
                .fg(theme.ink)
                .add_modifier(Modifier::ITALIC),
        ))]
    } else {
        filtered
            .iter()
            .skip(scroll_offset)
            .take(list_height)
            .enumerate()
            .map(|(rel_idx, &orig_idx)| {
                let actual_idx = scroll_offset + rel_idx;
                let is_selected = actual_idx == selected_idx;
                let entry = &dialog.items[orig_idx];
                let cursor = if is_selected { " › " } else { "   " };
                let confirming = dialog.confirm_delete == Some(orig_idx);
                let badge = if confirming { " [Ctrl+D again] " } else { "" };
                let row_width = chunks[2].width as usize;
                let used = UnicodeWidthStr::width(cursor) + UnicodeWidthStr::width(badge);
                let preview =
                    format_single_line_preview(&entry.content, row_width.saturating_sub(used));
                let text_style = if confirming {
                    Style::default()
                        .fg(theme.error)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.ink)
                };
                let mut spans = vec![
                    Span::styled(
                        cursor,
                        Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(preview, text_style),
                ];
                if confirming {
                    spans.push(Span::styled(
                        badge,
                        Style::default()
                            .fg(theme.error)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                Line::from(spans).style(if is_selected || confirming {
                    Style::default().bg(theme.bg_element)
                } else {
                    Style::default()
                })
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(item_lines), chunks[2]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(theme.quiet),
        ))),
        chunks[3],
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
        Span::styled("restore  ", Style::default().fg(theme.ink)),
        Span::styled(
            "Ctrl+D ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("delete  ", Style::default().fg(theme.ink)),
        Span::styled(
            "Esc ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("close", Style::default().fg(theme.ink)),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[4]);
}
