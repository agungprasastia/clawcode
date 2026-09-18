use std::time::Instant;

use super::ConversationStatus;

/// Manages smooth typewriter delivery of streamed provider tokens
/// and tracks real-time generation speed (tokens per second).
#[derive(Debug, Clone)]
pub struct TypewriterState {
    /// Text waiting to be printed into the transcript.
    queue: String,
    /// Pending status to transition to once queue is completely drained.
    pending_status: Option<ConversationStatus>,
    /// Instant when the current response stream started.
    stream_start: Option<Instant>,
    /// Accumulated token/chunk estimate for tokens-per-second metric.
    token_count: usize,
    /// Whether typewriter pacing is enabled (can be toggled for tests/config).
    enabled: bool,
}

impl Default for TypewriterState {
    fn default() -> Self {
        Self::new()
    }
}

impl TypewriterState {
    const MAX_QUEUE_BYTES: usize = 64 * 1024;
    pub fn new() -> Self {
        Self {
            queue: String::new(),
            pending_status: None,
            stream_start: None,
            token_count: 0,
            enabled: true,
        }
    }

    /// Reset stream timers and buffer for a new generation turn.
    pub fn start_stream(&mut self) {
        self.queue.clear();
        self.pending_status = None;
        self.stream_start = Some(Instant::now());
        self.token_count = 0;
    }

    pub fn push_delta(&mut self, delta: &str) {
        if self.stream_start.is_none() {
            self.stream_start = Some(Instant::now());
        }
        let remaining = Self::MAX_QUEUE_BYTES.saturating_sub(self.queue.len());
        let mut end = delta.len().min(remaining);
        while end > 0 && !delta.is_char_boundary(end) {
            end -= 1;
        }
        let accepted = &delta[..end];
        let est_tokens = accepted.len().div_ceil(4);
        self.token_count += est_tokens.max(1);
        self.queue.push_str(accepted);
    }

    /// Whether there are pending characters in the queue.
    pub fn is_typing(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Whether the typewriter is active (has text or pending status).
    pub fn is_active(&self) -> bool {
        !self.queue.is_empty() || self.pending_status.is_some()
    }

    /// Queue length in characters.
    pub fn char_count(&self) -> usize {
        self.queue.chars().count()
    }

    /// Set a status to apply once the queue is fully drained.
    pub fn set_pending_status(&mut self, status: ConversationStatus) {
        self.pending_status = Some(status);
    }

    /// Inspect pending status if any.
    pub fn pending_status(&self) -> Option<ConversationStatus> {
        self.pending_status
    }

    /// Take the pending status.
    pub fn take_pending_status(&mut self) -> Option<ConversationStatus> {
        self.pending_status.take()
    }

    /// Set whether typewriter pacing is enabled.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Drain a batch of characters based on adaptive pacing.
    /// Returns the drained string slice if any characters were drained.
    pub fn drain_step(&mut self) -> Option<String> {
        if self.queue.is_empty() {
            return None;
        }

        if !self.enabled {
            let all = std::mem::take(&mut self.queue);
            return Some(all);
        }

        let remaining = self.queue.chars().count();
        let count = if self.pending_status.is_some() {
            // Generation finished on server; pace smoothly out without stalling
            if remaining <= 40 {
                2
            } else if remaining <= 120 {
                4
            } else if remaining <= 300 {
                8
            } else {
                (remaining / 10).max(12)
            }
        } else {
            // Actively streaming from provider
            if remaining <= 20 {
                2
            } else if remaining <= 60 {
                4
            } else {
                (remaining / 8).max(6)
            }
        };

        let mut byte_idx = 0;
        for (char_count, (idx, ch)) in self.queue.char_indices().enumerate() {
            if char_count >= count {
                break;
            }
            byte_idx = idx + ch.len_utf8();
        }

        if byte_idx > 0 {
            let drained: String = self.queue.drain(..byte_idx).collect();
            Some(drained)
        } else {
            None
        }
    }

    /// Immediately drain all pending characters.
    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.queue)
    }

    /// Estimated tokens per second since stream start.
    pub fn tokens_per_second(&self) -> Option<f64> {
        if let Some(start) = self.stream_start {
            let elapsed = start.elapsed().as_secs_f64();
            if elapsed > 0.05 && self.token_count > 0 {
                return Some((self.token_count as f64) / elapsed);
            }
        }
        None
    }

    /// Elapsed seconds since stream start.
    pub fn elapsed_seconds(&self) -> Option<f64> {
        self.stream_start.map(|s| s.elapsed().as_secs_f64())
    }

    /// Reset everything.
    pub fn reset(&mut self) {
        self.queue.clear();
        self.pending_status = None;
        self.stream_start = None;
        self.token_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typewriter_pacing_and_multibyte() {
        let mut state = TypewriterState::new();
        state.start_stream();
        assert!(!state.is_typing());

        // Multibyte string: "Halo 🦀 dunia"
        state.push_delta("Halo 🦀 dunia");
        assert!(state.is_typing());
        assert_eq!(state.char_count(), 12);

        // First step drains 2 chars ("Ha")
        let first = state.drain_step();
        assert_eq!(first, Some("Ha".to_string()));
        assert_eq!(state.char_count(), 10);

        // Flush drains remaining without breaking UTF-8
        let rest = state.flush();
        assert_eq!(rest, "lo 🦀 dunia");
        assert!(!state.is_typing());
    }

    #[test]
    fn test_typewriter_pending_status() {
        let mut state = TypewriterState::new();
        state.push_delta("abc");
        state.set_pending_status(ConversationStatus::Idle);
        assert_eq!(state.pending_status(), Some(ConversationStatus::Idle));
        assert_eq!(state.take_pending_status(), Some(ConversationStatus::Idle));
        assert_eq!(state.pending_status(), None);
    }
}
