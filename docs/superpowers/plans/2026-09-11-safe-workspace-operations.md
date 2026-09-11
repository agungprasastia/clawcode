# Safe Workspace Operations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add canonical workspace boundaries, bounded reads, transactional BUILD mutations, approval policy, snapshots, undo/redo, and shell risk validation.

**Architecture:** `WorkspaceRoot` owns canonical-path validation. `Workspace` is a facade over a small `FileSystem` trait, policy evaluator, and in-memory snapshot ledger. A requested mutation is validated, snapshotted, diffed, policy-checked, approved when required, then applied by temporary-file rename; partial apply restores earlier paths.

**Tech Stack:** Rust 2024, standard library filesystem/process APIs, `sha2` for snapshot checksums, integration tests using temporary directories.

## Global Constraints

- Support Windows, Linux, macOS through portable stdlib APIs; no scattered `cfg(target_os)`.
- Warm startup has no network dependency.
- BUILD mutation order is exactly `validate → snapshot → diff → policy decision → approval → apply`.
- PLAN never mutates files or executes shell commands.
- No `#[allow(...)]` warning suppression.
- Shell has no rollback claim.
- All workspace failures surface as `Diagnostic` category `Workspace`.

---

### Task 1: Canonical root and bounded reads

**Files:**
- Create: `src/workspace/root.rs`
- Create: `src/workspace/files.rs`
- Modify: `src/workspace/mod.rs`, `src/lib.rs`, `Cargo.toml`
- Test: `tests/workspace_policy.rs`

**Interfaces:**
- Produces `WorkspaceRoot::open(path: impl AsRef<Path>) -> Result<WorkspaceRoot, Diagnostic>`.
- Produces `WorkspaceRoot::resolve(relative: impl AsRef<Path>) -> Result<PathBuf, Diagnostic>`.
- Produces `ReadResult { bytes: Vec<u8>, truncated: bool }` and `Workspace::read(relative, max_bytes)`.
- `FileSystem` exposes `read`, `write`, `rename`, `remove_file`, `exists`, and `canonicalize`.

- [ ] **Step 1: Write failing boundary and read tests**

```rust
#[test]
fn root_rejects_parent_traversal() {
    let root = test_root();
    assert!(WorkspaceRoot::open(&root).unwrap().resolve("../secret.txt").is_err());
}

#[test]
fn read_truncates_at_requested_limit() {
    let root = test_root_with_file("note.txt", b"abcdef");
    let workspace = Workspace::open(&root).unwrap();
    assert_eq!(workspace.read("note.txt", 4).unwrap(), ReadResult {
        bytes: b"abcd".to_vec(), truncated: true,
    });
}
```

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test --test workspace_policy root_rejects_parent_traversal read_truncates_at_requested_limit`

Expected: FAIL because workspace public API does not exist.

- [ ] **Step 3: Add SHA-256 dependency and filesystem/root implementation**

```toml
sha2 = "0.10"
```

```rust
pub trait FileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
}
```

Implement `WorkspaceRoot::resolve` by rejecting absolute paths and `..` components, canonicalizing existing targets, and canonicalizing target parent for new targets. Ensure canonical target starts with root. `Workspace::read` reads once, truncates to `max_bytes`, and reports truncation.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test --test workspace_policy root_rejects_parent_traversal read_truncates_at_requested_limit`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/workspace/root.rs src/workspace/files.rs src/workspace/mod.rs tests/workspace_policy.rs
git commit -m "feat: add workspace root boundary and reads"
```

### Task 2: Policy modes and shell risk classifier

**Files:**
- Create: `src/workspace/policy.rs`
- Create: `src/workspace/shell.rs`
- Modify: `src/workspace/mod.rs`
- Test: `tests/workspace_policy.rs`

**Interfaces:**
- Produces `Mode::{Plan, Build}`, `Operation::{Write, Delete, Shell}`, and `PolicyDecision::{Allowed, ApprovalRequired, Denied}`.
- Produces `Policy::evaluate(mode, operation) -> PolicyDecision`.
- Produces `classify_shell(command: &str) -> Risk`.
- Produces `Workspace::validate_shell(mode, relative_cwd, command) -> Result<PolicyDecision, Diagnostic>`.

- [ ] **Step 1: Write failing PLAN and shell-risk tests**

```rust
#[test]
fn plan_denies_file_mutation() {
    assert_eq!(Policy::evaluate(Mode::Plan, Operation::Write), PolicyDecision::Denied);
}

