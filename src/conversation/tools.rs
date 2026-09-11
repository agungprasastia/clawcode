use crate::workspace::{Mode, Mutation, TransactionResult, Workspace};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolRequest {
    Read { path: PathBuf, max_bytes: usize },
    Build { mutations: Vec<Mutation> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolStatus {
    Requested,
    Running,
    Completed,
    Failed(String),
    Cancelled,
    AwaitingApproval,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ToolResult {
    pub status: ToolStatus,
    pub read: Option<crate::workspace::ReadResult>,
    pub transaction: Option<TransactionResult>,
}

pub struct ToolLifecycle<'a, F: crate::workspace::FileSystem> {
    workspace: &'a Workspace<F>,
    mode: Mode,
    cancelled: bool,
    pending: Option<Vec<Mutation>>,
    diff: Option<TransactionResult>,
}

impl<'a, F: crate::workspace::FileSystem> ToolLifecycle<'a, F> {
    pub fn new(workspace: &'a Workspace<F>, mode: Mode) -> Self {
        Self {
            workspace,
            mode,
            cancelled: false,
            pending: None,
            diff: None,
        }
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    pub fn request(&mut self, request: ToolRequest) -> ToolResult {
        if self.cancelled {
            return self.cancelled_result();
        }
        match request {
            ToolRequest::Read { path, max_bytes } => match self.workspace.read(path, max_bytes) {
                Ok(read) => ToolResult {
                    status: ToolStatus::Completed,
                    read: Some(read),
                    transaction: None,
                },
                Err(error) => self.failed(error.to_string()),
            },
            ToolRequest::Build { mutations } => {
                self.pending = Some(mutations);
                self.diff = None;
                ToolResult {
                    status: ToolStatus::Requested,
                    read: None,
                    transaction: None,
                }
            }
        }
    }
    pub fn review(&mut self) -> ToolResult {
        if self.cancelled {
            return self.cancelled_result();
        }
        let Some(mutations) = self.pending.clone() else {
            return self.failed("no pending build".into());
        };
        match self.workspace.build(self.mode, mutations, false) {
            Ok(transaction) => {
                self.diff = Some(transaction.clone());
                ToolResult {
                    status: ToolStatus::AwaitingApproval,
                    read: None,
                    transaction: Some(transaction),
                }
            }
            Err(error) => self.failed(error.to_string()),
        }
    }
    pub fn approve(&mut self, approved: bool) -> ToolResult {
        if self.cancelled {
            return self.cancelled_result();
        }
        let Some(mutations) = self.pending.take() else {
            return self.failed("no pending build".into());
        };
        if !approved {
            self.diff = None;
            return ToolResult {
                status: ToolStatus::Completed,
                read: None,
                transaction: None,
            };
        }
        match self.workspace.build(self.mode, mutations, true) {
            Ok(transaction) => {
                self.diff = None;
                ToolResult {
                    status: ToolStatus::Completed,
                    read: None,
                    transaction: Some(transaction),
                }
            }
            Err(error) => self.failed(error.to_string()),
        }
    }
    pub fn diff(&self) -> Option<&TransactionResult> {
        self.diff.as_ref()
    }
    fn cancelled_result(&mut self) -> ToolResult {
        self.pending = None;
        self.diff = None;
        ToolResult {
            status: ToolStatus::Cancelled,
            read: None,
            transaction: None,
        }
    }
    fn failed(&self, message: String) -> ToolResult {
        ToolResult {
            status: ToolStatus::Failed(message),
            read: None,
            transaction: None,
        }
    }
}
