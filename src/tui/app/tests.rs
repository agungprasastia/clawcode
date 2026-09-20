use super::tool_rows::ToolRowUpdate;
use super::util::*;
use super::*;

#[test]
fn test_char_boundary_helpers_multibyte_and_oob() {
    let text = "🦀 clawcode 🚀 日本語";
    assert_eq!(floor_char_boundary(text, 0), 0);
    assert_eq!(floor_char_boundary(text, 1), 0); // Inside 🦀
    assert_eq!(floor_char_boundary(text, 2), 0);
    assert_eq!(floor_char_boundary(text, 3), 0);
    assert_eq!(floor_char_boundary(text, 4), 4); // After 🦀
    assert_eq!(floor_char_boundary(text, text.len() + 100), text.len());

    assert_eq!(ceil_char_boundary(text, 0), 0);
    assert_eq!(ceil_char_boundary(text, 1), 4); // Rounds up to end of 🦀
    assert_eq!(ceil_char_boundary(text, 2), 4);
    assert_eq!(ceil_char_boundary(text, 3), 4);
    assert_eq!(ceil_char_boundary(text, 4), 4);
    assert_eq!(ceil_char_boundary(text, text.len()), text.len());
    assert_eq!(ceil_char_boundary(text, text.len() + 100), text.len());
}

#[test]
fn test_compact_keeps_complete_utf8_user_turn() {
    let mut app = App {
        transcript: format!(
            "old\n{}\n```rust\n> literal\n```\n> recent prompt\nassistant",
            "x".repeat(1300)
        ),
        prompt: "/compact".to_string(),
        ..App::default()
    };

    app.submit_prompt();

    assert!(
        app.transcript()
            .starts_with("[earlier transcript compacted]\n> recent prompt\n")
    );
    assert!(app.transcript().is_char_boundary(app.transcript().len()));
}

#[test]
fn test_bounded_string_utf8() {
    let text = "🦀 clawcode 🚀".to_string();
    let truncated = bounded(text, 2);
    assert_eq!(truncated, ""); // 2 bytes is inside 4-byte 🦀, clamped to 0

    let text2 = "🦀 clawcode 🚀".to_string();
    let truncated2 = bounded(text2, 4);
    assert_eq!(truncated2, "🦀");
}

#[test]
fn test_ui_event_queue_safety() {
    let mut queue = UiEventQueue::new(0); // Should clamp to >= 3 without panicking
    assert!(queue.capacity() >= 3);

    for i in 0..100 {
        queue.push(UiEvent::Input(Input::Character(
            (b'a' + (i % 26) as u8) as char,
        )));
    }
    assert!(queue.len() <= queue.capacity());
}

#[test]
fn test_prompt_history_navigation_bounds() {
    let mut app = App::default();
    // Empty history
    app.navigate_history_up();
    assert_eq!(app.prompt(), "");
    app.navigate_history_down();
    assert_eq!(app.prompt(), "");

    // Submit some prompts
    app.prompt = "first".to_string();
    app.submit_prompt();
    app.prompt = "second".to_string();
    app.submit_prompt();

    app.navigate_history_up();
    assert_eq!(app.prompt(), "second");
    app.navigate_history_up();
    assert_eq!(app.prompt(), "first");
    app.navigate_history_up(); // Should clamp, not panic
    assert_eq!(app.prompt(), "first");
    app.navigate_history_down();
    assert_eq!(app.prompt(), "second");
    app.navigate_history_down();
    assert_eq!(app.prompt(), "");
}

#[test]
fn test_suggestion_navigation_bounds() {
    let mut app = App::default();
    assert_eq!(app.selected_suggestion_index(), 0);
    app.previous_suggestion();
    assert_eq!(app.selected_suggestion_index(), 0);
    app.next_suggestion();
    assert_eq!(app.selected_suggestion_index(), 0);

    app.prompt = "/m".to_string();
    let count = app.suggestion_count();
    if count > 0 {
        app.next_suggestion();
        assert!(app.selected_suggestion_index() < count);
        app.previous_suggestion();
        assert!(app.selected_suggestion_index() < count);
    }
}

#[test]
fn test_truncate_transcript_multibyte() {
    let mut app = App::default();
    // Fill with Japanese text
    let chunk = "こんにちは世界！🦀\n";
    let mut s = String::new();
    while s.len() < App::MAX_TRANSCRIPT_BYTES + 20 * 1024 {
        s.push_str(chunk);
    }
    app.apply(UiEvent::StreamDelta(s));
    assert!(app.transcript().len() <= App::MAX_TRANSCRIPT_BYTES);
    assert!(app.transcript().starts_with(App::TRUNCATION_MARKER));
}

