use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};

use super::{Input, UiEvent};

pub fn translate(event: Event) -> Option<UiEvent> {
    match event {
        Event::Resize(width, height) => Some(UiEvent::Resize { width, height }),
        Event::Paste(text) => Some(UiEvent::Paste(text)),
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            translate_key(key).map(UiEvent::Input)
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => Some(UiEvent::Input(Input::ScrollUp)),
            MouseEventKind::ScrollDown => Some(UiEvent::Input(Input::ScrollDown)),
            _ => None,
        },
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
        KeyCode::PageUp => Some(Input::PageUp),
        KeyCode::PageDown => Some(Input::PageDown),
        KeyCode::Home
            if key.modifiers.is_empty() || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            Some(Input::Home)
        }
        KeyCode::End
            if key.modifiers.is_empty() || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            Some(Input::End)
        }
        KeyCode::Up
            if key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(Input::ScrollUp)
        }
        KeyCode::Down
            if key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(Input::ScrollDown)
        }
        KeyCode::Up if key.modifiers.is_empty() => Some(Input::Up),
        KeyCode::Down if key.modifiers.is_empty() => Some(Input::Down),
        KeyCode::Left if key.modifiers.is_empty() => Some(Input::Left),
        KeyCode::Right if key.modifiers.is_empty() => Some(Input::Right),
        KeyCode::Esc | KeyCode::Char('q') if key.modifiers.is_empty() => Some(Input::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Input::Cancel),
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Input::Paste),
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Input::Clear),
        KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Input::WhichKey)
        }
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
    fn ctrl_l_translates_to_clear() {
        let key = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);

        assert_eq!(translate_key(key), Some(Input::Clear));
    }

    #[test]
    fn ctrl_x_translates_to_which_key() {
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);

        assert_eq!(translate_key(key), Some(Input::WhichKey));
    }

    #[test]
    fn tab_translates_to_toggle_mode() {
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(translate_key(tab), Some(Input::ToggleMode));

        let backtab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(translate_key(backtab), Some(Input::ToggleMode));
    }

    #[test]
    fn arrow_keys_translate_to_up_down_left_right() {
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(translate_key(up), Some(Input::Up));

        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(translate_key(down), Some(Input::Down));

        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(translate_key(left), Some(Input::Left));

        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(translate_key(right), Some(Input::Right));
    }

    #[test]
    fn page_keys_and_scroll_translate_properly() {
        let page_up = KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(translate_key(page_up), Some(Input::PageUp));

        let page_down = KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(translate_key(page_down), Some(Input::PageDown));

        let shift_up = KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT);
        assert_eq!(translate_key(shift_up), Some(Input::ScrollUp));

        let ctrl_down = KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL);
        assert_eq!(translate_key(ctrl_down), Some(Input::ScrollDown));

        let home = KeyEvent::new(KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(translate_key(home), Some(Input::Home));

        let end = KeyEvent::new(KeyCode::End, KeyModifiers::NONE);
        assert_eq!(translate_key(end), Some(Input::End));
    }

    #[test]
    fn ctrl_v_translates_to_paste() {
        let ctrl_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert_eq!(translate_key(ctrl_v), Some(Input::Paste));
    }

    #[test]
    fn event_paste_translates_to_ui_event_paste() {
        let event = Event::Paste("clipboard text".to_string());
        assert_eq!(
            translate(event),
            Some(UiEvent::Paste("clipboard text".to_string()))
        );
    }
}
