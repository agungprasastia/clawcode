mod files;
mod policy;
mod root;
mod shell;

use crate::core::error::{Diagnostic, ErrorCategory};
use std::path::Path;

pub use files::{FileSystem, RealFileSystem};
pub use policy::{Mode, Operation, Policy, PolicyDecision, Risk};
pub use root::WorkspaceRoot;
pub use shell::classify_shell;

#[derive(Debug, Eq, PartialEq)]
pub struct ReadResult {
    pub bytes: Vec<u8>,
    pub truncated: bool,
}

pub struct Workspace {
    root: WorkspaceRoot,
    filesystem: RealFileSystem,
}

impl Workspace {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Diagnostic> {
        Ok(Self {
            root: WorkspaceRoot::open(path)?,
            filesystem: RealFileSystem,
        })
    }

    pub fn read(
        &self,
        relative: impl AsRef<Path>,
        max_bytes: usize,
    ) -> Result<ReadResult, Diagnostic> {
        let path = self.root.resolve(relative)?;
        let mut bytes = self.filesystem.read(&path).map_err(|error| {
            Diagnostic::new(
                ErrorCategory::Workspace,
                format!("failed to read {}: {error}", path.display()),
            )
        })?;
        let truncated = bytes.len() > max_bytes;
        bytes.truncate(max_bytes);
        Ok(ReadResult { bytes, truncated })
    }

    pub fn validate_shell(
        &self,
        mode: Mode,
        relative_cwd: impl AsRef<Path>,
        command: &str,
    ) -> Result<PolicyDecision, Diagnostic> {
        self.root.resolve(relative_cwd)?;
        Ok(Policy::evaluate(
            mode,
            Operation::Shell(classify_shell(command)),
        ))
    }
}
