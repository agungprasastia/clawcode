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
