//! SQLite sessions and bounded state. Schema in `schema.rs`, storage in
//! `db.rs`, non-blocking batched writes in `writer.rs`.

pub mod db;
pub mod schema;
pub mod writer;

pub use db::{
    ContextEpoch, Db, DbStats, ForkBoundaryType, ForkSessionError, Generation, GenerationEvent,
    GenerationStatus, InputDelivery, InputStatus, MAX_MESSAGE_BYTES, MAX_MESSAGES_PER_SESSION,
    MAX_SESSIONS, MAX_TOOL_OUTPUT_BYTES, Message, MessageTooLarge, NewToolCall, RevertError,
    SavedPermission, SavedPermissionError, Session, SessionInput, SessionRevert, SessionStatus,
    SessionSummary, ToolCall, ToolCallStatus, Workspace,
};
pub use schema::{SCHEMA_VERSION, SchemaTooNew, migrate};
pub use writer::{WRITER_CHANNEL_CAPACITY, WriterHandle};
