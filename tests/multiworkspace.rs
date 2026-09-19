//! Multiworkspace foundation & runtime integration tests (M10).
//! Validates migration v1->v2, interrupted generation recovery, generation
//! lifecycle, monotonic events, pinned ordering, EventBus backpressure and
//! subscriber pruning, WriterHandle append_event, and workspace isolation.

use clawcode::persistence::{Db, GenerationStatus, SessionStatus, WriterHandle};
use clawcode::provider::{
    FinishReason, ModelInfo, Provider, ProviderCapabilities, ProviderError, ProviderId,
    StreamEvent, StreamRequest, StreamResponse, Usage,
};
use clawcode::runtime::{EventBus, RuntimeEvent};
use clawcode::tui::{App, Input, UiEvent};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_db_path(tag: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock drifted")
        .as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("clawcode-test-m10-{tag}-{nonce}-{count}.db"))
}

#[derive(Debug)]
struct MockProvider;

impl Provider for MockProvider {
    fn id(&self) -> &ProviderId {
        static ID: std::sync::OnceLock<ProviderId> = std::sync::OnceLock::new();
        ID.get_or_init(|| ProviderId::new("mock"))
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
    fn send(&self, request: &StreamRequest) -> Result<StreamResponse, ProviderError> {
        if request.prompt == "fail" {
            return Err(ProviderError::Network("network failure".into()));
        }
        Ok(StreamResponse {
            events: vec![
                StreamEvent::TextDelta(format!("ack:{}", request.prompt)),
                StreamEvent::Usage(Usage {
                    input_tokens: 5,
                    output_tokens: 10,
                }),
                StreamEvent::Finish {
                    reason: FinishReason::Stop,
                },
            ],
        })
    }
}

// ---------------------------------------------------------------------------
// 1. Migration v1 -> v2 preserves data and adds defaults
// ---------------------------------------------------------------------------
#[test]
fn migration_v1_to_v2_preserves_data_and_adds_workspace_defaults() {
    let path = temp_db_path("migration_v1_v2");
    {
        // Construct a raw v1 database with user_version = 1
        let conn = rusqlite::Connection::open(&path).expect("raw open");
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE sessions (
                 id INTEGER PRIMARY KEY,
                 title TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             );
             CREATE TABLE messages (
                 id INTEGER PRIMARY KEY,
                 session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             );
             CREATE INDEX idx_messages_session ON messages(session_id, id);
             PRAGMA user_version = 1;
             INSERT INTO sessions (id, title) VALUES (42, 'v1 session');
             INSERT INTO messages (session_id, role, content) VALUES (42, 'user', 'hello from v1');
             COMMIT;",
        )
        .expect("setup v1");
    }

    // Now open through Db::open, which triggers migration to v2
    {
        let db = Db::open(&path).expect("open and migrate to v2");
        assert_eq!(db.schema_version(), 2);

        // Verify session preserved and updated with defaults
        let sessions = db.list_sessions_in_workspace(1).expect("list workspace 1");
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.id, 42);
        assert_eq!(session.title, "v1 session");
        assert_eq!(session.workspace_id, 1);
        assert_eq!(session.status, SessionStatus::Idle);
        assert!(!session.pinned);

