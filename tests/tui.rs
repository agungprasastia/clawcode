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
fn runtime_step_keeps_bare_q_as_prompt_text_during_stream_flood() {
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

    assert!(app.is_running());
    assert_eq!(app.prompt(), "q");
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

    // Chat view replaces "> write unit tests" with styled "▌ write unit tests"
    assert!(text.contains("▌ write unit tests"));
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
    assert!(text.contains("Filter:"));
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
    assert!(
        app.diagnostic().contains("transcript copied")
            || app.diagnostic().contains("failed to copy transcript")
    );
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
    assert_eq!(
        app.wave_spinner().spans().len(),
        clawcode::tui::WaveSpinner::WIDTH as usize
    );

    let wide_spans = app
        .wave_spinner()
        .spans_for_width(clawcode::tui::WaveSpinner::WIDTH);
    assert_eq!(wide_spans.len(), clawcode::tui::WaveSpinner::WIDTH as usize);

    let compact_spans = app.wave_spinner().spans_for_width(1);
    assert_eq!(compact_spans.len(), 1);

    app.set_mode(clawcode::tui::ConversationMode::Build);
    let build_spans = app.wave_spinner().spans();
    assert_eq!(
        build_spans.len(),
        clawcode::tui::WaveSpinner::WIDTH as usize
    );
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
    db.append_message(s1.id, "assistant", "Rust is a fast systems language.")
        .unwrap();

    let s2 = db.create_session("Second Session").unwrap();
    db.append_message(s2.id, "user", "Explain SQLite").unwrap();
    db.append_message(s2.id, "assistant", "SQLite is an embedded database engine.")
        .unwrap();

    let mut app = App::default();
    let service = clawcode::cli::CommandService::with_db(clawcode::cli::CliDiscovery::new(), db);
    app.set_command_service(service);

    // Switch to session 1 -> hydrates messages from SQLite
    app.switch_session(s1.id);
    assert_eq!(app.active_session_id(), Some(s1.id));
    assert!(app.transcript().contains("> What is Rust?"));
    assert!(
        app.transcript()
            .contains("Rust is a fast systems language.")
    );

    // Switch to session 2 -> hydrates session 2 messages
    app.switch_session(s2.id);
    assert_eq!(app.active_session_id(), Some(s2.id));
    assert!(app.transcript().contains("> Explain SQLite"));
    assert!(
        app.transcript()
            .contains("SQLite is an embedded database engine.")
    );
    assert!(!app.transcript().contains("What is Rust?"));

    // Switch back to session 1 -> previous state intact
    app.switch_session(s1.id);
    assert_eq!(app.active_session_id(), Some(s1.id));
    assert!(app.transcript().contains("> What is Rust?"));
    assert!(
        app.transcript()
            .contains("Rust is a fast systems language.")
    );
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
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert!(app.active_tool().is_some());
    let active = app.active_tool().unwrap();
    assert_eq!(active.name, "read_file");
    assert_eq!(active.desc, "src/cli/mod.rs");

    // Rendering while tool is active displays the braille spinner and "Read src/cli/mod.rs"
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains(app.wave_spinner().compact_frame()));
    assert!(text.contains("Read src/cli/mod.rs"));
    // Hints row shows clean status ("running"), not "ready"
    assert!(!text.contains("● ready"));
    assert!(text.contains("running"));

    // 2. Tool finishes execution with 71 lines output
    let dummy_output = (0..71)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
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
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();
    assert!(app.active_tool().is_none());

    assert!(
        app.tool_rows()
            .iter()
            .any(|r| r.name == "read_file" && r.state == clawcode::tui::ToolRowState::Completed)
    );

    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Read src/cli/mod.rs"));
}

#[test]
fn tool_execution_update_plan_renders_checklist_and_tracks_plan() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let payload_args = serde_json::json!({
        "explanation": "Refactor architecture",
        "plan": [
            {"step": "Selesai langkah pertama", "status": "completed"},
            {"step": "Sedang menjalankan langkah kedua", "status": "in_progress"},
            {"step": "Langkah ketiga pending", "status": "pending"}
        ]
    });

    // 1. Tool starts executing
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".to_string(),
        payload_json: serde_json::json!({
            "name": "update_plan",
            "arguments": payload_args
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();

    assert!(app.active_tool().is_some());
    let active = app.active_tool().unwrap();
    assert_eq!(active.name, "update_plan");
    assert_eq!(active.desc, "Refactor architecture");

    // 2. Tool finishes execution
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "update_plan",
            "arguments": payload_args,
            "success": true,
            "output": "Plan updated: 3 steps (1 completed, 1 in progress, 1 pending)"
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();
    assert!(app.active_tool().is_none());

    // Verify current_plan snapshot on App
    let plan = app.current_plan();
    assert_eq!(plan.len(), 3);
    assert_eq!(
        plan[0],
        (
            "Selesai langkah pertama".to_string(),
            "completed".to_string()
        )
    );
    assert_eq!(
        plan[1],
        (
            "Sedang menjalankan langkah kedua".to_string(),
            "in_progress".to_string()
        )
    );
    assert_eq!(
        plan[2],
        ("Langkah ketiga pending".to_string(), "pending".to_string())
    );

    // 3. Render in terminal
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Updated Plan"));
    assert!(text.contains("✔ Selesai langkah pertama"));
    assert!(text.contains("• Sedang menjalankan langkah kedua"));
    assert!(text.contains("□ Langkah ketiga pending"));
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
        })
        .to_string(),
    })
    .unwrap();
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
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();

    assert!(
        app.tool_rows()
            .iter()
            .any(|r| r.name == "read_file" && r.state == clawcode::tui::ToolRowState::Failed)
    );

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Read foo.rs"));
    assert!(text.contains("failed: file not found"));
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
        })
        .to_string(),
    })
    .unwrap();

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
        })
        .to_string(),
    })
    .unwrap();

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
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    assert_eq!(app.tool_rows().len(), 3);
    assert!(
        app.tool_rows()
            .iter()
            .all(|r| r.state == clawcode::tui::ToolRowState::Completed)
    );
    assert_eq!(app.stream_parts().len(), 3);

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Grep /"));
    assert!(text.contains("Glob *ui*"));
    assert!(text.contains("List src"));
}

#[test]
fn historical_tool_lines_render_with_clean_styles() {
    let mut app = App::default();
    app.submit_user_prompt("h");

    app.set_tool_rows_for_test(vec![clawcode::tui::ToolRow {
        call_id: "grep-1".to_string(),
        name: "grep_search".to_string(),
        desc: "/".to_string(),
        arguments: serde_json::json!({ "query": "/" }).to_string(),
        output: "match 1\nmatch 2".to_string(),
        state: clawcode::tui::ToolRowState::Completed,
        arguments_complete: true,
        metadata: None,
        started_at: std::time::Instant::now(),
        expandable: false,
    }]);
    app.set_stream_parts_for_test(
        vec![
            clawcode::tui::StreamPart::User("h".into()),
            clawcode::tui::StreamPart::Tool("grep-1".into()),
        ],
        None,
    );

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Grep /"));
}
#[test]
fn test_app_permission_dialog_navigation_and_decisions() {
    let mut app = App::default();
    assert!(app.permission_dialog().is_none());

    // Open dialog
    app.open_permission_dialog("bash", "rm -rf /tmp/data", "Clean temporary data");
    assert!(app.permission_dialog().is_some());
    assert_eq!(
        app.permission_dialog().unwrap().selected(),
        clawcode::tui::PermissionDecision::AllowOnce
    );

    // Tab/Right advances to AllowAlways
    app.apply(UiEvent::Input(Input::Right));
    assert_eq!(
        app.permission_dialog().unwrap().selected(),
        clawcode::tui::PermissionDecision::AllowAlways
    );

    // Enter confirms AllowAlways
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.permission_dialog().is_none());
    assert_eq!(
        app.last_permission_decision(),
        Some(clawcode::tui::PermissionDecision::AllowAlways)
    );

    // Reopen and navigate to Deny using Left
    app.open_permission_dialog("edit_file", "edit /etc/hosts", "Update host entries");
    assert_eq!(
        app.permission_dialog().unwrap().selected(),
        clawcode::tui::PermissionDecision::AllowOnce
    );
    app.apply(UiEvent::Input(Input::Left));
    assert_eq!(
        app.permission_dialog().unwrap().selected(),
        clawcode::tui::PermissionDecision::Deny
    );

    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.permission_dialog().is_none());
    assert_eq!(
        app.last_permission_decision(),
        Some(clawcode::tui::PermissionDecision::Deny)
    );
}

#[test]
fn test_app_permission_dialog_quit_or_cancel_denies() {
    let mut app = App::default();
    app.open_permission_dialog("bash", "git reset --hard", "Discard changes");
    assert!(app.permission_dialog().is_some());

    // Esc / Quit cancels and denies permission without quitting application
    app.apply(UiEvent::Input(Input::Quit));
    assert!(app.permission_dialog().is_none());
    assert_eq!(
        app.last_permission_decision(),
        Some(clawcode::tui::PermissionDecision::Deny)
    );

    // Cancel (Ctrl+C) also denies
    app.open_permission_dialog("bash", "git clean -fd", "Clean untracked files");
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.permission_dialog().is_none());
    assert_eq!(
        app.last_permission_decision(),
        Some(clawcode::tui::PermissionDecision::Deny)
    );
}

#[test]
fn test_is_sensitive_command() {
    // Dangerous commands
    assert!(clawcode::tui::is_sensitive_command("rm -rf target"));
    assert!(clawcode::tui::is_sensitive_command("rm file.txt"));
    assert!(clawcode::tui::is_sensitive_command("rm"));
    assert!(clawcode::tui::is_sensitive_command(
        "git reset --hard HEAD~1"
    ));
    assert!(clawcode::tui::is_sensitive_command("git clean -fd"));
    assert!(clawcode::tui::is_sensitive_command("mkfs.ext4 /dev/sdb1"));
    assert!(clawcode::tui::is_sensitive_command(
        "dd if=/dev/zero of=/dev/sda"
    ));
    assert!(clawcode::tui::is_sensitive_command("kill -9 1234"));
    assert!(clawcode::tui::is_sensitive_command("chmod 777 script.sh"));
    assert!(clawcode::tui::is_sensitive_command(
        "chown root:root /etc/file"
    ));

    // Safe commands
    assert!(!clawcode::tui::is_sensitive_command("ls -la"));
    assert!(!clawcode::tui::is_sensitive_command("git status"));
    assert!(!clawcode::tui::is_sensitive_command("cargo check"));
    assert!(!clawcode::tui::is_sensitive_command("echo 'hello world'"));
}

