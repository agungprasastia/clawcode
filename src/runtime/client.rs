//! Client boundary to the runtime worker. Commands go in over a bounded
//! channel; events come back via the [`EventBus`]. Control commands run
//! inline on the worker thread; generations run on their own threads, so a
//! slow provider turn never blocks session creation or cancellation. The
//! worker thread never blocks on the network; SQLite is shared behind a
//! `Mutex` with short-lived accesses.

use super::coordinator::{SessionConfig, SessionCoordinator, WakeOutcome};
use super::{EventBus, RuntimeEvent};
use crate::persistence::{
    Db, GenerationStatus, MAX_MESSAGE_BYTES, MAX_TOOL_OUTPUT_BYTES, ToolCallStatus, WriterHandle,
};
use crate::provider::{FinishReason, Provider, StreamEvent, StreamRequest};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

/// Maximum output tokens per generation request.
pub const MAX_OUTPUT_TOKENS: u32 = 4_096;

/// Capacity of the command channel. Beyond this, commands error (backpressure).
pub const CLIENT_CHANNEL_CAPACITY: usize = 256;

/// How long a SQLite access waits for a competing writer before failing.
pub const DB_BUSY_TIMEOUT: Duration = Duration::from_millis(2_000);

/// Commands accepted by the runtime worker.
pub enum ClientCommand {
    /// Create a session in a workspace; replies with the session id.
    CreateSession {
        workspace_id: i64,
        title: String,
        reply: mpsc::Sender<Result<i64, String>>,
    },
    /// Start a generation: marks the session active, runs the provider turn,
    /// streams deltas onto the bus, persists the assistant message.
    StartGeneration {
        session_id: i64,
        agent_mode: String,
        provider: String,
        model: String,
        prompt: String,
    },
    /// Request cancellation of the active generation for a session.
    CancelGeneration { session_id: i64 },
}

/// Handle for talking to the runtime worker.
pub struct RuntimeClient {
    sender: Option<mpsc::SyncSender<ClientCommand>>,
    worker: Option<thread::JoinHandle<Arc<Mutex<Db>>>>,
    coordinator: Arc<SessionCoordinator>,
    db: Option<Arc<Mutex<Db>>>,
    bus: EventBus,
}

impl RuntimeClient {
    /// Spawn the worker. The boxed provider is shared across generation
    /// threads, hence the `Send + Sync` bound.
    pub fn spawn(
        db: Db,
        writer: WriterHandle,
        provider: Box<dyn Provider + Send + Sync>,
        bus: EventBus,
    ) -> Self {
        db.set_busy_timeout(DB_BUSY_TIMEOUT);
        let db = Arc::new(Mutex::new(db));
        let client_db = Arc::clone(&db);
        let coordinator = Arc::new(SessionCoordinator::new());
        let (sender, receiver) = mpsc::sync_channel::<ClientCommand>(CLIENT_CHANNEL_CAPACITY);
        let worker_coordinator = Arc::clone(&coordinator);
        let worker_bus = bus.clone();
        let worker = thread::spawn(move || {
            worker_loop(
                SharedDb::new(db.clone()),
                writer,
                Arc::from(provider),
                worker_bus,
                worker_coordinator,
                receiver,
            );
            db
        });
        Self {
            sender: Some(sender),
            worker: Some(worker),
            coordinator,
            db: Some(client_db),
            bus,
        }
    }

    /// Create a session and wait for the id.
    pub fn create_session(&self, workspace_id: i64, title: &str) -> Result<i64, String> {
        let (reply, reply_rx) = mpsc::channel();
        self.send(ClientCommand::CreateSession {
            workspace_id,
            title: title.to_string(),
            reply,
        })?;
        reply_rx.recv().map_err(|error| error.to_string())?
    }

    /// Queue a generation start. Non-blocking; progress arrives on the bus.
    pub fn start_generation(
        &self,
        session_id: i64,
        agent_mode: &str,
        provider: &str,
        model: &str,
        prompt: &str,
    ) -> Result<(), String> {
        if prompt.len() > MAX_MESSAGE_BYTES {
            return Err(format!(
                "message too large: {} bytes (max {MAX_MESSAGE_BYTES})",
                prompt.len()
            ));
        }
        self.try_send(ClientCommand::StartGeneration {
            session_id,
            agent_mode: agent_mode.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            prompt: prompt.to_string(),
        })
    }

    /// Request cancellation of the session's active generation.
    ///
    /// Uses a blocking send: unlike a stream chunk, a cancelled turn must
    /// never be silently dropped when the command channel is momentarily full.
    pub fn cancel_generation(&self, session_id: i64) -> Result<(), String> {
        self.send(ClientCommand::CancelGeneration { session_id })
    }

    /// Check if a session has an active drain running.
    pub fn is_active(&self, session_id: i64) -> bool {
        self.coordinator.is_active(session_id)
    }

    /// Access the session coordinator.
    pub fn coordinator(&self) -> &Arc<SessionCoordinator> {
        &self.coordinator
    }
    /// Stop the worker, wait for in-flight generations, take back the `Db`.
    pub fn shutdown(mut self) -> Db {
        drop(self.sender.take());
        drop(self.db.take());
        let worker = self.worker.take().expect("client shut down twice");
        let db_arc = worker.join().expect("runtime worker thread panicked");
        Arc::into_inner(db_arc)
            .and_then(|mutex| mutex.into_inner().ok())
            .expect("runtime worker sole owner of db")
    }

    /// Access the runtime event bus.
    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    /// Replay durable events for `session_id` after `after_seq` (exclusive).
    pub fn replay_events_after(
        &self,
        session_id: i64,
        after_seq: i64,
    ) -> Result<Vec<RuntimeEvent>, String> {
        let db_arc = self
            .db
            .as_ref()
            .ok_or_else(|| "client shut down".to_string())?;
        let guard = db_arc.lock().unwrap_or_else(|p| p.into_inner());
        let events = guard
            .events_after(session_id, after_seq)
            .map_err(|error| error.to_string())?;
        Ok(events.into_iter().map(RuntimeEvent::from).collect())
    }

    /// Subscribe to events for `session_id` starting after `after_seq`.
    /// Durable events are replayed from SQLite first, and subsequent live
    /// events from the bus are deduplicated against the replay cursor.
    pub fn subscribe_after(
        &self,
        session_id: i64,
        after_seq: i64,
    ) -> Result<ReplaySubscription, String> {
        let (sub_id, rx) = self.bus.subscribe(Some(session_id));
        let historical = match self.replay_events_after(session_id, after_seq) {
            Ok(events) => events,
            Err(err) => {
                self.bus.unsubscribe(sub_id);
                return Err(err);
            }
        };
        let max_historical_seq = historical
            .iter()
            .map(|e| e.seq)
            .max()
            .unwrap_or(after_seq)
            .max(after_seq);

        Ok(ReplaySubscription {
            sub_id,
            session_id,
            bus: self.bus.clone(),
            rx,
            historical: historical.into(),
            loaded_until_seq: after_seq,
            max_historical_seq,
        })
    }

