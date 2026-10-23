use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::config::TuiSettingsConfig;
use crate::tui::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    Animations,
    Thinking,
    Grouping,
    Verbosity,
    Markdown,
    Tps,
    Scrollbar,
    ScrollSpeed,
    ScrollAcceleration,
    Mouse,
    LeaderTimeoutMs,
    TurnTokens,
}

impl SettingKey {
    pub const ALL: [SettingKey; 12] = [
        Self::Animations,
        Self::Thinking,
        Self::Grouping,
        Self::Verbosity,
        Self::Markdown,
        Self::Tps,
        Self::Scrollbar,
        Self::ScrollSpeed,
        Self::ScrollAcceleration,
        Self::Mouse,
        Self::LeaderTimeoutMs,
        Self::TurnTokens,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Animations => "Animations",
            Self::Thinking => "Thinking",
            Self::Grouping => "Tool grouping",
            Self::Verbosity => "Verbosity",
            Self::Markdown => "Markdown",
            Self::Tps => "Tokens per second",
            Self::Scrollbar => "Scrollbar",
            Self::ScrollSpeed => "Scroll speed",
            Self::ScrollAcceleration => "Scroll acceleration",
            Self::Mouse => "Mouse",
            Self::LeaderTimeoutMs => "Leader timeout",
            Self::TurnTokens => "Turn tokens",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::Animations | Self::Scrollbar => "Appearance",
            Self::Thinking | Self::Grouping | Self::Verbosity | Self::Markdown => "Transcript",
            Self::Tps => "Session",
            Self::ScrollSpeed | Self::ScrollAcceleration | Self::Mouse | Self::LeaderTimeoutMs => {
                "Navigation"
            }
            Self::TurnTokens => "Debug",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Animations => "Typewriter pacing and spinner motion",
            Self::Thinking => "Default visibility of reasoning blocks",
            Self::Grouping => "Fold consecutive exploration tool calls",
            Self::Verbosity => "Transcript detail level (low, medium, high)",
            Self::Markdown => "Formatted markdown or raw source",
            Self::Tps => "Show live streaming tok/s and turn rate",
            Self::Scrollbar => "Show vertical transcript scrollbar",
            Self::ScrollSpeed => "Lines scrolled per scroll step (1-20)",
            Self::ScrollAcceleration => "Accelerate repeated scroll bursts",
            Self::Mouse => "Enable mouse wheel scrolling and clicks",
            Self::LeaderTimeoutMs => "Ctrl+X chord timeout in milliseconds",
            Self::TurnTokens => "Show token breakdown in turn footer",
        }
    }

    pub fn value_label(self, settings: &TuiSettingsConfig) -> String {
        match self {
            Self::Animations => on_off(settings.animations).to_string(),
            Self::Thinking => settings.thinking.as_str().to_string(),
            Self::Grouping => settings.grouping.as_str().to_string(),
            Self::Verbosity => settings.verbosity.as_str().to_string(),
            Self::Markdown => settings.markdown.as_str().to_string(),
            Self::Tps => on_off(settings.tps).to_string(),
            Self::Scrollbar => on_off(settings.scrollbar).to_string(),
            Self::ScrollSpeed => settings.scroll_speed.to_string(),
            Self::ScrollAcceleration => on_off(settings.scroll_acceleration).to_string(),
            Self::Mouse => on_off(settings.mouse).to_string(),
            Self::LeaderTimeoutMs => format!("{}ms", settings.leader_timeout_ms),
            Self::TurnTokens => settings.turn_tokens.as_str().to_string(),
        }
    }

    pub fn cycle(self, settings: &mut TuiSettingsConfig, delta: isize) {
        match self {
            Self::Animations => settings.animations = !settings.animations,
            Self::Thinking => settings.thinking = settings.thinking.toggle(),
            Self::Grouping => settings.grouping = settings.grouping.toggle(),
            Self::Verbosity => settings.verbosity = settings.verbosity.cycle(delta),
            Self::Markdown => settings.markdown = settings.markdown.toggle(),
            Self::Tps => settings.tps = !settings.tps,
            Self::Scrollbar => settings.scrollbar = !settings.scrollbar,
            Self::ScrollSpeed => {
                let zero_based = (settings.scroll_speed.clamp(1, 20) - 1) as isize;
                settings.scroll_speed = (zero_based + delta).rem_euclid(20) as u16 + 1;
            }
            Self::ScrollAcceleration => {
                settings.scroll_acceleration = !settings.scroll_acceleration;
            }
            Self::Mouse => settings.mouse = !settings.mouse,
            Self::LeaderTimeoutMs => {
                const PRESETS: [u64; 6] = [500, 1000, 1500, 2000, 3000, 5000];
                let current = PRESETS
                    .iter()
                    .position(|value| *value == settings.leader_timeout_ms)
                    .unwrap_or(3) as isize;
                let next = (current + delta).rem_euclid(PRESETS.len() as isize) as usize;
                settings.leader_timeout_ms = PRESETS[next];
            }
            Self::TurnTokens => settings.turn_tokens = settings.turn_tokens.cycle(delta),
        }
    }
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsDialogState {
    pub filter: String,
    pub selected: usize,
    pub scroll_offset: usize,
}