#[test]
fn test_permission_dialog_rendered_overlay() {
    let mut app = App::default();
    app.open_permission_dialog("bash", "rm -rf /tmp/test", "Remove temporary files");

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Security Confirmation"));
    assert!(text.contains("bash"));
    assert!(text.contains("rm -rf /tmp/test"));
    assert!(text.contains("Remove temporary files"));
    assert!(text.contains("[ Deny ]"));
    assert!(text.contains("[ Allow Once ]"));
    assert!(text.contains("[ Always Allow ]"));
    assert!(text.contains("←/→ or Tab to navigate"));
    assert!(text.contains("Enter to confirm"));
    assert!(text.contains("Esc to deny"));
}
#[test]
fn test_app_question_dialog_navigation_and_custom_answer() {
    let mut app = App::default();
    assert!(app.question_dialog().is_none());

    app.open_question_dialog(
        "Which frontend framework?",
        vec!["React".to_string(), "Vue".to_string()],
    );
    assert!(app.question_dialog().is_some());
    assert_eq!(app.question_dialog().unwrap().selected_answer(), "React");

    // Down moves to Vue
    app.apply(UiEvent::Input(Input::Down));
    assert_eq!(app.question_dialog().unwrap().selected_answer(), "Vue");

    // Down moves to Custom text option
    app.apply(UiEvent::Input(Input::Down));
    assert!(app.question_dialog().unwrap().typing_custom);

    // Type "Svelte"
    for c in "Svelte".chars() {
        app.apply(UiEvent::Input(Input::Character(c)));
    }
    assert_eq!(app.question_dialog().unwrap().selected_answer(), "Svelte");

    // Backspace removes last char -> "Svelt"
    app.apply(UiEvent::Input(Input::Backspace));
    assert_eq!(app.question_dialog().unwrap().selected_answer(), "Svelt");

    // Submit confirms "Svelt"
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.question_dialog().is_none());
    assert_eq!(app.last_question_answer(), Some("Svelt"));
}

#[test]
fn test_app_question_dialog_dismiss_esc() {
    let mut app = App::default();
    app.open_question_dialog("Proceed?", vec!["Yes".to_string(), "No".to_string()]);
    assert!(app.question_dialog().is_some());

    // Esc dismisses question dialog without quitting app
    app.apply(UiEvent::Input(Input::Quit));
    assert!(app.question_dialog().is_none());

    // Cancel (Ctrl+C) also dismisses
    app.open_question_dialog("Retry?", vec!["Yes".to_string()]);
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.question_dialog().is_none());
}

#[test]
fn test_app_question_dialog_rendered_overlay() {
    let mut app = App::default();
    app.open_question_dialog(
        "Which UI library?",
        vec!["Tailwind".to_string(), "Bootstrap".to_string()],
    );

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Question from Agent"));
    assert!(text.contains("Which UI library?"));
    assert!(text.contains("Tailwind"));
    assert!(text.contains("Bootstrap"));
    assert!(text.contains("Custom text:"));
    assert!(text.contains("↑/↓ select"));
    assert!(text.contains("Type custom answer"));
    assert!(text.contains("Enter submit"));
    assert!(text.contains("Esc dismiss"));
}

#[test]
fn test_app_poll_runtime_opens_question_dialog() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".to_string(),
        payload_json: serde_json::json!({
            "name": "question",
            "arguments": {
                "question": "Choose port number",
                "options": ["3000", "8080"]
            }
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    let dialog = app
        .question_dialog()
        .expect("question dialog should be open");
    assert_eq!(dialog.question, "Choose port number");
    assert_eq!(dialog.options, vec!["3000".to_string(), "8080".to_string()]);
}

#[test]
fn test_streaming_reasoning_buffer_and_flush_formatting() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    // 1. Send reasoning_delta
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "reasoning_delta".to_string(),
        payload_json: serde_json::json!({
            "delta": "Analyzing the bug in router...\nFound invalid route handler"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert!(app.is_reasoning());
    assert_eq!(
        app.reasoning_buffer(),
        "Analyzing the bug in router...\nFound invalid route handler"
    );

    // Renders "Thinking" in status/hints
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Thinking"));

    // 2. Incoming text_delta flushes reasoning to transcript
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 2,
        kind: "text_delta".to_string(),
        payload_json: serde_json::json!({
            "delta": "Here is the fix:"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert!(!app.is_reasoning());
    assert!(app.reasoning_buffer().is_empty());
    assert!(app.transcript().contains("Thought for "));
    assert!(!app.transcript().contains("💭"));
    // Verify transcript lines styling
    let theme = clawcode::tui::ThemeKind::ClawcodeDark.to_theme();
    let lines = clawcode::tui::format_transcript_lines(
        app.transcript(),
        &theme,
        ratatui::style::Color::Cyan,
    );
    let thought_header = lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.contains("Thought")));
    assert!(thought_header.is_some());
    let span = thought_header
        .unwrap()
        .spans
        .iter()
        .find(|s| s.content.contains("Thought"))
        .unwrap();
    assert_eq!(span.style.fg, Some(theme.amber));
    // 3. Test truncation for > 10 lines
    let mut long_reasoning_app = App::default();
    let (tx2, rx2) = std::sync::mpsc::channel();
    long_reasoning_app.set_runtime_receiver(rx2);
    let long_reasoning = (1..=15)
        .map(|i| format!("thought line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    tx2.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "reasoning_delta".to_string(),
        payload_json: serde_json::json!({
            "delta": long_reasoning
        })
        .to_string(),
    })
    .unwrap();
    long_reasoning_app.poll_runtime();
    long_reasoning_app.flush_reasoning();
    assert!(long_reasoning_app.transcript().contains("Thought for "));
    assert!(!long_reasoning_app.transcript().contains("💭"));
}

#[test]
fn test_tool_call_start_event_handling() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    // Submit user prompt so chat view is active
    app.apply(UiEvent::Input(Input::Character('c')));
    app.apply(UiEvent::Input(Input::Submit));

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_call_start".to_string(),
        payload_json: serde_json::json!({
            "id": "call_42",
            "name": "bash"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert!(app.active_tool().is_some());
    let active = app.active_tool().unwrap();
    assert_eq!(active.name, "bash");
    assert_eq!(active.desc, "preparing arguments...");

    // Renders "Preparing bash..."
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        ["⠋", "⠉", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇"]
            .iter()
            .any(|frame| text.contains(frame))
    );
    assert!(text.contains("Preparing bash..."));
}

#[test]
fn test_bash_terminal_card_rendering() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    // 1. Succeeded bash command
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "bash",
            "arguments": { "command": "cargo test" },
            "success": true,
            "output": "running 2 tests\ntest foo ... ok\ntest bar ... ok\n[Process exited with code 0]"
        }).to_string(),
    }).unwrap();

    app.poll_runtime();
    assert_eq!(app.tool_rows().len(), 1);
    assert_eq!(
        app.tool_rows()[0].state,
        clawcode::tui::ToolRowState::Completed
    );

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("$ cargo test"));
    assert!(text.contains("running 2 tests"));

    // 2. Failed bash command with exit 1
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "bash",
            "arguments": { "command": "failing_cmd" },
            "success": false,
            "output": "Error: command not found\n[Process exited with code 1]"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert_eq!(app.tool_rows().len(), 2);
    assert_eq!(
        app.tool_rows()[1].state,
        clawcode::tui::ToolRowState::Failed
    );

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("failing_cmd"));
    assert!(text.contains("Error: command not found"));

    // 3. Regression test: tools render as cards both during streaming and after turn completion!
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 3,
        kind: "generation_finished".to_string(),
        payload_json: serde_json::json!({ "status": "completed" }).to_string(),
    })
    .unwrap();
    app.poll_runtime();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("$ cargo test"));
    assert!(text.contains("failing_cmd"));
}

#[test]
fn test_websearch_visual_card_formatting() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    let raw_search_output = "Search results for \"rust tokio\" (2 results):\n\n1. Tokio Async Runtime\n   URL: https://tokio.rs\n   An event-driven platform...\n\n2. Tokio Tutorial\n   URL: https://tokio.rs/tutorial\n   Getting started...";

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "websearch",
            "arguments": { "query": "rust tokio" },
            "success": true,
            "output": raw_search_output
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert_eq!(app.tool_rows().len(), 1);
    assert_eq!(
        app.tool_rows()[0].state,
        clawcode::tui::ToolRowState::Completed
    );

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Search rust tokio"));
}

#[test]
fn test_edit_file_opencode_side_by_side_diff_in_transcript() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let payload_args = serde_json::json!({
        "path": "src/main.rs",
        "old_string": "fn main() {\n    // old\n}",
        "new_string": "fn main() {\n    println!(\"hello\");\n}",
    });

    let payload = serde_json::json!({
        "name": "edit_file",
        "arguments": payload_args,
        "success": true,
        "output": "Successfully edited 'src/main.rs' at line 10 (-3 lines, +3 lines)",
    });

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executed".to_string(),
        payload_json: payload.to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert_eq!(app.tool_rows().len(), 1);

    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("• Edit src/main.rs"));
    assert!(text.contains("-     // old"));
    assert!(text.contains("+     println!(\"hello\");"));
}

#[test]
fn test_write_file_clean_summary_no_diff() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    let payload_args = serde_json::json!({
        "path": "README.md",
        "content": "# Title\nFirst line\nSecond line\n",
    });

    let payload = serde_json::json!({
        "name": "write_file",
        "arguments": payload_args,
        "success": true,
        "output": "Successfully wrote 32 bytes to 'README.md'",
    });

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executed".to_string(),
        payload_json: payload.to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert_eq!(app.tool_rows().len(), 1);

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Write README.md"));
}

