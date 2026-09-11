mod files;
mod policy;
mod root;
mod shell;
mod snapshot;

use crate::core::error::{Diagnostic, ErrorCategory};
use std::path::Path;

pub use files::{FileSystem, RealFileSystem};
pub use policy::{Mode, Operation, Policy, PolicyDecision, Risk};
pub use root::WorkspaceRoot;
pub use shell::classify_shell;
pub use snapshot::{Diff, FileState, Snapshot, SnapshotId, SnapshotStore};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Mutation {
    Write {
        path: std::path::PathBuf,
        bytes: Vec<u8>,
    },
    Delete {
        path: std::path::PathBuf,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionResult {
    pub snapshot_ids: Vec<SnapshotId>,
    pub diffs: Vec<Diff>,
}

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

    pub fn build(
        &self,
        mode: Mode,
        mutations: Vec<Mutation>,
        approved: bool,
    ) -> Result<TransactionResult, Diagnostic> {
        let mut prepared = Vec::with_capacity(mutations.len());
        for mutation in mutations {
            let (relative, after, operation) = match mutation {
                Mutation::Write { path, bytes } => {
                    (path, FileState::present(bytes), Operation::Write)
                }
                Mutation::Delete { path } => (path, FileState::Missing, Operation::Delete),
            };
            let path = self.root.resolve(&relative)?;
            let before = if self.filesystem.exists(&path) {
                FileState::present(
                    self.filesystem
                        .read(&path)
                        .map_err(|error| diagnostic("read mutation target", &path, error))?,
                )
            } else {
                FileState::Missing
            };
            let operation =
                if matches!(operation, Operation::Write) && !matches!(before, FileState::Missing) {
                    Operation::SensitiveWrite
                } else {
                    operation
                };
            prepared.push((path, before, after, operation));
        }

        let mut snapshots = SnapshotStore::new();
        let mut entries = Vec::with_capacity(prepared.len());
        for (path, before, after, operation) in prepared {
            let decision = Policy::evaluate(mode, operation);
            if decision == PolicyDecision::Denied {
                return Err(Diagnostic::new(
                    ErrorCategory::Workspace,
                    "workspace mutation denied by policy",
                ));
            }
            let id = snapshots.capture(path.clone(), before.clone(), after.clone())?;
            entries.push((id, path, before, after, decision));
        }
        if entries
            .iter()
            .any(|(_, _, _, _, decision)| *decision == PolicyDecision::ApprovalRequired)
            && !approved
        {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                "workspace mutation requires approval",
            ));
        }

        let mut applied = Vec::new();
        for (id, path, _, after, _) in &entries {
            let result = match after {
                FileState::Missing => self.filesystem.remove_file(path),
                FileState::Present { bytes, .. } => {
                    let temporary =
                        crate::workspace::files::temporary_sibling(path, &id.value().to_string())
                            .map_err(|error| diagnostic("create mutation temporary", path, error))?;
                    let result = self
                        .filesystem
                        .write(&temporary, bytes)
                        .and_then(|_| self.filesystem.replace(&temporary, path));
                    if result.is_err() {
                        let _ = self.filesystem.remove_file(&temporary);
                    }
                    result
                }
            };
            if let Err(error) = result {
                for applied_id in applied.iter().rev() {
                    let _ = snapshots.restore_before(*applied_id, &self.filesystem);
                }
                return Err(diagnostic("apply workspace mutation", path, error));
            }
            applied.push(*id);
        }
        let mut diffs: Vec<_> = entries
            .iter()
            .map(|(_, path, before, after, _)| Diff {
                path: path.clone(),
                before: before.clone(),
                after: after.clone(),
            })
            .collect();
        diffs.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(TransactionResult {
            snapshot_ids: entries.iter().map(|entry| entry.0).collect(),
            diffs,
        })
    }
}

fn diagnostic(action: &str, path: &Path, error: std::io::Error) -> Diagnostic {
    Diagnostic::new(
        ErrorCategory::Workspace,
        format!("failed to {action} {}: {error}", path.display()),
    )
}