    fn send(&self, command: ClientCommand) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or_else(|| "client shut down".to_string())?
            .send(command)
            .map_err(|error| error.to_string())
    }

    fn try_send(&self, command: ClientCommand) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or_else(|| "client shut down".to_string())?
            .try_send(command)
            .map_err(|error| error.to_string())
    }
}

/// Subscription that replays historical events after a cursor and tails live bus events.
/// Drops and unsubscribe are handled automatically.
pub struct ReplaySubscription {
    sub_id: u64,
    session_id: i64,
    bus: EventBus,
    rx: mpsc::Receiver<RuntimeEvent>,
    historical: VecDeque<RuntimeEvent>,
    loaded_until_seq: i64,
    max_historical_seq: i64,
}

impl ReplaySubscription {
    pub fn subscription_id(&self) -> u64 {
        self.sub_id
    }

    pub fn session_id(&self) -> i64 {
        self.session_id
    }

    /// Highest durable event seq (`seq > 0`) delivered by this subscription so far.
    pub fn loaded_until_seq(&self) -> i64 {
        self.loaded_until_seq
    }

    pub fn is_historical_drained(&self) -> bool {
        self.historical.is_empty()
    }

    /// Try to receive the next event without blocking.
    pub fn try_recv(&mut self) -> Result<RuntimeEvent, mpsc::TryRecvError> {
        if let Some(event) = self.historical.pop_front() {
            if event.seq > 0 && event.seq > self.loaded_until_seq {
                self.loaded_until_seq = event.seq;
            }
            return Ok(event);
        }

        loop {
            let event = self.rx.try_recv()?;
            if event.seq > 0 && event.seq <= self.max_historical_seq {
                continue;
            }
            if event.seq > 0 {
                if event.seq > self.loaded_until_seq {
                    self.loaded_until_seq = event.seq;
                }
                if event.seq > self.max_historical_seq {
                    self.max_historical_seq = event.seq;
                }
            }
            return Ok(event);
        }
    }

    /// Block until the next event arrives.
    pub fn recv(&mut self) -> Result<RuntimeEvent, mpsc::RecvError> {
        if let Some(event) = self.historical.pop_front() {
            if event.seq > 0 && event.seq > self.loaded_until_seq {
                self.loaded_until_seq = event.seq;
            }
            return Ok(event);
        }

        loop {
            let event = self.rx.recv()?;
            if event.seq > 0 && event.seq <= self.max_historical_seq {
                continue;
            }
            if event.seq > 0 {
                if event.seq > self.loaded_until_seq {
                    self.loaded_until_seq = event.seq;
                }
                if event.seq > self.max_historical_seq {
                    self.max_historical_seq = event.seq;
                }
            }
            return Ok(event);
        }
    }

    /// Block with a timeout until the next event arrives.
    pub fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<RuntimeEvent, mpsc::RecvTimeoutError> {
        if let Some(event) = self.historical.pop_front() {
            if event.seq > 0 && event.seq > self.loaded_until_seq {
                self.loaded_until_seq = event.seq;
            }
            return Ok(event);
        }

        let start = std::time::Instant::now();
        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return Err(mpsc::RecvTimeoutError::Timeout);
            }
            let remaining = timeout - elapsed;
            let event = self.rx.recv_timeout(remaining)?;
            if event.seq > 0 && event.seq <= self.max_historical_seq {
                continue;
            }
            if event.seq > 0 {
                if event.seq > self.loaded_until_seq {
                    self.loaded_until_seq = event.seq;
                }
                if event.seq > self.max_historical_seq {
                    self.max_historical_seq = event.seq;
                }
            }
            return Ok(event);
        }
    }
}

impl Drop for ReplaySubscription {
    fn drop(&mut self) {
        self.bus.unsubscribe(self.sub_id);
    }
}

/// Shared handle to the worker-owned `Db`. All access is short-lived; no
/// lock is held across provider I/O.
#[derive(Clone)]
struct SharedDb(Arc<Mutex<Db>>);

impl SharedDb {
    fn new(db: Arc<Mutex<Db>>) -> Self {
        Self(db)
    }

    fn with<R>(&self, f: impl FnOnce(&Db) -> R) -> R {
        let guard = self.0.lock().unwrap_or_else(|p| p.into_inner());
        f(&guard)
    }

    fn last_error(&self, session_id: i64, message: &str) {
        self.with(|db| {
            let _ = db.set_session_last_error(session_id, Some(message));
        });
    }

    fn clear_last_error(&self, session_id: i64) {
        self.with(|db| {
            let _ = db.set_session_last_error(session_id, None);
        });
    }
}

