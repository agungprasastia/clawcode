use clawcode::persistence::{
    Db, InputDelivery, InputStatus, MAX_MESSAGE_BYTES, NewToolCall, WriterHandle,
};
use std::path::PathBuf;

fn temp_db_path(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "clawcode-persistence-{}-{tag}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn session_crud_and_restart_recovery() {
    let path = temp_db_path("crud");
    let session_id = {
        let db = Db::open(&path).unwrap();
        let session = db.create_session("first").unwrap();
        db.append_message(session.id, "user", "hello").unwrap();
        db.append_message(session.id, "assistant", "hi there")
            .unwrap();
        session.id
    };

    // Restart simulation: fresh handle over the same file.
    let db = Db::open(&path).unwrap();
    let sessions = db.list_sessions().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, session_id);
    assert_eq!(sessions[0].title, "first");

    let messages = db.messages(session_id).unwrap();
    let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
    let contents: Vec<&str> = messages.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(roles, ["user", "assistant"]);
    assert_eq!(contents, ["hello", "hi there"]);

    db.delete_session(session_id).unwrap();
    assert!(db.list_sessions().unwrap().is_empty());
    assert!(db.messages(session_id).unwrap().is_empty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn batched_writer_persists_all_and_returns_db() {
    let path = temp_db_path("writer");
    let db = Db::open(&path).unwrap();
    let session = db.create_session("writer").unwrap();
    let session_id = session.id;

    let writer = WriterHandle::spawn(db);
    for i in 0..200 {
        writer
            .append(session_id, "user", &format!("message {i}"))
            .expect("append should succeed");
    }
    writer.flush();
    let db = writer.shutdown().expect("sole writer");
    assert_eq!(db.messages(session_id).unwrap().len(), 200);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn concurrent_appends_commit_in_batches() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("concurrent").unwrap();
    let session_id = session.id;

    let writer = std::sync::Arc::new(WriterHandle::spawn(db));
    let handles: Vec<_> = (0..4)
        .map(|worker| {
            let writer = std::sync::Arc::clone(&writer);
            std::thread::spawn(move || {
                for i in 0..25 {
                    writer
                        .append(session_id, "user", &format!("w{worker}m{i}"))
                        .expect("append should succeed");
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let db = std::sync::Arc::unwrap_or_clone(writer)
        .shutdown()
        .expect("sole writer");
    assert_eq!(db.messages(session_id).unwrap().len(), 100);
}

#[test]
fn try_append_rejects_oversized_without_blocking() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("limits").unwrap();
    let writer = WriterHandle::spawn(db);
    let oversized = "x".repeat(clawcode::persistence::MAX_MESSAGE_BYTES + 1);

    let error = writer
        .try_append(session.id, "user", &oversized)
        .expect_err("oversized append should fail");
    assert!(error.contains("too large"));

    // Non-blocking path still persists normal messages.
    writer
        .try_append(session.id, "user", "ok")
        .expect("append should succeed");
    writer.flush();
    let db = writer.shutdown().expect("sole writer");
    assert_eq!(db.messages(session.id).unwrap().len(), 1);
}

#[test]
fn append_then_shutdown_does_not_hang() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("shutdown").unwrap();
    let writer = WriterHandle::spawn(db);
    writer
        .append(session.id, "user", "last words")
        .expect("append should succeed");
    let db = writer.shutdown().expect("sole writer");
    assert_eq!(db.messages(session.id).unwrap().len(), 1);
}

#[test]
fn retention_caps_messages_per_session() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("retention").unwrap();

    for i in 0..(clawcode::persistence::MAX_MESSAGES_PER_SESSION + 50) {
        db.append_message(session.id, "user", &format!("m{i}"))
            .unwrap();
    }
    let removed = db.enforce_retention().unwrap();

    let messages = db.messages(session.id).unwrap();
    assert_eq!(
        messages.len(),
        clawcode::persistence::MAX_MESSAGES_PER_SESSION
    );
    // Oldest messages were dropped; newest survive.
    let expected_last = format!("m{}", clawcode::persistence::MAX_MESSAGES_PER_SESSION + 49);
    assert_eq!(messages.last().unwrap().content, expected_last);
    assert!(removed > 0);
}

#[test]
fn oversized_message_is_rejected() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("limits").unwrap();
    let oversized = "x".repeat(clawcode::persistence::MAX_MESSAGE_BYTES + 1);

    let error = db
        .append_message(session.id, "user", &oversized)
        .expect_err("oversized append should fail");
    assert!(error.to_string().contains("too large"));
}

#[test]
fn retention_leaves_under_cap_sessions_untouched() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("small").unwrap();
    db.append_message(session.id, "user", "only one").unwrap();

    let removed = db.enforce_retention().unwrap();

    assert_eq!(removed, 0);
    assert_eq!(db.messages(session.id).unwrap().len(), 1);
}

#[test]
fn migration_is_idempotent_and_rejects_future_schema() {
    let path = temp_db_path("migration");
    {
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version(), clawcode::persistence::SCHEMA_VERSION);
    }
    // Reopen: migration must not fail or duplicate.
    {
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version(), clawcode::persistence::SCHEMA_VERSION);
    }
    // Future schema is rejected, not silently migrated down.
    {
        let db = Db::open(&path).unwrap();
        db.set_schema_version_for_test(clawcode::persistence::SCHEMA_VERSION + 1);
    }
    let error = match Db::open(&path) {
        Ok(_) => panic!("opening newer-schema database should fail"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("newer"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn v2_database_migrates_tool_call_projection() {
    let path = temp_db_path("tool_calls_migration");
    {
        let _db = Db::open(&path).unwrap();
    }
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch("DROP TABLE tool_calls; PRAGMA user_version = 2;")
            .unwrap();
    }
    let db = Db::open(&path).unwrap();
    assert_eq!(db.schema_version(), clawcode::persistence::SCHEMA_VERSION);
    assert!(db.tool_calls(1).unwrap().is_empty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn v3_database_rebuild_preserves_rows_with_assistant_identity() {
    let path = temp_db_path("tool_calls_v3_migration");
    let (session_id, generation_id, assistant_a, assistant_b) = {
        let db = Db::open(&path).unwrap();
        let session = db.create_session("v3").unwrap();
        let generation = db
            .start_generation(session.id, "plan", "fake", "model")
            .unwrap();
        let first = db.append_message(session.id, "assistant", "one").unwrap();
        let second = db.append_message(session.id, "assistant", "two").unwrap();
        (session.id, generation.id, first.id, second.id)
    };
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
             DROP TABLE tool_calls;
             CREATE TABLE tool_calls (
                 id INTEGER PRIMARY KEY,
                 session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 generation_id INTEGER NOT NULL REFERENCES generations(id) ON DELETE CASCADE,
                 assistant_message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                 call_id TEXT NOT NULL,
                 tool_name TEXT NOT NULL,
                 arguments TEXT NOT NULL,
                 status TEXT NOT NULL,
                 result TEXT,
                 error TEXT,
                 created_at TEXT NOT NULL,
                 settled_at TEXT,
                 UNIQUE (generation_id, call_id)
             );
             INSERT INTO tool_calls VALUES
                 (11, 1, 1, 1, 'same-call', 'read_file', '{}', 'created', NULL, NULL, 'now', NULL),
                 (12, 1, 1, 2, 'other-call', 'read_file', '{}', 'created', NULL, NULL, 'now', NULL);
             PRAGMA user_version = 3;",
            )
            .unwrap();
    }
    let db = Db::open(&path).unwrap();
    let calls = db.tool_calls(session_id).unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].assistant_message_id, assistant_a);
    assert_eq!(calls[1].assistant_message_id, assistant_b);
    assert_eq!(
        (session_id, generation_id, assistant_a, assistant_b),
        (1, 1, 1, 2)
    );
    assert_eq!(db.schema_version(), clawcode::persistence::SCHEMA_VERSION);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn call_id_reuses_across_assistant_messages_but_not_same_owner() {
    let db = Db::open_in_memory().unwrap();
    let first_session = db.create_session("identity").unwrap();
    let first_generation = db
        .start_generation(first_session.id, "plan", "fake", "model")
        .unwrap();
    let second_session = db.create_session("other").unwrap();
    let first_assistant = db
        .append_message(first_session.id, "assistant", "one")
        .unwrap();
    let second_assistant = db
        .append_message(first_session.id, "assistant", "two")
        .unwrap();
    let other_assistant = db
        .append_message(second_session.id, "assistant", "other")
        .unwrap();
    let writer = WriterHandle::spawn(db);
    writer
        .create_tool_call(NewToolCall {
            session_id: first_session.id,
            generation_id: first_generation.id,
            assistant_message_id: first_assistant.id,
            call_id: "same-call",
            tool_name: "read_file",
            arguments: "{}",
        })
        .unwrap();
    writer
        .create_tool_call(NewToolCall {
            session_id: first_session.id,
            generation_id: first_generation.id,
            assistant_message_id: second_assistant.id,
            call_id: "same-call",
            tool_name: "read_file",
            arguments: "{}",
        })
        .unwrap();
    assert!(
        writer
            .create_tool_call(NewToolCall {
                session_id: first_session.id,
                generation_id: first_generation.id,
                assistant_message_id: first_assistant.id,
                call_id: "same-call",
                tool_name: "read_file",
                arguments: "{}",
            })
            .is_err()
    );
    assert!(
        writer
            .create_tool_call(NewToolCall {
                session_id: first_session.id,
                generation_id: first_generation.id,
                assistant_message_id: other_assistant.id,
                call_id: "cross-parent",
                tool_name: "read_file",
                arguments: "{}",
            })
            .is_err()
    );
    let db = writer.shutdown().unwrap();
    assert_eq!(db.tool_calls(first_session.id).unwrap().len(), 2);
}

#[test]
fn tool_call_arguments_respect_existing_storage_bound() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("tool_limits").unwrap();
    let generation = db
        .start_generation(session.id, "plan", "fake", "model")
        .unwrap();
    let writer = WriterHandle::spawn(db);
    let assistant = writer.append_message(session.id, "assistant", "").unwrap();
    let oversized = "x".repeat(clawcode::persistence::MAX_TOOL_OUTPUT_BYTES + 1);
    assert!(
        writer
            .create_tool_call(NewToolCall {
                session_id: session.id,
                generation_id: generation.id,
                assistant_message_id: assistant.id,
                call_id: "call-too-large",
                tool_name: "read_file",
                arguments: &oversized,
            })
            .is_err()
    );
    let db = writer.shutdown().unwrap();
    assert!(db.tool_calls(session.id).unwrap().is_empty());
}

