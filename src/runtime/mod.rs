//! In-process runtime boundary: append-only event log plus fan-out to
//! subscribers. Fully synchronous (std `mpsc` + `Mutex`); the UI thread never
//! touches SQLite directly — everything flows through [`crate::runtime::client::RuntimeClient`].
//!
//! Replay model (crabcode OPTION4): the log is the source of truth. A
//! subscriber joins with `after_seq` and replays missed events from the DB;
//! live events are pushed through the bus with the committed seq.

pub mod client;

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

/// Capacity of one subscriber's queue. A subscriber that falls further
/// behind misses events (its receiver is closed); it must replay from the
/// log to catch up.
const SUBSCRIBER_CHANNEL_CAPACITY: usize = 1_024;

/// One runtime event, mirroring a `generation_events` row.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeEvent {
    pub seq: i64,
    pub session_id: i64,
    pub generation_id: Option<i64>,
    pub kind: String,
    pub payload_json: String,
}

/// Fan-out hub. Subscribers filter by session (`None` = all sessions).
/// Cheap to clone; all clones share the same subscriber list.
#[derive(Clone)]
pub struct EventBus {
    inner: std::sync::Arc<EventBusInner>,
}

struct EventBusInner {
    subscribers: Mutex<Vec<Subscriber>>,
    next_id: AtomicU64,
}

struct Subscriber {
    id: u64,
    session_id: Option<i64>,
    sender: mpsc::SyncSender<RuntimeEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(EventBusInner {
                subscribers: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    /// Register a subscriber. Returns the subscription id and the receiver.
    pub fn subscribe(&self, session_id: Option<i64>) -> (u64, mpsc::Receiver<RuntimeEvent>) {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::sync_channel(SUBSCRIBER_CHANNEL_CAPACITY);
        self.inner
            .subscribers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Subscriber {
                id,
                session_id,
                sender,
            });
        (id, receiver)
    }

    pub fn unsubscribe(&self, id: u64) {
        self.inner
            .subscribers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|subscriber| subscriber.id != id);
    }

    /// Push an event to matching subscribers. Matching subscribers apply
    /// backpressure instead of being disconnected when their queue is full.
    /// A dropped receiver is pruned here.
    pub fn publish(&self, event: RuntimeEvent) {
        let mut subscribers = self
            .inner
            .subscribers
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        subscribers.retain(|subscriber| {
            if subscriber.session_id.is_none_or(|s| s == event.session_id) {
                subscriber.sender.send(event.clone()).is_ok()
            } else {
                true
            }
        });
    }
}
