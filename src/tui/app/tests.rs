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

#[test]
fn reasoning_steps_are_separate_and_restart_timer_after_tools() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("inspect the repository");
    let session_id = app.active_session_id().unwrap();

    let send = |seq: i64, kind: &str, payload: serde_json::Value| {
        tx.send(RuntimeEvent {
            session_id,
            generation_id: Some(1),
            seq,
            kind: kind.to_string(),
            payload_json: payload.to_string(),
        })
        .unwrap();
    };

    send(1, "generation_started", serde_json::json!({}));
    send(
        2,
        "reasoning_delta",
        serde_json::json!({ "delta": "**Surveying**\n\nfirst step" }),
    );
    app.poll_runtime();

    let first_start = app.reasoning_start.expect("first reasoning timer");
    assert!(app.is_reasoning());

    std::thread::sleep(std::time::Duration::from_millis(10));
    send(
        3,
        "tool_call_start",
        serde_json::json!({ "id": "call-1", "name": "list_dir" }),
    );
    send(
        4,
        "tool_executing",
        serde_json::json!({
            "id": "call-1",
            "name": "list_dir",
            "arguments": { "path": "." }
        }),
    );
    send(
        5,
        "tool_call_settled",
        serde_json::json!({
            "call_id": "call-1",
            "tool_name": "list_dir",
            "status": "completed",
            "result": "src"
        }),
    );
    app.poll_runtime();

    assert!(!app.is_reasoning());
    assert!(app.reasoning_start.is_none());

    std::thread::sleep(std::time::Duration::from_millis(10));
    send(
        6,
        "reasoning_delta",
        serde_json::json!({ "delta": "**Planning**\n\nsecond step" }),
    );
    app.poll_runtime();

    let second_start = app.reasoning_start.expect("second reasoning timer");
    assert!(second_start > first_start);
    assert!(app.is_reasoning());

    let parts = app.stream_parts();
    assert_eq!(parts.len(), 4, "unexpected parts: {parts:?}");
    assert!(matches!(&parts[0], StreamPart::User(prompt) if prompt == "inspect the repository"));
    match &parts[1] {
        StreamPart::ReasoningDone { text, .. } => assert!(text.contains("first step")),
        other => panic!("expected completed first reasoning, got {other:?}"),
    }
    assert!(matches!(&parts[2], StreamPart::Tool(id) if id.as_str() == "call-1"));
    match &parts[3] {
        StreamPart::Reasoning(text) => assert!(text.contains("second step")),
        other => panic!("expected active second reasoning, got {other:?}"),
    }
}

#[test]
fn ctrl_c_clears_prompt_before_exiting() {
    let mut app = App {
        prompt: "draft text".into(),
        ..App::default()
    };
    app.apply(UiEvent::Input(Input::ClearOrExit));
    assert!(app.is_running());
    assert_eq!(app.prompt(), "");
    assert_eq!(app.cursor_position(), 0);

    app.apply(UiEvent::Input(Input::ClearOrExit));
    assert!(!app.is_running());
}

#[test]
fn ctrl_d_deletes_forward_then_exits_when_empty() {
    let mut app = App {
        prompt: "ab".into(),
        cursor_position: 0,
        ..App::default()
    };
    app.apply(UiEvent::Input(Input::DeleteOrExit));
    assert_eq!(app.prompt(), "b");
    assert_eq!(app.cursor_position(), 0);

    app.apply(UiEvent::Input(Input::DeleteOrExit));
    assert_eq!(app.prompt(), "");
    assert!(app.is_running());

    app.apply(UiEvent::Input(Input::DeleteOrExit));
    assert!(!app.is_running());
}

#[test]
fn ctrl_x_leader_opens_models_without_inserting_text() {
    let mut app = App::default();
    app.apply(UiEvent::Input(Input::Leader));
    assert!(app.which_key().visible);
    assert_eq!(app.prompt(), "");

    app.apply(UiEvent::Input(Input::Character('m')));
    assert!(!app.which_key().visible);
    assert!(app.models_dialog().is_some());
    assert_eq!(app.prompt(), "");
}

#[test]
fn leader_escape_dismisses_without_cancelling_turn() {
    let mut app = App {
        status: ConversationStatus::Active,
        ..App::default()
    };
    app.apply(UiEvent::Input(Input::Leader));
    app.apply(UiEvent::Input(Input::Cancel));

    assert!(!app.which_key().visible);
    assert_eq!(app.conversation_status(), ConversationStatus::Active);
}

