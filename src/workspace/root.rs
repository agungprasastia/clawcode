use crate::core::error::{Diagnostic, ErrorCategory};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct WorkspaceRoot {
    canonical_path: PathBuf,
}

fn strip_unc_prefix(path: &Path) -> &Path {
    if let Some(s) = path.to_str()
        && let Some(stripped) = s.strip_prefix(r"\\?\")
    {
        return Path::new(stripped);
    }
    path
}

impl WorkspaceRoot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Diagnostic> {
        let path = path.as_ref();
        let path = if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        };
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
        let stripped_canonical = strip_unc_prefix(&self.canonical_path);
        let stripped_rel = strip_unc_prefix(relative);
        let relative = if let Ok(stripped) = relative.strip_prefix(&self.canonical_path) {
            stripped
        } else if let Ok(stripped) = stripped_rel.strip_prefix(stripped_canonical) {
            stripped
        } else if let Ok(stripped) = relative.strip_prefix(stripped_canonical) {
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
            fs::canonicalize(&target).map_err(|error| {
                Diagnostic::new(
                    ErrorCategory::Workspace,
                    format!(
                        "failed to resolve workspace path {}: {error}",
                        relative.display()
                    ),
                )
            })?
        } else {
            let existing_ancestor = target
                .ancestors()
                .find(|ancestor| ancestor.exists())
                .ok_or_else(|| {
                    Diagnostic::new(
                        ErrorCategory::Workspace,
                        format!(
                            "failed to find existing ancestor for workspace path: {}",
                            relative.display()
                        ),
                    )
                })?;
            let tail = target.strip_prefix(existing_ancestor).map_err(|error| {
                Diagnostic::new(
                    ErrorCategory::Workspace,
                    format!(
                        "failed to determine relative tail for {}: {error}",
                        relative.display()
                    ),
                )
            })?;
            let canonical_ancestor = fs::canonicalize(existing_ancestor).map_err(|error| {
                Diagnostic::new(
                    ErrorCategory::Workspace,
                    format!(
                        "failed to resolve workspace path {}: {error}",
                        relative.display()
                    ),
                )
            })?;
            if !canonical_ancestor.starts_with(&self.canonical_path)
                && !strip_unc_prefix(&canonical_ancestor).starts_with(stripped_canonical)
            {
                return Err(Diagnostic::new(
                    ErrorCategory::Workspace,
                    format!("workspace path escapes root: {}", relative.display()),
                ));
            }
            canonical_ancestor.join(tail)
        };

        if !resolved_path.starts_with(&self.canonical_path)
            && !strip_unc_prefix(&resolved_path).starts_with(stripped_canonical)
        {
            return Err(Diagnostic::new(
                ErrorCategory::Workspace,
                format!("workspace path escapes root: {}", relative.display()),
            ));
        }

        Ok(resolved_path)
    }
}
