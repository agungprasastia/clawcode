use crate::persistence::WriterHandle;
use crate::provider::TurnMetrics;
use crate::provider::{
    FinishReason, Provider, ProviderError, ProviderStream, StreamEvent, StreamRequest, Usage,
};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub mod tools;

pub const DEFAULT_TEXT_LIMIT: usize = 256 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationEvent {
    PromptSubmitted {
        prompt: String,
        provider: String,
        model: String,
    },
    TextDelta(String),
    Usage(Usage),
    Finished(FinishReason),
    Error(String),
    Cancelled,
    MutationRequested(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnState {
    output: String,
    usage: Option<Usage>,
    finish_reason: Option<FinishReason>,
    metrics: TurnMetrics,
}

impl TurnState {
    pub fn from_events(events: impl IntoIterator<Item = StreamEvent>, text_limit: usize) -> Self {
        let mut state = Self {
            output: String::new(),
            usage: None,
            finish_reason: None,
            metrics: TurnMetrics {
                ttft: None,
                duration: std::time::Duration::ZERO,
                usage: None,
                finish_reason: None,
                provider: String::new(),
                model: String::new(),
            },
        };
        for event in events {
            state.apply(event, text_limit);
        }
        state
    }
    fn apply(&mut self, event: StreamEvent, text_limit: usize) {
        match event {
            StreamEvent::TextDelta(delta) => {
                let remaining = text_limit.saturating_sub(self.output.len());
                let end = delta
                    .char_indices()
                    .map(|(index, _)| index)
                    .chain(std::iter::once(delta.len()))
                    .take_while(|&index| index <= remaining)
                    .last()
                    .unwrap_or(0);
                self.output.push_str(&delta[..end]);
            }
            StreamEvent::Usage(usage) => self.usage = Some(usage),
            StreamEvent::Finish { reason } => self.finish_reason = Some(reason),
            _ => {}
        }
    }
    pub fn assistant_output(&self) -> &str {
        &self.output
    }
    pub fn usage(&self) -> Option<Usage> {
        self.usage
    }
    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish_reason
    }
    pub fn metrics(&self) -> &TurnMetrics {
        &self.metrics
    }
}

#[derive(Debug)]
pub struct ConversationRuntime {
    provider: Option<Box<dyn Provider>>,
    stream: Arc<Mutex<Option<ProviderStream>>>,
    text_limit: usize,
}

impl ConversationRuntime {
    pub fn new(provider: impl Provider + 'static) -> Self {
        Self {
            provider: Some(Box::new(provider)),
            stream: Arc::new(Mutex::new(None)),
            text_limit: DEFAULT_TEXT_LIMIT,
        }
    }
    pub fn from_stream(stream: ProviderStream) -> Self {
        Self::from_stream_with_text_limit(stream, DEFAULT_TEXT_LIMIT)
    }
    pub fn from_stream_with_text_limit(stream: ProviderStream, text_limit: usize) -> Self {
        Self {
            provider: None,
            stream: Arc::new(Mutex::new(Some(stream))),
            text_limit,
        }
    }
    pub fn run(&self, request: &StreamRequest) -> Result<TurnState, ProviderError> {
        let started = Instant::now();
        let provider = self
            .provider
            .as_ref()
            .expect("provider runtime has provider")
            .id()
            .to_string();
        let response = self
            .provider
            .as_ref()
            .expect("provider runtime has provider")
            .send(request)?;
        let mut state = TurnState::from_events(response.events, self.text_limit);
        state.metrics.provider = provider;
        state.metrics.model = request.model.clone();
        state.metrics.duration = started.elapsed();
        state.metrics.usage = state.usage;
        state.metrics.finish_reason = state.finish_reason;
        state.metrics.ttft = (!state.output.is_empty()).then_some(state.metrics.duration);
        Ok(state)
    }

    pub fn run_and_persist(
        &self,
        request: &StreamRequest,
        writer: &WriterHandle,
        session_id: i64,
    ) -> Result<TurnState, ProviderError> {
        let state = self.run(request)?;
        if state.finish_reason.is_some() {
            writer
                .try_append(session_id, "assistant", &state.output)
                .map_err(ProviderError::Protocol)?;
        }
        Ok(state)
    }
    pub fn events(&self, request: &StreamRequest) -> Vec<ConversationEvent> {
        match self
            .provider
            .as_ref()
            .expect("provider runtime has provider")
            .send(request)
        {
            Ok(response) => {
                let mut events = Vec::new();
                let mut collected_text = 0;
                for event in response.events {
                    match event {
                        StreamEvent::TextDelta(delta) => {
                            let remaining = self.text_limit.saturating_sub(collected_text);
                            let end = delta
                                .char_indices()
                                .map(|(index, _)| index)
                                .chain(std::iter::once(delta.len()))
                                .take_while(|&index| index <= remaining)
                                .last()
                                .unwrap_or(0);
                            collected_text += end;
                            if end > 0 {
                                events.push(ConversationEvent::TextDelta(delta[..end].to_owned()));
                            }
                        }
                        StreamEvent::Usage(usage) => events.push(ConversationEvent::Usage(usage)),
                        StreamEvent::Finish { reason } => {
                            events.push(ConversationEvent::Finished(reason))
                        }
                        StreamEvent::Cancelled => {
                            events.push(ConversationEvent::Cancelled);
                            break;
                        }
                        StreamEvent::Error(message) => {
                            events.push(ConversationEvent::Error(message))
                        }
                        _ => {
                            events.push(ConversationEvent::Error("unsupported stream event".into()))
                        }
                    }
                }
                events
            }
            Err(ProviderError::Cancelled) => vec![ConversationEvent::Cancelled],
            Err(error) => vec![ConversationEvent::Error(error.to_string())],
        }
    }
    pub fn cancel(&self) {
        if let Some(stream) = self
            .stream
            .lock()
            .expect("conversation stream poisoned")
            .as_ref()
        {
            stream.cancel();
        }
    }
    pub fn collect_events(&self) -> Vec<ConversationEvent> {
        let mut stream = self.stream.lock().expect("conversation stream poisoned");
        let Some(stream) = stream.as_mut() else {
            return Vec::new();
        };
        let mut collected_text = 0;
        stream
            .map(|event| match event {
                StreamEvent::TextDelta(delta) => {
                    let remaining = self.text_limit.saturating_sub(collected_text);
                    let end = delta
                        .char_indices()
                        .map(|(index, _)| index)
                        .chain(std::iter::once(delta.len()))
                        .take_while(|&index| index <= remaining)
                        .last()
                        .unwrap_or(0);
                    collected_text += end;
                    ConversationEvent::TextDelta(delta[..end].to_owned())
                }
                StreamEvent::Usage(usage) => ConversationEvent::Usage(usage),
                StreamEvent::Finish { reason } => ConversationEvent::Finished(reason),
                StreamEvent::Error(message) => ConversationEvent::Error(message),
                StreamEvent::Cancelled => ConversationEvent::Cancelled,
                _ => ConversationEvent::Error("unsupported stream event".into()),
            })
            .filter(
                |event| !matches!(event, ConversationEvent::TextDelta(delta) if delta.is_empty()),
            )
            .collect()
    }
}
