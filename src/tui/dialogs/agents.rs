use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::{ConversationMode, Theme};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub mode: ConversationMode,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsDialogState {
    pub items: Vec<AgentItem>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
}

impl AgentsDialogState {
    pub fn new(active_agent: &str) -> Self {
        let items = Self::default_items();
        let selected = items.iter().position(|a| a.id == active_agent).unwrap_or(0);
        Self {
            items,
            selected,
            filter: String::new(),
            scroll_offset: 0,
        }
    }

    pub fn default_items() -> Vec<AgentItem> {
        vec![
            AgentItem {
                id: "plan".to_string(),
                name: "Plan Agent".to_string(),
                description: "Read-only exploration, architecture analysis, and safe plans"
                    .to_string(),
                mode: ConversationMode::Plan,
                shortcut: Some("Tab /plan".to_string()),
            },
            AgentItem {
                id: "build".to_string(),
                name: "Build Agent".to_string(),
                description: "Autonomous code editing, tool execution, and verified mutations"
                    .to_string(),
                mode: ConversationMode::Build,
                shortcut: Some("Tab /build".to_string()),
            },
            AgentItem {
                id: "review".to_string(),
                name: "Review Agent".to_string(),
                description: "Code review, security checks, and PR readiness verification"
                    .to_string(),
                mode: ConversationMode::Plan,
                shortcut: Some("/review".to_string()),
            },
            AgentItem {
                id: "compact".to_string(),
                name: "Compact Agent".to_string(),
                description: "Terse output, minimal tokens, low-latency execution".to_string(),
                mode: ConversationMode::Build,
                shortcut: Some("/compact".to_string()),
            },
        ]
    }

