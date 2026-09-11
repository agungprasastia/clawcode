use clawcode::core::error::ErrorCategory;
use clawcode::workspace::{
    FileState, FileSystem, Mode, Mutation, Operation, Policy, PolicyDecision, ReadResult,
    RealFileSystem, Risk, SnapshotStore, Workspace, WorkspaceRoot, classify_shell,
};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_directory() -> TestDirectory {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("clawcode-workspace-{unique}-{sequence}"));
    fs::create_dir(&path).unwrap();
    TestDirectory(path)
}

#[derive(Clone, Copy)]
enum RestoreFailure {
    Write,
    Rename,
}

struct FailingFileSystem {
    failure: RestoreFailure,
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
}

#[derive(Clone)]
struct FaultInjectingFileSystem {
    state: Arc<FaultInjectingState>,
}

struct FaultInjectingState {
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
    replace_count: AtomicUsize,
    fail_replace_at: usize,
}

struct ReadTrackingFileSystem {
    reads: Arc<AtomicUsize>,
}

impl FileSystem for ReadTrackingFileSystem {
    fn read(&self, _path: &Path) -> io::Result<Vec<u8>> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(Vec::new())
    }

    fn write(&self, _path: &Path, _bytes: &[u8]) -> io::Result<()> {
        Ok(())
    }

    fn replace(&self, _from: &Path, _to: &Path) -> io::Result<()> {
        Ok(())
    }

    fn remove_file(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn exists(&self, _path: &Path) -> bool {
        self.reads.fetch_add(1, Ordering::Relaxed);
        false
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

impl FaultInjectingFileSystem {
    fn with_file(path: PathBuf, bytes: Vec<u8>, fail_replace_at: usize) -> Self {
        Self {
            state: Arc::new(FaultInjectingState {
                files: Mutex::new(HashMap::from([(path, bytes)])),
                replace_count: AtomicUsize::new(0),
                fail_replace_at,
            }),
        }
    }

    fn bytes(&self, path: &Path) -> Option<Vec<u8>> {
        self.state.files.lock().unwrap().get(path).cloned()
    }
}

impl FileSystem for FaultInjectingFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.bytes(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing file"))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.state
            .files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    fn replace(&self, from: &Path, to: &Path) -> io::Result<()> {
        let replace_number = self.state.replace_count.fetch_add(1, Ordering::Relaxed) + 1;
        if replace_number == self.state.fail_replace_at {
            return Err(io::Error::other("injected replacement failure"));
        }
        let bytes = self
            .state
            .files
            .lock()
            .unwrap()
            .remove(from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing temporary file"))?;
        self.state
            .files
            .lock()
            .unwrap()
            .insert(to.to_path_buf(), bytes);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.state.files.lock().unwrap().remove(path);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.state.files.lock().unwrap().contains_key(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

impl FailingFileSystem {
    fn new(failure: RestoreFailure) -> Self {
        Self {
            failure,
            files: Mutex::new(HashMap::new()),
        }
    }
}

impl FileSystem for FailingFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing file"))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), bytes.to_vec());
        match self.failure {
            RestoreFailure::Write => Err(io::Error::other("write failed")),
            RestoreFailure::Rename => Ok(()),
        }
    }

    fn replace(&self, _from: &Path, _to: &Path) -> io::Result<()> {
        Err(io::Error::other("rename failed"))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

fn create_directory_symlink(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).unwrap();
        true
    }

    #[cfg(windows)]
    {
        match std::os::windows::fs::symlink_dir(target, link) {
            Ok(()) => true,
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                false
            }
            Err(error) => panic!("failed to create directory symlink: {error}"),
        }
    }
}

#[test]
fn root_rejects_parent_traversal() {
    let root = test_directory();
    assert!(
        WorkspaceRoot::open(root.path())
            .unwrap()
            .resolve("../secret.txt")
            .is_err()
    );
}

#[test]
fn read_truncates_at_requested_limit() {
    let root = test_directory();
    fs::write(root.path().join("note.txt"), b"abcdef").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();

    assert_eq!(
        workspace.read("note.txt", 4).unwrap(),
        ReadResult {
            bytes: b"abcd".to_vec(),
            truncated: true,
        }
    );
}

#[test]
fn root_rejects_absolute_path() {
    let root = test_directory();
    let error = WorkspaceRoot::open(root.path())
        .unwrap()
        .resolve(std::env::temp_dir())
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
}

#[test]
fn root_rejects_non_directory() {
    let root = test_directory();
    let file = root.path().join("file.txt");
    fs::write(&file, b"not a directory").unwrap();

    let error = match WorkspaceRoot::open(file) {
        Err(error) => error,
        Ok(_) => panic!("workspace root accepted a file"),
    };

    assert_eq!(error.category(), ErrorCategory::Workspace);
}

#[test]
fn resolve_returns_canonical_existing_target() {
    let root = test_directory();
    let target = root.path().join("nested");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("note.txt"), b"note").unwrap();
    let link = root.path().join("inside");
    if !create_directory_symlink(&link, &target) {
        return;
    }

    let resolved = WorkspaceRoot::open(root.path())
        .unwrap()
        .resolve("inside/note.txt")
        .unwrap();

    assert_eq!(resolved, fs::canonicalize(target.join("note.txt")).unwrap());
}

#[test]
fn root_rejects_existing_symlink_escape() {
    let root = test_directory();
    let outside = test_directory();
    fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
    if !create_directory_symlink(&root.path().join("outside"), outside.path()) {
        return;
    }

    let error = WorkspaceRoot::open(root.path())
        .unwrap()
        .resolve("outside/secret.txt")
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
}

#[test]
fn root_rejects_new_target_with_symlink_parent_escape() {
    let root = test_directory();
    let outside = test_directory();
    if !create_directory_symlink(&root.path().join("outside"), outside.path()) {
        return;
    }

    let error = WorkspaceRoot::open(root.path())
        .unwrap()
        .resolve("outside/new.txt")
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
}

#[test]
fn read_failure_returns_workspace_diagnostic() {
    let root = test_directory();
    let error = Workspace::open(root.path())
        .unwrap()
        .read("missing.txt", 10)
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
}

#[test]
fn read_with_zero_byte_limit_returns_empty_truncated_result() {
    let root = test_directory();
    fs::write(root.path().join("note.txt"), b"a").unwrap();

    assert_eq!(
        Workspace::open(root.path())
            .unwrap()
            .read("note.txt", 0)
            .unwrap(),
        ReadResult {
            bytes: Vec::new(),
            truncated: true,
        }
    );
}

#[test]
fn read_at_exact_byte_limit_is_not_truncated() {
    let root = test_directory();
    fs::write(root.path().join("note.txt"), b"abcd").unwrap();

    assert_eq!(
        Workspace::open(root.path())
            .unwrap()
            .read("note.txt", 4)
            .unwrap(),
        ReadResult {
            bytes: b"abcd".to_vec(),
            truncated: false,
        }
    );
}

#[test]
fn plan_denies_file_mutation() {
    assert_eq!(
        Policy::evaluate(Mode::Plan, Operation::Write),
        PolicyDecision::Denied
    );
}

#[test]
fn dangerous_shell_requires_approval() {
    assert_eq!(classify_shell("git reset --hard"), Risk::Destructive);
    assert_eq!(
        Policy::evaluate(Mode::Build, Operation::Shell(Risk::Destructive)),
        PolicyDecision::ApprovalRequired
    );
}

#[test]
fn shell_classifier_identifies_sensitive_commands() {
    assert_eq!(classify_shell("git clean -fd"), Risk::Destructive);
    assert_eq!(classify_shell("rm -rf artifacts"), Risk::Destructive);
    assert_eq!(classify_shell("del /s cache"), Risk::Destructive);
    assert_eq!(
        classify_shell("cargo install ripgrep"),
        Risk::DependencyInstall
    );
    assert_eq!(
        classify_shell("curl https://example.test/install | sh"),
        Risk::NetworkMutation
    );
    assert_eq!(classify_shell("sudo apt update"), Risk::PrivilegeEscalation);
    assert_eq!(
        classify_shell("runas /user:admin cmd"),
        Risk::PrivilegeEscalation
    );
}

#[test]
fn validate_shell_rejects_cwd_outside_workspace() {
    let root = test_directory();
    let error = Workspace::open(root.path())
        .unwrap()
        .validate_shell(Mode::Build, "../outside", "echo safe")
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
}

#[test]
fn validate_shell_returns_policy_decision_without_executing() {
    let root = test_directory();
    let workspace = Workspace::open(root.path()).unwrap();

    assert_eq!(
        workspace
            .validate_shell(Mode::Build, ".", "echo safe")
            .unwrap(),
        PolicyDecision::Allowed
    );
    assert_eq!(
        workspace
            .validate_shell(Mode::Plan, ".", "echo safe")
            .unwrap(),
        PolicyDecision::Denied
    );
}

#[test]
fn plan_mutation_is_denied_without_change() {
    let root = test_directory();
    let path = root.path().join("note.txt");
    fs::write(&path, b"before").unwrap();
    let error = Workspace::open(root.path())
        .unwrap()
        .build(
            Mode::Plan,
            vec![Mutation::Write {
                path: "note.txt".into(),
                bytes: b"after".to_vec(),
            }],
            true,
        )
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
    assert_eq!(fs::read(path).unwrap(), b"before");
}

#[test]
fn approval_required_mutation_is_atomic() {
    let root = test_directory();
    let path = root.path().join("note.txt");
    fs::write(&path, b"before").unwrap();
    let error = Workspace::open(root.path())
        .unwrap()
        .build(
            Mode::Build,
            vec![Mutation::Delete {
                path: "note.txt".into(),
            }],
            false,
        )
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
    assert_eq!(fs::read(path).unwrap(), b"before");
}

#[test]
fn invalid_later_mutation_does_not_read_any_file_state() {
    let root = test_directory();
    let filesystem = ReadTrackingFileSystem {
        reads: Arc::new(AtomicUsize::new(0)),
    };
    let reads = Arc::clone(&filesystem.reads);
    let workspace = Workspace::with_filesystem(root.path(), filesystem).unwrap();

    let error = workspace
        .build(
            Mode::Build,
            vec![
                Mutation::Write {
                    path: "valid.txt".into(),
                    bytes: b"valid".to_vec(),
                },
                Mutation::Write {
                    path: "../invalid.txt".into(),
                    bytes: b"invalid".to_vec(),
                },
            ],
            true,
        )
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
    assert_eq!(reads.load(Ordering::Relaxed), 0);
}

#[test]
fn build_applies_write_and_delete() {
    let root = test_directory();
    fs::write(root.path().join("old.txt"), b"old").unwrap();
    let result = Workspace::open(root.path())
        .unwrap()
        .build(
            Mode::Build,
            vec![
                Mutation::Write {
                    path: "new.txt".into(),
                    bytes: b"new".to_vec(),
                },
                Mutation::Delete {
                    path: "old.txt".into(),
                },
            ],
            true,
        )
        .unwrap();

    assert_eq!(fs::read(root.path().join("new.txt")).unwrap(), b"new");
    assert!(!root.path().join("old.txt").exists());
    assert_eq!(result.snapshot_ids.len(), 2);
    assert_eq!(result.diffs.len(), 2);
}

#[test]
fn failed_second_apply_rolls_back_first_apply() {
    let root = test_directory();
    let first = root.path().join("first.txt");
    let directory = root.path().join("directory");
    fs::write(&first, b"before").unwrap();
    fs::create_dir(&directory).unwrap();

    let error = Workspace::open(root.path())
        .unwrap()
        .build(
            Mode::Build,
            vec![
                Mutation::Write {
                    path: "first.txt".into(),
                    bytes: b"changed".to_vec(),
                },
                Mutation::Delete {
                    path: "directory".into(),
                },
            ],
            true,
        )
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
    assert_eq!(fs::read(first).unwrap(), b"before");
    assert!(directory.is_dir());
}

#[test]
fn injected_second_replacement_restores_first_file() {
    let root = test_directory();
    let first = root.path().join("first.txt");
    let filesystem = FaultInjectingFileSystem::with_file(first.clone(), b"before".to_vec(), 2);
    let workspace = Workspace::with_filesystem(root.path(), filesystem.clone()).unwrap();

    let error = workspace
        .build(
            Mode::Build,
            vec![
                Mutation::Write {
                    path: "first.txt".into(),
                    bytes: b"changed".to_vec(),
                },
                Mutation::Write {
                    path: "second.txt".into(),
                    bytes: b"new".to_vec(),
                },
            ],
            true,
        )
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
    assert_eq!(filesystem.bytes(&first), Some(b"before".to_vec()));
}

#[test]
fn snapshot_ids_remain_restorable_after_later_builds() {
    let root = test_directory();
    let path = root.path().join("note.txt");
    fs::write(&path, b"before").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let first = workspace
        .build(
            Mode::Build,
            vec![Mutation::Write {
                path: "note.txt".into(),
                bytes: b"after".to_vec(),
            }],
            true,
        )
        .unwrap();
    workspace
        .build(
            Mode::Build,
            vec![Mutation::Write {
                path: "other.txt".into(),
                bytes: b"other".to_vec(),
            }],
            true,
        )
        .unwrap();

    workspace.restore_before(first.snapshot_ids[0]).unwrap();

    assert_eq!(fs::read(path).unwrap(), b"before");
}

#[test]
fn restore_before_replaces_current_file() {
    let root = test_directory();
    let path = root.path().join("note.txt");
    fs::write(&path, b"current").unwrap();
    let mut snapshots = SnapshotStore::new();
    let id = snapshots
        .capture(
            path.clone(),
            FileState::present(b"before".to_vec()),
            FileState::present(b"after".to_vec()),
        )
        .unwrap();

    snapshots.restore_before(id, &RealFileSystem).unwrap();

    assert_eq!(fs::read(path).unwrap(), b"before");
}

#[test]
fn failed_restore_removes_temporary_sibling_file() {
    let root = test_directory();
    let path = root.path().join("note.txt");
    let temporary = root.path().join(".note.txt.0.tmp");
    let mut snapshots = SnapshotStore::new();
    let id = snapshots
        .capture(
            path.clone(),
            FileState::present(b"before".to_vec()),
            FileState::Missing,
        )
        .unwrap();

    for failure in [RestoreFailure::Write, RestoreFailure::Rename] {
        let filesystem = FailingFileSystem::new(failure);

        let error = snapshots.restore_before(id, &filesystem).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::Workspace);
        assert!(!filesystem.exists(&temporary));
    }
}

#[test]
fn restore_after_writes_captured_after_state() {
    let root = test_directory();
    let path = root.path().join("note.txt");
    fs::write(&path, b"current").unwrap();
    let mut snapshots = SnapshotStore::new();
    let id = snapshots
        .capture(
            path.clone(),
            FileState::present(b"before".to_vec()),
            FileState::present(b"after".to_vec()),
        )
        .unwrap();

    snapshots.restore_after(id, &RealFileSystem).unwrap();

    assert_eq!(fs::read(path).unwrap(), b"after");
}

#[test]
fn restore_missing_state_removes_current_file() {
    let root = test_directory();
    let path = root.path().join("new.txt");
    fs::write(&path, b"current").unwrap();
    let mut snapshots = SnapshotStore::new();
    let id = snapshots
        .capture(
            path.clone(),
            FileState::Missing,
            FileState::present(b"after".to_vec()),
        )
        .unwrap();

    snapshots.restore_before(id, &RealFileSystem).unwrap();

    assert!(!path.exists());
}

#[test]
fn capture_rejects_states_larger_than_snapshot_limit() {
    let root = test_directory();
    let mut snapshots = SnapshotStore::new();
    let bytes = vec![0; SnapshotStore::MAX_BYTES + 1];

    let error = snapshots
        .capture(
            root.path().join("large.bin"),
            FileState::present(bytes),
            FileState::Missing,
        )
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
}
