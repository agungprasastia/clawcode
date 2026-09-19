use std::fmt;
use std::path::PathBuf;

mod clipboard;
mod credentials;
pub mod git;
mod shell;

pub use clipboard::{BackendClipboard, ClipboardBackend, SystemClipboard, trim_clipboard_newlines};
pub use credentials::UnsupportedCredentialStore;
pub use git::{get_branch_for_path, get_current_branch, is_git_repo};
pub use shell::PathShellDiscovery;

pub const MAX_CLIPBOARD_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    InvalidInput(String),
    Unavailable(String),
    Unsupported(String),
    NotFound(String),
    Io(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message)
            | Self::Unavailable(message)
            | Self::Unsupported(message)
            | Self::NotFound(message)
            | Self::Io(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for PlatformError {}

pub trait Clipboard {
    fn read(&self) -> Result<String, PlatformError>;
    fn write(&self, text: &str) -> Result<(), PlatformError>;
    fn get_text(&self) -> Result<String, PlatformError> {
        self.read()
    }
    fn set_text(&self, text: &str) -> Result<(), PlatformError> {
        self.write(text)
    }
}

pub trait ShellDiscovery {
    fn find(&self, name: &str) -> Result<PathBuf, PlatformError>;
}

pub trait CredentialStore {
    fn get(&self, key: &str) -> Result<String, PlatformError>;
}
