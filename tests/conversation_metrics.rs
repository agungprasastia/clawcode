use clawcode::conversation::{ConversationEvent, ConversationRuntime};
use clawcode::persistence::{Db, WriterHandle};
use clawcode::provider::{
    FinishReason, ModelInfo, Provider, ProviderCapabilities, ProviderError, ProviderId,
    StreamEvent, StreamRequest, StreamResponse, Usage,
};

#[derive(Debug)]
struct FakeProvider {
    response: StreamResponse,
}

impl Provider for FakeProvider {
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
        Ok(self.response.clone())
    }
}

fn request() -> StreamRequest {
    StreamRequest {
        model: "model-a".into(),
        prompt: "hello".into(),
        max_output_tokens: 16,
    }
}

#[test]
fn records_bounded_metrics_identity_usage_and_finish() {
    let runtime = ConversationRuntime::new(FakeProvider {
        response: StreamResponse {
            events: vec![
                StreamEvent::TextDelta("hello".into()),
                StreamEvent::Usage(Usage {
                    input_tokens: 2,
                    output_tokens: 1,
                }),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        },
    });
    let turn = runtime.run(&request()).unwrap();
    let metrics = turn.metrics().expect("successful turn metrics");
    assert!(metrics.duration().is_some());
    assert_eq!(
        metrics.usage(),
        Some(Usage {
            input_tokens: 2,
            output_tokens: 1
        })
    );
    assert_eq!(metrics.finish_reason(), Some(FinishReason::Stop));
    assert_eq!(metrics.provider(), "fake");
    assert_eq!(metrics.model(), "model-a");
}

#[test]
fn missing_usage_stays_absent() {
    let runtime = ConversationRuntime::new(FakeProvider {
        response: StreamResponse {
            events: vec![StreamEvent::Finish {
                reason: FinishReason::Length,
            }],
        },
    });
    assert_eq!(
        runtime
            .run(&request())
            .unwrap()
            .metrics()
            .expect("successful turn metrics")
            .usage(),
        None
    );
}

#[test]
fn failed_run_does_not_expose_success_metrics() {
    for events in [
        vec![StreamEvent::Error("boom".into())],
        vec![StreamEvent::Cancelled],
        vec![StreamEvent::TextDelta("partial".into())],
        vec![StreamEvent::Finish {
            reason: FinishReason::Error,
        }],
    ] {
        let runtime = ConversationRuntime::new(FakeProvider {
            response: StreamResponse { events },
        });
        assert_eq!(runtime.run(&request()).unwrap().metrics(), None);
    }
}

#[test]
fn persists_one_assembled_assistant_message_only_after_terminal_success() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("test").unwrap();
    let writer = WriterHandle::spawn(db);
    let runtime = ConversationRuntime::new(FakeProvider {
        response: StreamResponse {
            events: vec![
                StreamEvent::TextDelta("hello".into()),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        },
    });
    runtime
        .run_and_persist(&request(), &writer, session.id)
        .unwrap();
    let db = writer.shutdown();
    let messages = db.messages(session.id).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "assistant");
    assert_eq!(messages[0].content, "hello");
}

#[test]
fn does_not_persist_without_terminal_success() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("test").unwrap();
    let writer = WriterHandle::spawn(db);
    let runtime = ConversationRuntime::new(FakeProvider {
        response: StreamResponse {
            events: vec![
                StreamEvent::TextDelta("partial".into()),
                StreamEvent::Error("boom".into()),
            ],
        },
    });
    runtime
        .run_and_persist(&request(), &writer, session.id)
        .unwrap();
    let db = writer.shutdown();
    assert!(db.messages(session.id).unwrap().is_empty());
}

#[test]
fn does_not_persist_error_finish_or_cancellation() {
    for events in [
        vec![
            StreamEvent::TextDelta("partial".into()),
            StreamEvent::Finish {
                reason: FinishReason::Error,
            },
        ],
        vec![
            StreamEvent::TextDelta("partial".into()),
            StreamEvent::Cancelled,
        ],
    ] {
        let db = Db::open_in_memory().unwrap();
        let session = db.create_session("test").unwrap();
        let writer = WriterHandle::spawn(db);
        let runtime = ConversationRuntime::new(FakeProvider {
            response: StreamResponse { events },
        });
        runtime
            .run_and_persist(&request(), &writer, session.id)
            .unwrap();
        let db = writer.shutdown();
        assert!(db.messages(session.id).unwrap().is_empty());
    }
}

#[test]
fn does_not_persist_when_error_precedes_finish() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("test").unwrap();
    let writer = WriterHandle::spawn(db);
    let runtime = ConversationRuntime::new(FakeProvider {
        response: StreamResponse {
            events: vec![
                StreamEvent::TextDelta("partial".into()),
                StreamEvent::Error("boom".into()),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        },
    });
    runtime
        .run_and_persist(&request(), &writer, session.id)
        .unwrap();
    let db = writer.shutdown();
    assert!(db.messages(session.id).unwrap().is_empty());
}

#[test]
fn does_not_persist_when_cancellation_precedes_finish() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("test").unwrap();
    let writer = WriterHandle::spawn(db);
    let runtime = ConversationRuntime::new(FakeProvider {
        response: StreamResponse {
            events: vec![
                StreamEvent::TextDelta("partial".into()),
                StreamEvent::Cancelled,
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        },
    });
    runtime
        .run_and_persist(&request(), &writer, session.id)
        .unwrap();
    let db = writer.shutdown();
    assert!(db.messages(session.id).unwrap().is_empty());
}

#[test]
fn events_do_not_emit_metrics_after_stream_error() {
    let runtime = ConversationRuntime::new(FakeProvider {
        response: StreamResponse {
            events: vec![
                StreamEvent::TextDelta("partial".into()),
                StreamEvent::Error("boom".into()),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        },
    });

    let events = runtime.events(&request());

    assert!(
        events
            .iter()
            .any(|event| matches!(event, ConversationEvent::Error(_)))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ConversationEvent::Metrics(_)))
    );
}

#[test]
fn events_emit_metrics_only_after_successful_terminal_finish() {
    let runtime = ConversationRuntime::new(FakeProvider {
        response: StreamResponse {
            events: vec![
                StreamEvent::TextDelta("hello".into()),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        },
    });

    let events = runtime.events(&request());

    assert!(matches!(events.last(), Some(ConversationEvent::Metrics(_))));
}