#[test]
fn test_generation_finished_clears_streaming_status() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.switch_session(42);

    // Provider starts delta stream
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 42,
        generation_id: Some(1),
        seq: 1,
        kind: "text_delta".to_string(),
        payload_json: serde_json::json!({ "delta": "Hello from model" }).to_string(),
    })
    .unwrap();
    app.poll_runtime();

    assert!(app.is_streaming_active());
    assert!(app.streaming_elapsed_seconds().is_some());

    // Finish generation
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 42,
        generation_id: Some(1),
        seq: 2,
        kind: "generation_finished".to_string(),
        payload_json: serde_json::json!({ "status": "completed" }).to_string(),
    })
    .unwrap();

    // Poll until drained
    while app.poll_runtime() {}

    assert_eq!(
        app.conversation_status(),
        clawcode::tui::ConversationStatus::Finished(clawcode::provider::FinishReason::Stop)
    );
    assert!(!app.is_streaming_active());
    assert!(app.streaming_elapsed_seconds().is_none());

    // Render and check status label does not show STREAMING
    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!text.contains("STREAMING"));
    assert!(!text.contains("streaming "));
}

#[test]
fn test_bash_tool_opencode_style_running_and_executed() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    // Submit a prompt so chat view is active
    app.apply(UiEvent::Input(Input::Character('s')));
    app.apply(UiEvent::Input(Input::Submit));

    // 1. Bash tool running with command "git status"
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".to_string(),
        payload_json: serde_json::json!({
            "name": "bash",
            "arguments": { "command": "git status" }
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert!(app.active_tool().is_some());
    let active = app.active_tool().unwrap();
    assert_eq!(active.name, "bash");
    assert_eq!(active.desc, "git status");

    // While running: terminal displays "git status" with braille spinner (not "$ git status")
    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("git status"));

    // 2. Tool finishes execution
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "bash",
            "arguments": { "command": "git status" },
            "success": true,
            "output": "On branch main\nnothing to commit, working tree clean\n[Process exited with code 0]"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert!(app.active_tool().is_none());

    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("git status"));
    assert!(text.contains("On branch main"));
}

#[test]
fn test_opencode_bash_command_and_output_block_styling() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("run command");
    let session_id = app.active_session_id().unwrap();
    let theme = clawcode::tui::ThemeKind::ClawcodeDark.to_theme();

    // 1. Running command: renders clean panel row with bg_element, braille spinner, command in ink
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".into(),
        payload_json: serde_json::json!({
            "id": "bash-call-1",
            "name": "bash",
            "arguments": { "command": "git commit -m 'feat: align'" }
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let rendered = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("git commit -m 'feat: align'"));
    let running_row_y = (0..buffer.area.height)
        .find(|&y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .contains("git commit")
        })
        .expect("running row should be visible");
    assert_eq!(buffer[(5, running_row_y)].bg, theme.bg_element);

    // 2. Completed command with 15 lines of output (exceeds max 10 lines)
    let output = (1..=15)
        .map(|n| format!("output line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id": "bash-call-1",
            "name": "bash",
            "arguments": { "command": "git commit -m 'feat: align'" },
            "success": true,
            "output": output
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();

    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let rendered = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Line 1 has $ prefix
    assert!(rendered.contains("$ git commit -m 'feat: align'"));
    // Output lines render without │ prefix
    assert!(rendered.contains("  output line 1"));
    assert!(rendered.contains("  output line 10"));
    assert!(!rendered.contains("│ output line"));
    // Truncated at 10 lines, line 11 not yet visible
    assert!(!rendered.contains("output line 11"));
    assert!(rendered.contains("↳ click to expand (5 more lines)"));

    // Verify background color on completed command and output row
    let cmd_y = (0..buffer.area.height)
        .find(|&y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .contains("$ git commit")
        })
        .expect("command row should be visible");
    let out_y = (0..buffer.area.height)
        .find(|&y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .contains("output line 1")
        })
        .expect("output row should be visible");
    assert_eq!(buffer[(5, cmd_y)].bg, theme.bg_element);
    assert_eq!(buffer[(5, out_y)].bg, theme.bg_element);

    // 3. Click to expand
    app.apply(UiEvent::MouseClick { x: 4, y: cmd_y });
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    assert!(app.is_tool_expanded("bash-call-1"));
    let buffer = terminal.backend().buffer();
    let expanded = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("output line 11"));
    assert!(expanded.contains("output line 15"));
    assert!(expanded.contains("↳ click to collapse"));

    // 4. Click to collapse
    app.apply(UiEvent::MouseClick { x: 4, y: cmd_y });
    assert!(!app.is_tool_expanded("bash-call-1"));
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let collapsed = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!collapsed.contains("output line 11"));
    assert!(collapsed.contains("↳ click to expand (5 more lines)"));
}

#[test]
fn test_edit_patch_tool_opencode_style_running_and_executed() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    // Submit user prompt
    app.apply(UiEvent::Input(Input::Character('e')));
    app.apply(UiEvent::Input(Input::Submit));

    // 1. Tool running: edit_file
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".to_string(),
        payload_json: serde_json::json!({
            "name": "edit_file",
            "arguments": { "path": "src/lib.rs" }
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert!(app.active_tool().is_some());
    let active = app.active_tool().unwrap();
    assert_eq!(active.name, "edit_file");
    assert_eq!(active.desc, "src/lib.rs");

    // Render while active: should show Edit src/lib.rs with braille spinner
    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Edit src/lib.rs"));

    // 2. Patch tool executed with patch content
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: 1,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "name": "apply_patch",
            "arguments": {
                "path": "src/lib.rs",
                "patch": "--- a/src/lib.rs\n+++ b/src/lib.rs\n-fn old() {}\n+fn new() {}\n+fn extra() {}\n"
            },
            "success": true,
            "output": "Patch applied cleanly"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();
    assert!(app.active_tool().is_none());

    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Patch src/lib.rs (+2 -1)"));
    assert!(text.contains("- fn old() {}"));
    assert!(text.contains("+ fn extra() {}"));
}

#[test]
fn test_slash_command_autocomplete_on_enter_executes_sessions() {
    let mut app = App::default();

    // 1. Type "/se" then press Enter -> executes /sessions (not unknown command)
    for ch in "/se".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    assert_eq!(app.prompt(), "/se");
    assert!(!app.matching_suggestions().is_empty());
    assert_eq!(app.matching_suggestions()[0].name, "/sessions");

    app.apply(UiEvent::Input(Input::Submit));

    assert!(app.sessions_dialog().is_some());
    assert!(!app.diagnostic().contains("unknown command"));
    assert!(app.diagnostic().contains("session(s)"));
    assert!(app.prompt().is_empty());

    // 2. Tab completion also works smoothly
    app.apply(UiEvent::Input(Input::Quit));
    assert!(app.sessions_dialog().is_none());

    for ch in "/pl".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    app.apply(UiEvent::Input(Input::ToggleMode));
    assert_eq!(app.prompt(), "/plan");
    app.apply(UiEvent::Input(Input::Submit));
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Plan);
}

#[test]
fn test_mouse_click_on_autocomplete_popup() {
    let mut app = App::default();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    // Type "/se" and render so popup is visible
    for ch in "/se".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let popup_area = app.last_popup_area().expect("popup area should be present");

    // Click on the first item in popup
    let click_x = popup_area.x + 2;
    let click_y = popup_area.y + 1;
    app.apply(UiEvent::MouseClick {
        x: click_x,
        y: click_y,
    });

    assert!(app.sessions_dialog().is_some());
    assert!(!app.diagnostic().contains("unknown command"));
    assert!(app.prompt().is_empty());

    app.apply(UiEvent::Input(Input::Quit));
    assert!(app.sessions_dialog().is_none());

    // Click on suggestion that needs arguments (e.g. /new)
    for ch in "/n".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let popup_area = app.last_popup_area().expect("popup area should be present");
    app.apply(UiEvent::MouseClick {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
    });
    assert_eq!(app.prompt(), "/new ");
    assert_eq!(app.cursor_position(), 5);

    // Also verify Input::Click variant
    let mut app2 = App::default();
    for ch in "/pl".chars() {
        app2.apply(UiEvent::Input(Input::Character(ch)));
    }
    terminal.draw(|f| clawcode::tui::render(f, &app2)).unwrap();
    let popup_area = app2
        .last_popup_area()
        .expect("popup area should be present");
    app2.apply(UiEvent::Input(Input::Click {
        column: popup_area.x + 2,
        row: popup_area.y + 1,
    }));
    assert_eq!(app2.mode(), clawcode::tui::ConversationMode::Plan);
}

#[test]
fn test_mouse_click_on_home_quick_action_cards() {
    let mut app = App::default();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let cards = app
        .last_quick_actions_area()
        .expect("quick actions cards should be rendered");

    // Card 0: /plan
    app.set_mode(clawcode::tui::ConversationMode::Build);
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Build);
    app.apply(UiEvent::MouseClick {
        x: cards[0].x + 2,
        y: cards[0].y + 1,
    });
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Plan);
    assert!(app.diagnostic().contains("Plan mode"));

    // Card 1: /build
    app.apply(UiEvent::MouseClick {
        x: cards[1].x + 2,
        y: cards[1].y + 1,
    });
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Build);
    assert!(app.diagnostic().contains("Build mode"));

    // Card 2: /models
    assert!(app.models_dialog().is_none());
    app.apply(UiEvent::MouseClick {
        x: cards[2].x + 2,
        y: cards[2].y + 1,
    });
    assert!(app.models_dialog().is_some());
    app.apply(UiEvent::Input(Input::Quit));
    assert!(app.models_dialog().is_none());

    // Card 3: /help or /keys
    assert!(!app.which_key().visible);
    app.apply(UiEvent::MouseClick {
        x: cards[3].x + 2,
        y: cards[3].y + 1,
    });
    assert!(app.which_key().visible);
    app.apply(UiEvent::Input(Input::Quit));
    assert!(!app.which_key().visible);
}

