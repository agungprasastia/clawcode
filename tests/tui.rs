use clawcode::tui::{App, Input, UiEvent, UiEventQueue, render_to_test_backend, runtime_step};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

#[test]
fn quit_input_stops_app() {
    let mut app = App::default();

    app.apply(UiEvent::Input(Input::Quit));

    assert!(!app.is_running());
}

#[test]
fn workbench_layout_renders_at_desktop_and_small_terminal_sizes() {
    let app = App::default();

    render_to_test_backend(&app, 120, 40).unwrap();
    render_to_test_backend(&app, 80, 24).unwrap();
    render_to_test_backend(&app, 40, 12).unwrap();

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut rendered_lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        rendered_lines.push(line);
    }
    let text = rendered_lines.join("\n");
    assert!(text.contains("CLAWCODE"));
    assert!(text.contains("/plan"));
    assert!(text.contains("/build"));
    assert!(text.contains("PLAN"));
    assert!(text.contains("clawcode v0.1.0"));

    let backend_80 = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal_80 = ratatui::Terminal::new(backend_80).unwrap();
    terminal_80
        .draw(|f| clawcode::tui::render(f, &app))
        .unwrap();
    let buffer_80 = terminal_80.backend().buffer();
    let mut lines_80 = Vec::new();
    for y in 0..buffer_80.area.height {
        let mut line = String::new();
        for x in 0..buffer_80.area.width {
            line.push_str(buffer_80[(x, y)].symbol());
        }
        lines_80.push(line);
    }
    let text_80 = lines_80.join("\n");
    assert!(text_80.contains("CLAWCODE"));
    assert!(text_80.contains("/plan"));
    assert!(text_80.contains("clawcode v0.1.0"));
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
fn resize_is_translated_and_processed_by_runtime_step() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);
    let mut draws = 0;

    runtime_step(&mut app, &mut events, Some(Event::Resize(132, 43)), |_| {
        draws += 1;
        Ok::<_, std::convert::Infallible>(())
    })
    .unwrap();

    assert_eq!(draws, 1);
}

#[test]
fn idle_event_loop_iteration_draws_zero_frames() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);
    let mut draws = 0;

    runtime_step(&mut app, &mut events, None, |_| {
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

    runtime_step(&mut app, &mut events, None, |_| {
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

#[test]
fn submit_executes_slash_command_in_tui_app() {
    let mut app = App::default();
    app.apply_batch([
        UiEvent::Input(Input::Character('/')),
        UiEvent::Input(Input::Character('b')),
        UiEvent::Input(Input::Character('u')),
        UiEvent::Input(Input::Character('i')),
        UiEvent::Input(Input::Character('l')),
        UiEvent::Input(Input::Character('d')),
        UiEvent::Input(Input::Submit),
    ]);
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Build);
    assert!(app.prompt().is_empty());
}

#[test]
fn submit_executes_help_command_in_tui_app() {
    let mut app = App::default();
    app.apply_batch([
        UiEvent::Input(Input::Character('/')),
        UiEvent::Input(Input::Character('h')),
        UiEvent::Input(Input::Character('e')),
        UiEvent::Input(Input::Character('l')),
        UiEvent::Input(Input::Character('p')),
        UiEvent::Input(Input::Submit),
    ]);
    assert!(app.diagnostic().contains("/plan"));
    assert!(app.diagnostic().contains("/build"));
    assert!(app.prompt().is_empty());
}

#[test]
fn tab_key_toggles_mode_between_plan_and_build() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);

    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Plan);

    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Build);

    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Plan);
}

#[test]
fn retained_transcript_is_bounded_with_visible_truncation_marker() {
    let mut app = App::default();

    for _ in 0..10 {
        app.apply(UiEvent::StreamDelta("x".repeat(App::MAX_TRANSCRIPT_BYTES)));
    }

    assert!(app.transcript().len() <= App::MAX_TRANSCRIPT_BYTES);
    assert!(app.transcript().contains(App::TRUNCATION_MARKER));
}

#[test]
fn runtime_step_prioritizes_translated_cancel_and_quit_during_stream_flood() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);
    for _ in 0..10_000 {
        events.push(UiEvent::StreamDelta("x".to_owned()));
    }

    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert!(app.take_cancellation());

    for _ in 0..10_000 {
        events.push(UiEvent::StreamDelta("x".to_owned()));
    }
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();

    assert!(!app.is_running());
    assert!(events.is_empty());
}

#[test]
fn slash_command_suggestions_filter_cycle_and_autocomplete() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);

    // Initial prompt empty: no suggestions
    assert!(app.matching_suggestions().is_empty());

    // Type '/': all available suggestions listed
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Char('/'),
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert!(!app.matching_suggestions().is_empty());
    assert_eq!(app.selected_suggestion_index(), 0);

    // Arrow Down cycles to next suggestion
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert_eq!(app.selected_suggestion_index(), 1);

    // Arrow Up cycles back
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert_eq!(app.selected_suggestion_index(), 0);

    // Type 'b': filters suggestions to /build
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert_eq!(app.matching_suggestions()[0].name, "/build");

    // Press Tab: autocompletes /build into prompt
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert_eq!(app.prompt(), "/build");

    // Press Enter: submits and executes /build command
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Build);
}

