use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::tui::Theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionPrompt {
    pub tool_name: String,
    pub action_desc: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Deny,
    AllowOnce,
    AllowAlways,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDialogState {
    pub prompt: Option<PermissionPrompt>,
    pub selected_decision: usize, // 0 = Deny, 1 = AllowOnce, 2 = AllowAlways
}

impl Default for PermissionDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionDialogState {
    pub fn new() -> Self {
        Self {
            prompt: None,
            selected_decision: 1,
        }
    }

    pub fn with_prompt(
        tool_name: impl Into<String>,
        action_desc: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            prompt: Some(PermissionPrompt {
                tool_name: tool_name.into(),
                action_desc: action_desc.into(),
                reason: reason.into(),
            }),
            selected_decision: 1,
        }
    }

    pub fn next(&mut self) {
        self.selected_decision = (self.selected_decision + 1) % 3;
    }

    pub fn previous(&mut self) {
        self.selected_decision = if self.selected_decision == 0 {
            2
        } else {
            self.selected_decision - 1
        };
    }

    pub fn selected(&self) -> PermissionDecision {
        match self.selected_decision % 3 {
            0 => PermissionDecision::Deny,
            1 => PermissionDecision::AllowOnce,
            _ => PermissionDecision::AllowAlways,
        }
    }

    pub fn is_active(&self) -> bool {
        self.prompt.is_some()
    }

    pub fn clear(&mut self) {
        self.prompt = None;
        self.selected_decision = 1;
    }

    pub fn show(
        &mut self,
        tool_name: impl Into<String>,
        action_desc: impl Into<String>,
        reason: impl Into<String>,
    ) {
        self.prompt = Some(PermissionPrompt {
            tool_name: tool_name.into(),
            action_desc: action_desc.into(),
            reason: reason.into(),
        });
        self.selected_decision = 1;
    }

    pub fn show_prompt(&mut self, prompt: PermissionPrompt) {
        self.prompt = Some(prompt);
        self.selected_decision = 1;
    }
}