    pub fn filtered_items(&self) -> Vec<&AgentItem> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let q = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|a| {
                    a.name.to_lowercase().contains(&q)
                        || a.description.to_lowercase().contains(&q)
                        || a.id.to_lowercase().contains(&q)
                })
                .collect()
        }
    }

    pub fn selected_agent(&self) -> Option<&AgentItem> {
        let filtered = self.filtered_items();
        filtered.get(self.selected).copied()
    }

    pub fn next(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    pub fn previous(&mut self) {
        let count = self.filtered_items().len();
        if count > 0 {
            self.selected = if self.selected == 0 {
                count - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn pop_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

pub fn render_agents_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &AgentsDialogState,
    active_mode: ConversationMode,
    theme: &Theme,
) {
    let width = area.width.clamp(36, 74);
    let filtered = dialog.filtered_items();
    let max_items_visible = 8.min(area.height.saturating_sub(8) as usize).max(2);
    let visible_items_count = filtered.len().min(max_items_visible).max(1);
    let height = (visible_items_count as u16 * 2 + 6).min(area.height.saturating_sub(2));

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let total_count = dialog.items.len();
    let filtered_count = filtered.len();
    let title_text = if dialog.filter.is_empty() {
        format!(" Select Agent Mode ({total_count}) ")
    } else {
        format!(" Select Agent Mode ({filtered_count}/{total_count}) ")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.teal))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            title_text,
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
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
            Constraint::Length(1), // search input
            Constraint::Length(1), // top divider
            Constraint::Min(1),    // items
            Constraint::Length(1), // bottom divider
            Constraint::Length(1), // footer hints
        ])
        .split(inner);

    let search_line = if dialog.filter.is_empty() {
        Line::from(vec![
            Span::styled("Filter: ", Style::default().fg(theme.dim)),
            Span::styled(
                "type to filter agents...",
                Style::default()
                    .fg(theme.quiet)
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

    let divider_str = "─".repeat(chunks[1].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider_str,
            Style::default().fg(theme.quiet),
        ))),
        chunks[1],
    );

    let selected_idx = dialog.selected;
    let item_lines: Vec<Line> = if filtered.is_empty() {
        vec![Line::from(Span::styled(
            format!("  No agents matching \"{}\"", dialog.filter),
            Style::default()
                .fg(theme.dim)
                .add_modifier(Modifier::ITALIC),
        ))]
    } else {
        let mut lines = Vec::new();
        for (idx, agent) in filtered.iter().enumerate() {
            let is_selected = idx == selected_idx;
            let is_active = agent.mode == active_mode;

            let cursor = if is_selected { " › " } else { "   " };
            let active_dot = if is_active { "● " } else { "  " };

            let cursor_style = if is_selected {
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let dot_style = if is_active {
                Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.dim)
            };

            let name_style = if is_selected {
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)
            } else if is_active {
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.ink)
            };

            let shortcut_badge = agent
                .shortcut
                .as_deref()
                .map(|s| format!("[{s}]"))
                .unwrap_or_default();

            let row_width = chunks[2].width as usize;
            let prefix_len = cursor.len() + active_dot.len();
            let badge_len = shortcut_badge.len();
            let available_for_name = row_width.saturating_sub(prefix_len + badge_len + 2);
            let display_name = if agent.name.len() > available_for_name && available_for_name > 4 {
                format!("{}…", &agent.name[..available_for_name - 1])
            } else {
                agent.name.clone()
            };

            let pad_len = row_width.saturating_sub(prefix_len + display_name.len() + badge_len);

            let line_style = if is_selected {
                Style::default().bg(theme.bg_element)
            } else {
                Style::default()
            };

            lines.push(
                Line::from(vec![
                    Span::styled(cursor, cursor_style),
                    Span::styled(active_dot, dot_style),
                    Span::styled(display_name, name_style),
                    Span::raw(" ".repeat(pad_len)),
                    Span::styled(shortcut_badge, Style::default().fg(theme.quiet)),
                ])
                .style(line_style),
            );

            // Description line
            let desc_pad = "      ";
            let avail_desc = row_width.saturating_sub(desc_pad.len());
            let display_desc = if agent.description.len() > avail_desc && avail_desc > 4 {
                format!("{}…", &agent.description[..avail_desc - 1])
            } else {
                agent.description.clone()
            };

            lines.push(
                Line::from(vec![
                    Span::raw(desc_pad),
                    Span::styled(display_desc, Style::default().fg(theme.dim)),
                ])
                .style(line_style),
            );
        }
        lines
    };
    frame.render_widget(Paragraph::new(item_lines), chunks[2]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider_str,
            Style::default().fg(theme.quiet),
        ))),
        chunks[3],
    );

    let footer_line = Line::from(vec![
        Span::styled(
            "↑/↓ ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled("navigate  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Enter ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled("select  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Esc ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled("close  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Type ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled("filter", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(footer_line), chunks[4]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agents_dialog_default_items() {
        let items = AgentsDialogState::default_items();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].id, "plan");
        assert_eq!(items[0].mode, ConversationMode::Plan);
        assert_eq!(items[1].id, "build");
        assert_eq!(items[1].mode, ConversationMode::Build);
        assert_eq!(items[2].id, "review");
        assert_eq!(items[2].mode, ConversationMode::Plan);
        assert_eq!(items[3].id, "compact");
        assert_eq!(items[3].mode, ConversationMode::Build);
    }

    #[test]
    fn test_agents_dialog_state_new() {
        let state = AgentsDialogState::new("build");
        assert_eq!(state.selected, 1);
        assert!(state.filter.is_empty());
        assert_eq!(state.scroll_offset, 0);

        let fallback = AgentsDialogState::new("unknown_agent");
        assert_eq!(fallback.selected, 0);
    }

    #[test]
    fn test_agents_dialog_state_push_pop_char() {
        let mut state = AgentsDialogState::new("plan");
        state.selected = 2;
        state.scroll_offset = 1;

        state.push_char('b');
        state.push_char('u');
        assert_eq!(state.filter, "bu");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "b");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn test_agents_dialog_state_navigation() {
        let mut state = AgentsDialogState::new("plan");
        assert_eq!(state.selected, 0);

        state.next();
        assert_eq!(state.selected, 1);

        state.previous();
        assert_eq!(state.selected, 0);

        state.previous();
        assert_eq!(state.selected, 3);

        state.next();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_agents_dialog_state_filtered_items() {
        let mut state = AgentsDialogState::new("plan");
        assert_eq!(state.filtered_items().len(), 4);

        state.push_char('b');
        state.push_char('u');
        state.push_char('i');
        state.push_char('l');
        state.push_char('d');
        let filtered = state.filtered_items();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "build");

        state.filter = "non_existent_agent_xyz".to_string();
        assert!(state.filtered_items().is_empty());
    }

    #[test]
    fn test_agents_dialog_state_selected_agent() {
        let mut state = AgentsDialogState::new("build");
        let selected = state.selected_agent();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().id, "build");

        state.filter = "non_existent_agent_xyz".to_string();
        assert_eq!(state.selected_agent(), None);
    }
}
