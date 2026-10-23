use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRenameDialogState {
    pub session_id: i64,
    pub title: String,
    pub cursor: usize,
}

impl SessionRenameDialogState {
    pub fn new(session_id: i64, title: impl Into<String>) -> Self {
        let title = title.into();
        let cursor = title.chars().count();
        Self {
            session_id,
            title,
            cursor,
        }
    }

    pub fn push_char(&mut self, character: char) {
        let byte_index = self.byte_index(self.cursor);
        self.title.insert(byte_index, character);
        self.cursor += 1;
    }

    pub fn pop_char(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let previous = self.cursor - 1;
        let byte_index = self.byte_index(previous);
        self.title.remove(byte_index);
        self.cursor = previous;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.title.chars().count() {
            return;
        }
        let byte_index = self.byte_index(self.cursor);
        self.title.remove(byte_index);
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.title.chars().count());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.title.chars().count();
    }

    pub fn trimmed_title(&self) -> &str {
        self.title.trim()
    }

    fn byte_index(&self, character_index: usize) -> usize {
        self.title
            .char_indices()
            .nth(character_index)
            .map(|(index, _)| index)
            .unwrap_or(self.title.len())
    }
}

pub fn render_session_rename_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    dialog: &SessionRenameDialogState,
    theme: &Theme,
) {
    let width = area.width.clamp(44, 72).min(area.width);
    let height = 7.min(area.height.saturating_sub(2));
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
        .border_style(Style::default().fg(theme.amber))
        .style(Style::default().bg(theme.panel))
        .title(Span::styled(
            " Rename session ",
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
    if inner.width < 8 || inner.height < 3 {
        return;
    }

    let prompt = format!("Session #{}", dialog.session_id);
    let title = Line::from(vec![
        Span::styled("› ", Style::default().fg(theme.amber)),
        Span::styled(
            &dialog.title,
            Style::default().fg(theme.ink).add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(theme.amber)),
    ]);
    let footer = Line::from(vec![
        Span::styled("enter", Style::default().fg(theme.amber)),
        Span::styled(" save  ", Style::default().fg(theme.dim)),
        Span::styled("esc", Style::default().fg(theme.amber)),
        Span::styled(" cancel", Style::default().fg(theme.dim)),
    ]);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(prompt, Style::default().fg(theme.dim))),
            Line::from(""),
            title,
            Line::from(""),
            footer,
        ]),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_at_character_boundaries() {
        let mut dialog = SessionRenameDialogState::new(1, "abc");
        dialog.move_home();
        dialog.push_char('é');
        assert_eq!(dialog.title, "éabc");
        assert_eq!(dialog.cursor, 1);
        dialog.delete_forward();
        assert_eq!(dialog.title, "ébc");
        dialog.move_end();
        dialog.pop_char();
        assert_eq!(dialog.title, "éb");
        assert_eq!(dialog.trimmed_title(), "éb");
    }
}
