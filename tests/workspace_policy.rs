use clawcode::workspace::{ReadResult, Workspace, WorkspaceRoot};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn test_directory() -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("clawcode-workspace-{unique}"));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn root_rejects_parent_traversal() {
    let root = test_directory();
    assert!(
        WorkspaceRoot::open(&root)
            .unwrap()
            .resolve("../secret.txt")
            .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn read_truncates_at_requested_limit() {
    let root = test_directory();
    fs::write(root.join("note.txt"), b"abcdef").unwrap();
    let workspace = Workspace::open(&root).unwrap();

    assert_eq!(
        workspace.read("note.txt", 4).unwrap(),
        ReadResult {
            bytes: b"abcd".to_vec(),
            truncated: true,
        }
    );

    fs::remove_dir_all(root).unwrap();
}
