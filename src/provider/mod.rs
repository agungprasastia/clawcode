//! Normalized provider contract: events, traits, registry, metrics. Core
//! imports only these types, never a concrete adapter protocol.

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
pub use traits::{
    ChatMessage, Provider, ProviderCapabilities, ProviderError, StreamRequest, StreamResponse,
    ToolCall,
};

/// Resolve the selectable reasoning/effort variants for `(provider, model)`.
/// Always starts with `"default"` so cycling can return to the unmodified base behavior.
pub fn available_variants_for_model(
    provider: &str,
    model: &str,
    config: Option<&crate::config::Config>,
) -> Vec<String> {
    let provider_trimmed = provider.trim();
    let stripped_model = model
        .trim()
        .strip_prefix(&format!("{provider_trimmed}/"))
        .unwrap_or_else(|| model.trim());

    if let Some(cfg) = config
        && let Some(provider_cfg) = cfg.providers.get(provider_trimmed)
        && let Some(model_cfg) = provider_cfg
            .models
            .get(stripped_model)
            .or_else(|| provider_cfg.models.get(model.trim()))
        && !model_cfg.variants.is_empty()
    {
        let mut out = vec!["default".to_string()];
        for entry in &model_cfg.variants {
            let trimmed = entry.trim();
            if !trimmed.is_empty()
                && !trimmed.eq_ignore_ascii_case("default")
                && !out.iter().any(|existing| existing == trimmed)
            {
                out.push(trimmed.to_string());
            }
        }
        return out;
    }

    let lower = stripped_model
        .rsplit('/')
        .next()
        .unwrap_or(stripped_model)
        .to_ascii_lowercase();
    let is_reasoning = lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.starts_with("gpt-5")
        || lower.starts_with("codex")
        || lower.starts_with("claude-3-7")
        || lower.starts_with("claude-sonnet-4")
        || lower.starts_with("claude-opus-4")
        || lower.starts_with("claude-4")
        || lower.starts_with("gemini-2.5")
        || lower.starts_with("gemini-3")
        || lower.starts_with("deepseek-r1")
        || lower.starts_with("qwq")
        || lower.contains("thinking")
        || lower.contains("reasoner");

    if is_reasoning {
        vec![
            "default".to_string(),
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
        ]
    } else {
        vec!["default".to_string()]
    }
}
