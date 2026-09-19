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
    assert_eq!(schemas.len(), 12);
    let names: Vec<&str> = schemas
        .iter()
        .filter_map(|s| {
            s.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
        })
        .collect();
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"write_file"));
    assert!(names.contains(&"edit_file"));
    assert!(names.contains(&"list_dir"));
    assert!(names.contains(&"glob_search"));
    assert!(names.contains(&"grep_search"));
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"question"));
    assert!(names.contains(&"update_plan"));
    assert!(names.contains(&"webfetch"));
    assert!(names.contains(&"websearch"));
    assert!(names.contains(&"skill"));
}
#[test]
fn test_execute_tool_update_plan() {
    let (_root, workspace) = workspace("update-plan");

    // 1. Valid call with 3 steps and mixed status (todo, doing, done)
    let payload = serde_json::json!({
        "explanation": "Refactor database module",
        "plan": [
            {"step": "Step 1: Setup migrations", "status": "done"},
            {"step": "Step 2: Implement queries", "status": "doing"},
            {"step": "Step 3: Run regression tests", "status": "todo"}
        ]
    });

    let res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "update_plan",
        &payload.to_string(),
    );
    assert!(res.is_ok(), "update_plan failed: {:?}", res);
    assert_eq!(
        res.unwrap(),
        "Plan updated: 3 steps (1 completed, 1 in progress, 1 pending)"
    );

    // 2. Also allowed in Build mode
    let res_build = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "update_plan",
        &payload.to_string(),
    );
    assert!(res_build.is_ok());
    assert_eq!(
        res_build.unwrap(),
        "Plan updated: 3 steps (1 completed, 1 in progress, 1 pending)"
    );

    // 3. Alternative status representations: completed, in_progress, pending
    let payload_canonical = serde_json::json!({
        "plan": [
            {"step": "Task A", "status": "completed"},
            {"step": "Task B", "status": "in_progress"},
            {"step": "Task C", "status": "pending"}
        ]
    });
    let res_canonical = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "update_plan",
        &payload_canonical.to_string(),
    );
    assert!(res_canonical.is_ok());
    assert_eq!(
        res_canonical.unwrap(),
        "Plan updated: 3 steps (1 completed, 1 in progress, 1 pending)"
    );

    // 4. Validation: missing plan
    let res_missing_plan = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "update_plan",
        r#"{"explanation": "no plan"}"#,
    );
    assert!(res_missing_plan.is_err());
    assert!(
        res_missing_plan
            .unwrap_err()
            .contains("Missing required argument 'plan'")
    );

    // 5. Validation: empty plan array
    let res_empty_plan = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "update_plan",
        r#"{"plan": []}"#,
    );
    assert!(res_empty_plan.is_err());
    assert!(res_empty_plan.unwrap_err().contains("at least one step"));

    // 6. Validation: invalid status
    let res_invalid_status = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "update_plan",
        r#"{"plan": [{"step": "Task", "status": "unknown_status"}]}"#,
    );
    assert!(res_invalid_status.is_err());
    assert!(res_invalid_status.unwrap_err().contains("Allowed status"));
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
#[test]
fn test_execute_tool_bash_requires_approval_for_risky_commands() {
    let (root, workspace) = workspace("bash-approval");
    let marker = root.join("must-survive.txt");
    fs::write(&marker, "preserve").unwrap();

    let res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "bash",
        r#"{"command": "rm -rf must-survive.txt"}"#,
    );

    assert!(res.is_err(), "risky bash command must require approval");
    assert!(
        res.unwrap_err().to_lowercase().contains("approval"),
        "error must explain approval requirement"
    );
    assert!(marker.exists(), "blocked command must not mutate workspace");
}
#[test]
fn test_execute_tool_bash_stdin_null() {
    let (_root, workspace) = workspace("bash-stdin");
    let cmd = if cfg!(windows) {
        r#"{"command": "[Console]::In.ReadToEnd()"}"#
    } else {
        r#"{"command": "cat"}"#
    };
    let res = clawcode::conversation::tools::execute_tool(&workspace, Mode::Build, "bash", cmd);
    assert!(
        res.is_ok(),
        "command reading stdin should finish immediately: {:?}",
        res
    );
}

