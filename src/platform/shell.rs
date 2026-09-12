use super::{PlatformError, ShellDiscovery};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PathShellDiscovery {
    entries: Vec<PathBuf>,
}

impl PathShellDiscovery {
    pub fn new(entries: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }
}

impl ShellDiscovery for PathShellDiscovery {
    fn find(&self, name: &str) -> Result<PathBuf, PlatformError> {
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err(PlatformError::InvalidInput(
                "shell name must be a non-empty executable name".into(),
            ));
        }

        for directory in &self.entries {
            for candidate in candidates(directory, name) {
                if is_executable(&candidate) {
                    return Ok(candidate);
                }
            }
        }

        Err(PlatformError::NotFound(format!(
            "shell executable not found: {name}"
        )))
    }
}

#[cfg(windows)]
fn candidates(directory: &Path, name: &str) -> Vec<PathBuf> {
    if Path::new(name).extension().is_some() {
        return vec![directory.join(name)];
    }

    let mut candidates = vec![directory.join(name)];
    candidates.extend(
        std::env::var_os("PATHEXT")
            .unwrap_or_default()
            .to_string_lossy()
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| directory.join(format!("{name}{extension}")))
            .collect::<Vec<_>>(),
    );
    candidates
}

#[cfg(not(windows))]
fn candidates(directory: &Path, name: &str) -> Vec<PathBuf> {
    vec![directory.join(name)]
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.is_file()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(any(windows, unix)))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovery_checks_files_without_executing_them() {
        let directory =
            std::env::temp_dir().join(format!("clawcode-shell-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let marker = directory.join("marker");
        let shell_name = if cfg!(windows) {
            "fake-shell.CMD"
        } else {
            "fake-shell"
        };
        let shell = directory.join(shell_name);
        fs::write(&shell, format!("touch {}", marker.display())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&shell, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let found = PathShellDiscovery::new([directory.clone()]).find("fake-shell");

        assert_eq!(found, Ok(shell));
        assert!(!marker.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
