use clawcode::{
    conversation::ConversationEvent,
    provider::{FinishReason, TurnMetrics},
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
fn app_receives_runtime_metrics_through_conversation_events() {
    let mut app = App::default();
    let metrics = TurnMetrics {
        duration: std::time::Duration::from_millis(4),
        usage: None,
        finish_reason: Some(FinishReason::Stop),
        provider: "provider".into(),
        model: "model".into(),
    };
    app.apply_conversation(ConversationEvent::Metrics(metrics.clone()));
    assert_eq!(app.metrics(), Some(&metrics));
}

#[test]
fn app_bounds_metrics_identity_without_changing_other_values() {
    let mut app = App::default();
    let provider = format!("{}終", "p".repeat(255));
    let model = format!("{}界", "m".repeat(255));
    let metrics = TurnMetrics {
        duration: std::time::Duration::from_millis(4),
        usage: Some(clawcode::provider::Usage {
            input_tokens: 7,
            output_tokens: 11,
        }),
        finish_reason: Some(FinishReason::Length),
        provider,
        model,
    };

    app.apply_conversation(ConversationEvent::Metrics(metrics.clone()));

    let stored = app.metrics().expect("metrics stored");
    assert_eq!(stored.duration, metrics.duration);
    assert_eq!(stored.usage, metrics.usage);
    assert_eq!(stored.finish_reason, metrics.finish_reason);
    assert_eq!(stored.provider, "p".repeat(255));
    assert_eq!(stored.model, "m".repeat(255));
    assert!(stored.provider.is_char_boundary(stored.provider.len()));
    assert!(stored.model.is_char_boundary(stored.model.len()));
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