#[test]
fn palette_filters_and_runs_exit_command() {
    let mut app = App::default();
    app.apply(UiEvent::Input(Input::Palette));
    assert!(app.palette_dialog().is_some());

    for character in "exit".chars() {
        app.apply(UiEvent::Input(Input::Character(character)));
    }
    assert_eq!(
        app.palette_dialog()
            .and_then(|dialog| dialog.selected_item())
            .map(|item| item.title),
        Some("Exit the app")
    );
    app.apply(UiEvent::Input(Input::Submit));
    assert!(!app.is_running());
}

#[test]
fn rename_dialog_updates_persisted_session_title() {
    let mut app = App::default();
    app.new_session();
    let session_id = app.active_session_id().unwrap();
    assert!(app.is_running());

    app.open_session_rename_dialog();
    let dialog = app.session_rename_dialog.as_mut().unwrap();
    dialog.title = "Renamed session".into();
    dialog.cursor = dialog.title.chars().count();
    app.apply(UiEvent::Input(Input::Submit));

    let sessions = app.command_service.list_sessions().unwrap();
    let session = sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap();
    assert_eq!(session.title, "Renamed session");
    assert!(app.session_rename_dialog().is_none());
}

#[test]
fn ctrl_f_toggles_session_pin() {
    let mut app = App::default();
    app.new_session();
    let session_id = app.active_session_id().unwrap();

    app.apply(UiEvent::Input(Input::PinSession));
    let pinned = app
        .command_service
        .list_sessions()
        .unwrap()
        .into_iter()
        .find(|session| session.id == session_id)
        .unwrap()
        .pinned;
    assert!(pinned);

    app.apply(UiEvent::Input(Input::PinSession));
    let pinned = app
        .command_service
        .list_sessions()
        .unwrap()
        .into_iter()
        .find(|session| session.id == session_id)
        .unwrap()
        .pinned;
    assert!(!pinned);
}

#[test]
fn word_deletion_handles_unicode_and_word_boundaries() {
    let mut app = App {
        prompt: "alpha beta gamma".into(),
        cursor_position: 16,
        ..App::default()
    };
    app.handle_input_key(Input::DeleteWord);
    assert_eq!(app.prompt(), "alpha beta ");
    assert_eq!(app.cursor_position(), 11);

    app.handle_input_key(Input::WordLeft);
    assert_eq!(app.cursor_position(), 6);
    app.handle_input_key(Input::DeleteWordForward);
    assert_eq!(app.prompt(), "alpha ");
}

#[test]
fn session_tabs_cycle_close_and_reopen() {
    let mut app = App::default();
    app.new_session();
    let first = app.active_session_id().unwrap();
    app.new_session();
    let second = app.active_session_id().unwrap();

    assert_eq!(app.open_session_tabs(), &[first, second]);
    assert_eq!(app.active_session_id(), Some(second));

    app.apply(UiEvent::Input(Input::SessionPrevious));
    assert_eq!(app.active_session_id(), Some(first));

    app.close_active_session_tab();
    assert_eq!(app.active_session_id(), Some(second));
    assert_eq!(app.open_session_tabs(), &[second]);

    app.apply(UiEvent::Input(Input::ReopenTab));
    assert_eq!(app.active_session_id(), Some(first));
    assert_eq!(app.open_session_tabs(), &[second, first]);

    app.apply(UiEvent::Input(Input::SelectSession(0)));
    assert_eq!(app.active_session_id(), Some(second));
}

#[test]
fn rename_updates_cached_session_tab_title() {
    let mut app = App::default();
    app.new_session();
    let session_id = app.active_session_id().unwrap();
    app.open_session_rename_dialog();
    let dialog = app.session_rename_dialog.as_mut().unwrap();
    dialog.title = "Renamed tab".into();
    dialog.cursor = dialog.title.chars().count();
    app.apply(UiEvent::Input(Input::Submit));

    assert_eq!(app.session_tab_title(session_id), Some("Renamed tab"));
}

