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

/// Session status stored in `sessions.status`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SessionStatus {
    Idle,
    Running,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            // Unknown values fall back to idle: no generation can be
            // running when the runtime is not driving one.
            _ => Self::Idle,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Session {
    pub id: i64,
    pub title: String,
    pub workspace_id: i64,
    pub status: SessionStatus,
    pub pinned: bool,
}

impl Session {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            workspace_id: row.get(2)?,
            status: SessionStatus::from_str(&row.get::<_, String>(3)?),
            pinned: row.get::<_, Option<String>>(4)?.is_some(),
        })
    }

    const SELECT_COLUMNS: &str = "id, title, workspace_id, status, pinned_at";
}

#[derive(Debug)]
pub struct Message {
    pub id: i64,
    pub role: String,
    pub content: String,
}

/// A workspace root registered in the database.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Workspace {
    pub id: i64,
    pub root_path: String,
    pub display_name: String,
    pub sort_order: i64,
}

/// Generation lifecycle status stored in `generations.status`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GenerationStatus {
    Running,
    Cancelling,
    WaitingPermission,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

impl GenerationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::WaitingPermission => "waiting_permission",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "cancelling" => Self::Cancelling,
            "waiting_permission" => Self::WaitingPermission,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            "failed" => Self::Failed,
            "interrupted" => Self::Interrupted,
            // Unknown value: treat as failed rather than silently resuming.
            _ => {
                tracing::warn!(value, "unknown generation status");
                Self::Failed
            }
        }
    }

    /// True while the generation may still make progress.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Running | Self::Cancelling | Self::WaitingPermission
        )
    }
}

/// One generation (agent turn) within a session.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Generation {
    pub id: i64,
    pub session_id: i64,
    pub agent_mode: String,
    pub provider: String,
    pub model: String,
    pub status: GenerationStatus,
}

