use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

use super::{Input, UiEvent};

pub fn translate(event: Event) -> Option<UiEvent> {
    match event {
        Event::Resize(width, height) => Some(UiEvent::Resize { width, height }),
        Event::Paste(text) => Some(UiEvent::Paste(text)),
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            translate_key(key).map(UiEvent::Input)
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => Some(UiEvent::MouseScroll { up: true }),
            MouseEventKind::ScrollDown => Some(UiEvent::MouseScroll { up: false }),
            MouseEventKind::Down(MouseButton::Left) => Some(UiEvent::MouseClick {
                x: mouse.column,
                y: mouse.row,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn control(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
}

fn alt(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::ALT)
}

fn shift(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::SHIFT)
}

fn translate_key(key: KeyEvent) -> Option<Input> {
    match key.code {
        KeyCode::Enter if shift(&key) || alt(&key) || control(&key) => Some(Input::Newline),
        KeyCode::Enter => Some(Input::Submit),
        KeyCode::Char('j') if control(&key) => Some(Input::Newline),
        KeyCode::Tab if control(&key) => Some(Input::SessionNext),
        KeyCode::BackTab if control(&key) => Some(Input::SessionPrevious),
        KeyCode::Tab | KeyCode::BackTab
            if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
        {
            if key.code == KeyCode::BackTab || shift(&key) {
                Some(Input::CycleAgent)
            } else {
                Some(Input::ToggleMode)
            }
        }
        KeyCode::F(2) if shift(&key) => Some(Input::CycleModelReverse),
        KeyCode::F(2) => Some(Input::CycleModel),
        KeyCode::PageUp => Some(Input::PageUp),
        KeyCode::PageDown => Some(Input::PageDown),
        KeyCode::Home if alt(&key) => Some(Input::FirstMessage),
        KeyCode::Home if control(&key) || shift(&key) || key.modifiers.is_empty() => {
            Some(Input::Home)
        }
        KeyCode::End if alt(&key) => Some(Input::LastMessage),
        KeyCode::End if control(&key) || shift(&key) || key.modifiers.is_empty() => {
            Some(Input::End)
        }
        KeyCode::Up if alt(&key) && shift(&key) => Some(Input::SessionPrevious),
        KeyCode::Down if alt(&key) && shift(&key) => Some(Input::SessionNext),
        KeyCode::Up if alt(&key) => Some(Input::SessionPrevious),
        KeyCode::Down if alt(&key) => Some(Input::SessionNext),
        KeyCode::Up if shift(&key) || control(&key) => Some(Input::ScrollUp),
        KeyCode::Down if shift(&key) || control(&key) => Some(Input::ScrollDown),
        KeyCode::Up if key.modifiers.is_empty() => Some(Input::Up),
        KeyCode::Down if key.modifiers.is_empty() => Some(Input::Down),
        KeyCode::Left if alt(&key) || control(&key) => Some(Input::WordLeft),
        KeyCode::Right if alt(&key) || control(&key) => Some(Input::WordRight),
        KeyCode::Left if key.modifiers.is_empty() || shift(&key) => Some(Input::Left),
        KeyCode::Right if key.modifiers.is_empty() || shift(&key) => Some(Input::Right),
        KeyCode::Esc if key.modifiers.is_empty() => Some(Input::Cancel),
        KeyCode::Delete if control(&key) || alt(&key) => Some(Input::DeleteWordForward),
        KeyCode::Delete if key.modifiers.is_empty() || shift(&key) => Some(Input::Delete),
        KeyCode::Backspace if control(&key) || alt(&key) => Some(Input::DeleteWord),
        KeyCode::Backspace if key.modifiers.is_empty() || shift(&key) => Some(Input::Backspace),
        KeyCode::Char(character) if control(&key) && alt(&key) => match character {
            'b' => Some(Input::PageUp),
            'f' => Some(Input::PageDown),
            'u' => Some(Input::HalfPageUp),
            'd' => Some(Input::HalfPageDown),
            'y' => Some(Input::ScrollUp),
            'e' => Some(Input::ScrollDown),
            'g' => Some(Input::LastMessage),
            _ => None,
        },
        KeyCode::Char('g') if control(&key) => Some(Input::FirstMessage),
        KeyCode::Char('x') if control(&key) => Some(Input::Leader),
        KeyCode::Char('p') if control(&key) => Some(Input::Palette),
        KeyCode::Char('o') if control(&key) => Some(Input::OpenMenu),
        KeyCode::Char('z' | 'Z') if control(&key) && shift(&key) => Some(Input::Redo),
        KeyCode::Char('z' | 'Z') if control(&key) => Some(Input::Undo),
        KeyCode::Char('y' | 'Y') if control(&key) => Some(Input::Redo),
        KeyCode::Char('-' | '_') if control(&key) => Some(Input::Undo),
        KeyCode::Char('.') if control(&key) => Some(Input::Redo),
        KeyCode::Char('c') if control(&key) => Some(Input::ClearOrExit),
        KeyCode::Char('d' | 'D') if control(&key) && shift(&key) => Some(Input::DeleteLine),
        KeyCode::Char('d') if control(&key) => Some(Input::DeleteOrExit),
        KeyCode::Char('r') if control(&key) => Some(Input::RenameSession),
        KeyCode::Char('f') if control(&key) => Some(Input::PinSession),
        KeyCode::Char('t' | 'T') if control(&key) && shift(&key) => Some(Input::ReopenTab),
        KeyCode::Char('t') if control(&key) => Some(Input::CycleVariant),
        KeyCode::Char('a') if control(&key) => Some(Input::Home),
        KeyCode::Char('e') if control(&key) => Some(Input::End),
        KeyCode::Char('u') if control(&key) => Some(Input::DeleteToLineStart),
        KeyCode::Char('k') if control(&key) => Some(Input::DeleteToLineEnd),
        KeyCode::Char('w') if control(&key) => Some(Input::DeleteWord),
        KeyCode::Char('b') if control(&key) => Some(Input::Left),
        KeyCode::Char('n') if control(&key) => Some(Input::Down),
        KeyCode::Char('l') if control(&key) => Some(Input::Clear),
        KeyCode::Char('v') if control(&key) => Some(Input::Paste),
        KeyCode::Char(character @ '1'..='9') if control(&key) => Some(Input::SelectSession(
            character.to_digit(10).unwrap_or(1) as usize - 1,
        )),
        KeyCode::Char('0') if control(&key) => Some(Input::SelectSession(9)),
        KeyCode::Char('f') if alt(&key) => Some(Input::WordRight),
        KeyCode::Char('b') if alt(&key) => Some(Input::WordLeft),
        KeyCode::Char('d') if alt(&key) => Some(Input::DeleteWordForward),
        KeyCode::Char('a') if alt(&key) => Some(Input::Home),
        KeyCode::Char('e') if alt(&key) => Some(Input::End),
        KeyCode::Char(character)
            if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
        {
            Some(Input::Character(character))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn escape_interrupts_instead_of_quitting() {
        assert_eq!(
            translate_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Input::Cancel)
        );
    }

    #[test]
    fn ctrl_c_clears_input_or_exits() {
        assert_eq!(
            translate_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Input::ClearOrExit)
        );
    }

    #[test]
    fn ctrl_d_deletes_or_exits() {
        assert_eq!(
            translate_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Some(Input::DeleteOrExit)
        );
    }

    #[test]
    fn ctrl_l_translates_to_clear() {
        assert_eq!(
            translate_key(key(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            Some(Input::Clear)
        );
    }

    #[test]
    fn ctrl_x_starts_leader_mode() {
        assert_eq!(
            translate_key(key(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            Some(Input::Leader)
        );
    }

    #[test]
    fn ctrl_p_opens_command_palette() {
        assert_eq!(
            translate_key(key(KeyCode::Char('p'), KeyModifiers::CONTROL)),
            Some(Input::Palette)
        );
    }

    #[test]
    fn ctrl_o_opens_session_menu() {
        assert_eq!(
            translate_key(key(KeyCode::Char('o'), KeyModifiers::CONTROL)),
            Some(Input::OpenMenu)
        );
    }

    #[test]
    fn tab_completes_and_shift_tab_cycles_agents() {
        assert_eq!(
            translate_key(key(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Input::ToggleMode)
        );
        assert_eq!(
            translate_key(key(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some(Input::CycleAgent)
        );
    }

    #[test]
    fn ctrl_tab_switches_session_tabs() {
        assert_eq!(
            translate_key(key(KeyCode::Tab, KeyModifiers::CONTROL)),
            Some(Input::SessionNext)
        );
        assert_eq!(
            translate_key(key(
                KeyCode::BackTab,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            Some(Input::SessionPrevious)
        );
    }

    #[test]
    fn arrow_keys_translate_to_up_down_left_right() {
        assert_eq!(
            translate_key(key(KeyCode::Up, KeyModifiers::NONE)),
            Some(Input::Up)
        );
        assert_eq!(
            translate_key(key(KeyCode::Down, KeyModifiers::NONE)),
            Some(Input::Down)
        );
        assert_eq!(
            translate_key(key(KeyCode::Left, KeyModifiers::NONE)),
            Some(Input::Left)
        );
        assert_eq!(
            translate_key(key(KeyCode::Right, KeyModifiers::NONE)),
            Some(Input::Right)
        );
    }

    #[test]
    fn word_navigation_and_deletion_bindings() {
        assert_eq!(
            translate_key(key(KeyCode::Left, KeyModifiers::CONTROL)),
            Some(Input::WordLeft)
        );
        assert_eq!(
            translate_key(key(KeyCode::Right, KeyModifiers::ALT)),
            Some(Input::WordRight)
        );
        assert_eq!(
            translate_key(key(KeyCode::Backspace, KeyModifiers::CONTROL)),
            Some(Input::DeleteWord)
        );
        assert_eq!(
            translate_key(key(KeyCode::Delete, KeyModifiers::CONTROL)),
            Some(Input::DeleteWordForward)
        );
    }

    #[test]
    fn ctrl_editing_bindings() {
        assert_eq!(
            translate_key(key(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            Some(Input::Home)
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('e'), KeyModifiers::CONTROL)),
            Some(Input::End)
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            Some(Input::DeleteToLineStart)
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('k'), KeyModifiers::CONTROL)),
            Some(Input::DeleteToLineEnd)
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Some(Input::DeleteWord)
        );
    }

    #[test]
    fn input_undo_and_redo_bindings() {
        assert_eq!(
            translate_key(key(KeyCode::Char('-'), KeyModifiers::CONTROL)),
            Some(Input::Undo)
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('z'), KeyModifiers::CONTROL)),
            Some(Input::Undo)
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('.'), KeyModifiers::CONTROL)),
            Some(Input::Redo)
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('y'), KeyModifiers::CONTROL)),
            Some(Input::Redo)
        );
        assert_eq!(
            translate_key(key(
                KeyCode::Char('z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )),
            Some(Input::Redo)
        );
    }

    #[test]
    fn page_keys_and_scroll_translate_properly() {
        assert_eq!(
            translate_key(key(KeyCode::PageUp, KeyModifiers::NONE)),
            Some(Input::PageUp)
        );
        assert_eq!(
            translate_key(key(KeyCode::PageDown, KeyModifiers::NONE)),
            Some(Input::PageDown)
        );
        assert_eq!(
            translate_key(key(KeyCode::Up, KeyModifiers::SHIFT)),
            Some(Input::ScrollUp)
        );
        assert_eq!(
            translate_key(key(KeyCode::Down, KeyModifiers::CONTROL)),
            Some(Input::ScrollDown)
        );
        assert_eq!(
            translate_key(key(KeyCode::Home, KeyModifiers::NONE)),
            Some(Input::Home)
        );
        assert_eq!(
            translate_key(key(KeyCode::End, KeyModifiers::NONE)),
            Some(Input::End)
        );
    }

    #[test]
    fn ctrl_v_translates_to_paste() {
        assert_eq!(
            translate_key(key(KeyCode::Char('v'), KeyModifiers::CONTROL)),
            Some(Input::Paste)
        );
    }

    #[test]
    fn f2_cycles_models() {
        assert_eq!(
            translate_key(key(KeyCode::F(2), KeyModifiers::NONE)),
            Some(Input::CycleModel)
        );
        assert_eq!(
            translate_key(key(KeyCode::F(2), KeyModifiers::SHIFT)),
            Some(Input::CycleModelReverse)
        );
    }

    #[test]
    fn ctrl_digits_select_session_tabs() {
        assert_eq!(
            translate_key(key(KeyCode::Char('1'), KeyModifiers::CONTROL)),
            Some(Input::SelectSession(0))
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('0'), KeyModifiers::CONTROL)),
            Some(Input::SelectSession(9))
        );
    }

    #[test]
    fn newline_modifiers_insert_instead_of_submitting() {
        assert_eq!(
            translate_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Input::Submit)
        );
        for modifiers in [
            KeyModifiers::SHIFT,
            KeyModifiers::ALT,
            KeyModifiers::CONTROL,
        ] {
            assert_eq!(
                translate_key(key(KeyCode::Enter, modifiers)),
                Some(Input::Newline)
            );
        }
        assert_eq!(
            translate_key(key(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            Some(Input::Newline)
        );
    }

    #[test]
    fn ctrl_shift_t_reopens_closed_tab() {
        for character in ['t', 'T'] {
            assert_eq!(
                translate_key(key(
                    KeyCode::Char(character),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT
                )),
                Some(Input::ReopenTab)
            );
        }
    }

    #[test]
    fn lowercase_q_remains_prompt_text() {
        assert_eq!(
            translate_key(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Input::Character('q'))
        );
    }

    #[test]
    fn event_paste_translates_to_ui_event_paste() {
        let event = Event::Paste("clipboard text".to_string());
        assert_eq!(
            translate(event),
            Some(UiEvent::Paste("clipboard text".to_string()))
        );
    }

    #[test]
    fn mouse_click_translates_to_mouse_click_event() {
        use crossterm::event::MouseEvent;
        let down_event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 15,
            row: 20,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            translate(down_event),
            Some(UiEvent::MouseClick { x: 15, y: 20 })
        );

        let up_event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 15,
            row: 20,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(translate(up_event), None);
    }
}
