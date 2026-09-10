//! SQLite sessions and bounded state. Schema in `schema.rs`, storage in
//! `db.rs`, non-blocking batched writes in `writer.rs`.

pub mod db;
pub mod schema;
pub mod writer;

pub use db::{
    Db, MAX_MESSAGE_BYTES, MAX_MESSAGES_PER_SESSION, MAX_SESSIONS, Message, MessageTooLarge,
    Session,
};
pub use schema::{SCHEMA_VERSION, SchemaTooNew, migrate};
pub use writer::{WRITER_CHANNEL_CAPACITY, WriterHandle};
