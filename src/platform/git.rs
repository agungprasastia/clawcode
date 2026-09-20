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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileChange {
    pub path: String,
    pub status_code: String,
    pub staged: bool,
}

pub fn get_status(path: &Path) -> Result<Vec<GitFileChange>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["status", "--porcelain=v1"])
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "git status failed".to_string()
        } else {
            err
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut changes = Vec::new();

    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }
        let x = line.as_bytes()[0] as char;
        let y = line.as_bytes()[1] as char;
        let raw_path = line[3..].trim();
        let file_path = if let Some((_old, new)) = raw_path.split_once(" -> ") {
            new.trim_matches('"').to_string()
        } else {
            raw_path.trim_matches('"').to_string()
        };

        if x == '?' && y == '?' {
            changes.push(GitFileChange {
                path: file_path,
                status_code: "?".to_string(),
                staged: false,
            });
        } else {
            if x != ' ' {
                changes.push(GitFileChange {
                    path: file_path.clone(),
                    status_code: x.to_string(),
                    staged: true,
                });
            }
            if y != ' ' {
                changes.push(GitFileChange {
                    path: file_path,
                    status_code: y.to_string(),
                    staged: false,
                });
            }
        }
    }

    Ok(changes)
}

pub fn get_diff(path: &Path, file: &str, staged: bool) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(path).arg("diff");
    if staged {
        cmd.arg("--cached");
    }
    cmd.arg("--").arg(file);
    let output = cmd.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git diff failed".to_string()
        } else {
            err
        })
    }
}

pub fn stage_file(path: &Path, file: &str) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["add", "--", file])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git add failed".to_string()
        } else {
            err
        })
    }
}

pub fn unstage_file(path: &Path, file: &str) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["restore", "--staged", "--", file])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        return Ok(());
    }

    let fallback = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["reset", "HEAD", "--", file])
        .output()
        .map_err(|e| e.to_string())?;

    if fallback.status.success() {
        return Ok(());
    }

    let rm_cached = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rm", "--cached", "--", file])
        .output()
        .map_err(|e| e.to_string())?;

    if rm_cached.status.success() {
        return Ok(());
    }

    let err = String::from_utf8_lossy(&fallback.stderr).trim().to_string();
    Err(if err.is_empty() {
        "git unstage failed".to_string()
    } else {
        err
    })
}

pub fn commit(path: &Path, message: &str) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["commit", "-m", message])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            err
        })
    }
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
