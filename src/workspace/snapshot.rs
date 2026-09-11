use crate::core::error::{Diagnostic, ErrorCategory};
use crate::workspace::FileSystem;
use crate::workspace::files::temporary_sibling;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SnapshotId(u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileState {
    Missing,
    Present { bytes: Vec<u8>, checksum: [u8; 32] },
}

impl FileState {
    pub fn present(bytes: Vec<u8>) -> Self {
        let checksum = Sha256::digest(&bytes).into();
        Self::Present { bytes, checksum }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub path: PathBuf,
    pub before: FileState,
    pub after: FileState,
}

#[derive(Default)]
pub struct SnapshotStore {
    snapshots: Vec<Snapshot>,
    bytes: usize,
}

impl SnapshotStore {
    pub const MAX_BYTES: usize = 8 * 1024 * 1024;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn capture(
        &mut self,
        path: PathBuf,
        before: FileState,
        after: FileState,
    ) -> Result<SnapshotId, Diagnostic> {
        let bytes = state_size(&before).saturating_add(state_size(&after));
        if bytes > Self::MAX_BYTES.saturating_sub(self.bytes) {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                "snapshot storage limit exceeded",
            ));
        }

        let id = SnapshotId(self.snapshots.len() as u64);
        self.snapshots.push(Snapshot {
            id,
            path,
            before,
            after,
        });
        self.bytes += bytes;
        Ok(id)
    }

    pub fn restore_before(
        &self,
        id: SnapshotId,
        filesystem: &dyn FileSystem,
    ) -> Result<(), Diagnostic> {
        self.restore(id, filesystem, false)
    }

    pub fn restore_after(
        &self,
        id: SnapshotId,
        filesystem: &dyn FileSystem,
    ) -> Result<(), Diagnostic> {
        self.restore(id, filesystem, true)
    }

    fn restore(
        &self,
        id: SnapshotId,
        filesystem: &dyn FileSystem,
        after: bool,
    ) -> Result<(), Diagnostic> {
        let snapshot = self
            .snapshots
            .get(id.0 as usize)
            .ok_or_else(|| Diagnostic::new(ErrorCategory::Workspace, "snapshot does not exist"))?;
        let state = if after {
            &snapshot.after
        } else {
            &snapshot.before
        };

        match state {
            FileState::Missing => {
                if filesystem.exists(&snapshot.path) {
                    filesystem.remove_file(&snapshot.path).map_err(|error| {
                        diagnostic("remove snapshot target", &snapshot.path, error)
                    })?;
                }
            }
            FileState::Present { bytes, checksum } => {
                if Sha256::digest(bytes).as_slice() != checksum {
                    return Err(Diagnostic::new(
                        ErrorCategory::Workspace,
                        "snapshot checksum validation failed",
                    ));
                }
                let temporary =
                    temporary_sibling(&snapshot.path, &id.0.to_string()).map_err(|error| {
                        diagnostic("create snapshot temporary path", &snapshot.path, error)
                    })?;
                if let Err(error) = filesystem.write(&temporary, bytes) {
                    let _ = filesystem.remove_file(&temporary);
                    return Err(diagnostic("write snapshot temporary", &temporary, error));
                }
                if let Err(error) = filesystem.replace(&temporary, &snapshot.path) {
                    let _ = filesystem.remove_file(&temporary);
                    return Err(diagnostic("replace snapshot target", &snapshot.path, error));
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn tamper_before(&mut self, id: SnapshotId) {
        if let Some(Snapshot {
            before: FileState::Present { bytes, .. },
            ..
        }) = self.snapshots.get_mut(id.0 as usize)
        {
            bytes.push(0);
        }
    }
}

fn state_size(state: &FileState) -> usize {
    match state {
        FileState::Missing => 0,
        FileState::Present { bytes, .. } => bytes.len(),
    }
}

fn diagnostic(action: &str, path: &std::path::Path, error: std::io::Error) -> Diagnostic {
    Diagnostic::new(
        ErrorCategory::Workspace,
        format!("failed to {action} {}: {error}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::{FileState, SnapshotStore};
    use crate::core::error::ErrorCategory;
    use crate::workspace::RealFileSystem;
    use std::fs;

    #[test]
    fn restore_rejects_tampered_stored_bytes() {
        let directory = std::env::temp_dir().join("clawcode-snapshot-tamper-test");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        let path = directory.join("note.txt");
        let mut snapshots = SnapshotStore::new();
        let id = snapshots
            .capture(
                path.clone(),
                FileState::present(b"before".to_vec()),
                FileState::Missing,
            )
            .unwrap();
        snapshots.tamper_before(id);

        let error = snapshots.restore_before(id, &RealFileSystem).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::Workspace);
        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