#[test]
fn slash_command_suggestion_submits_directly_on_enter() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);

    // Type "/co": matches /connect
    for ch in "/co".chars() {
        runtime_step(
            &mut app,
            &mut events,
            Some(Event::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            ))),
            |_| Ok::<_, std::convert::Infallible>(()),
        )
        .unwrap();
    }
    assert_eq!(app.matching_suggestions()[0].name, "/connect");

    // Press Enter directly: should execute /connect without requiring manual Tab completion
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();

    // /connect executed -> provider connected or diagnostic set
    assert!(!app.selected_provider().is_empty() || !app.diagnostic().is_empty());
    assert!(app.prompt().is_empty());

    // Also verify /new template suggestion on Enter populates template
    for ch in "/n".chars() {
        runtime_step(
            &mut app,
            &mut events,
            Some(Event::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            ))),
            |_| Ok::<_, std::convert::Infallible>(()),
        )
        .unwrap();
    }
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();
    assert_eq!(app.prompt(), "/new ");
    assert!(app.diagnostic().contains("usage: /new <title>"));
}

#[test]
fn renders_with_command_popup_active_without_panic() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);

    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Char('/'),
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();

    assert!(render_to_test_backend(&app, 120, 30).is_ok());
    assert!(render_to_test_backend(&app, 60, 15).is_ok());
}

#[test]
fn submitting_non_slash_prompt_adds_to_transcript_and_sets_active_status() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);

    for c in "explain this code".chars() {
        runtime_step(
            &mut app,
            &mut events,
            Some(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            ))),
            |_| Ok::<_, std::convert::Infallible>(()),
        )
        .unwrap();
    }
    assert_eq!(app.prompt(), "explain this code");

    // Press Enter to submit freeform user prompt
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();

    // Prompt input should be cleared
    assert_eq!(app.prompt(), "");
    // Transcript should contain the formatted prompt turn
    assert!(app.transcript().contains("> explain this code"));
    // Status should be Active
    assert_eq!(
        app.conversation_status(),
        clawcode::tui::ConversationStatus::Active
    );
    // Session should be created automatically
    assert!(app.active_session_id().is_some());
    // View switches to chat rendering seamlessly
    assert!(render_to_test_backend(&app, 120, 30).is_ok());
    assert!(render_to_test_backend(&app, 60, 15).is_ok());
}

#[test]
fn multiple_user_prompts_and_stream_deltas_accumulate() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(8);

    // Submit prompt 1
    for c in "first prompt".chars() {
        runtime_step(
            &mut app,
            &mut events,
            Some(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            ))),
            |_| Ok::<_, std::convert::Infallible>(()),
        )
        .unwrap();
    }
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();

    // Stream response delta
    events.push(UiEvent::StreamDelta("First answer.".into()));
    app.apply_pending(&mut events);

    // Submit prompt 2
    for c in "second prompt".chars() {
        runtime_step(
            &mut app,
            &mut events,
            Some(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            ))),
            |_| Ok::<_, std::convert::Infallible>(()),
        )
        .unwrap();
    }
    runtime_step(
        &mut app,
        &mut events,
        Some(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        |_| Ok::<_, std::convert::Infallible>(()),
    )
    .unwrap();

    assert!(app.transcript().contains("> first prompt"));
    assert!(app.transcript().contains("First answer."));
    assert!(app.transcript().contains("> second prompt"));
}

#[test]
fn sessions_panel_renders_grouped_and_esc_clears() {
    let mut app = App::default();
    let sessions = vec![
        clawcode::persistence::Session {
            id: 1,
            title: "planning".into(),
            workspace_id: 1,
            status: clawcode::persistence::SessionStatus::Running,
            pinned: true,
        },
        clawcode::persistence::Session {
            id: 2,
            title: "spike".into(),
            workspace_id: 1,
            status: clawcode::persistence::SessionStatus::Idle,
            pinned: false,
        },
        clawcode::persistence::Session {
            id: 3,
            title: "other repo".into(),
            workspace_id: 2,
            status: clawcode::persistence::SessionStatus::Idle,
            pinned: false,
        },
    ];
    app.apply_command_output(clawcode::cli::CommandOutput::Sessions(sessions));

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut rendered_lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        rendered_lines.push(line);
    }
    let text = rendered_lines.join("\n");
    assert!(text.contains("Sessions"));
    assert!(text.contains("planning"));
    assert!(text.contains("spike"));
    assert!(text.contains("other repo"));
    assert!(text.contains("ws#1"));
    assert!(text.contains("ws#2"));

    // Esc closes the panel instead of quitting the app.
    app.apply(UiEvent::Input(Input::Quit));
    assert!(app.is_running());
    assert!(app.session_listings().is_empty());

    // Rendering returns to the normal home view.
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut after_lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        after_lines.push(line);
    }
    let after = after_lines.join("\n");
    assert!(after.contains("CLAWCODE"));
    assert!(!after.contains("other repo"));
}

