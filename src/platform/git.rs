use std::path::Path;
use std::process::Command;

pub fn get_current_branch() -> Option<String> {
    get_branch_for_path(Path::new("."))
}

pub fn get_branch_for_path(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;

    if output.status.success() {
        let branch = String::from_utf8(output.stdout).ok()?;
        let branch = branch.trim();
        if branch.is_empty() || branch == "HEAD" {
            None
        } else {
            Some(branch.to_string())
        }
    } else {
        None
    }
}

pub fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_current_branch_returns_non_empty_if_in_repo() {
        if let Some(branch) = get_current_branch() {
            assert!(!branch.is_empty());
            assert_ne!(branch, "HEAD");
        }
    }

    #[test]
    fn is_git_repo_matches_status() {
        let in_repo = is_git_repo(Path::new("."));
        assert!(in_repo);
    }
}
