//! Normalized provider contract: events, traits, registry, metrics. Core
//! imports only these types — never a concrete adapter protocol.

pub mod discovery;
pub mod events;
pub mod metrics;
pub mod registry;
pub mod stream;
pub mod traits;

pub use discovery::{DiscoveryService, DiscoverySource};
pub use events::{FinishReason, StreamEvent, ToolCallAssembler, Usage};
pub use metrics::{ProviderMetrics, TurnMetrics};
pub use registry::{ModelInfo, ProviderId, ProviderRegistry};
pub use stream::{MAX_COALESCED_DELTA_BYTES, ProviderStream, StreamSender};
pub use traits::{ChatMessage, Provider, ProviderCapabilities, ProviderError, StreamRequest, StreamResponse};