#[test]
fn mcp_dialog_lists_disabled_servers_without_connecting() {
    use crate::tui::dialogs::McpDialogStatus;

    let mut app = App::default();
    app.mcp_servers.clear();
    app.mcp_servers.insert(
        "offline".to_string(),
        crate::config::McpServerConfig {
            enabled: false,
            command: vec!["definitely-not-a-real-mcp-command".to_string()],
            ..crate::config::McpServerConfig::default()
        },
    );
    let cwd = std::env::current_dir().unwrap();
    app.mcp_registry = crate::mcp::McpRegistry::new(&app.mcp_servers, &cwd);

    app.prompt = "/mcps".to_string();
    app.submit_prompt();

    let dialog = app.mcp_dialog().unwrap();
    assert_eq!(dialog.items.len(), 1);
    assert_eq!(dialog.items[0].name, "offline");
    assert_eq!(dialog.items[0].status, McpDialogStatus::Disabled);
    assert!(app.mcp_probing.is_empty());
}

#[test]
fn mcp_dialog_probe_results_update_status() {
    use crate::tui::dialogs::{McpDialogItem, McpDialogState, McpDialogStatus};

    let mut app = App {
        mcp_dialog: Some(McpDialogState::new(vec![McpDialogItem::new(
            "local",
            McpDialogStatus::Pending,
        )])),
        ..App::default()
    };
    app.mcp_probing.insert("local".to_string());
    app.mcp_probe_tx
        .send(McpProbeOutcome {
            name: "local".to_string(),
            result: Ok(3),
        })
        .unwrap();

    assert!(app.poll_mcp_probes());
    assert!(app.mcp_probing.is_empty());
    assert_eq!(
        app.mcp_dialog().unwrap().items[0].status,
        McpDialogStatus::Connected { tool_count: 3 }
    );

    app.mcp_probing.insert("local".to_string());
    app.mcp_probe_tx
        .send(McpProbeOutcome {
            name: "local".to_string(),
            result: Err("connection refused".to_string()),
        })
        .unwrap();
    assert!(app.poll_mcp_probes());
    assert_eq!(
        app.mcp_dialog().unwrap().items[0].status,
        McpDialogStatus::Failed("connection refused".to_string())
    );
}

#[test]
fn mcp_dialog_can_disconnect_and_show_error_details() {
    use crate::tui::dialogs::{McpDialogItem, McpDialogState, McpDialogStatus};

    let mut app = App::default();
    app.mcp_servers.clear();
    app.mcp_servers.insert(
        "local".to_string(),
        crate::config::McpServerConfig {
            enabled: true,
            command: vec!["definitely-not-a-real-mcp-command".to_string()],
            ..crate::config::McpServerConfig::default()
        },
    );
    let cwd = std::env::current_dir().unwrap();
    let registry = crate::mcp::McpRegistry::new(&app.mcp_servers, &cwd).unwrap();
    app.mcp_registry = Some(registry);
    app.mcp_dialog = Some(McpDialogState::new(vec![
        McpDialogItem::new(
            "broken",
            McpDialogStatus::Failed("connection refused".to_string()),
        ),
        McpDialogItem::new("local", McpDialogStatus::Connected { tool_count: 2 }),
    ]));

    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.mcp_dialog().unwrap().detail.is_some());
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.mcp_dialog().unwrap().detail.is_none());

    app.mcp_dialog.as_mut().unwrap().selected = 1;
    app.apply(UiEvent::Input(Input::Character(' ')));
    assert_eq!(
        app.mcp_dialog().unwrap().items[1].status,
        McpDialogStatus::Disabled
    );
    assert!(!app.mcp_registry.as_ref().unwrap().is_enabled("local"));
}

