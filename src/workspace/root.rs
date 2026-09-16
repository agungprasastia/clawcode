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
        if !canonical_path.is_dir() {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                format!(
                    "workspace root is not a directory: {}",
                    canonical_path.display()
                ),
            ));
        }
        Ok(Self { canonical_path })
    }

    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn resolve(&self, relative: impl AsRef<Path>) -> Result<PathBuf, Diagnostic> {
        let relative = relative.as_ref();
        let relative = if let Ok(stripped) = relative.strip_prefix(&self.canonical_path) {
            stripped
        } else {
            relative
        };
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
        let resolved_path = if target.exists() {
            fs::canonicalize(&target)
        } else {
            let parent = target.parent().ok_or_else(|| {
                Diagnostic::new(ErrorCategory::Workspace, "workspace path has no parent")
            })?;
            let final_component = target.file_name().ok_or_else(|| {
                Diagnostic::new(
                    ErrorCategory::Workspace,
                    format!(
                        "workspace path has no final component: {}",
                        relative.display()
                    ),
                )
            })?;
            fs::canonicalize(parent).map(|canonical_parent| canonical_parent.join(final_component))
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

        if !resolved_path.starts_with(&self.canonical_path) {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                format!("workspace path escapes root: {}", relative.display()),
            ));
        }

        Ok(resolved_path)
    }
}