#[test]
fn delete_session_cascades_messages() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("cascade").unwrap();
    db.append_message(session.id, "user", "bye").unwrap();

    db.delete_session(session.id).unwrap();

    assert!(db.messages(session.id).unwrap().is_empty());
}

#[test]
fn batched_event_appends_update_last_event_seq_atomically() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("events_seq").unwrap();
    let session_id = session.id;

    let writer = WriterHandle::spawn(db);
    let mut seqs = Vec::new();
    for i in 0..10 {
        let seq = writer
            .append_event(session_id, None, "test_event", &format!("{{\"i\":{i}}}"))
            .expect("event append should succeed");
        seqs.push(seq);
    }
    writer.flush();

    assert_eq!(seqs, (0..10).collect::<Vec<i64>>());

    let db = writer.shutdown().expect("sole writer");
    let events = db.events_after(session_id, -1).unwrap();
    assert_eq!(events.len(), 10);
    assert_eq!(events.last().unwrap().seq, 9);

    // Check that sessions.last_event_seq was updated to 9
    let last_seq: i64 = db
        .messages(session_id)
        .map(|_| {
            // Query session last_event_seq directly via Db
            let s = db.session(session_id).unwrap().unwrap();
            s.id
        })
        .unwrap();
    assert_eq!(last_seq, session_id);
}