#[test]
fn test_truncate_transcript_user_turn_boundary_and_stream_base() {
    let mut app = App::default();
    let turn1 = format!("> user turn 1\n{}\n", "a".repeat(100 * 1024));
    let turn2 = format!("> user turn 2\n{}\n", "b".repeat(100 * 1024));
    let turn3 = format!("> user turn 3\n{}\n", "c".repeat(100 * 1024));
    app.transcript = format!("{turn1}{turn2}{turn3}");
    let base = app.transcript.len() - 1000;
    app.stream_base_len = Some(base);
    app.truncate_transcript();
    assert!(app.transcript.starts_with(App::TRUNCATION_MARKER));
    assert!(app.transcript[App::TRUNCATION_MARKER.len()..].starts_with("> user turn"));
    assert!(app.stream_base_len.is_some());
    assert!(app.stream_base_len.unwrap() < base);
}

#[test]
fn test_copy_command_writes_to_clipboard() {
    let mut app = App {
        transcript: "hello transcript".to_string(),
        prompt: "/copy".to_string(),
        ..App::default()
    };
    app.submit_prompt();
    assert!(
        app.diagnostic.contains("transcript copied to clipboard")
            || app.diagnostic.contains("failed to copy transcript")
    );

    let mut app_empty = App {
        prompt: "/copy".to_string(),
        ..App::default()
    };
    app_empty.submit_prompt();
    assert!(
        app_empty.diagnostic.contains("copied status")
            || app_empty.diagnostic.contains("failed to copy status")
    );
}

#[test]
fn test_paste_input_inserts_at_cursor() {
    let mut app = App::default();
    app.apply(UiEvent::Paste("hello world".to_string()));
    assert_eq!(app.prompt(), "hello world");
    assert_eq!(app.cursor_position(), 11);

    app.apply(UiEvent::Input(Input::Left));
    app.apply(UiEvent::Input(Input::Left));
    app.apply(UiEvent::Paste("!".to_string()));
    assert_eq!(app.prompt(), "hello wor!ld");
}

#[test]
fn test_theme_autocompletion_and_no_mode_toggle() {
    let mut app = App::default();
    let original_mode = app.mode();
    app.prompt = "/theme ".to_string();
    app.cursor_position = app.prompt.chars().count();
    let suggestions = app.matching_theme_suggestions();
    assert!(!suggestions.is_empty());
    assert_eq!(suggestions[0], "Clawcode Dark");

    app.apply(UiEvent::Input(Input::ToggleMode));
    assert_eq!(app.mode(), original_mode);
    assert!(app.prompt().starts_with("/theme "));
    assert_eq!(app.prompt(), "/theme Clawcode Dark");
}

#[test]
fn test_cursor_navigation_and_editing() {
    let mut app = App::default();
    for c in "hello".chars() {
        app.apply(UiEvent::Input(Input::Character(c)));
    }
    assert_eq!(app.prompt(), "hello");
    assert_eq!(app.cursor_position(), 5);

    app.apply(UiEvent::Input(Input::Left));
    assert_eq!(app.cursor_position(), 4);

    app.apply(UiEvent::Input(Input::Character('X')));
    assert_eq!(app.prompt(), "hellXo");
    assert_eq!(app.cursor_position(), 5);

    app.apply(UiEvent::Input(Input::Home));
    assert_eq!(app.cursor_position(), 0);

    app.apply(UiEvent::Input(Input::Right));
    assert_eq!(app.cursor_position(), 1);

    app.apply(UiEvent::Input(Input::Backspace));
    assert_eq!(app.prompt(), "ellXo");
    assert_eq!(app.cursor_position(), 0);

    app.apply(UiEvent::Input(Input::End));
    assert_eq!(app.cursor_position(), 5);
}

#[test]
fn test_delete_session_in_sessions_dialog() {
    let mut app = App::default();
    let session = app
        .command_service
        .create_session("delete me session")
        .unwrap();
    let id = session.id;
    let title = session.title.clone();
    app.sessions_dialog = Some(SessionsDialogState::new(vec![session], Some(id)));
    assert_eq!(app.sessions_dialog.as_ref().unwrap().items.len(), 1);

    app.apply(UiEvent::Input(Input::Character('d')));
    assert_eq!(app.sessions_dialog.as_ref().unwrap().items.len(), 0);
    assert_eq!(app.diagnostic(), format!("session deleted: {title}"));
}

