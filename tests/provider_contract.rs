use clawcode::provider::{
    FinishReason, ModelInfo, ProviderCapabilities, ProviderError, ProviderId, ProviderRegistry,
    ProviderStream, StreamEvent, StreamRequest, StreamResponse, ToolCallAssembler, Usage,
};
use std::sync::mpsc;
use std::time::Duration;

fn request() -> StreamRequest {
    StreamRequest::new("test-model", "hello", 256)
}

#[test]
fn event_variants_cover_stream_lifecycle() {
    let events = [
        StreamEvent::TextDelta("hel".into()),
        StreamEvent::TextDelta("lo".into()),
        StreamEvent::ToolCallStart {
            id: "t1".into(),
            name: "read_file".into(),
        },
        StreamEvent::ToolCallDelta {
            id: "t1".into(),
            arguments: "{}".into(),
        },
        StreamEvent::ToolCallEnd { id: "t1".into() },
        StreamEvent::Usage(Usage {
            input_tokens: 3,
            output_tokens: 2,
        }),
        StreamEvent::Finish {
            reason: FinishReason::Stop,
        },
    ];
    let mut text = String::new();
    for event in &events {
        match event {
            StreamEvent::TextDelta(delta) => text.push_str(delta),
            StreamEvent::ToolCallStart { name, .. } => text.push_str(name),
            _ => {}
        }
    }
    assert_eq!(text, "helloread_file");
}

#[test]
fn assembler_accumulates_arguments_and_enforces_cap() {
    let mut assembler = ToolCallAssembler::new(16);
    assembler.push("t1", r#"{"path":"a"}"#).unwrap();
    assert!(assembler.push("t1", ",\"extra\":").is_err());
    assert_eq!(assembler.finish("t1").as_deref(), Some("{\"path\":\"a\"}"));
}

#[test]
fn assembler_completes_valid_json() {
    let mut assembler = ToolCallAssembler::new(1024);
    assembler.push("t1", "{\"path").unwrap();
    assembler.push("t1", "\":\"src\"}").unwrap();
    assert_eq!(assembler.finish("t1").unwrap(), "{\"path\":\"src\"}");
}

#[test]
fn cancellation_is_terminal_and_priority() {
    let (sender, receiver) = mpsc::sync_channel::<StreamEvent>(8);
    sender
        .send(StreamEvent::TextDelta("partial".into()))
        .unwrap();
    sender.send(StreamEvent::Cancelled).unwrap();
    drop(sender);
    let mut cancelled = false;
    for event in receiver {
        if cancelled {
            assert!(!matches!(event, StreamEvent::TextDelta(_)));
        }
        cancelled |= matches!(event, StreamEvent::Cancelled);
    }
    assert!(cancelled);
}

#[test]
fn registry_resolves_by_id_and_lists_models() {
    let mut registry = ProviderRegistry::new();
    registry.register(FakeProvider::new(ProviderId::new("fake")));
    let provider = registry.get(&ProviderId::new("fake")).unwrap();
    assert_eq!(provider.id().as_str(), "fake");
    assert_eq!(registry.models().len(), 1);
    assert!(registry.get(&ProviderId::new("missing")).is_none());
}

#[test]
fn fake_provider_streams_and_finishes_with_usage() {
    let mut registry = ProviderRegistry::new();
    registry.register(FakeProvider::new(ProviderId::new("fake")));
    let response = registry
        .get(&ProviderId::new("fake"))
        .unwrap()
        .send(&request())
        .unwrap();
    let text = response
        .events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta(delta) => Some(delta.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, "hello");
    assert!(matches!(
        response.events.last(),
        Some(StreamEvent::Finish { .. })
    ));
}

#[test]
fn capabilities_flag_streaming_and_tools() {
    let capabilities = ProviderCapabilities {
        streaming: true,
        tools: false,
    };
    assert!(capabilities.streaming);
    assert!(!capabilities.tools);
}

#[test]
fn provider_error_carries_category_and_message() {
    let error = ProviderError::RateLimited {
        retry_after: Duration::from_secs(2),
    };
    assert!(error.to_string().contains("rate limited"));
}

#[test]
fn bounded_stream_coalesces_deltas_and_emits_terminal_cancel() {
    let (sender, mut stream) = ProviderStream::channel(1);
    sender.send(StreamEvent::TextDelta("a".into())).unwrap();
    sender.send(StreamEvent::TextDelta("b".into())).unwrap();
    sender.flush().unwrap();
    assert_eq!(stream.next(), Some(StreamEvent::TextDelta("ab".into())));
    sender
        .send(StreamEvent::Usage(Usage {
            input_tokens: 1,
            output_tokens: 2,
        }))
        .unwrap();
    assert_eq!(
        stream.next(),
        Some(StreamEvent::Usage(Usage {
            input_tokens: 1,
            output_tokens: 2,
        }))
    );
    stream.cancel();
    assert_eq!(stream.next(), Some(StreamEvent::Cancelled));
    assert_eq!(stream.next(), None);
}

#[derive(Debug)]
struct FakeProvider {
    id: ProviderId,
}

impl FakeProvider {
    fn new(id: ProviderId) -> Self {
        Self { id }
    }
}

impl clawcode::provider::Provider for FakeProvider {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
        }
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: "test-model".into(),
            context_window: 8_192,
        }]
    }

    fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        Ok(StreamResponse {
            events: vec![
                StreamEvent::TextDelta(request.prompt.clone()),
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
