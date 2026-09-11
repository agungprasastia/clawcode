use clawcode::{
    conversation::ConversationEvent,
    provider::FinishReason,
    tui::{App, ConversationMode, ConversationStatus},
};

#[test]
fn conversation_events_update_turn_and_diagnostic_state() {
    let mut app = App::default();

    app.apply_conversation(ConversationEvent::PromptSubmitted {
        prompt: "hello".into(),
        provider: "openai".into(),
        model: "gpt-test".into(),
    });
    assert_eq!(app.conversation_status(), ConversationStatus::Active);
    assert_eq!(app.selected_provider(), "openai");
    assert_eq!(app.selected_model(), "gpt-test");

    app.apply_conversation(ConversationEvent::TextDelta("hai".into()));
    app.apply_conversation(ConversationEvent::Finished(FinishReason::Stop));
    assert_eq!(
        app.conversation_status(),
        ConversationStatus::Finished(FinishReason::Stop)
    );
    assert_eq!(app.transcript(), "hai");
}

#[test]
fn cancellation_wins_over_later_finish_and_error() {
    let mut app = App::default();
    app.apply_conversation(ConversationEvent::PromptSubmitted {
        prompt: "hello".into(),
        provider: "p".into(),
        model: "m".into(),
    });
    app.apply_conversation(ConversationEvent::Cancelled);
    app.apply_conversation(ConversationEvent::Finished(FinishReason::Stop));
    app.apply_conversation(ConversationEvent::Error("late".into()));
    assert_eq!(app.conversation_status(), ConversationStatus::Cancelled);
    assert_eq!(app.diagnostic(), "");
}

#[test]
fn stale_prompt_and_text_cannot_revive_cancelled_turn() {
    let mut app = App::default();
    app.apply_conversation(ConversationEvent::PromptSubmitted {
        prompt: "hello".into(),
        provider: "p".into(),
        model: "m".into(),
    });
    app.apply_conversation(ConversationEvent::Cancelled);
    app.apply_conversation(ConversationEvent::PromptSubmitted {
        prompt: "stale".into(),
        provider: "stale-provider".into(),
        model: "stale-model".into(),
    });
    app.apply_conversation(ConversationEvent::TextDelta("stale output".into()));

    assert_eq!(app.conversation_status(), ConversationStatus::Cancelled);
    assert_eq!(app.selected_provider(), "p");
    assert_eq!(app.selected_model(), "m");
    assert_eq!(app.transcript(), "");
}

#[test]
fn plan_mode_rejects_mutation() {
    let mut app = App::default();
    app.set_mode(ConversationMode::Plan);
    app.apply_conversation(ConversationEvent::MutationRequested("write file".into()));
    assert_eq!(app.conversation_status(), ConversationStatus::Rejected);
    assert!(app.diagnostic().contains("PLAN"));
}

#[test]
fn conversation_text_remains_bounded_and_utf8_safe() {
    let mut app = App::default();
    for _ in 0..4 {
        app.apply_conversation(ConversationEvent::TextDelta("é".repeat(100_000)));
    }
    assert!(app.transcript().len() <= App::MAX_TRANSCRIPT_BYTES);
    assert!(app.transcript().is_char_boundary(app.transcript().len()));
}