#[test]
fn test_tool_row_expandable_and_click() {
    let mut app = App::default();
    app.upsert_tool_row(ToolRowUpdate {
        call_id: "bash-short",
        name: "bash",
        state: ToolRowState::Running,
        desc: "echo 1".to_string(),
        arguments: String::new(),
        metadata: None,
    });
    app.complete_tool_row("bash-short", "bash", true, "line 1\nline 2");
    assert!(!app.tool_rows[0].expandable);

    app.upsert_tool_row(ToolRowUpdate {
        call_id: "bash-long",
        name: "bash",
        state: ToolRowState::Running,
        desc: "echo many".to_string(),
        arguments: String::new(),
        metadata: None,
    });
    let long_output = (1..=12)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.complete_tool_row("bash-long", "bash", true, &long_output);
    assert!(app.tool_rows[1].expandable);

    app.set_tool_row_clicks(vec![
        (
            "bash-short".to_string(),
            ratatui::layout::Rect {
                x: 0,
                y: 10,
                width: 50,
                height: 1,
            },
        ),
        (
            "bash-long".to_string(),
            ratatui::layout::Rect {
                x: 0,
                y: 15,
                width: 50,
                height: 1,
            },
        ),
    ]);

    app.handle_mouse_click(5, 10);
    assert!(!app.is_tool_expanded("bash-short"));

    app.handle_mouse_click(5, 15);
    assert!(app.is_tool_expanded("bash-long"));
    assert_eq!(app.diagnostic(), "tool output expanded");

    app.handle_mouse_click(5, 15);
    assert!(!app.is_tool_expanded("bash-long"));
    assert_eq!(app.diagnostic(), "tool output collapsed");
}

#[test]
fn cancel_blocks_stale_same_generation_events() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("first");
    let session_id = app.active_session_id().unwrap();

    tx.send(RuntimeEvent {
        session_id,
        generation_id: Some(7),
        seq: 1,
        kind: "generation_started".into(),
        payload_json: "{}".into(),
    })
    .unwrap();
    app.poll_runtime();
    assert_eq!(app.active_generation_id, Some(7));

    app.apply(UiEvent::Input(Input::Cancel));
    assert_eq!(app.conversation_status(), ConversationStatus::Cancelled);

    tx.send(RuntimeEvent {
        session_id,
        generation_id: Some(7),
        seq: 2,
        kind: "generation_started".into(),
        payload_json: "{}".into(),
    })
    .unwrap();
    tx.send(RuntimeEvent {
        session_id,
        generation_id: Some(7),
        seq: 3,
        kind: "text_delta".into(),
        payload_json: serde_json::json!({"delta": "stale"}).to_string(),
    })
    .unwrap();
    app.poll_runtime();

    assert_eq!(app.conversation_status(), ConversationStatus::Cancelled);
    assert_eq!(app.active_generation_id, None);
    assert_eq!(app.stream_parts, vec![StreamPart::User("first".into())]);
    assert!(!app.transcript().contains("stale"));
}

#[test]
fn generation_started_does_not_bypass_generation_mismatch_guard() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("first");
    let session_id = app.active_session_id().unwrap();

    for (seq, generation_id, kind, payload_json) in [
        (1, 7, "generation_started", "{}".to_string()),
        (
            2,
            7,
            "text_delta",
            serde_json::json!({"delta": "kept"}).to_string(),
        ),
        (3, 8, "generation_started", "{}".to_string()),
    ] {
        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(generation_id),
            seq,
            kind: kind.into(),
            payload_json,
        })
        .unwrap();
    }
    app.poll_runtime();

    assert_eq!(app.active_generation_id, Some(7));
    assert_eq!(
        app.stream_parts,
        vec![
            StreamPart::User("first".into()),
            StreamPart::Text("kept".into())
        ]
    );
}

#[test]
fn cancelled_generation_followed_by_prompt_starts_fresh_active_turn() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("first");
    let session_id = app.active_session_id().unwrap();
    tx.send(RuntimeEvent {
        session_id,
        generation_id: Some(7),
        seq: 1,
        kind: "generation_started".into(),
        payload_json: "{}".into(),
    })
    .unwrap();
    app.poll_runtime();
    app.apply(UiEvent::Input(Input::Cancel));

    app.submit_user_prompt("second");
    assert_eq!(app.conversation_status(), ConversationStatus::Active);

    tx.send(RuntimeEvent {
        session_id,
        generation_id: Some(7),
        seq: 2,
        kind: "text_delta".into(),
        payload_json: serde_json::json!({"delta": "stale"}).to_string(),
    })
    .unwrap();
    tx.send(RuntimeEvent {
        session_id,
        generation_id: Some(8),
        seq: 3,
        kind: "generation_started".into(),
        payload_json: "{}".into(),
    })
    .unwrap();
    tx.send(RuntimeEvent {
        session_id,
        generation_id: Some(8),
        seq: 4,
        kind: "text_delta".into(),
        payload_json: serde_json::json!({"delta": "fresh"}).to_string(),
    })
    .unwrap();
    app.poll_runtime();

    assert_eq!(app.conversation_status(), ConversationStatus::Active);
    assert_eq!(app.active_generation_id, Some(8));
    assert_eq!(
        app.stream_parts,
        vec![
            StreamPart::User("first".into()),
            StreamPart::User("second".into()),
            StreamPart::Text("fresh".into())
        ]
    );
}

