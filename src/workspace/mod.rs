mod files;
mod policy;
mod root;
mod shell;
mod snapshot;

use crate::core::error::{Diagnostic, ErrorCategory};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

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

pub struct Workspace<F: FileSystem = RealFileSystem> {
    root: WorkspaceRoot,
    filesystem: F,
    snapshots: Mutex<SnapshotStore>,
}

impl Workspace<RealFileSystem> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Diagnostic> {
        Self::with_filesystem(path, RealFileSystem)
    }
}

impl<F: FileSystem> Workspace<F> {
    pub fn with_filesystem(root: impl AsRef<Path>, filesystem: F) -> Result<Self, Diagnostic> {
        Ok(Self {
            root: WorkspaceRoot::open(root)?,
            filesystem,
            snapshots: Mutex::new(SnapshotStore::new()),
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
        let mut resolved = Vec::with_capacity(mutations.len());
        let mut paths = HashSet::new();
        for mutation in mutations {
            let (relative, after, operation) = match mutation {
                Mutation::Write { path, bytes } => {
                    (path, FileState::present(bytes), Operation::Write)
                }
                Mutation::Delete { path } => (path, FileState::Missing, Operation::Delete),
            };
            let path = self.root.resolve(&relative)?;
            if !paths.insert(path.clone()) {
                return Err(Diagnostic::new(
                    ErrorCategory::Workspace,
                    format!("duplicate mutation path: {}", path.display()),
                ));
            }
            resolved.push((path, after, operation));
        }

        let mut prepared = Vec::with_capacity(resolved.len());
        for (path, after, operation) in resolved {
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

        let mut diffs: Vec<_> = prepared
            .iter()
            .map(|(path, before, after, _)| Diff {
                path: path.clone(),
                before: before.clone(),
                after: after.clone(),
            })
            .collect();
        diffs.sort_by(|left, right| left.path.cmp(&right.path));
        let decisions: Vec<_> = prepared
            .iter()
            .map(|(_, _, _, operation)| Policy::evaluate(mode, *operation))
            .collect();
        if decisions.contains(&PolicyDecision::Denied) {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                "workspace mutation denied by policy",
            ));
        }
        if prepared
            .iter()
            .zip(&decisions)
            .any(|(_, decision)| *decision == PolicyDecision::ApprovalRequired)
            && !approved
        {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                "workspace mutation requires approval",
            ));
        }

        let mut snapshots = self.snapshots.lock().map_err(|_| {
            Diagnostic::new(ErrorCategory::Workspace, "snapshot store lock poisoned")
        })?;
        let checkpoint = snapshots.checkpoint();
        let mut entries = Vec::with_capacity(prepared.len());
        for (path, before, after, operation) in prepared {
            let id = match snapshots.capture(path.clone(), before.clone(), after.clone()) {
                Ok(id) => id,
                Err(error) => {
                    snapshots.discard(checkpoint);
                    return Err(error);
                }
            };
            entries.push((id, path, before, after, operation));
        }

        let mut applied = Vec::new();
        for ((id, path, _, after, _), _) in entries.iter().zip(&decisions) {
            let result = match after {
                FileState::Missing if self.filesystem.exists(path) => {
                    self.filesystem.remove_file(path)
                }
                FileState::Missing => Ok(()),
                FileState::Present { bytes, .. } => {
                    let temporary =
                        crate::workspace::files::temporary_sibling(path, &id.value().to_string())
                            .map_err(|error| diagnostic("create mutation temporary", path, error))?;
                    match self.filesystem.write_new(&temporary, bytes) {
                        Ok(()) => match self.filesystem.replace(&temporary, path) {
                            Ok(()) => Ok(()),
                            Err(error) => {
                                let _ = self.filesystem.remove_file(&temporary);
                                Err(error)
                            }
                        },
                        Err(error) => Err(error),
                    }
                }
            };
            if let Err(error) = result {
                let mut rollback_failure = None;
                for applied_id in applied.iter().rev() {
                    if let Err(rollback_error) =
                        snapshots.restore_before(*applied_id, &self.filesystem)
                    {
                        rollback_failure = Some(rollback_error.to_string());
                        break;
                    }
                }
                let mut diagnostic = diagnostic("apply workspace mutation", path, error);
                if let Some(rollback_failure) = rollback_failure {
                    diagnostic = Diagnostic::new(
                        ErrorCategory::Workspace,
                        format!("{diagnostic}; rollback failed: {rollback_failure}"),
                    );
                }
                snapshots.discard(checkpoint);
                return Err(diagnostic);
            }
            applied.push(*id);
        }
        Ok(TransactionResult {
            snapshot_ids: entries.iter().map(|entry| entry.0).collect(),
            diffs,
        })
    }

    pub fn restore_before(&self, id: SnapshotId) -> Result<(), Diagnostic> {
        self.snapshots
            .lock()
            .map_err(|_| Diagnostic::new(ErrorCategory::Workspace, "snapshot store lock poisoned"))?
            .restore_before(id, &self.filesystem)
    }

    pub fn restore_after(&self, id: SnapshotId) -> Result<(), Diagnostic> {
        self.snapshots
            .lock()
            .map_err(|_| Diagnostic::new(ErrorCategory::Workspace, "snapshot store lock poisoned"))?
            .restore_after(id, &self.filesystem)
    }
}

fn diagnostic(action: &str, path: &Path, error: std::io::Error) -> Diagnostic {
    Diagnostic::new(
        ErrorCategory::Workspace,
        format!("failed to {action} {}: {error}", path.display()),
    )
}