#[test]
fn submitting_prompt_clears_sessions_panel() {
    let mut app = App::default();
    app.apply_command_output(clawcode::cli::CommandOutput::Sessions(vec![
        clawcode::persistence::Session {
            id: 1,
            title: "planning".into(),
            workspace_id: 1,
            status: clawcode::persistence::SessionStatus::Running,
            pinned: false,
        },
    ]));

    app.apply(UiEvent::Input(Input::Character('h')));
    app.apply(UiEvent::Input(Input::Submit));

    assert!(app.session_listings().is_empty());
}

#[test]
fn status_bar_and_input_card_visual_parity() {
    let mut app = App::default();
    app.set_git_branch(Some("feature-tui-polish".into()));
    app.set_placeholder("Ask anything... \"Refactor this function\"".into());

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");

    // Left status bar has git branch
    assert!(text.contains(":feature-tui-polish"));
    // Right status bar has version
    assert!(text.contains("clawcode v0.1.0"));
    // Input card has vertical accent bar
    assert!(text.contains("┃"));
    // Input card has ghost text placeholder
    assert!(text.contains("Ask anything... \"Refactor this function\""));
    // Input card has Plan badge
    assert!(text.contains("[PLAN]"));
    // Input card has bottom cap
    assert!(text.contains("╹"));
    assert!(text.contains("▀"));
    // No unwanted prompt prefix `› ` in the input box
    assert!(!text.contains("› Ask anything"));

    // Docking check: in 100x30 terminal, input card is docked at the bottom
    // Row 23: placeholder, Row 25: [PLAN], Row 26: cap, Row 27: hints, Row 29: status bar
    assert!(lines[23].contains("Ask anything..."));
    assert!(lines[25].contains("[PLAN]"));
    assert!(lines[26].contains("╹"));
    assert!(lines[27].contains("Enter"));
    assert!(lines[29].contains(":feature-tui-polish"));

    // Toggle mode to Build and check [BUILD]
    app.toggle_mode();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines_build = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines_build.push(line);
    }
    let text_build = lines_build.join("\n");
    assert!(text_build.contains("[BUILD]"));
}

#[test]
fn placeholder_rotation_on_submit() {
    let mut app = App::default();
    assert!(app.placeholder().starts_with("Ask anything..."));

    app.apply(UiEvent::Input(Input::Character('h')));
    app.apply(UiEvent::Input(Input::Character('i')));
    app.apply(UiEvent::Input(Input::Submit));

    assert_eq!(app.prompt(), "");
    assert!(app.placeholder().starts_with("Ask anything..."));
}

#[test]
fn chat_view_renders_user_accent_border_and_ai_metadata_badge() {
    let mut app = App::default();
    // Submit user prompt
    for c in "write unit tests".chars() {
        app.apply(UiEvent::Input(Input::Character(c)));
    }
    app.apply(UiEvent::Input(Input::Submit));

    // Simulate response text delta
    app.apply(UiEvent::StreamDelta("Generated tests successfully.".into()));

    // Attach turn metrics
    app.set_metrics(Some(clawcode::provider::TurnMetrics {
        duration: std::time::Duration::from_millis(1500),
        usage: Some(clawcode::provider::Usage {
            input_tokens: 120,
            output_tokens: 45,
        }),
        finish_reason: Some(clawcode::provider::FinishReason::Stop),
        provider: "anthropic".into(),
        model: "claude-3-7-sonnet".into(),
    }));

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");

    // Chat view replaces "> write unit tests" with styled "┃ write unit tests"
    assert!(text.contains("┃ write unit tests"));
    assert!(!text.contains("> write unit tests"));
    // Contains AI response text
    assert!(text.contains("Generated tests successfully."));
    // Contains AI metadata badge icon and components
    assert!(text.contains("▣"));
    assert!(text.contains("PLAN"));
    assert!(text.contains("claude-3-7-sonnet"));
    assert!(text.contains("30t/s"));
    assert!(text.contains("1.5s"));
}

#[test]
fn models_dialog_opens_navigates_filters_and_selects_model() {
    let mut app = App::default();

    // Opening models dialog via /models slash command
    for ch in "/models".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));

    assert!(app.models_dialog().is_some());
    // Typing a filter character filters items
    app.apply(UiEvent::Input(Input::Character('c')));
    app.apply(UiEvent::Input(Input::Character('l')));
    app.apply(UiEvent::Input(Input::Character('a')));
    app.apply(UiEvent::Input(Input::Character('u')));
    app.apply(UiEvent::Input(Input::Character('d')));
    app.apply(UiEvent::Input(Input::Character('e')));
    assert_eq!(app.models_dialog().unwrap().filter, "claude");

    let filtered = app.models_dialog().unwrap().filtered_items();
    assert!(!filtered.is_empty());
    for item in &filtered {
        assert!(item.id.to_lowercase().contains("claude"));
    }

    // Enter selects the currently focused model and closes dialog
    let selected_id = app
        .models_dialog()
        .unwrap()
        .selected_model()
        .unwrap()
        .id
        .clone();
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.models_dialog().is_none());
    assert_eq!(app.selected_model(), selected_id);
    assert!(app.diagnostic().contains(&selected_id));

    // Reopen and test Esc dismisses without quitting
    for ch in "/model".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.models_dialog().is_some());

    app.apply(UiEvent::Input(Input::Quit));
    assert!(app.models_dialog().is_none());
    assert!(app.is_running());
}