#[test]
fn test_mouse_click_works_without_prior_render() {
    // App created, never passed to render/draw: geometry computed automatically
    let mut app = App::default();

    // 1. Click on Card 0 (/plan) on home screen without rendering
    app.set_mode(clawcode::tui::ConversationMode::Build);
    let cards = app
        .get_or_compute_quick_actions_area()
        .expect("quick actions area computable");
    app.apply(UiEvent::MouseClick {
        x: cards[0].x + 2,
        y: cards[0].y + 1,
    });
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Plan);

    // 2. Click on Card 1 (/build) without rendering
    app.apply(UiEvent::MouseClick {
        x: cards[1].x + 2,
        y: cards[1].y + 1,
    });
    assert_eq!(app.mode(), clawcode::tui::ConversationMode::Build);

    // 3. Autocomplete popup click without prior render
    for ch in "/se".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    let popup_area = app
        .get_or_compute_popup_area()
        .expect("popup area computable");
    app.apply(UiEvent::MouseClick {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
    });
    assert!(app.sessions_dialog().is_some());
}

#[test]
fn test_partial_theme_and_model_autocomplete_on_enter() {
    let mut app = App::default();

    // 1. Partial theme: "/theme cat" -> matches Catppuccin themes
    for ch in "/theme cat".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    assert!(!app.matching_theme_suggestions().is_empty());
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.diagnostic().contains("theme switched to: Catppuccin"));

    // 2. Down arrow navigation in theme suggestions preserves selected index on Enter
    for ch in "/theme ".chars() {
        app.apply(UiEvent::Input(Input::Character(ch)));
    }
    let themes = app.matching_theme_suggestions();
    assert!(themes.len() > 2);
    app.apply(UiEvent::Input(Input::Down));
    assert_eq!(app.selected_suggestion_index(), 1);
    app.apply(UiEvent::Input(Input::Submit));
    assert_eq!(
        app.diagnostic(),
        format!("theme switched to: {}", themes[1])
    );
}

#[test]
fn structured_tool_rows_track_concurrent_lifecycle_and_bound_payloads() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("inspect tools");
    let session_id = app.active_session_id().expect("session created");

    for (seq, id, name) in [(1, "call-a", "read_file"), (2, "call-b", "bash")] {
        tx.send(clawcode::runtime::RuntimeEvent {
            session_id,
            generation_id: Some(7),
            seq,
            kind: "tool_call_start".to_string(),
            payload_json: serde_json::json!({ "id": id, "name": name }).to_string(),
        })
        .unwrap();
    }
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(7),
        seq: 3,
        kind: "tool_call_delta".to_string(),
        payload_json: serde_json::json!({
            "id": "call-a",
            "arguments": format!(r#"{{"path":"{}"}}"#, "x".repeat(8_000))
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();

    assert_eq!(app.tool_rows().len(), 2);
    assert!(
        app.tool_rows()
            .iter()
            .all(|row| row.state == clawcode::tui::app::ToolRowState::Pending)
    );
    assert!(app.tool_rows()[0].arguments.len() <= 4 * 1024);
    assert!(!app.is_typing());

    for (seq, (id, name, args)) in [
        (
            4,
            (
                "call-a",
                "read_file",
                serde_json::json!({ "path": "src/lib.rs" }),
            ),
        ),
        (
            5,
            (
                "call-b",
                "bash",
                serde_json::json!({ "command": "echo ok" }),
            ),
        ),
    ] {
        tx.send(clawcode::runtime::RuntimeEvent {
            session_id,
            generation_id: Some(7),
            seq,
            kind: "tool_executing".to_string(),
            payload_json: serde_json::json!({ "id": id, "name": name, "arguments": args })
                .to_string(),
        })
        .unwrap();
    }
    app.poll_runtime();
    assert!(
        app.tool_rows()
            .iter()
            .all(|row| row.state == clawcode::tui::app::ToolRowState::Running)
    );

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(7),
        seq: 6,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "id": "call-a",
            "name": "read_file",
            "arguments": { "path": "src/lib.rs" },
            "success": false,
            "output": "permission denied\n".repeat(2_000)
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();

    assert_eq!(
        app.tool_rows()
            .iter()
            .find(|row| row.call_id == "call-a")
            .map(|row| row.state),
        Some(clawcode::tui::app::ToolRowState::Failed)
    );
    assert!(
        app.tool_rows()
            .iter()
            .find(|row| row.call_id == "call-a")
            .is_some_and(|row| row.output.len() <= 16 * 1024)
    );
    assert!(app.active_tool().is_some());
}

#[test]
fn tool_only_active_turn_has_no_assistant_caret() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("run tool");
    let session_id = app.active_session_id().expect("session created");
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(9),
        seq: 1,
        kind: "tool_executing".to_string(),
        payload_json: serde_json::json!({
            "id": "call-only",
            "name": "bash",
            "arguments": { "command": "echo ok" }
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains('▋'));
    assert!(text.contains("echo ok"));
}

#[test]
fn active_text_wait_keeps_assistant_caret() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("answer");
    let session_id = app.active_session_id().unwrap();
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(11),
        seq: 1,
        kind: "text_delta".into(),
        payload_json: serde_json::json!({"delta": "hello"}).to_string(),
    })
    .unwrap();
    app.poll_runtime();
    assert!(app.is_typing());
}

#[test]
fn runtime_events_ignore_other_sessions_and_generations() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("isolate");
    let session_id = app.active_session_id().unwrap();
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id: session_id + 1000,
        generation_id: Some(99),
        seq: 1,
        kind: "generation_started".into(),
        payload_json: "{}".into(),
    })
    .unwrap();
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(11),
        seq: 2,
        kind: "generation_started".into(),
        payload_json: "{}".into(),
    })
    .unwrap();
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(12),
        seq: 3,
        kind: "tool_call_start".into(),
        payload_json: serde_json::json!({"id":"wrong","name":"bash"}).to_string(),
    })
    .unwrap();
    app.poll_runtime();
    assert_eq!(app.tool_rows().len(), 0);
    assert!(app.active_tool().is_none());
}

#[test]
fn completed_tool_row_renders_once_without_transcript_duplicate() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("run");
    let session_id = app.active_session_id().unwrap();
    for (seq, kind, payload) in [
        (
            1,
            "tool_call_start",
            serde_json::json!({"id":"call-1","name":"bash"}),
        ),
        (
            2,
            "tool_executing",
            serde_json::json!({"id":"call-1","name":"bash","arguments":{"command":"echo ok"}}),
        ),
        (
            3,
            "tool_executed",
            serde_json::json!({"id":"call-1","name":"bash","arguments":{"command":"echo ok"},"success":true,"output":"ok"}),
        ),
    ] {
        tx.send(clawcode::runtime::RuntimeEvent {
            session_id,
            generation_id: Some(3),
            seq,
            kind: kind.into(),
            payload_json: payload.to_string(),
        })
        .unwrap();
    }
    app.poll_runtime();
    assert_eq!(
        app.tool_rows()
            .iter()
            .filter(|row| row.state == clawcode::tui::app::ToolRowState::Completed)
            .count(),
        1
    );
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("$ echo ok"));
    let theme = clawcode::tui::ThemeKind::ClawcodeDark.to_theme();
    let cmd_y = (0..30)
        .find(|&y| {
            let row: String = (0..100).map(|x| buffer[(x, y)].symbol()).collect();
            row.contains("$ echo ok")
        })
        .expect("cmd line should exist");
    assert_eq!(buffer[(50, cmd_y)].bg, theme.bg_element);
    assert_eq!(buffer[(50, cmd_y - 1)].bg, theme.bg_element);
    assert_eq!(buffer[(50, cmd_y + 1)].bg, theme.bg_element);
    assert_eq!(buffer[(50, cmd_y + 2)].bg, theme.bg_element);
}

#[test]
fn active_tool_rows_stay_bounded_and_completion_uses_call_id() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("many tools");
    let session_id = app.active_session_id().unwrap();
    for seq in 0..70 {
        tx.send(clawcode::runtime::RuntimeEvent {
            session_id,
            generation_id: Some(5),
            seq,
            kind: "tool_call_start".into(),
            payload_json: serde_json::json!({"id":format!("call-{seq}"),"name":"read_file"})
                .to_string(),
        })
        .unwrap();
    }
    app.poll_runtime();
    assert_eq!(app.tool_rows().len(), 64);
    assert!(!app.tool_rows().iter().any(|row| row.call_id == "call-0"));
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(5),
        seq: 71,
        kind: "tool_executed".into(),
        payload_json:
            serde_json::json!({"id":"call-69","name":"read_file","success":true,"output":"done"})
                .to_string(),
    })
    .unwrap();
    app.poll_runtime();
    assert_eq!(
        app.tool_rows()
            .iter()
            .find(|row| row.call_id == "call-69")
            .map(|row| row.state),
        Some(clawcode::tui::app::ToolRowState::Completed)
    );
    assert!(
        app.tool_rows()
            .iter()
            .filter(|row| row.state == clawcode::tui::app::ToolRowState::Running
                || row.state == clawcode::tui::app::ToolRowState::Pending)
            .count()
            <= 63
    );
}

#[test]
fn ordered_stream_keeps_text_tools_and_followup_text_in_event_order() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("ordered");
    let session_id = app.active_session_id().unwrap();
    for (seq, kind, payload) in [
        (1, "text_delta", serde_json::json!({"delta":"Before"})),
        (
            2,
            "tool_executing",
            serde_json::json!({"id":"ordered-tool","name":"bash","arguments":{"command":"echo ok"}}),
        ),
        (
            3,
            "tool_executed",
            serde_json::json!({"id":"ordered-tool","name":"bash","arguments":{"command":"echo ok"},"success":true,"output":"ok"}),
        ),
        (4, "text_delta", serde_json::json!({"delta":"After"})),
    ] {
        tx.send(clawcode::runtime::RuntimeEvent {
            session_id,
            generation_id: Some(1),
            seq,
            kind: kind.into(),
            payload_json: payload.to_string(),
        })
        .unwrap();
    }
    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    let text = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let before = text.find("Before").unwrap();
    let tool = text.find("$ echo ok").unwrap();
    let after = text.find("After").unwrap();
    assert!(before < tool && tool < after);
}

