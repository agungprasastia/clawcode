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
fn approval_requires_review_of_current_pending_build() {
    let (root, workspace) = workspace("approval-gate");
    fs::write(root.join("file.txt"), "before").unwrap();
    let mut tools = ToolLifecycle::new(&workspace, Mode::Build);
    tools.request(ToolRequest::Build {
        mutations: vec![Mutation::Write {
            path: "file.txt".into(),
            bytes: b"after".to_vec(),
        }],
    });

    assert!(matches!(tools.approve(true).status, ToolStatus::Failed(_)));
    assert_eq!(fs::read_to_string(root.join("file.txt")).unwrap(), "before");
}

#[test]
fn replacing_pending_build_clears_previous_review() {
    let (root, workspace) = workspace("replacement");
    fs::write(root.join("file.txt"), "before").unwrap();
    let mut tools = ToolLifecycle::new(&workspace, Mode::Build);
    tools.request(ToolRequest::Build {
        mutations: vec![Mutation::Write {
            path: "file.txt".into(),
            bytes: b"first".to_vec(),
        }],
    });
    assert_eq!(tools.review().status, ToolStatus::AwaitingApproval);

    tools.request(ToolRequest::Build {
        mutations: vec![Mutation::Write {
            path: "file.txt".into(),
            bytes: b"second".to_vec(),
        }],
    });
    assert!(matches!(tools.approve(true).status, ToolStatus::Failed(_)));
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

#[test]
fn test_coding_tools_schemas_validity() {
    let schemas = clawcode::conversation::tools::coding_tools_schemas();
    assert_eq!(schemas.len(), 8);
    let names: Vec<&str> = schemas
        .iter()
        .filter_map(|s| s.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()))
        .collect();
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"write_file"));
    assert!(names.contains(&"edit_file"));
    assert!(names.contains(&"list_dir"));
    assert!(names.contains(&"glob_search"));
    assert!(names.contains(&"grep_search"));
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"question"));
}

#[test]
fn test_execute_tool_file_lifecycle() {
    let (root, workspace) = workspace("tool-exec-lifecycle");

    // Write file in Build mode
    let write_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "write_file",
        r#"{"path": "hello.txt", "content": "Hello World\nLine 2"}"#,
    );
    assert!(write_res.is_ok(), "write_file failed: {:?}", write_res);

    // Read file back
    let read_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "read_file",
        r#"{"path": "hello.txt"}"#,
    );
    assert!(read_res.is_ok());
    assert!(read_res.unwrap().contains("Hello World"));

    // Edit file
    let edit_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "edit_file",
        r#"{"path": "hello.txt", "old_string": "Hello World", "new_string": "Hello Rust"}"#,
    );
    assert!(edit_res.is_ok(), "edit_file failed: {:?}", edit_res);

    // Verify disk content
    let content = fs::read_to_string(root.join("hello.txt")).unwrap();
    assert!(content.contains("Hello Rust"));

    // List dir
    let list_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "list_dir",
        r#"{"path": "."}"#,
    );
    assert!(list_res.is_ok());
    assert!(list_res.unwrap().contains("hello.txt"));

    // Grep search
    let grep_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "grep_search",
        r#"{"query": "Hello Rust"}"#,
    );
    assert!(grep_res.is_ok());
    assert!(grep_res.unwrap().contains("hello.txt"));

    // Glob search
    let glob_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "glob_search",
        r#"{"pattern": "*.txt"}"#,
    );
    assert!(glob_res.is_ok());
    assert!(glob_res.unwrap().contains("hello.txt"));
}

#[test]
fn test_execute_tool_plan_mode_blocks_writes() {
    let (_root, workspace) = workspace("plan-blocks");

    let write_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "write_file",
        r#"{"path": "bad.txt", "content": "blocked"}"#,
    );
    assert!(write_res.is_err());
    assert!(write_res.unwrap_err().to_lowercase().contains("plan mode"));

    let edit_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "edit_file",
        r#"{"path": "bad.txt", "old_string": "a", "new_string": "b"}"#,
    );
    assert!(edit_res.is_err());
    assert!(edit_res.unwrap_err().to_lowercase().contains("plan mode"));
}

#[test]
fn test_execute_tool_bash() {
    let (_root, workspace) = workspace("bash-exec");
    let res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "bash",
        r#"{"command": "echo clawcode_agent_test"}"#,
    );
    assert!(res.is_ok(), "bash execution failed: {:?}", res);
    assert!(res.unwrap().contains("clawcode_agent_test"));
}
