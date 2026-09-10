use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCategory {
    Config,
    Provider,
    Workspace,
    Persistence,
    Internal,
}

impl ErrorCategory {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Provider => "provider",
            Self::Workspace => "workspace",
            Self::Persistence => "persistence",
            Self::Internal => "internal",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    path: String,
    line: usize,
    column: usize,
}

impl SourceLocation {
    pub fn new(path: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            path: path.into(),
            line,
            column,
        }
    }
}

#[derive(Debug)]
pub struct Diagnostic {
    category: ErrorCategory,
    message: String,
    source: Option<SourceLocation>,
}

impl Diagnostic {
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(mut self, source: SourceLocation) -> Self {
        self.source = Some(source);
        self
    }

    pub const fn category(&self) -> ErrorCategory {
        self.category
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn source(&self) -> Option<&SourceLocation> {
        self.source.as_ref()
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.category.as_str(), self.message)?;
        if let Some(source) = &self.source {
            write!(
                formatter,
                " ({}:{}:{})",
                source.path, source.line, source.column
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostic {}
