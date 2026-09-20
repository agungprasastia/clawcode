use std::convert::Infallible;

use clawcode::tui::{
    App, ConversationMode, ConversationStatus, Input, UiEvent, UiEventQueue, runtime_step,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

fn press(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn type_text(app: &mut App, events: &mut UiEventQueue, value: &str) {
    for character in value.chars() {
        runtime_step(app, events, None, |_| Ok::<_, Infallible>(())).unwrap();
        events.push(UiEvent::Input(Input::Character(character)));
    }
    runtime_step(app, events, None, |_| Ok::<_, Infallible>(())).unwrap();
}

fn submit(app: &mut App, events: &mut UiEventQueue) {
    runtime_step(app, events, Some(press(KeyCode::Enter)), |_| {
        Ok::<_, Infallible>(())
    })
    .unwrap();
}

#[test]
fn scripted_commands_run_through_runtime_step() {
    let mut app = App::new();
    let mut events = UiEventQueue::new(16);

    for command in ["/new demo", "/sessions", "/connect openai", "/models"] {
        type_text(&mut app, &mut events, command);
        submit(&mut app, &mut events);
    }
    assert_eq!(app.selected_provider(), "openai");
    assert_eq!(app.diagnostic(), "0 model(s)");

    type_text(&mut app, &mut events, "/plan");
    submit(&mut app, &mut events);
    assert_eq!(app.mode(), ConversationMode::Plan);

    type_text(&mut app, &mut events, "/build");
    submit(&mut app, &mut events);
    assert_eq!(app.mode(), ConversationMode::Build);

    app.apply_conversation(clawcode::conversation::ConversationEvent::PromptSubmitted {
        provider: "openai".into(),
        model: "mock".into(),
        prompt: "run".into(),
    });
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ))),
        |_| Ok::<_, Infallible>(()),
    )
    .unwrap();
    assert!(app.take_cancellation());
    assert_eq!(app.conversation_status(), ConversationStatus::Cancelled);

    type_text(&mut app, &mut events, "/exit");
    submit(&mut app, &mut events);
    assert!(!app.is_running());
}
