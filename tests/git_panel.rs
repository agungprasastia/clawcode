use std::path::PathBuf;
use std::process::Command;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use clawcode::platform::git::{
    GitFileChange, commit, get_diff, get_status, stage_file, unstage_file,
};
use clawcode::tui::Theme;
use clawcode::tui::dialogs::git::{GitDialogState, render_git_dialog};

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new() -> Self {
        let unique = format!(
            "clawcode_git_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap();

        let init = Command::new("git")
            .arg("-C")
            .arg(&path)
            .arg("init")
            .output()
            .unwrap();
        assert!(init.status.success());

        let _ = Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["config", "user.name", "ClawcodeTest"])
            .output();
        let _ = Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["config", "user.email", "test@clawcode.dev"])
            .output();

        Self { path }
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn git_get_status_clean_repo() {
    let repo = TempRepo::new();
    let status = get_status(&repo.path).expect("status in clean repo");
    assert!(status.is_empty());
}

#[test]
fn git_stage_and_unstage_file() {
    let repo = TempRepo::new();
    let file_path = repo.path.join("file1.txt");
    std::fs::write(&file_path, "initial content\n").unwrap();

    // Verify untracked
    let status = get_status(&repo.path).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].path, "file1.txt");
    assert_eq!(status[0].status_code, "?");
    assert!(!status[0].staged);

    // Stage file
    stage_file(&repo.path, "file1.txt").expect("stage file");
    let status = get_status(&repo.path).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].path, "file1.txt");
    assert_eq!(status[0].status_code, "A");
    assert!(status[0].staged);

    // Initial commit so HEAD exists
    let commit_out = commit(&repo.path, "feat: initial commit").expect("commit");
    assert!(!commit_out.is_empty());

    let status = get_status(&repo.path).unwrap();
    assert!(status.is_empty());

    // Modify file
    std::fs::write(&file_path, "modified content\n").unwrap();
    let status = get_status(&repo.path).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].path, "file1.txt");
    assert_eq!(status[0].status_code, "M");
    assert!(!status[0].staged);

    // Stage modification
    stage_file(&repo.path, "file1.txt").expect("stage modification");
    let status = get_status(&repo.path).unwrap();
    assert_eq!(status.len(), 1);
    assert!(status[0].staged);

    // Verify diff
    let diff = get_diff(&repo.path, "file1.txt", true).expect("cached diff");
    assert!(diff.contains("+modified content"));
    assert!(diff.contains("-initial content"));

    // Unstage modification
    unstage_file(&repo.path, "file1.txt").expect("unstage file");
    let status = get_status(&repo.path).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].path, "file1.txt");
    assert!(!status[0].staged);
}

#[test]
fn git_unstage_in_empty_repo() {
    let repo = TempRepo::new();
    let file_path = repo.path.join("empty_repo_file.txt");
    std::fs::write(&file_path, "brand new file\n").unwrap();

    stage_file(&repo.path, "empty_repo_file.txt").unwrap();
    let status = get_status(&repo.path).unwrap();
    assert!(
        status
            .iter()
            .any(|c| c.staged && c.path == "empty_repo_file.txt")
    );

    unstage_file(&repo.path, "empty_repo_file.txt").unwrap();
    let status = get_status(&repo.path).unwrap();
    assert!(
        status
            .iter()
            .any(|c| !c.staged && c.path == "empty_repo_file.txt")
    );
}

