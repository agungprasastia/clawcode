//! End-to-end wiring: prompt submission flows through `RuntimeClient` into a
//! generation thread, and `App::poll_runtime` drains bus events into the
//! transcript view. Provider is a fake; no network involved.

use clawcode::persistence::{Db, InputStatus, ToolCallStatus, WriterHandle};
use clawcode::provider::{
    FinishReason, ModelInfo, Provider, ProviderCapabilities, ProviderError, ProviderId,
    StreamEvent, StreamRequest, StreamResponse, Usage,
};
use clawcode::runtime::{EventBus, client::RuntimeClient};
use clawcode::tui::{App, Input, UiEvent};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_db_path(tag: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock drifted")
        .as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("clawcode-test-wiring-{tag}-{nonce}-{count}.db"))
}

#[derive(Debug)]
struct EchoProvider;

impl Provider for EchoProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::OnceLock<ProviderId> = std::sync::OnceLock::new();
        ID.get_or_init(|| ProviderId::new("fake"))
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: false,
        }
    }
    fn models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }
    fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        Ok(StreamResponse {
            events: vec![
                StreamEvent::TextDelta(format!("echo:{}", request.prompt)),
                StreamEvent::Usage(Usage {
                    input_tokens: 1,
                    output_tokens: 2,
                }),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        })
    }
}

fn attach(app: &mut App, path: &Path) {
    let db = Db::open(path).expect("runtime db");
    let writer = WriterHandle::spawn(Db::open(path).expect("writer db"));
    app.attach_runtime(db, writer, Box::new(EchoProvider));
}

#[test]
fn prompt_submission_runs_generation_and_polls_transcript() {
    let path = temp_db_path("prompt_sub");
    let mut app = App::default();
    attach(&mut app, &path);

    app.apply(UiEvent::Input(Input::Character('h')));
    app.apply(UiEvent::Input(Input::Character('i')));
    app.apply(UiEvent::Input(Input::Submit));

    assert!(app.active_session_id().is_some());

    // Drain until the generation finishes (poll in a loop; the worker and
    // generation thread run concurrently with the test thread).
    for _ in 0..500 {
        app.poll_runtime();
        if matches!(
            app.conversation_status(),
            clawcode::tui::ConversationStatus::Finished(_)
        ) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.poll_runtime();

    assert!(
        app.transcript().contains("echo:hi"),
        "transcript should contain echoed delta, got: {}",
        app.transcript()
    );
    assert!(
        matches!(
            app.conversation_status(),
            clawcode::tui::ConversationStatus::Finished(_)
        ),
        "generation should finish"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn poll_runtime_without_attached_runtime_is_noop() {
    let mut app = App::default();
    app.poll_runtime();
    assert!(app.transcript().is_empty());
}

#[test]
fn error_from_provider_surfaces_as_diagnostic() {
    #[derive(Debug)]
    struct FailingProvider;

    impl Provider for FailingProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::OnceLock<ProviderId> = std::sync::OnceLock::new();
            ID.get_or_init(|| ProviderId::new("fake"))
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                streaming: true,
                tools: false,
            }
        }
        fn models(&self) -> Vec<ModelInfo> {
            Vec::new()
        }
        fn send(&self, _: &StreamRequest) -> Result<StreamResponse, ProviderError> {
            Err(ProviderError::Network("boom".into()))
        }
    }

    let path = temp_db_path("failing_provider");
    let mut app = App::default();
    let db = Db::open(&path).expect("runtime db");
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    app.attach_runtime(db, writer, Box::new(FailingProvider));

    app.apply(UiEvent::Input(Input::Character('h')));
    app.apply(UiEvent::Input(Input::Submit));

    for _ in 0..500 {
        app.poll_runtime();
        if matches!(
            app.conversation_status(),
            clawcode::tui::ConversationStatus::Error
        ) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.poll_runtime();

    assert!(app.diagnostic().contains("boom"));
    assert!(matches!(
        app.conversation_status(),
        clawcode::tui::ConversationStatus::Error
    ));

    let _ = std::fs::remove_file(&path);
}
#[derive(Debug)]
struct SingleToolProvider {
    calls: Arc<AtomicUsize>,
    path: &'static str,
}

impl Provider for SingleToolProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::LazyLock<ProviderId> =
            std::sync::LazyLock::new(|| ProviderId::new("fake-tool"));
        &ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
        }
    }

    fn models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }

    fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(StreamResponse {
            events: vec![
                StreamEvent::ToolCallStart {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    id: "call-1".to_string(),
                    arguments: format!(r#"{{"path":"{}"}}"#, self.path),
                },
                StreamEvent::ToolCallEnd {
                    id: "call-1".to_string(),
                },
                StreamEvent::Finish {
                    reason: FinishReason::ToolCall,
                },
            ],
        })
    }
}

