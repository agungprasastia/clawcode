//! Client boundary to the runtime worker. Commands go in over a bounded
//! channel; events come back via the [`EventBus`]. Control commands run
//! inline on the worker thread; generations run on their own threads, so a
//! slow provider turn never blocks session creation or cancellation. The
//! worker thread never blocks on the network; SQLite is shared behind a
//! `Mutex` with short-lived accesses.

use super::{EventBus, RuntimeEvent};
use crate::persistence::{Db, GenerationStatus, WriterHandle};
use crate::provider::{FinishReason, Provider, StreamEvent, StreamRequest};
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
        let (sender, receiver) = mpsc::sync_channel::<ClientCommand>(CLIENT_CHANNEL_CAPACITY);
        let worker = thread::spawn(move || {
            worker_loop(
                SharedDb::new(db.clone()),
                writer,
                Arc::from(provider),
                bus,
                receiver,
            );
            db
        });
        Self {
            sender: Some(sender),
            worker: Some(worker),
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

    /// Stop the worker, wait for in-flight generations, take back the `Db`.
    pub fn shutdown(self) -> Db {
        drop(self.sender);
        let worker = self.worker.expect("client shut down twice");
        let db_arc = worker.join().expect("runtime worker thread panicked");
        Arc::into_inner(db_arc)
            .and_then(|mutex| mutex.into_inner().ok())
            .expect("runtime worker sole owner of db")
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
    receiver: mpsc::Receiver<ClientCommand>,
) {
    // (session_id, generation_id) per in-flight generation thread.
    let active: Arc<Mutex<Vec<(i64, i64)>>> = Default::default();
    let mut handles: Vec<thread::JoinHandle<()>> = Vec::new();

    while let Ok(command) = receiver.recv() {
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
                if active
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter()
                    .any(|(s, _)| *s == session_id)
                {
                    publish_error(
                        &bus,
                        session_id,
                        None,
                        "generation already in progress for this session",
                    );
                    continue;
                }
                let generation = match db
                    .with(|db| db.start_generation(session_id, &agent_mode, &provider_name, &model))
                {
                    Ok(generation) => generation,
                    Err(error) => {
                        publish_error(&bus, session_id, None, &error.to_string());
                        continue;
                    }
                };
                active
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((session_id, generation.id));
                let ctx = SharedGenerationCtx {
                    db: db.clone(),
                    writer: writer.clone(),
                    bus: bus.clone(),
                    active: Arc::clone(&active),
                    session_id,
                    generation_id: generation.id,
                };
                let generation_provider = provider.clone();
                handles.push(thread::spawn(move || {
                    // A panic must not leave the session stuck as "running":
                    // finalise the row, notify subscribers, then deactivate.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_generation(
                            &ctx,
                            generation_provider.as_ref(),
                            &model,
                            &prompt,
                            &agent_mode,
                            &provider_name,
                        );
                    }));
                    if result.is_err() {
                        tracing::error!(
                            session_id,
                            generation_id = ctx.generation_id,
                            "generation thread panicked"
                        );
                        let _ = ctx.db.with(|db| {
                            db.finish_generation(ctx.generation_id, GenerationStatus::Failed, None)
                        });
                        ctx.emit_status("failed");
                    }
                    ctx.deactivate();
                }));
            }
            ClientCommand::CancelGeneration { session_id } => {
                let generation_ids: Vec<i64> = active
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter()
                    .filter(|(active_session, _)| *active_session == session_id)
                    .map(|(_, generation_id)| *generation_id)
                    .collect();
                if generation_ids.is_empty() {
                    publish_status(&bus, session_id, None, "cancel_rejected");
                } else {
                    for generation_id in generation_ids {
                        match db.with(|db| db.request_cancel(generation_id)) {
                            Ok(true) => {}
                            Ok(false) => {
                                publish_status(&bus, session_id, Some(generation_id), "cancel_noop")
                            }
                            Err(error) => publish_error(
                                &bus,
                                session_id,
                                Some(generation_id),
                                &error.to_string(),
                            ),
                        }
                    }
                }
            }
        }
    }
    for handle in handles {
        if let Err(panic) = handle.join() {
            tracing::error!(?panic, "generation thread panicked");
        }
    }
    let _ = writer.shutdown();
}

