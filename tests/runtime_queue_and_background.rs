use clawcode::persistence::{Db, ToolCallStatus, WriterHandle};
use clawcode::provider::{
    FinishReason, ModelInfo, Provider, ProviderCapabilities, ProviderError, ProviderId,
    ProviderStream, StreamEvent, StreamRequest, StreamResponse,
};
use clawcode::runtime::EventBus;
use clawcode::runtime::client::RuntimeClient;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

fn temp_db_path(tag: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "clawcode-runtime-qbg-{}-{tag}-{nonce}.db",
        std::process::id()
    ))
}

#[derive(Clone, Default)]
struct Gate {
    open: Arc<(Mutex<bool>, Condvar)>,
}

impl Gate {
    fn release(&self) {
        let (lock, cvar) = &*self.open;
        let mut opened = lock.lock().unwrap_or_else(|p| p.into_inner());
        *opened = true;
        cvar.notify_all();
    }
}

#[derive(Debug)]
struct GatedEchoProvider {
    first_turn_started: Arc<AtomicBool>,
    gate: Arc<(Mutex<bool>, Condvar)>,
    seen_prompts: Arc<Mutex<Vec<String>>>,
}

impl Provider for GatedEchoProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::LazyLock<ProviderId> =
            std::sync::LazyLock::new(|| ProviderId::new("gated"));
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
        Ok(StreamResponse { events: Vec::new() })
    }

    fn stream(&self, request: &StreamRequest) -> Result<ProviderStream, ProviderError> {
        let prompt = request.prompt.clone();
        self.seen_prompts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(prompt.clone());

        let is_first = !self.first_turn_started.swap(true, Ordering::AcqRel);
        let gate = Arc::clone(&self.gate);
        let (sender, stream) = ProviderStream::channel(16);
        std::thread::spawn(move || {
            if is_first {
                let (lock, cvar) = &*gate;
                let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                let _ = cvar.wait_timeout_while(guard, Duration::from_secs(5), |opened| !*opened);
            }
            let _ = sender.send(StreamEvent::TextDelta(format!("reply:{prompt}")));
            let _ = sender.flush();
            let _ = sender.send(StreamEvent::Finish {
                reason: FinishReason::Stop,
            });
            let _ = sender.flush();
        });
        Ok(stream)
    }
}

#[test]
fn submitting_while_active_queues_and_drains_in_order() {
    let path = temp_db_path("queue_order");
    let db = Db::open(&path).unwrap();
    let session = db.create_session("queue_order").unwrap();
    let session_id = session.id;
    let writer = WriterHandle::spawn(Db::open(&path).unwrap());
    let bus = EventBus::new();
    let (_sub, rx) = bus.subscribe(Some(session_id));

    let gate = Gate::default();
    let seen_prompts = Arc::new(Mutex::new(Vec::new()));
    let first_started = Arc::new(AtomicBool::new(false));
    let provider = GatedEchoProvider {
        first_turn_started: Arc::clone(&first_started),
        gate: Arc::clone(&gate.open),
        seen_prompts: Arc::clone(&seen_prompts),
    };

    let client = RuntimeClient::spawn(db, writer, Box::new(provider), bus);
    let id1 = client
        .start_generation_admitted(session_id, "plan", "gated", "m1", "prompt-1")
        .unwrap();

    let wait_start = Instant::now();
    while !first_started.load(Ordering::Acquire) && wait_start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(first_started.load(Ordering::Acquire));

    let id2 = client
        .start_generation_admitted(session_id, "plan", "gated", "m1", "prompt-2")
        .unwrap();
    let id3 = client
        .start_generation_admitted(session_id, "plan", "gated", "m1", "prompt-3")
        .unwrap();
    assert!(id2 > id1 && id3 > id2);

    let pending = client.pending_inputs(session_id).unwrap();
    let pending_contents: Vec<&str> = pending.iter().map(|i| i.content.as_str()).collect();
    assert_eq!(pending_contents, vec!["prompt-2", "prompt-3"]);

    gate.release();

    let mut completed = 0usize;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && completed < 3 {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(50))
            && event.kind == "generation_finished"
        {
            assert!(event.payload_json.contains("completed"));
            completed += 1;
        }
    }
    assert_eq!(completed, 3);
    assert_eq!(
        *seen_prompts.lock().unwrap(),
        vec!["prompt-1", "prompt-2", "prompt-3"]
    );

    let _ = client.shutdown();
    let _ = std::fs::remove_file(&path);
}

