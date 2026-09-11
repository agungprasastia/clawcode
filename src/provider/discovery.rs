//! Non-blocking model discovery service. Cached models remain available while
//! a refresh runs or a provider is unavailable.

use super::{ModelInfo, ProviderError, ProviderId};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub trait DiscoverySource: Send + Sync + 'static {
    fn discover(&self, provider: &ProviderId) -> Result<Vec<ModelInfo>, ProviderError>;
}

#[derive(Debug)]
struct Entry {
    provider: ProviderId,
    models: Vec<ModelInfo>,
    refreshed_at: Instant,
    failures: u32,
    next_retry: Instant,
}

#[derive(Debug)]
pub struct DiscoveryService {
    entries: Vec<Entry>,
    ttl: Duration,
    base_backoff: Duration,
}

impl DiscoveryService {
    pub fn new(ttl: Duration, base_backoff: Duration) -> Self {
        Self {
            entries: Vec::new(),
            ttl,
            base_backoff,
        }
    }

    pub fn models(&self, provider: &ProviderId) -> Option<&[ModelInfo]> {
        self.entries
            .iter()
            .find(|entry| &entry.provider == provider)
            .map(|entry| entry.models.as_slice())
    }

    /// Handles `/models refresh`; caller polls returned receiver from its event loop.
    pub fn models_refresh<D: DiscoverySource>(
        &mut self,
        provider: ProviderId,
        source: D,
        timeout: Duration,
    ) -> mpsc::Receiver<Result<Vec<ModelInfo>, ProviderError>> {
        self.refresh(provider, source, timeout)
    }

    pub fn is_stale(&self, provider: &ProviderId) -> bool {
        self.entries
            .iter()
            .find(|entry| &entry.provider == provider)
            .is_none_or(|entry| entry.refreshed_at.elapsed() >= self.ttl)
    }

    pub fn refresh<D: DiscoverySource>(
        &mut self,
        provider: ProviderId,
        source: D,
        timeout: Duration,
    ) -> mpsc::Receiver<Result<Vec<ModelInfo>, ProviderError>> {
        let (sender, receiver) = mpsc::channel();
        let allowed = self
            .entries
            .iter()
            .find(|entry| entry.provider == provider)
            .is_none_or(|entry| Instant::now() >= entry.next_retry);
        if !allowed {
            return receiver;
        }
        thread::spawn(move || {
            let (worker_sender, worker_receiver) = mpsc::channel();
            let requested = provider.clone();
            thread::spawn(move || {
                let _ = worker_sender.send(source.discover(&requested));
            });
            let result = worker_receiver
                .recv_timeout(timeout)
                .unwrap_or_else(|_| Err(ProviderError::Network("model discovery timeout".into())));
            let _ = sender.send(result);
        });
        receiver
    }

    pub fn apply(&mut self, provider: ProviderId, result: Result<Vec<ModelInfo>, ProviderError>) {
        let now = Instant::now();
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.provider == provider);
        match (entry, result) {
            (Some(entry), Ok(models)) => {
                entry.models = models;
                entry.refreshed_at = now;
                entry.failures = 0;
                entry.next_retry = now;
            }
            (Some(entry), Err(_)) => {
                entry.failures = entry.failures.saturating_add(1);
                entry.next_retry = now + self.base_backoff.saturating_mul(entry.failures);
            }
            (None, Ok(models)) => self.entries.push(Entry {
                provider,
                models,
                refreshed_at: now,
                failures: 0,
                next_retry: now,
            }),
            (None, Err(_)) => self.entries.push(Entry {
                provider,
                models: Vec::new(),
                refreshed_at: now,
                failures: 1,
                next_retry: now + self.base_backoff,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Source(Result<Vec<ModelInfo>, ProviderError>);

    impl DiscoverySource for Source {
        fn discover(&self, _provider: &ProviderId) -> Result<Vec<ModelInfo>, ProviderError> {
            match &self.0 {
                Ok(models) => Ok(models.clone()),
                Err(ProviderError::Network(message)) => {
                    Err(ProviderError::Network(message.clone()))
                }
                Err(_) => Err(ProviderError::Protocol("mock failure".into())),
            }
        }
    }

    fn model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            context_window: 4096,
        }
    }

    #[test]
    fn stale_cache_survives_failed_refresh() {
        let provider = ProviderId::new("openai");
        let mut service = DiscoveryService::new(Duration::ZERO, Duration::from_secs(60));
        service.apply(provider.clone(), Ok(vec![model("cached")]));
        let receiver = service.refresh(
            provider.clone(),
            Source(Err(ProviderError::Auth)),
            Duration::from_secs(1),
        );
        let result = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        service.apply(provider.clone(), result);
        assert_eq!(service.models(&provider).unwrap()[0].id, "cached");
    }

    #[test]
    fn failed_provider_backoff_does_not_block_other_provider() {
        let first = ProviderId::new("openai");
        let second = ProviderId::new("ollama");
        let mut service = DiscoveryService::new(Duration::from_secs(60), Duration::from_secs(60));
        service.apply(first.clone(), Err(ProviderError::Auth));
        service.apply(second.clone(), Ok(vec![model("local")]));
        assert!(service.models(&first).unwrap().is_empty());
        assert_eq!(service.models(&second).unwrap()[0].id, "local");
    }
}
