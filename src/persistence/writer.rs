//! Bounded async-batch writer. Owns the `Db` on a worker thread; the UI thread
//! only sends appends through a bounded channel and never blocks on SQLite.

use crate::persistence::db::{
    ContextEpoch, Db, InputDelivery, MAX_MESSAGE_BYTES, MAX_TOOL_OUTPUT_BYTES, Message,
    SessionInput, ToolCall, ToolCallStatus,
};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

/// Capacity of the append channel. Senders get an error beyond this, so a
/// stalled DB cannot balloon memory.
pub const WRITER_CHANNEL_CAPACITY: usize = 1_024;

/// Pending append queued for a batched commit.
type PendingAppend = (i64, String, String, mpsc::Sender<Result<(), String>>);

/// Pending event append queued for a batched commit.
type PendingEvent = (
    i64,
    Option<i64>,
    String,
    String,
    mpsc::Sender<Result<i64, String>>,
);

enum Command {
    Append {
        session_id: i64,
        role: String,
        content: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
    AppendMessage {
        session_id: i64,
        role: String,
        content: String,
        reply: mpsc::Sender<Result<Message, String>>,
    },
    AppendEvent {
        session_id: i64,
        generation_id: Option<i64>,
        kind: String,
        payload_json: String,
        reply: mpsc::Sender<Result<i64, String>>,
    },
    CreateToolCall {
        session_id: i64,
        generation_id: i64,
        assistant_message_id: i64,
        call_id: String,
        tool_name: String,
        arguments: String,
        reply: mpsc::Sender<Result<(ToolCall, i64), String>>,
    },
    StartToolCall {
        id: i64,
        reply: mpsc::Sender<Result<(ToolCall, i64), String>>,
    },
    SettleToolCall {
        id: i64,
        status: ToolCallStatus,
        result: Option<String>,
        error: Option<String>,
        reply: mpsc::Sender<Result<(ToolCall, i64), String>>,
    },
    AdmitInput {
        session_id: i64,
        content: String,
        delivery: InputDelivery,
        reply: mpsc::Sender<Result<(SessionInput, i64), String>>,
    },
    PromoteInput {
        input_id: i64,
        reply: mpsc::Sender<Result<(SessionInput, Message, i64), String>>,
    },
    InsertContextEpoch {
        session_id: i64,
        epoch_id: String,
        baseline_system_text: String,
        source_snapshot_json: String,
        reply: mpsc::Sender<Result<ContextEpoch, String>>,
    },
    UpdateContextEpochSnapshot {
        session_id: i64,
        epoch_id: String,
        new_snapshot_json: String,
        reply: mpsc::Sender<Result<bool, String>>,
    },
    ReconcileEpochChange {
        session_id: i64,
        epoch_id: String,
        new_snapshot_json: String,
        delta_system_message: String,
        reply: mpsc::Sender<Result<Message, String>>,
    },
    Flush(mpsc::Sender<()>),
}

/// Handle for sending batched writes from any thread. Clonable; all clones
/// feed the same worker. The last `shutdown` takes back the `Db`.
#[derive(Clone)]
pub struct WriterHandle {
    inner: Arc<Mutex<Option<mpsc::SyncSender<Command>>>>,
    worker: Arc<Mutex<Option<thread::JoinHandle<Db>>>>,
}

impl WriterHandle {
    /// Spawn the writer thread owning `db`.
    pub fn spawn(db: Db) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Command>(WRITER_CHANNEL_CAPACITY);
        let worker = thread::spawn(move || {
            // ...existing worker loop, returns db at the end...
            let mut pending: Vec<PendingAppend> = Vec::new();
            let mut pending_events: Vec<PendingEvent> = Vec::new();
            while let Ok(command) = receiver.recv() {
                match command {
                    Command::Append {
                        session_id,
                        role,
                        content,
                        reply,
                    } => pending.push((session_id, role, content, reply)),
                    Command::AppendMessage {
                        session_id,
                        role,
                        content,
                        reply,
                    } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = reply.send(
                            db.append_message(session_id, &role, &content)
                                .map_err(|error| error.to_string()),
                        );
                    }
                    Command::AppendEvent {
                        session_id,
                        generation_id,
                        kind,
                        payload_json,
                        reply,
                    } => {
                        pending_events.push((session_id, generation_id, kind, payload_json, reply))
                    }
                    Command::CreateToolCall {
                        session_id,
                        generation_id,
                        assistant_message_id,
                        call_id,
                        tool_name,
                        arguments,
                        reply,
                    } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = reply.send(
                            db.create_tool_call(
                                session_id,
                                generation_id,
                                assistant_message_id,
                                &call_id,
                                &tool_name,
                                &arguments,
                            )
                            .map_err(|error| error.to_string()),
                        );
                    }
                    Command::StartToolCall { id, reply } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ =
                            reply.send(db.start_tool_call(id).map_err(|error| error.to_string()));
                    }
                    Command::SettleToolCall {
                        id,
                        status,
                        result,
                        error,
                        reply,
                    } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = reply.send(
                            db.settle_tool_call(id, status, result.as_deref(), error.as_deref())
                                .map_err(|error| error.to_string()),
                        );
                    }
                    Command::AdmitInput {
                        session_id,
                        content,
                        delivery,
                        reply,
                    } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = reply.send(
                            db.admit_input(session_id, &content, delivery)
                                .map_err(|error| error.to_string()),
                        );
                    }
                    Command::PromoteInput { input_id, reply } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = reply.send(
                            db.promote_input(input_id)
                                .map_err(|error| error.to_string()),
                        );
                    }
                    Command::InsertContextEpoch {
                        session_id,
                        epoch_id,
                        baseline_system_text,
                        source_snapshot_json,
                        reply,
                    } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = reply.send(
                            db.insert_context_epoch(
                                session_id,
                                &epoch_id,
                                &baseline_system_text,
                                &source_snapshot_json,
                            )
                            .map_err(|error| error.to_string()),
                        );
                    }
                    Command::UpdateContextEpochSnapshot {
                        session_id,
                        epoch_id,
                        new_snapshot_json,
                        reply,
                    } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = reply.send(
                            db.update_context_epoch_snapshot(
                                session_id,
                                &epoch_id,
                                &new_snapshot_json,
                            )
                            .map_err(|error| error.to_string()),
                        );
                    }
                    Command::ReconcileEpochChange {
                        session_id,
                        epoch_id,
                        new_snapshot_json,
                        delta_system_message,
                        reply,
                    } => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = reply.send(
                            db.reconcile_epoch_change(
                                session_id,
                                &epoch_id,
                                &new_snapshot_json,
                                &delta_system_message,
                            )
                            .map_err(|error| error.to_string()),
                        );
                    }
                    Command::Flush(ack) => {
                        flush_batch(&db, &mut pending);
                        flush_events(&db, &mut pending_events);
                        let _ = ack.send(());
                    }
                }
                // Drain what is already queued without blocking.
                while let Ok(command) = receiver.try_recv() {
                    match command {
                        Command::Append {
                            session_id,
                            role,
                            content,
                            reply,
                        } => pending.push((session_id, role, content, reply)),
                        Command::AppendMessage {
                            session_id,
                            role,
                            content,
                            reply,
                        } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply.send(
                                db.append_message(session_id, &role, &content)
                                    .map_err(|error| error.to_string()),
                            );
                        }
                        Command::AppendEvent {
                            session_id,
                            generation_id,
                            kind,
                            payload_json,
                            reply,
                        } => pending_events.push((
                            session_id,
                            generation_id,
                            kind,
                            payload_json,
                            reply,
                        )),
                        Command::CreateToolCall {
                            session_id,
                            generation_id,
                            assistant_message_id,
                            call_id,
                            tool_name,
                            arguments,
                            reply,
                        } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply.send(
                                db.create_tool_call(
                                    session_id,
                                    generation_id,
                                    assistant_message_id,
                                    &call_id,
                                    &tool_name,
                                    &arguments,
                                )
                                .map_err(|error| error.to_string()),
                            );
                        }
                        Command::StartToolCall { id, reply } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply
                                .send(db.start_tool_call(id).map_err(|error| error.to_string()));
                        }
                        Command::SettleToolCall {
                            id,
                            status,
                            result,
                            error,
                            reply,
                        } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply.send(
                                db.settle_tool_call(
                                    id,
                                    status,
                                    result.as_deref(),
                                    error.as_deref(),
                                )
                                .map_err(|error| error.to_string()),
                            );
                        }
                        Command::AdmitInput {
                            session_id,
                            content,
                            delivery,
                            reply,
                        } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply.send(
                                db.admit_input(session_id, &content, delivery)
                                    .map_err(|error| error.to_string()),
                            );
                        }
                        Command::PromoteInput { input_id, reply } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply.send(
                                db.promote_input(input_id)
                                    .map_err(|error| error.to_string()),
                            );
                        }
                        Command::InsertContextEpoch {
                            session_id,
                            epoch_id,
                            baseline_system_text,
                            source_snapshot_json,
                            reply,
                        } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply.send(
                                db.insert_context_epoch(
                                    session_id,
                                    &epoch_id,
                                    &baseline_system_text,
                                    &source_snapshot_json,
                                )
                                .map_err(|error| error.to_string()),
                            );
                        }
                        Command::UpdateContextEpochSnapshot {
                            session_id,
                            epoch_id,
                            new_snapshot_json,
                            reply,
                        } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply.send(
                                db.update_context_epoch_snapshot(
                                    session_id,
                                    &epoch_id,
                                    &new_snapshot_json,
                                )
                                .map_err(|error| error.to_string()),
                            );
                        }
                        Command::ReconcileEpochChange {
                            session_id,
                            epoch_id,
                            new_snapshot_json,
                            delta_system_message,
                            reply,
                        } => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = reply.send(
                                db.reconcile_epoch_change(
                                    session_id,
                                    &epoch_id,
                                    &new_snapshot_json,
                                    &delta_system_message,
                                )
                                .map_err(|error| error.to_string()),
                            );
                        }
                        Command::Flush(ack) => {
                            flush_batch(&db, &mut pending);
                            flush_events(&db, &mut pending_events);
                            let _ = ack.send(());
                        }
                    }
                }
                flush_batch(&db, &mut pending);
                flush_events(&db, &mut pending_events);
            }
            flush_batch(&db, &mut pending);
            flush_events(&db, &mut pending_events);
            db
        });
        Self {
            inner: Arc::new(Mutex::new(Some(sender))),
            worker: Arc::new(Mutex::new(Some(worker))),
        }
    }

    fn sender(&self) -> Result<mpsc::SyncSender<Command>, String> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| "writer shut down".to_string())
    }

    /// Queue an append without waiting for the commit. Returns the receiver
    /// for the eventual result so callers that care can await it; dropping
    /// the receiver is fine — the write still happens.
    ///
    /// Errors when the bounded channel is full (backpressure) or the content
    /// exceeds [`MAX_MESSAGE_BYTES`].
    pub fn try_append(
        &self,
        session_id: i64,
        role: &str,
        content: &str,
    ) -> Result<mpsc::Receiver<Result<(), String>>, String> {
        if content.len() > MAX_MESSAGE_BYTES {
            return Err(format!(
                "message too large: {} bytes (max {MAX_MESSAGE_BYTES})",
                content.len()
            ));
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::Append {
                session_id,
                role: role.to_string(),
                content: content.to_string(),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        Ok(reply_rx)
    }

    /// Queue an append and wait until it is committed. Blocking variant for
    /// tests and callers that need the durable ack.
    pub fn append(&self, session_id: i64, role: &str, content: &str) -> Result<(), String> {
        let reply_rx = self.try_append(session_id, role, content)?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }
    /// Queue an assistant/tool message and return its committed SQLite id.
    pub fn append_message(
        &self,
        session_id: i64,
        role: &str,
        content: &str,
    ) -> Result<Message, String> {
        if content.len() > MAX_MESSAGE_BYTES {
            return Err(format!(
                "message too large: {} bytes (max {MAX_MESSAGE_BYTES})",
                content.len()
            ));
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::AppendMessage {
                session_id,
                role: role.to_string(),
                content: content.to_string(),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    pub fn create_tool_call(
        &self,
        session_id: i64,
        generation_id: i64,
        assistant_message_id: i64,
        call_id: &str,
        tool_name: &str,
        arguments: &str,
    ) -> Result<(ToolCall, i64), String> {
        if arguments.len() > MAX_TOOL_OUTPUT_BYTES {
            return Err(format!(
                "tool arguments too large: {} bytes (max {MAX_TOOL_OUTPUT_BYTES})",
                arguments.len()
            ));
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::CreateToolCall {
                session_id,
                generation_id,
                assistant_message_id,
                call_id: call_id.to_string(),
                tool_name: tool_name.to_string(),
                arguments: arguments.to_string(),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    pub fn start_tool_call(&self, id: i64) -> Result<(ToolCall, i64), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::StartToolCall {
                id,
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    pub fn settle_tool_call(
        &self,
        id: i64,
        status: ToolCallStatus,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Result<(ToolCall, i64), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::SettleToolCall {
                id,
                status,
                result: result.map(str::to_string),
                error: error.map(str::to_string),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    pub fn admit_input(
        &self,
        session_id: i64,
        content: &str,
        delivery: InputDelivery,
    ) -> Result<(SessionInput, i64), String> {
        if content.len() > MAX_MESSAGE_BYTES {
            return Err(format!(
                "message too large: {} bytes (max {MAX_MESSAGE_BYTES})",
                content.len()
            ));
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::AdmitInput {
                session_id,
                content: content.to_string(),
                delivery,
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    pub fn promote_input(&self, input_id: i64) -> Result<(SessionInput, Message, i64), String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::PromoteInput {
                input_id,
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    pub fn insert_context_epoch(
        &self,
        session_id: i64,
        epoch_id: &str,
        baseline_system_text: &str,
        source_snapshot_json: &str,
    ) -> Result<ContextEpoch, String> {
        if baseline_system_text.len() > MAX_MESSAGE_BYTES {
            return Err(format!(
                "message too large: {} bytes (max {MAX_MESSAGE_BYTES})",
                baseline_system_text.len()
            ));
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::InsertContextEpoch {
                session_id,
                epoch_id: epoch_id.to_string(),
                baseline_system_text: baseline_system_text.to_string(),
                source_snapshot_json: source_snapshot_json.to_string(),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    pub fn update_context_epoch_snapshot(
        &self,
        session_id: i64,
        epoch_id: &str,
        new_snapshot_json: &str,
    ) -> Result<bool, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::UpdateContextEpochSnapshot {
                session_id,
                epoch_id: epoch_id.to_string(),
                new_snapshot_json: new_snapshot_json.to_string(),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    pub fn reconcile_epoch_change(
        &self,
        session_id: i64,
        epoch_id: &str,
        new_snapshot_json: &str,
        delta_system_message: &str,
    ) -> Result<Message, String> {
        if delta_system_message.len() > MAX_MESSAGE_BYTES {
            return Err(format!(
                "message too large: {} bytes (max {MAX_MESSAGE_BYTES})",
                delta_system_message.len()
            ));
        }
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::ReconcileEpochChange {
                session_id,
                epoch_id: epoch_id.to_string(),
                new_snapshot_json: new_snapshot_json.to_string(),
                delta_system_message: delta_system_message.to_string(),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    /// Queue an event-log append without waiting for the commit. Returns the
    /// receiver for the eventual seq; dropping it is fine.
    pub fn try_append_event(
        &self,
        session_id: i64,
        generation_id: Option<i64>,
        kind: &str,
        payload_json: &str,
    ) -> Result<mpsc::Receiver<Result<i64, String>>, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender()?
            .try_send(Command::AppendEvent {
                session_id,
                generation_id,
                kind: kind.to_string(),
                payload_json: payload_json.to_string(),
                reply: reply_tx,
            })
            .map_err(|error| error.to_string())?;
        Ok(reply_rx)
    }

    /// Queue an event-log append and wait for the committed seq.
    pub fn append_event(
        &self,
        session_id: i64,
        generation_id: Option<i64>,
        kind: &str,
        payload_json: &str,
    ) -> Result<i64, String> {
        let reply_rx = self.try_append_event(session_id, generation_id, kind, payload_json)?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    /// Wait until all queued writes before this call are committed.
    pub fn flush(&self) {
        let (ack_tx, ack_rx) = mpsc::channel();
        if let Ok(sender) = self.sender()
            && sender.send(Command::Flush(ack_tx)).is_ok()
        {
            let _ = ack_rx.recv();
        }
    }

    /// Stop the worker and take back the `Db`. Blocks until all other
    /// handles are dropped, so clones must die first.
    pub fn shutdown(&self) -> Option<Db> {
        let sender = self
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()?;
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()?;
        drop(sender);
        let db = worker.join().ok()?;
        Some(db)
    }
}

/// Commit pending appends in one transaction and ack each sender. A row that
/// fails to insert gets `Err` in its own reply; remaining rows still commit.
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

/// Commit pending event appends in one transaction and ack each sender with
/// the assigned seq.
fn flush_events(db: &Db, pending: &mut Vec<PendingEvent>) {
    let batch: Vec<PendingEvent> = std::mem::take(pending);
    let tx = match db.connection.unchecked_transaction() {
        Ok(tx) => tx,
        Err(error) => {
            for (_, _, _, _, reply) in &batch {
                let _ = reply.send(Err(error.to_string()));
            }
            return;
        }
    };
    let mut results: Vec<Result<i64, String>> = Vec::with_capacity(batch.len());
    let mut session_max_seq: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    for (session_id, generation_id, kind, payload_json, _) in &batch {
        let res = tx
            .query_row(
                "INSERT INTO generation_events (seq, session_id, generation_id, kind, payload_json)
             VALUES (
                 COALESCE((SELECT MAX(seq) + 1 FROM generation_events WHERE session_id = ?1), 0),
                 ?1, ?2, ?3, ?4
             )
             RETURNING seq",
                rusqlite::params![session_id, generation_id, kind, payload_json],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string());
        if let Ok(seq) = res {
            let entry = session_max_seq.entry(*session_id).or_insert(seq);
            if seq > *entry {
                *entry = seq;
            }
        }
        results.push(res);
    }
    for (session_id, max_seq) in session_max_seq {
        let _ = tx.execute(
            "UPDATE sessions SET last_event_seq = MAX(last_event_seq, ?2),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1",
            rusqlite::params![session_id, max_seq],
        );
    }
    if let Err(error) = tx.commit() {
        for result in &mut results {
            *result = Err(error.to_string());
        }
        tracing::error!(%error, "persistence event batch commit failed");
    }
    for ((_, _, _, _, reply), result) in batch.into_iter().zip(results) {
        let _ = reply.send(result);
    }
}