#[derive(Debug)]
struct SteerTestProvider {
    first_turn_started: Arc<AtomicBool>,
    seen_prompts: Arc<Mutex<Vec<String>>>,
}

impl Provider for SteerTestProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::LazyLock<ProviderId> =
            std::sync::LazyLock::new(|| ProviderId::new("steer"));
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
        Ok(StreamResponse { events: Vec::new() })
    }

    fn stream(&self, request: &StreamRequest) -> Result<ProviderStream, ProviderError> {
        let prompt = request.prompt.clone();
        self.seen_prompts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(prompt.clone());
        let is_first = !self.first_turn_started.swap(true, Ordering::AcqRel);
        let (sender, stream) = ProviderStream::channel(32);
        std::thread::spawn(move || {
            if is_first {
                for _ in 0..40 {
                    if sender.send(StreamEvent::TextDelta(".".into())).is_err() {
                        return;
                    }
                    let _ = sender.flush();
                    std::thread::sleep(Duration::from_millis(25));
                }
            } else {
                let _ = sender.send(StreamEvent::TextDelta(format!("done:{prompt}")));
                let _ = sender.flush();
            }
            let _ = sender.send(StreamEvent::Finish {
                reason: FinishReason::Stop,
            });
            let _ = sender.flush();
        });
        Ok(stream)
    }
}

#[test]
fn steering_queued_prompt_interrupts_current_turn_and_runs_steered_first() {
    let path = temp_db_path("steer_priority");
    let db = Db::open(&path).unwrap();
    let session = db.create_session("steer_priority").unwrap();
    let session_id = session.id;
    let writer = WriterHandle::spawn(Db::open(&path).unwrap());
    let bus = EventBus::new();
    let (_sub, rx) = bus.subscribe(Some(session_id));

    let first_started = Arc::new(AtomicBool::new(false));
    let seen_prompts = Arc::new(Mutex::new(Vec::new()));
    let provider = SteerTestProvider {
        first_turn_started: Arc::clone(&first_started),
        seen_prompts: Arc::clone(&seen_prompts),
    };

    let client = RuntimeClient::spawn(db, writer, Box::new(provider), bus);
    let _id1 = client
        .start_generation_admitted(session_id, "plan", "steer", "m1", "slow-1")
        .unwrap();

    let wait_start = Instant::now();
    while !first_started.load(Ordering::Acquire) && wait_start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(first_started.load(Ordering::Acquire));

    let id2 = client
        .start_generation_admitted(session_id, "plan", "steer", "m1", "queued-2")
        .unwrap();
    let id3 = client
        .start_generation_admitted(session_id, "plan", "steer", "m1", "steered-3")
        .unwrap();

    // Discard queued-2 before it runs and verify its content is returned for undo/restore.
    let discarded_text = client.discard_input(session_id, id2).unwrap();
    assert_eq!(discarded_text.as_deref(), Some("queued-2"));

    // Steer id3: interrupts slow-1 without aborting drain_session_loop, so steered-3 runs next.
    assert!(client.steer_input(session_id, id3).unwrap());

    let mut statuses = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && statuses.len() < 2 {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(50))
            && event.kind == "generation_finished"
        {
            let status = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                .ok()
                .and_then(|v| v.get("status")?.as_str().map(String::from))
                .unwrap_or_default();
            statuses.push(status);
        }
    }

    assert_eq!(statuses, vec!["cancelled", "completed"]);
    assert_eq!(*seen_prompts.lock().unwrap(), vec!["slow-1", "steered-3"]);

    // Also verify kv_state roundtrip via RuntimeClient.
    client.set_kv("test.key", r#"{"ok":true}"#).unwrap();
    assert_eq!(
        client.get_kv("test.key").unwrap().as_deref(),
        Some(r#"{"ok":true}"#)
    );

    let _ = client.shutdown();
    let _ = std::fs::remove_file(&path);
}

#[derive(Debug)]
struct SlowBashToolProvider {
    turns: Arc<Mutex<usize>>,
}

impl Provider for SlowBashToolProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::LazyLock<ProviderId> =
            std::sync::LazyLock::new(|| ProviderId::new("bgtool"));
        &ID
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
        }
    }

    fn models(&self) -> Vec<ModelInfo> {
        Vec::new()
    }

    fn send(&self, _request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        Ok(StreamResponse { events: Vec::new() })
    }

    fn stream(&self, _request: &StreamRequest) -> Result<ProviderStream, ProviderError> {
        let mut turn = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        let current_turn = *turn;
        *turn += 1;
        drop(turn);

        let (sender, stream) = ProviderStream::channel(16);
        std::thread::spawn(move || {
            if current_turn == 0 {
                let cmd = if cfg!(windows) {
                    "Start-Sleep -Milliseconds 400; Write-Output bg_finished_marker"
                } else {
                    "sleep 0.4; echo bg_finished_marker"
                };
                let args = serde_json::json!({ "command": cmd }).to_string();
                let _ = sender.send(StreamEvent::ToolCallStart {
                    id: "call_bg_1".into(),
                    name: "bash".into(),
                });
                let _ = sender.send(StreamEvent::ToolCallDelta {
                    id: "call_bg_1".into(),
                    arguments: args,
                });
                let _ = sender.send(StreamEvent::ToolCallEnd {
                    id: "call_bg_1".into(),
                });
                let _ = sender.send(StreamEvent::Finish {
                    reason: FinishReason::ToolCall,
                });
                let _ = sender.flush();
            } else {
                let _ = sender.send(StreamEvent::TextDelta(
                    "Turn continued while shell ran in background.".into(),
                ));
                let _ = sender.send(StreamEvent::Finish {
                    reason: FinishReason::Stop,
                });
                let _ = sender.flush();
            }
        });
        Ok(stream)
    }
}