#[test]
fn test_read_file_inline_selectors_and_formatting() {
    let (root, workspace) = workspace("read-selectors");

    let file_content = "line 1\nline 2\nline 3\nline 4\nline 5\n";
    fs::write(root.join("data.txt"), file_content).unwrap();

    // 1. Membaca dengan range path:2-4
    let res_range = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "read_file",
        r#"{"path": "data.txt:2-4"}"#,
    )
    .unwrap();
    assert!(res_range.contains("[data.txt#"));
    assert!(res_range.contains("(lines 2..4 of 5)"));
    assert!(res_range.contains("  2: line 2"));
    assert!(res_range.contains("  3: line 3"));
    assert!(res_range.contains("  4: line 4"));
    assert!(!res_range.contains("line 1"));
    assert!(!res_range.contains("line 5"));

    // 2. Membaca dengan path:3+2
    let res_plus = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "read_file",
        r#"{"path": "data.txt:3+2"}"#,
    )
    .unwrap();
    assert!(res_plus.contains("(lines 3..4 of 5)"));
    assert!(res_plus.contains("  3: line 3"));
    assert!(res_plus.contains("  4: line 4"));
    assert!(!res_plus.contains("line 1"));
    assert!(!res_plus.contains("line 2"));
    assert!(!res_plus.contains("line 5"));

    // 3. Membaca dengan path:-2 (last 2 lines)
    let res_last = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "read_file",
        r#"{"path": "data.txt:-2"}"#,
    )
    .unwrap();
    assert!(res_last.contains("(lines 4..5 of 5)"));
    assert!(res_last.contains("  4: line 4"));
    assert!(res_last.contains("  5: line 5"));
    assert!(!res_last.contains("line 1"));
    assert!(!res_last.contains("line 2"));
    assert!(!res_last.contains("line 3"));

    // 4. Membaca dengan path:raw
    let res_raw = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "read_file",
        r#"{"path": "data.txt:raw"}"#,
    )
    .unwrap();
    assert_eq!(res_raw, "line 1\nline 2\nline 3\nline 4\nline 5\n");
    assert!(!res_raw.contains('['));
    assert!(!res_raw.contains(':'));

    // 5. Membaca file dengan path biasa + offset/limit
    let res_offset_limit = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "read_file",
        r#"{"path": "data.txt", "offset": 2, "limit": 2}"#,
    )
    .unwrap();
    assert!(res_offset_limit.contains("(lines 2..3 of 5)"));
    assert!(res_offset_limit.contains("  2: line 2"));
    assert!(res_offset_limit.contains("  3: line 3"));
    assert!(!res_offset_limit.contains("line 1"));
    assert!(!res_offset_limit.contains("line 4"));
    assert!(!res_offset_limit.contains("line 5"));

    // 6. Windows drive letter selector test (C:\test\file.rs:10-20)
    let parsed_win = clawcode::conversation::tools::parse_read_path(r"C:\test\file.rs:10-20");
    assert_eq!(parsed_win.path, r"C:\test\file.rs");
    assert_eq!(
        parsed_win.selector,
        Some(clawcode::conversation::tools::LineSelector::Range(10, 20))
    );
    assert!(!parsed_win.is_raw);

    let abs_file = root.join("data.txt");
    let abs_str = abs_file.to_str().unwrap();
    if abs_str.len() >= 2 && abs_str.as_bytes()[1] == b':' {
        let path_arg = format!("{abs_str}:1-2");
        let arg_json = serde_json::json!({ "path": path_arg }).to_string();
        let res_win_exec = clawcode::conversation::tools::execute_tool(
            &workspace,
            Mode::Plan,
            "read_file",
            &arg_json,
        );
        assert!(
            res_win_exec.is_ok(),
            "Windows drive letter exec failed: {:?}",
            res_win_exec
        );
        let out = res_win_exec.unwrap();
        assert!(out.contains("(lines 1..2 of 5)"));
        assert!(out.contains("  1: line 1"));
        assert!(out.contains("  2: line 2"));
    }

    // 7. Single letter filenames with selectors
    let parsed_single = clawcode::conversation::tools::parse_read_path("a:10");
    assert_eq!(parsed_single.path, "a");
    assert_eq!(
        parsed_single.selector,
        Some(clawcode::conversation::tools::LineSelector::From(10))
    );

    // 8. Verbatim Windows path
    let parsed_verbatim =
        clawcode::conversation::tools::parse_read_path(r"\\?\C:\test\file.rs:10-20");
    assert_eq!(parsed_verbatim.path, r"C:\test\file.rs");
    assert_eq!(
        parsed_verbatim.selector,
        Some(clawcode::conversation::tools::LineSelector::Range(10, 20))
    );

    // 9. Path separator before colon
    let parsed_col_dir = clawcode::conversation::tools::parse_read_path("dir:subdir/file.txt:5");
    assert_eq!(parsed_col_dir.path, "dir:subdir/file.txt");
    assert_eq!(
        parsed_col_dir.selector,
        Some(clawcode::conversation::tools::LineSelector::From(5))
    );
}