/// Worker main loop: control commands run inline; each `StartGeneration`
/// spawns its own thread so the loop never waits on a provider turn.
fn worker_loop(
    db: SharedDb,
    writer: WriterHandle,
    provider: Arc<dyn Provider + Send + Sync>,
    bus: EventBus,
    coordinator: Arc<SessionCoordinator>,
    receiver: mpsc::Receiver<ClientCommand>,
) {
    let mut handles: Vec<thread::JoinHandle<()>> = Vec::new();

    while let Ok(command) = receiver.recv() {
        handles.retain(|h| !h.is_finished());
        match command {
            ClientCommand::CreateSession {
                workspace_id,
                title,
                reply,
            } => {
                let result = db
                    .with(|db| db.create_session_in_workspace(workspace_id, &title))
                    .map(|session| session.id)
                    .map_err(|error| error.to_string());
                let _ = reply.send(result);
            }
            ClientCommand::StartGeneration {
                session_id,
                agent_mode,
                provider: provider_name,
                model,
                prompt,
            } => {
                let (input, admit_seq) = match writer.admit_input(
                    session_id,
                    &prompt,
                    crate::persistence::db::InputDelivery::Queue,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        publish_error(&bus, session_id, None, &error);
                        continue;
                    }
                };
                bus.publish(RuntimeEvent {
                    seq: admit_seq,
                    session_id,
                    generation_id: None,
                    kind: "prompt_admitted".to_string(),
                    payload_json: serde_json::json!({
                        "input_id": input.id,
                        "session_id": session_id,
                        "delivery": input.delivery.as_str(),
                    })
                    .to_string(),
                });

                coordinator.set_session_config(
                    session_id,
                    SessionConfig {
                        agent_mode,
                        provider: provider_name,
                        model,
                    },
                );

                match coordinator.wake(session_id) {
                    WakeOutcome::Scheduled => {
                        let drain_db = db.clone();
                        let drain_writer = writer.clone();
                        let drain_bus = bus.clone();
                        let drain_coordinator = Arc::clone(&coordinator);
                        let drain_provider = Arc::clone(&provider);
                        handles.push(thread::spawn(move || {
                            let res =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    drain_session_loop(
                                        drain_db,
                                        drain_writer,
                                        drain_bus,
                                        drain_coordinator.clone(),
                                        drain_provider,
                                        session_id,
                                    );
                                }));
                            if res.is_err() {
                                tracing::error!(session_id, "drain thread panicked");
                                drain_coordinator.finish_drain(session_id);
                            }
                        }));
                    }
                    WakeOutcome::Coalesced | WakeOutcome::Ignored => {}
                }
            }
            ClientCommand::CancelGeneration { session_id } => {
                let active_gen_id = coordinator.interrupt(session_id);
                if let Some(generation_id) = active_gen_id {
                    match db.with(|db| db.request_cancel(generation_id)) {
                        Ok(true) => {}
                        Ok(false) => {
                            publish_status(&bus, session_id, Some(generation_id), "cancel_noop")
                        }
                        Err(error) => {
                            publish_error(&bus, session_id, Some(generation_id), &error.to_string())
                        }
                    }
                } else if coordinator.is_cleaning_up(session_id) {
                    // Interrupted before generation started; drain loop will observe cleaning_up and exit.
                } else {
                    publish_status(&bus, session_id, None, "cancel_rejected");
                }
            }
        }
    }
    for handle in handles {
        let _ = handle.join();
    }
    let _ = writer.shutdown();
}

fn drain_session_loop(
    db: SharedDb,
    writer: WriterHandle,
    bus: EventBus,
    coordinator: Arc<SessionCoordinator>,
    provider: Arc<dyn Provider + Send + Sync>,
    session_id: i64,
) {
    loop {
        let next_input = match db.with(|db| db.next_pending_input(session_id)) {
            Ok(Some(input)) => input,
            Ok(None) => {
                coordinator.finish_drain(session_id);
                break;
            }
            Err(error) => {
                publish_error(&bus, session_id, None, &error.to_string());
                coordinator.finish_drain(session_id);
                break;
            }
        };

        let workspace_dir = db
            .with(|db| {
                let session = db.session(session_id).ok().flatten();
                let ws = session.and_then(|s| db.workspace(s.workspace_id).ok().flatten());
                if let Some(ws) = ws {
                    if !ws.root_path.trim().is_empty()
                        && std::path::Path::new(&ws.root_path).is_dir()
                    {
                        Some(std::path::PathBuf::from(ws.root_path))
                    } else if let Ok(cwd) = std::env::current_dir() {
                        let _ = db.update_workspace_root_path(ws.id, &cwd.to_string_lossy());
                        Some(cwd)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });

        let active_epoch = match db.with(|db| db.get_active_context_epoch(session_id)) {
            Ok(epoch) => epoch,
            Err(error) => {
                publish_error(&bus, session_id, None, &error.to_string());
                coordinator.finish_drain(session_id);
                break;
            }
        };

        let epoch = match active_epoch {
            Some(epoch) => epoch,
            None => {
                let source = crate::conversation::context::InstructionSource::new(&workspace_dir);
                let payload = match source.load() {
                    Ok(payload) => payload,
                    Err(crate::conversation::context::InstructionError::Unavailable(error)) => {
                        let msg = format!("instruction source unavailable: {error}");
                        let _ = db.with(|db| db.set_session_last_error(session_id, Some(&msg)));
                        publish_error(&bus, session_id, None, &msg);
                        coordinator.finish_drain(session_id);
                        break;
                    }
                };

                let config =
                    coordinator
                        .session_config(session_id)
                        .unwrap_or_else(|| SessionConfig {
                            agent_mode: "plan".to_string(),
                            provider: "default".to_string(),
                            model: "default".to_string(),
                        });
                let mode = if config.agent_mode.eq_ignore_ascii_case("build") {
                    crate::workspace::Mode::Build
                } else {
                    crate::workspace::Mode::Plan
                };
                let composer = crate::conversation::prompt::SystemPromptComposer::new(
                    &config.model,
                    &config.provider,
                    workspace_dir.clone(),
                    mode,
                )
                .with_project_instructions(&payload.aggregate_text);
                let baseline_system_text = composer.compose();
                let epoch_id = "epoch-1";

                match writer.insert_context_epoch(
                    session_id,
                    epoch_id,
                    &baseline_system_text,
                    &payload.snapshot_json,
                ) {
                    Ok(epoch) => epoch,
                    Err(error) => {
                        publish_error(&bus, session_id, None, &error);
                        coordinator.finish_drain(session_id);
                        break;
                    }
                }
            }
        };

        let (promoted_input, user_message, promo_seq) = match writer.promote_input(next_input.id) {
            Ok(result) => result,
            Err(error) => {
                publish_error(&bus, session_id, None, &error);
                coordinator.finish_drain(session_id);
                break;
            }
        };
        bus.publish(RuntimeEvent {
            seq: promo_seq,
            session_id,
            generation_id: None,
            kind: "prompt_promoted".to_string(),
            payload_json: serde_json::json!({
                "input_id": promoted_input.id,
                "session_id": session_id,
                "user_message_id": user_message.id,
            })
            .to_string(),
        });

        // Reconcile instruction source changes at safe boundary before generation request.
        let source = crate::conversation::context::InstructionSource::new(&workspace_dir);
        if let Ok(current_payload) = source.load()
            && current_payload.snapshot_json != epoch.source_snapshot_json
        {
            let delta_message = if current_payload.aggregate_text.trim().is_empty() {
                "# Updated Project Instructions\n\nProject instructions were removed.".to_string()
            } else {
                format!(
                    "# Updated Project Instructions\n\n{}",
                    current_payload.aggregate_text.trim()
                )
            };
            if let Err(error) = writer.reconcile_epoch_change(
                session_id,
                &epoch.epoch_id,
                &current_payload.snapshot_json,
                &delta_message,
            ) {
                publish_error(&bus, session_id, None, &error);
            }
        }

        let config = coordinator
            .session_config(session_id)
            .unwrap_or_else(|| SessionConfig {
                agent_mode: "plan".to_string(),
                provider: "default".to_string(),
                model: "default".to_string(),
            });

        let generation = match db.with(|db| {
            db.start_generation(
                session_id,
                &config.agent_mode,
                &config.provider,
                &config.model,
            )
        }) {
            Ok(generation) => generation,
            Err(error) => {
                publish_error(&bus, session_id, None, &error.to_string());
                coordinator.finish_drain(session_id);
                break;
            }
        };

        coordinator.set_active_generation(session_id, generation.id);
        if coordinator.is_cleaning_up(session_id) {
            let _ = db.with(|db| db.request_cancel(generation.id));
        }

        let ctx = SharedGenerationCtx {
            db: db.clone(),
            writer: writer.clone(),
            bus: bus.clone(),
            coordinator: Arc::clone(&coordinator),
            active_tool_call: Arc::new(Mutex::new(None)),
            session_id,
            generation_id: generation.id,
        };

        let gen_provider = Arc::clone(&provider);
        let gen_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_generation(
                &ctx,
                gen_provider.as_ref(),
                &config.model,
                &promoted_input.content,
                &config.agent_mode,
                &config.provider,
            );
        }));

        if gen_result.is_err() {
            tracing::error!(
                session_id,
                generation_id = ctx.generation_id,
                "generation thread panicked"
            );
            let message = match ctx.fail_active_tool("generation thread panicked") {
                Ok(()) => "generation thread panicked".to_string(),
                Err(error) => {
                    format!("generation thread panicked; durable call may remain running: {error}")
                }
            };
            ctx.fail_generation_state(&message);
        }

        ctx.deactivate();

        if coordinator.is_cleaning_up(session_id) {
            coordinator.finish_interrupt(session_id);
            break;
        }

        let should_continue = coordinator.complete_turn(session_id);
        let has_more = db
            .with(|db| db.has_pending_inputs(session_id))
            .unwrap_or(false);

        if !should_continue || !has_more {
            coordinator.finish_drain(session_id);
            break;
        }
    }
}

