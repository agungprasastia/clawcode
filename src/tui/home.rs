use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use super::App;
use super::render::{
    mode_label, render_command_popup, render_hints_row, render_input_card, status_label,
};
use super::theme::{Theme, darken_color};

pub const PHASE_DURATIONS: [u32; 5] = [14, 7, 7, 7, 14];
pub const PHASE_FRAMES: [usize; 5] = [0, 1, 0, 1, 0];

pub const LOGO: [&str; 6] = [
    " ██████╗██╗      █████╗ ██╗    ██╗ ██████╗ ██████╗ ██████╗ ███████╗",
    "██╔════╝██║     ██╔══██╗██║    ██║██╔════╝██╔═══██╗██╔══██╗██╔════╝",
    "██║     ██║     ███████║██║ █╗ ██║██║     ██║   ██║██║  ██║█████╗  ",
    "██║     ██║     ██╔══██║██║███╗██║██║     ██║   ██║██║  ██║██╔══╝  ",
    "╚██████╗███████╗██║  ██║╚███╔███╔╝╚██████╗╚██████╔╝██████╔╝███████╗",
    " ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝  ╚═════╝ ╚═════╝ ╚═════╝ ╚══════╝",
];

pub const MASCOT_FRAMES: [[&str; 3]; 2] = [
    ["   ▃▃▛████▜▃▃", "█▟▟▜████████▛▙▙█", "   ▞ ▘    ▝ ▚"],
    ["   ▃▃▛████▜▃▃", "█▙▟▜████████▛▙▟█", "   ▞ ▘    ▝ ▚"],
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeState {
    pub phase: u8,
    pub tick_count: u32,
}

impl Default for HomeState {
    fn default() -> Self {
        Self::new()
    }
}

impl HomeState {
    pub fn new() -> Self {
        Self {
            phase: 0,
            tick_count: 0,
        }
    }

    pub fn tick(&mut self) {
        self.tick_count += 1;
        if self.tick_count >= PHASE_DURATIONS[self.phase as usize] {
            self.tick_count = 0;
            self.phase = (self.phase + 1) % (PHASE_DURATIONS.len() as u8);
        }
    }

    pub fn frame(&self) -> usize {
        PHASE_FRAMES[self.phase as usize]
    }
}

pub fn render_home(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme, mode_color: Color) {
    if area.height < 14 || area.width < 50 {
        render_compact_home(frame, area, app, theme, mode_color);
        return;
    }

    let input_height = 5;
    let hints_height = 1;

    // Match Crabcode dock layout: top canvas stretches, input card & hints dock at bottom
    let home_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(input_height),
            Constraint::Length(hints_height),
            Constraint::Length(1),
        ])
        .split(area);

    let top_canvas = home_chunks[0];
    let input_area = home_chunks[1];
    let hints_area = home_chunks[2];

    let show_big_logo = top_canvas.height >= 11 && area.width >= 70;
    let show_cards = top_canvas.height >= 15 && area.width >= 70;

    let hero_height = if show_big_logo { 10 } else { 3 };
    let cards_height = if show_cards { 4 } else { 0 };
    let gap_height = if show_cards { 1 } else { 0 };
    let content_height = hero_height + gap_height + cards_height;

    // Center logo and quick action cards vertically within the top canvas
    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(content_height),
            Constraint::Min(0),
        ])
        .split(top_canvas);

    let hero_cards_canvas = v_chunks[1];
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(hero_height),
            Constraint::Length(gap_height),
            Constraint::Length(cards_height),
        ])
        .split(hero_cards_canvas);

    let hero_area = inner_chunks[0];
    let cards_area = inner_chunks[2];

    let content_width = if area.width >= 106 {
        100
    } else {
        area.width.saturating_sub(4)
    };
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(content_width),
            Constraint::Min(0),
        ])
        .split(area);
    let centered_x = h_chunks[1].x;
    let centered_w = h_chunks[1].width;

    render_hero(
        frame,
        hero_area,
        show_big_logo,
        app.home_state().frame(),
        theme,
    );
    if show_cards {
        let centered_cards = Rect {
            x: centered_x,
            y: cards_area.y,
            width: centered_w,
            height: cards_area.height,
        };
        let cards = render_quick_actions(frame, centered_cards, theme);
        if cards.len() >= 4 {
            app.set_last_quick_actions_area(Some([cards[0], cards[1], cards[2], cards[3]]));
        }
    } else {
        app.set_last_quick_actions_area(None);
    }

    let centered_input = Rect {
        x: centered_x,
        y: input_area.y,
        width: centered_w,
        height: input_area.height,
    };
    render_input_card(frame, centered_input, app, theme, mode_color);
    render_command_popup(frame, centered_input, app, theme);

    let centered_hints = Rect {
        x: centered_x,
        y: hints_area.y,
        width: centered_w,
        height: hints_area.height,
    };
    render_hints_row(frame, centered_hints, app, theme);
}

