//! Protocol adapters. Wire parsing stays here; provider core sees normalized events.

use crate::provider::{
    FinishReason, ModelInfo, ProviderCapabilities, ProviderError, ProviderId, StreamEvent,
    StreamRequest, StreamResponse,
};
use serde_json::Value;

pub mod anthropic;
pub mod ollama;
pub mod openai_compatible;
pub trait Transport: std::fmt::Debug + Send + Sync {
    fn request(&self, endpoint: &str, body: &str) -> Result<String, ProviderError>;
}

#[derive(Debug)]
pub struct MockTransport(pub String);
impl Transport for MockTransport {
    fn request(&self, _endpoint: &str, _body: &str) -> Result<String, ProviderError> {
        Ok(self.0.clone())
    }
}

#[derive(Debug)]
pub struct JsonProvider<T> {
    id: ProviderId,
    endpoint: String,
    capabilities: ProviderCapabilities,
    models: Vec<ModelInfo>,
    transport: T,
}

pub type OpenAiCompatible<T> = JsonProvider<T>;
pub type Anthropic<T> = JsonProvider<T>;
pub type Ollama<T> = JsonProvider<T>;

impl<T: Transport + 'static> JsonProvider<T> {
    pub fn new(id: impl Into<String>, endpoint: impl Into<String>, transport: T) -> Self {
        Self {
            id: ProviderId::new(id),
            endpoint: endpoint.into(),
            capabilities: ProviderCapabilities {
                streaming: true,
                tools: false,
            },
            models: Vec::new(),
            transport,
        }
    }
    pub(crate) fn request_body(&self, request: &StreamRequest) -> Result<String, ProviderError> {
        let body = serde_json::json!({"model": request.model, "prompt": request.prompt, "max_tokens": request.max_output_tokens}).to_string();
        self.transport.request(&self.endpoint, &body)
    }

    pub fn with_models(mut self, models: Vec<ModelInfo>) -> Self {
        self.models = models;
        self
    }
    pub fn with_capabilities(mut self, capabilities: ProviderCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
    pub fn parse_openai(body: &str) -> Result<StreamResponse, ProviderError> {
        parse_lines(body, openai_event)
    }
    pub fn parse_anthropic(body: &str) -> Result<StreamResponse, ProviderError> {
        parse_lines(body, anthropic_event)
    }
    pub fn parse_ollama(body: &str) -> Result<StreamResponse, ProviderError> {
        parse_lines(body, ollama_event)
    }
}

impl<T: Transport + 'static> crate::provider::Provider for JsonProvider<T> {
    fn id(&self) -> &ProviderId {
        &self.id
    }
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }
    fn models(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }
    fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        let response = self.request_body(request)?;
        match self.id.as_str() {
            "anthropic" => Self::parse_anthropic(&response),
            "ollama" => Self::parse_ollama(&response),
            _ => Self::parse_openai(&response),
        }
    }
}

fn parse_lines(
    body: &str,
    parser: fn(Value) -> Result<Option<StreamEvent>, ProviderError>,
) -> Result<StreamResponse, ProviderError> {
    let mut events = Vec::new();
    let mut usage = None;
    for line in body
        .lines()
        .filter(|line| !line.trim().is_empty() && *line != "data: [DONE]")
    {
        let json = line.strip_prefix("data:").unwrap_or(line).trim();
        let value: Value = serde_json::from_str(json)
            .map_err(|error| ProviderError::Protocol(error.to_string()))?;
        usage = usage.or_else(|| value.get("usage").and_then(parse_usage));
        if let Some(event) = parser(value)? {
            let event = match event {
                StreamEvent::Finish { reason } => {
                    if let Some(value) = usage {
                        events.push(StreamEvent::Usage(value));
                    }
                    StreamEvent::Finish { reason }
                }
                other => other,
            };
            events.push(event);
        }
    }
    Ok(StreamResponse { events })
}

fn parse_usage(value: &Value) -> Option<crate::provider::Usage> {
    Some(crate::provider::Usage {
        input_tokens: value
            .get("prompt_tokens")
            .or_else(|| value.get("input_tokens"))
            .or_else(|| value.get("prompt_eval_count"))
            .and_then(Value::as_u64)?,
        output_tokens: value
            .get("completion_tokens")
            .or_else(|| value.get("output_tokens"))
            .or_else(|| value.get("eval_count"))
            .and_then(Value::as_u64)?,
    })
}

fn openai_event(value: Value) -> Result<Option<StreamEvent>, ProviderError> {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(ProviderError::Protocol(error.into()));
    }
    let text = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .map(|s| StreamEvent::TextDelta(s.into()));
    let finish = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .map(finish_event);
    Ok(text.or(finish))
}

fn anthropic_event(value: Value) -> Result<Option<StreamEvent>, ProviderError> {
    if value.get("type").and_then(Value::as_str) == Some("error") {
        return Err(ProviderError::Protocol(value.to_string()));
    }
    Ok(value
        .pointer("/delta/text")
        .and_then(Value::as_str)
        .map(|s| StreamEvent::TextDelta(s.into()))
        .or_else(|| {
            value
                .get("stop_reason")
                .and_then(Value::as_str)
                .map(finish_event)
        }))
}

fn ollama_event(value: Value) -> Result<Option<StreamEvent>, ProviderError> {
    if value.get("error").is_some() {
        return Err(ProviderError::Protocol(value.to_string()));
    }
    Ok(value
        .get("response")
        .and_then(Value::as_str)
        .map(|s| StreamEvent::TextDelta(s.into()))
        .or_else(|| {
            value
                .get("done_reason")
                .and_then(Value::as_str)
                .map(finish_event)
        }))
}

fn finish_event(reason: &str) -> StreamEvent {
    StreamEvent::Finish {
        reason: if reason == "length" {
            FinishReason::Length
        } else {
            FinishReason::Stop
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_sse_text_usage_and_finish() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\ndata: {\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3},\"choices\":[{\"finish_reason\":\"stop\"}]}\ndata: [DONE]";
        let response = JsonProvider::<MockTransport>::parse_openai(body).unwrap();
        assert_eq!(response.text(), "hi");
        assert_eq!(response.usage().unwrap().output_tokens, 3);
    }

    #[test]
    fn parses_anthropic_and_ollama_text() {
        assert_eq!(
            JsonProvider::<MockTransport>::parse_anthropic("data: {\"delta\":{\"text\":\"a\"}}")
                .unwrap()
                .text(),
            "a"
        );
        assert_eq!(
            JsonProvider::<MockTransport>::parse_ollama("{\"response\":\"b\"}")
                .unwrap()
                .text(),
            "b"
        );
    }
}
