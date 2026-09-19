use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::persistence::{Session, SessionStatus};
use crate::tui::{App, Theme};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionsDialogState {
    pub items: Vec<Session>,
    pub selected: usize,
    pub filter: String,
    pub scroll_offset: usize,
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
    for ch in text.chars() {
        let char_width = Span::raw(ch.to_string()).width();
        if width + char_width > max_width.saturating_sub(1) {
            break;
        }
        result.push(ch);
        width += char_width;
    }
    result.push('…');
    result
}

impl SessionsDialogState {
    pub fn new(items: Vec<Session>, active_session_id: Option<i64>) -> Self {
        let selected = active_session_id
            .and_then(|id| items.iter().position(|s| s.id == id))
            .unwrap_or(0);
        Self {
            items,
            selected,
            filter: String::new(),
            scroll_offset: 0,
        }
    }

    pub fn filtered_items(&self) -> Vec<&Session> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let q = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|s| {
                    s.title.to_lowercase().contains(&q)
                        || s.id.to_string().contains(&q)
                        || format!("ws#{}", s.workspace_id).to_lowercase().contains(&q)
                })
                .collect()
        }
    }

    pub fn selected_session(&self) -> Option<&Session> {
        let filtered = self.filtered_items();
        if filtered.is_empty() {
            return None;
        }
        let idx = self.selected.min(filtered.len() - 1);
        filtered.get(idx).copied()
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

    pub fn remove_item(&mut self, id: i64) {
        self.items.retain(|s| s.id != id);
        let count = self.filtered_items().len();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }
}

/// Overlay panel listing all sessions grouped by workspace. Pinned sessions
/// first within each group; running sessions show a spinner glyph.
pub fn render_sessions_panel(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    const SPINNER: [&str; 7] = ["·", "✻", "✽", "✶", "✳", "✢", "*"];

    let sessions = app.session_listings();
    let group_count = sessions
        .iter()
        .map(|session| session.workspace_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    // Borders + one header line per workspace + one line per session.
    let height = (2 + group_count + sessions.len()).min(area.height as usize);
    let width = area.width.min(70);

    let rows: Vec<Line> = sessions
        .iter()
        .scan(None::<i64>, |last_workspace, session| {
            let new_group = *last_workspace != Some(session.workspace_id);
            *last_workspace = Some(session.workspace_id);
            Some((new_group, session))
        })
        .flat_map(|(new_group, session)| {
            let glyph = if session.status == SessionStatus::Running {
                SPINNER[(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() / 120)
                    .unwrap_or(0) as usize)
                    % SPINNER.len()]
            } else {
                " "
            };
            let mut lines = Vec::new();
            if new_group {
                lines.push(Line::from(Span::styled(
                    format!("  ws#{}", session.workspace_id),
                    Style::new().fg(theme.quiet),
                )));
            }
            let title = if session.pinned { "📌 " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(title, Style::new().fg(theme.amber)),
                Span::raw(&session.title),
                if session.status == SessionStatus::Running {
                    Span::styled(format!("  {glyph}"), Style::new().fg(theme.teal))
                } else {
                    Span::raw("")
                },
            ]));
            lines
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.panel))
        .title(Span::styled(
            " Sessions ",
            Style::new().fg(theme.ink).bold(),
        ));

    let paragraph = Paragraph::new(rows)
        .block(block)
        .style(Style::new().bg(theme.bg_element));
    let panel_area = Rect::new(
        area.width.saturating_sub(width) / 2,
        1,
        width,
        height.min(area.height.saturating_sub(2) as usize) as u16,
    );
    frame.render_widget(Clear, panel_area);
    frame.render_widget(paragraph, panel_area);
}