#[test]
fn dangerous_shell_requires_approval() {
    assert_eq!(classify_shell("git reset --hard"), Risk::Destructive);
    assert_eq!(Policy::evaluate(Mode::Build, Operation::Shell(Risk::Destructive)),
        PolicyDecision::ApprovalRequired);
}
```

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test --test workspace_policy plan_denies_file_mutation dangerous_shell_requires_approval`

Expected: FAIL because policy types and classifier do not exist.

- [ ] **Step 3: Implement minimal policy and shell validation**

```rust
pub enum Risk { Safe, Destructive, DependencyInstall, NetworkMutation, PrivilegeEscalation }
```

Make PLAN deny `Write`, `Delete`, and every `Shell`. In BUILD, safe write is allowed; delete, sensitive overwrite, and every non-safe shell risk requires approval. Classify `git reset`, `git clean`, `rm -rf`, `del /s`, package installers, `curl|sh`, `sudo`, and `runas` as non-safe. Validate shell CWD through `WorkspaceRoot::resolve`.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test --test workspace_policy plan_denies_file_mutation dangerous_shell_requires_approval`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workspace/policy.rs src/workspace/shell.rs src/workspace/mod.rs tests/workspace_policy.rs
git commit -m "feat: add workspace policy and shell risks"
```

### Task 3: Snapshots, checksums, and restore

**Files:**
- Create: `src/workspace/snapshot.rs`
- Modify: `src/workspace/files.rs`, `src/workspace/mod.rs`
- Test: `tests/workspace_policy.rs`

**Interfaces:**
- Produces `SnapshotId`, `FileState::{Missing, Present { bytes, checksum }}`, and `Snapshot`.
- Produces `SnapshotStore::capture(path, before, after) -> SnapshotId`.
- Produces `SnapshotStore::restore_before(id, filesystem)` and `restore_after(id, filesystem)`.

- [ ] **Step 1: Write failing checksum and restore tests**

```rust
#[test]
fn restore_rejects_tampered_snapshot() {
    let mut store = SnapshotStore::default();
    let id = store.capture(PathBuf::from("note.txt"), FileState::present(b"old"), FileState::present(b"new"));
    store.tamper_for_test(id, b"changed".to_vec());
    assert!(store.restore_before(id, &RealFileSystem).is_err());
}

#[test]
fn restore_replaces_file_with_before_state() {
    // create note.txt="new", capture before="old", restore_before, then assert file="old"
}
```

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test --test workspace_policy restore_rejects_tampered_snapshot restore_replaces_file_with_before_state`

Expected: FAIL because snapshots do not exist.

- [ ] **Step 3: Implement checksum-validated atomic restore**

Hash each present state with `Sha256`. Before restore, recompute hash and reject mismatch. Write present state to sibling temporary path, then rename it over target; restore `Missing` by removing target if present. Keep testing tampering in a test-only helper, not production API.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test --test workspace_policy restore_rejects_tampered_snapshot restore_replaces_file_with_before_state`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workspace/snapshot.rs src/workspace/files.rs src/workspace/mod.rs tests/workspace_policy.rs
git commit -m "feat: add checksum workspace snapshots"
```

### Task 4: Transactional mutations and approval

**Files:**
- Modify: `src/workspace/mod.rs`, `src/workspace/policy.rs`, `src/workspace/snapshot.rs`
- Test: `tests/workspace_policy.rs`

**Interfaces:**
- Produces `Mutation::{Write { path, bytes }, Delete { path }}`, `Diff`, and `TransactionResult { snapshot_ids, diffs }`.
- Produces `Workspace::build(mode, mutations, approved) -> Result<TransactionResult, Diagnostic>`.
- `build` follows validate, snapshot, diff, policy, approval, apply.

- [ ] **Step 1: Write failing ordering, approval, and rollback tests**

```rust
#[test]
fn denied_transaction_does_not_mutate_file() {
    let workspace = test_workspace_with_file("note.txt", b"old");
    assert!(workspace.build(Mode::Plan, vec![write("note.txt", b"new")], false).is_err());
    assert_eq!(read_file(workspace.root(), "note.txt"), b"old");
}

