# M6 Task 1 fix round 1 report

## Scope

Fixed all Important findings in `task-1-review-package.md` for safe workspace operations.

## Root cause

`WorkspaceRoot::open` accepted any canonical filesystem object, including regular files. `WorkspaceRoot::resolve` verified a canonical path but returned the original joined path, leaving callers with an uncanonicalized symlink path.

## Changes

- `WorkspaceRoot::open` now rejects canonical paths that are not directories with `ErrorCategory::Workspace`.
- `WorkspaceRoot::resolve` now returns:
  - canonical target for existing paths;
  - canonical parent joined to final validated component for new paths.
- Resolution rejects paths whose effective canonical target escapes workspace root.
- Added behavior coverage for:
  - absolute path rejection;
  - existing symlink escape;
  - new target whose parent symlink escapes root;
  - non-directory root rejection;
  - read failure workspace diagnostic;
  - zero-byte limit;
  - exact-byte limit;
  - canonical existing target return.
- Tests use RAII directory cleanup. Windows symlink tests skip only when symlink privilege is unavailable (`PermissionDenied` or Windows error 1314); all other symlink errors panic. Unix uses native directory symlinks.

## Test-first evidence

1. Added behavior tests before changing production workspace code.
2. Initial red run compiled tests and failed as expected for missing required behavior.
3. Implemented smallest root validation and canonical return-path changes.
4. Re-ran `cargo test --test workspace_policy`; all 10 tests passed. Windows environment skipped the three symlink-dependent assertions because error 1314 indicates unavailable symlink privilege.

## Verification

All commands exited successfully:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- `git diff --check`

## Concern

Windows host lacks directory-symlink privilege (error 1314), so symlink escape assertions are intentionally skipped on this host. They execute on Windows where symlink creation is permitted and on Unix.
