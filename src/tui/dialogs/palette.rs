use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    SwitchSession,
    NewSession,
    ClearSession,
    OpenSessionOrProject,
    SwitchModel,
    SwitchVariant,
    SwitchAgent,
    QueuedPrompts,
    PromptStash,
    PromptStashList,
    McpServers,
    ConnectIntegration,
    ReleaseHighlights,
    Settings,
    ToggleThinking,
    ViewStatus,
    Update,
    PairDevice,
    RestartService,
    ReloadConfiguration,
    DebugInfo,
    SwitchTheme,
    Help,
    OpenDocs,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteItem {
    pub action: PaletteAction,
    pub title: &'static str,
    pub category: &'static str,
    pub shortcut: &'static str,
}

pub const PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem {
        action: PaletteAction::SwitchSession,
        title: "Switch session",
        category: "Session",
        shortcut: "<leader>l",
    },
    PaletteItem {
        action: PaletteAction::NewSession,
        title: "New session",
        category: "Session",
        shortcut: "<leader>n",
    },
    PaletteItem {
        action: PaletteAction::ClearSession,
        title: "Clear session",
        category: "Session",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::OpenSessionOrProject,
        title: "Open session or project",
        category: "Session",
        shortcut: "ctrl+o",
    },
    PaletteItem {
        action: PaletteAction::SwitchModel,
        title: "Switch model",
        category: "Agent",
        shortcut: "<leader>m",
    },
    PaletteItem {
        action: PaletteAction::SwitchVariant,
        title: "Switch model variant",
        category: "Agent",
        shortcut: "ctrl+t",
    },
    PaletteItem {
        action: PaletteAction::SwitchAgent,
        title: "Switch agent",
        category: "Agent",
        shortcut: "<leader>a",
    },
    PaletteItem {
        action: PaletteAction::QueuedPrompts,
        title: "Queued prompts",
        category: "Prompt",
        shortcut: "/queued",
    },
    PaletteItem {
        action: PaletteAction::PromptStash,
        title: "Stash prompt",
        category: "Prompt",
        shortcut: "/stash",
    },
    PaletteItem {
        action: PaletteAction::PromptStashList,
        title: "Open prompt stash",
        category: "Prompt",
        shortcut: "/stash list",
    },
    PaletteItem {
        action: PaletteAction::McpServers,
        title: "MCP servers",
        category: "Agent",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::ConnectIntegration,
        title: "Connect an integration",
        category: "Integration",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::ReleaseHighlights,
        title: "Show release highlights",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::Settings,
        title: "Open settings",
        category: "System",
        shortcut: "/settings",
    },
    PaletteItem {
        action: PaletteAction::ToggleThinking,
        title: "Toggle thinking blocks",
        category: "Transcript",
        shortcut: "/thinking",
    },
    PaletteItem {
        action: PaletteAction::ViewStatus,
        title: "View status",
        category: "System",
        shortcut: "<leader>s",
    },
    PaletteItem {
        action: PaletteAction::Update,
        title: "Update Clawcode",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::PairDevice,
        title: "Pair device",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::RestartService,
        title: "Restart service",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::ReloadConfiguration,
        title: "Reload configuration",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::DebugInfo,
        title: "View debug info",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::SwitchTheme,
        title: "Switch theme",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::Help,
        title: "Help",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::OpenDocs,
        title: "Open docs",
        category: "System",
        shortcut: "",
    },
    PaletteItem {
        action: PaletteAction::Exit,
        title: "Exit the app",
        category: "System",
        shortcut: "ctrl+c, ctrl+d, <leader>q",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteDialogState {
    pub filter: String,
    pub selected: usize,
    pub scroll_offset: usize,
}

impl Default for PaletteDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl PaletteDialogState {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn filtered_items(&self) -> Vec<&'static PaletteItem> {
        if self.filter.is_empty() {
            return PALETTE_ITEMS.iter().collect();
        }
        let query = self.filter.to_lowercase();
        PALETTE_ITEMS
            .iter()
            .filter(|item| {
                item.title.to_lowercase().contains(&query)
                    || item.category.to_lowercase().contains(&query)
                    || item.shortcut.to_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn selected_item(&self) -> Option<&'static PaletteItem> {
        let filtered = self.filtered_items();
        if filtered.is_empty() {
            return None;
        }
        filtered.get(self.selected.min(filtered.len() - 1)).copied()
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
        self.filter.push(character);
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn pop_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

pub fn render_palette_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &PaletteDialogState,
    theme: &Theme,
) {
    let width = area.width.clamp(44, 86).min(area.width);
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

    let total_count = PALETTE_ITEMS.len();
    let title_text = if dialog.filter.is_empty() {
        " Commands ".to_string()
    } else {
        format!(" Commands ({}/{total_count}) ", filtered.len())
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
                "Search commands",
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
            Span::styled("█", Style::default().fg(theme.amber)),
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
        vec![Line::from(Span::styled(
            format!("  No commands matching \"{}\"", dialog.filter),
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
                let selected = index == selected_idx;
                let cursor = if selected { " › " } else { "   " };
                let title_style = if selected {
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.ink)
                };
                let category = format!("  {}", item.category);
                let shortcut = if item.shortcut.is_empty() {
                    String::new()
                } else {
                    format!("{}  ", item.shortcut)
                };
                let used = Span::raw(cursor).width()
                    + Span::raw(item.title).width()
                    + Span::raw(&category).width()
                    + Span::raw(&shortcut).width();
                let gap = (chunks[2].width as usize).saturating_sub(used);
                Line::from(vec![
                    Span::styled(cursor, Style::default().fg(theme.amber)),
                    Span::styled(item.title, title_style),
                    Span::styled(category, Style::default().fg(theme.dim)),
                    Span::raw(" ".repeat(gap)),
                    Span::styled(
                        if shortcut.is_empty() {
                            String::new()
                        } else {
                            shortcut
                        },
                        Style::default().fg(theme.quiet),
                    ),
                ])
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(item_lines), chunks[2]);

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
            Span::styled("↑↓", Style::default().fg(theme.amber)),
            Span::styled(" navigate  ", Style::default().fg(theme.dim)),
            Span::styled("enter", Style::default().fg(theme.amber)),
            Span::styled(" select  ", Style::default().fg(theme.dim)),
            Span::styled("esc", Style::default().fg(theme.amber)),
            Span::styled(" close", Style::default().fg(theme.dim)),
        ])),
        chunks[4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_titles_and_categories() {
        let mut dialog = PaletteDialogState::new();
        dialog.push_char('m');
        dialog.push_char('c');
        dialog.push_char('p');
        let items = dialog.filtered_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action, PaletteAction::McpServers);

        dialog.filter.clear();
        dialog.filter.push_str("session");
        let items = dialog.filtered_items();
        assert!(
            items
                .iter()
                .any(|item| item.action == PaletteAction::SwitchSession)
        );
        assert!(
            items
                .iter()
                .any(|item| item.action == PaletteAction::OpenSessionOrProject)
        );
    }

    #[test]
    fn selection_wraps_and_preserves_valid_item() {
        let mut dialog = PaletteDialogState::new();
        dialog.previous();
        assert_eq!(dialog.selected, PALETTE_ITEMS.len() - 1);
        dialog.next();
        assert_eq!(dialog.selected, 0);
        dialog.selected = usize::MAX;
        assert_eq!(
            dialog.selected_item().map(|item| item.action),
            Some(PALETTE_ITEMS[PALETTE_ITEMS.len() - 1].action)
        );
    }
}