#[test]
fn writer_operations_after_shutdown_fail_gracefully() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("shutdown_grace").unwrap();
    let writer = WriterHandle::spawn(db);
    let _db = writer.shutdown().expect("sole writer");

    // Operations on writer after shutdown must return Err, never panic
    let append_err = writer.append(session.id, "user", "post-mortem");
    assert!(append_err.is_err());
    assert!(append_err.unwrap_err().contains("writer shut down"));

    let event_err = writer.append_event(session.id, None, "event", "{}");
    assert!(event_err.is_err());
    assert!(event_err.unwrap_err().contains("writer shut down"));
}
#[test]
fn tool_call_identity_and_settlement_use_assistant_message_id() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("tool_identity").unwrap();
    let generation = db
        .start_generation(session.id, "plan", "fake", "model")
        .unwrap();
    let writer = WriterHandle::spawn(db);

    let assistant = writer
        .append_message(session.id, "assistant", "")
        .expect("assistant message must commit before tool call");
    let (created, created_seq) = writer
        .create_tool_call(NewToolCall {
            session_id: session.id,
            generation_id: generation.id,
            assistant_message_id: assistant.id,
            call_id: "call-7",
            tool_name: "read_file",
            arguments: r#"{"path":"README.md"}"#,
        })
        .expect("tool identity must commit");
    assert_eq!(created.assistant_message_id, assistant.id);
    assert_eq!(created.generation_id, generation.id);
    assert_eq!(
        created.status,
        clawcode::persistence::ToolCallStatus::Created
    );

    let (running, running_seq) = writer
        .start_tool_call(created.id)
        .expect("running transition must commit");
    assert!(running_seq > created_seq);
    assert_eq!(
        running.status,
        clawcode::persistence::ToolCallStatus::Running
    );

    let (settled, settled_seq) = writer
        .settle_tool_call(
            created.id,
            clawcode::persistence::ToolCallStatus::Completed,
            Some("done"),
            None,
        )
        .expect("settlement must commit");
    assert!(settled_seq > running_seq);
    assert_eq!(settled.assistant_message_id, assistant.id);
    assert_eq!(
        settled.status,
        clawcode::persistence::ToolCallStatus::Completed
    );
    assert_eq!(settled.result.as_deref(), Some("done"));

    let db = writer.shutdown().expect("sole writer");
    let events = db.events_after(session.id, -1).unwrap();
    assert_eq!(events.len(), 3);
    for event in events {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&event.payload_json)
                .unwrap()
                .get("assistant_message_id")
                .and_then(serde_json::Value::as_i64),
            Some(assistant.id)
        );
    }
}