#[test]
fn test_webfetch_parameter_validation() {
    let (_root, workspace) = workspace("webfetch-val");

    // 1. Missing url
    let res_missing =
        clawcode::conversation::tools::execute_tool(&workspace, Mode::Plan, "webfetch", r#"{}"#);
    assert!(res_missing.is_err());
    assert!(
        res_missing
            .unwrap_err()
            .contains("Missing required argument 'url'")
    );

    // 2. Invalid scheme
    let res_ftp = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "webfetch",
        r#"{"url": "ftp://files.example.com/data.txt"}"#,
    );
    assert!(res_ftp.is_err());
    assert!(
        res_ftp
            .unwrap_err()
            .contains("URL must begin with 'http://' or 'https://'")
    );

    let res_relative = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "webfetch",
        r#"{"url": "example.com/test"}"#,
    );
    assert!(res_relative.is_err());
    assert!(
        res_relative
            .unwrap_err()
            .contains("URL must begin with 'http://' or 'https://'")
    );
}

#[test]
fn test_websearch_parameter_validation() {
    let (_root, workspace) = workspace("websearch-val");

    // 1. Missing query
    let res_missing =
        clawcode::conversation::tools::execute_tool(&workspace, Mode::Plan, "websearch", r#"{}"#);
    assert!(res_missing.is_err());
    assert!(
        res_missing
            .unwrap_err()
            .contains("Missing required argument 'query'")
    );

    // 2. Empty query
    let res_empty = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "websearch",
        r#"{"query": ""}"#,
    );
    assert!(res_empty.is_err());
    assert!(
        res_empty
            .unwrap_err()
            .contains("Search query cannot be empty")
    );

    // 3. Whitespace query
    let res_spaces = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "websearch",
        r#"{"query": "   "}"#,
    );
    assert!(res_spaces.is_err());
    assert!(
        res_spaces
            .unwrap_err()
            .contains("Search query cannot be empty")
    );
}

#[test]
fn test_skill_tool_execution_and_listing() {
    let (root, workspace) = workspace("skill-tool");

    // 1. Missing name parameter
    let res_missing =
        clawcode::conversation::tools::execute_tool(&workspace, Mode::Plan, "skill", r#"{}"#);
    assert!(res_missing.is_err());
    assert!(
        res_missing
            .unwrap_err()
            .contains("Missing required argument 'name'")
    );

    // 2. Empty name parameter
    let res_empty = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "skill",
        r#"{"name": "   "}"#,
    );
    assert!(res_empty.is_err());
    assert!(
        res_empty
            .unwrap_err()
            .contains("Missing required argument 'name'")
    );

    // 3. Create workspace skills
    let skills_dir = root.join("skills");
    fs::create_dir_all(skills_dir.join("best-practices")).unwrap();
    fs::write(
        skills_dir.join("best-practices").join("SKILL.md"),
        "# Best Practices\nWrite modular Rust code with rigorous testing.",
    )
    .unwrap();

    fs::write(
        skills_dir.join("testing.md"),
        "# Testing Skill\nDeterministic tests first.",
    )
    .unwrap();

    // 4. Load skill from directory (./skills/best-practices/SKILL.md)
    let res_dir_skill = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "skill",
        r#"{"name": "best-practices"}"#,
    )
    .unwrap();
    assert!(res_dir_skill.contains("<skill_content name=\"best-practices\">"));
    assert!(res_dir_skill.contains("Write modular Rust code with rigorous testing."));
    assert!(res_dir_skill.contains("</skill_content>"));

    // 5. Load skill from markdown file (./skills/testing.md)
    let res_file_skill = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "skill",
        r#"{"name": "testing"}"#,
    )
    .unwrap();
    assert!(res_file_skill.contains("<skill_content name=\"testing\">"));
    assert!(res_file_skill.contains("Deterministic tests first."));
    assert!(res_file_skill.contains("</skill_content>"));

    // 6. Non-existent skill returns available skills list
    let res_not_found = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "skill",
        r#"{"name": "non-existent-skill"}"#,
    );
    assert!(res_not_found.is_err());
    let err_msg = res_not_found.unwrap_err();
    assert!(err_msg.contains("Skill \"non-existent-skill\" not found. Available skills:"));
    assert!(err_msg.contains("best-practices"));
    assert!(err_msg.contains("testing"));
}

