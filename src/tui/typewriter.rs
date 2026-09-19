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
    pub const MAX_QUEUE_BYTES: usize = 256 * 1024;
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

    /// Push incoming stream delta into the typewriter queue.
    ///
    /// If incoming text exceeds `MAX_QUEUE_BYTES`, oldest characters are adaptively
    /// drained or flushed and returned to avoid data loss and buffer overflow.
    pub fn push_delta(&mut self, delta: &str) -> Option<String> {
        if self.stream_start.is_none() {
            self.stream_start = Some(Instant::now());
        }
        if delta.is_empty() {
            return None;
        }
        let est_tokens = delta.len().div_ceil(4);
        self.token_count += est_tokens.max(1);

        let total_len = self.queue.len() + delta.len();
        if total_len <= Self::MAX_QUEUE_BYTES {
            self.queue.push_str(delta);
            return None;
        }

        let overflow = total_len - Self::MAX_QUEUE_BYTES;
        if overflow >= self.queue.len() {
            let mut flushed = std::mem::take(&mut self.queue);
            let delta_overflow = delta.len().saturating_sub(Self::MAX_QUEUE_BYTES);
            if delta_overflow > 0 {
                let mut split_idx = delta_overflow;
                while split_idx < delta.len() && !delta.is_char_boundary(split_idx) {
                    split_idx += 1;
                }
                flushed.push_str(&delta[..split_idx]);
                self.queue.push_str(&delta[split_idx..]);
            } else {
                self.queue.push_str(delta);
            }
            Some(flushed)
        } else {
            let drain_target = overflow.max(self.queue.len() / 8);
            let mut drain_end = drain_target.min(self.queue.len());
            while drain_end < self.queue.len() && !self.queue.is_char_boundary(drain_end) {
                drain_end += 1;
            }
            let drained: String = self.queue.drain(..drain_end).collect();
            self.queue.push_str(delta);
            Some(drained)
        }
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

    #[test]
    fn test_typewriter_empty_delta_does_not_increment_token_count() {
        let mut state = TypewriterState::new();
        state.start_stream();
        assert_eq!(state.token_count, 0);
        state.push_delta("");
        assert_eq!(state.token_count, 0);
    }

    #[test]
    fn test_push_large_100kb_delta_preserves_multibyte() {
        let mut state = TypewriterState::new();
        state.start_stream();

        let pattern = "🦀 Crab 🚀 Rocket 🌟 Star 🌸 Blossom 漢字 日本語 한국어\n";
        let mut large_chunk = String::new();
        while large_chunk.len() < 100 * 1024 {
            large_chunk.push_str(pattern);
        }
        let expected_bytes = large_chunk.len();
        let expected_chars = large_chunk.chars().count();
        assert!(expected_bytes >= 100 * 1024);

        let flushed = state.push_delta(&large_chunk);
        assert!(flushed.is_none());
        assert!(state.is_typing());
        assert_eq!(state.char_count(), expected_chars);

        let drained_all = state.flush();
        assert_eq!(drained_all.len(), expected_bytes);
        assert_eq!(drained_all.chars().count(), expected_chars);
        assert_eq!(drained_all, large_chunk);
    }

    #[test]
    fn test_push_delta_overflow_adaptively_drains_without_data_loss() {
        let mut state = TypewriterState::new();
        state.start_stream();

        let pattern = "🦀 Emoji test 🚀 Multi-byte: 漢字 日本語 한국어 ✨\n";
        let mut chunk1 = String::new();
        while chunk1.len() < 200 * 1024 {
            chunk1.push_str(pattern);
        }

        let mut chunk2 = String::new();
        while chunk2.len() < 100 * 1024 {
            chunk2.push_str(pattern);
        }

        let initial_overflow = state.push_delta(&chunk1);
        assert!(initial_overflow.is_none());

        // Pushing chunk2 overflows MAX_QUEUE_BYTES (200KB + 100KB = 300KB > 256KB)
        let drained = state.push_delta(&chunk2);
        assert!(drained.is_some());

        let mut total = drained.unwrap();
        total.push_str(&state.flush());

        let mut expected = chunk1;
        expected.push_str(&chunk2);

        assert_eq!(total.len(), expected.len());
        assert_eq!(total.chars().count(), expected.chars().count());
        assert_eq!(total, expected);
    }

    #[test]
    fn test_push_delta_exceeding_max_queue_in_single_call() {
        let mut state = TypewriterState::new();
        state.start_stream();

        let pattern = "🌊 Ocean 🐬 Dolphin 🌴 Palm 🌺 Hibiscus 🎋 Bamboo\n";
        let mut huge_chunk = String::new();
        while huge_chunk.len() < 300 * 1024 {
            huge_chunk.push_str(pattern);
        }

        let drained = state.push_delta(&huge_chunk);
        assert!(drained.is_some());

        let mut total = drained.unwrap();
        total.push_str(&state.flush());

        assert_eq!(total.len(), huge_chunk.len());
        assert_eq!(total.chars().count(), huge_chunk.chars().count());
        assert_eq!(total, huge_chunk);
    }
}