#[test]
fn git_dialog_state_transitions() {
    let changes = vec![
        GitFileChange {
            path: "staged.rs".to_string(),
            status_code: "A".to_string(),
            staged: true,
        },
        GitFileChange {
            path: "modified.rs".to_string(),
            status_code: "M".to_string(),
            staged: false,
        },
        GitFileChange {
            path: "untracked.rs".to_string(),
            status_code: "?".to_string(),
            staged: false,
        },
    ];

    let mut state = GitDialogState::with_changes(PathBuf::from("."), changes);
    assert_eq!(state.selected, 0);
    assert_eq!(
        state.selected_change(),
        Some(&GitFileChange {
            path: "staged.rs".to_string(),
            status_code: "A".to_string(),
            staged: true,
        })
    );

    // Navigation down & wrap
    state.next();
    assert_eq!(state.selected, 1);
    state.next();
    assert_eq!(state.selected, 2);
    state.next();
    assert_eq!(state.selected, 0);

    // Navigation up & wrap
    state.previous();
    assert_eq!(state.selected, 2);
    state.previous();
    assert_eq!(state.selected, 1);
    state.previous();
    assert_eq!(state.selected, 0);

    // Commit mode transitions
    assert!(!state.commit_mode);
    state.enter_commit_mode();
    assert!(state.commit_mode);

    state.push_commit_char('f');
    state.push_commit_char('i');
    state.push_commit_char('x');
    assert_eq!(state.commit_message, "fix");

    state.push_commit_str(": typo");
    assert_eq!(state.commit_message, "fix: typo");

    state.pop_commit_char();
    assert_eq!(state.commit_message, "fix: typ");

    state.exit_commit_mode();
    assert!(!state.commit_mode);

    // Commit empty error
    state.commit_message.clear();
    let empty_res = state.commit();
    assert!(empty_res.is_err());
    assert!(state.status_message.is_some());

    // Diff scroll
    state.diff = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(state.diff_scroll, 0);
    state.scroll_diff_down(5);
    assert_eq!(state.diff_scroll, 5);
    state.scroll_diff_up(2);
    assert_eq!(state.diff_scroll, 3);
    state.scroll_diff_up(10);
    assert_eq!(state.diff_scroll, 0);
}

#[test]
fn git_dialog_toggle_stage_integration() {
    let repo = TempRepo::new();
    let file1 = repo.path.join("file1.txt");
    std::fs::write(&file1, "hello\n").unwrap();

    let mut state = GitDialogState::new(&repo.path);
    assert_eq!(state.changes.len(), 1);
    assert!(!state.changes[0].staged);

    // Toggle stage -> should stage
    state.toggle_stage();
    assert_eq!(state.changes.len(), 1);
    assert!(state.changes[0].staged);

    // Toggle stage -> should unstage
    state.toggle_stage();
    assert_eq!(state.changes.len(), 1);
    assert!(!state.changes[0].staged);
}

#[test]
fn git_dialog_commit_flow() {
    let repo = TempRepo::new();
    let file1 = repo.path.join("file1.txt");
    std::fs::write(&file1, "commit test\n").unwrap();

    let mut state = GitDialogState::new(&repo.path);
    state.toggle_stage();
    assert!(state.changes[0].staged);

    state.enter_commit_mode();
    state.push_commit_str("test commit from dialog");
    let res = state.commit();
    assert!(res.is_ok());
    assert!(!state.commit_mode);
    assert!(state.changes.is_empty());
}

#[test]
fn git_dialog_render_test() {
    let changes = vec![
        GitFileChange {
            path: "staged_file.rs".to_string(),
            status_code: "A".to_string(),
            staged: true,
        },
        GitFileChange {
            path: "modified_file.rs".to_string(),
            status_code: "M".to_string(),
            staged: false,
        },
        GitFileChange {
            path: "untracked_file.rs".to_string(),
            status_code: "?".to_string(),
            staged: false,
        },
    ];

    let mut state = GitDialogState::with_changes(PathBuf::from("."), changes);
    state.diff = "+added line\n-removed line\n@@ -1,3 +1,3 @@\n regular line".to_string();

    let theme = Theme::new();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            render_git_dialog(f, f.area(), &state, &theme);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(content.contains("[S]"));
    assert!(content.contains("[U]"));
    assert!(content.contains("[?]"));
    assert!(content.contains("staged_file.rs"));
    assert!(content.contains("Git Panel"));

    // Also test render in commit mode
    state.enter_commit_mode();
    state.push_commit_str("my message");

    terminal
        .draw(|f| {
            render_git_dialog(f, f.area(), &state, &theme);
        })
        .unwrap();

    let buffer2 = terminal.backend().buffer();
    let content2 = (0..buffer2.area.height)
        .map(|y| {
            (0..buffer2.area.width)
                .map(|x| buffer2[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(content2.contains("Commit message:"));
    assert!(content2.contains("my message"));
}
