use clawcode::persistence::{Db, WriterHandle};
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
        .create_tool_call(
            first_session.id,
            first_generation.id,
            first_assistant.id,
            "same-call",
            "read_file",
            "{}",
        )
        .unwrap();
    writer
        .create_tool_call(
            first_session.id,
            first_generation.id,
            second_assistant.id,
            "same-call",
            "read_file",
            "{}",
        )
        .unwrap();
    assert!(
        writer
            .create_tool_call(
                first_session.id,
                first_generation.id,
                first_assistant.id,
                "same-call",
                "read_file",
                "{}",
            )
            .is_err()
    );
    assert!(
        writer
            .create_tool_call(
                first_session.id,
                first_generation.id,
                other_assistant.id,
                "cross-parent",
                "read_file",
                "{}",
            )
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
            .create_tool_call(
                session.id,
                generation.id,
                assistant.id,
                "call-too-large",
                "read_file",
                &oversized,
            )
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
        .create_tool_call(
            session.id,
            generation.id,
            assistant.id,
            "call-7",
            "read_file",
            r#"{"path":"README.md"}"#,
        )
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
        .create_tool_call(
            session.id,
            generation.id,
            assistant.id,
            "call-9",
            "read_file",
            "{}",
        )
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
            .create_tool_call(
                session.id,
                generation.id,
                user_msg.id,
                "call-user",
                "read_file",
                "{}",
            )
            .is_err()
    );

    assert!(
        writer
            .create_tool_call(
                session.id,
                generation.id,
                tool_msg.id,
                "call-tool",
                "read_file",
                "{}",
            )
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
        .create_tool_call(
            session.id,
            generation.id,
            assistant.id,
            "call-1",
            "read_file",
            "{}",
        )
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
        .create_tool_call(
            session.id,
            generation.id,
            assistant.id,
            "call-2",
            "read_file",
            "{}",
        )
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
        .create_tool_call(
            session.id,
            generation.id,
            assistant.id,
            "call-3",
            "read_file",
            "{}",
        )
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
