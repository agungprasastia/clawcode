//! SQLite schema and migrations. Versioned via `PRAGMA user_version`.

use rusqlite::Connection;

/// Current persistence schema version.
pub const SCHEMA_VERSION: i64 = 2;

/// Apply all migrations up to [`SCHEMA_VERSION`]. Idempotent.
pub fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
            SchemaTooNew(current),
        )));
    }
    if current < 1 {
        connection.execute_batch(
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
             COMMIT;",
        )?;
    }
    if current < 2 {
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE workspaces (
                 id INTEGER PRIMARY KEY,
                 root_path TEXT NOT NULL UNIQUE,
                 display_name TEXT NOT NULL,
                 sort_order INTEGER NOT NULL DEFAULT 0,
                 archived_at TEXT,
                 last_opened_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             );
             CREATE TABLE generations (
                 id INTEGER PRIMARY KEY,
                 session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 agent_mode TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 model TEXT NOT NULL,
                 status TEXT NOT NULL,
                 started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                 ended_at TEXT,
                 cancel_requested_at TEXT,
                 metrics TEXT
             );
             CREATE INDEX idx_generations_session ON generations(session_id, id);
             CREATE TABLE generation_events (
                 seq INTEGER NOT NULL,
                 session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 generation_id INTEGER REFERENCES generations(id) ON DELETE SET NULL,
                 kind TEXT NOT NULL,
                 payload_json TEXT NOT NULL,
                 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                 PRIMARY KEY (session_id, seq)
             );
             INSERT INTO workspaces (id, root_path, display_name)
                 VALUES (1, '', 'default');
             ALTER TABLE sessions ADD COLUMN workspace_id INTEGER NOT NULL DEFAULT 1;
             ALTER TABLE sessions ADD COLUMN status TEXT NOT NULL DEFAULT 'idle';
             ALTER TABLE sessions ADD COLUMN active_generation_id INTEGER;
             ALTER TABLE sessions ADD COLUMN last_error TEXT;
             ALTER TABLE sessions ADD COLUMN last_event_seq INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE sessions ADD COLUMN pinned_at TEXT;
             ALTER TABLE sessions ADD COLUMN archived_at TEXT;
             CREATE INDEX idx_sessions_workspace ON sessions(workspace_id);
             PRAGMA user_version = 2;
             COMMIT;",
        )?;
    }
    Ok(())
}

/// Error for databases written by a newer clawcode version.
#[derive(Debug)]
pub struct SchemaTooNew(pub i64);

impl std::fmt::Display for SchemaTooNew {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "database schema v{} is newer than supported v{SCHEMA_VERSION}",
            self.0
        )
    }
}

impl std::error::Error for SchemaTooNew {}