#[test]
fn models_dialog_renders_centered_with_selection_indicators() {
    let mut app = App::default();

    // Trigger dialog
    for ch in "/models".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.models_dialog().is_some());

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");

    // Title and count
    assert!(text.contains("Select Model"));
    // Search filter bar
    assert!(text.contains("Search:"));
    // Active / cursor glyphs
    assert!(text.contains("›"));
    assert!(text.contains("●"));
    // Keybinding hints
    assert!(text.contains("navigate"));
    assert!(text.contains("select"));
    assert!(text.contains("close"));
    assert!(text.contains("filter"));
}

#[test]
fn slash_model_autocompletion_and_popup() {
    let mut app = App::default();

    // Type /model (with space)
    for ch in "/model ".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }

    let model_suggestions = app.matching_model_suggestions();
    assert!(!model_suggestions.is_empty());

    // Cycle suggestions with Down
    app.apply(UiEvent::Input(Input::Down));
    assert_eq!(app.selected_suggestion_index(), 1);

    // Tab autocompletes the selected model
    app.apply(UiEvent::Input(Input::ToggleMode));
    assert!(app.prompt().starts_with("/model "));
    assert_eq!(app.prompt(), format!("/model {}", model_suggestions[1]));
}

#[test]
fn home_state_ticks_and_animates_blinking_frames() {
    let mut state = clawcode::tui::HomeState::new();
    assert_eq!(state.frame(), 0);

    // Phase 0: duration 14
    for _ in 0..13 {
        state.tick();
        assert_eq!(state.frame(), 0);
    }
    state.tick();
    assert_eq!(state.frame(), 1); // Phase 1: blink (frame 1)

    // Phase 1: duration 7
    for _ in 0..6 {
        state.tick();
        assert_eq!(state.frame(), 1);
    }
    state.tick();
    assert_eq!(state.frame(), 0); // Phase 2: eyes open (frame 0)

    // Phase 2: duration 7
    for _ in 0..7 {
        state.tick();
    }
    assert_eq!(state.frame(), 1); // Phase 3: blink (frame 1)

    // Phase 3: duration 7
    for _ in 0..7 {
        state.tick();
    }
    assert_eq!(state.frame(), 0); // Phase 4: eyes open (frame 0)

    // Phase 4: duration 14
    for _ in 0..14 {
        state.tick();
    }
    assert_eq!(state.frame(), 0); // Wraps back to Phase 0
}

#[test]
fn home_screen_renders_mascot_and_reflects_frame_transition() {
    let mut app = App::default();

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");

    // Frame 0 has open eyes: █▟▟▜
    assert!(text.contains("█▟▟▜"));
    assert!(text.contains("CLAWCODE"));
    assert!(text.contains("clawcode v0.1.0"));

    // Advance 14 ticks to trigger blink frame 1
    for _ in 0..14 {
        app.home_state_mut().tick();
    }
    assert_eq!(app.home_state().frame(), 1);

    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer_blink = terminal.backend().buffer();
    let mut lines_blink = Vec::new();
    for y in 0..buffer_blink.area.height {
        let mut line = String::new();
        for x in 0..buffer_blink.area.width {
            line.push_str(buffer_blink[(x, y)].symbol());
        }
        lines_blink.push(line);
    }
    let text_blink = lines_blink.join("\n");

    // Frame 1 has blinking eyes: █▙▟▜
    assert!(text_blink.contains("█▙▟▜"));
}