#[test]
fn long_tool_output_expands_and_collapses_from_rendered_row() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("expand");
    let session_id = app.active_session_id().unwrap();
    let output = (0..12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(2),
        seq: 1,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({"id":"expand-tool","name":"bash","arguments":{"command":"printf lines"},"success":true,"output":output}).to_string(),
    }).unwrap();
    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = (0..buffer.area.height)
        .find(|&y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .contains("printf lines")
        })
        .expect("tool row should be visible");
    app.apply(UiEvent::MouseClick { x: 4, y: row });
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    assert!(app.is_tool_expanded("expand-tool"));
    let expanded = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expanded.contains("line 11"));
    app.apply(UiEvent::MouseClick { x: 4, y: row });
    assert!(!app.is_tool_expanded("expand-tool"));
}

#[test]
fn task_and_execute_rows_render_nested_work_when_metadata_exists() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("children");
    let session_id = app.active_session_id().unwrap();
    for (seq, name, arguments) in [
        (
            1,
            "task",
            serde_json::json!({"subagent_type":"scout","description":"inspect parser"}),
        ),
        (
            2,
            "execute",
            serde_json::json!({"toolCalls":[{"tool":"read","status":"running"},{"tool":"grep","status":"completed"}]}),
        ),
    ] {
        tx.send(clawcode::runtime::RuntimeEvent {
            session_id,
            generation_id: Some(3),
            seq,
            kind: "tool_executed".into(),
            payload_json: serde_json::json!({"id":format!("child-{seq}"),"name":name,"arguments":arguments,"success":true,"output":"ok"}).to_string(),
        }).unwrap();
    }
    app.poll_runtime();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    let text = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("↳ scout: inspect parser"));
    assert!(text.contains("⠋ read"));
    assert!(text.contains("✓ grep"));
}

#[test]
fn streaming_markdown_keeps_structure_without_raw_link_urls() {
    let theme = clawcode::tui::ThemeKind::ClawcodeDark.to_theme();
    let lines = clawcode::tui::format_transcript_lines(
        "# Heading\n\n- item `value`\n\n[docs](https://example.com)\n\n```rust\nlet x = 1;\n```",
        &theme,
        ratatui::style::Color::Cyan,
    );
    let text = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("# Heading"));
    assert!(text.contains("• item"));
    assert!(text.contains("docs"));
    assert!(text.contains("let x = 1;"));
    assert!(!text.contains("https://example.com"));
}

#[test]
fn tool_call_end_marks_arguments_complete_without_claiming_result() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("lifecycle");
    let session_id = app.active_session_id().unwrap();
    for (seq, kind, payload) in [
        (
            1,
            "tool_call_start",
            serde_json::json!({"id":"call-end","name":"bash"}),
        ),
        (
            2,
            "tool_call_delta",
            serde_json::json!({"id":"call-end","arguments":r#"{"command":"echo ok"}"#}),
        ),
        (
            3,
            "tool_call_end",
            serde_json::json!({"id":"call-end","arguments_complete":true}),
        ),
    ] {
        tx.send(clawcode::runtime::RuntimeEvent {
            session_id,
            generation_id: Some(21),
            seq,
            kind: kind.into(),
            payload_json: payload.to_string(),
        })
        .unwrap();
    }
    app.poll_runtime();
    let row = app
        .tool_rows()
        .iter()
        .find(|row| row.call_id == "call-end")
        .unwrap();
    assert!(row.arguments_complete);
    assert_eq!(row.state, clawcode::tui::app::ToolRowState::Pending);
    assert!(row.output.is_empty());

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(21),
        seq: 4,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id":"call-end",
            "name":"bash",
            "success":true
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();
    assert_eq!(
        app.tool_rows()
            .iter()
            .find(|row| row.call_id == "call-end")
            .map(|row| row.state),
        Some(clawcode::tui::app::ToolRowState::Failed)
    );
}

#[test]
fn child_session_row_needs_explicit_metadata() {
    let mut with_metadata = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    with_metadata.set_runtime_receiver(rx);
    with_metadata.submit_user_prompt("child metadata");
    let session_id = with_metadata.active_session_id().unwrap();
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(22),
        seq: 1,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id":"task-with-child",
            "name":"task",
            "arguments":{"subagent_type":"scout","description":"inspect"},
            "success":true,
            "output":"done",
            "metadata":{"sessionId":"child-22"}
        })
        .to_string(),
    })
    .unwrap();
    with_metadata.poll_runtime();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &with_metadata))
        .unwrap();
    let rendered = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("child session child-22"));

    let mut without_metadata = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    without_metadata.set_runtime_receiver(rx);
    without_metadata.submit_user_prompt("child absent");
    let session_id = without_metadata.active_session_id().unwrap();
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(23),
        seq: 1,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id":"task-without-child",
            "name":"task",
            "arguments":{"subagent_type":"scout","description":"inspect"},
            "success":true,
            "output":"done"
        })
        .to_string(),
    })
    .unwrap();
    without_metadata.poll_runtime();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &without_metadata))
        .unwrap();
    let rendered = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!rendered.contains("child session"));
}

#[test]
fn opencode_visual_streaming_parity_suite() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("check streaming visual parity");
    let session_id = app.active_session_id().unwrap();

    // 1. Reasoning delta 1
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(10),
        seq: 1,
        kind: "reasoning_delta".into(),
        payload_json:
            serde_json::json!({ "delta": "Initial investigation focuses on the README..." })
                .to_string(),
    })
    .unwrap();

    // 2. Tool call start for read_file
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(10),
        seq: 2,
        kind: "tool_call_start".into(),
        payload_json: serde_json::json!({ "id": "call-read-1", "name": "read_file" }).to_string(),
    })
    .unwrap();

    // 3. Tool executed for read_file
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(10),
        seq: 3,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id": "call-read-1",
            "name": "read_file",
            "arguments": { "path": "README.md" },
            "success": true,
            "output": "line 1\nline 2\nline 3\n"
        })
        .to_string(),
    })
    .unwrap();

    // 4. Reasoning delta 2 (should combine into the SAME thought block, not duplicate!)
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(10),
        seq: 4,
        kind: "reasoning_delta".into(),
        payload_json: serde_json::json!({ "delta": "\nNow checking status..." }).to_string(),
    })
    .unwrap();

    // 5. Tool executed for bash (shell command)
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(10),
        seq: 5,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id": "call-bash-1",
            "name": "bash",
            "arguments": { "command": "git status" },
            "success": true,
            "output": "On branch main\nnothing to commit, working tree clean"
        })
        .to_string(),
    })
    .unwrap();

    // 6. Text delta
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(10),
        seq: 6,
        kind: "text_delta".into(),
        payload_json: serde_json::json!({ "delta": "Here is the summary." }).to_string(),
    })
    .unwrap();

    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(100, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();

    let rendered = (0..40)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("▌ check streaming visual parity"));
    let theme = clawcode::tui::ThemeKind::ClawcodeDark.to_theme();
    let prompt_y = (0..40)
        .find(|&y| {
            let row: String = (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect();
            row.contains("▌ check streaming visual parity")
        })
        .expect("prompt line should exist");
    assert_eq!(
        terminal.backend().buffer()[(50, prompt_y)].bg,
        theme.bg_element
    );
    // Thought rendered cleanly without emoji or bulky boxes
    assert!(rendered.contains("Thought for "));
    assert!(!rendered.contains("💭"));

    // Tool rows use clean dot icon
    assert!(rendered.contains("● Read README.md"));
    // No noisy line count suffix
    assert!(!rendered.contains("● Read README.md · 3 lines"));

    // Shell row uses $ prefix
    assert!(rendered.contains("$ git status"));
    // Shell output preview rendered cleanly without left border │
    assert!(rendered.contains("  On branch main"));
    assert!(rendered.contains("  nothing to commit, working tree clean"));
    assert!(!rendered.contains("│ On branch main"));

    // Assistant text rendered with 3-space padding
    assert!(rendered.contains("   Here is the summary."));
}

#[test]
fn test_edit_file_shows_real_unified_diff_with_colors_and_card() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("edit file test");
    let session_id = app.active_session_id().unwrap();

    let old_content = "fn main() {\n    let a = 1;\n    let b = 2;\n}";
    let new_content = "fn main() {\n    let a = 10;\n    let b = 2;\n    let c = 3;\n}";

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".into(),
        payload_json: serde_json::json!({
            "id": "edit-1",
            "name": "edit_file",
            "arguments": {
                "path": "src/main.rs",
                "old_string": old_content,
                "new_string": new_content,
            }
        })
        .to_string(),
    })
    .unwrap();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id": "edit-1",
            "name": "edit_file",
            "arguments": {
                "path": "src/main.rs",
                "old_string": old_content,
                "new_string": new_content,
            },
            "success": true,
            "output": "Successfully edited src/main.rs"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();

    let rendered = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Title line contains edit icon, path, and (+A -R) badge
    assert!(rendered.contains("• Edit src/main.rs (+2 -1)"));
    // Unified diff rendered with - and + markers
    assert!(rendered.contains("-     let a = 1;"));
    assert!(rendered.contains("+     let a = 10;"));
    assert!(rendered.contains("+     let c = 3;"));

    // Verify background spans full card width (columns 0..98) even past text
    let theme = clawcode::tui::ThemeKind::ClawcodeDark.to_theme();
    let buffer = terminal.backend().buffer();
    let title_y = (0..30)
        .find(|&y| {
            let row: String = (0..100).map(|x| buffer[(x, y)].symbol()).collect();
            row.contains("• Edit src/main.rs")
        })
        .expect("title line should exist");
    assert_eq!(buffer[(50, title_y)].bg, theme.bg_element);
    assert_eq!(buffer[(0, title_y - 1)].bg, theme.bg_element);
    assert_eq!(buffer[(50, title_y - 1)].bg, theme.bg_element);
    assert_eq!(buffer[(97, title_y - 1)].bg, theme.bg_element);
}

