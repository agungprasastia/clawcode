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
        let call_count = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_count == 0 {
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
        } else {
            Ok(StreamResponse {
                events: vec![
                    StreamEvent::TextDelta("File read complete.".to_string()),
                    StreamEvent::Finish {
                        reason: FinishReason::Stop,
                    },
                ],
            })
        }
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
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let db = client.shutdown();
    let messages = db.messages(session.id).unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.role.as_str())
            .collect::<Vec<_>>(),
        vec!["user", "assistant", "tool", "assistant"]
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
fn tool_failure_does_not_abort_generation_and_feeds_back_error() {
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
    assert!(finished.payload_json.contains("completed"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let db = client.shutdown();
    let tool_calls = db.tool_calls(session.id).unwrap();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].status, ToolCallStatus::Failed);

    let messages = db.messages(session.id).unwrap();
    assert_eq!(
        messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
        vec!["user", "assistant", "tool", "assistant"]
    );
    assert!(!messages[2].content.is_empty());
    assert_eq!(messages[3].content, "File read complete.");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn incomplete_or_malformed_tool_calls_fail_before_execution() {
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

#[derive(Debug)]
struct ParallelToolProvider {
    calls: Arc<AtomicUsize>,
    turn2_request: Arc<std::sync::Mutex<Option<StreamRequest>>>,
}

impl Provider for ParallelToolProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::LazyLock<ProviderId> =
            std::sync::LazyLock::new(|| ProviderId::new("parallel-tool"));
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
        let call_count = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_count == 0 {
            Ok(StreamResponse {
                events: vec![
                    StreamEvent::ToolCallStart {
                        id: "call-1".to_string(),
                        name: "read_file".to_string(),
                    },
                    StreamEvent::ToolCallDelta {
                        id: "call-1".to_string(),
                        arguments: r#"{"path":"Cargo.toml"}"#.to_string(),
                    },
                    StreamEvent::ToolCallEnd {
                        id: "call-1".to_string(),
                    },
                    StreamEvent::ToolCallStart {
                        id: "call-2".to_string(),
                        name: "read_file".to_string(),
                    },
                    StreamEvent::ToolCallDelta {
                        id: "call-2".to_string(),
                        arguments: r#"{"path":"Cargo.toml"}"#.to_string(),
                    },
                    StreamEvent::ToolCallEnd {
                        id: "call-2".to_string(),
                    },
                    StreamEvent::Finish {
                        reason: FinishReason::ToolCall,
                    },
                ],
            })
        } else {
            *self.turn2_request.lock().unwrap() = Some(request.clone());
            Ok(StreamResponse {
                events: vec![
                    StreamEvent::TextDelta("Parallel tools executed.".to_string()),
                    StreamEvent::Finish {
                        reason: FinishReason::Stop,
                    },
                ],
            })
        }
    }
}