#[test]
fn prompt_history_navigation_and_draft_preservation() {
    let mut app = App::default();

    // 1. Initially empty history; Up does nothing
    app.apply(UiEvent::Input(Input::Up));
    assert_eq!(app.prompt(), "");

    // 2. Submit "alpha"
    for c in "alpha".chars() {
        app.apply(UiEvent::Input(Input::Character(c)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert_eq!(app.prompt_history(), &["alpha"]);

    // 3. Submit "beta"
    for c in "beta".chars() {
        app.apply(UiEvent::Input(Input::Character(c)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert_eq!(app.prompt_history(), &["alpha", "beta"]);

    // 4. Consecutive duplicate "beta" is not re-added
    for c in "beta".chars() {
        app.apply(UiEvent::Input(Input::Character(c)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert_eq!(app.prompt_history(), &["alpha", "beta"]);

    // 5. User types draft "drafting"
    for c in "drafting".chars() {
        app.apply(UiEvent::Input(Input::Character(c)));
    }
    assert_eq!(app.prompt(), "drafting");

    // 6. Up -> recalled "beta"
    app.apply(UiEvent::Input(Input::Up));
    assert_eq!(app.prompt(), "beta");

    // 7. Up -> recalled "alpha"
    app.apply(UiEvent::Input(Input::Up));
    assert_eq!(app.prompt(), "alpha");

    // 8. Up at top -> stays at "alpha"
    app.apply(UiEvent::Input(Input::Up));
    assert_eq!(app.prompt(), "alpha");

    // 9. Down -> "beta"
    app.apply(UiEvent::Input(Input::Down));
    assert_eq!(app.prompt(), "beta");

    // 10. Down past newest -> restores draft
    app.apply(UiEvent::Input(Input::Down));
    assert_eq!(app.prompt(), "drafting");

    // 11. Typing character resets history navigation state
    app.apply(UiEvent::Input(Input::Up));
    assert_eq!(app.prompt(), "beta");
    app.apply(UiEvent::Input(Input::Character('!')));
    assert_eq!(app.prompt(), "beta!");
    // Pressing Down now should not restore "drafting" because history navigation was reset
    app.apply(UiEvent::Input(Input::Down));
    assert_eq!(app.prompt(), "beta!");
}

#[test]
fn original_logo_preserved_and_rendered() {
    let app = App::default();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");

    // Original block ASCII logo elements
    assert!(text.contains("██████╗"));
    assert!(text.contains("AUTONOMOUS AGENT WORKBENCH"));
}

#[test]
fn theme_switching_via_slash_command_and_dialog() {
    let mut app = App::default();
    assert_eq!(app.theme(), clawcode::tui::ThemeKind::ClawcodeDark);

    // Direct /theme command
    for ch in "/theme catppuccin".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert_eq!(app.theme(), clawcode::tui::ThemeKind::CatppuccinMocha);
    assert!(app.diagnostic().contains("Catppuccin Mocha"));

    for ch in "/theme dracula".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert_eq!(app.theme(), clawcode::tui::ThemeKind::Dracula);

    // Interactive /themes dialog
    for ch in "/themes".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.themes_dialog().is_some());

    // Filter themes
    for ch in "nord".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    assert_eq!(app.themes_dialog().unwrap().filter, "nord");
    assert_eq!(app.themes_dialog().unwrap().filtered_items().len(), 1);

    // Submit selection
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.themes_dialog().is_none());
    assert_eq!(app.theme(), clawcode::tui::ThemeKind::Nord);

    // Render themes dialog test
    app.apply(UiEvent::Input(Input::Character('/')));
    for ch in "themes".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.themes_dialog().is_some());

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");
    assert!(text.contains("Color Theme"));
    assert!(text.contains("Filter:"));
    assert!(text.contains("navigate"));
    assert!(text.contains("select"));
    assert!(text.contains("close"));

    // Close with Cancel / Esc
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.themes_dialog().is_none());
}

#[test]
fn agents_dialog_opens_navigates_filters_and_switches_mode() {
    let mut app = App::default();
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Plan);

    // Open /agents dialog
    for ch in "/agents".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.agents_dialog().is_some());

    // Navigate to Build Agent
    app.apply(UiEvent::Input(Input::Down));
    let selected = app.agents_dialog().unwrap().selected_agent().unwrap();
    assert_eq!(selected.id, "build");

    // Select with Enter
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.agents_dialog().is_none());
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Build);

    // Reopen and filter
    for ch in "/agents".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.agents_dialog().is_some());

    for ch in "plan".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    assert_eq!(app.agents_dialog().unwrap().filter, "plan");
    let filtered = app.agents_dialog().unwrap().filtered_items();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "plan");

    // Render agents dialog test
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");
    assert!(text.contains("Select Agent Mode"));
    assert!(text.contains("Filter:"));
    assert!(text.contains("Plan Agent"));

    // Select Plan
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.agents_dialog().is_none());
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Plan);
}

#[test]
fn which_key_shortcuts_cheatsheet_toggle_and_direct_key_actions() {
    let mut app = App::default();
    assert!(!app.which_key().visible);

    // Trigger via Ctrl+X (WhichKey)
    app.apply(UiEvent::Input(Input::WhichKey));
    assert!(app.which_key().visible);

    // Render WhichKey test
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");
    assert!(text.contains("Keyboard Shortcuts (Cheatsheet)"));
    assert!(text.contains("Toggle Plan/Build"));
    assert!(text.contains("Open Agents dialog"));

    // Direct key 'a' opens agents dialog and closes which_key
    app.apply(UiEvent::Input(Input::Character('a')));
    assert!(!app.which_key().visible);
    assert!(app.agents_dialog().is_some());

    // Close agents dialog
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.agents_dialog().is_none());

    // Trigger via /keys
    for ch in "/keys".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.which_key().visible);

    // Direct key 't' opens themes dialog
    app.apply(UiEvent::Input(Input::Character('t')));
    assert!(!app.which_key().visible);
    assert!(app.themes_dialog().is_some());

    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.themes_dialog().is_none());

    // Direct mode toggle via 'b' and 'p'
    app.apply(UiEvent::Input(Input::WhichKey));
    app.apply(UiEvent::Input(Input::Character('b')));
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Build);

    app.apply(UiEvent::Input(Input::WhichKey));
    app.apply(UiEvent::Input(Input::Character('p')));
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Plan);

    // Esc dismisses which_key
    app.apply(UiEvent::Input(Input::WhichKey));
    assert!(app.which_key().visible);
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(!app.which_key().visible);
}

