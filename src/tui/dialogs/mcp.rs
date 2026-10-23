use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::tui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpDialogStatus {
    Pending,
    Connected { tool_count: usize },
    Failed(String),
    Disabled,
    Unavailable(String),
}

impl McpDialogStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "Connecting ...",
            Self::Connected { .. } => "Connected",
            Self::Failed(_) => "Failed",
            Self::Disabled => "Disabled",
            Self::Unavailable(_) => "Unavailable",
        }
    }

    pub fn status_name(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Connected { .. } => "connected",
            Self::Failed(_) => "failed",
            Self::Disabled => "disabled",
            Self::Unavailable(_) => "unavailable",
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(error) | Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }

    pub fn toggle_label(&self) -> &'static str {
        match self {
            Self::Connected { .. } => "disconnect",
            Self::Failed(_) => "retry",
            Self::Disabled => "connect",
            Self::Pending => "connecting",
            Self::Unavailable(_) => "unavailable",
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpDialogItem {
    pub name: String,
    pub status: McpDialogStatus,
}

impl McpDialogItem {
    pub fn new(name: impl Into<String>, status: McpDialogStatus) -> Self {
        Self {
            name: name.into(),
            status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpErrorDetail {
    pub server: String,
    pub status: String,
    pub error: String,
    pub configuration: String,
    pub tool_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpDialogState {
    pub items: Vec<McpDialogItem>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
    pub detail: Option<McpErrorDetail>,
    pub detail_scroll: u16,
}

impl McpDialogState {
    pub fn new(mut items: Vec<McpDialogItem>) -> Self {
        items.sort_by(|left, right| left.name.cmp(&right.name));
        Self {
            items,
            selected: 0,
            filter: String::new(),
            scroll_offset: 0,
            detail: None,
            detail_scroll: 0,
        }
    }

    pub fn filtered_items(&self) -> Vec<&McpDialogItem> {
        if self.filter.is_empty() {
            return self.items.iter().collect();
        }
        let query = self.filter.to_lowercase();
        self.items
            .iter()
            .filter(|item| item.name.to_lowercase().contains(&query))
            .collect()
    }

    pub fn selected_item(&self) -> Option<&McpDialogItem> {
        let filtered = self.filtered_items();
        if filtered.is_empty() {
            return None;
        }
        let index = self.selected.min(filtered.len() - 1);
        filtered.get(index).copied()
    }

    pub fn next(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        } else {
            self.selected = 0;
        }
    }

    pub fn previous(&mut self) {
        let count = self.filtered_items().len();
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

    pub fn push_char(&mut self, character: char) {
        let previous = self.selected_item().map(|item| item.name.clone());
        self.filter.push(character);
        self.restore_selection(previous);
    }

    pub fn pop_char(&mut self) {
        let previous = self.selected_item().map(|item| item.name.clone());
        self.filter.pop();
        self.restore_selection(previous);
    }

    fn restore_selection(&mut self, previous: Option<String>) {
        let next_selected = previous.and_then(|name| {
            self.filtered_items()
                .iter()
                .position(|item| item.name == name)
        });
        let count = self.filtered_items().len();
        self.selected = next_selected.unwrap_or_else(|| self.selected.min(count.saturating_sub(1)));
        self.scroll_offset = 0;
    }

    pub fn set_status(&mut self, name: &str, status: McpDialogStatus) {
        if let Some(item) = self.items.iter_mut().find(|item| item.name == name) {
            item.status = status;
        }
    }

    pub fn status(&self, name: &str) -> Option<&McpDialogStatus> {
        self.items
            .iter()
            .find(|item| item.name == name)
            .map(|item| &item.status)
    }

    pub fn open_selected_error(&mut self) -> bool {
        let Some(item) = self.selected_item() else {
            return false;
        };
        let Some(error) = item.status.error() else {
            return false;
        };
        let tool_count = match &item.status {
            McpDialogStatus::Connected { tool_count } => Some(*tool_count),
            _ => None,
        };
        self.detail = Some(McpErrorDetail {
            server: item.name.clone(),
            status: item.status.status_name().to_string(),
            error: error.to_string(),
            configuration: format!("mcp.servers.{}", item.name),
            tool_count,
        });
        self.detail_scroll = 0;
        true
    }

    pub fn close_detail(&mut self) -> bool {
        if self.detail.take().is_some() {
            self.detail_scroll = 0;
            true
        } else {
            false
        }
    }

    pub fn scroll_detail_up(&mut self, lines: u16) {
        self.detail_scroll = self.detail_scroll.saturating_sub(lines);
    }

    pub fn scroll_detail_down(&mut self, lines: u16) {
        self.detail_scroll = self.detail_scroll.saturating_add(lines).min(1000);
    }

    pub fn detail_copy_text(&self) -> Option<String> {
        let detail = self.detail.as_ref()?;
        Some(format!(
            "MCP server: {}\nStatus: {}\nConfiguration: {}\n\n{}",
            detail.server, detail.status, detail.configuration, detail.error
        ))
    }
}

fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
    if Span::raw(text).width() <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = Span::raw(character.to_string()).width();
        if width + character_width > max_width.saturating_sub(1) {
            break;
        }
        result.push(character);
        width += character_width;
    }
    result.push('…');
    result
}

pub fn render_mcp_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &McpDialogState,
    theme: &Theme,
) {
    if let Some(detail) = &dialog.detail {
        render_error_detail(frame, area, dialog, detail, theme);
    } else {
        render_server_list(frame, area, dialog, theme);
    }
}

fn render_server_list(frame: &mut Frame<'_>, area: Rect, dialog: &McpDialogState, theme: &Theme) {
    let width = area.width.clamp(46, 88).min(area.width);
    let filtered = dialog.filtered_items();
    let max_items_visible = 12.min(area.height.saturating_sub(8) as usize).max(3);
    let visible_items_count = filtered.len().min(max_items_visible).max(1);
    let height = (visible_items_count as u16 + 6).min(area.height.saturating_sub(2));

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, dialog_area);

    let total_count = dialog.items.len();
    let title_text = if dialog.filter.is_empty() {
        format!(" MCP servers ({total_count}) ")
    } else {
        format!(" MCP servers ({}/{total_count}) ", filtered.len())
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.amber))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            title_text,
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, dialog_area);

    let inner = Rect {
        x: dialog_area.x + 2,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(4),
        height: dialog_area.height.saturating_sub(2),
    };
    if inner.width < 10 || inner.height < 4 {
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
            Span::styled("Search: ", Style::default().fg(theme.dim)),
            Span::styled(
                "Search MCP servers",
                Style::default()
                    .fg(theme.quiet)
                    .add_modifier(Modifier::ITALIC),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "Search: ",
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &dialog.filter,
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▏", Style::default().fg(theme.amber)),
        ])
    };
    frame.render_widget(Paragraph::new(search_line), chunks[0]);

    let divider = "─".repeat(chunks[1].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(theme.quiet),
        ))),
        chunks[1],
    );

    let list_height = chunks[2].height as usize;
    let selected_index = if filtered.is_empty() {
        0
    } else {
        dialog.selected.min(filtered.len() - 1)
    };
    let scroll_offset = if selected_index >= list_height {
        (selected_index + 1).saturating_sub(list_height)
    } else {
        0
    };

    let lines: Vec<Line> = if filtered.is_empty() {
        let message = if dialog.items.is_empty() {
            "  No MCP servers configured.".to_string()
        } else {
            format!("  No MCP servers matching \"{}\"", dialog.filter)
        };
        vec![Line::from(Span::styled(
            message,
            Style::default()
                .fg(theme.dim)
                .add_modifier(Modifier::ITALIC),
        ))]
    } else {
        filtered
            .iter()
            .skip(scroll_offset)
            .take(list_height)
            .enumerate()
            .map(|(relative, item)| {
                let index = scroll_offset + relative;
                let selected = index == selected_index;
                let cursor = if selected { "> " } else { "  " };
                let status_width = Span::raw(item.status.label()).width();
                let name_width = (chunks[2].width as usize).saturating_sub(status_width + 3);
                let name = truncate_with_ellipsis(&item.name, name_width);
                let left = format!("{cursor}{name}");
                let gap = (chunks[2].width as usize)
                    .saturating_sub(Span::raw(&left).width() + status_width + 1);
                let (status_color, status_modifier) = match &item.status {
                    McpDialogStatus::Connected { .. } => (theme.success, Modifier::BOLD),
                    McpDialogStatus::Failed(_) => (theme.error, Modifier::empty()),
                    McpDialogStatus::Unavailable(_) => (theme.warning, Modifier::empty()),
                    McpDialogStatus::Pending | McpDialogStatus::Disabled => {
                        (theme.dim, Modifier::empty())
                    }
                };
                Line::from(vec![
                    Span::styled(cursor, Style::default().fg(theme.amber)),
                    Span::styled(
                        name,
                        if selected {
                            Style::default()
                                .fg(theme.amber)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.ink)
                        },
                    ),
                    Span::raw(" ".repeat(gap)),
                    Span::styled(
                        item.status.label(),
                        Style::default()
                            .fg(status_color)
                            .add_modifier(status_modifier),
                    ),
                ])
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines), chunks[2]);

    let divider = "─".repeat(chunks[3].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(theme.quiet),
        ))),
        chunks[3],
    );

    let toggle = dialog
        .selected_item()
        .map(|item| item.status.toggle_label())
        .unwrap_or("connect");
    let has_error = dialog
        .selected_item()
        .is_some_and(|item| item.status.error().is_some());
    let mut hints = vec![
        Span::styled("↑↓", Style::default().fg(theme.amber)),
        Span::styled(" navigate  ", Style::default().fg(theme.dim)),
        Span::styled("space", Style::default().fg(theme.amber)),
        Span::styled(format!(" {toggle}  "), Style::default().fg(theme.dim)),
    ];
    if has_error {
        hints.push(Span::styled("enter", Style::default().fg(theme.amber)));
        hints.push(Span::styled(
            " view error  ",
            Style::default().fg(theme.dim),
        ));
    }
    hints.push(Span::styled("esc", Style::default().fg(theme.amber)));
    hints.push(Span::styled(" close", Style::default().fg(theme.dim)));
    frame.render_widget(Paragraph::new(Line::from(hints)), chunks[4]);
}

