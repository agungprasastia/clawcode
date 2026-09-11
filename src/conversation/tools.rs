use crate::workspace::{Mode, Mutation, TransactionResult, Workspace, WorkspacePreview};
use std::path::PathBuf;

pub const TOOL_READ_MAX_BYTES: usize = 256 * 1024;
pub const TOOL_MUTATION_MAX_BYTES: usize = 1024 * 1024;

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
    pub preview: Option<WorkspacePreview>,
}

pub struct ToolLifecycle<'a, F: crate::workspace::FileSystem> {
    workspace: &'a Workspace<F>,
    mode: Mode,
    cancelled: bool,
    pending: Option<Vec<Mutation>>,
    diff: Option<TransactionResult>,
    preview: Option<WorkspacePreview>,
}

impl<'a, F: crate::workspace::FileSystem> ToolLifecycle<'a, F> {
    pub fn new(workspace: &'a Workspace<F>, mode: Mode) -> Self {
        Self {
            workspace,
            mode,
            cancelled: false,
            pending: None,
            diff: None,
            preview: None,
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
            ToolRequest::Read { path, max_bytes } => match self
                .workspace
                .read(path, max_bytes.min(TOOL_READ_MAX_BYTES))
            {
                Ok(read) => ToolResult {
                    status: ToolStatus::Completed,
                    read: Some(read),
                    transaction: None,
                    preview: None,
                },
                Err(error) => self.failed(error.to_string()),
            },
            ToolRequest::Build { mutations } => {
                if mutations.iter().any(|mutation| matches!(mutation, Mutation::Write { bytes, .. } if bytes.len() > TOOL_MUTATION_MAX_BYTES)) {
                    return self.failed("workspace mutation exceeds tool byte limit".into());
                }
                self.pending = Some(mutations);
                self.diff = None;
                ToolResult {
                    status: ToolStatus::Requested,
                    read: None,
                    transaction: None,
                    preview: None,
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
        match self.workspace.preview(self.mode, &mutations) {
            Ok(preview) => {
                if preview
                    .decisions
                    .contains(&crate::workspace::PolicyDecision::Denied)
                {
                    self.clear_pending();
                    return self.failed("workspace mutation denied by policy".into());
                }
                let requires_approval = preview
                    .decisions
                    .contains(&crate::workspace::PolicyDecision::ApprovalRequired);
                let transaction = TransactionResult {
                    snapshot_ids: Vec::new(),
                    diffs: preview.diffs.clone(),
                };
                self.diff = Some(transaction.clone());
                self.preview = Some(preview.clone());
                if !requires_approval {
                    return match self.workspace.build(self.mode, mutations, true) {
                        Ok(applied) => {
                            self.clear_pending();
                            ToolResult {
                                status: ToolStatus::Completed,
                                read: None,
                                transaction: Some(applied),
                                preview: Some(preview),
                            }
                        }
                        Err(error) => {
                            self.clear_pending();
                            self.failed(error.to_string())
                        }
                    };
                }
                ToolResult {
                    status: ToolStatus::AwaitingApproval,
                    read: None,
                    transaction: Some(transaction),
                    preview: Some(preview),
                }
            }
            Err(error) => {
                self.clear_pending();
                self.failed(error.to_string())
            }
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
            self.preview = None;
            return ToolResult {
                status: ToolStatus::Completed,
                read: None,
                transaction: None,
                preview: None,
            };
        }
        match self.workspace.build(self.mode, mutations, true) {
            Ok(transaction) => {
                self.diff = None;
                self.preview = None;
                ToolResult {
                    status: ToolStatus::Completed,
                    read: None,
                    transaction: Some(transaction),
                    preview: None,
                }
            }
            Err(error) => {
                self.clear_pending();
                self.failed(error.to_string())
            }
        }
    }
    pub fn diff(&self) -> Option<&TransactionResult> {
        self.diff.as_ref()
    }
    fn cancelled_result(&mut self) -> ToolResult {
        self.clear_pending();
        ToolResult {
            status: ToolStatus::Cancelled,
            read: None,
            transaction: None,
            preview: None,
        }
    }
    fn clear_pending(&mut self) {
        self.pending = None;
        self.diff = None;
        self.preview = None;
    }
    fn failed(&mut self, message: String) -> ToolResult {
        self.clear_pending();
        ToolResult {
            status: ToolStatus::Failed(message),
            read: None,
            transaction: None,
            preview: None,
        }
    }
}
