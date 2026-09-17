//! End-to-end wiring: prompt submission flows through `RuntimeClient` into a
//! generation thread, and `App::poll_runtime` drains bus events into the
//! transcript view. Provider is a fake; no network involved.

use clawcode::persistence::{Db, WriterHandle};
use clawcode::provider::{
    FinishReason, ModelInfo, Provider, ProviderCapabilities, ProviderError, ProviderId,
    StreamEvent, StreamRequest, StreamResponse, Usage,
};
use clawcode::tui::{App, Input, UiEvent};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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

#[test]
fn recovery_empty_turn_loop_recovers_after_empty_response() {
    use std::sync::atomic::AtomicUsize;

    #[derive(Debug)]
    struct ToolThenEmptyThenSummaryProvider {
        turn: AtomicUsize,
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

    let path = temp_db_path("empty_turn_recovery");
    let mut app = App::default();
    let db = Db::open(&path).expect("runtime db");
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    app.attach_runtime(
        db,
        writer,
        Box::new(ToolThenEmptyThenSummaryProvider {
            turn: AtomicUsize::new(0),
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

    assert!(
        app.transcript().contains("Here is the recovered summary."),
        "transcript should contain recovered summary, got: {}",
        app.transcript()
    );
    assert!(matches!(
        app.conversation_status(),
        clawcode::tui::ConversationStatus::Finished(_)
    ));

    let _ = std::fs::remove_file(&path);
}
