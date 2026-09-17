use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::tui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionDialogState {
    pub question: String,
    pub options: Vec<String>,
    pub selected_option: usize,
    pub custom_answer: String,
    pub typing_custom: bool,
}

impl Default for QuestionDialogState {
    fn default() -> Self {
        Self::new("", Vec::new())
    }
}

impl QuestionDialogState {
    pub fn new(question: &str, options: Vec<String>) -> Self {
        let typing_custom = options.is_empty();
        Self {
            question: question.to_string(),
            options,
            selected_option: 0,
            custom_answer: String::new(),
            typing_custom,
        }
    }

    pub fn next(&mut self) {
        let total_count = self.options.len() + 1;
        self.selected_option = (self.selected_option + 1) % total_count;
        self.typing_custom = self.selected_option == self.options.len();
    }

    pub fn previous(&mut self) {
        let total_count = self.options.len() + 1;
        self.selected_option = if self.selected_option == 0 || self.selected_option >= total_count {
            total_count - 1
        } else {
            self.selected_option - 1
        };
        self.typing_custom = self.selected_option == self.options.len();
    }

    pub fn push_char(&mut self, c: char) {
        self.custom_answer.push(c);
        self.selected_option = self.options.len();
        self.typing_custom = true;
    }

    pub fn pop_char(&mut self) {
        self.custom_answer.pop();
        self.selected_option = self.options.len();
        self.typing_custom = true;
    }

    pub fn selected_answer(&self) -> String {
        if let Some(opt) = self.options.get(self.selected_option) {
            opt.clone()
        } else {
            self.custom_answer.clone()
        }
    }

    pub fn is_active(&self) -> bool {
        !self.question.is_empty() || !self.options.is_empty()
    }

    pub fn clear(&mut self) {
        self.question.clear();
        self.options.clear();
        self.selected_option = 0;
        self.custom_answer.clear();
        self.typing_custom = false;
    }
}