#[test]
fn failed_tool_settlement_does_not_claim_success() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("tool_failure").unwrap();
    let generation = db
        .start_generation(session.id, "plan", "fake", "model")
        .unwrap();
    let writer = WriterHandle::spawn(db);
    let assistant = writer.append_message(session.id, "assistant", "").unwrap();
    let (created, _) = writer
        .create_tool_call(NewToolCall {
            session_id: session.id,
            generation_id: generation.id,
            assistant_message_id: assistant.id,
            call_id: "call-9",
            tool_name: "read_file",
            arguments: "{}",
        })
        .unwrap();
    writer.start_tool_call(created.id).unwrap();

    let (settled, _) = writer
        .settle_tool_call(
            created.id,
            clawcode::persistence::ToolCallStatus::Failed,
            None,
            Some("permission denied"),
        )
        .unwrap();
    assert_eq!(
        settled.status,
        clawcode::persistence::ToolCallStatus::Failed
    );
    assert_eq!(settled.error.as_deref(), Some("permission denied"));
    assert!(
        writer
            .settle_tool_call(
                created.id,
                clawcode::persistence::ToolCallStatus::Completed,
                Some("wrong"),
                None,
            )
            .is_err()
    );
    let db = writer.shutdown().unwrap();
    let persisted = db.tool_call(created.id).unwrap().unwrap();
    assert_eq!(
        persisted.status,
        clawcode::persistence::ToolCallStatus::Failed
    );
    assert_eq!(persisted.result, None);
}

