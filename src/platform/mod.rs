use std::fmt;
use std::path::PathBuf;

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
}

pub trait ShellDiscovery {
    fn find(&self, name: &str) -> Result<PathBuf, PlatformError>;
}

pub trait CredentialStore {
    fn get(&self, key: &str) -> Result<String, PlatformError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedCredentialStore;

impl CredentialStore for UnsupportedCredentialStore {
    fn get(&self, _key: &str) -> Result<String, PlatformError> {
        Err(PlatformError::Unsupported(
            "credential store is not available in MVP".into(),
        ))
    }
}