/// Shared per-generation context: log-persisted emits with committed seqs.
#[derive(Clone)]
struct SharedGenerationCtx {
    db: SharedDb,
    writer: WriterHandle,
    bus: EventBus,
    coordinator: Arc<SessionCoordinator>,
    active_tool_call: Arc<Mutex<Option<i64>>>,
    session_id: i64,
    generation_id: i64,
}

impl SharedGenerationCtx {
    fn emit(&self, kind: &str, payload: &serde_json::Value) {
        let payload_json = payload.to_string();
        let seq = self
            .writer
            .append_event(
                self.session_id,
                Some(self.generation_id),
                kind,
                &payload_json,
            )
            .ok()
            .unwrap_or(0);
        self.publish(seq, kind, payload_json);
    }

    fn fail_generation_state(&self, message: &str) {
        self.db.last_error(self.session_id, message);
        let _ = self
            .db
            .with(|db| db.finish_generation(self.generation_id, GenerationStatus::Failed, None));
        self.emit_error(message);
        self.emit_status("failed", None);
    }

    fn set_active_tool_call(&self, id: i64) {
        *self
            .active_tool_call
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(id);
    }

    fn clear_active_tool_call(&self) {
        *self
            .active_tool_call
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }

    fn fail_active_tool(&self, message: &str) -> Result<(), String> {
        let id = self
            .active_tool_call
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let Some(id) = id else {
            return Ok(());
        };
        let (call, seq) =
            self.writer
                .settle_tool_call(id, ToolCallStatus::Failed, None, Some(message))?;
        self.publish(
            seq,
            "tool_call_settled",
            tool_call_event_payload(&call, None, Some(message)),
        );
        Ok(())
    }

    fn publish(&self, seq: i64, kind: &str, payload_json: String) {
        self.bus.publish(RuntimeEvent {
            seq,
            session_id: self.session_id,
            generation_id: Some(self.generation_id),
            kind: kind.to_string(),
            payload_json,
        });
    }

    fn emit_status(&self, status: &str, finish_reason: Option<FinishReason>) {
        let mut payload = serde_json::json!({ "status": status });
        if let Some(reason) = finish_reason {
            payload["finish_reason"] = serde_json::Value::String(format!("{reason:?}"));
        }
        self.emit("generation_finished", &payload);
    }

    fn emit_error(&self, message: &str) {
        self.emit("error", &serde_json::json!({ "message": message }));
    }

    /// Clear the active generation from the coordinator.
    fn deactivate(&self) {
        self.coordinator
            .clear_active_generation(self.session_id, self.generation_id);
    }
}

