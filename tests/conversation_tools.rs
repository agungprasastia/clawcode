use clawcode::conversation::tools::{ToolLifecycle, ToolRequest, ToolStatus};
use clawcode::workspace::{Mode, Mutation, RealFileSystem, Workspace};
use std::fs;

fn workspace(name: &str) -> (std::path::PathBuf, Workspace<RealFileSystem>) {
    let root =
        std::env::temp_dir().join(format!("clawcode-tools-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let workspace = Workspace::with_filesystem(&root, RealFileSystem).unwrap();
    (root, workspace)
}

#[test]
fn build_diff_precedes_approval_and_rejection_preserves_workspace() {
    let (root, workspace) = workspace("rejection");
    fs::write(root.join("file.txt"), "before").unwrap();
    let mut tools = ToolLifecycle::new(&workspace, Mode::Build);
    tools.request(ToolRequest::Build {
        mutations: vec![Mutation::Write {
            path: "file.txt".into(),
            bytes: b"after".to_vec(),
        }],
    });
    assert_eq!(tools.review().status, ToolStatus::AwaitingApproval);
    assert!(tools.diff().is_some());
    assert_eq!(tools.approve(false).status, ToolStatus::Completed);
    assert_eq!(fs::read_to_string(root.join("file.txt")).unwrap(), "before");
}

#[test]
fn plan_read_only_and_cancellation_are_isolated() {
    let (root, workspace) = workspace("plan");
    fs::write(root.join("file.txt"), "safe").unwrap();
    let mut plan = ToolLifecycle::new(&workspace, Mode::Plan);
    assert_eq!(
        plan.request(ToolRequest::Read {
            path: "file.txt".into(),
            max_bytes: 10
        })
        .status,
        ToolStatus::Completed
    );
    plan.request(ToolRequest::Build {
        mutations: vec![Mutation::Write {
            path: "file.txt".into(),
            bytes: b"bad".to_vec(),
        }],
    });
    assert!(matches!(plan.review().status, ToolStatus::Failed(_)));
    plan.cancel();
    assert_eq!(
        plan.request(ToolRequest::Read {
            path: "file.txt".into(),
            max_bytes: 10
        })
        .status,
        ToolStatus::Cancelled
    );
}

#[test]
fn allowed_write_applies_after_exposing_diff_without_approval() {
    let (root, workspace) = workspace("allowed-write");
    let mut tools = ToolLifecycle::new(&workspace, Mode::Build);
    assert_eq!(
        tools
            .request(ToolRequest::Build {
                mutations: vec![Mutation::Write {
                    path: "new.txt".into(),
                    bytes: b"ok".to_vec()
                }],
            })
            .status,
        ToolStatus::Requested
    );
    let reviewed = tools.review();
    assert_eq!(reviewed.status, ToolStatus::Completed);
    assert!(reviewed.transaction.is_some());
    assert_eq!(fs::read(root.join("new.txt")).unwrap(), b"ok");
}

#[test]
fn failed_review_and_apply_clear_pending_diff_and_preview() {
    let (_root, workspace) = workspace("cleanup");
    let mut tools = ToolLifecycle::new(&workspace, Mode::Build);
    tools.request(ToolRequest::Build {
        mutations: vec![Mutation::Write {
            path: "new.txt".into(),
            bytes: b"ok".to_vec(),
        }],
    });
    assert_eq!(tools.review().status, ToolStatus::Completed);
    assert!(tools.diff().is_none());
    assert!(matches!(tools.approve(true).status, ToolStatus::Failed(_)));
    assert!(tools.diff().is_none());
}

#[test]
fn tool_layer_enforces_read_and_mutation_bounds() {
    let (_root, workspace) = workspace("bounds");
    let mut tools = ToolLifecycle::new(&workspace, Mode::Build);
    assert!(matches!(
        tools
            .request(ToolRequest::Read {
                path: "missing".into(),
                max_bytes: usize::MAX
            })
            .status,
        ToolStatus::Failed(_)
    ));
    assert!(matches!(
        tools
            .request(ToolRequest::Build {
                mutations: vec![Mutation::Write {
                    path: "big".into(),
                    bytes: vec![0; 1024 * 1024 + 1]
                }]
            })
            .status,
        ToolStatus::Failed(_)
    ));
}
