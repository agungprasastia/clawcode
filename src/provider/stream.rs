use super::events::StreamEvent;
use std::sync::{Arc, Mutex, mpsc};

#[derive(Clone, Debug)]
pub struct StreamSender {
    sender: mpsc::SyncSender<StreamEvent>,
    state: Arc<StreamState>,
}

#[derive(Debug)]
struct StreamState {
    cancelled: Mutex<bool>,
    pending_delta: Mutex<String>,
}

#[derive(Debug)]
pub struct ProviderStream {
    receiver: mpsc::Receiver<StreamEvent>,
    state: Arc<StreamState>,
    cancelled_emitted: bool,
}

impl ProviderStream {
    pub fn channel(capacity: usize) -> (StreamSender, Self) {
        assert!(capacity > 0, "stream capacity must be positive");
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let state = Arc::new(StreamState {
            cancelled: Mutex::new(false),
            pending_delta: Mutex::new(String::new()),
        });
        (
            StreamSender {
                sender,
                state: Arc::clone(&state),
            },
            Self {
                receiver,
                state,
                cancelled_emitted: false,
            },
        )
    }

    pub fn cancel(&self) {
        *self.state.cancelled.lock().expect("stream state poisoned") = true;
    }

    fn cancellation_pending(&self) -> bool {
        *self.state.cancelled.lock().expect("stream state poisoned")
    }
}

impl Iterator for ProviderStream {
    type Item = StreamEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cancellation_pending() && !self.cancelled_emitted {
            self.cancelled_emitted = true;
            return Some(StreamEvent::Cancelled);
        }
        if self.cancelled_emitted {
            return None;
        }
        match self.receiver.recv() {
            Ok(event) => {
                if self.cancellation_pending() {
                    self.cancelled_emitted = true;
                    Some(StreamEvent::Cancelled)
                } else {
                    Some(event)
                }
            }
            Err(_) => None,
        }
    }
}

impl StreamSender {
    pub fn send(&self, event: StreamEvent) -> Result<(), mpsc::SendError<StreamEvent>> {
        if *self.state.cancelled.lock().expect("stream state poisoned") {
            return Ok(());
        }
        match event {
            StreamEvent::TextDelta(delta) => {
                self.state
                    .pending_delta
                    .lock()
                    .expect("stream state poisoned")
                    .push_str(&delta);
                Ok(())
            }
            event => {
                self.flush_delta()?;
                self.sender.send(event)
            }
        }
    }

    pub fn flush(&self) -> Result<(), mpsc::SendError<StreamEvent>> {
        self.flush_delta()
    }

    fn flush_delta(&self) -> Result<(), mpsc::SendError<StreamEvent>> {
        let delta = {
            let mut pending = self
                .state
                .pending_delta
                .lock()
                .expect("stream state poisoned");
            std::mem::take(&mut *pending)
        };
        if delta.is_empty() {
            return Ok(());
        }
        self.sender.send(StreamEvent::TextDelta(delta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesces_deltas_and_prioritizes_cancellation() {
        let (sender, mut stream) = ProviderStream::channel(1);
        sender.send(StreamEvent::TextDelta("a".into())).unwrap();
        sender.send(StreamEvent::TextDelta("b".into())).unwrap();
        sender.flush().unwrap();
        stream.cancel();
        assert!(matches!(stream.next(), Some(StreamEvent::Cancelled)));
        assert!(stream.next().is_none());
    }
}
