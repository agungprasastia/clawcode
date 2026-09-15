use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{Input, UiEvent};

pub fn translate(event: Event) -> Option<UiEvent> {
    match event {
        Event::Resize(width, height) => Some(UiEvent::Resize { width, height }),
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            translate_key(key).map(UiEvent::Input)
        }
        _ => None,
    }
}

fn translate_key(key: KeyEvent) -> Option<Input> {
    match key.code {
        KeyCode::Enter => Some(Input::Submit),
        KeyCode::Tab | KeyCode::BackTab
            if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
        {
            Some(Input::ToggleMode)
        }
        KeyCode::Esc | KeyCode::Char('q') if key.modifiers.is_empty() => Some(Input::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Input::Cancel),
        KeyCode::Char(character)
            if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
        {
            Some(Input::Character(character))
        }
        KeyCode::Backspace => Some(Input::Backspace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_cancels_without_quitting() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        assert_eq!(translate_key(key), Some(Input::Cancel));
    }

    #[test]
    fn tab_translates_to_toggle_mode() {
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(translate_key(tab), Some(Input::ToggleMode));

        let backtab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(translate_key(backtab), Some(Input::ToggleMode));
    }
}
