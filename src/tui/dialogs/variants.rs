use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantsDialogState {
    pub items: Vec<String>,
    pub selected: usize,
}

impl VariantsDialogState {
    pub fn new(variants: Vec<String>, active_variant: Option<&str>) -> Self {
        let mut items = Vec::with_capacity(variants.len() + 1);
        items.push("default".to_string());
        for v in variants {
            if !v.eq_ignore_ascii_case("default") && !items.contains(&v) {
                items.push(v);
            }
        }
        let target = active_variant.unwrap_or("default");
        let selected = items
            .iter()
            .position(|item| item.eq_ignore_ascii_case(target))
            .unwrap_or(0);
        Self { items, selected }
    }

    pub fn selected_variant(&self) -> Option<Option<&str>> {
        let item = self.items.get(self.selected)?;
        if item.eq_ignore_ascii_case("default") {
            Some(None)
        } else {
            Some(Some(item.as_str()))
        }
    }

    pub fn next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.items.is_empty() {
            self.selected = if self.selected == 0 {
                self.items.len() - 1
            } else {
                self.selected - 1
            };
        }
    }
}

pub fn render_variants_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &VariantsDialogState,
    active_variant: Option<&str>,
    model_label: &str,
    theme: &Theme,
) {
    let width = area.width.clamp(34, 58).min(area.width);
    let height = (dialog.items.len() as u16 + 5).min(area.height.saturating_sub(2));
    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let title = format!(" Model Variant ({model_label}) ");
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
    if inner.width < 10 || inner.height < 3 {
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

    let active_target = active_variant.unwrap_or("default");
    let lines: Vec<Line> = dialog
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_selected = idx == dialog.selected;
            let is_active = item.eq_ignore_ascii_case(active_target);
            let cursor = if is_selected { " › " } else { "   " };
            let dot = if is_active { "● " } else { "  " };
            Line::from(vec![
                Span::styled(
                    cursor,
                    Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    dot,
                    if is_active {
                        Style::default()
                            .fg(theme.success)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.dim)
                    },
                ),
                Span::styled(
                    item.clone(),
                    if is_selected {
                        Style::default()
                            .fg(theme.amber)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.ink)
                    },
                ),
            ])
            .style(if is_selected {
                Style::default().bg(theme.bg_element)
            } else {
                Style::default()
            })
        })
        .collect();
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
        Span::styled("select  ", Style::default().fg(theme.ink)),
        Span::styled(
            "Enter ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("apply  ", Style::default().fg(theme.ink)),
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
