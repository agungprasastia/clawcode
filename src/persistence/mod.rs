//! SQLite sessions and bounded state. Schema in `schema.rs`, storage in
//! `db.rs`, non-blocking batched writes in `writer.rs`.

pub mod db;
pub mod schema;
pub mod writer;

pub use db::{
    Db, Generation, GenerationEvent, GenerationStatus, MAX_MESSAGE_BYTES, MAX_MESSAGES_PER_SESSION,
    MAX_SESSIONS, MAX_TOOL_OUTPUT_BYTES, Message, MessageTooLarge, Session, SessionStatus,
    ToolCall, ToolCallStatus, Workspace,
};
pub use schema::{SCHEMA_VERSION, SchemaTooNew, migrate};
pub use writer::{WRITER_CHANNEL_CAPACITY, WriterHandle};
