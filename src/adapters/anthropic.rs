//! Native Anthropic adapter surface.

use super::JsonProvider;
use crate::provider::{
    ModelInfo, Provider, ProviderCapabilities, ProviderError, ProviderId, StreamRequest,
    StreamResponse,
};

#[derive(Debug)]
pub struct Anthropic<T>(JsonProvider<T>);

impl<T: super::Transport + Clone + 'static> Anthropic<T> {
    pub fn new(id: impl Into<String>, endpoint: impl Into<String>, transport: T) -> Self {
        Self(JsonProvider::new(id, endpoint, transport))
    }
}

impl<T: super::Transport + Clone + 'static> Provider for Anthropic<T> {
    fn id(&self) -> &ProviderId {
        self.0.id()
    }
    fn capabilities(&self) -> ProviderCapabilities {
        self.0.capabilities()
    }
    fn models(&self) -> Vec<ModelInfo> {
        self.0.models()
    }
    fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        self.0.send(request)
    }
    fn stream(&self, request: &StreamRequest) -> Result<crate::provider::ProviderStream, ProviderError> {
        self.0.stream(request)
    }
}

pub use super::{MockTransport, Transport};