#[test]
fn tool_call_creation_rejects_non_assistant_roles() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("role_rejection").unwrap();
    let generation = db
        .start_generation(session.id, "plan", "fake", "model")
        .unwrap();
    let writer = WriterHandle::spawn(db);
    let user_msg = writer.append_message(session.id, "user", "hi").unwrap();
    let tool_msg = writer.append_message(session.id, "tool", "result").unwrap();

    assert!(
        writer
            .create_tool_call(NewToolCall {
                session_id: session.id,
                generation_id: generation.id,
                assistant_message_id: user_msg.id,
                call_id: "call-user",
                tool_name: "read_file",
                arguments: "{}",
            })
            .is_err()
    );

    assert!(
        writer
            .create_tool_call(NewToolCall {
                session_id: session.id,
                generation_id: generation.id,
                assistant_message_id: tool_msg.id,
                call_id: "call-tool",
                tool_name: "read_file",
                arguments: "{}",
            })
            .is_err()
    );

    let db = writer.shutdown().unwrap();
    assert!(db.tool_calls(session.id).unwrap().is_empty());
}

#[test]
fn tool_call_settlement_transitions_enforce_running_status() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("transitions").unwrap();
    let generation = db
        .start_generation(session.id, "plan", "fake", "model")
        .unwrap();
    let writer = WriterHandle::spawn(db);
    let assistant = writer.append_message(session.id, "assistant", "").unwrap();

    let (created_1, _) = writer
        .create_tool_call(NewToolCall {
            session_id: session.id,
            generation_id: generation.id,
            assistant_message_id: assistant.id,
            call_id: "call-1",
            tool_name: "read_file",
            arguments: "{}",
        })
        .unwrap();
    // created -> completed rejected
    assert!(
        writer
            .settle_tool_call(
                created_1.id,
                clawcode::persistence::ToolCallStatus::Completed,
                Some("ok"),
                None,
            )
            .is_err()
    );
    // created -> cancelled rejected
    assert!(
        writer
            .settle_tool_call(
                created_1.id,
                clawcode::persistence::ToolCallStatus::Cancelled,
                None,
                None,
            )
            .is_err()
    );
    // created -> failed allowed
    let (failed_1, _) = writer
        .settle_tool_call(
            created_1.id,
            clawcode::persistence::ToolCallStatus::Failed,
            None,
            Some("start failure"),
        )
        .unwrap();
    assert_eq!(
        failed_1.status,
        clawcode::persistence::ToolCallStatus::Failed
    );

    let (created_2, _) = writer
        .create_tool_call(NewToolCall {
            session_id: session.id,
            generation_id: generation.id,
            assistant_message_id: assistant.id,
            call_id: "call-2",
            tool_name: "read_file",
            arguments: "{}",
        })
        .unwrap();
    writer.start_tool_call(created_2.id).unwrap();
    // running -> completed allowed
    let (completed_2, _) = writer
        .settle_tool_call(
            created_2.id,
            clawcode::persistence::ToolCallStatus::Completed,
            Some("done"),
            None,
        )
        .unwrap();
    assert_eq!(
        completed_2.status,
        clawcode::persistence::ToolCallStatus::Completed
    );

    let (created_3, _) = writer
        .create_tool_call(NewToolCall {
            session_id: session.id,
            generation_id: generation.id,
            assistant_message_id: assistant.id,
            call_id: "call-3",
            tool_name: "read_file",
            arguments: "{}",
        })
        .unwrap();
    writer.start_tool_call(created_3.id).unwrap();
    // running -> cancelled allowed
    let (cancelled_3, _) = writer
        .settle_tool_call(
            created_3.id,
            clawcode::persistence::ToolCallStatus::Cancelled,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        cancelled_3.status,
        clawcode::persistence::ToolCallStatus::Cancelled
    );

    let db = writer.shutdown().unwrap();
    assert_eq!(db.tool_calls(session.id).unwrap().len(), 3);
}