#[test]
fn multiple_parallel_tool_calls_execute_sequentially_settle_and_feed_back() {
    let path = temp_db_path("parallel_tools");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("parallel tools").unwrap();
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_subscription, receiver) = bus.subscribe(Some(session.id));
    let calls = Arc::new(AtomicUsize::new(0));
    let turn2_request = Arc::new(std::sync::Mutex::new(None));
    let client = RuntimeClient::spawn(
        db,
        writer,
        Box::new(ParallelToolProvider {
            calls: Arc::clone(&calls),
            turn2_request: Arc::clone(&turn2_request),
        }),
        bus,
    );
    client
        .start_generation(session.id, "plan", "parallel-tool", "model", "read")
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
    let finished = finished.expect("parallel tool generation must finish");
    assert!(finished.payload_json.contains("completed"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let db = client.shutdown();
    let tool_calls = db.tool_calls(session.id).unwrap();
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0].call_id, "call-1");
    assert_eq!(tool_calls[0].status, ToolCallStatus::Completed);
    assert_eq!(tool_calls[1].call_id, "call-2");
    assert_eq!(tool_calls[1].status, ToolCallStatus::Completed);

    let messages = db.messages(session.id).unwrap();
    assert_eq!(
        messages.iter().map(|m| m.role.as_str()).collect::<Vec<_>>(),
        vec!["user", "assistant", "tool", "tool", "assistant"]
    );

    let req = turn2_request
        .lock()
        .unwrap()
        .take()
        .expect("turn 2 request recorded");
    let assistant_msg = req
        .messages
        .iter()
        .find(|m| m.role == "assistant")
        .expect("assistant msg");
    assert_eq!(
        assistant_msg.tool_calls.as_ref().map(|tc| tc.len()),
        Some(2)
    );
    let tool_msgs: Vec<_> = req.messages.iter().filter(|m| m.role == "tool").collect();
    assert_eq!(tool_msgs.len(), 2);
    assert_eq!(tool_msgs[0].tool_call_id.as_deref(), Some("call-1"));
    assert_eq!(tool_msgs[1].tool_call_id.as_deref(), Some("call-2"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn cross_turn_history_preserves_tool_messages_and_tool_calls() {
    let path = temp_db_path("cross_turn_history");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("cross-turn").unwrap();
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_subscription, receiver) = bus.subscribe(Some(session.id));
    let calls = Arc::new(AtomicUsize::new(0));
    let turn2_request = Arc::new(std::sync::Mutex::new(None));
    let client = RuntimeClient::spawn(
        db,
        writer,
        Box::new(ParallelToolProvider {
            calls: Arc::clone(&calls),
            turn2_request: Arc::clone(&turn2_request),
        }),
        bus,
    );

    client
        .start_generation(session.id, "plan", "parallel-tool", "model", "first prompt")
        .unwrap();

    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(3) {
        if let Ok(event) = receiver.recv_timeout(std::time::Duration::from_millis(50))
            && event.kind == "generation_finished"
        {
            break;
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    #[derive(Debug)]
    struct Turn2CaptureProvider {
        captured: Arc<std::sync::Mutex<Option<StreamRequest>>>,
    }
    impl Provider for Turn2CaptureProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::LazyLock<ProviderId> =
                std::sync::LazyLock::new(|| ProviderId::new("turn2-capture"));
            &ID
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
            *self.captured.lock().unwrap() = Some(request.clone());
            Ok(StreamResponse {
                events: vec![
                    StreamEvent::TextDelta("Turn 2 answered.".to_string()),
                    StreamEvent::Finish {
                        reason: FinishReason::Stop,
                    },
                ],
            })
        }
    }

    let captured = Arc::new(std::sync::Mutex::new(None));
    let db2 = client.shutdown();
    let writer2 = WriterHandle::spawn(Db::open(&path).expect("writer db 2"));
    let bus2 = EventBus::new();
    let (_sub2, receiver2) = bus2.subscribe(Some(session.id));
    let client2 = RuntimeClient::spawn(
        db2,
        writer2,
        Box::new(Turn2CaptureProvider {
            captured: Arc::clone(&captured),
        }),
        bus2,
    );

    client2
        .start_generation(
            session.id,
            "plan",
            "turn2-capture",
            "model",
            "second prompt",
        )
        .unwrap();

    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(3) {
        if let Ok(event) = receiver2.recv_timeout(std::time::Duration::from_millis(50))
            && event.kind == "generation_finished"
        {
            break;
        }
    }

    let req = captured
        .lock()
        .unwrap()
        .take()
        .expect("turn 2 must have received request");
    let roles: Vec<&str> = req.messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(
        roles,
        vec![
            "system",
            "user",
            "assistant",
            "tool",
            "tool",
            "assistant",
            "user"
        ]
    );
    let gen1_assistant = &req.messages[2];
    assert!(gen1_assistant.tool_calls.is_some());
    let tcs = gen1_assistant.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 2);
    assert_eq!(tcs[0].id, "call-1");
    assert_eq!(tcs[1].id, "call-2");

    let tool1 = &req.messages[3];
    assert_eq!(tool1.tool_call_id.as_deref(), Some("call-1"));
    assert_eq!(tool1.name.as_deref(), Some("read_file"));

    let tool2 = &req.messages[4];
    assert_eq!(tool2.tool_call_id.as_deref(), Some("call-2"));
    assert_eq!(tool2.name.as_deref(), Some("read_file"));

    let _ = client2.shutdown();
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

#[test]
fn per_session_serialization_drains_sequentially_without_overlap() {
    let path = temp_db_path("session_serialization");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("serial").unwrap();
    let session_id = session.id;
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_sub, rx) = bus.subscribe(Some(session_id));

    let events_log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log_clone = Arc::clone(&events_log);

    #[derive(Debug)]
    struct SerialProvider {
        log: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Provider for SerialProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::LazyLock<ProviderId> =
                std::sync::LazyLock::new(|| ProviderId::new("serial-fake"));
            &ID
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
        fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
            Ok(StreamResponse { events: vec![] })
        }
        fn stream(
            &self,
            request: &StreamRequest,
        ) -> Result<clawcode::provider::ProviderStream, ProviderError> {
            let prompt = request.prompt.clone();
            let log = Arc::clone(&self.log);
            let (sender, stream) = clawcode::provider::ProviderStream::channel(16);
            std::thread::spawn(move || {
                log.lock().unwrap().push(format!("start:{prompt}"));
                std::thread::sleep(std::time::Duration::from_millis(60));
                let _ = sender.send(StreamEvent::TextDelta(format!("reply:{prompt}")));
                let _ = sender.flush();
                let _ = sender.send(StreamEvent::Finish {
                    reason: FinishReason::Stop,
                });
                let _ = sender.flush();
                log.lock().unwrap().push(format!("finish:{prompt}"));
            });
            Ok(stream)
        }
    }

    let client = RuntimeClient::spawn(db, writer, Box::new(SerialProvider { log: log_clone }), bus);

    client
        .start_generation(session_id, "plan", "fake", "serial", "prompt 1")
        .unwrap();
    client
        .start_generation(session_id, "plan", "fake", "serial", "prompt 2")
        .unwrap();

    let mut finished_count = 0;
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(5) {
        if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
            && event.kind == "generation_finished"
        {
            finished_count += 1;
            if finished_count >= 2 {
                break;
            }
        }
    }
    assert_eq!(finished_count, 2, "both turns must finish");

    let log = events_log.lock().unwrap().clone();
    assert_eq!(
        log,
        vec![
            "start:prompt 1",
            "finish:prompt 1",
            "start:prompt 2",
            "finish:prompt 2"
        ],
        "turns must run sequentially in order without overlapping"
    );

    let db = client.shutdown();
    let messages = db.messages(session_id).unwrap();
    let user_msgs: Vec<_> = messages.iter().filter(|m| m.role == "user").collect();
    assert_eq!(user_msgs.len(), 2);
    assert_eq!(user_msgs[0].content, "prompt 1");
    assert_eq!(user_msgs[1].content, "prompt 2");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn cross_session_concurrency_runs_in_parallel() {
    let path = temp_db_path("cross_session_parallel");
    let db = Db::open(&path).expect("runtime db");
    let session_a = db.create_session("session A").unwrap();
    let session_b = db.create_session("session B").unwrap();
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_sub_a, rx_a) = bus.subscribe(Some(session_a.id));
    let (_sub_b, rx_b) = bus.subscribe(Some(session_b.id));

    let barrier = Arc::new(std::sync::Barrier::new(2));
    let barrier_clone = Arc::clone(&barrier);

    #[derive(Debug)]
    struct BarrierProvider {
        barrier: Arc<std::sync::Barrier>,
    }

    impl Provider for BarrierProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::LazyLock<ProviderId> =
                std::sync::LazyLock::new(|| ProviderId::new("barrier-fake"));
            &ID
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
        fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
            Ok(StreamResponse { events: vec![] })
        }
        fn stream(
            &self,
            request: &StreamRequest,
        ) -> Result<clawcode::provider::ProviderStream, ProviderError> {
            let barrier = Arc::clone(&self.barrier);
            let prompt = request.prompt.clone();
            let (sender, stream) = clawcode::provider::ProviderStream::channel(16);
            std::thread::spawn(move || {
                barrier.wait();
                let _ = sender.send(StreamEvent::TextDelta(format!("done:{prompt}")));
                let _ = sender.flush();
                let _ = sender.send(StreamEvent::Finish {
                    reason: FinishReason::Stop,
                });
                let _ = sender.flush();
            });
            Ok(stream)
        }
    }

    let client = RuntimeClient::spawn(
        db,
        writer,
        Box::new(BarrierProvider {
            barrier: barrier_clone,
        }),
        bus,
    );

    client
        .start_generation(session_a.id, "plan", "fake", "barrier", "hello A")
        .unwrap();
    client
        .start_generation(session_b.id, "plan", "fake", "barrier", "hello B")
        .unwrap();

    let start = std::time::Instant::now();
    let mut a_done = false;
    let mut b_done = false;

    while start.elapsed() < std::time::Duration::from_secs(10) && (!a_done || !b_done) {
        while let Ok(event) = rx_a.try_recv() {
            if event.kind == "generation_finished" {
                a_done = true;
                break;
            }
        }
        while let Ok(event) = rx_b.try_recv() {
            if event.kind == "generation_finished" {
                b_done = true;
                break;
            }
        }
        if a_done && b_done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(a_done, "session A generation must finish");
    assert!(b_done, "session B generation must finish");

    let _db = client.shutdown();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn wake_coalescing_without_duplicate_promotions_or_provider_turns() {
    let path = temp_db_path("wake_coalescing");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("coalesce").unwrap();
    let session_id = session.id;
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_sub, rx) = bus.subscribe(Some(session_id));

    let started_p1 = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let release_p1 = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let turn_calls = Arc::new(AtomicUsize::new(0));

    let started_clone = Arc::clone(&started_p1);
    let release_clone = Arc::clone(&release_p1);
    let calls_clone = Arc::clone(&turn_calls);

    #[derive(Debug)]
    struct CoalescingProvider {
        started: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
        release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
        calls: Arc<AtomicUsize>,
    }

    impl Provider for CoalescingProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::LazyLock<ProviderId> =
                std::sync::LazyLock::new(|| ProviderId::new("coalesce-fake"));
            &ID
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
        fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
            Ok(StreamResponse { events: vec![] })
        }
        fn stream(
            &self,
            request: &StreamRequest,
        ) -> Result<clawcode::provider::ProviderStream, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (sender, stream) = clawcode::provider::ProviderStream::channel(16);
            let is_p1 = request.prompt == "P1";
            let started = Arc::clone(&self.started);
            let release = Arc::clone(&self.release);

            std::thread::spawn(move || {
                if is_p1 {
                    *started.0.lock().unwrap() = true;
                    started.1.notify_all();

                    let mut lock = release.0.lock().unwrap();
                    while !*lock {
                        lock = release.1.wait(lock).unwrap();
                    }
                }
                let _ = sender.send(StreamEvent::TextDelta("ok".into()));
                let _ = sender.flush();
                let _ = sender.send(StreamEvent::Finish {
                    reason: FinishReason::Stop,
                });
                let _ = sender.flush();
            });
            Ok(stream)
        }
    }

    let client = RuntimeClient::spawn(
        db,
        writer,
        Box::new(CoalescingProvider {
            started: started_clone,
            release: release_clone,
            calls: calls_clone,
        }),
        bus,
    );

    client
        .start_generation(session_id, "plan", "fake", "coalesce", "P1")
        .unwrap();

    let mut lock = started_p1.0.lock().unwrap();
    while !*lock {
        lock = started_p1.1.wait(lock).unwrap();
    }
    drop(lock);

    client
        .start_generation(session_id, "plan", "fake", "coalesce", "P2")
        .unwrap();
    client
        .start_generation(session_id, "plan", "fake", "coalesce", "P3")
        .unwrap();

    *release_p1.0.lock().unwrap() = true;
    release_p1.1.notify_all();

    let start = std::time::Instant::now();
    let mut finished = 0;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
            && event.kind == "generation_finished"
        {
            finished += 1;
            if finished >= 2 {
                break;
            }
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(150));

    assert_eq!(finished, 2, "must finish exactly 2 generations");
    assert_eq!(
        turn_calls.load(Ordering::SeqCst),
        2,
        "coalesced wakes must produce at most one follow-up turn"
    );

    let db = client.shutdown();
    let messages = db.messages(session_id).unwrap();
    let user_msgs: Vec<_> = messages.iter().filter(|m| m.role == "user").collect();
    assert_eq!(user_msgs.len(), 2);
    assert_eq!(user_msgs[0].content, "P1");
    assert_eq!(user_msgs[1].content, "P2");

    let inputs = db.session_inputs(session_id).unwrap();
    assert_eq!(inputs.len(), 3);
    assert_eq!(inputs[0].status, InputStatus::Promoted);
    assert_eq!(inputs[1].status, InputStatus::Promoted);
    assert_eq!(inputs[2].status, InputStatus::Pending);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn interrupt_cleanup_cancels_active_session_and_preserves_pending_input() {
    let path = temp_db_path("interrupt_cleanup");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("interrupt").unwrap();
    let session_id = session.id;
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_sub, rx) = bus.subscribe(Some(session_id));

    let started = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let started_clone = Arc::clone(&started);

    #[derive(Debug)]
    struct BlockUntilCancelledProvider {
        started: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    }

    impl Provider for BlockUntilCancelledProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::LazyLock<ProviderId> =
                std::sync::LazyLock::new(|| ProviderId::new("cancel-fake"));
            &ID
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
        fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
            Ok(StreamResponse { events: vec![] })
        }
        fn stream(
            &self,
            _request: &StreamRequest,
        ) -> Result<clawcode::provider::ProviderStream, ProviderError> {
            let started = Arc::clone(&self.started);
            let (sender, stream) = clawcode::provider::ProviderStream::channel(16);
            std::thread::spawn(move || {
                *started.0.lock().unwrap() = true;
                started.1.notify_all();

                for _ in 0..100 {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    if sender
                        .send(StreamEvent::TextDelta("streaming...".into()))
                        .is_err()
                    {
                        break;
                    }
                    let _ = sender.flush();
                }
            });
            Ok(stream)
        }
    }

    let client = RuntimeClient::spawn(
        db,
        writer,
        Box::new(BlockUntilCancelledProvider {
            started: started_clone,
        }),
        bus,
    );

    client
        .start_generation(session_id, "plan", "fake", "cancel", "P1")
        .unwrap();

    let mut lock = started.0.lock().unwrap();
    while !*lock {
        lock = started.1.wait(lock).unwrap();
    }
    drop(lock);

    client
        .start_generation(session_id, "plan", "fake", "cancel", "P2")
        .unwrap();

    client.cancel_generation(session_id).unwrap();

    let start = std::time::Instant::now();
    let mut got_cancelled = false;
    while start.elapsed() < std::time::Duration::from_secs(4) {
        if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
            && event.kind == "generation_finished"
            && event.payload_json.contains("cancelled")
        {
            got_cancelled = true;
            break;
        }
    }
    assert!(got_cancelled, "active turn must emit cancelled event");

    let drain_start = std::time::Instant::now();
    while drain_start.elapsed() < std::time::Duration::from_secs(2) {
        if !client.is_active(session_id) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        !client.is_active(session_id),
        "coordinator must be idle after interrupt cleanup"
    );

    let db = client.shutdown();
    let inputs = db.session_inputs(session_id).unwrap();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].content, "P1");
    assert_eq!(inputs[0].status, InputStatus::Promoted);
    assert_eq!(inputs[1].content, "P2");
    assert_eq!(inputs[1].status, InputStatus::Pending);

    let messages = db.messages(session_id).unwrap();
    let user_messages: Vec<_> = messages.iter().filter(|m| m.role == "user").collect();
    assert_eq!(
        user_messages.len(),
        1,
        "only P1 should have been promoted to a message"
    );
    assert_eq!(user_messages[0].content, "P1");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn idle_or_unknown_interrupt_is_safe_noop() {
    let path = temp_db_path("idle_interrupt");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("idle_session").unwrap();
    let session_id = session.id;
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let (_sub, rx) = bus.subscribe(Some(session_id));

    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    assert!(client.cancel_generation(session_id).is_ok());
    assert!(client.cancel_generation(999_999).is_ok());

    let mut got_rejected = false;
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(2) {
        if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
            && event.kind == "generation_status"
            && event.payload_json.contains("cancel_rejected")
        {
            got_rejected = true;
            break;
        }
    }
    assert!(got_rejected, "idle cancel must emit cancel_rejected");
    assert!(!client.is_active(session_id));

    let _db = client.shutdown();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_events_after_cursor_boundary() {
    let path = temp_db_path("cursor_boundary");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("cursor_session").unwrap();
    let session_id = session.id;

    for i in 0..5 {
        db.append_event(
            session_id,
            None,
            "test_event",
            &serde_json::json!({ "index": i }).to_string(),
        )
        .unwrap();
    }

    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    let events = client.replay_events_after(session_id, 2).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].seq, 3);
    assert_eq!(events[1].seq, 4);

    let all_events = client.replay_events_after(session_id, -1).unwrap();
    assert_eq!(all_events.len(), 5);
    for (idx, ev) in all_events.iter().enumerate() {
        assert_eq!(ev.seq, idx as i64);
    }

    let empty_events = client.replay_events_after(session_id, 4).unwrap();
    assert!(empty_events.is_empty());

    let _db = client.shutdown();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_subscription_handoff_deduplication() {
    let path = temp_db_path("handoff_dedup");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("dedup_session").unwrap();
    let session_id = session.id;

    let s0 = db.append_event(session_id, None, "event_0", "{}").unwrap();
    let s1 = db.append_event(session_id, None, "event_1", "{}").unwrap();
    let s2 = db.append_event(session_id, None, "event_2", "{}").unwrap();
    assert_eq!(s0, 0);
    assert_eq!(s1, 1);
    assert_eq!(s2, 2);

    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    let mut sub = client.subscribe_after(session_id, 0).unwrap();

    client.bus().publish(clawcode::runtime::RuntimeEvent {
        seq: 2,
        session_id,
        generation_id: None,
        kind: "event_2".into(),
        payload_json: "{}".into(),
    });
    client.bus().publish(clawcode::runtime::RuntimeEvent {
        seq: 3,
        session_id,
        generation_id: None,
        kind: "event_3".into(),
        payload_json: "{}".into(),
    });

    let ev1 = sub.try_recv().unwrap();
    assert_eq!(ev1.seq, 1);
    assert_eq!(sub.loaded_until_seq(), 1);

    let ev2 = sub.try_recv().unwrap();
    assert_eq!(ev2.seq, 2);
    assert_eq!(sub.loaded_until_seq(), 2);

    let ev3 = sub.try_recv().unwrap();
    assert_eq!(ev3.seq, 3);
    assert_eq!(sub.loaded_until_seq(), 3);

    assert!(matches!(
        sub.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));

    let _db = client.shutdown();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_subscription_seq_zero_never_advances_cursor() {
    let path = temp_db_path("seq_zero_control");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("seq_zero_session").unwrap();
    let session_id = session.id;

    db.append_event(session_id, None, "durable_0", "{}")
        .unwrap();

    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    let mut sub = client.subscribe_after(session_id, -1).unwrap();

    let ev0 = sub.try_recv().unwrap();
    assert_eq!(ev0.seq, 0);
    assert_eq!(sub.loaded_until_seq(), -1);

    client.bus().publish(clawcode::runtime::RuntimeEvent {
        seq: 1,
        session_id,
        generation_id: None,
        kind: "durable_1".into(),
        payload_json: "{}".into(),
    });
    let ev1 = sub.try_recv().unwrap();
    assert_eq!(ev1.seq, 1);
    assert_eq!(sub.loaded_until_seq(), 1);

    client.bus().publish(clawcode::runtime::RuntimeEvent {
        seq: 0,
        session_id,
        generation_id: None,
        kind: "status_ping".into(),
        payload_json: "{}".into(),
    });
    client.bus().publish(clawcode::runtime::RuntimeEvent {
        seq: 0,
        session_id,
        generation_id: None,
        kind: "text_delta".into(),
        payload_json: serde_json::json!({ "delta": "hello" }).to_string(),
    });

    let ping = sub.try_recv().unwrap();
    assert_eq!(ping.kind, "status_ping");
    assert_eq!(ping.seq, 0);
    assert_eq!(sub.loaded_until_seq(), 1, "seq 0 must NEVER advance cursor");

    let delta = sub.try_recv().unwrap();
    assert_eq!(delta.kind, "text_delta");
    assert_eq!(delta.seq, 0);
    assert_eq!(sub.loaded_until_seq(), 1, "seq 0 must NEVER advance cursor");

    let _db = client.shutdown();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn replay_subscription_reconnect_recovery_no_loss() {
    let path = temp_db_path("reconnect_recovery");
    let db = Db::open(&path).expect("runtime db");
    let session = db.create_session("reconnect_session").unwrap();
    let session_id = session.id;

    db.append_event(session_id, None, "event_0", "{}").unwrap();
    db.append_event(session_id, None, "event_1", "{}").unwrap();

    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    let bus = EventBus::new();
    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    let mut sub = client.subscribe_after(session_id, -1).unwrap();
    let ev0 = sub.try_recv().unwrap();
    assert_eq!(ev0.seq, 0);
    let ev1 = sub.try_recv().unwrap();
    assert_eq!(ev1.seq, 1);
    assert_eq!(sub.loaded_until_seq(), 1);

    let last_cursor = sub.loaded_until_seq();
    drop(sub);

    let writer_db = Db::open(&path).expect("writer db 2");
    writer_db
        .append_event(session_id, None, "event_2", "{}")
        .unwrap();
    writer_db
        .append_event(session_id, None, "event_3", "{}")
        .unwrap();

    let mut new_sub = client.subscribe_after(session_id, last_cursor).unwrap();

    let ev2 = new_sub.try_recv().unwrap();
    assert_eq!(ev2.seq, 2);
    assert_eq!(new_sub.loaded_until_seq(), 2);

    let ev3 = new_sub.try_recv().unwrap();
    assert_eq!(ev3.seq, 3);
    assert_eq!(new_sub.loaded_until_seq(), 3);

    assert!(matches!(
        new_sub.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));

    let _db = client.shutdown();
    let _ = std::fs::remove_file(&path);
}

#[derive(Debug, Clone)]
struct CapturingProvider {
    requests: Arc<std::sync::Mutex<Vec<StreamRequest>>>,
}

impl CapturingProvider {
    fn new() -> Self {
        Self {
            requests: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

impl Provider for CapturingProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::LazyLock<ProviderId> =
            std::sync::LazyLock::new(|| ProviderId::new("capturing"));
        &ID
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
        self.requests.lock().unwrap().push(request.clone());
        Ok(StreamResponse {
            events: vec![
                StreamEvent::TextDelta(format!("ack:{}", request.prompt)),
                StreamEvent::Usage(Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                }),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        })
    }
}

#[cfg(windows)]
fn lock_file_exclusive(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .expect("exclusive lock")
}

#[cfg(not(windows))]
fn lock_file_exclusive(path: &Path) -> std::fs::File {
    use std::os::unix::fs::PermissionsExt;
    let file = std::fs::File::open(path).expect("open file");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    file
}

#[cfg(windows)]
fn unlock_file(_file: std::fs::File, _path: &Path) {}

#[cfg(not(windows))]
fn unlock_file(_file: std::fs::File, path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644));
}

fn wait_for_finish_or_error(
    rx: &std::sync::mpsc::Receiver<clawcode::runtime::RuntimeEvent>,
) -> String {
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(5) {
        if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
            && (event.kind == "generation_finished" || event.kind == "error")
        {
            return event.kind;
        }
    }
    panic!("timed out waiting for finish or error");
}

fn setup_session_with_workspace(path: &Path, ws_path: &Path) -> (Db, WriterHandle, i64) {
    let db = Db::open(path).expect("db open");
    let ws = db
        .create_workspace(&ws_path.to_string_lossy(), "test ws")
        .unwrap();
    let session = db
        .create_session_in_workspace(ws.id, "test session")
        .unwrap();
    let writer = WriterHandle::spawn(Db::open(path).expect("writer db"));
    (db, writer, session.id)
}

#[test]
fn context_epoch_baseline_exact_and_reusable_after_restart() {
    let db_path = temp_db_path("epoch_restart");
    let ws_dir = std::env::temp_dir().join(format!("clawcode-test-ws-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&ws_dir);
    std::fs::write(ws_dir.join("AGENTS.md"), "initial baseline instructions").unwrap();

    let (db, writer, session_id) = setup_session_with_workspace(&db_path, &ws_dir);
    let provider = CapturingProvider::new();
    let captured = provider.requests.clone();
    let bus = EventBus::new();
    let (_sub_id, rx) = bus.subscribe(None);
    let client = RuntimeClient::spawn(db, writer, Box::new(provider), bus);

    client
        .start_generation(
            session_id,
            "plan",
            "capturing",
            "test-model",
            "turn 1 prompt",
        )
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    let reqs = captured.lock().unwrap().clone();
    assert_eq!(reqs.len(), 1);
    assert!(
        reqs[0].messages[0]
            .content
            .contains("initial baseline instructions")
    );

    // Restart: shutdown client
    let db = client.shutdown();
    let epoch = db.get_active_context_epoch(session_id).unwrap().unwrap();
    assert!(
        epoch
            .baseline_system_text
            .contains("initial baseline instructions")
    );

    // Now update AGENTS.md on disk
    std::fs::write(ws_dir.join("AGENTS.md"), "updated baseline instructions").unwrap();

    // Spawn new client with same db file
    drop(db);
    let db2 = Db::open(&db_path).unwrap();
    let writer2 = WriterHandle::spawn(Db::open(&db_path).unwrap());
    let provider2 = CapturingProvider::new();
    let captured2 = provider2.requests.clone();
    let bus2 = EventBus::new();
    let (_sub_id2, rx2) = bus2.subscribe(None);
    let client2 = RuntimeClient::spawn(db2, writer2, Box::new(provider2), bus2);

    client2
        .start_generation(
            session_id,
            "plan",
            "capturing",
            "test-model",
            "turn 2 prompt",
        )
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx2), "generation_finished");

    let reqs2 = captured2.lock().unwrap().clone();
    assert_eq!(reqs2.len(), 1);
    // Exact baseline system prompt is preserved in messages[0]!
    assert!(
        reqs2[0].messages[0]
            .content
            .contains("initial baseline instructions")
    );
    assert!(
        !reqs2[0].messages[0]
            .content
            .contains("updated baseline instructions")
    );

    // System delta message is present in request history before turn 2 prompt
    let messages = &reqs2[0].messages;
    let has_delta = messages
        .iter()
        .any(|m| m.role == "system" && m.content.contains("updated baseline instructions"));
    assert!(
        has_delta,
        "system delta message must be included in conversation stream"
    );
    assert_eq!(messages.last().unwrap().role, "user");
    assert_eq!(messages.last().unwrap().content, "turn 2 prompt");

    let _db2 = client2.shutdown();
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&ws_dir);
}

