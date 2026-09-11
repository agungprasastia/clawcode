use crate::core::error::{Diagnostic, ErrorCategory};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct WorkspaceRoot {
    canonical_path: PathBuf,
}

impl WorkspaceRoot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Diagnostic> {
        let path = path.as_ref();
        let canonical_path = fs::canonicalize(path).map_err(|error| {
            Diagnostic::new(
                ErrorCategory::Workspace,
                format!("failed to open workspace {}: {error}", path.display()),
            )
        })?;
        Ok(Self { canonical_path })
    }

    pub fn resolve(&self, relative: impl AsRef<Path>) -> Result<PathBuf, Diagnostic> {
        let relative = relative.as_ref();
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::Prefix(_) | Component::RootDir
                )
            })
        {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                format!(
                    "workspace path must be relative without parent traversal: {}",
                    relative.display()
                ),
            ));
        }

        let target = self.canonical_path.join(relative);
        let checked_path = if target.exists() {
            fs::canonicalize(&target)
        } else {
            let parent = target.parent().ok_or_else(|| {
                Diagnostic::new(ErrorCategory::Workspace, "workspace path has no parent")
            })?;
            fs::canonicalize(parent)
        }
        .map_err(|error| {
            Diagnostic::new(
                ErrorCategory::Workspace,
                format!(
                    "failed to resolve workspace path {}: {error}",
                    relative.display()
                ),
            )
        })?;

        if !checked_path.starts_with(&self.canonical_path) {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                format!("workspace path escapes root: {}", relative.display()),
            ));
        }

        Ok(target)
    }
}
