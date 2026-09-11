//! Per-provider counters and bounded per-turn timing data.

use super::events::{FinishReason, Usage};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnMetrics {
    pub ttft: Option<Duration>,
    pub duration: Duration,
    pub usage: Option<Usage>,
    pub finish_reason: Option<FinishReason>,
    pub provider: String,
    pub model: String,
}

impl TurnMetrics {
    pub fn ttft(&self) -> Option<Duration> {
        self.ttft
    }
    pub fn duration(&self) -> Option<Duration> {
        Some(self.duration)
    }
    pub fn usage(&self) -> Option<Usage> {
        self.usage
    }
    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish_reason
    }
    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn model(&self) -> &str {
        &self.model
    }
}

/// Counters accumulated across requests. All fields saturate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProviderMetrics {
    pub requests: u64,
    pub failures: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl ProviderMetrics {
    pub fn record_success(&mut self, usage: super::events::Usage) {
        self.requests = self.requests.saturating_add(1);
        self.input_tokens = self.input_tokens.saturating_add(usage.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(usage.output_tokens);
    }

    pub fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::events::Usage;

    #[test]
    fn counters_saturate() {
        let mut metrics = ProviderMetrics {
            requests: u64::MAX,
            input_tokens: u64::MAX,
            ..ProviderMetrics::default()
        };
        metrics.record_success(Usage {
            input_tokens: 1,
            output_tokens: 1,
        });
        assert_eq!(metrics.requests, u64::MAX);
        assert_eq!(metrics.input_tokens, u64::MAX);
    }
}
