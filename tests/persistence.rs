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
    let db = writer.shutdown();
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
    let db = std::sync::Arc::into_inner(writer)
        .expect("sole owner")
        .shutdown();
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
    let db = writer.shutdown();
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
    let db = writer.shutdown();
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
fn delete_session_cascades_messages() {
    let db = Db::open_in_memory().unwrap();
    let session = db.create_session("cascade").unwrap();
    db.append_message(session.id, "user", "bye").unwrap();

    db.delete_session(session.id).unwrap();

    assert!(db.messages(session.id).unwrap().is_empty());
}