#[test]
fn status_dialog_clear_compact_and_copy_parity() {
    let mut app = App::default();

    // 1. /status opens status dialog
    for ch in "/status".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.status_dialog().is_some());

    // 2. Render status dialog
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    let text = lines.join("\n");
    assert!(text.contains("System Status & Diagnostics"));
    assert!(text.contains("Agent Mode"));
    assert!(text.contains("Active Model"));
    assert!(text.contains("Git Branch"));

    // 3. Esc dismisses status dialog
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.status_dialog().is_none());

    // 4. WhichKey 's' also opens status dialog
    app.apply(UiEvent::Input(Input::WhichKey));
    app.apply(UiEvent::Input(Input::Character('s')));
    assert!(app.status_dialog().is_some());
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.status_dialog().is_none());

    // 5. Add transcript content and test /copy
    app.apply(UiEvent::StreamDelta(
        "Generated AI reply text for testing".to_string(),
    ));
    assert!(!app.transcript().is_empty());

    for ch in "/copy".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.diagnostic().contains("transcript copied"));

    // 6. Test /compact
    let large_transcript = "x".repeat(2000);
    app.apply(UiEvent::StreamDelta(large_transcript));
    assert!(app.transcript().len() > 1500);

    for ch in "/compact".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.diagnostic().contains("compacted"));
    assert!(app.transcript().contains("[earlier transcript compacted]"));

    // 7. Clear via Ctrl+L (Input::Clear)
    app.apply(UiEvent::Input(Input::Clear));
    assert!(app.transcript().is_empty());
    assert_eq!(app.diagnostic(), "screen cleared");

    // 8. Clear via /clear command
    app.apply(UiEvent::StreamDelta("New response".to_string()));
    assert!(!app.transcript().is_empty());

    for ch in "/clear".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.transcript().is_empty());
    assert_eq!(app.diagnostic(), "screen cleared");
}

#[test]
fn wave_spinner_renders_and_animates_across_frames() {
    let mut app = App::default();
    assert_eq!(app.wave_spinner().spans().len(), clawcode::tui::WaveSpinner::WIDTH as usize);

    let wide_spans = app.wave_spinner().spans_for_width(clawcode::tui::WaveSpinner::WIDTH);
    assert_eq!(wide_spans.len(), clawcode::tui::WaveSpinner::WIDTH as usize);

    let compact_spans = app.wave_spinner().spans_for_width(1);
    assert_eq!(compact_spans.len(), 1);

    app.set_mode(clawcode::tui::ConversationMode::Build);
    let build_spans = app.wave_spinner().spans();
    assert_eq!(build_spans.len(), clawcode::tui::WaveSpinner::WIDTH as usize);
}

#[test]
fn typewriter_pacing_animates_in_chat_view_and_renders_cursor() {
    let mut app = App::default();
    app.submit_user_prompt("Halo");
    assert!(app.transcript().contains("> Halo"));

    // Feed a fast delta (all at once)
    app.apply_conversation(clawcode::conversation::ConversationEvent::TextDelta(
        "Halo. Butuh apa?".into(),
    ));

    // Typewriter is now active and pacing output
    assert!(app.is_typing());
    assert!(app.is_streaming_active());

    // Render frame to ensure cursor and wave spinner render without issues
    render_to_test_backend(&app, 80, 24).unwrap();

    // Advancing tick drains small increments (typewriter effect)
    let initial_transcript_len = app.transcript().len();
    std::thread::sleep(std::time::Duration::from_millis(50));
    app.tick();
    assert!(app.transcript().len() > initial_transcript_len);

    // Finish conversation
    app.apply_conversation(clawcode::conversation::ConversationEvent::Finished(
        clawcode::provider::FinishReason::Stop,
    ));

    // Full transcript is now flushed
    assert!(app.transcript().contains("Halo. Butuh apa?"));
    assert!(!app.is_typing());
    render_to_test_backend(&app, 80, 24).unwrap();
}

