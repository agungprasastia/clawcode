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