#[test]
fn settings_dialog_cycles_values_and_persists_to_config() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "clawcode_settings_test_{}_{}",
        std::process::id(),
        unique
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("clawcode.jsonc");
    std::fs::write(
        &config_path,
        "{\n  // preserve user comment\n  \"schema_version\": 1\n}\n",
    )
    .unwrap();

    let mut app = App::default();
    app.with_config_settings(crate::config::Config::default(), Some(config_path.clone()));

    app.prompt = "/settings".to_string();
    app.submit_prompt();
    assert!(app.settings_dialog().is_some());

    for ch in "verbosity".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Right));

    assert_eq!(
        app.tui_settings().verbosity,
        crate::config::TranscriptVerbosity::High
    );

    let saved_raw = std::fs::read_to_string(&config_path).unwrap();
    assert!(saved_raw.contains("// preserve user comment"));
    let reloaded = crate::config::ConfigLoader
        .parse_str(config_path.display().to_string(), &saved_raw)
        .unwrap();
    assert_eq!(
        reloaded.tui.verbosity,
        crate::config::TranscriptVerbosity::High
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn transcript_respects_verbosity_grouping_markdown_and_sticky_scroll() {
    use crate::config::{
        Config, MarkdownMode, ThinkingDisplay, ToolGrouping, TranscriptVerbosity, TuiSettingsConfig,
    };
    use crate::tui::app::tool_rows::{ToolRow, ToolRowState};

    let render_text = |app: &App| -> String {
        let backend = ratatui::backend::TestBackend::new(100, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| crate::tui::render(frame, app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = Vec::new();
        for y in 0..buffer.area.height {
            let mut row = String::new();
            for x in 0..buffer.area.width {
                row.push_str(buffer[(x, y)].symbol());
            }
            out.push(row);
        }
        out.join("\n")
    };

    let mut app = App::default();
    let now = std::time::Instant::now();
    app.set_tool_rows_for_test(vec![
        ToolRow {
            call_id: "r1".into(),
            name: "read".into(),
            desc: "src/main.rs".into(),
            arguments: r#"{"path":"src/main.rs"}"#.into(),
            output: "fn main() {}".into(),
            state: ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: now,
            expandable: false,
        },
        ToolRow {
            call_id: "r2".into(),
            name: "read".into(),
            desc: "src/lib.rs".into(),
            arguments: r#"{"path":"src/lib.rs"}"#.into(),
            output: "pub mod cli;".into(),
            state: ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: now,
            expandable: false,
        },
    ]);
    let mut body = String::from("Use **bold** text\n");
    for idx in 1..=25 {
        body.push_str(&format!("filler line {idx}\n"));
    }
    app.set_stream_parts_for_test(
        vec![
            StreamPart::User("inspect".into()),
            StreamPart::Tool("r1".into()),
            StreamPart::Tool("r2".into()),
            StreamPart::ReasoningDone {
                text: "deep internal check".into(),
                duration: std::time::Duration::from_secs(1),
            },
            StreamPart::Text(body.clone()),
        ],
        Some(0),
    );

    app.scroll_to_top();
    let default_rendered = render_text(&app);
    assert!(default_rendered.contains("Explored: 2 reads"));
    assert!(default_rendered.contains("+ Thought"));
    assert!(!default_rendered.contains("deep internal check"));
    assert!(default_rendered.contains("Use bold text"));
    assert!(!default_rendered.contains("**bold**"));

    let mut source_cfg = Config::default();
    source_cfg.tui = TuiSettingsConfig {
        markdown: MarkdownMode::Source,
        grouping: ToolGrouping::None,
        thinking: ThinkingDisplay::Show,
        ..TuiSettingsConfig::default()
    };
    app.with_config_settings(source_cfg, None);
    let source_rendered = render_text(&app);
    assert!(source_rendered.contains("**bold**"));
    assert!(source_rendered.contains("Read src/main.rs"));
    assert!(source_rendered.contains("Read src/lib.rs"));
    assert!(!source_rendered.contains("Explored: 2 reads"));
    assert!(source_rendered.contains("deep internal check"));

    let mut low_cfg = Config::default();
    low_cfg.tui = TuiSettingsConfig {
        verbosity: TranscriptVerbosity::Low,
        ..TuiSettingsConfig::default()
    };
    app.with_config_settings(low_cfg, None);
    let low_rendered = render_text(&app);
    assert!(low_rendered.contains("Activity:"));
    assert!(!low_rendered.contains("Read src/main.rs"));

    app.with_config_settings(Config::default(), None);
    app.scroll_to_bottom();
    app.scroll_up(4);
    let scrolled_rendered = render_text(&app);
    assert!(scrolled_rendered.contains("Jump to latest ↓"));
    let scroll_before = app.chat_scroll();
    app.apply(UiEvent::StreamDelta(
        "\nextra delta 1\nextra delta 2\nextra delta 3".into(),
    ));
    assert!(
        app.chat_scroll() > scroll_before,
        "sticky scroll should compensate for newly streamed lines while scrolled up"
    );
    let after_delta_rendered = render_text(&app);
    assert!(after_delta_rendered.contains("Jump to latest ↓"));

    let badge_rect = app
        .jump_to_latest_area()
        .expect("Jump to latest badge rect recorded");
    app.apply(UiEvent::MouseClick {
        x: badge_rect.x + 1,
        y: badge_rect.y,
    });
    assert_eq!(app.chat_scroll(), 0);
}