#[test]
fn chat_scrolling_mouse_and_keyboard_controls() {
    let mut app = App::default();
    assert_eq!(app.chat_scroll(), 0);

    // Populate transcript with multi-line text
    let mut transcript_lines = String::new();
    for i in 1..=50 {
        transcript_lines.push_str(&format!("Line #{i}: This is transcript content\n"));
    }
    app.apply(UiEvent::StreamDelta(transcript_lines));

    // 1. Mouse wheel / Shift+Up scroll up
    app.apply(UiEvent::Input(Input::ScrollUp));
    assert_eq!(app.chat_scroll(), 3);

    // 2. PageUp scrolls 15 lines
    app.apply(UiEvent::Input(Input::PageUp));
    assert_eq!(app.chat_scroll(), 18);

    // 3. Mouse wheel / Shift+Down scroll down
    app.apply(UiEvent::Input(Input::ScrollDown));
    assert_eq!(app.chat_scroll(), 15);

    // 4. PageDown scrolls down 15 lines
    app.apply(UiEvent::Input(Input::PageDown));
    assert_eq!(app.chat_scroll(), 0);

    // 5. Home key jumps to top
    app.apply(UiEvent::Input(Input::Home));
    assert_eq!(app.chat_scroll(), u16::MAX);

    // 6. End key snaps back to bottom
    app.apply(UiEvent::Input(Input::End));
    assert_eq!(app.chat_scroll(), 0);

    // 7. Render with scroll offset shows scroll indicator
    app.apply(UiEvent::Input(Input::ScrollUp));
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut rendered_lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        rendered_lines.push(line);
    }
    let rendered = rendered_lines.join("\n");
    assert!(rendered.contains("lines up (End to bottom)"));

    // 8. Submitting a new prompt resets scroll to 0
    app.apply(UiEvent::Input(Input::Character('h')));
    app.apply(UiEvent::Input(Input::Character('i')));
    app.apply(UiEvent::Input(Input::Submit));
    assert_eq!(app.chat_scroll(), 0);
}

#[test]
fn sessions_dialog_opens_navigates_filters_and_switches_session() {
    let mut app = App::default();
    app.apply_command_output(clawcode::cli::CommandOutput::Sessions(vec![
        clawcode::persistence::Session {
            id: 1,
            title: "planning session".into(),
            workspace_id: 1,
            status: clawcode::persistence::SessionStatus::Idle,
            pinned: false,
        },
        clawcode::persistence::Session {
            id: 2,
            title: "spike optimization".into(),
            workspace_id: 1,
            status: clawcode::persistence::SessionStatus::Running,
            pinned: true,
        },
        clawcode::persistence::Session {
            id: 3,
            title: "other project".into(),
            workspace_id: 2,
            status: clawcode::persistence::SessionStatus::Idle,
            pinned: false,
        },
    ]));

    assert!(app.sessions_dialog().is_some());
    assert_eq!(app.sessions_dialog().unwrap().selected, 0);

    // Navigate down
    app.apply(UiEvent::Input(Input::Down));
    assert_eq!(app.sessions_dialog().unwrap().selected, 1);
    let selected = app.sessions_dialog().unwrap().selected_session().unwrap();
    assert_eq!(selected.id, 2);

    // Typing a filter string
    for ch in "spike".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    assert_eq!(app.sessions_dialog().unwrap().filter, "spike");
    let filtered = app.sessions_dialog().unwrap().filtered_items();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, 2);

    // Submit selection -> switches session and closes dialog
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.sessions_dialog().is_none());
    assert_eq!(app.active_session_id(), Some(2));
    assert!(app.diagnostic().contains("switched to session #2"));

    // Esc closes dialog without switching
    app.apply_command_output(clawcode::cli::CommandOutput::Sessions(vec![
        clawcode::persistence::Session {
            id: 1,
            title: "planning session".into(),
            workspace_id: 1,
            status: clawcode::persistence::SessionStatus::Idle,
            pinned: false,
        },
    ]));
    assert!(app.sessions_dialog().is_some());
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.sessions_dialog().is_none());
    assert_eq!(app.active_session_id(), Some(2)); // Still session 2
}

#[test]
fn sessions_switching_hydrates_messages_from_database() {
    let db = clawcode::persistence::Db::open_in_memory().unwrap();
    let s1 = db.create_session("First Session").unwrap();
    db.append_message(s1.id, "user", "What is Rust?").unwrap();
    db.append_message(s1.id, "assistant", "Rust is a fast systems language.").unwrap();

    let s2 = db.create_session("Second Session").unwrap();
    db.append_message(s2.id, "user", "Explain SQLite").unwrap();
    db.append_message(s2.id, "assistant", "SQLite is an embedded database engine.").unwrap();

    let mut app = App::default();
    let service = clawcode::cli::CommandService::with_db(clawcode::cli::CliDiscovery::new(), db);
    app.set_command_service(service);

    // Switch to session 1 -> hydrates messages from SQLite
    app.switch_session(s1.id);
    assert_eq!(app.active_session_id(), Some(s1.id));
    assert!(app.transcript().contains("> What is Rust?"));
    assert!(app.transcript().contains("Rust is a fast systems language."));

    // Switch to session 2 -> hydrates session 2 messages
    app.switch_session(s2.id);
    assert_eq!(app.active_session_id(), Some(s2.id));
    assert!(app.transcript().contains("> Explain SQLite"));
    assert!(app.transcript().contains("SQLite is an embedded database engine."));
    assert!(!app.transcript().contains("What is Rust?"));

    // Switch back to session 1 -> previous state intact
    app.switch_session(s1.id);
    assert_eq!(app.active_session_id(), Some(s1.id));
    assert!(app.transcript().contains("> What is Rust?"));
    assert!(app.transcript().contains("Rust is a fast systems language."));
}