pub fn render_compact_home(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: &Theme,
    mode_color: Color,
) {
    app.set_last_quick_actions_area(None);
    if area.width == 0 || area.height == 0 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(area);

    let brand_line = Line::from(vec![
        Span::styled(
            " CLAWCODE ",
            Style::default()
                .fg(theme.amber)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("[{}]", mode_label(app)),
            Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(status_label(app), Style::default().fg(theme.quiet)),
        Span::raw(" "),
        Span::styled("clawcode v0.1.0", Style::default().fg(theme.dim)),
    ]);
    frame.render_widget(Paragraph::new(brand_line), chunks[0]);

    render_input_card(frame, chunks[2], app, theme, mode_color);
    render_command_popup(frame, chunks[2], app, theme);
    render_hints_row(frame, chunks[3], app, theme);
}

pub fn render_hero(
    frame: &mut Frame<'_>,
    area: Rect,
    show_big_logo: bool,
    mascot_frame: usize,
    theme: &Theme,
) {
    if show_big_logo {
        let hero_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(6),
                Constraint::Length(1),
            ])
            .split(area);

        let mascot = MASCOT_FRAMES[mascot_frame % 2];
        let mascot_lines: Vec<Line> = mascot
            .iter()
            .map(|l| {
                Line::from(Span::styled(
                    *l,
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD),
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(mascot_lines).alignment(Alignment::Center),
            hero_chunks[0],
        );

        let logo_lines: Vec<Line> = LOGO
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let color = if i == 5 {
                    darken_color(theme.amber, 0.7)
                } else {
                    theme.amber
                };
                Line::from(Span::styled(
                    *l,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(logo_lines).alignment(Alignment::Center),
            hero_chunks[1],
        );

        let subtitle = Line::from(vec![
            Span::styled(
                "CLAWCODE",
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  ", Style::default().fg(theme.panel)),
            Span::styled(
                "AUTONOMOUS AGENT WORKBENCH",
                Style::default().fg(theme.quiet),
            ),
            Span::styled("  ·  ", Style::default().fg(theme.panel)),
            Span::styled("clawcode v0.1.0", Style::default().fg(theme.dim)),
        ]);
        frame.render_widget(
            Paragraph::new(subtitle).alignment(Alignment::Center),
            hero_chunks[2],
        );
    } else {
        let mascot = MASCOT_FRAMES[mascot_frame % 2];
        let logo_lines = vec![
            Line::from(Span::styled(
                mascot[1],
                Style::default()
                    .fg(theme.amber)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled(
                    " CLAWCODE ",
                    Style::default()
                        .fg(theme.amber)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "· AUTONOMOUS AGENT WORKBENCH",
                    Style::default().fg(theme.quiet),
                ),
                Span::styled(" · clawcode v0.1.0", Style::default().fg(theme.dim)),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(logo_lines).alignment(Alignment::Center),
            area,
        );
    }
}

pub fn render_quick_actions(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
) -> std::rc::Rc<[Rect]> {
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .spacing(1)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(area);

    let actions = [
        ("/plan", "Plan & Analyze", "Read-only plan", theme.amber),
        ("/build", "Execute & Edit", "Verified edits", theme.teal),
        ("/models", "Models & LLMs", "Providers & IDs", theme.ink),
        ("/help", "Manual & Keys", "Commands & tips", theme.quiet),
    ];

    for (i, &(cmd, title, desc, col)) in actions.iter().enumerate() {
        if i >= cards.len() {
            break;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.panel))
            .style(Style::default().bg(theme.bg_element))
            .title(Span::styled(
                format!(" {cmd} "),
                Style::default().fg(col).add_modifier(Modifier::BOLD),
            ));

        let content = vec![
            Line::from(Span::styled(
                format!(" {title}"),
                Style::default().fg(theme.ink),
            )),
            Line::from(Span::styled(
                format!(" {desc}"),
                Style::default().fg(theme.dim),
            )),
        ];

        frame.render_widget(Paragraph::new(content).block(block), cards[i]);
    }
    cards
}

pub fn compute_quick_actions_area_for_size(width: u16, height: u16) -> Option<[Rect; 4]> {
    let workspace_area = Rect {
        x: 0,
        y: 0,
        width,
        height: height.saturating_sub(1),
    };
    if workspace_area.width < 70 || workspace_area.height < 18 {
        return None;
    }
    let input_height = 5.min(workspace_area.height.saturating_sub(6));
    let hints_height = 1;
    let home_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(input_height),
            Constraint::Length(hints_height),
            Constraint::Length(1),
        ])
        .split(workspace_area);

    let top_canvas = home_chunks[0];
    let show_big_logo = top_canvas.height >= 11 && workspace_area.width >= 70;
    let show_cards = top_canvas.height >= 15 && workspace_area.width >= 70;
    if !show_cards {
        return None;
    }

    let hero_height = if show_big_logo { 10 } else { 3 };
    let cards_height = 4;
    let gap_height = 1;
    let content_height = hero_height + gap_height + cards_height;

    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(content_height),
            Constraint::Min(0),
        ])
        .split(top_canvas);

    let hero_cards_canvas = v_chunks[1];
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(hero_height),
            Constraint::Length(gap_height),
            Constraint::Length(cards_height),
        ])
        .split(hero_cards_canvas);

    let cards_area = inner_chunks[2];

    let content_width = if workspace_area.width >= 106 {
        100
    } else {
        workspace_area.width.saturating_sub(4)
    };
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(content_width),
            Constraint::Min(0),
        ])
        .split(workspace_area);
    let centered_x = h_chunks[1].x;
    let centered_w = h_chunks[1].width;

    let centered_cards = Rect {
        x: centered_x,
        y: cards_area.y,
        width: centered_w,
        height: cards_area.height,
    };
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .spacing(1)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(centered_cards);

    if cards.len() >= 4 {
        Some([cards[0], cards[1], cards[2], cards[3]])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_home_state_new() {
        let state = HomeState::new();
        assert_eq!(state.phase, 0);
        assert_eq!(state.tick_count, 0);
        assert_eq!(state.frame(), 0);
    }

    #[test]
    fn test_home_state_default() {
        let state = HomeState::default();
        assert_eq!(state, HomeState::new());
        assert_eq!(state.phase, 0);
        assert_eq!(state.tick_count, 0);
        assert_eq!(state.frame(), 0);
    }

    #[test]
    fn test_home_state_tick() {
        let mut state = HomeState::new();
        state.tick();
        assert_eq!(state.phase, 0);
        assert_eq!(state.tick_count, 1);
        assert_eq!(state.frame(), 0);
    }

    #[test]
    fn test_home_state_frame() {
        let mut state = HomeState::new();
        assert_eq!(state.frame(), PHASE_FRAMES[0]);
        state.phase = 1;
        assert_eq!(state.frame(), PHASE_FRAMES[1]);
        state.phase = 2;
        assert_eq!(state.frame(), PHASE_FRAMES[2]);
        state.phase = 3;
        assert_eq!(state.frame(), PHASE_FRAMES[3]);
        state.phase = 4;
        assert_eq!(state.frame(), PHASE_FRAMES[4]);
    }

    #[test]
    fn test_home_state_duration_cycle() {
        let mut state = HomeState::new();
        assert_eq!(state.phase, 0);
        assert_eq!(state.frame(), 0);

        for expected_phase in 0..PHASE_DURATIONS.len() {
            assert_eq!(state.phase, expected_phase as u8);
            let duration = PHASE_DURATIONS[expected_phase];
            let expected_frame = PHASE_FRAMES[expected_phase];

            for t in 0..duration {
                assert_eq!(state.tick_count, t);
                assert_eq!(state.frame(), expected_frame);
                state.tick();
            }
        }

        // After full cycle, wraps back to Phase 0
        assert_eq!(state.phase, 0);
        assert_eq!(state.tick_count, 0);
        assert_eq!(state.frame(), 0);
    }
}
