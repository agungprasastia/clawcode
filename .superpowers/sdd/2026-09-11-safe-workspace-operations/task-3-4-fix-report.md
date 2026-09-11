# Tasks 3-4 fix report

Status: complete

- `Workspace::build` resolves and de-duplicates every mutation path before reading mutation target state.
- `Workspace<F: FileSystem = RealFileSystem>` retains `Workspace::open` and adds `Workspace::with_filesystem`.
- Snapshot storage now persists on `Workspace`; returned IDs support `restore_before` and `restore_after` after later builds.
- Added injected-filesystem regression coverage: second apply replacement fails, first changed file rolls back to original bytes.
- Temporary sibling names now include a monotonic operation ID to avoid deterministic collisions.
- Added validation-order regression coverage: invalid later path produces zero filesystem state reads.

TDD evidence:

- RED: `cargo test --test workspace_policy injected_second_replacement_restores_first_file` failed with missing `Workspace::with_filesystem` and `Workspace::restore_before`.
- GREEN: focused `workspace_policy` suite passed after implementation.

Verification:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- `git diff 946700f...HEAD --check`

All commands exited 0.

Concerns: snapshot storage is bounded at 8 MiB per workspace lifetime. Future Task 5 should specify snapshot eviction/retention semantics before unbounded transaction history is required.