#[test]
fn approval_required_transaction_applies_only_when_approved() {
    // deleting a file with approved=false leaves it unchanged; approved=true removes it
}

#[test]
fn failed_second_apply_restores_first_file() {
    // FaultInjectingFileSystem fails second rename; both files retain before bytes
}
```

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test --test workspace_policy denied_transaction_does_not_mutate_file approval_required_transaction_applies_only_when_approved failed_second_apply_restores_first_file`

Expected: FAIL because `build` does not exist.

- [ ] **Step 3: Implement transaction pipeline**

Resolve and validate all mutation paths before reading state. Capture before/after state and produce deterministic diff before policy. Deny immediately on PLAN. If any decision requires approval and `approved` is false, return workspace diagnostic before apply. Apply writes using temporary path plus rename; apply deletes only after policy. On failure, restore all already-applied snapshots in reverse order and return error.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test --test workspace_policy denied_transaction_does_not_mutate_file approval_required_transaction_applies_only_when_approved failed_second_apply_restores_first_file`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workspace/mod.rs src/workspace/policy.rs src/workspace/snapshot.rs tests/workspace_policy.rs
git commit -m "feat: add transactional workspace mutations"
```

### Task 5: Undo/redo, symlink escape, and M6 gate

**Files:**
- Modify: `src/workspace/root.rs`, `src/workspace/mod.rs`, `docs/TODO.md`
- Test: `tests/workspace_policy.rs`

**Interfaces:**
- Produces `Workspace::undo() -> Result<(), Diagnostic>` and `Workspace::redo() -> Result<(), Diagnostic>`.
- Undo applies snapshot before-state; redo applies after-state.

- [ ] **Step 1: Write failing undo/redo and symlink tests**

```rust
#[test]
fn undo_then_redo_restores_both_file_versions() {
    // build old→new, undo asserts old, redo asserts new
}

#[cfg(unix)]
#[test]
fn root_rejects_symlink_escape() {
    // symlink root/link to an external directory; resolve("link/secret.txt") returns error
}
```

On Windows, create the symlink only when test environment grants privilege; otherwise retain portable canonical traversal test as the required gate.

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test --test workspace_policy undo_then_redo_restores_both_file_versions`

Expected: FAIL because history operations do not exist.

- [ ] **Step 3: Implement history and finalize boundary checks**

Keep applied transaction snapshot IDs in undo stack; `undo` restores before-state and moves ID to redo stack; `redo` restores after-state and moves ID back. Clear redo stack after a new successful transaction. Ensure `resolve` follows existing symlink targets before checking root prefix.

- [ ] **Step 4: Run M6 validation gates**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

Expected: all pass with no warning suppression.

- [ ] **Step 5: Update ledger and commit**

Mark `P0-M6-01` through `P0-M6-06` complete only after gates pass.

```bash
git add src/workspace/root.rs src/workspace/files.rs src/workspace/policy.rs src/workspace/shell.rs src/workspace/snapshot.rs src/workspace/mod.rs tests/workspace_policy.rs docs/TODO.md Cargo.toml Cargo.lock
git commit -m "feat: add safe workspace operations"
```
