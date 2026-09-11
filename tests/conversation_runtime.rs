use clawcode::conversation::{ConversationEvent, ConversationRuntime, TurnState};
use clawcode::provider::{
    FinishReason, ModelInfo, Provider, ProviderCapabilities, ProviderError, ProviderId,
    ProviderStream, StreamEvent, StreamRequest, StreamResponse, Usage,
};

#[derive(Debug)]
struct FakeProvider {
    result: Result<StreamResponse, ProviderError>,
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
        self.result
            .as_ref()
            .map(|response| response.clone())
            .map_err(|error| match error {
                ProviderError::Network(message) => ProviderError::Network(message.clone()),
                ProviderError::Auth => ProviderError::Auth,
                ProviderError::Protocol(message) => ProviderError::Protocol(message.clone()),
                ProviderError::Cancelled => ProviderError::Cancelled,
                ProviderError::RateLimited { retry_after } => ProviderError::RateLimited {
                    retry_after: *retry_after,
                },
            })
    }
}

fn request() -> StreamRequest {
    StreamRequest {
        model: "test".into(),
        prompt: "hello".into(),
        max_output_tokens: 16,
    }
}

#[test]
fn assembles_text_and_propagates_usage_and_finish() {
    let runtime = ConversationRuntime::new(FakeProvider {
        result: Ok(StreamResponse {
            events: vec![
                StreamEvent::TextDelta("hel".into()),
                StreamEvent::TextDelta("lo".into()),
                StreamEvent::Usage(Usage {
                    input_tokens: 2,
                    output_tokens: 1,
                }),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        }),
    });
    let turn = runtime.run(&request()).unwrap();
    assert_eq!(turn.assistant_output(), "hello");
    assert_eq!(
        turn.usage(),
        Some(Usage {
            input_tokens: 2,
            output_tokens: 1
        })
    );
    assert_eq!(turn.finish_reason(), Some(FinishReason::Stop));
}

#[test]
fn provider_error_is_terminal_event() {
    let runtime = ConversationRuntime::new(FakeProvider {
        result: Err(ProviderError::Network("offline".into())),
    });
    let events = runtime.events(&request());
    assert!(
        matches!(events.as_slice(), [ConversationEvent::Error(message)] if message == "network error: offline")
    );
}

#[test]
fn stream_cancellation_emits_only_terminal_cancelled() {
    let (sender, stream) = ProviderStream::channel(2);
    sender
        .send(StreamEvent::TextDelta("partial".into()))
        .unwrap();
    let runtime = ConversationRuntime::from_stream(stream);
    runtime.cancel();
    let events = runtime.collect_events();
    assert!(matches!(events.as_slice(), [ConversationEvent::Cancelled]));
}

#[test]
fn event_queue_is_bounded_by_text_limit() {
    let turn = TurnState::from_events(
        [
            StreamEvent::TextDelta("12345".into()),
            StreamEvent::TextDelta("678".into()),
        ],
        5,
    );
    assert_eq!(turn.assistant_output(), "12345");
}

#[test]
fn text_limit_inside_multibyte_character_does_not_panic_or_split_utf8() {
    let turn = TurnState::from_events([StreamEvent::TextDelta("aé".into())], 2);

    assert_eq!(turn.assistant_output(), "a");
}

#[test]
fn collected_stream_text_respects_configured_utf8_safe_limit() {
    let (sender, stream) = ProviderStream::channel(4);
    sender.send(StreamEvent::TextDelta("aé漢".into())).unwrap();
    sender
        .send(StreamEvent::Finish {
            reason: FinishReason::Stop,
        })
        .unwrap();
    drop(sender);
    let runtime = ConversationRuntime::from_stream_with_text_limit(stream, 2);

    let events = runtime.collect_events();

    assert_eq!(
        events,
        vec![
            ConversationEvent::TextDelta("a".into()),
            ConversationEvent::Finished(FinishReason::Stop),
        ]
    );
}
