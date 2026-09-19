use clawcode::core::error::ErrorCategory;
use clawcode::workspace::{Mode, Mutation, Workspace, WorkspaceRoot};
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
    let path = std::env::temp_dir().join(format!("clawcode-test-workspace-{unique}-{sequence}"));
    fs::create_dir(&path).unwrap();
    TestDirectory(path)
}

fn create_directory_symlink(link: &Path, target: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
    }
}

#[test]
fn resolve_nested_nonexistent_path_succeeds_and_stays_within_root() {
    let dir = test_directory();
    let root = WorkspaceRoot::open(dir.path()).unwrap();

    let resolved = root.resolve("nested/sub/file.txt").unwrap();
    assert!(resolved.starts_with(root.canonical_path()));
    assert_eq!(
        resolved,
        root.canonical_path()
            .join("nested")
            .join("sub")
            .join("file.txt")
    );
    assert!(!resolved.exists());
}

#[test]
fn write_to_nested_nonexistent_path_creates_directories_and_file() {
    let dir = test_directory();
    let workspace = Workspace::open(dir.path()).unwrap();

    let relative_path = "nested/sub/file.txt";
    let file_bytes = b"hello nested workspace file";

    let result = workspace.build(
        Mode::Build,
        vec![Mutation::Write {
            path: relative_path.into(),
            bytes: file_bytes.to_vec(),
        }],
        true,
    );
    assert!(result.is_ok(), "build failed: {:?}", result.err());

    let read_result = workspace.read(relative_path, 100).unwrap();
    assert_eq!(read_result.bytes, file_bytes);

    let resolved_on_disk = dir.path().join("nested").join("sub").join("file.txt");
    assert!(resolved_on_disk.exists());
    assert_eq!(fs::read(&resolved_on_disk).unwrap(), file_bytes);
}

#[test]
fn undo_and_redo_nested_file_write() {
    let dir = test_directory();
    let workspace = Workspace::open(dir.path()).unwrap();

    let relative_path = "deep/dir/structure/test.txt";
    let file_bytes = b"content to undo";

    workspace
        .build(
            Mode::Build,
            vec![Mutation::Write {
                path: relative_path.into(),
                bytes: file_bytes.to_vec(),
            }],
            true,
        )
        .unwrap();

    assert_eq!(
        workspace.read(relative_path, 100).unwrap().bytes,
        file_bytes
    );

    workspace.undo().unwrap();
    assert!(workspace.read(relative_path, 100).is_err());

    workspace.redo().unwrap();
    assert_eq!(
        workspace.read(relative_path, 100).unwrap().bytes,
        file_bytes
    );
}

#[test]
fn path_traversal_attempts_are_strictly_rejected() {
    let dir = test_directory();
    let root = WorkspaceRoot::open(dir.path()).unwrap();

    let traversal_paths = [
        "../../outside.txt",
        "../outside.txt",
        "nested/../../outside.txt",
        "nested/sub/../../../outside.txt",
        "/absolute/path/file.txt",
    ];

    for path in traversal_paths {
        let error = root.resolve(path).unwrap_err();
        assert_eq!(
            error.category(),
            ErrorCategory::Workspace,
            "expected workspace error for path: {path}"
        );
    }

    #[cfg(windows)]
    {
        let windows_absolute_paths = [
            r"C:\outside.txt",
            r"C:/outside.txt",
            r"\\server\share\file.txt",
        ];
        for path in windows_absolute_paths {
            let error = root.resolve(path).unwrap_err();
            assert_eq!(
                error.category(),
                ErrorCategory::Workspace,
                "expected workspace error for path: {path}"
            );
        }
    }
}

#[test]
fn write_mutation_rejects_path_traversal() {
    let dir = test_directory();
    let workspace = Workspace::open(dir.path()).unwrap();

    let error = workspace
        .build(
            Mode::Build,
            vec![Mutation::Write {
                path: "../../outside.txt".into(),
                bytes: b"danger".to_vec(),
            }],
            true,
        )
        .unwrap_err();

    assert_eq!(error.category(), ErrorCategory::Workspace);
}

#[test]
fn symlink_ancestor_escaping_root_is_strictly_rejected() {
    let root = test_directory();
    let outside = test_directory();

    let link_path = root.path().join("symlink_outside");
    if !create_directory_symlink(&link_path, outside.path()) {
        return;
    }

    let workspace_root = WorkspaceRoot::open(root.path()).unwrap();

    // Resolving non-existent nested path through symlink pointing outside must be rejected
    let error = workspace_root
        .resolve("symlink_outside/nested/new_file.txt")
        .unwrap_err();
    assert_eq!(error.category(), ErrorCategory::Workspace);

    // Workspace build write through symlink pointing outside must be rejected
    let workspace = Workspace::open(root.path()).unwrap();
    let error = workspace
        .build(
            Mode::Build,
            vec![Mutation::Write {
                path: "symlink_outside/nested/new_file.txt".into(),
                bytes: b"escaped".to_vec(),
            }],
            true,
        )
        .unwrap_err();
    assert_eq!(error.category(), ErrorCategory::Workspace);
}