/// Shared per-generation context: log-persisted emits with committed seqs.
struct SharedGenerationCtx {
    db: SharedDb,
    writer: WriterHandle,
    bus: EventBus,
    active: Arc<Mutex<Vec<(i64, i64)>>>,
    session_id: i64,
    generation_id: i64,
}

impl SharedGenerationCtx {
    /// Persist an event to the log and publish it with the committed seq.
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
        self.bus.publish(RuntimeEvent {
            seq,
            session_id: self.session_id,
            generation_id: Some(self.generation_id),
            kind: kind.to_string(),
            payload_json,
        });
    }

    fn emit_status(&self, status: &str) {
        self.emit(
            "generation_finished",
            &serde_json::json!({ "status": status }),
        );
    }

    fn emit_error(&self, message: &str) {
        self.emit("error", &serde_json::json!({ "message": message }));
    }

    /// Remove this generation from the active set.
    fn deactivate(&self) {
        self.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|&(session_id, generation_id)| {
                session_id != self.session_id || generation_id != self.generation_id
            });
    }
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

    let composer = crate::conversation::prompt::SystemPromptComposer::new(
        model,
        provider_name,
        workspace_dir.clone(),
        mode,
    );
    let system_prompt = composer.compose();

    let history: Vec<crate::provider::ChatMessage> = ctx
        .db
        .with(|db| db.messages(ctx.session_id))
        .unwrap_or_default()
        .into_iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
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
        let _ = ctx.writer.append(ctx.session_id, "user", prompt);
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
                    turn_tool_calls.push((id.clone(), name.clone(), String::new()));
                    ctx.emit(
                        "tool_call_start",
                        &serde_json::json!({ "id": id, "name": name }),
                    );
                }
                StreamEvent::ToolCallDelta { id, arguments } => {
                    if let Some(call) = turn_tool_calls.iter_mut().find(|(cid, _, _)| cid == &id) {
                        call.2.push_str(&arguments);
                    }
                    ctx.emit(
                        "tool_call_delta",
                        &serde_json::json!({ "id": id, "arguments": arguments }),
                    );
                }
                StreamEvent::ToolCallEnd { id } => {
                    ctx.emit("tool_call_end", &serde_json::json!({ "id": id }));
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
                _ => {}
            }
        }

        if cancel_requested(&ctx.db, ctx.generation_id) || provider_cancelled {
            if !text.is_empty() {
                let _ = ctx.writer.append(ctx.session_id, "assistant", &text);
            }
            ctx.db.clear_last_error(ctx.session_id);
            let _ = ctx.db.with(|db| {
                db.finish_generation(ctx.generation_id, GenerationStatus::Cancelled, None)
            });
            ctx.emit_status("cancelled");
            ctx.deactivate();
            return;
        }

        if failed {
            ctx.db.last_error(ctx.session_id, "stream error");
            fail_generation(ctx, "stream error");
            ctx.deactivate();
            return;
        }

        if turn_tool_calls.is_empty() {
            if !text.trim().is_empty() {
                let _ = ctx.writer.append(ctx.session_id, "assistant", &text);
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

        empty_turn_retries = 0;
        // Assistant called tools
        let tool_calls_json: Vec<serde_json::Value> = turn_tool_calls
            .iter()
            .map(|(id, name, args)| {
                serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": args,
                    }
                })
            })
            .collect();

        if !text.is_empty() {
            let _ = ctx.writer.append(ctx.session_id, "assistant", &text);
            ctx.emit("assistant_message", &serde_json::json!({ "content": text }));
        }

        messages.push(crate::provider::ChatMessage {
            role: "assistant".to_string(),
            content: text,
            tool_call_id: None,
            tool_calls: Some(tool_calls_json),
            name: None,
        });

        for (call_id, tool_name, args_str) in turn_tool_calls {
            let args_val: serde_json::Value = serde_json::from_str(&args_str)
                .unwrap_or_else(|_| serde_json::json!({ "raw": args_str }));
            ctx.emit(
                "tool_executing",
                &serde_json::json!({
                    "id": &call_id,
                    "name": &tool_name,
                    "arguments": &args_val,
                }),
            );

            let effective_ws_dir =
                if workspace_dir.as_os_str().is_empty() || !workspace_dir.exists() {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                } else {
                    workspace_dir.clone()
                };

            let result = match crate::workspace::Workspace::with_filesystem(
                &effective_ws_dir,
                crate::workspace::RealFileSystem,
            ) {
                Ok(ws) => {
                    crate::conversation::tools::execute_tool(&ws, mode, &tool_name, &args_str)
                }
                Err(e) => Err(format!(
                    "Failed to open workspace {}: {e}",
                    effective_ws_dir.display()
                )),
            };

            let (success, output) = match result {
                Ok(out) => (true, out),
                Err(err) => (false, format!("Error executing {tool_name}: {err}")),
            };

            ctx.emit(
                "tool_executed",
                &serde_json::json!({
                    "id": &call_id,
                    "name": &tool_name,
                    "arguments": &args_val,
                    "success": success,
                    "output": &output,
                }),
            );

            messages.push(crate::provider::ChatMessage {
                role: "tool".to_string(),
                content: output,
                tool_call_id: Some(call_id),
                tool_calls: None,
                name: Some(tool_name),
            });
        }

        ctx.emit("agent_turn", &serde_json::json!({ "turn": turn + 1 }));
    }

    if cancel_requested(&ctx.db, ctx.generation_id) {
        ctx.db.clear_last_error(ctx.session_id);
        let _ = ctx
            .db
            .with(|db| db.finish_generation(ctx.generation_id, GenerationStatus::Cancelled, None));
        ctx.emit_status("cancelled");
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
    ctx.emit_status("completed");
    ctx.deactivate();
}

