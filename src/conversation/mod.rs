use crate::provider::{
    FinishReason, Provider, ProviderError, ProviderStream, StreamEvent, StreamRequest, Usage,
};
use std::sync::{Arc, Mutex};

pub const DEFAULT_TEXT_LIMIT: usize = 256 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationEvent {
    TextDelta(String),
    Usage(Usage),
    Finished(FinishReason),
    Error(String),
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnState {
    output: String,
    usage: Option<Usage>,
    finish_reason: Option<FinishReason>,
}

impl TurnState {
    pub fn from_events(events: impl IntoIterator<Item = StreamEvent>, text_limit: usize) -> Self {
        let mut state = Self {
            output: String::new(),
            usage: None,
            finish_reason: None,
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
        Self {
            provider: None,
            stream: Arc::new(Mutex::new(Some(stream))),
            text_limit: DEFAULT_TEXT_LIMIT,
        }
    }
    pub fn run(&self, request: &StreamRequest) -> Result<TurnState, ProviderError> {
        Ok(TurnState::from_events(
            self.provider
                .as_ref()
                .expect("provider runtime has provider")
                .send(request)?
                .events,
            self.text_limit,
        ))
    }
    pub fn events(&self, request: &StreamRequest) -> Vec<ConversationEvent> {
        match self.run(request) {
            Ok(turn) => {
                let mut events = Vec::new();
                if !turn.assistant_output().is_empty() {
                    events.push(ConversationEvent::TextDelta(
                        turn.assistant_output().to_owned(),
                    ));
                }
                if let Some(usage) = turn.usage() {
                    events.push(ConversationEvent::Usage(usage));
                }
                if let Some(reason) = turn.finish_reason() {
                    events.push(ConversationEvent::Finished(reason));
                }
                events
            }
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
        stream
            .map(|event| match event {
                StreamEvent::TextDelta(delta) => ConversationEvent::TextDelta(delta),
                StreamEvent::Usage(usage) => ConversationEvent::Usage(usage),
                StreamEvent::Finish { reason } => ConversationEvent::Finished(reason),
                StreamEvent::Error(message) => ConversationEvent::Error(message),
                StreamEvent::Cancelled => ConversationEvent::Cancelled,
                _ => ConversationEvent::Error("unsupported stream event".into()),
            })
            .collect()
    }
}