#[test]
fn clear_and_commands_reset_all_turn_view_state() {
    for command in ["/clear", "/home", "/compact"] {
        let mut app = App {
            transcript: "x".repeat(2048),
            prompt: command.to_string(),
            status: ConversationStatus::Active,
            active_generation_id: Some(7),
            text_stream_active: true,
            stream_parts: vec![StreamPart::Text("stale".into())],
            stream_base_len: Some(1),
            reasoning_buffer: "thinking".into(),
            reasoning_start: Some(std::time::Instant::now()),
            reasoning_duration: Some(std::time::Duration::from_secs(1)),
            reasoning_active: true,
            current_plan: vec![("step".into(), "pending".into())],
            chat_scroll: 9,
            thought_expanded: true,
            expanded_tool_rows: HashSet::from(["call".into()]),
            ..App::default()
        };
        app.upsert_tool_row(ToolRowUpdate {
            call_id: "call",
            name: "bash",
            state: ToolRowState::Running,
            desc: "echo stale".into(),
            arguments: String::new(),
            metadata: None,
        });
        app.active_tool = Some(ActiveToolInfo {
            name: "bash".into(),
            desc: "echo stale".into(),
            started_at: std::time::Instant::now(),
        });

        app.submit_prompt();

        assert!(app.stream_parts.is_empty(), "{command}");
        assert_eq!(app.stream_base_len, None, "{command}");
        assert!(!app.typewriter.is_active(), "{command}");
        assert!(!app.text_stream_active, "{command}");
        assert!(
            app.reasoning_buffer.is_empty(),
            "{command}: {:?}",
            app.reasoning_buffer
        );
        assert!(app.reasoning_start.is_none(), "{command}");
        assert!(app.reasoning_duration.is_none(), "{command}");
        assert!(!app.reasoning_active, "{command}");
        assert!(app.active_tool.is_none(), "{command}");
        assert!(app.tool_rows.is_empty(), "{command}");
        assert!(app.expanded_tool_rows.is_empty(), "{command}");
        assert!(!app.thought_expanded, "{command}");
        if command == "/compact" {
            assert_eq!(app.current_plan.len(), 1, "{command}");
        } else {
            assert!(app.current_plan.is_empty(), "{command}");
        }
        assert_eq!(app.chat_scroll, 0, "{command}");
    }
}

#[test]
fn fresh_prompt_after_cancel_accepts_prompt_submitted() {
    let mut app = App::default();
    app.submit_user_prompt("first");
    app.status = ConversationStatus::Cancelled;
    app.active_generation_id = Some(7);
    app.text_stream_active = true;
    app.stream_parts = vec![StreamPart::Text("stale".into())];
    app.stream_base_len = Some(1);
    app.reasoning_buffer = "thinking".into();
    app.reasoning_active = true;
    app.current_plan = vec![("step".into(), "pending".into())];
    app.thought_expanded = true;
    app.upsert_tool_row(ToolRowUpdate {
        call_id: "call",
        name: "bash",
        state: ToolRowState::Running,
        desc: "echo stale".into(),
        arguments: String::new(),
        metadata: None,
    });

    app.submit_user_prompt("fresh");

    assert_eq!(app.conversation_status(), ConversationStatus::Active);
    assert_eq!(app.active_generation_id, None);
    assert_eq!(
        app.stream_parts,
        vec![
            StreamPart::Text("stale".into()),
            StreamPart::User("fresh".into())
        ]
    );
    assert_eq!(app.tool_rows.len(), 1);
    assert_eq!(app.current_plan.len(), 1);
    assert!(!app.thought_expanded);
    assert!(!app.text_stream_active);
}

#[test]
fn runtime_finish_reason_survives_generation_finished() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("finish with length");
    let session_id = app.active_session_id().unwrap();
    for (seq, kind, payload_json) in [
        (1, "generation_started", "{}".to_string()),
        (
            2,
            "finish",
            serde_json::json!({"reason": "Length"}).to_string(),
        ),
        (
            3,
            "generation_finished",
            serde_json::json!({"status": "completed", "finish_reason": "Length"}).to_string(),
        ),
    ] {
        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(1),
            seq,
            kind: kind.into(),
            payload_json,
        })
        .unwrap();
    }
    app.poll_runtime();

    assert_eq!(
        app.conversation_status(),
        ConversationStatus::Finished(FinishReason::Length)
    );
}
