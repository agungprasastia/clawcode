use clawcode::core::error::ErrorCategory;
use clawcode::workspace::{
    FileState, Mode, Operation, Policy, PolicyDecision, ReadResult, RealFileSystem, Risk,
    SnapshotStore, Workspace, WorkspaceRoot, classify_shell,
};
use std::fs;
use std::path::{Path, PathBuf};
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