fn render_error_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &McpDialogState,
    detail: &McpErrorDetail,
    theme: &Theme,
) {
    let width = area.width.clamp(52, 96).min(area.width);
    let height = area.height.clamp(16, 30).min(area.height);
    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.error))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            format!(" MCP server: {} ", detail.server),
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, dialog_area);

    let inner = Rect {
        x: dialog_area.x + 2,
        y: dialog_area.y + 1,
        width: dialog_area.width.saturating_sub(4),
        height: dialog_area.height.saturating_sub(2),
    };
    if inner.width < 12 || inner.height < 6 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let error = if detail.error.trim().is_empty() {
        "Unknown MCP connection error"
    } else {
        &detail.error
    };
    frame.render_widget(
        Paragraph::new(error)
            .style(Style::default().fg(theme.ink))
            .wrap(Wrap { trim: false })
            .scroll((dialog.detail_scroll, 0)),
        chunks[0],
    );

    let divider = "─".repeat(chunks[1].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(theme.quiet),
        ))),
        chunks[1],
    );

    let mut context = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(theme.dim)),
            Span::styled(&detail.status, Style::default().fg(theme.ink)),
        ]),
        Line::from(vec![
            Span::styled("Configuration: ", Style::default().fg(theme.dim)),
            Span::styled(&detail.configuration, Style::default().fg(theme.quiet)),
        ]),
    ];
    if let Some(tool_count) = detail.tool_count {
        context.push(Line::from(vec![
            Span::styled("Tools: ", Style::default().fg(theme.dim)),
            Span::styled(tool_count.to_string(), Style::default().fg(theme.ink)),
        ]));
    }
    frame.render_widget(Paragraph::new(context), chunks[2]);

    let divider = "─".repeat(chunks[3].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            divider,
            Style::default().fg(theme.quiet),
        ))),
        chunks[3],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("esc", Style::default().fg(theme.amber)),
            Span::styled(" back  ", Style::default().fg(theme.dim)),
            Span::styled("c", Style::default().fg(theme.amber)),
            Span::styled(" copy error  ", Style::default().fg(theme.dim)),
            Span::styled("ctrl+c", Style::default().fg(theme.amber)),
            Span::styled(" close", Style::default().fg(theme.dim)),
        ])),
        chunks[4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_servers_and_filters_titles() {
        let mut dialog = McpDialogState::new(vec![
            McpDialogItem::new("zeta", McpDialogStatus::Disabled),
            McpDialogItem::new("alpha", McpDialogStatus::Pending),
        ]);
        assert_eq!(
            dialog
                .filtered_items()
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        dialog.push_char('z');
        assert_eq!(dialog.selected_item().unwrap().name, "zeta");
        dialog.pop_char();
        assert_eq!(dialog.selected_item().unwrap().name, "zeta");
    }

    #[test]
    fn only_errors_open_details() {
        let mut dialog = McpDialogState::new(vec![
            McpDialogItem::new(
                "broken",
                McpDialogStatus::Failed("connection refused".to_string()),
            ),
            McpDialogItem::new("ready", McpDialogStatus::Connected { tool_count: 2 }),
        ]);
        assert!(dialog.open_selected_error());
        assert_eq!(dialog.detail.as_ref().unwrap().server, "broken");
        assert_eq!(
            dialog.detail_copy_text().unwrap(),
            "MCP server: broken\nStatus: failed\nConfiguration: mcp.servers.broken\n\nconnection refused"
        );
        assert!(dialog.close_detail());
        dialog.next();
        assert!(!dialog.open_selected_error());
    }

    #[test]
    fn status_labels_match_toggle_actions() {
        assert_eq!(McpDialogStatus::Pending.toggle_label(), "connecting");
        assert_eq!(
            McpDialogStatus::Connected { tool_count: 1 }.toggle_label(),
            "disconnect"
        );
        assert_eq!(
            McpDialogStatus::Failed("nope".to_string()).toggle_label(),
            "retry"
        );
        assert_eq!(McpDialogStatus::Disabled.toggle_label(), "connect");
        assert_eq!(
            McpDialogStatus::Unavailable("no runtime".to_string()).toggle_label(),
            "unavailable"
        );
    }

    #[test]
    fn renders_list_and_error_detail() {
        let theme = Theme::new();
        let mut dialog = McpDialogState::new(vec![
            McpDialogItem::new("alpha", McpDialogStatus::Connected { tool_count: 1 }),
            McpDialogItem::new(
                "broken",
                McpDialogStatus::Failed("connection refused".to_string()),
            ),
        ]);
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_mcp_dialog(frame, frame.area(), &dialog, &theme))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("MCP servers (2)"));
        assert!(content.contains("Connected"));
        assert!(content.contains("Failed"));

        dialog.next();
        assert!(dialog.open_selected_error());
        terminal
            .draw(|frame| render_mcp_dialog(frame, frame.area(), &dialog, &theme))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(content.contains("MCP server: broken"));
        assert!(content.contains("connection refused"));
        assert!(content.contains("mcp.servers.broken"));
    }
}