        // Verify message preserved
        let messages = db.messages(42).expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello from v1");
    }

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// 2. recover_interrupted resets stale running generations & sessions to idle
// ---------------------------------------------------------------------------
#[test]
fn recover_interrupted_generations_resets_stale_running_to_idle() {
    let path = temp_db_path("recover_interrupted");
    {
        let db = Db::open(&path).expect("create db");
        let session = db.create_session("interrupted test").expect("create");
        let generation = db
            .start_generation(session.id, "plan", "anthropic", "claude-3")
            .expect("start");

        assert_eq!(generation.status, GenerationStatus::Running);

        // Verify session is marked running
        let listed = db.list_sessions_in_workspace(1).expect("list");
        assert_eq!(listed[0].status, SessionStatus::Running);
    }

    // Reopen simulated crash/restart: recover_interrupted_generations runs during Db::init
    {
        let db = Db::open(&path).expect("reopen");
        let listed = db.list_sessions_in_workspace(1).expect("list");
        assert_eq!(
            listed[0].status,
            SessionStatus::Idle,
            "session should recover to idle"
        );

        // Generations table status should be interrupted
        let conn = rusqlite::Connection::open(&path).expect("raw open");
        let gen_status: String = conn
            .query_row("SELECT status FROM generations WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("query gen status");
        assert_eq!(gen_status, "interrupted");
    }

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// 3. Generation lifecycle matrix: start, finish, error, cancel
// ---------------------------------------------------------------------------
#[test]
fn generation_lifecycle_matrix_start_finish_error_and_cancel() {
    let path = temp_db_path("gen_lifecycle");
    let db = Db::open(&path).expect("open db");
    let session = db.create_session("lifecycle").expect("session");

    // Start -> Running
    let generation = db
        .start_generation(session.id, "plan", "provider", "model")
        .expect("start");
    assert_eq!(generation.status, GenerationStatus::Running);
    assert_eq!(
        db.list_sessions_in_workspace(1).unwrap()[0].status,
        SessionStatus::Running
    );

    // Cancel request -> Cancelling
    let cancel_res = db.request_cancel(generation.id).expect("cancel");
    assert!(cancel_res);

    // Finish -> Idle
    db.finish_generation(generation.id, GenerationStatus::Cancelled, None)
        .expect("finish");
    assert_eq!(
        db.list_sessions_in_workspace(1).unwrap()[0].status,
        SessionStatus::Idle
    );

    // Second cycle: error with last_error recorded
    let gen2 = db
        .start_generation(session.id, "build", "provider", "model")
        .expect("start2");
    db.set_session_last_error(session.id, Some("token limit exceeded"))
        .expect("set last error");
    db.finish_generation(gen2.id, GenerationStatus::Failed, None)
        .expect("finish2");
    assert_eq!(
        db.list_sessions_in_workspace(1).unwrap()[0].status,
        SessionStatus::Idle
    );

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// 4. Monotonic events_after ordered and filtered per session
// ---------------------------------------------------------------------------
#[test]
fn monotonic_events_after_ordered_and_filtered_per_session() {
    let path = temp_db_path("events_after");
    let db = Db::open(&path).expect("open db");
    let s1 = db.create_session("s1").expect("s1");
    let s2 = db.create_session("s2").expect("s2");

    let g1 = db.start_generation(s1.id, "plan", "p", "m").expect("g1");
    let g2 = db.start_generation(s2.id, "plan", "p", "m").expect("g2");

    // Append 3 events to s1 and 2 events to s2
    let seq1 = db
        .append_event(s1.id, Some(g1.id), "delta", "{\"d\":\"1\"}")
        .expect("e1");
    let seq2 = db
        .append_event(s1.id, Some(g1.id), "delta", "{\"d\":\"2\"}")
        .expect("e2");
    let _ = db
        .append_event(s2.id, Some(g2.id), "delta", "{\"d\":\"s2_1\"}")
        .expect("e3");
    let seq3 = db
        .append_event(s1.id, Some(g1.id), "finish", "{\"reason\":\"stop\"}")
        .expect("e4");

    assert_eq!(seq1, 0);
    assert_eq!(seq2, 1);
    assert_eq!(seq3, 2);

    // Query events for s1 with after_seq = -1 (all events)
    let all_s1 = db.events_after(s1.id, -1).expect("events");
    assert_eq!(all_s1.len(), 3);
    assert_eq!(all_s1[0].seq, 0);
    assert_eq!(all_s1[1].seq, 1);
    assert_eq!(all_s1[2].seq, 2);
    assert_eq!(all_s1[0].kind, "delta");
    assert_eq!(all_s1[2].kind, "finish");

    // Query events after seq 0 (should return seq 1 and 2)
    let after_0 = db.events_after(s1.id, 0).expect("events after 0");
    assert_eq!(after_0.len(), 2);
    assert_eq!(after_0[0].seq, 1);
    assert_eq!(after_0[1].seq, 2);

    // Query events for s2 (should return only 1 event)
    let s2_events = db.events_after(s2.id, -1).expect("s2 events");
    assert_eq!(s2_events.len(), 1);
    assert_eq!(s2_events[0].seq, 0);
    assert_eq!(s2_events[0].payload_json, "{\"d\":\"s2_1\"}");

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// 5. Pinned ordering and toggling in workspace
// ---------------------------------------------------------------------------
#[test]
fn pinned_ordering_and_toggling_in_workspace() {
    let path = temp_db_path("pinned_ordering");
    let db = Db::open(&path).expect("open db");

    let s1 = db.create_session("session 1").expect("s1");
    let s2 = db.create_session("session 2").expect("s2");
    let s3 = db.create_session("session 3").expect("s3");

    // Initially newest first: s3, s2, s1
    let list = db.list_sessions_in_workspace(1).expect("list");
    assert_eq!(list[0].id, s3.id);
    assert_eq!(list[1].id, s2.id);
    assert_eq!(list[2].id, s1.id);

    // Pin s1: s1 must now appear first, ahead of newer sessions
    db.set_session_pinned(s1.id, true).expect("pin s1");
    let list = db.list_sessions_in_workspace(1).expect("list");
    assert_eq!(list[0].id, s1.id);
    assert!(list[0].pinned);
    assert_eq!(list[1].id, s3.id);
    assert_eq!(list[2].id, s2.id);

    // Pin s2: s2 pinned later, so s2 comes first, then s1, then s3
    db.set_session_pinned(s2.id, true).expect("pin s2");
    let list = db.list_sessions_in_workspace(1).expect("list");
    assert_eq!(list[0].id, s2.id);
    assert_eq!(list[1].id, s1.id);
    assert_eq!(list[2].id, s3.id);

    // Unpin s1: s2 stays first, then s3, then s1
    db.set_session_pinned(s1.id, false).expect("unpin s1");
    let list = db.list_sessions_in_workspace(1).expect("list");
    assert_eq!(list[0].id, s2.id);
    assert_eq!(list[1].id, s3.id);
    assert_eq!(list[2].id, s1.id);
    assert!(!list[2].pinned);

    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// 6. EventBus backpressure and subscriber pruning
// ---------------------------------------------------------------------------
#[test]
fn event_bus_bounded_drop_and_subscriber_pruning() {
    let bus = EventBus::new();
    let (sub_id, rx) = bus.subscribe(Some(1));

    // Send an event matching session 1
    bus.publish(RuntimeEvent {
        seq: 1,
        session_id: 1,
        generation_id: Some(10),
        kind: "delta".into(),
        payload_json: "{}".into(),
    });

    // Send an event for session 2 (should NOT reach session 1 subscriber)
    bus.publish(RuntimeEvent {
        seq: 2,
        session_id: 2,
        generation_id: Some(11),
        kind: "delta".into(),
        payload_json: "{}".into(),
    });

    let received = rx.try_recv().expect("receive session 1 event");
    assert_eq!(received.seq, 1);
    assert!(rx.try_recv().is_err());

    // Explicit unsubscribe works
    bus.unsubscribe(sub_id);
    bus.publish(RuntimeEvent {
        seq: 3,
        session_id: 1,
        generation_id: Some(10),
        kind: "finish".into(),
        payload_json: "{}".into(),
    });
    assert!(rx.try_recv().is_err());

    // Dropping receiver prunes subscriber on next publish without panic
    let (_sub_id2, rx2) = bus.subscribe(None);
    drop(rx2);
    bus.publish(RuntimeEvent {
        seq: 4,
        session_id: 1,
        generation_id: None,
        kind: "test".into(),
        payload_json: "{}".into(),
    });
}

#[test]
fn event_bus_backpressures_instead_of_disconnect_on_full_queue() {
    let bus = EventBus::new();
    let (_sub_id, rx) = bus.subscribe(Some(1));

    for seq in 0..1_024 {
        bus.publish(RuntimeEvent {
            seq,
            session_id: 1,
            generation_id: None,
            kind: "delta".into(),
            payload_json: "{}".into(),
        });
    }

    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let producer_bus = bus.clone();
    let producer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        producer_bus.publish(RuntimeEvent {
            seq: 1_024,
            session_id: 1,
            generation_id: None,
            kind: "delta".into(),
            payload_json: "{}".into(),
        });
        done_tx.send(()).unwrap();
    });

    started_rx.recv().unwrap();
    assert!(
        done_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err()
    );
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .seq,
        0
    );
    assert!(
        done_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok()
    );
    producer.join().unwrap();
    let mut last_seq = 0;
    for _ in 1..=1_024 {
        last_seq = rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .seq;
    }
    assert_eq!(last_seq, 1_024);
}

// ---------------------------------------------------------------------------
// 7. WriterHandle append_event and flush
// ---------------------------------------------------------------------------
#[test]
fn writer_handle_append_event_and_flush() {
    let path = temp_db_path("writer_append_event");
    let db = Db::open(&path).expect("open db");
    let session = db.create_session("writer test").expect("create session");
    let generation = db
        .start_generation(session.id, "plan", "provider", "model")
        .expect("start");

    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));

    let seq1 = writer
        .append_event(session.id, Some(generation.id), "delta", "{\"text\":\"a\"}")
        .expect("append 1");
    let seq2 = writer
        .append_event(session.id, Some(generation.id), "delta", "{\"text\":\"b\"}")
        .expect("append 2");

    assert_eq!(seq1, 0);
    assert_eq!(seq2, 1);

    writer.flush();

    // Events must now be committed to SQLite
    let events = db.events_after(session.id, -1).expect("events_after");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].payload_json, "{\"text\":\"a\"}");
    assert_eq!(events[1].payload_json, "{\"text\":\"b\"}");

    let _ = writer.shutdown();
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// 8. Multiworkspace isolation and client state preservation
// ---------------------------------------------------------------------------
#[test]
fn multiworkspace_isolation_and_client_state() {
    let path = temp_db_path("ws_isolation");
    let db = Db::open(&path).expect("open db");

    // Create a second workspace in DB
    let conn = rusqlite::Connection::open(&path).expect("raw open");
    conn.execute(
        "INSERT INTO workspaces (id, root_path, display_name) VALUES (2, '/ws2', 'Second WS')",
        [],
    )
    .expect("insert ws 2");

    let s_ws1 = db
        .create_session_in_workspace(1, "WS1 Session")
        .expect("create ws1");
    let s_ws2 = db
        .create_session_in_workspace(2, "WS2 Session")
        .expect("create ws2");

    // List workspace 1: only s_ws1
    let ws1_sessions = db.list_sessions_in_workspace(1).expect("list ws1");
    assert_eq!(ws1_sessions.len(), 1);
    assert_eq!(ws1_sessions[0].id, s_ws1.id);

    // List workspace 2: only s_ws2
    let ws2_sessions = db.list_sessions_in_workspace(2).expect("list ws2");
    assert_eq!(ws2_sessions.len(), 1);
    assert_eq!(ws2_sessions[0].id, s_ws2.id);

    // Test TUI App state isolation across sessions
    let mut app = App::default();
    let writer = WriterHandle::spawn(Db::open(&path).expect("writer db"));
    app.attach_runtime(db, writer, Box::new(MockProvider));

    // Explicitly activate session 1
    app.switch_session(s_ws1.id);
    assert_eq!(app.active_session_id(), Some(s_ws1.id));

    // Type in session 1
    app.apply(UiEvent::Input(Input::Character('f')));
    app.apply(UiEvent::Input(Input::Character('o')));
    app.apply(UiEvent::Input(Input::Character('o')));
    app.apply(UiEvent::Input(Input::Submit));

    assert_eq!(app.active_session_id(), Some(s_ws1.id));
    assert!(app.transcript().contains("foo"));

    // Switch to session 2
    app.switch_session(s_ws2.id);
    assert_eq!(app.active_session_id(), Some(s_ws2.id));
    assert!(
        app.transcript().is_empty(),
        "switched session starts with clean transcript"
    );

    // Type in session 2
    app.apply(UiEvent::Input(Input::Character('b')));
    app.apply(UiEvent::Input(Input::Character('a')));
    app.apply(UiEvent::Input(Input::Character('r')));
    app.apply(UiEvent::Input(Input::Submit));
    assert!(app.transcript().contains("bar"));

    // Switch back to session 1: transcript restored!
    app.switch_session(s_ws1.id);
    assert!(
        app.transcript().contains("foo"),
        "session 1 transcript restored upon return"
    );
    assert!(
        !app.transcript().contains("bar"),
        "session 1 must not contain session 2 transcript"
    );

    let _ = std::fs::remove_file(&path);
}