#[derive(Debug)]
struct MalformedToolProvider {
    calls: Arc<AtomicUsize>,
    events: Vec<StreamEvent>,
}

impl Provider for MalformedToolProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::LazyLock<ProviderId> =
            std::sync::LazyLock::new(|| ProviderId::new("malformed-tool"));
        &ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
        }
    }

    fn models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }

    fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(StreamResponse {
            events: self.events.clone(),
        })
    }
}

#[test]
fn one_tool_call_is_joined_and_settled_before_generation_finishes() {
    let path = temp_db_path("one_tool_settlement");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("tool").unwrap();
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_subscription, receiver) = bus.subscribe(Some(session.id));
    let calls = Arc::new(AtomicUsize::new(0));
    let client = RuntimeClient::spawn(
        db,
        writer,
        Box::new(SingleToolProvider {
            calls: Arc::clone(&calls),
            path: "Cargo.toml",
        }),
        bus,
    );
    client
        .start_generation(session.id, "plan", "fake-tool", "model", "read")
        .unwrap();

    let started = std::time::Instant::now();
    let mut finished = None;
    while started.elapsed() < std::time::Duration::from_secs(3) {
        if let Ok(event) = receiver.recv_timeout(std::time::Duration::from_millis(50))
            && event.kind == "generation_finished"
        {
            finished = Some(event);
            break;
        }
    }
    let finished = finished.expect("generation must finish after joined tool");
    assert!(finished.payload_json.contains("completed"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let db = client.shutdown();
    let messages = db.messages(session.id).unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.role.as_str())
            .collect::<Vec<_>>(),
        vec!["user", "assistant", "tool"]
    );
    let assistant_id = messages[1].id;
    let tool_calls = db.tool_calls(session.id).unwrap();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].assistant_message_id, assistant_id);
    assert_eq!(tool_calls[0].status, ToolCallStatus::Completed);
    let events = db.events_after(session.id, -1).unwrap();
    let created = events
        .iter()
        .find(|event| event.kind == "tool_call_created")
        .expect("creation event must be durable");
    assert!(created.payload_json.contains(&assistant_id.to_string()));
    assert!(
        events
            .iter()
            .position(|event| event.kind == "tool_call_settled")
            .unwrap()
            < events
                .iter()
                .position(|event| event.kind == "generation_finished")
                .unwrap()
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn failed_local_tool_never_completes_generation() {
    let path = temp_db_path("failed_tool");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("tool failure").unwrap();
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_subscription, receiver) = bus.subscribe(Some(session.id));
    let calls = Arc::new(AtomicUsize::new(0));
    let client = RuntimeClient::spawn(
        db,
        writer,
        Box::new(SingleToolProvider {
            calls: Arc::clone(&calls),
            path: "missing-phase1-file",
        }),
        bus,
    );
    client
        .start_generation(session.id, "plan", "fake-tool", "model", "read")
        .unwrap();

    let started = std::time::Instant::now();
    let mut finished = None;
    while started.elapsed() < std::time::Duration::from_secs(3) {
        if let Ok(event) = receiver.recv_timeout(std::time::Duration::from_millis(50))
            && event.kind == "generation_finished"
        {
            finished = Some(event);
            break;
        }
    }
    let finished = finished.expect("failed tool generation must finish");
    assert!(finished.payload_json.contains("failed"));
    assert!(!finished.payload_json.contains("completed"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let db = client.shutdown();
    let tool_calls = db.tool_calls(session.id).unwrap();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].status, ToolCallStatus::Failed);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn incomplete_or_multiple_tool_calls_fail_before_execution() {
    let cases = [
        (
            "incomplete",
            vec![
                StreamEvent::ToolCallStart {
                    id: "call-1".into(),
                    name: "read_file".into(),
                },
                StreamEvent::ToolCallDelta {
                    id: "call-1".into(),
                    arguments: r#"{"path":"Cargo.toml"}"#.into(),
                },
                StreamEvent::Finish {
                    reason: FinishReason::ToolCall,
                },
            ],
        ),
        (
            "multiple",
            vec![
                StreamEvent::ToolCallStart {
                    id: "call-1".into(),
                    name: "read_file".into(),
                },
                StreamEvent::ToolCallEnd {
                    id: "call-1".into(),
                },
                StreamEvent::ToolCallStart {
                    id: "call-2".into(),
                    name: "read_file".into(),
                },
                StreamEvent::ToolCallEnd {
                    id: "call-2".into(),
                },
                StreamEvent::Finish {
                    reason: FinishReason::ToolCall,
                },
            ],
        ),
        (
            "delta_after_end",
            vec![
                StreamEvent::ToolCallStart {
                    id: "call-1".into(),
                    name: "read_file".into(),
                },
                StreamEvent::ToolCallEnd {
                    id: "call-1".into(),
                },
                StreamEvent::ToolCallDelta {
                    id: "call-1".into(),
                    arguments: "{}".into(),
                },
                StreamEvent::Finish {
                    reason: FinishReason::ToolCall,
                },
            ],
        ),
        (
            "oversized_delta",
            vec![
                StreamEvent::ToolCallStart {
                    id: "call-1".into(),
                    name: "read_file".into(),
                },
                StreamEvent::ToolCallDelta {
                    id: "call-1".into(),
                    arguments: "x".repeat(clawcode::persistence::MAX_TOOL_OUTPUT_BYTES + 1),
                },
                StreamEvent::ToolCallEnd {
                    id: "call-1".into(),
                },
                StreamEvent::Finish {
                    reason: FinishReason::ToolCall,
                },
            ],
        ),
    ];
    for (tag, events) in cases {
        let path = temp_db_path(tag);
        let db = Db::open(&path).unwrap();
        let session = db.create_session(tag).unwrap();
        let writer = WriterHandle::spawn(Db::open(&path).unwrap());
        let bus = EventBus::new();
        let (_subscription, receiver) = bus.subscribe(Some(session.id));
        let calls = Arc::new(AtomicUsize::new(0));
        let client = RuntimeClient::spawn(
            db,
            writer,
            Box::new(MalformedToolProvider {
                calls: Arc::clone(&calls),
                events,
            }),
            bus,
        );
        client
            .start_generation(session.id, "plan", "malformed-tool", "model", "reject")
            .unwrap();
        let started = std::time::Instant::now();
        let mut finished = None;
        while started.elapsed() < std::time::Duration::from_secs(3) {
            if let Ok(event) = receiver.recv_timeout(std::time::Duration::from_millis(50))
                && event.kind == "generation_finished"
            {
                finished = Some(event);
                break;
            }
        }
        let finished = finished.expect("malformed tool stream must finish");
        assert!(finished.payload_json.contains("failed"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let generation_id = finished.generation_id.unwrap();
        let db = client.shutdown();
        assert_eq!(db.tool_calls(session.id).unwrap().len(), 0);
        assert_eq!(
            db.generation_status(generation_id).unwrap(),
            Some(clawcode::persistence::GenerationStatus::Failed)
        );
        let _ = std::fs::remove_file(&path);
    }
}
#[test]
fn one_tool_call_does_not_start_provider_continuation() {
    let calls = Arc::new(AtomicUsize::new(0));

    #[derive(Debug)]
    struct ToolThenEmptyThenSummaryProvider {
        turn: Arc<AtomicUsize>,
    }

    impl Provider for ToolThenEmptyThenSummaryProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::LazyLock<ProviderId> =
                std::sync::LazyLock::new(|| ProviderId::new("fake"));
            &ID
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                streaming: true,
                tools: true,
            }
        }
        fn models(&self) -> Vec<ModelInfo> {
            Vec::new()
        }
        fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
            let turn = self.turn.fetch_add(1, Ordering::SeqCst);
            match turn {
                0 => Ok(StreamResponse {
                    events: vec![
                        StreamEvent::ToolCallStart {
                            id: "call_1".into(),
                            name: "list_dir".into(),
                        },
                        StreamEvent::ToolCallDelta {
                            id: "call_1".into(),
                            arguments: r#"{"path":"."}"#.into(),
                        },
                        StreamEvent::ToolCallEnd {
                            id: "call_1".into(),
                        },
                        StreamEvent::Finish {
                            reason: FinishReason::ToolCall,
                        },
                    ],
                }),
                1 => {
                    let last_msg = request.messages.last().expect("must have tool message");
                    assert_eq!(last_msg.role, "tool");
                    assert_eq!(last_msg.name.as_deref(), Some("list_dir"));

                    Ok(StreamResponse {
                        events: vec![StreamEvent::Finish {
                            reason: FinishReason::Stop,
                        }],
                    })
                }
                _ => {
                    let last_msg = request.messages.last().expect("must have recovery message");
                    assert_eq!(last_msg.role, "user");
                    assert!(last_msg.content.contains("summarize your findings"));

                    Ok(StreamResponse {
                        events: vec![
                            StreamEvent::TextDelta("Here is the recovered summary.".into()),
                            StreamEvent::Finish {
                                reason: FinishReason::Stop,
                            },
                        ],
                    })
                }
            }
        }
    }

    let path = temp_db_path("one_call_no_continuation");
    let mut app = App::default();
    let db = Db::open(&path).expect("runtime db");
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    app.attach_runtime(
        db,
        writer,
        Box::new(ToolThenEmptyThenSummaryProvider {
            turn: Arc::clone(&calls),
        }),
    );

    app.apply(UiEvent::Input(Input::Character('g')));
    app.apply(UiEvent::Input(Input::Character('o')));
    app.apply(UiEvent::Input(Input::Submit));

    for _ in 0..500 {
        app.poll_runtime();
        if matches!(
            app.conversation_status(),
            clawcode::tui::ConversationStatus::Finished(_)
        ) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    app.poll_runtime();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        !app.transcript().contains("Here is the recovered summary."),
        "one-call tool path must not request continuation"
    );
    assert!(matches!(
        app.conversation_status(),
        clawcode::tui::ConversationStatus::Finished(_)
    ));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn runtime_backed_prompt_submission_admits_and_promotes_input() {
    let path = temp_db_path("admit_and_promote");
    let mut app = App::default();
    attach(&mut app, &path);

    app.apply(UiEvent::Input(Input::Character('p')));
    app.apply(UiEvent::Input(Input::Character('i')));
    app.apply(UiEvent::Input(Input::Character('n')));
    app.apply(UiEvent::Input(Input::Character('g')));
    app.apply(UiEvent::Input(Input::Submit));

    let session_id = app.active_session_id().expect("active session id");

    for _ in 0..500 {
        app.poll_runtime();
        if matches!(
            app.conversation_status(),
            clawcode::tui::ConversationStatus::Finished(_)
        ) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.poll_runtime();

    let db = Db::open(&path).expect("open db");

    // DB has session_inputs with status Promoted and user_message_id linked
    let inputs = db.session_inputs(session_id).expect("session inputs");
    assert_eq!(inputs.len(), 1);
    let input = &inputs[0];
    assert_eq!(input.content, "ping");
    assert_eq!(input.status, InputStatus::Promoted);
    let user_message_id = input.user_message_id.expect("linked user_message_id");

    // DB has messages with role="user" created via promotion
    let messages = db.messages(session_id).expect("messages");
    assert!(
        messages
            .iter()
            .any(|m| m.id == user_message_id && m.role == "user" && m.content == "ping"),
        "db must have message with role='user' created via promotion"
    );

    // Events include prompt_admitted and prompt_promoted
    let events = db.events_after(session_id, -1).expect("events");
    assert!(
        events.iter().any(|e| e.kind == "prompt_admitted"),
        "events must include prompt_admitted"
    );
    assert!(
        events.iter().any(|e| e.kind == "prompt_promoted"),
        "events must include prompt_promoted"
    );

    // App transcript shows user prompt and echo response
    assert!(
        app.transcript().contains("> ping"),
        "transcript must show user prompt, got: {}",
        app.transcript()
    );
    assert!(
        app.transcript().contains("echo:ping"),
        "transcript must show echo response, got: {}",
        app.transcript()
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn runtime_backed_oversized_prompt_rejected_without_transcript_or_db_write() {
    let path = temp_db_path("oversized_rejected");
    let mut app = App::default();
    attach(&mut app, &path);

    let oversized = "x".repeat(clawcode::persistence::MAX_MESSAGE_BYTES + 1);
    app.submit_user_prompt(&oversized);

    // App transcript remains empty (does not show rejected prompt)
    assert!(
        app.transcript().is_empty(),
        "transcript must remain empty, got: {}",
        app.transcript()
    );

    // Diagnostic contains "message too large"
    assert!(
        app.diagnostic().contains("message too large"),
        "diagnostic must contain 'message too large', got: {}",
        app.diagnostic()
    );

    // DB has 0 session_inputs and 0 messages
    let conn = rusqlite::Connection::open(&path).expect("open sqlite");
    let inputs_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM session_inputs", [], |r| r.get(0))
        .unwrap();
    let messages_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(inputs_count, 0, "session_inputs must have 0 rows");
    assert_eq!(messages_count, 0, "messages must have 0 rows");

    let _ = std::fs::remove_file(&path);
}
