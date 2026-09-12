# Security Review — P1-M8-05

Date: 2026-09-12
Scope: PRD trust boundaries — configuration secrets, workspace paths, transactional mutations, shell approval, platform command adapters, and persistence limits.

## Findings

### S1 — Workspace boundary checks

`WorkspaceRoot::resolve` rejects absolute paths, parent traversal, root/prefix components, and canonical paths outside the workspace. Existing tests cover traversal, symlink escape, and mutation boundaries. Residual risk: filesystem changes between validation and apply are a TOCTOU class; transactional writes use temporary siblings and atomic replacement, but a future hardening pass should use directory handles or platform no-follow primitives.

Status: accepted residual risk for MVP; no silent boundary bypass found.

### S2 — Mutation policy

PLAN denies every mutation. BUILD requires approval for sensitive writes, deletes, and non-safe shell risk classes. Duplicate mutation paths and directory targets are rejected. Snapshots are checksum validated and bounded.

Status: pass.

### S3 — Shell execution classification

Shell commands are classified before policy evaluation. Dangerous classes include privilege escalation, destructive operations, dependency installation, and network mutation. Classification is conservative for covered patterns, but it is not a shell parser and must not be treated as complete command sandboxing.

Status: accepted MVP limitation; approval remains required for covered risky classes. Future work should use argv-level execution and explicit allowlists.

### S4 — Secrets

Config accepts only `env:` and `credential:` references; plaintext API keys are rejected. Debug output does not expose resolved values. Credential-store resolution remains explicitly unsupported and returns a non-secret diagnostic.

Status: pass for MVP scope.

### S5 — Platform commands and notifications

Clipboard and notification payloads are bounded. Arguments are passed separately where possible. Windows toast content is XML-escaped; macOS notification content is AppleScript-escaped. Notification failures are isolated from conversation results.

Status: pass for MVP scope.

### S6 — Persistence limits

Messages, sessions, queued writes, stream coalescing, tool arguments, and snapshots have explicit bounds. Async writer uses a bounded channel and returns backpressure errors.

Status: pass.

## Gate Evidence

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --all-targets --all-features -- -D warnings`: pass.
- `cargo test --all-targets --all-features -j 1`: pass after fixing Windows exact-name shell discovery.
- `git diff --check`: pass.

## Decision

No high-severity blocker found for MVP. Mark `P1-M8-05` complete only with this review committed and the validation commands rerun on the final tree. Keep TOCTOU and shell-parser limitations visible in release documentation.