#[test]
fn test_edit_diff_collapse_and_expand_when_lines_over_10() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("edit large diff");
    let session_id = app.active_session_id().unwrap();

    let old_lines: Vec<String> = (0..20).map(|i| format!("old line {i}")).collect();
    let new_lines: Vec<String> = (0..20).map(|i| format!("new line {i}")).collect();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".into(),
        payload_json: serde_json::json!({
            "id": "edit-large",
            "name": "edit",
            "arguments": {
                "path": "test.txt",
                "old_string": old_lines.join("\n"),
                "new_string": new_lines.join("\n"),
            }
        })
        .to_string(),
    })
    .unwrap();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id": "edit-large",
            "name": "edit",
            "arguments": {
                "path": "test.txt",
                "old_string": old_lines.join("\n"),
                "new_string": new_lines.join("\n"),
            },
            "success": true,
            "output": "ok"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();

    let rendered = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Collapsed initially, shows expand prompt
    assert!(rendered.contains("↳ click to expand"));

    // Toggle expansion
    app.toggle_tool_expanded("edit-large");
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();

    let rendered_expanded = (0..30)
        .map(|y| {
            (0..100)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered_expanded.contains("↳ click to collapse"));
}

#[test]
fn test_tool_executed_missing_id_resolves_without_duplicate_row() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("grep test");
    let session_id = app.active_session_id().unwrap();

    // tool_executing with no id
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".into(),
        payload_json: serde_json::json!({
            "name": "grep",
            "arguments": { "query": "adalah" }
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    // tool_executed with no id
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "name": "grep",
            "arguments": { "query": "adalah" },
            "success": false,
            "output": "Error executing grep: 'README.md' is not a directory"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();

    let rendered = (0..30)
        .map(|y| {
            (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Exactly one row rendered
    let occurrences = rendered.matches("Grep adalah").count();
    assert_eq!(occurrences, 1, "rendered was:\n{rendered}");
    assert!(rendered.contains("failed: 'README.md' is not a directory"));
    // Must NOT have click to expand
    assert!(!rendered.contains("↳ click to expand"));
}

#[test]
fn test_glob_grep_read_have_no_click_to_expand() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("search files");
    let session_id = app.active_session_id().unwrap();

    // Glob with multiline output
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executing".into(),
        payload_json: serde_json::json!({
            "id": "glob-1",
            "name": "glob",
            "arguments": { "pattern": "**/*.md" }
        })
        .to_string(),
    })
    .unwrap();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 2,
        kind: "tool_executed".into(),
        payload_json: serde_json::json!({
            "id": "glob-1",
            "name": "glob",
            "arguments": { "pattern": "**/*.md" },
            "success": true,
            "output": "README.md\nCONTRIBUTING.md\nDOCS.md\nCHANGELOG.md\nNOTES.md"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();

    let rendered = (0..30)
        .map(|y| {
            (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Must be rendered as clean inline tool with match count
    assert!(rendered.contains("● Glob **/*.md (5 matches)"));
    // Must NEVER show generic click to expand for inline tools
    assert!(!rendered.contains("↳ click to expand"));
}

#[test]
fn test_thought_expansion_mouse_click_toggle() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    app.submit_user_prompt("write a poem");
    let session_id = app.active_session_id().unwrap();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 1,
        kind: "reasoning_delta".into(),
        payload_json: serde_json::json!({
            "delta": "Thinking about the rhyming scheme\nChoosing iambic pentameter"
        })
        .to_string(),
    })
    .unwrap();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 2,
        kind: "text_delta".into(),
        payload_json: serde_json::json!({
            "delta": "The woods are lovely, dark and deep"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    // 1. Initial render: thought is completed, collapsed by default
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    let rendered = (0..30)
        .map(|y| {
            (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("+ Thought for"));
    assert!(!rendered.contains("Choosing iambic pentameter"));
    assert!(!app.is_thought_expanded());

    // 2. Find row where "+ Thought for" appears and click it
    let thought_y = (0..30)
        .find(|&y| {
            let row = (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>();
            row.contains("+ Thought for")
        })
        .expect("thought row found");

    app.handle_mouse_click(5, thought_y);
    assert!(app.is_thought_expanded());

    // 3. Render after click: thought should be expanded
    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    let rendered_expanded = (0..30)
        .map(|y| {
            (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered_expanded.contains("- Thought for"));
    assert!(rendered_expanded.contains("Choosing iambic pentameter"));
    assert!(rendered_expanded.contains("│"));

    // 4. Click again: thought should collapse
    let expanded_y = (0..30)
        .find(|&y| {
            let row = (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>();
            row.contains("- Thought for")
        })
        .expect("expanded thought row found");

    app.handle_mouse_click(5, expanded_y);
    assert!(!app.is_thought_expanded());

    terminal
        .draw(|frame| clawcode::tui::render(frame, &app))
        .unwrap();
    let rendered_collapsed_again = (0..30)
        .map(|y| {
            (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered_collapsed_again.contains("+ Thought for"));
    assert!(!rendered_collapsed_again.contains("Choosing iambic pentameter"));
}

#[test]
fn test_session_switch_and_restore_no_duplicate_messages() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("clawcode-test-tui-switch-{nonce}.db"));
    let db = clawcode::persistence::Db::open(&path).expect("runtime db");
    let s1 = db.create_session("session 1").unwrap();
    let s2 = db.create_session("session 2").unwrap();

    let writer = clawcode::persistence::WriterHandle::spawn(
        clawcode::persistence::Db::open(&path).expect("writer db"),
    );

    #[derive(Debug)]
    struct DummyProvider;
    impl clawcode::provider::Provider for DummyProvider {
        fn id(&self) -> &clawcode::provider::ProviderId {
            static ID: std::sync::LazyLock<clawcode::provider::ProviderId> =
                std::sync::LazyLock::new(|| clawcode::provider::ProviderId::new("dummy"));
            &ID
        }
        fn capabilities(&self) -> clawcode::provider::ProviderCapabilities {
            clawcode::provider::ProviderCapabilities {
                streaming: false,
                tools: false,
            }
        }
        fn models(&self) -> Vec<clawcode::provider::ModelInfo> {
            Vec::new()
        }
        fn send(
            &self,
            _: &clawcode::provider::StreamRequest,
        ) -> Result<clawcode::provider::StreamResponse, clawcode::provider::ProviderError> {
            Ok(clawcode::provider::StreamResponse { events: Vec::new() })
        }
    }

    let mut app = App::default();
    app.attach_runtime(db, writer, Box::new(DummyProvider));

    app.switch_session(s1.id);
    assert_eq!(app.active_session_id(), Some(s1.id));

    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);
    tx.send(clawcode::runtime::RuntimeEvent {
        seq: 1,
        session_id: s1.id,
        generation_id: Some(10),
        kind: "assistant_message".into(),
        payload_json: serde_json::json!({
            "content": "Hello from assistant turn 1"
        })
        .to_string(),
    })
    .unwrap();
    app.poll_runtime();

    assert!(app.transcript().contains("Hello from assistant turn 1"));
    assert_eq!(app.loaded_until_seq(), 1);

    app.switch_session(s2.id);
    assert_eq!(app.active_session_id(), Some(s2.id));
    assert!(!app.transcript().contains("Hello from assistant turn 1"));

    app.switch_session(s1.id);
    assert_eq!(app.active_session_id(), Some(s1.id));

    let count = app
        .transcript()
        .matches("Hello from assistant turn 1")
        .count();
    assert_eq!(
        count,
        1,
        "message must not duplicate on switch and restore, transcript: {}",
        app.transcript()
    );
    assert_eq!(app.loaded_until_seq(), 1);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_disconnected_durable_events_replay_into_transcript() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("clawcode-test-tui-replay-{nonce}.db"));
    let db = clawcode::persistence::Db::open(&path).expect("runtime db");
    let session = db.create_session("replay session").unwrap();
    let session_id = session.id;

    let _s0 = db
        .append_event(
            session_id,
            None,
            "tool_call_created",
            &serde_json::json!({
                "call_id": "call_abc",
                "tool_name": "read_file",
                "arguments": { "path": "src/main.rs" }
            })
            .to_string(),
        )
        .unwrap();

    let _s1 = db
        .append_event(
            session_id,
            None,
            "tool_call_settled",
            &serde_json::json!({
                "call_id": "call_abc",
                "tool_name": "read_file",
                "status": "completed",
                "result": "fn main() {}"
            })
            .to_string(),
        )
        .unwrap();

    let _s2 = db
        .append_event(
            session_id,
            None,
            "assistant_message",
            &serde_json::json!({
                "content": "I have read main.rs"
            })
            .to_string(),
        )
        .unwrap();

    let s3 = db
        .append_event(
            session_id,
            None,
            "generation_finished",
            &serde_json::json!({
                "status": "finished",
                "finish_reason": "stop"
            })
            .to_string(),
        )
        .unwrap();

    let writer = clawcode::persistence::WriterHandle::spawn(
        clawcode::persistence::Db::open(&path).expect("writer db"),
    );

    #[derive(Debug)]
    struct DummyProvider;
    impl clawcode::provider::Provider for DummyProvider {
        fn id(&self) -> &clawcode::provider::ProviderId {
            static ID: std::sync::LazyLock<clawcode::provider::ProviderId> =
                std::sync::LazyLock::new(|| clawcode::provider::ProviderId::new("dummy"));
            &ID
        }
        fn capabilities(&self) -> clawcode::provider::ProviderCapabilities {
            clawcode::provider::ProviderCapabilities {
                streaming: false,
                tools: false,
            }
        }
        fn models(&self) -> Vec<clawcode::provider::ModelInfo> {
            Vec::new()
        }
        fn send(
            &self,
            _: &clawcode::provider::StreamRequest,
        ) -> Result<clawcode::provider::StreamResponse, clawcode::provider::ProviderError> {
            Ok(clawcode::provider::StreamResponse { events: Vec::new() })
        }
    }

    let mut app = App::default();
    app.attach_runtime(db, writer, Box::new(DummyProvider));

    app.switch_session(session_id);

    assert!(
        app.transcript().contains("I have read main.rs"),
        "replayed assistant message should appear in transcript, got: {}",
        app.transcript()
    );

    assert!(
        app.tool_rows()
            .iter()
            .any(|r| r.call_id == "call_abc" && r.state == clawcode::tui::ToolRowState::Completed),
        "replayed tool call should be completed in tool rows"
    );

    assert_eq!(app.loaded_until_seq(), s3);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn previous_response_and_plan_persist_when_user_chats_again() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    app.submit_user_prompt("First question");
    let session_id = app.active_session_id().unwrap();

    let payload_args = serde_json::json!({
        "explanation": "Create plan",
        "plan": [
            {"step": "Step 1", "status": "completed"},
            {"step": "Step 2", "status": "pending"}
        ]
    });
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 1,
        kind: "tool_executed".to_string(),
        payload_json: serde_json::json!({
            "id": "call-plan",
            "name": "update_plan",
            "arguments": payload_args,
            "success": true,
            "output": "Plan updated: 2 steps"
        })
        .to_string(),
    })
    .unwrap();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 2,
        kind: "assistant_message".to_string(),
        payload_json: serde_json::json!({
            "content": "Here is the response to turn 1"
        })
        .to_string(),
    })
    .unwrap();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 3,
        kind: "generation_finished".to_string(),
        payload_json: serde_json::json!({
            "status": "completed",
            "finish_reason": "Stop"
        })
        .to_string(),
    })
    .unwrap();

    app.poll_runtime();

    assert!(app.transcript().contains("First question"));
    assert!(app.tool_rows().iter().any(|r| r.call_id == "call-plan"));
    assert!(
        app.stream_parts()
            .iter()
            .any(|p| matches!(p, clawcode::tui::StreamPart::Tool(id) if id == "call-plan"))
    );
    assert_eq!(app.current_plan().len(), 2);

    app.submit_user_prompt("Second question");

    assert!(app.transcript().contains("First question"));
    assert!(app.transcript().contains("Second question"));
    assert!(app.tool_rows().iter().any(|r| r.call_id == "call-plan"));
    assert!(app.stream_parts().iter().any(
        |p| matches!(p, clawcode::tui::StreamPart::User(prompt) if prompt == "Second question")
    ));
    assert_eq!(app.current_plan().len(), 2);
}

#[test]
fn test_resize_event_handled_with_open_dialogs() {
    let mut app = App::default();
    assert_eq!(app.terminal_size(), (100, 30));

    // 1. Resize with permission_dialog open
    app.open_permission_dialog("bash", "echo hi", "test");
    assert!(app.permission_dialog().is_some());
    app.apply(UiEvent::Resize {
        width: 120,
        height: 40,
    });
    assert_eq!(app.terminal_size(), (120, 40));
    assert!(app.permission_dialog().is_some());
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.permission_dialog().is_none());

    // 2. Resize with question_dialog open
    app.open_question_dialog("Proceed?", vec!["Yes".to_string(), "No".to_string()]);
    assert!(app.question_dialog().is_some());
    app.apply(UiEvent::Resize {
        width: 140,
        height: 50,
    });
    assert_eq!(app.terminal_size(), (140, 50));
    assert!(app.question_dialog().is_some());
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.question_dialog().is_none());

    // 3. Resize with status_dialog open
    app.open_status_dialog();
    assert!(app.status_dialog().is_some());
    app.apply(UiEvent::Resize {
        width: 80,
        height: 24,
    });
    assert_eq!(app.terminal_size(), (80, 24));
    assert!(app.status_dialog().is_some());
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(app.status_dialog().is_none());

    // 4. Resize with which_key open
    app.apply(UiEvent::Input(Input::WhichKey));
    assert!(app.which_key().visible);
    app.apply(UiEvent::Resize {
        width: 90,
        height: 35,
    });
    assert_eq!(app.terminal_size(), (90, 35));
    assert!(app.which_key().visible);
    app.apply(UiEvent::Input(Input::Cancel));
    assert!(!app.which_key().visible);
}

#[test]
fn test_dialog_precedence_aligned_with_render() {
    let mut app = App::default();

    // Open status dialog
    app.open_status_dialog();
    assert!(app.status_dialog().is_some());

    // Submit should close status dialog
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.status_dialog().is_none());
}