#[test]
fn test_webfetch_mock_http_and_offline() {
    let (_root, workspace) = workspace("webfetch-mock");

    // 1. Mock HTTP server using std::net::TcpListener
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_thread = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            use std::io::{Read, Write};
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let html = r#"
                <!DOCTYPE html>
                <html>
                <head>
                    <title>Mock Doc</title>
                    <style>h1 { color: red; }</style>
                    <script>console.log("drop me");</script>
                </head>
                <body>
                    <h1>Rust Documentation</h1>
                    <p>Welcome to &quot;safe&quot; &amp; fast systems programming.</p>
                    <ul>
                        <li>Memory safety without GC</li>
                        <li>Zero-cost abstractions</li>
                    </ul>
                </body>
                </html>
            "#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let fetch_url = format!("http://127.0.0.1:{port}/docs");
    let args = serde_json::json!({ "url": fetch_url }).to_string();
    let res =
        clawcode::conversation::tools::execute_tool(&workspace, Mode::Plan, "webfetch", &args)
            .unwrap();

    server_thread.join().unwrap();

    assert!(!res.contains("console.log"));
    assert!(!res.contains("color: red"));
    assert!(res.contains("Rust Documentation"));
    assert!(res.contains("Welcome to \"safe\" & fast systems programming."));
    assert!(res.contains("- Memory safety without GC"));
    assert!(res.contains("- Zero-cost abstractions"));

    // 2. Offline / unreachable connection error
    let offline_args = serde_json::json!({ "url": "http://127.0.0.1:1/offline" }).to_string();
    let offline_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "webfetch",
        &offline_args,
    );
    assert!(offline_res.is_err());
    let err_msg = offline_res.unwrap_err();
    assert!(err_msg.contains("Webfetch failed for 'http://127.0.0.1:1/offline'"));
}

#[test]
fn test_websearch_mock_http_and_offline() {
    let (_root, workspace) = workspace("websearch-mock");

    // 1. Mock HTTP server serving DuckDuckGo HTML search results
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_thread = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            use std::io::{Read, Write};
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let html = r##"
                <div class="result results_links results_links_deep web-result">
                  <div class="links_main links_deep result__body">
                    <h2 class="result__title">
                      <a class="result__url" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fcrates.io%2Fcrates%2Fureq&rut=1">ureq - crates.io</a>
                    </h2>
                    <a class="result__snippet" href="#">A simple, safe HTTP client. Minimal dependencies.</a>
                  </div>
                </div>
            "##;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(),
                html
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    // Point DDG search URL to mock server
    let mock_ddg_url = format!("http://127.0.0.1:{port}/html/?q=");
    unsafe {
        std::env::set_var("CLAWCODE_DDG_SEARCH_URL", &mock_ddg_url);
    }

    let res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "websearch",
        r#"{"query": "rust ureq"}"#,
    )
    .unwrap();

    server_thread.join().unwrap();

    assert!(
        res.contains("Search results for \"rust ureq\""),
        "actual res was: {res}"
    );
    assert!(res.contains("ureq - crates.io"));
    assert!(res.contains("https://crates.io/crates/ureq"));
    assert!(res.contains("A simple, safe HTTP client. Minimal dependencies."));

    // 2. Offline / unreachable fallback
    unsafe {
        std::env::set_var("CLAWCODE_DDG_SEARCH_URL", "http://127.0.0.1:1/html/?q=");
        std::env::set_var("CLAWCODE_DDG_API_URL", "http://127.0.0.1:1/?q=");
    }

    let offline_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "websearch",
        r#"{"query": "offline query"}"#,
    );
    assert!(offline_res.is_err());
    let err_str = offline_res.unwrap_err();
    assert!(err_str.contains("Web search failed: network unreachable"));

    unsafe {
        std::env::remove_var("CLAWCODE_DDG_SEARCH_URL");
        std::env::remove_var("CLAWCODE_DDG_API_URL");
    }
}

