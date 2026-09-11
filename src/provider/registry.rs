//! Provider registry: id-keyed lookup plus the model list union.

/// Stable provider identifier (e.g. "openai", "anthropic", "ollama").
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One model exposed by a provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub context_window: u64,
}

/// Id-keyed provider registry. Lookup is O(n) over a handful of providers.
#[derive(Debug, Default)]
pub struct ProviderRegistry {
    providers: Vec<Box<dyn super::traits::Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, provider: impl super::traits::Provider + 'static) {
        self.providers.push(Box::new(provider));
    }

    pub fn get(&self, id: &ProviderId) -> Option<&dyn super::traits::Provider> {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .map(|provider| &**provider)
    }

    /// Union of every provider's models, in registration order.
    pub fn models(&self) -> Vec<ModelInfo> {
        self.providers
            .iter()
            .flat_map(|provider| provider.models())
            .collect()
    }
}