#[test]
fn test_chat_rendering_handles_large_content_without_overflow() {
    let mut app = App::default();
    let long_text = "line\n".repeat(1000);
    app.apply(UiEvent::StreamDelta(long_text));

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    let res = terminal.draw(|f| clawcode::tui::render(f, &app));
    assert!(res.is_ok());
}

#[test]
fn test_render_status_bar_uses_cached_git_branch() {
    let mut app = App::default();
    app.set_git_branch(Some("feature-xyz".into()));
    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut rendered = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            rendered.push_str(buffer[(x, y)].symbol());
        }
    }
    assert!(rendered.contains(":feature-xyz"));

    app.set_git_branch(None);
    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut rendered_none = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            rendered_none.push_str(buffer[(x, y)].symbol());
        }
    }
    assert!(!rendered_none.contains(":feature-xyz"));
}

#[test]
fn history_preservation_when_stream_base_len_is_none_or_reset() {
    let mut app = App::default();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_runtime_receiver(rx);

    // Initial user prompt and finished response
    app.submit_user_prompt("First user question");
    assert_eq!(app.stream_base_len(), Some(app.transcript().len()));
    let session_id = app.active_session_id().unwrap();

    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 1,
        kind: "generation_started".into(),
        payload_json: "{}".into(),
    })
    .unwrap();
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 2,
        kind: "text_delta".into(),
        payload_json: serde_json::json!({"delta": "First assistant answer"}).to_string(),
    })
    .unwrap();
    tx.send(clawcode::runtime::RuntimeEvent {
        session_id,
        generation_id: Some(1),
        seq: 3,
        kind: "generation_finished".into(),
        payload_json: "{}".into(),
    })
    .unwrap();
    app.poll_runtime();

    // Verify transcript has both
    assert!(app.transcript().contains("First user question"));
    assert!(app.transcript().contains("First assistant answer"));

    // User submits second prompt
    app.submit_user_prompt("Second user question");
    assert_eq!(app.stream_base_len(), Some(app.transcript().len()));

    // Cancel / reset happens which clears stream_base_len to None
    app.apply(clawcode::tui::UiEvent::Input(clawcode::tui::Input::Cancel));
    assert_eq!(app.stream_base_len(), None);

    // Stream parts exist while stream_base_len is None
    app.set_stream_parts_for_test(
        vec![
            clawcode::tui::StreamPart::User("First user question".into()),
            clawcode::tui::StreamPart::Text("First assistant answer".into()),
            clawcode::tui::StreamPart::User("Second user question".into()),
            clawcode::tui::StreamPart::Text("Streaming in progress...".into()),
        ],
        None,
    );
    assert!(!app.stream_parts().is_empty());
    assert_eq!(app.stream_base_len(), None);

    // Render in terminal - prior history must NOT vanish!
    let backend = ratatui::backend::TestBackend::new(120, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("First user question"));
    assert!(text.contains("First assistant answer"));
    assert!(text.contains("Second user question"));
    assert!(text.contains("Streaming in progress..."));
}

#[test]
fn tool_card_rendering_and_raw_tool_bracket_badge_formatting() {
    let mut app = App::default();
    let theme = clawcode::tui::ThemeKind::ClawcodeDark.to_theme();

    app.set_tool_rows_for_test(vec![
        clawcode::tui::ToolRow {
            call_id: "read-1".to_string(),
            name: "read_file".to_string(),
            desc: "Cargo.toml".to_string(),
            arguments: serde_json::json!({ "path": "Cargo.toml" }).to_string(),
            output: "27 lines".to_string(),
            state: clawcode::tui::ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        },
        clawcode::tui::ToolRow {
            call_id: "plan-1".to_string(),
            name: "update_plan".to_string(),
            desc: "Plan updated: 3 steps".to_string(),
            arguments: serde_json::json!({ "plan": [] }).to_string(),
            output: "Plan updated: 3 steps".to_string(),
            state: clawcode::tui::ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        },
        clawcode::tui::ToolRow {
            call_id: "list-1".to_string(),
            name: "list_dir".to_string(),
            desc: "src".to_string(),
            arguments: serde_json::json!({ "path": "src" }).to_string(),
            output: "src".to_string(),
            state: clawcode::tui::ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        },
        clawcode::tui::ToolRow {
            call_id: "tool-1".to_string(),
            name: "calculate".to_string(),
            desc: "finished calculation".to_string(),
            arguments: String::new(),
            output: "finished calculation".to_string(),
            state: clawcode::tui::ToolRowState::Completed,
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        },
    ]);
    app.set_stream_parts_for_test(
        vec![
            clawcode::tui::StreamPart::Tool("read-1".into()),
            clawcode::tui::StreamPart::Tool("plan-1".into()),
            clawcode::tui::StreamPart::Tool("list-1".into()),
            clawcode::tui::StreamPart::Tool("tool-1".into()),
        ],
        None,
    );

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!text.contains("[tool]:"));
    assert!(text.contains("Read Cargo.toml"));
    assert!(text.contains("Updated Plan"));
    assert!(text.contains("List src"));
    assert!(text.contains("Run finished calculation"));

    let tool_y = (0..buffer.area.height)
        .find(|&y| {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            row.contains("Read Cargo.toml")
        })
        .expect("tool header line exists");
    assert_ne!(buffer[(50, tool_y)].bg, theme.bg_element);
}

