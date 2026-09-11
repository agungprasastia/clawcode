#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Plan,
    Build,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Risk {
    Safe,
    Destructive,
    DependencyInstall,
    NetworkMutation,
    PrivilegeEscalation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Write,
    Delete,
    Shell(Risk),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyDecision {
    Allowed,
    ApprovalRequired,
    Denied,
}

pub struct Policy;

impl Policy {
    pub fn evaluate(mode: Mode, operation: Operation) -> PolicyDecision {
        match (mode, operation) {
            (Mode::Plan, _) => PolicyDecision::Denied,
            (Mode::Build, Operation::Write) | (Mode::Build, Operation::Shell(Risk::Safe)) => {
                PolicyDecision::Allowed
            }
            (Mode::Build, Operation::Delete | Operation::Shell(_)) => {
                PolicyDecision::ApprovalRequired
            }
        }
    }
}
