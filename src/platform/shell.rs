use super::{PlatformError, ShellDiscovery};
use std::path::PathBuf;

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
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }

        Err(PlatformError::NotFound(format!(
            "shell executable not found: {name}"
        )))
    }
}