enum LocalToolOutcome {
    Completed(String),
    Failed(String),
    Cancelled,
}
fn bound_tool_text(mut value: String) -> String {
    if value.len() <= MAX_TOOL_OUTPUT_BYTES {
        return value;
    }
    let mut end = MAX_TOOL_OUTPUT_BYTES;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

/// Run one provider turn, streaming deltas onto the bus and persisting the
/// final assistant message. Honours a `cancelling` status between events.
fn run_generation(
    ctx: &SharedGenerationCtx,
    provider: &dyn Provider,
    model: &str,
    prompt: &str,
    agent_mode: &str,
    provider_name: &str,
) {
    ctx.emit(
        "generation_started",
        &serde_json::json!({
            "agent_mode": agent_mode,
            "provider": provider_name,
            "model": model,
        }),
    );

    let workspace_dir = ctx
        .db
        .with(|db| {
            let session = db.session(ctx.session_id).ok().flatten();
            let ws = session.and_then(|s| db.workspace(s.workspace_id).ok().flatten());
            if let Some(ws) = ws {
                if !ws.root_path.trim().is_empty() && std::path::Path::new(&ws.root_path).is_dir() {
                    Some(std::path::PathBuf::from(ws.root_path))
                } else if let Ok(cwd) = std::env::current_dir() {
                    let _ = db.update_workspace_root_path(ws.id, &cwd.to_string_lossy());
                    Some(cwd)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });

    let mode = if agent_mode.eq_ignore_ascii_case("build") {
        crate::workspace::Mode::Build
    } else {
        crate::workspace::Mode::Plan
    };

    let active_epoch = ctx
        .db
        .with(|db| db.get_active_context_epoch(ctx.session_id))
        .ok()
        .flatten();

    let system_prompt = if let Some(epoch) = active_epoch {
        epoch.baseline_system_text
    } else {
        let composer = crate::conversation::prompt::SystemPromptComposer::new(
            model,
            provider_name,
            workspace_dir.clone(),
            mode,
        );
        composer.compose()
    };

    let mut raw_history = ctx
        .db
        .with(|db| db.messages(ctx.session_id))
        .unwrap_or_default();

    let n = raw_history.len();
    if n >= 2
        && raw_history[n - 1].role == "system"
        && raw_history[n - 2].role == "user"
        && raw_history[n - 2].content == prompt
    {
        raw_history.swap(n - 2, n - 1);
    }

    let history: Vec<crate::provider::ChatMessage> = raw_history
        .into_iter()
        .filter(|m| m.role == "user" || m.role == "assistant" || m.role == "system")
        .map(|m| crate::provider::ChatMessage {
            role: m.role,
            content: m.content,
            tool_call_id: None,
            tool_calls: None,
            name: None,
        })
        .collect();

    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(crate::provider::ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
        tool_call_id: None,
        tool_calls: None,
        name: None,
    });
    messages.extend(history);

    if messages
        .last()
        .map(|m| (m.role.as_str(), m.content.as_str()))
        != Some(("user", prompt))
    {
        if persist_message(ctx, "user", prompt).is_none() {
            ctx.deactivate();
            return;
        }
        messages.push(crate::provider::ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
            tool_call_id: None,
            tool_calls: None,
            name: None,
        });
    }

    let tools = crate::conversation::tools::coding_tools_schemas();
    const MAX_AGENT_TURNS: usize = 25;
    let mut total_usage = crate::provider::Usage {
        input_tokens: 0,
        output_tokens: 0,
    };
    let mut final_finish_reason = None;
    let mut empty_turn_retries = 0usize;

    for turn in 0..MAX_AGENT_TURNS {
        if cancel_requested(&ctx.db, ctx.generation_id) {
            break;
        }

        let request = StreamRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            max_output_tokens: MAX_OUTPUT_TOKENS,
            messages: messages.clone(),
            provider: Some(provider_name.to_string()),
            tools: tools.clone(),
        };

        let mut stream = match provider.stream(&request) {
            Ok(stream) => stream,
            Err(error) => {
                ctx.db.last_error(ctx.session_id, &error.to_string());
                fail_generation(ctx, &error.to_string());
                ctx.deactivate();
                return;
            }
        };

        let mut text = String::new();
        let mut turn_tool_calls: Vec<(String, String, String)> = Vec::new();
        let mut completed_tool_call_ids: Vec<String> = Vec::new();
        let mut malformed_tool_stream = false;
        let mut provider_tool_results: Vec<(String, String)> = Vec::new();
        let mut failed = false;
        let mut provider_cancelled = false;

        for event in &mut stream {
            if cancel_requested(&ctx.db, ctx.generation_id) {
                break;
            }
            match event {
                StreamEvent::TextDelta(delta) => {
                    text.push_str(&delta);
                    ctx.emit("text_delta", &serde_json::json!({ "delta": delta }));
                }
                StreamEvent::ReasoningDelta(delta) => {
                    ctx.emit("reasoning_delta", &serde_json::json!({ "delta": delta }));
                }
                StreamEvent::ToolCallStart { id, name } => {
                    if turn_tool_calls.iter().any(|(call_id, _, _)| call_id == &id) {
                        malformed_tool_stream = true;
                    } else {
                        turn_tool_calls.push((id, name, String::new()));
                    }
                }
                StreamEvent::ToolCallDelta { id, arguments } => {
                    if completed_tool_call_ids.contains(&id) {
                        malformed_tool_stream = true;
                    } else if let Some(call) =
                        turn_tool_calls.iter_mut().find(|(cid, _, _)| cid == &id)
                    {
                        if call.2.len().saturating_add(arguments.len()) > MAX_TOOL_OUTPUT_BYTES {
                            malformed_tool_stream = true;
                        } else {
                            call.2.push_str(&arguments);
                        }
                    } else {
                        malformed_tool_stream = true;
                    }
                }
                StreamEvent::ToolCallEnd { id } => {
                    if !turn_tool_calls.iter().any(|(call_id, _, _)| call_id == &id)
                        || completed_tool_call_ids.contains(&id)
                    {
                        malformed_tool_stream = true;
                    } else {
                        completed_tool_call_ids.push(id);
                    }
                }
                StreamEvent::ToolResult { id, result } => {
                    provider_tool_results.push((id, result));
                }
                StreamEvent::Usage(value) => {
                    total_usage.input_tokens += value.input_tokens;
                    total_usage.output_tokens += value.output_tokens;
                    ctx.emit(
                        "usage",
                        &serde_json::json!({
                            "input_tokens": total_usage.input_tokens,
                            "output_tokens": total_usage.output_tokens,
                        }),
                    );
                }
                StreamEvent::Finish { reason } => {
                    final_finish_reason = Some(reason);
                    ctx.emit(
                        "finish",
                        &serde_json::json!({ "reason": format!("{reason:?}") }),
                    );
                }
                StreamEvent::Cancelled => {
                    provider_cancelled = true;
                    break;
                }
                StreamEvent::Error(message) => {
                    ctx.emit_error(&message);
                    failed = true;
                }
            }
        }
        if cancel_requested(&ctx.db, ctx.generation_id) || provider_cancelled {
            if !text.is_empty() && persist_message(ctx, "assistant", &text).is_none() {
                ctx.deactivate();
                return;
            }
            ctx.db.clear_last_error(ctx.session_id);
            let _ = ctx.db.with(|db| {
                db.finish_generation(ctx.generation_id, GenerationStatus::Cancelled, None)
            });
            ctx.emit_status("cancelled", None);
            ctx.deactivate();
            return;
        }

        if failed {
            ctx.db.last_error(ctx.session_id, "stream error");
            fail_generation(ctx, "stream error");
            ctx.deactivate();
            return;
        }

        if !turn_tool_calls.is_empty() {
            let complete_count = turn_tool_calls
                .iter()
                .filter(|(call_id, _, _)| completed_tool_call_ids.contains(call_id))
                .count();
            if malformed_tool_stream || turn_tool_calls.len() != 1 || complete_count != 1 {
                let message = "first slice requires exactly one complete local tool call";
                ctx.db.last_error(ctx.session_id, message);
                fail_generation(ctx, message);
                ctx.deactivate();
                return;
            }
        }

        if turn_tool_calls.is_empty() {
            if !text.trim().is_empty() {
                if persist_message(ctx, "assistant", &text).is_none() {
                    ctx.deactivate();
                    return;
                }
                ctx.emit("assistant_message", &serde_json::json!({ "content": text }));
                break;
            }

            if turn > 0 && empty_turn_retries < 2 {
                empty_turn_retries += 1;
                messages.push(crate::provider::ChatMessage {
                    role: "user".to_string(),
                    content: "Please provide your response and summarize your findings or code changes based on the tool results above.".to_string(),
                    tool_call_id: None,
                    tool_calls: None,
                    name: None,
                });
                continue;
            }

            break;
        }

        // Persist owning assistant row before any tool identity or side effect.
        let assistant_message = match persist_message(ctx, "assistant", &text) {
            Some(message) => message,
            None => {
                ctx.deactivate();
                return;
            }
        };
        ctx.emit(
            "assistant_message",
            &serde_json::json!({ "content": text, "assistant_message_id": assistant_message.id }),
        );

        for (call_id, tool_name, args_str) in &turn_tool_calls {
            ctx.publish(
                0,
                "tool_call_start",
                serde_json::json!({
                    "id": call_id,
                    "name": tool_name,
                    "assistant_message_id": assistant_message.id,
                })
                .to_string(),
            );
            ctx.publish(
                0,
                "tool_call_delta",
                serde_json::json!({
                    "id": call_id,
                    "arguments": args_str,
                    "assistant_message_id": assistant_message.id,
                })
                .to_string(),
            );
            ctx.publish(
                0,
                "tool_call_end",
                serde_json::json!({
                    "id": call_id,
                    "name": tool_name,
                    "arguments_complete": true,
                    "assistant_message_id": assistant_message.id,
                })
                .to_string(),
            );
        }
        for (call_id, result) in provider_tool_results {
            ctx.publish(
                0,
                "tool_result",
                serde_json::json!({
                    "id": call_id,
                    "result": result,
                    "assistant_message_id": assistant_message.id,
                })
                .to_string(),
            );
        }

        let Some((call_id, tool_name, args_str)) = turn_tool_calls.into_iter().next() else {
            fail_generation(ctx, "provider returned no complete local tool call");
            ctx.deactivate();
            return;
        };
        let (created_call, created_seq) = match ctx.writer.create_tool_call(
            ctx.session_id,
            ctx.generation_id,
            assistant_message.id,
            &call_id,
            &tool_name,
            &args_str,
        ) {
            Ok(value) => value,
            Err(error) => {
                fail_generation(ctx, &error);
                ctx.deactivate();
                return;
            }
        };
        ctx.set_active_tool_call(created_call.id);
        ctx.publish(
            created_seq,
            "tool_call_created",
            tool_call_event_payload(&created_call, None, None),
        );
        let (running_call, running_seq) = match ctx.writer.start_tool_call(created_call.id) {
            Ok(value) => value,
            Err(error) => {
                let mut message = format!("tool start failed before execution: {error}");
                match ctx.writer.settle_tool_call(
                    created_call.id,
                    ToolCallStatus::Failed,
                    None,
                    Some(&message),
                ) {
                    Ok((failed_call, failed_seq)) => ctx.publish(
                        failed_seq,
                        "tool_call_settled",
                        tool_call_event_payload(&failed_call, None, Some(&message)),
                    ),
                    Err(settle_error) => {
                        message.push_str(&format!(
                            "; durable call may remain created: {settle_error}"
                        ));
                    }
                }
                ctx.clear_active_tool_call();
                fail_generation(ctx, &message);
                ctx.deactivate();
                return;
            }
        };
        ctx.publish(
            running_seq,
            "tool_call_started",
            tool_call_event_payload(&running_call, None, None),
        );

        let effective_ws_dir = if workspace_dir.as_os_str().is_empty() || !workspace_dir.exists() {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            workspace_dir.clone()
        };
        let child_db = ctx.db.clone();
        let child_generation_id = ctx.generation_id;
        let child_tool_name = tool_name.clone();
        let child_args = args_str.clone();
        let child = thread::Builder::new()
            .name("clawcode-local-tool".to_string())
            .spawn(move || {
                if cancel_requested(&child_db, child_generation_id) {
                    return LocalToolOutcome::Cancelled;
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match crate::workspace::Workspace::with_filesystem(
                        &effective_ws_dir,
                        crate::workspace::RealFileSystem,
                    ) {
                        Ok(ws) => crate::conversation::tools::execute_tool(
                            &ws,
                            mode,
                            &child_tool_name,
                            &child_args,
                        ),
                        Err(error) => Err(format!(
                            "Failed to open workspace {}: {error}",
                            effective_ws_dir.display()
                        )),
                    }
                }));
                match result {
                    Ok(Ok(output)) => LocalToolOutcome::Completed(output),
                    Ok(Err(error)) => LocalToolOutcome::Failed(error),
                    Err(_) => LocalToolOutcome::Failed("local tool thread panicked".to_string()),
                }
            });
        let outcome = match child {
            Ok(handle) => match handle.join() {
                Ok(outcome) => outcome,
                Err(_) => LocalToolOutcome::Failed("local tool thread panicked".to_string()),
            },
            Err(error) => LocalToolOutcome::Failed(format!("failed to spawn local tool: {error}")),
        };
        let outcome = if cancel_requested(&ctx.db, ctx.generation_id) {
            LocalToolOutcome::Cancelled
        } else {
            outcome
        };

        let (settlement_status, result, error) = match outcome {
            LocalToolOutcome::Completed(output) => (
                ToolCallStatus::Completed,
                Some(bound_tool_text(output)),
                None,
            ),
            LocalToolOutcome::Failed(error) => {
                (ToolCallStatus::Failed, None, Some(bound_tool_text(error)))
            }
            LocalToolOutcome::Cancelled => (ToolCallStatus::Cancelled, None, None),
        };
        let (settled_call, settled_seq) = match ctx.writer.settle_tool_call(
            created_call.id,
            settlement_status,
            result.as_deref(),
            error.as_deref(),
        ) {
            Ok(value) => {
                ctx.clear_active_tool_call();
                value
            }
            Err(error) => {
                ctx.clear_active_tool_call();
                let message =
                    format!("tool settlement failed; durable call may remain running: {error}");
                ctx.db.last_error(ctx.session_id, &message);
                fail_generation(ctx, &message);
                ctx.deactivate();
                return;
            }
        };
        ctx.publish(
            settled_seq,
            "tool_call_settled",
            tool_call_event_payload(&settled_call, result.as_deref(), error.as_deref()),
        );
        let projected_content = result.as_deref().or(error.as_deref()).unwrap_or("");
        if persist_message(ctx, "tool", projected_content).is_none() {
            ctx.deactivate();
            return;
        }
        let _reloaded_messages = ctx.db.with(|db| db.messages(ctx.session_id));
        let success = settled_call.status == ToolCallStatus::Completed;
        let output = result.as_deref().or(error.as_deref()).unwrap_or("");
        ctx.emit(
            "tool_executed",
            &serde_json::json!({
                "id": call_id,
                "name": tool_name,
                "arguments": serde_json::from_str::<serde_json::Value>(&args_str)
                    .unwrap_or_else(|_| serde_json::Value::String(args_str.clone())),
                "success": success,
                "output": output,
                "assistant_message_id": assistant_message.id,
            }),
        );
        if settled_call.status == ToolCallStatus::Cancelled {
            ctx.db.clear_last_error(ctx.session_id);
            let _ = ctx.db.with(|db| {
                db.finish_generation(ctx.generation_id, GenerationStatus::Cancelled, None)
            });
            ctx.emit_status("cancelled", None);
            ctx.deactivate();
            return;
        }
        if settled_call.status == ToolCallStatus::Failed {
            let message = error.as_deref().unwrap_or("local tool failed");
            ctx.db.last_error(ctx.session_id, message);
            fail_generation(ctx, message);
            ctx.deactivate();
            return;
        }
        ctx.emit(
            "agent_turn",
            &serde_json::json!({
                "turn": turn + 1,
                "assistant_message_id": assistant_message.id,
            }),
        );
        messages.push(crate::provider::ChatMessage {
            role: "assistant".to_string(),
            content: text.clone(),
            tool_call_id: None,
            tool_calls: Some(vec![crate::provider::ToolCall {
                id: call_id.clone(),
                name: tool_name.clone(),
                arguments: args_str.clone(),
            }]),
            name: None,
        });
        messages.push(crate::provider::ChatMessage {
            role: "tool".to_string(),
            content: projected_content.to_string(),
            tool_call_id: Some(call_id.clone()),
            tool_calls: None,
            name: Some(tool_name.clone()),
        });
        continue;
    }

    if cancel_requested(&ctx.db, ctx.generation_id) {
        ctx.db.clear_last_error(ctx.session_id);
        let _ = ctx
            .db
            .with(|db| db.finish_generation(ctx.generation_id, GenerationStatus::Cancelled, None));
        ctx.emit_status("cancelled", None);
        ctx.deactivate();
        return;
    }

    let metrics_json = if total_usage.input_tokens > 0 || total_usage.output_tokens > 0 {
        Some(
            serde_json::json!({
                "input_tokens": total_usage.input_tokens,
                "output_tokens": total_usage.output_tokens,
                "finish_reason": format!("{:?}", final_finish_reason.unwrap_or(FinishReason::Stop)),
            })
            .to_string(),
        )
    } else {
        None
    };

    ctx.db.clear_last_error(ctx.session_id);
    let _ = ctx.db.with(|db| {
        db.finish_generation(
            ctx.generation_id,
            GenerationStatus::Completed,
            metrics_json.as_deref(),
        )
    });
    ctx.emit_status(
        "completed",
        Some(final_finish_reason.unwrap_or(FinishReason::Stop)),
    );
    ctx.deactivate();
}

