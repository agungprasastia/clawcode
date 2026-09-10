use clawcode::tui::{App, Input, UiEvent, UiEventQueue, process_pending};

#[test]
fn quit_input_stops_app() {
    let mut app = App::default();

    app.apply(UiEvent::Input(Input::Quit));

    assert!(!app.is_running());
}

#[test]
fn stream_flood_is_coalesced_and_priority_quit_bounds_redraws() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);
    for _ in 0..10_000 {
        events.push(UiEvent::StreamDelta("x".to_owned()));
    }
    events.push(UiEvent::Input(Input::Quit));

    let mut redraws = 0;
    if app.apply_pending(&mut events) {
        redraws += 1;
    }

    assert!(!app.is_running());
    assert_eq!(app.transcript(), "");
    assert!(events.len() <= events.capacity());
    assert_eq!(redraws, 1);
    assert!(!app.apply_pending(&mut events));
    assert_eq!(redraws, 1);
}

#[test]
fn cancel_has_priority_over_stream_flood_without_quitting() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(4);
    for _ in 0..10_000 {
        events.push(UiEvent::StreamDelta("x".to_owned()));
    }
    events.push(UiEvent::Input(Input::Cancel));

    assert!(app.apply_pending(&mut events));
    assert!(app.is_running());
    assert!(app.take_cancellation());
    assert!(!app.take_cancellation());
    assert!(app.transcript().len() <= UiEventQueue::MAX_COALESCED_STREAM_BYTES);
}

#[test]
fn resize_updates_known_terminal_size() {
    let mut app = App::default();

    app.apply(UiEvent::Resize {
        width: 132,
        height: 43,
    });

    assert_eq!(app.terminal_size(), Some((132, 43)));
}

#[test]
fn idle_event_loop_iteration_draws_zero_frames() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);
    let mut draws = 0;

    process_pending(&mut app, &mut events, |_| {
        draws += 1;
        Ok::<_, std::convert::Infallible>(())
    })
    .unwrap();

    assert_eq!(draws, 0);
}

#[test]
fn coalesced_event_batch_draws_one_frame() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);
    events.push(UiEvent::StreamDelta("first".to_owned()));
    events.push(UiEvent::StreamDelta(" second".to_owned()));
    events.push(UiEvent::Input(Input::Character('x')));
    let mut draws = 0;

    process_pending(&mut app, &mut events, |_| {
        draws += 1;
        Ok::<_, std::convert::Infallible>(())
    })
    .unwrap();

    assert_eq!(draws, 1);
    assert_eq!(app.transcript(), "first second");
    assert_eq!(app.prompt(), "x");
}

#[test]
fn coalesced_stream_memory_is_capped_on_utf8_boundary() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(4);
    for _ in 0..UiEventQueue::MAX_COALESCED_STREAM_BYTES {
        events.push(UiEvent::StreamDelta("é".to_owned()));
    }

    assert!(app.apply_pending(&mut events));

    assert!(app.transcript().len() <= UiEventQueue::MAX_COALESCED_STREAM_BYTES);
    assert!(app.transcript().is_char_boundary(app.transcript().len()));
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