pub fn render_permission_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &PermissionDialogState,
    theme: &Theme,
) {
    if area.width < 10 || area.height < 6 {
        return;
    }

    let width = area.width.clamp(50, 70).min(area.width);
    let height = 11.min(area.height);

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let border_color = theme.amber;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            " Security Confirmation ",
            Style::default()
                .fg(border_color)
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
            Constraint::Length(1), // Tool header with ⚠
            Constraint::Length(2), // Action description
            Constraint::Length(1), // Reason
            Constraint::Min(0),    // Spacer
            Constraint::Length(1), // Buttons
            Constraint::Length(1), // Footer hints
        ])
        .split(inner);

    let (tool_name, action_desc, reason) = match &dialog.prompt {
        Some(p) => (p.tool_name.as_str(), p.action_desc.as_str(), p.reason.as_str()),
        None => ("unknown", "No pending action description", "No reason provided"),
    };

    // Tool line
    let tool_line = Line::from(vec![
        Span::styled("⚠  Tool: ", Style::default().fg(theme.amber).add_modifier(Modifier::BOLD)),
        Span::styled(tool_name, Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)),
    ]);
    frame.render_widget(Paragraph::new(tool_line), chunks[0]);

    // Action description
    let action_line = Line::from(vec![
        Span::styled("Action: ", Style::default().fg(theme.dim)),
        Span::styled(action_desc, Style::default().fg(theme.ink)),
    ]);
    let action_widget = Paragraph::new(action_line).wrap(Wrap { trim: true });
    frame.render_widget(action_widget, chunks[1]);

    // Reason
    let reason_line = Line::from(vec![
        Span::styled("Reason: ", Style::default().fg(theme.dim)),
        Span::styled(reason, Style::default().fg(theme.quiet)),
    ]);
    let reason_widget = Paragraph::new(reason_line).wrap(Wrap { trim: true });
    frame.render_widget(reason_widget, chunks[2]);

    // Buttons: 0 = Deny, 1 = Allow Once, 2 = Always Allow
    let deny_style = if dialog.selected_decision == 0 {
        Style::default()
            .fg(theme.bg_element)
            .bg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.dim)
    };

    let once_style = if dialog.selected_decision == 1 {
        Style::default()
            .fg(theme.bg_element)
            .bg(theme.amber)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.dim)
    };

    let always_style = if dialog.selected_decision == 2 {
        Style::default()
            .fg(theme.bg_element)
            .bg(theme.teal)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.dim)
    };

    let buttons_line = Line::from(vec![
        Span::styled("[ Deny ]", deny_style),
        Span::raw("    "),
        Span::styled("[ Allow Once ]", once_style),
        Span::raw("    "),
        Span::styled("[ Always Allow ]", always_style),
    ]);
    let buttons_widget = Paragraph::new(buttons_line).alignment(Alignment::Center);
    frame.render_widget(buttons_widget, chunks[4]);

    // Footer hints
    let footer_line = Line::from(vec![
        Span::styled("←/→ or Tab to navigate", Style::default().fg(theme.dim)),
        Span::styled(" · ", Style::default().fg(theme.quiet)),
        Span::styled("Enter to confirm", Style::default().fg(theme.dim)),
        Span::styled(" · ", Style::default().fg(theme.quiet)),
        Span::styled("Esc to deny", Style::default().fg(theme.dim)),
    ]);
    let footer_widget = Paragraph::new(footer_line).alignment(Alignment::Center);
    frame.render_widget(footer_widget, chunks[5]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_permission_dialog_state_default_and_new() {
        let state_new = PermissionDialogState::new();
        assert_eq!(state_new.prompt, None);
        assert_eq!(state_new.selected_decision, 1);
        assert_eq!(state_new.selected(), PermissionDecision::AllowOnce);
        assert!(!state_new.is_active());

        let state_default = PermissionDialogState::default();
        assert_eq!(state_default.prompt, None);
        assert_eq!(state_default.selected_decision, 1);
        assert_eq!(state_default.selected(), PermissionDecision::AllowOnce);
        assert!(!state_default.is_active());
    }

    #[test]
    fn test_permission_dialog_state_with_prompt() {
        let state = PermissionDialogState::with_prompt(
            "bash",
            "rm -rf target",
            "Clean build directory",
        );
        assert!(state.is_active());
        assert_eq!(state.selected_decision, 1);
        assert_eq!(state.selected(), PermissionDecision::AllowOnce);

        let prompt = state.prompt.as_ref().expect("prompt should be present");
        assert_eq!(prompt.tool_name, "bash");
        assert_eq!(prompt.action_desc, "rm -rf target");
        assert_eq!(prompt.reason, "Clean build directory");
    }

    #[test]
    fn test_permission_dialog_state_navigation() {
        let mut state = PermissionDialogState::new();
        assert_eq!(state.selected_decision, 1);
        assert_eq!(state.selected(), PermissionDecision::AllowOnce);

        // Next moves right: 1 -> 2 (AllowAlways)
        state.next();
        assert_eq!(state.selected_decision, 2);
        assert_eq!(state.selected(), PermissionDecision::AllowAlways);

        // Next wraps around: 2 -> 0 (Deny)
        state.next();
        assert_eq!(state.selected_decision, 0);
        assert_eq!(state.selected(), PermissionDecision::Deny);

        // Next moves right: 0 -> 1 (AllowOnce)
        state.next();
        assert_eq!(state.selected_decision, 1);
        assert_eq!(state.selected(), PermissionDecision::AllowOnce);

        // Previous moves left: 1 -> 0 (Deny)
        state.previous();
        assert_eq!(state.selected_decision, 0);
        assert_eq!(state.selected(), PermissionDecision::Deny);

        // Previous wraps around: 0 -> 2 (AllowAlways)
        state.previous();
        assert_eq!(state.selected_decision, 2);
        assert_eq!(state.selected(), PermissionDecision::AllowAlways);

        // Previous moves left: 2 -> 1 (AllowOnce)
        state.previous();
        assert_eq!(state.selected_decision, 1);
        assert_eq!(state.selected(), PermissionDecision::AllowOnce);
    }

    #[test]
    fn test_permission_dialog_state_show_and_clear() {
        let mut state = PermissionDialogState::new();
        assert!(!state.is_active());

        state.show("edit_file", "edit src/main.rs", "Modify entrypoint");
        assert!(state.is_active());
        assert_eq!(state.selected(), PermissionDecision::AllowOnce);

        state.next();
        assert_eq!(state.selected(), PermissionDecision::AllowAlways);

        state.clear();
        assert!(!state.is_active());
        assert_eq!(state.prompt, None);
        assert_eq!(state.selected_decision, 1);
        assert_eq!(state.selected(), PermissionDecision::AllowOnce);

        state.show_prompt(PermissionPrompt {
            tool_name: "write_file".to_string(),
            action_desc: "create config.toml".to_string(),
            reason: "Setup initial configuration".to_string(),
        });
        assert!(state.is_active());
        assert_eq!(state.selected(), PermissionDecision::AllowOnce);
    }

    #[test]
    fn test_permission_dialog_state_decision_values() {
        let mut state = PermissionDialogState::new();
        state.selected_decision = 0;
        assert_eq!(state.selected(), PermissionDecision::Deny);

        state.selected_decision = 1;
        assert_eq!(state.selected(), PermissionDecision::AllowOnce);

        state.selected_decision = 2;
        assert_eq!(state.selected(), PermissionDecision::AllowAlways);
    }

    #[test]
    fn test_render_permission_dialog() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::new();
        let state = PermissionDialogState::with_prompt(
            "bash",
            "rm -rf /tmp/test",
            "Remove temporary files",
        );

        terminal
            .draw(|f| {
                render_permission_dialog(f, f.area(), &state, &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("Security Confirmation"));
        assert!(content.contains("bash"));
        assert!(content.contains("rm -rf /tmp/test"));
        assert!(content.contains("Remove temporary files"));
        assert!(content.contains("[ Deny ]"));
        assert!(content.contains("[ Allow Once ]"));
        assert!(content.contains("[ Always Allow ]"));
        assert!(content.contains("←/→ or Tab to navigate"));
        assert!(content.contains("Enter to confirm"));
        assert!(content.contains("Esc to deny"));
    }
}
