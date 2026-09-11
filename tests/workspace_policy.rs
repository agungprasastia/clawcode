use clawcode::core::error::ErrorCategory;
use clawcode::workspace::{ReadResult, Workspace, WorkspaceRoot};
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