pub fn render_question_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &QuestionDialogState,
    theme: &Theme,
) {
    if area.width < 10 || area.height < 6 {
        return;
    }

    let width = area.width.clamp(50, 70).min(area.width);
    let opt_count = (dialog.options.len() as u16) + 1;
    let height = (opt_count + 7).clamp(9, 18).min(area.height);

    let dialog_area = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, dialog_area);

    let border_color = theme.teal;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            " Question from Agent ",
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
            Constraint::Length(2), // Question text
            Constraint::Min(opt_count), // Options list
            Constraint::Length(1), // Footer hints
        ])
        .split(inner);

    // Question
    let question_line = Line::from(vec![
        Span::styled("? ", Style::default().fg(theme.amber).add_modifier(Modifier::BOLD)),
        Span::styled(
            if dialog.question.is_empty() {
                "Agent asked a question:"
            } else {
                &dialog.question
            },
            Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
        ),
    ]);
    let question_widget = Paragraph::new(question_line).wrap(Wrap { trim: true });
    frame.render_widget(question_widget, chunks[0]);

    // Options list
    let mut option_lines = Vec::new();
    for (i, opt) in dialog.options.iter().enumerate() {
        let is_selected = i == dialog.selected_option;
        let line = if is_selected {
            Line::from(vec![
                Span::styled(" › (•) ", Style::default().fg(theme.teal).add_modifier(Modifier::BOLD)),
                Span::styled(opt, Style::default().fg(theme.ink).add_modifier(Modifier::BOLD)),
            ])
        } else {
            Line::from(vec![
                Span::styled("   ( ) ", Style::default().fg(theme.dim)),
                Span::styled(opt, Style::default().fg(theme.quiet)),
            ])
        };
        option_lines.push(line);
    }

    // Custom option
    let is_custom_selected = dialog.selected_option == dialog.options.len();
    let custom_line = if is_custom_selected {
        let display_text = if dialog.custom_answer.is_empty() {
            Span::styled("[input]", Style::default().fg(theme.dim))
        } else {
            Span::styled(
                &dialog.custom_answer,
                Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
            )
        };
        Line::from(vec![
            Span::styled(
                " › (•) Custom text: ",
                Style::default().fg(theme.teal).add_modifier(Modifier::BOLD),
            ),
            display_text,
            Span::styled("█", Style::default().fg(theme.teal)),
        ])
    } else {
        let display_text = if dialog.custom_answer.is_empty() {
            Span::styled("[input]", Style::default().fg(theme.dim))
        } else {
            Span::styled(&dialog.custom_answer, Style::default().fg(theme.quiet))
        };
        Line::from(vec![
            Span::styled("   ( ) Custom text: ", Style::default().fg(theme.dim)),
            display_text,
        ])
    };
    option_lines.push(custom_line);

    let options_widget = Paragraph::new(option_lines);
    frame.render_widget(options_widget, chunks[1]);

    // Footer hints: `↑/↓ select · Type custom answer · Enter submit · Esc dismiss`
    let footer_line = Line::from(vec![
        Span::styled("↑/↓ select", Style::default().fg(theme.dim)),
        Span::styled(" · ", Style::default().fg(theme.quiet)),
        Span::styled("Type custom answer", Style::default().fg(theme.dim)),
        Span::styled(" · ", Style::default().fg(theme.quiet)),
        Span::styled("Enter submit", Style::default().fg(theme.dim)),
        Span::styled(" · ", Style::default().fg(theme.quiet)),
        Span::styled("Esc dismiss", Style::default().fg(theme.dim)),
    ]);
    let footer_widget = Paragraph::new(footer_line).alignment(Alignment::Center);
    frame.render_widget(footer_widget, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_question_dialog_state_new() {
        let dialog = QuestionDialogState::new(
            "Which database adapter should be used?",
            vec!["PostgreSQL".to_string(), "SQLite".to_string()],
        );
        assert_eq!(dialog.question, "Which database adapter should be used?");
        assert_eq!(dialog.options.len(), 2);
        assert_eq!(dialog.selected_option, 0);
        assert_eq!(dialog.custom_answer, "");
        assert!(!dialog.typing_custom);
        assert!(dialog.is_active());

        // Empty options defaults to custom answer typing
        let empty_opts = QuestionDialogState::new("What is your name?", Vec::new());
        assert_eq!(empty_opts.options.len(), 0);
        assert_eq!(empty_opts.selected_option, 0);
        assert!(empty_opts.typing_custom);
        assert!(empty_opts.is_active());
    }

    #[test]
    fn test_question_dialog_state_navigation() {
        let mut dialog = QuestionDialogState::new(
            "Select language",
            vec!["Rust".to_string(), "TypeScript".to_string()],
        );
        assert_eq!(dialog.selected_option, 0);
        assert_eq!(dialog.selected_answer(), "Rust");
        assert!(!dialog.typing_custom);

        // Next moves to option 1
        dialog.next();
        assert_eq!(dialog.selected_option, 1);
        assert_eq!(dialog.selected_answer(), "TypeScript");
        assert!(!dialog.typing_custom);

        // Next moves to option 2 (custom answer)
        dialog.next();
        assert_eq!(dialog.selected_option, 2);
        assert_eq!(dialog.selected_answer(), "");
        assert!(dialog.typing_custom);

        // Next wraps around to option 0
        dialog.next();
        assert_eq!(dialog.selected_option, 0);
        assert_eq!(dialog.selected_answer(), "Rust");
        assert!(!dialog.typing_custom);

        // Previous wraps to option 2 (custom answer)
        dialog.previous();
        assert_eq!(dialog.selected_option, 2);
        assert!(dialog.typing_custom);

        // Previous moves to option 1
        dialog.previous();
        assert_eq!(dialog.selected_option, 1);
        assert_eq!(dialog.selected_answer(), "TypeScript");
        assert!(!dialog.typing_custom);
    }

    #[test]
    fn test_question_dialog_state_custom_answer_input() {
        let mut dialog = QuestionDialogState::new(
            "Enter port",
            vec!["8080".to_string(), "3000".to_string()],
        );
        assert_eq!(dialog.selected_option, 0);

        dialog.push_char('9');
        assert_eq!(dialog.custom_answer, "9");
        assert_eq!(dialog.selected_option, 2);
        assert!(dialog.typing_custom);
        assert_eq!(dialog.selected_answer(), "9");

        dialog.push_char('0');
        dialog.push_char('0');
        dialog.push_char('0');
        assert_eq!(dialog.custom_answer, "9000");
        assert_eq!(dialog.selected_answer(), "9000");

        dialog.pop_char();
        assert_eq!(dialog.custom_answer, "900");
        assert_eq!(dialog.selected_answer(), "900");
    }

    #[test]
    fn test_question_dialog_state_clear_and_active() {
        let mut dialog = QuestionDialogState::new("Test", vec!["A".to_string()]);
        assert!(dialog.is_active());

        dialog.clear();
        assert!(!dialog.is_active());
        assert_eq!(dialog.question, "");
        assert_eq!(dialog.options.len(), 0);
        assert_eq!(dialog.selected_option, 0);
        assert_eq!(dialog.custom_answer, "");
        assert!(!dialog.typing_custom);
    }

    #[test]
    fn test_render_question_dialog() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::new();
        let dialog = QuestionDialogState::new(
            "Which framework do you prefer?",
            vec!["Axum".to_string(), "Actix-web".to_string()],
        );

        terminal
            .draw(|f| {
                render_question_dialog(f, f.area(), &dialog, &theme);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("Question from Agent"));
        assert!(content.contains("Which framework do you prefer?"));
        assert!(content.contains("Axum"));
        assert!(content.contains("Actix-web"));
        assert!(content.contains("Custom text:"));
        assert!(content.contains("↑/↓ select"));
        assert!(content.contains("Type custom answer"));
        assert!(content.contains("Enter submit"));
        assert!(content.contains("Esc dismiss"));
    }
}
