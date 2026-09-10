//! Session/message storage. Synchronous API; hot-path appends go through
//! `writer.rs` so the render loop never blocks on SQLite.

use crate::persistence::schema;
use rusqlite::{Connection, OptionalExtension, params};

/// Retention limits. Enforced by [`Db::enforce_retention`].
pub const MAX_MESSAGES_PER_SESSION: usize = 1_200;
/// Maximum stored message size in bytes.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// Maximum sessions kept.
pub const MAX_SESSIONS: usize = 200;

#[derive(Debug)]
pub struct Session {
    pub id: i64,
    pub title: String,
}

#[derive(Debug)]
pub struct Message {
    pub id: i64,
    pub role: String,
    pub content: String,
}

/// SQLite-backed store.
pub struct Db {
    pub(crate) connection: Connection,
}

impl Db {
    /// Open (creating or migrating) the database at `path`.
    pub fn open(path: &std::path::Path) -> Result<Self, rusqlite::Error> {
        let connection = Connection::open(path)?;
        Self::init(connection)
    }

    /// In-memory database for tests and ephemeral use.
    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(connection: Connection) -> Result<Self, rusqlite::Error> {
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate(&connection)?;
        Ok(Self { connection })
    }

    /// Applied schema version.
    pub fn schema_version(&self) -> i64 {
        self.connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap_or(0)
    }

    /// Test-only: simulate a database written by a newer version.
    pub fn set_schema_version_for_test(&self, version: i64) {
        let _ = self.connection.pragma_update(None, "user_version", version);
    }

    pub fn create_session(&self, title: &str) -> Result<Session, rusqlite::Error> {
        self.connection.query_row(
            "INSERT INTO sessions (title) VALUES (?1) RETURNING id, title",
            params![title],
            |row| {
                Ok(Session {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            },
        )
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>, rusqlite::Error> {
        let mut statement = self
            .connection
            .prepare("SELECT id, title FROM sessions ORDER BY updated_at DESC, id DESC")?;
        let rows = statement.query_map([], |row| {
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
            })
        })?;
        rows.collect()
    }

    pub fn append_message(
        &self,
        session_id: i64,
        role: &str,
        content: &str,
    ) -> Result<Message, rusqlite::Error> {
        if content.len() > MAX_MESSAGE_BYTES {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                MessageTooLarge(content.len()),
            )));
        }
        self.connection.query_row(
            "INSERT INTO messages (session_id, role, content) VALUES (?1, ?2, ?3)
             RETURNING id, role, content",
            params![session_id, role, content],
            |row| {
                Ok(Message {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: row.get(2)?,
                })
            },
        )
    }

    pub fn messages(&self, session_id: i64) -> Result<Vec<Message>, rusqlite::Error> {
        let mut statement = self
            .connection
            .prepare("SELECT id, role, content FROM messages WHERE session_id = ?1 ORDER BY id")?;
        let rows = statement.query_map(params![session_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn delete_session(&self, session_id: i64) -> Result<(), rusqlite::Error> {
        self.connection
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
        Ok(())
    }

    /// Drop oldest messages beyond the per-session cap and oldest sessions
    /// beyond the session cap. Returns number of removed message rows.
    pub fn enforce_retention(&self) -> Result<usize, rusqlite::Error> {
        let mut removed = 0;
        let mut session_ids: Vec<i64> = {
            let mut statement = self
                .connection
                .prepare("SELECT id FROM sessions ORDER BY updated_at DESC, id DESC")?;
            let rows = statement.query_map([], |row| row.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for session_id in &session_ids {
            let count: i64 = self.connection.query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?;
            let excess = count.saturating_sub(MAX_MESSAGES_PER_SESSION as i64);
            if excess > 0 {
                removed += self.connection.execute(
                    "DELETE FROM messages WHERE id IN (
                         SELECT id FROM messages WHERE session_id = ?1 ORDER BY id LIMIT ?2
                     )",
                    params![session_id, excess],
                )?;
            }
        }
        if session_ids.len() > MAX_SESSIONS {
            let excess_sessions = session_ids.split_off(MAX_SESSIONS);
            for session_id in &excess_sessions {
                self.connection
                    .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
            }
        }
        Ok(removed)
    }

    /// Latest session id, if any. Used for resume-on-start.
    pub fn latest_session_id(&self) -> Result<Option<i64>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT id FROM sessions ORDER BY updated_at DESC, id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
    }
}

/// Message exceeds [`MAX_MESSAGE_BYTES`].
#[derive(Debug)]
pub struct MessageTooLarge(pub usize);

impl std::fmt::Display for MessageTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "message too large: {} bytes (max {MAX_MESSAGE_BYTES})",
            self.0
        )
    }
}

impl std::error::Error for MessageTooLarge {}