fn cancel_requested(db: &SharedDb, generation_id: i64) -> bool {
    db.with(|db| db.generation_status(generation_id))
        .ok()
        .flatten()
        .is_some_and(|status| status == GenerationStatus::Cancelling)
}

fn tool_call_event_payload(
    tool_call: &crate::persistence::ToolCall,
    result: Option<&str>,
    error: Option<&str>,
) -> String {
    serde_json::json!({
        "tool_call_id": tool_call.id,
        "call_id": tool_call.call_id,
        "tool_name": tool_call.tool_name,
        "arguments": serde_json::from_str::<serde_json::Value>(&tool_call.arguments)
            .unwrap_or_else(|_| serde_json::Value::String(tool_call.arguments.clone())),
        "assistant_message_id": tool_call.assistant_message_id,
        "generation_id": tool_call.generation_id,
        "status": tool_call.status.as_str(),
        "result": result,
        "error": error,
    })
    .to_string()
}

fn persist_message(
    ctx: &SharedGenerationCtx,
    role: &str,
    content: &str,
) -> Option<crate::persistence::Message> {
    match ctx.writer.append_message(ctx.session_id, role, content) {
        Ok(message) => Some(message),
        Err(error) => {
            ctx.db.last_error(ctx.session_id, &error);
            fail_generation(ctx, &error);
            None
        }
    }
}

