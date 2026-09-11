//! Normalized provider contract: events, traits, registry, metrics. Core
//! imports only these types — never a concrete adapter protocol.

pub mod events;
pub mod metrics;
pub mod registry;
pub mod stream;
pub mod traits;

pub use events::{FinishReason, StreamEvent, ToolCallAssembler, Usage};
pub use metrics::ProviderMetrics;
pub use registry::{ModelInfo, ProviderId, ProviderRegistry};
pub use stream::{ProviderStream, StreamSender};
pub use traits::{Provider, ProviderCapabilities, ProviderError, StreamRequest, StreamResponse};
