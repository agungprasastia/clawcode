use std::time::{Duration, Instant};

use clawcode::tui::{App, Input, UiEvent};

#[test]
fn quit_input_stops_app() {
    let mut app = App::default();

    app.apply(UiEvent::Input(Input::Quit));

    assert!(!app.is_running());
}

#[test]
fn resize_updates_viewport() {
    let mut app = App::default();

    app.apply(UiEvent::Resize {
        width: 120,
        height: 40,
    });

    assert_eq!(app.viewport(), (120, 40));
}

#[test]
fn synthetic_stream_batch_remains_responsive_to_quit() {
    let mut app = App::default();
    let mut events = (0..10_000)
        .map(|_| UiEvent::StreamDelta("x".to_owned()))
        .collect::<Vec<_>>();
    events.push(UiEvent::Input(Input::Quit));

    let started = Instant::now();
    app.apply_batch(events);

    assert!(!app.is_running());
    assert_eq!(app.transcript().len(), 10_000);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn text_input_and_backspace_edit_prompt() {
    let mut app = App::default();

    app.apply_batch([
        UiEvent::Input(Input::Character('o')),
        UiEvent::Input(Input::Character('k')),
        UiEvent::Input(Input::Backspace),
    ]);

    assert_eq!(app.prompt(), "o");
}