impl Default for SettingsDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsDialogState {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn filtered_items(&self) -> Vec<SettingKey> {
        if self.filter.is_empty() {
            return SettingKey::ALL.to_vec();
        }
        let query = self.filter.to_ascii_lowercase();
        SettingKey::ALL
            .iter()
            .copied()
            .filter(|key| {
                key.title().to_ascii_lowercase().contains(&query)
                    || key.category().to_ascii_lowercase().contains(&query)
                    || key.description().to_ascii_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn selected_key(&self) -> Option<SettingKey> {
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

fn truncate_text(text: &str, max_width: usize) -> String {
    if Span::raw(text).width() <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut width = 0;
    for ch in text.chars() {
        let ch_width = Span::raw(ch.to_string()).width();
        if width + ch_width > max_width.saturating_sub(1) {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push('…');
    out
}

pub fn render_settings_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &SettingsDialogState,
    settings: &TuiSettingsConfig,
    theme: &Theme,
) {
    let width = area.width.clamp(44, 84).min(area.width);
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

    let total_count = SettingKey::ALL.len();
    let title_text = if dialog.filter.is_empty() {
        format!(" Settings ({total_count}) ")
    } else {
        format!(" Settings ({}/{total_count}) ", filtered.len())
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
                "type to filter settings...",
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
        vec![Line::from(Span::styled(
            format!("  No settings matching \"{}\"", dialog.filter),
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
            .map(|(rel_idx, &key)| {
                let actual_idx = scroll_offset + rel_idx;
                let selected = actual_idx == selected_idx;
                let cursor = if selected { " › " } else { "   " };
                let value_pill = format!("‹ {} › ", key.value_label(settings));
                let title_style = if selected {
                    Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.ink)
                };
                let value_style = if selected {
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.teal)
                };

                let row_width = chunks[2].width as usize;
                let cursor_w = Span::raw(cursor).width();
                let title_w = Span::raw(key.title()).width();
                let value_w = Span::raw(&value_pill).width();
                let desc_budget = row_width.saturating_sub(cursor_w + title_w + value_w + 4);
                let desc = if desc_budget >= 12 {
                    format!("  {}", truncate_text(key.description(), desc_budget))
                } else {
                    String::new()
                };
                let used = cursor_w + title_w + Span::raw(&desc).width() + value_w;
                let gap = row_width.saturating_sub(used);

                let line_style = if selected {
                    Style::default().bg(theme.bg_element)
                } else {
                    Style::default()
                };

                Line::from(vec![
                    Span::styled(cursor, Style::default().fg(theme.teal)),
                    Span::styled(key.title(), title_style),
                    Span::styled(
                        desc,
                        Style::default()
                            .fg(theme.ink)
                            .add_modifier(Modifier::ITALIC),
                    ),
                    Span::raw(" ".repeat(gap)),
                    Span::styled(value_pill, value_style),
                ])
                .style(line_style)
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(item_lines), chunks[2]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            &divider,
            Style::default().fg(theme.quiet),
        ))),
        chunks[3],
    );

    let footer = Line::from(vec![
        Span::styled(
            "↑/↓ ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled("navigate  ", Style::default().fg(theme.ink)),
        Span::styled(
            "←/→/Enter ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled("change  ", Style::default().fg(theme.ink)),
        Span::styled(
            "Esc ",
            Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
        ),
        Span::styled("close", Style::default().fg(theme.ink)),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[4]);
}
