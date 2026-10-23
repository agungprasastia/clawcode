//! Session/message storage. Synchronous API; hot-path appends go through
//! `writer.rs` so the render loop never blocks on SQLite.

use crate::persistence::schema;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;

/// Retention limits. Enforced by [`Db::enforce_retention`].
pub const MAX_MESSAGES_PER_SESSION: usize = 1_200;
/// Maximum stored message size in bytes.
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// Maximum stored tool arguments and settlement output size in bytes.
pub const MAX_TOOL_OUTPUT_BYTES: usize = MAX_MESSAGE_BYTES;
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SessionSummary {
    pub id: i64,
    pub title: String,
    pub workspace_id: i64,
    pub created_at: String,
    pub updated_at: String,
    pub pinned: bool,
    pub archived: bool,
    pub parent_id: Option<i64>,
    pub fork_boundary_type: Option<ForkBoundaryType>,
    pub fork_boundary_message_id: Option<i64>,
    pub revert_message_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ForkBoundaryType {
    Before,
    Through,
}

impl ForkBoundaryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::Through => "through",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "before" => Some(Self::Before),
            "through" => Some(Self::Through),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SessionRevert {
    pub message_id: i64,
}

#[derive(Debug)]
pub enum ForkSessionError {
    SessionNotFound,
    MessageNotFound(i64),
    Empty(i64),
    Database(rusqlite::Error),
}

impl std::fmt::Display for ForkSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionNotFound => write!(f, "session not found"),
            Self::MessageNotFound(id) => write!(f, "message not found: msg_{id}"),
            Self::Empty(id) => write!(f, "cannot fork empty session: ses_{id}"),
            Self::Database(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ForkSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for ForkSessionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug)]
pub enum RevertError {
    SessionNotFound,
    MessageNotFound(i64),
    Database(rusqlite::Error),
}

impl std::fmt::Display for RevertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionNotFound => write!(f, "session not found"),
            Self::MessageNotFound(id) => write!(f, "message not found: msg_{id}"),
            Self::Database(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for RevertError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for RevertError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SavedPermission {
    pub id: i64,
    pub project_id: String,
    pub action: String,
    pub resource: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug)]
pub enum SavedPermissionError {
    NotFound(i64),
    Conflict { action: String, resource: String },
    Database(rusqlite::Error),
}

impl std::fmt::Display for SavedPermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "saved permission not found: psv_{id}"),
            Self::Conflict { action, resource } => {
                write!(f, "saved permission already allows {action} for {resource}")
            }
            Self::Database(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SavedPermissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for SavedPermissionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct DbStats {
    pub sessions: u64,
    pub prompts: u64,
    pub steps: u64,
    pub tool_successes: u64,
    pub tool_failures: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub active_days: u64,
}

impl SessionSummary {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let fork_boundary_type = row
            .get::<_, Option<String>>(8)?
            .and_then(|value| ForkBoundaryType::from_str(&value));
        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            workspace_id: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            pinned: row.get::<_, Option<String>>(5)?.is_some(),
            archived: row.get::<_, Option<String>>(6)?.is_some(),
            parent_id: row.get(7)?,
            fork_boundary_type,
            fork_boundary_message_id: row.get(9)?,
            revert_message_id: row.get(10)?,
        })
    }
}

#[derive(Debug)]
pub struct Message {
    pub id: i64,
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InputDelivery {
    Queue,
    Steer,
}

impl InputDelivery {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Steer => "steer",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "steer" => Self::Steer,
            _ => Self::Queue,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InputStatus {
    Pending,
    Promoted,
    Discarded,
}

impl InputStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Promoted => "promoted",
            Self::Discarded => "discarded",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "promoted" => Self::Promoted,
            _ => Self::Discarded,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SessionInput {
    pub id: i64,
    pub session_id: i64,
    pub content: String,
    pub delivery: InputDelivery,
    pub status: InputStatus,
    pub created_at: String,
    pub promoted_at: Option<String>,
    pub user_message_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ToolCallStatus {
    Created,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ToolCallStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "created" => Self::Created,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ToolCall {
    pub id: i64,
    pub session_id: i64,
    pub generation_id: i64,
    pub assistant_message_id: i64,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub status: ToolCallStatus,
    pub result: Option<String>,
    pub error: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewToolCall<'a> {
    pub session_id: i64,
    pub generation_id: i64,
    pub assistant_message_id: i64,
    pub call_id: &'a str,
    pub tool_name: &'a str,
    pub arguments: &'a str,
}

/// Durable Context Epoch boundary stored in `context_epochs`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContextEpoch {
    pub id: i64,
    pub session_id: i64,
    pub epoch_id: String,
    pub baseline_system_text: String,
    pub source_snapshot_json: String,
    pub created_at: String,
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
        connection.busy_timeout(std::time::Duration::from_millis(10_000))?;
        schema::migrate(&connection)?;
        // If workspace 1 exists with an empty root_path, seed it with current working directory
        if let Ok(cwd) = std::env::current_dir() {
            let cwd_str = cwd.to_string_lossy();
            let _ = connection.execute(
                "UPDATE workspaces SET root_path = ?1 WHERE id = 1 AND (root_path = '' OR root_path IS NULL)",
                params![cwd_str],
            );
        }
        let db = Self { connection };
        db.recover_interrupted_generations()?;
        Ok(db)
    }

    /// How long a SQLite access waits for a competing writer before failing.
    /// Relevant once the `Db` is shared behind a mutex across threads.
    pub fn set_busy_timeout(&self, timeout: std::time::Duration) {
        let _ = self
            .connection
            .busy_timeout(timeout.max(std::time::Duration::from_millis(10_000)));
    }

    pub fn transaction_with_behavior(
        &mut self,
        behavior: rusqlite::TransactionBehavior,
    ) -> Result<rusqlite::Transaction<'_>, rusqlite::Error> {
        self.connection.transaction_with_behavior(behavior)
    }