#[test]
fn unchanged_instruction_source_emits_no_system_message() {
    let db_path = temp_db_path("epoch_unchanged");
    let ws_dir =
        std::env::temp_dir().join(format!("clawcode-test-ws-unchanged-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&ws_dir);
    std::fs::write(ws_dir.join("AGENTS.md"), "static instructions").unwrap();

    let (db, writer, session_id) = setup_session_with_workspace(&db_path, &ws_dir);
    let bus = EventBus::new();
    let (_sub_id, rx) = bus.subscribe(None);
    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    client
        .start_generation(session_id, "plan", "fake", "echo", "prompt 1")
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    // Turn 2 without touching AGENTS.md
    client
        .start_generation(session_id, "plan", "fake", "echo", "prompt 2")
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    let db = client.shutdown();
    let messages = db.messages(session_id).unwrap();
    let system_msgs: Vec<_> = messages.iter().filter(|m| m.role == "system").collect();
    assert!(
        system_msgs.is_empty(),
        "unchanged source must emit no system message"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&ws_dir);
}

#[test]
fn changed_instruction_source_emits_exactly_one_atomic_system_message_and_updates_snapshot() {
    let db_path = temp_db_path("epoch_changed");
    let ws_dir =
        std::env::temp_dir().join(format!("clawcode-test-ws-changed-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&ws_dir);
    std::fs::write(ws_dir.join("AGENTS.md"), "v1 instructions").unwrap();

    let (db, writer, session_id) = setup_session_with_workspace(&db_path, &ws_dir);
    let bus = EventBus::new();
    let (_sub_id, rx) = bus.subscribe(None);
    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    client
        .start_generation(session_id, "plan", "fake", "echo", "prompt 1")
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    let reader_db = Db::open(&db_path).unwrap();
    let snapshot_v1 = reader_db
        .get_active_context_epoch(session_id)
        .unwrap()
        .unwrap()
        .source_snapshot_json;
    drop(reader_db);

    // Update AGENTS.md
    std::fs::write(ws_dir.join("AGENTS.md"), "v2 modified instructions").unwrap();

    client
        .start_generation(session_id, "plan", "fake", "echo", "prompt 2")
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    let db = client.shutdown();
    let active_epoch = db.get_active_context_epoch(session_id).unwrap().unwrap();
    assert_ne!(active_epoch.source_snapshot_json, snapshot_v1);
    assert!(active_epoch.source_snapshot_json.contains("AGENTS.md"));

    let messages = db.messages(session_id).unwrap();
    let system_msgs: Vec<_> = messages.iter().filter(|m| m.role == "system").collect();
    assert_eq!(
        system_msgs.len(),
        1,
        "changed source must emit exactly one atomic system message"
    );
    assert!(system_msgs[0].content.contains("v2 modified instructions"));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&ws_dir);
}

#[test]
fn initial_unavailable_instruction_source_blocks_promotion_leaves_prompt_pending() {
    let db_path = temp_db_path("epoch_unavailable");
    let ws_dir =
        std::env::temp_dir().join(format!("clawcode-test-ws-unavail-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&ws_dir);
    let agents_path = ws_dir.join("AGENTS.md");
    std::fs::write(&agents_path, "will be locked").unwrap();

    // Lock file exclusively to simulate read failure (PermissionDenied/SharingViolation)
    let lock = lock_file_exclusive(&agents_path);

    let (db, writer, session_id) = setup_session_with_workspace(&db_path, &ws_dir);
    let bus = EventBus::new();
    let (_sub_id, rx) = bus.subscribe(None);
    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    client
        .start_generation(session_id, "plan", "fake", "echo", "retryable prompt")
        .unwrap();
    let res = wait_for_finish_or_error(&rx);
    assert_eq!(res, "error");

    let reader_db = Db::open(&db_path).unwrap();
    // Prompt was admitted but NOT promoted!
    let inputs = reader_db.session_inputs(session_id).unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].status, InputStatus::Pending);
    // Messages must be empty: no user message was created!
    assert!(reader_db.messages(session_id).unwrap().is_empty());
    // Context epoch not created
    assert!(
        reader_db
            .get_active_context_epoch(session_id)
            .unwrap()
            .is_none()
    );
    // Session error was recorded
    let last_error = reader_db.session_last_error(session_id).unwrap();
    assert!(
        last_error
            .as_deref()
            .unwrap()
            .contains("instruction source unavailable")
    );
    drop(reader_db);

    // Now unlock the file
    unlock_file(lock, &agents_path);

    // Retry: start_generation will wake drain loop and find pending input
    client
        .start_generation(session_id, "plan", "fake", "echo", "retryable prompt")
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    let db = client.shutdown();
    let inputs = db.session_inputs(session_id).unwrap();
    assert_eq!(inputs[0].status, InputStatus::Promoted);
    assert_eq!(db.messages(session_id).unwrap().len(), 2); // user + assistant
    assert!(db.get_active_context_epoch(session_id).unwrap().is_some());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&ws_dir);
}

#[test]
fn subsequent_unavailable_instruction_source_keeps_prior_snapshot_and_emits_no_delta() {
    let db_path = temp_db_path("epoch_subsequent_unavail");
    let ws_dir =
        std::env::temp_dir().join(format!("clawcode-test-ws-subseq-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&ws_dir);
    let agents_path = ws_dir.join("AGENTS.md");
    std::fs::write(&agents_path, "stable v1 rules").unwrap();

    let (db, writer, session_id) = setup_session_with_workspace(&db_path, &ws_dir);
    let bus = EventBus::new();
    let (_sub_id, rx) = bus.subscribe(None);
    let client = RuntimeClient::spawn(db, writer, Box::new(EchoProvider), bus);

    // Turn 1 initializes epoch
    client
        .start_generation(session_id, "plan", "fake", "echo", "turn 1")
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    let reader_db = Db::open(&db_path).unwrap();
    let initial_snapshot = reader_db
        .get_active_context_epoch(session_id)
        .unwrap()
        .unwrap()
        .source_snapshot_json;
    drop(reader_db);

    // Lock file exclusively so subsequent read fails
    let lock = lock_file_exclusive(&agents_path);

    // Turn 2 runs with locked file
    client
        .start_generation(session_id, "plan", "fake", "echo", "turn 2")
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    unlock_file(lock, &agents_path);

    let db = client.shutdown();
    let active_epoch = db.get_active_context_epoch(session_id).unwrap().unwrap();
    assert_eq!(
        active_epoch.source_snapshot_json, initial_snapshot,
        "prior snapshot must not be erased on read failure"
    );

    let messages = db.messages(session_id).unwrap();
    let system_msgs: Vec<_> = messages.iter().filter(|m| m.role == "system").collect();
    assert!(
        system_msgs.is_empty(),
        "read failure must emit no delta message"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&ws_dir);
}

#[test]
fn instruction_source_parent_chain_traversal_ordering() {
    let db_path = temp_db_path("epoch_parent_chain");
    let parent_dir =
        std::env::temp_dir().join(format!("clawcode-test-parent-{}", std::process::id()));
    let child_dir = parent_dir.join("child");
    let _ = std::fs::create_dir_all(&child_dir);

    std::fs::write(parent_dir.join("AGENTS.md"), "PARENT LEVEL INSTRUCTIONS").unwrap();
    std::fs::write(child_dir.join("AGENTS.md"), "CHILD LEVEL INSTRUCTIONS").unwrap();

    let (db, writer, session_id) = setup_session_with_workspace(&db_path, &child_dir);
    let provider = CapturingProvider::new();
    let captured = provider.requests.clone();
    let bus = EventBus::new();
    let (_sub_id, rx) = bus.subscribe(None);
    let client = RuntimeClient::spawn(db, writer, Box::new(provider), bus);

    client
        .start_generation(session_id, "plan", "capturing", "model", "test prompt")
        .unwrap();
    assert_eq!(wait_for_finish_or_error(&rx), "generation_finished");

    let reqs = captured.lock().unwrap().clone();
    assert_eq!(reqs.len(), 1);
    let system_prompt = &reqs[0].messages[0].content;
    assert!(system_prompt.contains("CHILD LEVEL INSTRUCTIONS"));
    assert!(system_prompt.contains("PARENT LEVEL INSTRUCTIONS"));

    let child_pos = system_prompt.find("CHILD LEVEL INSTRUCTIONS").unwrap();
    let parent_pos = system_prompt.find("PARENT LEVEL INSTRUCTIONS").unwrap();
    assert!(
        child_pos < parent_pos,
        "workspace child instructions must precede parent instructions"
    );

    let _db = client.shutdown();
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&parent_dir);
}