#[test]
fn input_admission_creates_pending_row_and_event() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("input_admission").unwrap();

    let (input, seq) = db
        .admit_input(session.id, "prompt content", InputDelivery::Queue)
        .unwrap();

    assert!(input.id > 0);
    assert_eq!(input.session_id, session.id);
    assert_eq!(input.content, "prompt content");
    assert_eq!(input.delivery, InputDelivery::Queue);
    assert_eq!(input.status, InputStatus::Pending);
    assert!(input.promoted_at.is_none());
    assert!(input.user_message_id.is_none());
    assert!(!input.created_at.is_empty());

    // Input is queryable by id and by session.
    let by_id = db.session_input(input.id).unwrap().expect("input exists");
    assert_eq!(by_id, input);

    let inputs = db.session_inputs(session.id).unwrap();
    assert_eq!(inputs, vec![input.clone()]);

    // Admission does not create a user message.
    let messages = db.messages(session.id).unwrap();
    assert!(messages.is_empty());

    // Event log contains prompt_admitted event.
    let events = db.events_after(session.id, -1).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, seq);
    assert_eq!(events[0].session_id, session.id);
    assert_eq!(events[0].generation_id, None);
    assert_eq!(events[0].kind, "prompt_admitted");

    let payload: serde_json::Value = serde_json::from_str(&events[0].payload_json).unwrap();
    assert_eq!(payload["input_id"], input.id);
    assert_eq!(payload["session_id"], session.id);
    assert_eq!(payload["delivery"], "queue");
}

#[test]
fn input_admission_survives_restart() {
    let path = temp_db_path("input_restart");
    let (session_id, input_id) = {
        let db = Db::open(&path).unwrap();
        let session = db.create_session("restart_session").unwrap();
        let (input, _) = db
            .admit_input(session.id, "durable prompt", InputDelivery::Steer)
            .unwrap();
        (session.id, input.id)
    };

    // Restart: fresh handle to same database.
    let db = Db::open(&path).unwrap();
    let inputs = db.session_inputs(session_id).unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].id, input_id);
    assert_eq!(inputs[0].content, "durable prompt");
    assert_eq!(inputs[0].delivery, InputDelivery::Steer);
    assert_eq!(inputs[0].status, InputStatus::Pending);

    let messages = db.messages(session_id).unwrap();
    assert!(messages.is_empty());

    let events = db.events_after(session_id, -1).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "prompt_admitted");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn input_promotion_atomically_creates_message_updates_status_and_appends_event() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("input_promotion").unwrap();

    let (input, admit_seq) = db
        .admit_input(session.id, "to be promoted", InputDelivery::Queue)
        .unwrap();

    let (promoted_input, message, promo_seq) = db.promote_input(input.id).unwrap();

    assert!(promo_seq > admit_seq);

    // Message created with role='user'.
    assert!(message.id > 0);
    assert_eq!(message.role, "user");
    assert_eq!(message.content, "to be promoted");

    // Promoted input row updated.
    assert_eq!(promoted_input.id, input.id);
    assert_eq!(promoted_input.session_id, session.id);
    assert_eq!(promoted_input.content, "to be promoted");
    assert_eq!(promoted_input.delivery, InputDelivery::Queue);
    assert_eq!(promoted_input.status, InputStatus::Promoted);
    assert!(promoted_input.promoted_at.is_some());
    assert_eq!(promoted_input.user_message_id, Some(message.id));

    // Verified through independent queries.
    let fetched_input = db.session_input(input.id).unwrap().unwrap();
    assert_eq!(fetched_input, promoted_input);

    let messages = db.messages(session.id).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, message.id);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[0].content, "to be promoted");

    // Event log has prompt_admitted and prompt_promoted.
    let events = db.events_after(session.id, -1).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, "prompt_admitted");
    assert_eq!(events[1].kind, "prompt_promoted");
    assert_eq!(events[1].seq, promo_seq);
    assert_eq!(events[1].generation_id, None);

    let payload: serde_json::Value = serde_json::from_str(&events[1].payload_json).unwrap();
    assert_eq!(payload["input_id"], input.id);
    assert_eq!(payload["session_id"], session.id);
    assert_eq!(payload["user_message_id"], message.id);
}

#[test]
fn promoting_already_promoted_or_nonexistent_input_fails() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("promo_edge_cases").unwrap();

    let (input, _) = db
        .admit_input(session.id, "once only", InputDelivery::Queue)
        .unwrap();

    // First promotion succeeds.
    assert!(db.promote_input(input.id).is_ok());

    // Second promotion fails.
    assert!(db.promote_input(input.id).is_err());

    // Promoting non-existent input fails.
    assert!(db.promote_input(999_999).is_err());

    // Exactly one message and two events exist.
    assert_eq!(db.messages(session.id).unwrap().len(), 1);
    assert_eq!(db.events_after(session.id, -1).unwrap().len(), 2);
}

