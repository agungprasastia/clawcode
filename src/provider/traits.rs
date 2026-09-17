//! Provider trait and request/response types. Adapters implement `Provider`;
//! core depends only on this trait plus the normalized event vocabulary.

use super::events::{FinishReason, StreamEvent, Usage};
use std::fmt;
use std::time::Duration;

/// A single message in a chat conversation.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
            name: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// A streaming completion request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamRequest {
    pub model: String,
    pub prompt: String,
    pub max_output_tokens: u32,
    pub messages: Vec<ChatMessage>,
    pub provider: Option<String>,
    pub tools: Vec<serde_json::Value>,
}

impl StreamRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>, max_output_tokens: u32) -> Self {
        let p = prompt.into();
        Self {
            model: model.into(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: p.clone(),
                tool_call_id: None,
                tool_calls: None,
                name: None,
            }],
            prompt: p,
            max_output_tokens,
            provider: None,
            tools: Vec::new(),
        }
    }

    pub fn with_messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    pub fn with_tools(mut self, tools: Vec<serde_json::Value>) -> Self {
        self.tools = tools;
        self
    }
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

    fn stream(&self, request: &StreamRequest) -> Result<super::stream::ProviderStream, ProviderError> {
        let response = self.send(request)?;
        let (sender, stream) = super::stream::ProviderStream::channel(response.events.len().max(1));
        for event in response.events {
            let _ = sender.send(event);
        }
        let _ = sender.flush();
        Ok(stream)
    }
}