#[test]
fn background_tools_unblocks_generation_turn_without_deadlocking_drop() {
    let path = temp_db_path("bg_tool");
    let db = Db::open(&path).unwrap();
    let session = db.create_session("bg_tool").unwrap();
    let session_id = session.id;
    let writer = WriterHandle::spawn(Db::open(&path).unwrap());
    let bus = EventBus::new();
    let (_sub, rx) = bus.subscribe(Some(session_id));

    let provider = SlowBashToolProvider {
        turns: Arc::new(Mutex::new(0)),
    };
    let client = RuntimeClient::spawn(db, writer, Box::new(provider), bus);

    client
        .start_generation(session_id, "build", "bgtool", "m1", "run slow command")
        .unwrap();

    // Wait until tool_call_started arrives so we know the bash worker is running.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut tool_started = false;
    while Instant::now() < deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(50))
            && event.kind == "tool_call_started"
        {
            tool_started = true;
            break;
        }
    }
    assert!(tool_started, "bash tool should start running");

    assert!(client.background_tools(session_id).unwrap());

    let mut settled_bg = false;
    let mut generation_completed = false;
    let mut bg_finished_output = String::new();
    let wait_deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < wait_deadline
        && !(settled_bg && generation_completed && !bg_finished_output.is_empty())
    {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(50)) {
            match event.kind.as_str() {
                "tool_call_settled" => {
                    let v: serde_json::Value =
                        serde_json::from_str(&event.payload_json).unwrap_or_default();
                    if v.pointer("/metadata/background").and_then(|b| b.as_bool()) == Some(true)
                        && v.pointer("/metadata/status").and_then(|s| s.as_str()) == Some("running")
                    {
                        settled_bg = true;
                    }
                }
                "generation_finished" => {
                    if event.payload_json.contains("completed") {
                        generation_completed = true;
                    }
                }
                "tool_background_finished" => {
                    let v: serde_json::Value =
                        serde_json::from_str(&event.payload_json).unwrap_or_default();
                    bg_finished_output = v
                        .get("output")
                        .and_then(|s| s.as_str())
                        .unwrap_or_default()
                        .to_string();
                }
                _ => {}
            }
        }
    }

    assert!(
        settled_bg,
        "tool call should settle immediately with background metadata"
    );
    assert!(
        generation_completed,
        "generation should complete without blocking on Drop"
    );
    assert!(
        bg_finished_output.contains("bg_finished_marker"),
        "expected background completion output, got: {bg_finished_output}"
    );

    let db = client.shutdown();
    let calls = db.tool_calls(session_id).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].status, ToolCallStatus::Completed);
    let _ = std::fs::remove_file(&path);
}