fn cancel_requested(db: &SharedDb, generation_id: i64) -> bool {
    db.with(|db| db.generation_status(generation_id))
        .ok()
        .flatten()
        .is_some_and(|status| status == GenerationStatus::Cancelling)
}

fn fail_generation(ctx: &SharedGenerationCtx, message: &str) {
    let _ = ctx
        .db
        .with(|db| db.finish_generation(ctx.generation_id, GenerationStatus::Failed, None));
    ctx.emit_error(message);
    ctx.emit_status("failed");
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

    #[test]
    fn cancel_generation_marks_cancelled_not_completed() {
        let db = Db::open_in_memory().unwrap();
        let session = db.create_session("cancel_test").unwrap();
        let session_id = session.id;
        let bus = EventBus::new();
        let (_sub_id, rx) = bus.subscribe(Some(session_id));
        let writer = WriterHandle::spawn(Db::open_in_memory().unwrap());
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
                && event.kind == "generation_finished" {
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
    }

    #[test]
    fn concurrent_generation_on_same_session_is_rejected() {
        let db = Db::open_in_memory().unwrap();
        let session = db.create_session("concurrent_test").unwrap();
        let session_id = session.id;
        let bus = EventBus::new();
        let (_sub_id, rx) = bus.subscribe(Some(session_id));
        let writer = WriterHandle::spawn(Db::open_in_memory().unwrap());
        let client = RuntimeClient::spawn(db, writer, Box::new(SlowProvider), bus);

        client
            .start_generation(session_id, "plan", "fake", "slow", "turn 1")
            .unwrap();
        client
            .start_generation(session_id, "plan", "fake", "slow", "turn 2")
            .unwrap();

        let mut error_seen = false;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(1) {
            if let Ok(event) = rx.recv_timeout(std::time::Duration::from_millis(50))
                && event.kind == "error" && event.payload_json.contains("already in progress") {
                    error_seen = true;
                    break;
                }
        }
        assert!(
            error_seen,
            "concurrent generation on same session must be rejected"
        );
        let _ = client.shutdown();
    }

    #[test]
    fn send_after_shutdown_returns_err() {
        let client = RuntimeClient {
            sender: None,
            worker: None,
        };
        let res = client.create_session(1, "test");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("client shut down"));
    }
}
