# M8-02 Platform Adapters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add minimal cross-platform clipboard and shell discovery adapters while keeping credential-store access explicitly unsupported.

**Architecture:** Add a platform-neutral `platform` module exposing typed clipboard, shell discovery, and credential-store contracts. Keep `cfg(target_os)` and process invocation inside platform adapter code. Shell discovery uses filesystem inspection only; clipboard backends are isolated and testable through injected traits.

**Tech Stack:** Rust 2024, standard library, existing test harness; no new dependency unless current repository inspection proves one is already required.

## Global Constraints

- Clipboard payloads must be bounded before backend invocation.
- Shell discovery must never execute discovered binaries.
- Credential-store MVP returns explicit `Unsupported` and never reads or logs secrets.
- OS-specific code stays inside platform adapter modules.
- Adapter failures must return diagnostics, not panic.
- Render loop must not perform blocking adapter work.
- Required validation: `cargo fmt --all -- --check`, strict Clippy, all tests, `git diff --check`.

---

### Task 1: Add platform-neutral contracts and errors

**Files:**
- Create: `src/platform/mod.rs`
- Modify: `src/lib.rs`
- Test: `tests/platform.rs`

**Interfaces:**
- Produces `PlatformError`, `Clipboard`, `ShellDiscovery`, `CredentialStore`, `UnsupportedCredentialStore`, and `MAX_CLIPBOARD_BYTES`.
- `Clipboard::read() -> Result<String, PlatformError>`
- `Clipboard::write(&self, text: &str) -> Result<(), PlatformError>`
- `ShellDiscovery::find(&self, name: &str) -> Result<PathBuf, PlatformError>`
- `CredentialStore::get(&self, key: &str) -> Result<String, PlatformError>`

- [ ] **Step 1: Write failing contract tests**

Add tests for oversized clipboard input, empty shell name, unsupported credential lookup, and fake clipboard success/failure.

- [ ] **Step 2: Run focused tests**

Run: `cargo test --test platform`
Expected: FAIL because `clawcode::platform` and its contracts do not exist.

- [ ] **Step 3: Implement contracts and typed errors**

Define `PlatformError` variants for `InvalidInput`, `Unavailable`, `Unsupported`, `NotFound`, and `Io`. Export the module from `src/lib.rs`. Keep interfaces platform-neutral and avoid OS conditionals in tests.

- [ ] **Step 4: Run focused tests**

Run: `cargo test --test platform`
Expected: PASS.

- [ ] **Step 5: Commit**

Run: `git add src/platform/mod.rs src/lib.rs tests/platform.rs; git commit -m "feat: add platform adapter contracts"`

### Task 2: Implement PATH shell discovery

**Files:**
- Create: `src/platform/shell.rs`
- Modify: `src/platform/mod.rs`
- Modify: `tests/platform.rs`

**Interfaces:**
- Produces `PathShellDiscovery` with `new(path_entries: impl IntoIterator<Item = PathBuf>)`.
- `PathShellDiscovery::find(&self, name: &str) -> Result<PathBuf, PlatformError>`.

- [ ] **Step 1: Add failing discovery tests**

Use a unique temporary directory and marker file. Assert matching executable path is returned, missing names return `NotFound`, empty names return `InvalidInput`, and discovery does not execute the marker.

- [ ] **Step 2: Run focused test**

Run: `cargo test --test platform shell_discovery`
Expected: FAIL because `PathShellDiscovery` does not exist.

- [ ] **Step 3: Implement filesystem-only discovery**

Iterate supplied directories, reject empty names and names containing path separators, check candidate metadata, and return the first regular file. On Unix require executable permission; on Windows check the candidate and `PATHEXT` suffixes. Never call `Command`.

- [ ] **Step 4: Run focused tests and lint**

Run: `cargo test --test platform shell_discovery; cargo clippy --all-targets --all-features -- -D warnings`
Expected: PASS with no warnings.

- [ ] **Step 5: Commit**

Run: `git add src/platform/shell.rs src/platform/mod.rs tests/platform.rs; git commit -m "feat: add safe shell discovery"`

### Task 3: Implement clipboard and credential-store adapters

**Files:**
- Create: `src/platform/clipboard.rs`
- Create: `src/platform/credentials.rs`
- Modify: `src/platform/mod.rs`
- Modify: `tests/platform.rs`

**Interfaces:**
- Produces `SystemClipboard` implementing `Clipboard`.
- Produces `UnsupportedCredentialStore` implementing `CredentialStore`.

- [ ] **Step 1: Add failing adapter tests**

Assert oversized writes fail before backend invocation through a fake backend. Assert `UnsupportedCredentialStore::get("key")` returns `PlatformError::Unsupported` and does not expose the key in the error text.

- [ ] **Step 2: Run focused tests**

Run: `cargo test --test platform clipboard credentials`
Expected: FAIL because adapters do not exist.

- [ ] **Step 3: Implement bounded clipboard adapter**

Keep backend execution behind a private platform-specific command adapter. Reject text whose byte length exceeds `MAX_CLIPBOARD_BYTES` before spawning. Return `Unavailable` or `Io` diagnostics without including clipboard contents.

- [ ] **Step 4: Implement unsupported credential adapter**

Return stable `PlatformError::Unsupported("credential store is not available in MVP")` from every lookup. Do not read environment variables, files, or OS credential APIs.

- [ ] **Step 5: Run focused tests**

Run: `cargo test --test platform`
Expected: PASS.

- [ ] **Step 6: Commit**

Run: `git add src/platform/clipboard.rs src/platform/credentials.rs src/platform/mod.rs tests/platform.rs; git commit -m "feat: add clipboard and credential adapters"`

### Task 4: Final integration and validation

**Files:**
- Modify: `docs/TODO.md`
- Modify: `docs/ROADMAP.md` only if current checklist requires M8-02 status update.
- Test: `tests/platform.rs`

**Interfaces:**
- Consumes all adapters from Tasks 1–3.
- Produces M8-02 acceptance evidence without changing conversation result handling.

- [ ] **Step 1: Add cross-adapter failure isolation test**

Use fake clipboard and unsupported credential implementations. Assert adapter errors are returned independently and no test invokes a real credential store or shell command.

- [ ] **Step 2: Run complete validation**

Run: `cargo fmt --all -- --check; cargo clippy --all-targets --all-features -- -D warnings; cargo test --all-targets --all-features; git diff --check`
Expected: all commands pass.

- [ ] **Step 3: Update roadmap checklist honestly**

Mark M8-02 complete only when clipboard contract, PATH discovery, unsupported credential diagnostic, isolation tests, and validation all pass. Keep any unsupported platform behavior documented.

- [ ] **Step 4: Commit milestone**

Run: `git add docs/TODO.md docs/ROADMAP.md; git commit -m "feat: complete platform adapter milestone"`