/// One event in the per-session replay log.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GenerationEvent {
    pub seq: i64,
    pub session_id: i64,
    pub generation_id: Option<i64>,
    pub kind: String,
    pub payload_json: String,
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
        // Every connection (runtime + writer) waits for a competing writer
        // instead of failing the batch with SQLITE_BUSY.
        connection
            .busy_timeout(std::time::Duration::from_millis(2_000))
            .expect("set busy timeout");
        schema::migrate(&connection)?;
        let db = Self { connection };
        db.recover_interrupted_generations()?;
        Ok(db)
    }

    /// How long a SQLite access waits for a competing writer before failing.
    /// Relevant once the `Db` is shared behind a mutex across threads.
    pub fn set_busy_timeout(&self, timeout: std::time::Duration) {
        self.connection
            .busy_timeout(timeout)
            .expect("set busy timeout");
    }

    /// Record or clear a session's last error.
    pub fn set_session_last_error(
        &self,
        session_id: i64,
        message: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        self.connection.execute(
            "UPDATE sessions SET last_error = ?2
             WHERE id = ?1",
            params![session_id, message],
        )?;
        Ok(())
    }

    /// Mark generations left active by a previous runtime death as
    /// `interrupted`, and clear their sessions' active pointers. Runs at
    /// open: if this process is starting, no generation can still run.
    fn recover_interrupted_generations(&self) -> Result<(), rusqlite::Error> {
        self.connection.execute(
            "UPDATE generations SET status = 'interrupted',
                 ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE status IN ('running', 'cancelling', 'waiting_permission')",
            [],
        )?;
        self.connection.execute(
            "UPDATE sessions SET active_generation_id = NULL, status = 'idle'
             WHERE active_generation_id IS NOT NULL",
            [],
        )?;
        Ok(())
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
            "INSERT INTO sessions (title) VALUES (?1)
             RETURNING id, title, workspace_id, status, pinned_at",
            params![title],
            Session::from_row,
        )
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>, rusqlite::Error> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {} FROM sessions ORDER BY updated_at DESC, id DESC",
            Session::SELECT_COLUMNS
        ))?;
        let rows = statement.query_map([], Session::from_row)?;
        rows.collect()
    }

    pub fn session(&self, session_id: i64) -> Result<Option<Session>, rusqlite::Error> {
        self.connection
            .query_row(
                &format!("SELECT {} FROM sessions WHERE id = ?1", Session::SELECT_COLUMNS),
                params![session_id],
                Session::from_row,
            )
            .optional()
    }

    pub fn workspace(&self, workspace_id: i64) -> Result<Option<Workspace>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT id, root_path, display_name, sort_order FROM workspaces WHERE id = ?1",
                params![workspace_id],
                |row| {
                    Ok(Workspace {
                        id: row.get(0)?,
                        root_path: row.get(1)?,
                        display_name: row.get(2)?,
                        sort_order: row.get(3)?,
                    })
                },
            )
            .optional()
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

    /// Register a workspace root. Idempotent on `root_path`.
    pub fn create_workspace(
        &self,
        root_path: &str,
        display_name: &str,
    ) -> Result<Workspace, rusqlite::Error> {
        self.connection.query_row(
            "INSERT INTO workspaces (root_path, display_name) VALUES (?1, ?2)
             ON CONFLICT(root_path) DO UPDATE SET last_opened_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             RETURNING id, root_path, display_name, sort_order",
            params![root_path, display_name],
            |row| {
                Ok(Workspace {
                    id: row.get(0)?,
                    root_path: row.get(1)?,
                    display_name: row.get(2)?,
                    sort_order: row.get(3)?,
                })
            },
        )
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, rusqlite::Error> {
        let mut statement = self.connection.prepare(
            "SELECT id, root_path, display_name, sort_order FROM workspaces
             WHERE archived_at IS NULL ORDER BY sort_order, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Workspace {
                id: row.get(0)?,
                root_path: row.get(1)?,
                display_name: row.get(2)?,
                sort_order: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    /// Create a session bound to `workspace_id`.
    pub fn create_session_in_workspace(
        &self,
        workspace_id: i64,
        title: &str,
    ) -> Result<Session, rusqlite::Error> {
        self.connection.query_row(
            "INSERT INTO sessions (title, workspace_id) VALUES (?1, ?2)
             RETURNING id, title, workspace_id, status, pinned_at",
            params![title, workspace_id],
            Session::from_row,
        )
    }

    /// Sessions for one workspace, pinned first, then most recent.
    pub fn list_sessions_in_workspace(
        &self,
        workspace_id: i64,
    ) -> Result<Vec<Session>, rusqlite::Error> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {} FROM sessions
             WHERE workspace_id = ?1 AND archived_at IS NULL
             ORDER BY pinned_at IS NULL, pinned_at DESC, updated_at DESC, id DESC",
            Session::SELECT_COLUMNS
        ))?;
        let rows = statement.query_map(params![workspace_id], Session::from_row)?;
        rows.collect()
    }

    /// Start a generation and mark the session active.
    pub fn start_generation(
        &self,
        session_id: i64,
        agent_mode: &str,
        provider: &str,
        model: &str,
    ) -> Result<Generation, rusqlite::Error> {
        let generation = self.connection.query_row(
            "INSERT INTO generations (session_id, agent_mode, provider, model, status)
             VALUES (?1, ?2, ?3, ?4, 'running')
             RETURNING id, session_id, agent_mode, provider, model, status",
            params![session_id, agent_mode, provider, model],
            |row| {
                Ok(Generation {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    agent_mode: row.get(2)?,
                    provider: row.get(3)?,
                    model: row.get(4)?,
                    status: GenerationStatus::from_str(&row.get::<_, String>(5)?),
                })
            },
        )?;
        self.connection.execute(
            "UPDATE sessions SET active_generation_id = ?1, status = 'running',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?2",
            params![generation.id, session_id],
        )?;
        Ok(generation)
    }

    /// Transition a generation to a terminal (or waiting) state.
    pub fn finish_generation(
        &self,
        generation_id: i64,
        status: GenerationStatus,
        metrics_json: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        self.connection.execute(
            "UPDATE generations SET status = ?2, ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 metrics = COALESCE(?3, metrics)
             WHERE id = ?1",
            params![generation_id, status.as_str(), metrics_json],
        )?;
        self.connection.execute(
            "UPDATE sessions SET active_generation_id = NULL, status = 'idle',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE active_generation_id = ?1",
            params![generation_id],
        )?;
        Ok(())
    }

    /// Request cancellation: `running` → `cancelling`. Worker observes and
    /// finishes with [`GenerationStatus::Cancelled`].
    pub fn request_cancel(&self, generation_id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.connection.execute(
            "UPDATE generations SET status = 'cancelling',
                 cancel_requested_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1 AND status IN ('running', 'waiting_permission')",
            params![generation_id],
        )?;
        Ok(changed > 0)
    }

    /// Mark a waiting generation as running again (permission granted).
    pub fn resume_generation(&self, generation_id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.connection.execute(
            "UPDATE generations SET status = 'running' WHERE id = ?1 AND status = 'waiting_permission'",
            params![generation_id],
        )?;
        Ok(changed > 0)
    }

    /// Fetch a generation's current status.
    pub fn generation_status(
        &self,
        generation_id: i64,
    ) -> Result<Option<GenerationStatus>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT status FROM generations WHERE id = ?1",
                params![generation_id],
                |row| {
                    let value: String = row.get(0)?;
                    Ok(GenerationStatus::from_str(&value))
                },
            )
            .optional()
    }

    /// Generations still active for a session (used by recovery).
    pub fn active_generations(&self, session_id: i64) -> Result<Vec<Generation>, rusqlite::Error> {
        let mut statement = self.connection.prepare(
            "SELECT id, session_id, agent_mode, provider, model, status FROM generations
             WHERE session_id = ?1 AND status IN ('running', 'cancelling', 'waiting_permission')
             ORDER BY id",
        )?;
        let rows = statement.query_map(params![session_id], |row| {
            Ok(Generation {
                id: row.get(0)?,
                session_id: row.get(1)?,
                agent_mode: row.get(2)?,
                provider: row.get(3)?,
                model: row.get(4)?,
                status: GenerationStatus::from_str(&row.get::<_, String>(5)?),
            })
        })?;
        rows.collect()
    }

    /// Append one event to the per-session log; returns its seq.
    pub fn append_event(
        &self,
        session_id: i64,
        generation_id: Option<i64>,
        kind: &str,
        payload_json: &str,
    ) -> Result<i64, rusqlite::Error> {
        let seq = self.connection.query_row(
            "INSERT INTO generation_events (seq, session_id, generation_id, kind, payload_json)
             VALUES (
                 COALESCE((SELECT MAX(seq) + 1 FROM generation_events WHERE session_id = ?1), 0),
                 ?1, ?2, ?3, ?4
             )
             RETURNING seq",
            params![session_id, generation_id, kind, payload_json],
            |row| row.get(0),
        )?;
        self.connection.execute(
            "UPDATE sessions SET last_event_seq = ?2,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1",
            params![session_id, seq],
        )?;
        Ok(seq)
    }

    /// Events after `after_seq` (exclusive), ascending. `after_seq = -1`
    /// returns the full log.
    pub fn events_after(
        &self,
        session_id: i64,
        after_seq: i64,
    ) -> Result<Vec<GenerationEvent>, rusqlite::Error> {
        let mut statement = self.connection.prepare(
            "SELECT seq, session_id, generation_id, kind, payload_json FROM generation_events
             WHERE session_id = ?1 AND seq > ?2 ORDER BY seq",
        )?;
        let rows = statement.query_map(params![session_id, after_seq], |row| {
            Ok(GenerationEvent {
                seq: row.get(0)?,
                session_id: row.get(1)?,
                generation_id: row.get(2)?,
                kind: row.get(3)?,
                payload_json: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Set the pinned timestamp for a session (`None` unpins).
    pub fn set_session_pinned(&self, session_id: i64, pinned: bool) -> Result<(), rusqlite::Error> {
        self.connection.execute(
            "UPDATE sessions SET pinned_at = CASE WHEN ?2 = 1
                 THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE NULL END
             WHERE id = ?1",
            params![session_id, pinned as i64],
        )?;
        Ok(())
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
