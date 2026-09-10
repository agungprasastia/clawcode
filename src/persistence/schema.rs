//! SQLite schema and migrations. Versioned via `PRAGMA user_version`.

use rusqlite::Connection;

/// Current persistence schema version.
pub const SCHEMA_VERSION: i64 = 1;

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