#[test]
fn restore_session_from_database_formats_tool_messages_as_clean_cards() {
    let db = clawcode::persistence::Db::open_in_memory().unwrap();
    let s1 = db.create_session("Tool Session").unwrap();
    db.append_message(s1.id, "user", "Run my tools").unwrap();
    db.append_message(s1.id, "assistant", "Running now...")
        .unwrap();
    db.append_message(s1.id, "tool", "Plan updated: 2 steps")
        .unwrap();
    db.append_message(s1.id, "tool", "read Cargo.toml output")
        .unwrap();
    db.append_message(s1.id, "tool", "   ").unwrap();

    let mut app = App::default();
    let service = clawcode::cli::CommandService::with_db(clawcode::cli::CliDiscovery::new(), db);
    app.set_command_service(service);

    app.switch_session(s1.id);
    assert_eq!(app.active_session_id(), Some(s1.id));

    // Must never contain raw [tool]: or ⬢ in transcript
    assert!(!app.transcript().contains("[tool]:"));
    assert!(!app.transcript().contains("⬢ "));
    assert_eq!(app.tool_rows().len(), 2);
    assert!(
        app.tool_rows()
            .iter()
            .all(|r| r.state == clawcode::tui::ToolRowState::Completed)
    );

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|f| clawcode::tui::render(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(text.contains("Run my tools"));
    assert!(text.contains("Running now..."));
    assert!(text.contains("Updated Plan"));
    assert!(text.contains("read Cargo.toml output"));
}

#[test]
fn cancel_discards_buffered_stream_deltas_without_transcript_leak() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(10);

    // Initial prompt submitted so transcript has baseline content
    app.submit_user_prompt("my initial prompt");
    let baseline_transcript = app.transcript().to_string();

    // Set a running tool row
    app.set_tool_rows_for_test(vec![clawcode::tui::ToolRow {
        call_id: "tool-1".to_string(),
        name: "bash".to_string(),
        desc: "executing command".to_string(),
        arguments: "{}".to_string(),
        output: String::new(),
        state: clawcode::tui::ToolRowState::Running,
        arguments_complete: true,
        metadata: None,
        started_at: std::time::Instant::now(),
        expandable: false,
    }]);
    assert!(app.is_streaming_active());

    // Push stream deltas and Cancel into events queue
    events.push(UiEvent::StreamDelta("leaked stream chunk 1".to_string()));
    events.push(UiEvent::StreamDelta("leaked stream chunk 2".to_string()));
    events.push(UiEvent::Input(Input::Cancel));

    // apply_pending pops Cancel, sets status Cancelled, calls events.clear()
    assert!(app.apply_pending(&mut events));

    assert_eq!(
        app.conversation_status(),
        clawcode::tui::ConversationStatus::Cancelled
    );
    assert!(!app.transcript().contains("leaked stream chunk"));
    assert_eq!(app.transcript(), baseline_transcript);
    assert!(events.is_empty(), "buffered deltas must be cleared");
    assert_eq!(
        app.tool_rows()[0].state,
        clawcode::tui::ToolRowState::Failed
    );
    assert!(!app.is_streaming_active(), "spinner should stop");

    // Subsequent drain is empty; no chunk leaked
    assert!(!app.apply_pending(&mut events));
    assert!(!app.transcript().contains("leaked stream chunk"));
}

#[test]
fn rapid_cancel_stress_test_maintains_consistent_state() {
    let mut app = App::default();
    let mut events = UiEventQueue::new(16);

    for i in 0..50 {
        // Submit prompt
        app.submit_user_prompt(&format!("rapid prompt {i}"));
        assert_eq!(
            app.conversation_status(),
            clawcode::tui::ConversationStatus::Active
        );

        // Add tool row in Running or Pending state
        app.set_tool_rows_for_test(vec![clawcode::tui::ToolRow {
            call_id: format!("tool-{i}"),
            name: "tool".to_string(),
            desc: format!("tool run {i}"),
            arguments: "{}".to_string(),
            output: String::new(),
            state: if i % 2 == 0 {
                clawcode::tui::ToolRowState::Running
            } else {
                clawcode::tui::ToolRowState::Pending
            },
            arguments_complete: true,
            metadata: None,
            started_at: std::time::Instant::now(),
            expandable: false,
        }]);
        assert!(app.is_streaming_active());

        // Push stream delta and Cancel
        let stale_delta = format!("stale chunk {i}");
        events.push(UiEvent::StreamDelta(stale_delta.clone()));
        events.push(UiEvent::Input(Input::Cancel));

        // Apply pending
        assert!(app.apply_pending(&mut events));

        // Verify state consistency
        assert_eq!(
            app.conversation_status(),
            clawcode::tui::ConversationStatus::Cancelled,
            "status must be Cancelled on iteration {i}"
        );
        assert!(
            !app.is_streaming_active(),
            "no stuck active spinners on iteration {i}"
        );
        assert!(
            !app.is_typing(),
            "typewriter must not be typing on iteration {i}"
        );
        assert!(
            !app.transcript().contains(&stale_delta),
            "no leaked delta on iteration {i}"
        );
        assert!(events.is_empty(), "queue must be cleared on iteration {i}");
        assert!(
            app.tool_rows().iter().all(|r| !matches!(
                r.state,
                clawcode::tui::ToolRowState::Pending | clawcode::tui::ToolRowState::Running
            )),
            "no tool row pending or running on iteration {i}"
        );
    }
}

#[test]
fn multi_turn_conversation_persistence_10_turns_reloads_cleanly() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("clawcode-test-multiturn-{nonce}.db"));
    let db = clawcode::persistence::Db::open(&path).expect("runtime db");
    let session = db.create_session("multi-turn").unwrap();
    let session_id = session.id;

    let other_session = db.create_session("other-session").unwrap();
    db.append_message(other_session.id, "user", "Other user question")
        .unwrap();
    db.append_message(other_session.id, "assistant", "Other assistant answer")
        .unwrap();

    // Store 12 turns (user + assistant = 24 messages, plus corresponding generation events)
    for i in 0..12 {
        let user_prompt = format!("User turn {i} question about Rust");
        let assistant_reply = format!("Assistant turn {i} explanation with details");
        db.append_message(session_id, "user", &user_prompt).unwrap();
        db.append_message(session_id, "assistant", &assistant_reply)
            .unwrap();

        db.append_event(
            session_id,
            None,
            "user_message",
            &serde_json::json!({ "content": user_prompt }).to_string(),
        )
        .unwrap();
        db.append_event(
            session_id,
            None,
            "assistant_message",
            &serde_json::json!({ "content": assistant_reply }).to_string(),
        )
        .unwrap();
    }

    #[derive(Debug)]
    struct TestDummyProvider;
    impl clawcode::provider::Provider for TestDummyProvider {
        fn id(&self) -> &clawcode::provider::ProviderId {
            static ID: std::sync::LazyLock<clawcode::provider::ProviderId> =
                std::sync::LazyLock::new(|| clawcode::provider::ProviderId::new("test-dummy"));
            &ID
        }
        fn capabilities(&self) -> clawcode::provider::ProviderCapabilities {
            clawcode::provider::ProviderCapabilities {
                streaming: false,
                tools: false,
            }
        }
        fn models(&self) -> Vec<clawcode::provider::ModelInfo> {
            Vec::new()
        }
        fn send(
            &self,
            _: &clawcode::provider::StreamRequest,
        ) -> Result<clawcode::provider::StreamResponse, clawcode::provider::ProviderError> {
            Ok(clawcode::provider::StreamResponse { events: Vec::new() })
        }
    }

    let writer = clawcode::persistence::WriterHandle::spawn(
        clawcode::persistence::Db::open(&path).expect("writer db"),
    );

    let mut app = App::default();
    let service_db = clawcode::persistence::Db::open(&path).expect("service db");
    app.set_command_service(clawcode::cli::CommandService::with_db(
        clawcode::cli::CliDiscovery::new(),
        service_db,
    ));
    app.attach_runtime(db, writer, Box::new(TestDummyProvider));
    // Switch to session: hydrations + replay_events_after
    app.switch_session(session_id);
    assert_eq!(app.active_session_id(), Some(session_id));

    // Verify all 12 turns present in transcript without gaps or duplicates
    for i in 0..12 {
        let user_str = format!("User turn {i} question about Rust");
        let asst_str = format!("Assistant turn {i} explanation with details");
        assert!(
            app.transcript().contains(&user_str),
            "missing user turn {i}"
        );
        assert!(
            app.transcript().contains(&asst_str),
            "missing assistant turn {i}"
        );

        let user_count = app.transcript().matches(&user_str).count();
        let asst_count = app.transcript().matches(&asst_str).count();
        assert_eq!(user_count, 1, "duplicate user turn {i}");
        assert_eq!(asst_count, 1, "duplicate assistant turn {i}");
    }

    // Switch to other session
    app.switch_session(other_session.id);
    assert_eq!(app.active_session_id(), Some(other_session.id));
    assert!(app.transcript().contains("Other user question"));
    assert!(!app.transcript().contains("User turn 0 question"));

    // Switch back to multi-turn session
    app.switch_session(session_id);
    assert_eq!(app.active_session_id(), Some(session_id));

    // Verify all turns still intact with no duplicate messages after round-trip switch
    for i in 0..12 {
        let user_str = format!("User turn {i} question about Rust");
        let asst_str = format!("Assistant turn {i} explanation with details");
        assert!(
            app.transcript().contains(&user_str),
            "missing user turn {i} after switch"
        );
        assert!(
            app.transcript().contains(&asst_str),
            "missing assistant turn {i} after switch"
        );

        let user_count = app.transcript().matches(&user_str).count();
        let asst_count = app.transcript().matches(&asst_str).count();
        assert_eq!(user_count, 1, "duplicate user turn {i} after switch");
        assert_eq!(asst_count, 1, "duplicate assistant turn {i} after switch");
    }

    // Verify replay_events_after returns events in strict sequential order without gaps
    let events = app
        .runtime()
        .expect("runtime client")
        .replay_events_after(session_id, -1)
        .expect("replay succeeds");
    assert_eq!(events.len(), 24, "12 turns * 2 events");
    for (idx, event) in events.iter().enumerate() {
        assert_eq!(event.seq, idx as i64, "sequential seq without gaps");
    }

    let _ = std::fs::remove_file(&path);
}