pub fn render_sessions_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &SessionsDialogState,
    active_session_id: Option<i64>,
    theme: &Theme,
) {
    let width = area.width.clamp(40, 78).min(area.width);
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
    let filtered_count = filtered.len();
    let title_text = if dialog.filter.is_empty() {
        format!(" Sessions ({total_count}) ")
    } else {
        format!(" Sessions ({filtered_count}/{total_count}) ")
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
            Constraint::Length(1), // search input
            Constraint::Length(1), // top divider
            Constraint::Min(1),    // items
            Constraint::Length(1), // bottom divider
            Constraint::Length(1), // footer hints
        ])
        .split(inner);

    let search_line = if dialog.filter.is_empty() {
        Line::from(vec![
            Span::styled("Search: ", Style::default().fg(theme.dim)),
            Span::styled(
                "type to filter sessions...",
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

    let divider_str = "─".repeat(chunks[1].width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider_str,
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
            format!("  No sessions matching \"{}\"", dialog.filter),
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
            .map(|(rel_idx, session)| {
                let actual_idx = scroll_offset + rel_idx;
                let is_selected = actual_idx == selected_idx;
                let is_active = Some(session.id) == active_session_id;

                let cursor = if is_selected { " › " } else { "   " };
                let active_dot = if is_active { "● " } else { "  " };

                let cursor_style = if is_selected {
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
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
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
                } else if is_active {
                    Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.ink)
                };

                let badge = if is_active {
                    "[active]".to_string()
                } else if session.status == SessionStatus::Running {
                    "[running]".to_string()
                } else if session.pinned {
                    "[pinned]".to_string()
                } else {
                    String::new()
                };

                let badge_style = if is_active {
                    Style::default()
                        .fg(theme.success)
                        .add_modifier(Modifier::BOLD)
                } else if session.status == SessionStatus::Running {
                    Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.amber)
                };

                let pin_icon = if session.pinned { "★ " } else { "" };
                let ws_tag = format!("ws#{} ", session.workspace_id);
                let id_tag = format!("#{} ", session.id);
                let title_prefix = format!("{ws_tag}{id_tag}{pin_icon}");
                let row_width = chunks[2].width as usize;
                let prefix_len = Span::raw(format!("{cursor}{active_dot}{title_prefix}")).width();
                let badge_len = Span::raw(&badge).width();
                let available_for_title = row_width.saturating_sub(prefix_len + badge_len + 2);

                let display_title = truncate_with_ellipsis(&session.title, available_for_title);

                let display_full = format!("{title_prefix}{display_title}");
                let pad_len = row_width.saturating_sub(
                    Span::raw(format!("{cursor}{active_dot}{display_full}")).width() + badge_len,
                );

                let line_style = if is_selected {
                    Style::default().bg(theme.bg_element)
                } else {
                    Style::default()
                };

                Line::from(vec![
                    Span::styled(cursor, cursor_style),
                    Span::styled(active_dot, dot_style),
                    Span::styled(display_full, name_style),
                    Span::raw(" ".repeat(pad_len)),
                    Span::styled(badge, badge_style),
                ])
                .style(line_style)
            })
            .collect()
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
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("navigate  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Enter ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("switch  ", Style::default().fg(theme.dim)),
        Span::styled(
            "d ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("delete  ", Style::default().fg(theme.dim)),
        Span::styled(
            "Esc ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("close", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(footer_line), chunks[4]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_sessions() -> Vec<Session> {
        vec![
            Session {
                id: 1,
                title: "Rust refactor".to_string(),
                workspace_id: 10,
                status: SessionStatus::Running,
                pinned: true,
            },
            Session {
                id: 2,
                title: "UI bugfix".to_string(),
                workspace_id: 10,
                status: SessionStatus::Idle,
                pinned: false,
            },
            Session {
                id: 3,
                title: "Documentation".to_string(),
                workspace_id: 20,
                status: SessionStatus::Idle,
                pinned: false,
            },
        ]
    }

    #[test]
    fn test_sessions_dialog_state_new() {
        let sessions = sample_sessions();
        let state = SessionsDialogState::new(sessions.clone(), Some(2));
        assert_eq!(state.selected, 1);
        assert!(state.filter.is_empty());
        assert_eq!(state.scroll_offset, 0);
        assert_eq!(state.items.len(), 3);

        let fallback = SessionsDialogState::new(sessions, Some(999));
        assert_eq!(fallback.selected, 0);

        let empty = SessionsDialogState::new(Vec::new(), None);
        assert_eq!(empty.selected, 0);
        assert!(empty.items.is_empty());
    }

    #[test]
    fn test_sessions_dialog_state_push_pop_char() {
        let mut state = SessionsDialogState::new(sample_sessions(), Some(1));
        state.selected = 2;
        state.scroll_offset = 1;

        state.push_char('u');
        state.push_char('i');
        assert_eq!(state.filter, "ui");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "u");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "");
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);

        state.pop_char();
        assert_eq!(state.filter, "");
    }

    #[test]
    fn test_sessions_dialog_state_navigation() {
        let mut state = SessionsDialogState::new(sample_sessions(), Some(1));
        assert_eq!(state.selected, 0);

        state.next();
        assert_eq!(state.selected, 1);

        state.next();
        assert_eq!(state.selected, 2);

        state.next();
        assert_eq!(state.selected, 0);

        state.previous();
        assert_eq!(state.selected, 2);

        state.previous();
        assert_eq!(state.selected, 1);

        let mut empty = SessionsDialogState::new(Vec::new(), None);
        empty.next();
        assert_eq!(empty.selected, 0);
        empty.previous();
        assert_eq!(empty.selected, 0);
    }

    #[test]
    fn test_sessions_dialog_state_filtered_items() {
        let mut state = SessionsDialogState::new(sample_sessions(), Some(1));
        assert_eq!(state.filtered_items().len(), 3);

        state.push_char('R');
        state.push_char('U');
        state.push_char('S');
        state.push_char('T');
        let filtered = state.filtered_items();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, 1);

        state.filter = "3".to_string();
        let filtered_id = state.filtered_items();
        assert_eq!(filtered_id.len(), 1);
        assert_eq!(filtered_id[0].title, "Documentation");

        state.filter = "ws#20".to_string();
        let filtered_ws = state.filtered_items();
        assert_eq!(filtered_ws.len(), 1);
        assert_eq!(filtered_ws[0].id, 3);

        state.filter = "nomatch_xyz".to_string();
        assert!(state.filtered_items().is_empty());
    }

    #[test]
    fn test_sessions_dialog_state_selected_session() {
        let mut state = SessionsDialogState::new(sample_sessions(), Some(2));
        assert_eq!(state.selected_session().map(|s| s.id), Some(2));

        state.filter = "nonexistent".to_string();
        assert_eq!(state.selected_session(), None);
    }

    #[test]
    fn test_sessions_dialog_state_remove_item() {
        let mut state = SessionsDialogState::new(sample_sessions(), Some(3));
        assert_eq!(state.selected, 2);

        state.remove_item(3);
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.selected, 1);
        assert_eq!(state.selected_session().map(|s| s.id), Some(2));

        state.remove_item(1);
        assert_eq!(state.items.len(), 1);
        assert_eq!(state.selected, 0);
        assert_eq!(state.selected_session().map(|s| s.id), Some(2));

        state.remove_item(2);
        assert_eq!(state.items.len(), 0);
        assert_eq!(state.selected, 0);
        assert_eq!(state.selected_session(), None);
    }
    #[test]
    fn render_sessions_dialog_truncates_multibyte_title_at_narrow_width() {
        let dialog = SessionsDialogState::new(
            vec![Session {
                id: 1,
                title: "界".repeat(20),
                workspace_id: 1,
                status: SessionStatus::Idle,
                pinned: false,
            }],
            None,
        );
        let theme = Theme::new();
        let backend = ratatui::backend::TestBackend::new(40, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_sessions_dialog(frame, frame.area(), &dialog, None, &theme))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("…"));
        assert!(Span::raw(truncate_with_ellipsis(&"界".repeat(20), 10)).width() <= 10);
        for y in 0..buffer.area.height {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            assert!(line.chars().count() <= buffer.area.width as usize);
        }
    }
}