#[test]
fn test_tui_app_web_and_skill_verbs_and_formatting() {
    use clawcode::tui::app::{format_tool_success_detail, tool_target_and_verbs};

    // 1. tool_target_and_verbs
    let fetch_args = serde_json::json!({ "url": "https://example.com/docs" });
    let (verb, active_verb, target) = tool_target_and_verbs("webfetch", Some(&fetch_args));
    assert_eq!(verb, "Fetched");
    assert_eq!(active_verb, "Fetching");
    assert_eq!(target, "https://example.com/docs");

    let search_args = serde_json::json!({ "query": "rust ownership" });
    let (verb, active_verb, target) = tool_target_and_verbs("websearch", Some(&search_args));
    assert_eq!(verb, "Searched");
    assert_eq!(active_verb, "Searching");
    assert_eq!(target, "rust ownership");

    let skill_args = serde_json::json!({ "name": "best-practices" });
    let (verb, active_verb, target) = tool_target_and_verbs("skill", Some(&skill_args));
    assert_eq!(verb, "Loaded skill");
    assert_eq!(active_verb, "Loading skill");
    assert_eq!(target, "best-practices");

    // 2. format_tool_success_detail
    let fetch_detail = format_tool_success_detail("webfetch", "line 1\nline 2\nline 3\n");
    assert_eq!(fetch_detail, "3 lines");

    let fetch_single = format_tool_success_detail("webfetch", "single line");
    assert_eq!(fetch_single, "1 line");

    let search_output = "Search results for \"rust\":\n\n1. Result One\n   URL: https://one\n2. Result Two\n   URL: https://two\n";
    let search_detail = format_tool_success_detail("websearch", search_output);
    assert_eq!(search_detail, "2 results");

    let search_none =
        format_tool_success_detail("websearch", "No search results found for query \"foo\".");
    assert_eq!(search_none, "0 results");

    let skill_detail =
        format_tool_success_detail("skill", "<skill_content name=\"test\">...</skill_content>");
    assert_eq!(skill_detail, "skill loaded successfully");
}

#[test]
fn test_edit_file_crlf_mixed_and_normalization() {
    let (root, workspace) = workspace("edit-crlf-norm");
    let file_path = root.join("crlf.txt");
    fs::write(&file_path, "header\r\nline one\r\nline two\r\nfooter\r\n").unwrap();

    // 1. Edit with mixed CRLF and LF in old_string, and LF in new_string
    let edit_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Build,
        "edit_file",
        &serde_json::json!({
            "path": "crlf.txt",
            "old_string": "line one\r\nline two\n",
            "new_string": "alpha\nbeta\n"
        })
        .to_string(),
    );
    assert!(edit_res.is_ok(), "Edit failed: {:?}", edit_res);

    let updated_bytes = fs::read(&file_path).unwrap();
    let updated_str = String::from_utf8(updated_bytes).unwrap();
    assert_eq!(updated_str, "header\r\nalpha\r\nbeta\r\nfooter\r\n");
}

#[test]
fn test_walk_dir_recursion_limit() {
    let (root, workspace) = workspace("recursion-limit");
    let mut current = root.clone();
    for i in 0..40 {
        current = current.join(format!("d{i}"));
    }
    fs::create_dir_all(&current).unwrap();
    fs::write(current.join("deep.txt"), "target content").unwrap();

    let glob_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "glob_search",
        r#"{"pattern": "*.txt"}"#,
    );
    assert!(glob_res.is_ok());

    let grep_res = clawcode::conversation::tools::execute_tool(
        &workspace,
        Mode::Plan,
        "grep_search",
        r#"{"query": "target"}"#,
    );
    assert!(grep_res.is_ok());
}