#[test]
fn which_key_r_opens_sessions_dialog() {
    let db = clawcode::persistence::Db::open_in_memory().unwrap();
    let _s = db.create_session("Mock session").unwrap();

    let mut app = App::default();
    let service = clawcode::cli::CommandService::with_db(clawcode::cli::CliDiscovery::new(), db);
    app.set_command_service(service);

    // Open WhichKey
    app.apply(UiEvent::Input(Input::WhichKey));
    assert!(app.which_key().visible);

    // 'r' opens sessions dialog
    app.apply(UiEvent::Input(Input::Character('r')));
    assert!(!app.which_key().visible);
    assert!(app.sessions_dialog().is_some());
}

#[test]
fn tool_execution_formats_crabcode_style_and_tracks_active_tool() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    // Submit user prompt so chat view is active
    app.apply(UiEvent::Input(Input::Character('r')));
    app.apply(UiEvent::Input(Input::Submit));
    // 1. Tool starts executing
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".to_string(),
        payload_json: serde_json::json!({
            "name": "read_file",
            "arguments": { "path": "src/cli/mod.rs" }
        }).to_string(),
    }).unwrap();

    app.poll_runtime();
    assert!(app.active_tool().is_some());
    let active = app.active_tool().unwrap();
    assert_eq!(active.name, "read_file");
    assert_eq!(active.desc, "src/cli/mod.rs");

    // Rendering while tool is active displays the ⬡ marker and "Reading src/cli/mod.rs..."
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("⬡"));
    assert!(text.contains("Reading src/cli/mod.rs..."));
    // Hints row shows active tool line, not "ready"
    assert!(!text.contains("● ready"));
    assert!(text.contains("read_file: src/cli/mod.rs"));

    // 2. Tool finishes execution with 71 lines output
    let dummy_output = (0..71).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "read_file",
            "arguments": { "path": "src/cli/mod.rs" },
            "success": true,
            "output": dummy_output
        }).to_string(),
    }).unwrap();
    app.poll_runtime();
    assert!(app.active_tool().is_none());

    // Clean Crabcode-style format in transcript:
    // ⬢ Read src/cli/mod.rs
    //   └ 71 lines
    assert!(app.transcript().contains("⬢ Read src/cli/mod.rs"));
    assert!(app.transcript().contains("  └ 71 lines"));

    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("⬢ Read src/cli/mod.rs"));
    assert!(text.contains("└ 71 lines"));
}

#[test]
fn tool_failure_formats_cleanly_with_error_branch() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".to_string(),
        payload_json: serde_json::json!({
            "name": "read_file",
            "arguments": { "path": "foo.rs" }
        }).to_string(),
    }).unwrap();
    app.poll_runtime();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "read_file",
            "arguments": { "path": "foo.rs" },
            "success": false,
            "output": "Error executing read_file: file not found"
        }).to_string(),
    }).unwrap();
    app.poll_runtime();

    assert!(app.transcript().contains("⬢ Read foo.rs"));
    assert!(app.transcript().contains("  └ failed: file not found"));

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("⬢ Read foo.rs"));
    assert!(text.contains("└ failed: file not found"));
}

#[test]
fn various_tool_entries_format_matching_crabcode_spec() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    // grep_search
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "grep_search",
            "arguments": { "query": "/" },
            "success": true,
            "output": (0..100).map(|i| format!("match {i}")).collect::<Vec<_>>().join("\n")
        }).to_string(),
    }).unwrap();

    // glob_search
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "glob_search",
            "arguments": { "pattern": "*ui*" },
            "success": true,
            "output": "src/ui/mod.rs\nsrc/ui/render.rs"
        }).to_string(),
    }).unwrap();

    // list_dir
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 3,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "list_dir",
            "arguments": { "path": "src" },
            "success": true,
            "output": (0..14).map(|i| format!("entry {i}")).collect::<Vec<_>>().join("\n")
        }).to_string(),
    }).unwrap();

    app.poll_runtime();

    assert!(app.transcript().contains("⬢ Grep /"));
    assert!(app.transcript().contains("  └ 100 lines"));

    assert!(app.transcript().contains("⬢ Glob *ui*"));
    assert!(app.transcript().contains("  └ succeeded"));

    assert!(app.transcript().contains("⬢ List src"));
    assert!(app.transcript().contains("  └ 14 entries"));
}

#[test]
fn historical_tool_lines_render_with_clean_styles() {
    let mut app = App::default();
    app.apply(UiEvent::Input(Input::Character('h')));
    app.apply(UiEvent::Input(Input::Submit));

    // Simulate historical transcript format
    app.apply(UiEvent::StreamDelta("\n⚙ [grep_search: /]\n✓ grep_search succeeded (100 lines)\n\n".into()));

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("⚙"));
    assert!(text.contains("grep_search"));
    assert!(text.contains("└"));
    assert!(text.contains("grep_search succeeded (100 lines)"));
}
