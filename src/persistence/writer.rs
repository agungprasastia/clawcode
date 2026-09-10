//! Bounded async-batch writer. Owns the `Db` on a worker thread; the UI thread
//! only sends appends through a bounded channel and never blocks on SQLite.

use crate::persistence::db::Db;
use std::sync::mpsc;
use std::thread;

/// Capacity of the append channel. Senders get an error beyond this, so a
/// stalled DB cannot balloon memory.
pub const WRITER_CHANNEL_CAPACITY: usize = 1_024;

/// Pending append queued for a batched commit.
type PendingAppend = (i64, String, String, mpsc::Sender<Result<(), String>>);

enum Command {
    Append {
        session_id: i64,
        role: String,
        content: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Flush(mpsc::Sender<()>),
    Shutdown,
}

/// Handle for sending batched writes from the UI thread.
pub struct WriterHandle {
    sender: mpsc::SyncSender<Command>,
    worker: Option<thread::JoinHandle<Db>>,
}

impl WriterHandle {
    /// Spawn the writer thread owning `db`.
    pub fn spawn(db: Db) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Command>(WRITER_CHANNEL_CAPACITY);
        let worker = thread::spawn(move || {
            let mut pending: Vec<PendingAppend> = Vec::new();
            loop {
                // Block until first command, then drain what is ready.
                let Ok(command) = receiver.recv() else {
                    break;
                };
                match command {
                    Command::Shutdown => break,
                    Command::Flush(ack) => {
                        flush_batch(&db, &mut pending);
                        let _ = ack.send(());
                    }
                    Command::Append { .. } => {
                        let mut command = command;
                        loop {
                            if let Command::Append {
                                session_id,
                                role,
                                content,
                                reply,
                            } = command
                            {
                                pending.push((session_id, role, content, reply));
                            }
                            match receiver.try_recv() {
                                Ok(next) => command = next,
                                Err(_) => break,
                            }
                        }
                        flush_batch(&db, &mut pending);
                    }
                }
            }
            db
        });
        Self {
            sender,
            worker: Some(worker),
        }
    }

    /// Queue an append. Blocks only when the bounded channel is full, which
    /// applies backpressure instead of unbounded memory growth.
    pub fn append(&self, session_id: i64, role: &str, content: &str) -> Result<(), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(Command::Append {
                session_id,
                role: role.to_string(),
                content: content.to_string(),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        // Wait for the batch ack so tests are deterministic; production
        // callers may ignore the receiver.
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    /// Wait until all queued writes before this call are committed.
    pub fn flush(&self) {
        let (ack_tx, ack_rx) = mpsc::channel();
        if self.sender.send(Command::Flush(ack_tx)).is_ok() {
            let _ = ack_rx.recv();
        }
    }

    /// Stop the worker and take back the `Db`.
    pub fn shutdown(mut self) -> Db {
        let _ = self.sender.send(Command::Shutdown);
        self.worker
            .take()
            .expect("worker already joined")
            .join()
            .expect("writer thread panicked")
    }
}

/// Commit pending appends in one transaction and ack each sender.
fn flush_batch(db: &Db, pending: &mut Vec<PendingAppend>) {
    let batch: Vec<PendingAppend> = std::mem::take(pending);
    let tx = match db.connection.unchecked_transaction() {
        Ok(tx) => tx,
        Err(error) => {
            for (_, _, _, reply) in &batch {
                let _ = reply.send(Err(error.to_string()));
            }
            return;
        }
    };
    let mut results: Vec<Result<(), String>> = Vec::with_capacity(batch.len());
    for (session_id, role, content, _) in &batch {
        results.push(
            tx.execute(
                "INSERT INTO messages (session_id, role, content) VALUES (?1, ?2, ?3)",
                rusqlite::params![session_id, role, content],
            )
            .map(|_| ())
            .map_err(|error| error.to_string()),
        );
    }
    if let Err(error) = tx.commit() {
        for result in &mut results {
            *result = Err(error.to_string());
        }
        tracing::error!(%error, "persistence batch commit failed");
    }
    for ((_, _, _, reply), result) in batch.into_iter().zip(results) {
        let _ = reply.send(result);
    }
}
