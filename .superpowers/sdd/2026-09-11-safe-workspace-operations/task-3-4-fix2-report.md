# Tasks 3-4 M6 fix 2 report

Status: complete

## Changes

- `RealFileSystem::write_new` now uses `OpenOptions::create_new(true)` for exclusive temporary sibling creation. Windows replacement still uses `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`.
- `Workspace::build` evaluates policy and approval before capturing snapshots. It checkpoints `SnapshotStore` and discards transaction-only snapshots on capture or apply failure.
- Added regression coverage for denied policy, missing approval, failed apply quota retention, and temporary-file collision safety.

## TDD evidence

- RED: `cargo test --test workspace_policy consume_snapshot_quota` exited 101 before snapshot discard implementation.
- GREEN: focused quota and collision regressions exited 0 after implementation.

## Verification

All exited 0:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- `git diff ec6e4ec..HEAD --check`
- `git diff --check`

## Concerns

- Generic test `FileSystem` implementations retain default check-then-write behavior. Production `RealFileSystem` uses exclusive OS creation; custom filesystems that need equivalent safety must override `write_new`.