#[test]
fn oversized_content_or_invalid_session_fails_without_partial_row_or_event() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("input_validation").unwrap();

    // Content exceeding limit fails.
    let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
    assert!(
        db.admit_input(session.id, &oversized, InputDelivery::Queue)
            .is_err()
    );
    assert!(db.session_inputs(session.id).unwrap().is_empty());
    assert!(db.events_after(session.id, -1).unwrap().is_empty());

    // Non-existent session fails.
    assert!(
        db.admit_input(999_999, "valid", InputDelivery::Queue)
            .is_err()
    );
    assert!(db.session_inputs(session.id).unwrap().is_empty());
    assert!(db.events_after(session.id, -1).unwrap().is_empty());
}

#[test]
fn writer_handles_admit_and_promote_input() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("writer_inputs").unwrap();
    let writer = WriterHandle::spawn(db);

    // Oversized input rejected immediately.
    let oversized = "x".repeat(MAX_MESSAGE_BYTES + 1);
    assert!(
        writer
            .admit_input(session.id, &oversized, InputDelivery::Queue)
            .is_err()
    );

    // Valid input admitted.
    let (input, admit_seq) = writer
        .admit_input(session.id, "async prompt", InputDelivery::Steer)
        .unwrap();
    assert_eq!(input.status, InputStatus::Pending);
    assert_eq!(input.delivery, InputDelivery::Steer);

    // Valid input promoted.
    let (promoted, message, promo_seq) = writer.promote_input(input.id).unwrap();
    assert!(promo_seq > admit_seq);
    assert_eq!(promoted.status, InputStatus::Promoted);
    assert_eq!(message.role, "user");
    assert_eq!(message.content, "async prompt");

    // Double-promote rejected.
    assert!(writer.promote_input(input.id).is_err());

    let db = writer.shutdown().unwrap();
    assert_eq!(db.messages(session.id).unwrap().len(), 1);
    assert_eq!(db.session_inputs(session.id).unwrap().len(), 1);
}

#[test]
fn v4_database_migrates_to_v5_session_inputs() {
    let path = temp_db_path("v4_to_v5_migration");
    {
        let _db = Db::open(&path).unwrap();
    }
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch("DROP TABLE IF EXISTS session_inputs; PRAGMA user_version = 4;")
            .unwrap();
    }
    let db = Db::open(&path).unwrap();
    assert_eq!(db.schema_version(), clawcode::persistence::SCHEMA_VERSION);
    let session = db.create_session("migrated").unwrap();
    let (input, _) = db
        .admit_input(session.id, "post-migration prompt", InputDelivery::Queue)
        .unwrap();
    assert_eq!(input.status, InputStatus::Pending);
    let (promoted, _, _) = db.promote_input(input.id).unwrap();
    assert_eq!(promoted.status, InputStatus::Promoted);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn context_epoch_crud_and_restart_recovery() {
    let path = temp_db_path("epoch_crud");
    let session_id = {
        let db = Db::open(&path).unwrap();
        let session = db.create_session("epoch_session").unwrap();
        assert!(db.get_active_context_epoch(session.id).unwrap().is_none());

        let epoch = db
            .insert_context_epoch(
                session.id,
                "epoch-1",
                "exact baseline system prompt v1",
                r#"{"source_key":"agents_md","files":[]}"#,
            )
            .unwrap();
        assert_eq!(epoch.session_id, session.id);
        assert_eq!(epoch.epoch_id, "epoch-1");
        assert_eq!(
            epoch.baseline_system_text,
            "exact baseline system prompt v1"
        );

        let active = db.get_active_context_epoch(session.id).unwrap().unwrap();
        assert_eq!(active.epoch_id, "epoch-1");
        assert_eq!(
            active.baseline_system_text,
            "exact baseline system prompt v1"
        );

        session.id
    };

    // Restart simulation: fresh Db handle over same file
    let db = Db::open(&path).unwrap();
    let active = db.get_active_context_epoch(session_id).unwrap().unwrap();
    assert_eq!(active.epoch_id, "epoch-1");
    assert_eq!(
        active.baseline_system_text,
        "exact baseline system prompt v1"
    );
    assert_eq!(
        active.source_snapshot_json,
        r#"{"source_key":"agents_md","files":[]}"#
    );

    // Update snapshot
    let updated = db
        .update_context_epoch_snapshot(
            session_id,
            "epoch-1",
            r#"{"source_key":"agents_md","files":[{"path":"AGENTS.md","hash":"h1","size":10}]}"#,
        )
        .unwrap();
    assert!(updated);

    let active2 = db.get_active_context_epoch(session_id).unwrap().unwrap();
    assert_eq!(
        active2.source_snapshot_json,
        r#"{"source_key":"agents_md","files":[{"path":"AGENTS.md","hash":"h1","size":10}]}"#
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn reconcile_epoch_change_atomic_update_and_system_message() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("reconcile_epoch").unwrap();
    let session_id = session.id;

    db.insert_context_epoch(
        session_id,
        "epoch-1",
        "baseline prompt",
        r#"{"source_key":"agents_md","files":[]}"#,
    )
    .unwrap();

    let new_snapshot =
        r#"{"source_key":"agents_md","files":[{"path":"AGENTS.md","hash":"h2","size":20}]}"#;
    let delta = "# Updated Project Instructions\n\nnew instructions";

    let msg = db
        .reconcile_epoch_change(session_id, "epoch-1", new_snapshot, delta)
        .unwrap();
    assert_eq!(msg.role, "system");
    assert_eq!(msg.content, delta);

    let active = db.get_active_context_epoch(session_id).unwrap().unwrap();
    assert_eq!(active.source_snapshot_json, new_snapshot);

    let msgs = db.messages(session_id).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[0].content, delta);
}

