//! Provider trait and request/response types. Adapters implement `Provider`;
//! core depends only on this trait plus the normalized event vocabulary.

use super::events::{FinishReason, StreamEvent, Usage};
use std::fmt;
use std::time::Duration;

/// A streaming completion request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamRequest {
    pub model: String,
    pub prompt: String,
    pub max_output_tokens: u32,
}

/// The full event stream for one request, in order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamResponse {
    pub events: Vec<StreamEvent>,
}

impl StreamResponse {
    /// Concatenated text deltas, if any.
    pub fn text(&self) -> String {
        self.events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::TextDelta(delta) => Some(delta.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Final usage, if the stream finished normally.
    pub fn usage(&self) -> Option<Usage> {
        self.events.iter().find_map(|event| match event {
            StreamEvent::Usage(usage) => Some(*usage),
            _ => None,
        })
    }

    /// Finish reason, if the stream finished.
    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.events.iter().find_map(|event| match event {
            StreamEvent::Finish { reason } => Some(*reason),
            _ => None,
        })
    }
}

/// What a provider supports. Adapters declare; core gates features.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderCapabilities {
    pub streaming: bool,
    pub tools: bool,
}

/// Errors surfaced by a provider. `RetryAfter` hints backoff scheduling.
#[derive(Debug)]
pub enum ProviderError {
    RateLimited { retry_after: Duration },
    Auth,
    Network(String),
    Protocol(String),
    Cancelled,
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RateLimited { retry_after } => {
                write!(f, "rate limited; retry after {retry_after:?}")
            }
            Self::Auth => write!(f, "authentication failed"),
            Self::Network(message) => write!(f, "network error: {message}"),
            Self::Protocol(message) => write!(f, "protocol error: {message}"),
            Self::Cancelled => write!(f, "stream cancelled"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// A chat provider. `send` returns the normalized event stream for one
/// request; cancellation is expressed as a terminal `StreamEvent::Cancelled`.
pub trait Provider: fmt::Debug {
    fn id(&self) -> &super::registry::ProviderId;
    fn capabilities(&self) -> ProviderCapabilities;
    fn models(&self) -> Vec<super::registry::ModelInfo>;
    fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError>;
}