fn fail_generation(ctx: &SharedGenerationCtx, message: &str) {
    let _ = ctx
        .db
        .with(|db| db.finish_generation(ctx.generation_id, GenerationStatus::Failed, None));
    ctx.emit_error(message);
    ctx.emit_status("failed", None);
}

/// Bus-only status ping (`seq = 0`, not persisted) for control acks.
fn publish_status(bus: &EventBus, session_id: i64, generation_id: Option<i64>, status: &str) {
    bus.publish(RuntimeEvent {
        seq: 0,
        session_id,
        generation_id,
        kind: "generation_status".to_string(),
        payload_json: serde_json::json!({ "status": status }).to_string(),
    });
}

fn publish_error(bus: &EventBus, session_id: i64, generation_id: Option<i64>, message: &str) {
    bus.publish(RuntimeEvent {
        seq: 0,
        session_id,
        generation_id,
        kind: "error".to_string(),
        payload_json: serde_json::json!({ "message": message }).to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        ModelInfo, ProviderCapabilities, ProviderError, ProviderId, StreamResponse,
    };

    #[derive(Debug)]
    struct SlowProvider;

    impl Provider for SlowProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::LazyLock<ProviderId> =
                std::sync::LazyLock::new(|| ProviderId::new("fake"));
            &ID
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                streaming: true,
                tools: false,
            }
        }
        fn models(&self) -> Vec<ModelInfo> {
            Vec::new()
        }
        fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
            Ok(StreamResponse { events: vec![] })
        }
        fn stream(
            &self,
            _request: &StreamRequest,
        ) -> Result<crate::provider::ProviderStream, ProviderError> {
            let (sender, stream) = crate::provider::ProviderStream::channel(16);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = sender.send(StreamEvent::TextDelta("delayed text".into()));
                let _ = sender.flush();
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = sender.send(StreamEvent::Finish {
                    reason: FinishReason::Stop,
                });
                let _ = sender.flush();
            });
            Ok(stream)
        }
    }

    #[derive(Debug)]
    struct TextProvider;

    impl Provider for TextProvider {
        fn id(&self) -> &ProviderId {
            static ID: std::sync::LazyLock<ProviderId> =
                std::sync::LazyLock::new(|| ProviderId::new("fake"));
            &ID
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                streaming: true,
                tools: false,
            }
        }
        fn models(&self) -> Vec<ModelInfo> {
            Vec::new()
        }
        fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
            Ok(StreamResponse {
                events: vec![
                    StreamEvent::TextDelta("persist me".into()),
                    StreamEvent::Finish {
                        reason: FinishReason::Stop,
                    },
                ],
            })
        }
    }

    #[test]
    fn failed_assistant_persistence_fails_generation() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("clawcode-test-persist-fail-{nonce}.db"));
        let db = Db::open(&path).unwrap();
        let session = db.create_session("persistence_failure").unwrap();
        let session_id = session.id;
        let bus = EventBus::new();
        let (_sub_id, rx) = bus.subscribe(Some(session_id));
        let writer = WriterHandle::spawn(Db::open(&path).unwrap());
        // Force assistant message append to fail via trigger
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_assistant BEFORE INSERT ON messages
                 WHEN NEW.role = 'assistant'
                 BEGIN
                     SELECT RAISE(ABORT, 'forced assistant failure');
                 END;",
            )
            .unwrap();
        let client = RuntimeClient::spawn(db, writer, Box::new(TextProvider), bus);

        client
            .start_generation(session_id, "plan", "fake", "text", "hello")
            .unwrap();

        let start = std::time::Instant::now();
        let mut finished = None;
        while start.elapsed() < std::time::Duration::from_secs(2) {
            if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
                && event.kind == "generation_finished"
            {
                finished = Some(event);
                break;
            }
        }

        let finished = finished.expect("generation must emit terminal status");
        let generation_id = finished.generation_id.expect("generation id missing");
        assert!(finished.payload_json.contains("failed"));
        assert!(!finished.payload_json.contains("completed"));

        let db = client.shutdown();
        assert_eq!(
            db.generation_status(generation_id).unwrap(),
            Some(GenerationStatus::Failed)
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cancel_generation_marks_cancelled_not_completed() {
        let path = std::env::temp_dir().join(format!(
            "clawcode-test-client-cancel-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Db::open(&path).unwrap();
        let session = db.create_session("cancel_test").unwrap();
        let session_id = session.id;
        let bus = EventBus::new();
        let (_sub_id, rx) = bus.subscribe(Some(session_id));
        let writer_db = Db::open(&path).unwrap();
        let writer = WriterHandle::spawn(writer_db);
        let client = RuntimeClient::spawn(db, writer, Box::new(SlowProvider), bus);

        client
            .start_generation(session_id, "plan", "fake", "slow", "hello")
            .unwrap();
        client.cancel_generation(session_id).unwrap();

        let mut got_cancelled = false;
        let mut got_completed = false;
        let mut target_generation_id = None;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(2) {
            if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
                && event.kind == "generation_finished"
            {
                target_generation_id = event.generation_id;
                if event.payload_json.contains("cancelled") {
                    got_cancelled = true;
                }
                if event.payload_json.contains("completed") {
                    got_completed = true;
                }
                break;
            }
        }

        assert!(got_cancelled, "generation should emit cancelled event");
        assert!(
            !got_completed,
            "generation must never emit completed when cancelled"
        );

        let db = client.shutdown();
        let gid = target_generation_id.expect("must have generation id");
        assert_eq!(
            db.generation_status(gid).unwrap(),
            Some(GenerationStatus::Cancelled)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn concurrent_generation_on_same_session_is_coalesced_and_serialized() {
        let path = std::env::temp_dir().join(format!(
            "clawcode-test-client-concurrent-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Db::open(&path).unwrap();
        let session = db.create_session("concurrent_test").unwrap();
        let session_id = session.id;
        let bus = EventBus::new();
        let (_sub_id, rx) = bus.subscribe(Some(session_id));
        let writer_db = Db::open(&path).unwrap();
        let writer = WriterHandle::spawn(writer_db);
        let client = RuntimeClient::spawn(db, writer, Box::new(SlowProvider), bus);

        client
            .start_generation(session_id, "plan", "fake", "slow", "turn 1")
            .unwrap();
        client
            .start_generation(session_id, "plan", "fake", "slow", "turn 2")
            .unwrap();

        let mut finished_count = 0;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(5) {
            if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
                && event.kind == "generation_finished"
            {
                finished_count += 1;
                if finished_count >= 2 {
                    break;
                }
            }
        }
        assert_eq!(finished_count, 2, "both turns should finish sequentially");
        let db = client.shutdown();
        let messages = db.messages(session_id).unwrap();
        let user_messages: Vec<_> = messages.iter().filter(|m| m.role == "user").collect();
        assert_eq!(user_messages.len(), 2);
        assert_eq!(user_messages[0].content, "turn 1");
        assert_eq!(user_messages[1].content, "turn 2");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn send_after_shutdown_returns_err() {
        let client = RuntimeClient {
            sender: None,
            worker: None,
            coordinator: Arc::new(SessionCoordinator::new()),
            db: None,
            bus: EventBus::new(),
        };
        let res = client.create_session(1, "test");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("client shut down"));
    }

    #[test]
    fn panic_cleanup_settles_active_tool_call_as_failed() {
        let db = Db::open_in_memory().unwrap();
        let session = db.create_session("panic_test").unwrap();
        let generation = db
            .start_generation(session.id, "plan", "fake", "model")
            .unwrap();
        let writer = WriterHandle::spawn(db);
        let assistant = writer.append_message(session.id, "assistant", "").unwrap();
        let (created, _) = writer
            .create_tool_call(
                session.id,
                generation.id,
                assistant.id,
                "call-panic",
                "read_file",
                "{}",
            )
            .unwrap();
        let (running, _) = writer.start_tool_call(created.id).unwrap();

        let bus = EventBus::new();
        let (_sub, rx) = bus.subscribe(Some(session.id));
        let coordinator = Arc::new(SessionCoordinator::new());
        coordinator.wake(session.id);
        coordinator.set_active_generation(session.id, generation.id);
        let dummy_db = Db::open_in_memory().unwrap();
        let ctx = SharedGenerationCtx {
            db: SharedDb::new(Arc::new(Mutex::new(dummy_db))),
            writer: writer.clone(),
            bus,
            coordinator,
            active_tool_call: Arc::new(Mutex::new(Some(running.id))),
            session_id: session.id,
            generation_id: generation.id,
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            panic!("simulated generation panic");
        }));
        assert!(result.is_err());
        assert!(ctx.fail_active_tool("generation thread panicked").is_ok());

        let db = writer.shutdown().unwrap();
        let tool_call = db.tool_call(running.id).unwrap().unwrap();
        assert_eq!(tool_call.status, ToolCallStatus::Failed);
        assert_eq!(
            tool_call.error.as_deref(),
            Some("generation thread panicked")
        );
        let event = rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .unwrap();
        assert_eq!(event.kind, "tool_call_settled");
    }
}