    pub(crate) fn write_transaction(&self) -> Result<rusqlite::Transaction<'_>, rusqlite::Error> {
        rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )
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

    /// Fetch a session's last error message, if any.
    pub fn session_last_error(&self, session_id: i64) -> Result<Option<String>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT last_error FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map(|opt| opt.flatten())
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

    pub fn session_summaries(
        &self,
        include_archived: bool,
        limit: usize,
    ) -> Result<Vec<SessionSummary>, rusqlite::Error> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let archived_filter = if include_archived {
            ""
        } else {
            "WHERE archived_at IS NULL"
        };
        let mut statement = self.connection.prepare(&format!(
            "SELECT id, title, workspace_id, created_at, updated_at, pinned_at, archived_at,
                    parent_id, fork_boundary_type, fork_boundary_message_id, revert_message_id
             FROM sessions
             {archived_filter}
             ORDER BY pinned_at IS NULL, pinned_at DESC, updated_at DESC, id DESC
             LIMIT ?1"
        ))?;
        let rows = statement.query_map(params![limit], SessionSummary::from_row)?;
        rows.collect()
    }

    pub fn rename_session(&self, session_id: i64, title: &str) -> Result<(), rusqlite::Error> {
        self.connection.execute(
            "UPDATE sessions SET title = ?2,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1",
            params![session_id, title],
        )?;
        Ok(())
    }

    pub fn set_session_workspace(
        &self,
        session_id: i64,
        workspace_id: i64,
    ) -> Result<(), rusqlite::Error> {
        self.connection.execute(
            "UPDATE sessions SET workspace_id = ?2 WHERE id = ?1",
            params![session_id, workspace_id],
        )?;
        Ok(())
    }

    pub fn stats(&self) -> Result<DbStats, rusqlite::Error> {
        let sessions = count(&self.connection, "SELECT COUNT(*) FROM sessions")?;
        let prompts = count(
            &self.connection,
            "SELECT COUNT(*) FROM messages WHERE role = 'user'",
        )?;
        let steps = count(&self.connection, "SELECT COUNT(*) FROM tool_calls")?;
        let tool_successes = count(
            &self.connection,
            "SELECT COUNT(*) FROM tool_calls WHERE status = 'completed'",
        )?;
        let tool_failures = count(
            &self.connection,
            "SELECT COUNT(*) FROM tool_calls WHERE status = 'failed'",
        )?;
        let input_tokens = count(
            &self.connection,
            "SELECT COALESCE(SUM(json_extract(metrics, '$.input_tokens')), 0) FROM generations",
        )?;
        let output_tokens = count(
            &self.connection,
            "SELECT COALESCE(SUM(json_extract(metrics, '$.output_tokens')), 0) FROM generations",
        )?;
        let active_days = count(
            &self.connection,
            "SELECT COUNT(DISTINCT date(created_at)) FROM messages",
        )?;
        Ok(DbStats {
            sessions,
            prompts,
            steps,
            tool_successes,
            tool_failures,
            input_tokens,
            output_tokens,
            active_days,
        })
    }

    pub fn session(&self, session_id: i64) -> Result<Option<Session>, rusqlite::Error> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {} FROM sessions WHERE id = ?1",
                    Session::SELECT_COLUMNS
                ),
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

    /// Fetch one session input by row id.
    pub fn session_input(&self, id: i64) -> Result<Option<SessionInput>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT id, session_id, content, delivery, status, created_at, promoted_at, user_message_id
                 FROM session_inputs WHERE id = ?1",
                params![id],
                session_input_from_row,
            )
            .optional()
    }

    /// Fetch session inputs for one session in admission order.
    pub fn session_inputs(&self, session_id: i64) -> Result<Vec<SessionInput>, rusqlite::Error> {
        let mut statement = self.connection.prepare(
            "SELECT id, session_id, content, delivery, status, created_at, promoted_at, user_message_id
             FROM session_inputs WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map(params![session_id], session_input_from_row)?;
        rows.collect()
    }

    /// Fetch the next pending input for a session in admission order.
    pub fn next_pending_input(
        &self,
        session_id: i64,
    ) -> Result<Option<SessionInput>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT id, session_id, content, delivery, status, created_at, promoted_at, user_message_id
                 FROM session_inputs WHERE session_id = ?1 AND status = 'pending' ORDER BY id LIMIT 1",
                params![session_id],
                session_input_from_row,
            )
            .optional()
    }

    /// Check if there are any pending inputs for a session.
    pub fn has_pending_inputs(&self, session_id: i64) -> Result<bool, rusqlite::Error> {
        self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_inputs WHERE session_id = ?1 AND status = 'pending')",
            params![session_id],
            |row| row.get(0),
        )
    }

    /// Atomically admit an input to the inbox and record prompt_admitted event.
    pub fn admit_input(
        &self,
        session_id: i64,
        content: &str,
        delivery: InputDelivery,
    ) -> Result<(SessionInput, i64), rusqlite::Error> {
        if content.len() > MAX_MESSAGE_BYTES {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                MessageTooLarge(content.len()),
            )));
        }
        let tx = self.write_transaction()?;
        let session_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            params![session_id],
            |row| row.get(0),
        )?;
        if !session_exists {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        let input = tx.query_row(
            "INSERT INTO session_inputs (session_id, content, delivery, status)
             VALUES (?1, ?2, ?3, 'pending')
             RETURNING id, session_id, content, delivery, status, created_at, promoted_at, user_message_id",
            params![session_id, content, delivery.as_str()],
            session_input_from_row,
        )?;
        let payload = serde_json::json!({
            "input_id": input.id,
            "session_id": session_id,
            "delivery": delivery.as_str(),
        })
        .to_string();
        let seq = append_event_in_transaction(&tx, session_id, None, "prompt_admitted", &payload)?;
        tx.commit()?;
        Ok((input, seq))
    }

    /// Atomically promote a pending input to a user message and record prompt_promoted event.
    pub fn promote_input(
        &self,
        input_id: i64,
    ) -> Result<(SessionInput, Message, i64), rusqlite::Error> {
        let tx = self.write_transaction()?;
        let (session_id, content): (i64, String) = tx.query_row(
            "SELECT session_id, content FROM session_inputs WHERE id = ?1 AND status = 'pending'",
            params![input_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let message = tx.query_row(
            "INSERT INTO messages (session_id, role, content) VALUES (?1, 'user', ?2)
             RETURNING id, role, content",
            params![session_id, content],
            |row| {
                Ok(Message {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: row.get(2)?,
                })
            },
        )?;
        let input = tx.query_row(
            "UPDATE session_inputs
             SET status = 'promoted',
                 promoted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 user_message_id = ?1
             WHERE id = ?2
             RETURNING id, session_id, content, delivery, status, created_at, promoted_at, user_message_id",
            params![message.id, input_id],
            session_input_from_row,
        )?;
        let payload = serde_json::json!({
            "input_id": input_id,
            "session_id": session_id,
            "user_message_id": message.id,
        })
        .to_string();
        let seq = append_event_in_transaction(&tx, session_id, None, "prompt_promoted", &payload)?;
        tx.commit()?;
        Ok((input, message, seq))
    }
    /// Fetch one durable local tool call by row id.
    pub fn tool_call(&self, id: i64) -> Result<Option<ToolCall>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT id, session_id, generation_id, assistant_message_id,
                        call_id, tool_name, arguments, status, result, error, metadata_json
                 FROM tool_calls WHERE id = ?1",
                params![id],
                tool_call_from_row,
            )
            .optional()
    }

    /// Fetch durable local tool calls for one session in creation order.
    pub fn tool_calls(&self, session_id: i64) -> Result<Vec<ToolCall>, rusqlite::Error> {
        let mut statement = self.connection.prepare(
            "SELECT id, session_id, generation_id, assistant_message_id,
                    call_id, tool_name, arguments, status, result, error, metadata_json
             FROM tool_calls WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map(params![session_id], tool_call_from_row)?;
        rows.collect()
    }

    /// Atomically create a durable call identity and its lifecycle event.
    pub fn create_tool_call(
        &self,
        tool: NewToolCall<'_>,
    ) -> Result<(ToolCall, i64), rusqlite::Error> {
        let NewToolCall {
            session_id,
            generation_id,
            assistant_message_id,
            call_id,
            tool_name,
            arguments,
        } = tool;
        ensure_tool_text_size(arguments)?;
        let tx = self.write_transaction()?;
        let owns_message: bool = tx.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM messages
                 WHERE id = ?1 AND session_id = ?2 AND role = 'assistant'
             ) AND EXISTS(
                 SELECT 1 FROM generations
                 WHERE id = ?3 AND session_id = ?2
             )",
            params![assistant_message_id, session_id, generation_id],
            |row| row.get(0),
        )?;
        if !owns_message {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        tx.execute(
            "INSERT INTO tool_calls (
                 session_id, generation_id, assistant_message_id,
                 call_id, tool_name, arguments, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'created')",
            params![
                session_id,
                generation_id,
                assistant_message_id,
                call_id,
                tool_name,
                arguments
            ],
        )?;
        let id = tx.last_insert_rowid();
        let payload = serde_json::json!({
            "tool_call_id": id,
            "call_id": call_id,
            "tool_name": tool_name,
            "arguments": serde_json::from_str::<serde_json::Value>(arguments)
                .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string())),
            "assistant_message_id": assistant_message_id,
            "generation_id": generation_id,
            "status": ToolCallStatus::Created.as_str(),
        })
        .to_string();
        let seq = append_event_in_transaction(
            &tx,
            session_id,
            Some(generation_id),
            "tool_call_created",
            &payload,
        )?;
        let tool_call = tx.query_row(
            "SELECT id, session_id, generation_id, assistant_message_id,
                    call_id, tool_name, arguments, status, result, error, metadata_json
             FROM tool_calls WHERE id = ?1",
            params![id],
            tool_call_from_row,
        )?;
        tx.commit()?;
        Ok((tool_call, seq))
    }

    /// Atomically mark a created call as running and append its event.
    pub fn start_tool_call(&self, id: i64) -> Result<(ToolCall, i64), rusqlite::Error> {
        let tx = self.write_transaction()?;
        if tx.execute(
            "UPDATE tool_calls SET status = 'running'
             WHERE id = ?1 AND status = 'created'",
            params![id],
        )? != 1
        {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        let tool_call = tx.query_row(
            "SELECT id, session_id, generation_id, assistant_message_id,
                    call_id, tool_name, arguments, status, result, error, metadata_json
             FROM tool_calls WHERE id = ?1",
            params![id],
            tool_call_from_row,
        )?;
        let payload = tool_call_payload(&tool_call, None, None);
        let seq = append_event_in_transaction(
            &tx,
            tool_call.session_id,
            Some(tool_call.generation_id),
            "tool_call_started",
            &payload,
        )?;
        tx.commit()?;
        Ok((tool_call, seq))
    }

    /// Atomically settle a running call and append the terminal event.
    pub fn settle_tool_call(
        &self,
        id: i64,
        status: ToolCallStatus,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Result<(ToolCall, i64), rusqlite::Error> {
        self.settle_tool_call_with_metadata(id, status, result, error, None)
    }

    /// Settle a call while preserving tool-owned metadata (for example a
    /// background shell's `{ "status": "running" }` marker).
    pub fn settle_tool_call_with_metadata(
        &self,
        id: i64,
        status: ToolCallStatus,
        result: Option<&str>,
        error: Option<&str>,
        metadata_json: Option<&str>,
    ) -> Result<(ToolCall, i64), rusqlite::Error> {
        if !matches!(
            status,
            ToolCallStatus::Completed | ToolCallStatus::Failed | ToolCallStatus::Cancelled
        ) {
            return Err(rusqlite::Error::InvalidParameterName(
                "tool call settlement must be terminal".to_string(),
            ));
        }
        if let Some(value) = result {
            ensure_tool_text_size(value)?;
        }
        if let Some(value) = error {
            ensure_tool_text_size(value)?;
        }
        let tx = self.write_transaction()?;
        let updated = match status {
            ToolCallStatus::Failed => tx.execute(
                "UPDATE tool_calls SET status = ?2, result = ?3, error = ?4,
                     metadata_json = COALESCE(?5, metadata_json)
                 WHERE id = ?1 AND status IN ('created', 'running')",
                params![id, status.as_str(), result, error, metadata_json],
            )?,
            ToolCallStatus::Completed | ToolCallStatus::Cancelled => tx.execute(
                "UPDATE tool_calls SET status = ?2, result = ?3, error = ?4,
                     metadata_json = COALESCE(?5, metadata_json)
                 WHERE id = ?1 AND status = 'running'",
                params![id, status.as_str(), result, error, metadata_json],
            )?,
            _ => unreachable!(),
        };
        if updated != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        let tool_call = tx.query_row(
            "SELECT id, session_id, generation_id, assistant_message_id,
                    call_id, tool_name, arguments, status, result, error, metadata_json
             FROM tool_calls WHERE id = ?1",
            params![id],
            tool_call_from_row,
        )?;
        let payload = tool_call_payload(&tool_call, result, error);
        let seq = append_event_in_transaction(
            &tx,
            tool_call.session_id,
            Some(tool_call.generation_id),
            "tool_call_settled",
            &payload,
        )?;
        tx.commit()?;
        Ok((tool_call, seq))
    }

    /// Replace an existing message body. Used to grow the assistant placeholder
    /// created before a streaming turn starts executing tools.
    pub fn update_message_content(
        &self,
        session_id: i64,
        message_id: i64,
        content: &str,
    ) -> Result<Message, rusqlite::Error> {
        if content.len() > MAX_MESSAGE_BYTES {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                MessageTooLarge(content.len()),
            )));
        }
        self.connection.query_row(
            "UPDATE messages SET content = ?3
             WHERE session_id = ?1 AND id = ?2
             RETURNING id, role, content",
            params![session_id, message_id, content],
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

    /// Copy full projected history into a child session.
    pub fn fork_session(&self, source_id: i64) -> Result<Session, ForkSessionError> {
        self.fork_session_before(source_id, None)
    }

    /// Copy projected history up to a boundary. `Some(message_id)` copies
    /// strictly earlier messages; `None` copies through the newest message.
    pub fn fork_session_before(
        &self,
        source_id: i64,
        before: Option<i64>,
    ) -> Result<Session, ForkSessionError> {
        let tx = self.write_transaction()?;
        let (title, workspace_id): (String, i64) = tx
            .query_row(
                "SELECT title, workspace_id FROM sessions WHERE id = ?1",
                params![source_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(ForkSessionError::SessionNotFound)?;
        let boundary_message_id = if let Some(message_id) = before {
            tx.query_row(
                "SELECT id FROM messages WHERE session_id = ?1 AND id = ?2",
                params![source_id, message_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(ForkSessionError::MessageNotFound(message_id))?
        } else {
            tx.query_row(
                "SELECT id FROM messages WHERE session_id = ?1 ORDER BY id DESC LIMIT 1",
                params![source_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(ForkSessionError::Empty(source_id))?
        };
        let boundary_type = if before.is_some() {
            ForkBoundaryType::Before
        } else {
            ForkBoundaryType::Through
        };
        let forked = tx.query_row(
            "INSERT INTO sessions (
                 title, workspace_id, parent_id, fork_boundary_type, fork_boundary_message_id
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             RETURNING id, title, workspace_id, status, pinned_at",
            params![
                title,
                workspace_id,
                source_id,
                boundary_type.as_str(),
                boundary_message_id
            ],
            Session::from_row,
        )?;

        let mut generation_rows = Vec::new();
        {
            let mut statement = tx.prepare(
                "SELECT id, agent_mode, provider, model, status, started_at, ended_at,
                        cancel_requested_at, metrics
                 FROM generations WHERE session_id = ?1 ORDER BY id",
            )?;
            let mut rows = statement.query(params![source_id])?;
            while let Some(row) = rows.next()? {
                generation_rows.push((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ));
            }
        }
        let mut generation_map = HashMap::new();
        for (old_id, agent_mode, provider, model, status, started_at, ended_at, cancel, metrics) in
            generation_rows
        {
            // A fork has no worker, so an active source generation becomes a
            // terminal one rather than an orphan that looks runnable.
            let status = if matches!(
                status.as_str(),
                "running" | "cancelling" | "waiting_permission"
            ) {
                "completed".to_string()
            } else {
                status
            };
            let new_id: i64 = tx.query_row(
                "INSERT INTO generations (
                     session_id, agent_mode, provider, model, status,
                     started_at, ended_at, cancel_requested_at, metrics
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 RETURNING id",
                params![
                    forked.id, agent_mode, provider, model, status, started_at, ended_at, cancel,
                    metrics
                ],
                |row| row.get(0),
            )?;
            generation_map.insert(old_id, new_id);
        }

        let mut message_rows = Vec::new();
        {
            let comparison = if before.is_some() { "<" } else { "<=" };
            let mut statement = tx.prepare(&format!(
                "SELECT id, role, content FROM messages
                 WHERE session_id = ?1 AND id {comparison} ?2 ORDER BY id"
            ))?;
            let mut rows = statement.query(params![source_id, boundary_message_id])?;
            while let Some(row) = rows.next()? {
                message_rows.push((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ));
            }
        }
        let mut message_map = HashMap::new();
        for (old_id, role, content) in message_rows {
            let new_id: i64 = tx.query_row(
                "INSERT INTO messages (session_id, role, content) VALUES (?1, ?2, ?3)
                 RETURNING id",
                params![forked.id, role, content],
                |row| row.get(0),
            )?;
            message_map.insert(old_id, new_id);
        }

        let mut tool_rows = Vec::new();
        {
            let mut statement = tx.prepare(
                "SELECT generation_id, assistant_message_id, call_id, tool_name, arguments,
                        status, result, error, created_at, settled_at, metadata_json
                 FROM tool_calls WHERE session_id = ?1 ORDER BY id",
            )?;
            let mut rows = statement.query(params![source_id])?;
            while let Some(row) = rows.next()? {
                tool_rows.push((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                ));
            }
        }
        for (
            generation_id,
            assistant_message_id,
            call_id,
            tool_name,
            arguments,
            status,
            result,
            error,
            created_at,
            settled_at,
            metadata_json,
        ) in tool_rows
        {
            let Some(generation_id) = generation_map.get(&generation_id) else {
                continue;
            };
            let Some(assistant_message_id) = message_map.get(&assistant_message_id) else {
                continue;
            };
            tx.execute(
                "INSERT INTO tool_calls (
                     session_id, generation_id, assistant_message_id, call_id, tool_name,
                     arguments, status, result, error, created_at, settled_at, metadata_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    forked.id,
                    generation_id,
                    assistant_message_id,
                    call_id,
                    tool_name,
                    arguments,
                    status,
                    result,
                    error,
                    created_at,
                    settled_at,
                    metadata_json
                ],
            )?;
        }

        tx.commit()?;
        Ok(forked)
    }

    /// Stage a revert at `message_id`. The boundary and later messages are
    /// deleted only when the revert is committed.
    pub fn stage_revert(
        &self,
        session_id: i64,
        message_id: i64,
    ) -> Result<SessionRevert, RevertError> {
        let tx = self.write_transaction()?;
        let session_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            params![session_id],
            |row| row.get(0),
        )?;
        if !session_exists {
            return Err(RevertError::SessionNotFound);
        }
        let message_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE session_id = ?1 AND id = ?2)",
            params![session_id, message_id],
            |row| row.get(0),
        )?;
        if !message_exists {
            return Err(RevertError::MessageNotFound(message_id));
        }
        tx.execute(
            "UPDATE sessions SET revert_message_id = ?2,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1",
            params![session_id, message_id],
        )?;
        tx.commit()?;
        Ok(SessionRevert { message_id })
    }

    /// Discard a staged revert without deleting history.
    pub fn clear_revert(&self, session_id: i64) -> Result<(), RevertError> {
        let tx = self.write_transaction()?;
        let changed = tx.execute(
            "UPDATE sessions SET revert_message_id = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1",
            params![session_id],
        )?;
        if changed == 0 {
            return Err(RevertError::SessionNotFound);
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete the staged boundary and everything after it, then clear the
    /// staged revert. No-op when no revert is staged.
    pub fn commit_revert(&self, session_id: i64) -> Result<(), RevertError> {
        let tx = self.write_transaction()?;
        let revert = tx
            .query_row(
                "SELECT revert_message_id FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?;
        let Some(revert) = revert else {
            return Err(RevertError::SessionNotFound);
        };
        let Some(boundary) = revert else {
            tx.commit()?;
            return Ok(());
        };
        let created_at = tx
            .query_row(
                "SELECT created_at FROM messages WHERE id = ?1 AND session_id = ?2",
                params![boundary, session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(created_at) = created_at else {
            return Err(RevertError::MessageNotFound(boundary));
        };
        tx.execute(
            "DELETE FROM session_inputs
             WHERE session_id = ?1
               AND (
                   (user_message_id IS NOT NULL AND user_message_id >= ?2)
                   OR (status = 'pending' AND created_at >= ?3)
               )",
            params![session_id, boundary, created_at],
        )?;
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND id >= ?2",
            params![session_id, boundary],
        )?;
        tx.execute(
            "UPDATE sessions SET revert_message_id = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1",
            params![session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// List saved permissions, optionally scoped to one project.
    pub fn saved_permissions(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<SavedPermission>, rusqlite::Error> {
        let mut statement = self.connection.prepare(
            "SELECT id, project_id, action, resource, created_at, updated_at
             FROM saved_permissions
             WHERE ?1 IS NULL OR project_id = ?1
             ORDER BY id",
        )?;
        let rows = statement.query_map(params![project_id], |row| {
            Ok(SavedPermission {
                id: row.get(0)?,
                project_id: row.get(1)?,
                action: row.get(2)?,
                resource: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    /// Save "always allow" resources. Existing identical rules are ignored.
    pub fn add_saved_permissions(
        &self,
        project_id: &str,
        action: &str,
        resources: &[String],
    ) -> Result<(), rusqlite::Error> {
        let tx = self.write_transaction()?;
        {
            let mut statement = tx.prepare(
                "INSERT OR IGNORE INTO saved_permissions (project_id, action, resource)
                 VALUES (?1, ?2, ?3)",
            )?;
            for resource in resources {
                statement.execute(params![project_id, action, resource])?;
            }
        }
        tx.commit()
    }

    /// Update the action or resource of a saved permission.
    pub fn update_saved_permission(
        &self,
        id: i64,
        action: Option<&str>,
        resource: Option<&str>,
    ) -> Result<SavedPermission, SavedPermissionError> {
        let current = self
            .saved_permission(id)?
            .ok_or(SavedPermissionError::NotFound(id))?;
        let action = action.unwrap_or(&current.action);
        let resource = resource.unwrap_or(&current.resource);
        let result = self.connection.query_row(
            "UPDATE saved_permissions SET action = ?2, resource = ?3,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE id = ?1
             RETURNING id, project_id, action, resource, created_at, updated_at",
            params![id, action, resource],
            |row| {
                Ok(SavedPermission {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    action: row.get(2)?,
                    resource: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        );
        match result {
            Ok(saved) => Ok(saved),
            Err(error) if is_unique_violation(&error) => Err(SavedPermissionError::Conflict {
                action: action.to_string(),
                resource: resource.to_string(),
            }),
            Err(error) => Err(SavedPermissionError::Database(error)),
        }
    }

    fn saved_permission(&self, id: i64) -> Result<Option<SavedPermission>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT id, project_id, action, resource, created_at, updated_at
                 FROM saved_permissions WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SavedPermission {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        action: row.get(2)?,
                        resource: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    /// Remove a saved permission. Missing rows are a successful no-op.
    pub fn remove_saved_permission(&self, id: i64) -> Result<(), rusqlite::Error> {
        self.connection
            .execute("DELETE FROM saved_permissions WHERE id = ?1", params![id])?;
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

    /// Update the root path for a workspace.
    pub fn update_workspace_root_path(
        &self,
        id: i64,
        root_path: &str,
    ) -> Result<(), rusqlite::Error> {
        self.connection.execute(
            "UPDATE workspaces SET root_path = ?1 WHERE id = ?2",
            params![root_path, id],
        )?;
        Ok(())
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

    /// Most recent generation, used to restore a session's agent and model.
    pub fn last_generation(&self, session_id: i64) -> Result<Option<Generation>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT id, session_id, agent_mode, provider, model, status
                 FROM generations WHERE session_id = ?1
                 ORDER BY id DESC LIMIT 1",
                params![session_id],
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

    /// Fetch the active (latest) context epoch for a session.
    pub fn get_active_context_epoch(
        &self,
        session_id: i64,
    ) -> Result<Option<ContextEpoch>, rusqlite::Error> {
        self.connection
            .query_row(
                "SELECT id, session_id, epoch_id, baseline_system_text, source_snapshot_json, created_at
                 FROM context_epochs
                 WHERE session_id = ?1
                 ORDER BY id DESC
                 LIMIT 1",
                params![session_id],
                context_epoch_from_row,
            )
            .optional()
    }

    /// Insert a new context epoch boundary for a session.
    pub fn insert_context_epoch(
        &self,
        session_id: i64,
        epoch_id: &str,
        baseline_system_text: &str,
        source_snapshot_json: &str,
    ) -> Result<ContextEpoch, rusqlite::Error> {
        if baseline_system_text.len() > MAX_MESSAGE_BYTES {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                MessageTooLarge(baseline_system_text.len()),
            )));
        }
        self.connection.query_row(
            "INSERT INTO context_epochs (session_id, epoch_id, baseline_system_text, source_snapshot_json)
             VALUES (?1, ?2, ?3, ?4)
             RETURNING id, session_id, epoch_id, baseline_system_text, source_snapshot_json, created_at",
            params![session_id, epoch_id, baseline_system_text, source_snapshot_json],
            context_epoch_from_row,
        )
    }

    /// Update the snapshot JSON for an active context epoch.
    pub fn update_context_epoch_snapshot(
        &self,
        session_id: i64,
        epoch_id: &str,
        new_snapshot_json: &str,
    ) -> Result<bool, rusqlite::Error> {
        let rows = self.connection.execute(
            "UPDATE context_epochs
             SET source_snapshot_json = ?1
             WHERE session_id = ?2 AND epoch_id = ?3",
            params![new_snapshot_json, session_id, epoch_id],
        )?;
        Ok(rows > 0)
    }

    /// Atomically update epoch snapshot and append a system delta message.
    pub fn reconcile_epoch_change(
        &self,
        session_id: i64,
        epoch_id: &str,
        new_snapshot_json: &str,
        delta_system_message: &str,
    ) -> Result<Message, rusqlite::Error> {
        if delta_system_message.len() > MAX_MESSAGE_BYTES {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                MessageTooLarge(delta_system_message.len()),
            )));
        }
        let tx = self.write_transaction()?;
        let rows = tx.execute(
            "UPDATE context_epochs
             SET source_snapshot_json = ?1
             WHERE session_id = ?2 AND epoch_id = ?3",
            params![new_snapshot_json, session_id, epoch_id],
        )?;
        if rows == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        let message = tx.query_row(
            "INSERT INTO messages (session_id, role, content) VALUES (?1, 'system', ?2)
             RETURNING id, role, content",
            params![session_id, delta_system_message],
            |row| {
                Ok(Message {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: row.get(2)?,
                })
            },
        )?;
        tx.commit()?;
        Ok(message)
    }
}

fn ensure_tool_text_size(value: &str) -> Result<(), rusqlite::Error> {
    if value.len() > MAX_TOOL_OUTPUT_BYTES {
        return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
            MessageTooLarge(value.len()),
        )));
    }
    Ok(())
}

fn tool_call_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolCall> {
    Ok(ToolCall {
        id: row.get(0)?,
        session_id: row.get(1)?,
        generation_id: row.get(2)?,
        assistant_message_id: row.get(3)?,
        call_id: row.get(4)?,
        tool_name: row.get(5)?,
        arguments: row.get(6)?,
        status: ToolCallStatus::from_str(&row.get::<_, String>(7)?),
        result: row.get(8)?,
        error: row.get(9)?,
        metadata_json: row.get(10)?,
    })
}

fn count(connection: &Connection, sql: &str) -> rusqlite::Result<u64> {
    let value: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    Ok(u64::try_from(value).unwrap_or(0))
}

fn session_input_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionInput> {
    Ok(SessionInput {
        id: row.get(0)?,
        session_id: row.get(1)?,
        content: row.get(2)?,
        delivery: InputDelivery::from_str(&row.get::<_, String>(3)?),
        status: InputStatus::from_str(&row.get::<_, String>(4)?),
        created_at: row.get(5)?,
        promoted_at: row.get(6)?,
        user_message_id: row.get(7)?,
    })
}

fn context_epoch_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextEpoch> {
    Ok(ContextEpoch {
        id: row.get(0)?,
        session_id: row.get(1)?,
        epoch_id: row.get(2)?,
        baseline_system_text: row.get(3)?,
        source_snapshot_json: row.get(4)?,
        created_at: row.get(5)?,
    })
}
fn is_unique_violation(error: &rusqlite::Error) -> bool {
    match error {
        rusqlite::Error::SqliteFailure(details, _) => {
            details.code == rusqlite::ErrorCode::ConstraintViolation
                && details.extended_code == 2067
        }
        _ => false,
    }
}

fn tool_call_payload(tool_call: &ToolCall, result: Option<&str>, error: Option<&str>) -> String {
    serde_json::json!({
        "tool_call_id": tool_call.id,
        "call_id": tool_call.call_id,
        "tool_name": tool_call.tool_name,
        "arguments": serde_json::from_str::<serde_json::Value>(&tool_call.arguments)
            .unwrap_or_else(|_| serde_json::Value::String(tool_call.arguments.clone())),
        "assistant_message_id": tool_call.assistant_message_id,
        "generation_id": tool_call.generation_id,
        "status": tool_call.status.as_str(),
        "result": result,
        "error": error,
        "metadata": tool_call.metadata_json.as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok()),
    })
    .to_string()
}

fn append_event_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    session_id: i64,
    generation_id: Option<i64>,
    kind: &str,
    payload_json: &str,
) -> Result<i64, rusqlite::Error> {
    let seq = tx.query_row(
        "INSERT INTO generation_events (seq, session_id, generation_id, kind, payload_json)
         VALUES (
             COALESCE((SELECT MAX(seq) + 1 FROM generation_events WHERE session_id = ?1), 0),
             ?1, ?2, ?3, ?4
         )
         RETURNING seq",
        params![session_id, generation_id, kind, payload_json],
        |row| row.get(0),
    )?;
    tx.execute(
        "UPDATE sessions SET last_event_seq = MAX(last_event_seq, ?2),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
         WHERE id = ?1",
        params![session_id, seq],
    )?;
    Ok(seq)
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