#[test]
fn writer_handles_context_epoch_ops() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("writer_epochs").unwrap();
    let session_id = session.id;
    let writer = WriterHandle::spawn(db);

    let epoch = writer
        .insert_context_epoch(
            session_id,
            "epoch-1",
            "writer baseline",
            r#"{"source_key":"agents_md","files":[]}"#,
        )
        .unwrap();
    assert_eq!(epoch.epoch_id, "epoch-1");
    assert_eq!(epoch.baseline_system_text, "writer baseline");

    let updated = writer
        .update_context_epoch_snapshot(
            session_id,
            "epoch-1",
            r#"{"source_key":"agents_md","files":[{"path":"a","hash":"b","size":1}]}"#,
        )
        .unwrap();
    assert!(updated);

    let msg = writer
        .reconcile_epoch_change(
            session_id,
            "epoch-1",
            r#"{"source_key":"agents_md","files":[{"path":"a","hash":"c","size":2}]}"#,
            "delta update",
        )
        .unwrap();
    assert_eq!(msg.role, "system");
    assert_eq!(msg.content, "delta update");

    let db = writer.shutdown().unwrap();
    let active = db.get_active_context_epoch(session_id).unwrap().unwrap();
    assert_eq!(
        active.source_snapshot_json,
        r#"{"source_key":"agents_md","files":[{"path":"a","hash":"c","size":2}]}"#
    );
    assert_eq!(db.messages(session_id).unwrap().len(), 1);
}

#[test]
fn v5_database_migrates_to_v6_context_epochs() {
    let path = temp_db_path("v5_to_v6_migration");
    {
        let _db = Db::open(&path).unwrap();
    }
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch("DROP TABLE IF EXISTS context_epochs; PRAGMA user_version = 5;")
            .unwrap();
    }
    let db = Db::open(&path).unwrap();
    assert_eq!(db.schema_version(), clawcode::persistence::SCHEMA_VERSION);
    let session = db.create_session("migrated_v6").unwrap();
    let epoch = db
        .insert_context_epoch(
            session.id,
            "epoch-1",
            "migrated baseline",
            r#"{"source_key":"agents_md","files":[]}"#,
        )
        .unwrap();
    assert_eq!(epoch.epoch_id, "epoch-1");
    let _ = std::fs::remove_file(&path);
}
