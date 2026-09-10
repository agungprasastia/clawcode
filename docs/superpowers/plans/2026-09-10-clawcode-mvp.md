# Clawcode MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build fast, portable Rust terminal coding assistant with provider abstraction, safe PLAN/BUILD workflow, sessions, streaming, and OpenCode-style compatibility.

**Architecture:** One Rust binary with thin `main.rs` composition root and focused modules: `core`, `provider`, `adapters`, `config`, `workspace`, `persistence`, `tui`, `notify`, `cli`. Traits isolate core from provider and OS details. No daemon, ACP, MCP, or plugin system in MVP.

**Tech Stack:** Rust stable, Ratatui, Crossterm, Tokio, SQLite, JSONC parser, HTTP client, serde, tracing, OS adapter modules.

## Global Constraints

- MVP supports Windows, Linux, and macOS architecture; platform parity may ship progressively.
- Warm/local startup target is ≤100 ms on baseline machine; network never blocks startup critical path.
- Build mutation order is `validate → snapshot → diff → policy decision → apply`.
- Read/edit/shell auto-allowed only inside project when policy allows; risky and outside-project operations require approval.
- Streaming uses normalized events, bounded channels, delta coalescing, render batching, and priority cancellation.
- Streaming deltas are not persisted as SQLite rows; persist assembled recovery/audit data only.
- SQLite writes are asynchronous/batched and never block render loop.
- Notifications/sound are best-effort and non-blocking.
- JSONC is primary global/project config; project overrides global; migrations use `schema_version`.
- Platform-specific code belongs in OS submodules/adapters; do not spread `cfg(target_os)` through codebase.
- All tool output, snapshots, model cache, and session history have size/retention limits.

---

### Task 1: Bootstrap crate and boundaries

**Files:** Create `Cargo.toml`, `src/main.rs`, `src/core/mod.rs`, `src/provider/mod.rs`, `src/adapters/mod.rs`, `src/config/mod.rs`, `src/workspace/mod.rs`, `src/persistence/mod.rs`, `src/tui/mod.rs`, `src/notify/mod.rs`, `src/cli/mod.rs`; Test `tests/bootstrap.rs`.

- [ ] Write bootstrap test asserting binary starts and exits cleanly with `--version`.
- [ ] Run `cargo test --test bootstrap`; expect initial failure before binary exists.
- [ ] Add minimal package and modules; keep `main.rs` as composition root only.
- [ ] Run `cargo test --test bootstrap`; expect pass.
- [ ] Commit `chore: bootstrap modular rust crate`.

### Task 2: Error model, tracing, and CI

**Files:** Create `src/core/error.rs`, `.github/workflows/ci.yml`, `tests/error.rs`; Modify `src/main.rs`.

- [ ] Test stable error categories and diagnostic source location.
- [ ] Implement typed errors and tracing initialization without changing module boundaries.
- [ ] Run `cargo test`; run `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] Add Windows/Linux/macOS CI jobs.
- [ ] Commit `chore: add diagnostics and platform ci`.

### Task 3: TUI shell and performance harness

**Files:** Create `src/tui/app.rs`, `src/tui/render.rs`, `src/tui/input.rs`, `benches/first_frame.rs`, `tests/tui.rs`; Modify `src/tui/mod.rs`.

- [x] Test quit, resize, and synthetic stream responsiveness.
- [x] Implement Ratatui/Crossterm event loop, Quiet editorial layout, keyboard input, and clean terminal restore.
- [x] Implement render batching interface accepting coalesced UI events.
- [x] Run TUI tests and benchmark; record warm/local first-frame measurement.
- [x] Commit `feat: add terminal ui shell`.

Benchmark recorded 2026-09-10 on local Windows development machine using Criterion release build: `first_frame_in_process_test_backend_120x40` mean **443.01 µs** (95% CI **429.58–456.97 µs**). This measures in-process Ratatui construction and first render to `TestBackend`; it is not process startup or real terminal I/O. Process startup remains unmeasured because Task 3 has no non-interactive TUI startup mode suitable for a truthful subprocess benchmark.

### Task 4: JSONC configuration

**Files:** Create `src/config/parser.rs`, `src/config/merge.rs`, `src/config/migration.rs`, `src/config/diagnostic.rs`, `tests/config.rs`, `config/schema.json`; Modify `src/config/mod.rs`.

- [ ] Add fixtures for valid JSONC, malformed line/column, unknown field, global/project override, and schema migration.
- [ ] Implement parsing, diagnostics, merge, `schema_version`, env references, and local schema without public URL dependency.
- [ ] Run `cargo test config`; verify no secret plaintext is emitted.
- [ ] Commit `feat: add jsonc project configuration`.

### Task 5: SQLite sessions and bounded state

**Files:** Create `src/persistence/db.rs`, `src/persistence/schema.rs`, `src/persistence/writer.rs`, `src/persistence/retention.rs`, `tests/persistence.rs`; Modify `src/persistence/mod.rs`.

- [ ] Test session CRUD, assembled message recovery, batched writes, migrations, and retention limits.
- [ ] Implement SQLite schema and async writer channel separate from render loop.
- [ ] Add size limits for history, output, snapshots, and model cache.
- [ ] Run persistence tests with restart simulation.
- [ ] Commit `feat: add session persistence`.

### Task 6: Normalized provider contract

**Files:** Create `src/provider/events.rs`, `src/provider/traits.rs`, `src/provider/registry.rs`, `src/provider/metrics.rs`, `tests/provider_contract.rs`.

- [ ] Test all normalized event variants, usage fields, finish normalization, incremental argument cap, and priority cancellation.
- [ ] Implement provider trait, capabilities, bounded stream, coalescing, and cancellation channel.
- [ ] Ensure core imports only internal events and traits.
- [ ] Run contract tests including a fake provider.
- [ ] Commit `feat: add normalized provider contract`.

### Task 7: Provider adapters and discovery

**Files:** Create `src/adapters/openai_compatible.rs`, `src/adapters/anthropic.rs`, `src/adapters/ollama.rs`, `src/provider/discovery.rs`, `tests/adapters.rs`; Modify `src/adapters/mod.rs`.

- [ ] Add mocked stream fixtures for each protocol, missing usage/reasoning, provider errors, and cancellation.
- [ ] Implement adapters translating native events into normalized events; support custom endpoint and capability flags.
- [ ] Implement cached stale-while-revalidate discovery, per-provider isolation, TTL/backoff, timeout, and `/models refresh` service API.
- [ ] Run adapter and cache fallback tests; verify startup path does not await discovery.
- [ ] Commit `feat: add provider adapters and discovery`.

### Task 8: Workspace tools and transactional BUILD

**Files:** Create `src/workspace/root.rs`, `src/workspace/files.rs`, `src/workspace/shell.rs`, `src/workspace/snapshot.rs`, `src/workspace/policy.rs`, `tests/workspace_policy.rs`; Modify `src/workspace/mod.rs`, `src/core/mod.rs`.

- [ ] Test canonical boundary, symlink escape, PLAN mutation denial, risky shell approval, snapshot checksum, atomic restore, and edit transaction order.
- [ ] Implement read limits and file mutation pipeline `validate → snapshot → diff → policy decision → apply`.
- [ ] Implement policy-gated shell execution; do not claim shell rollback.
- [ ] Run workspace security tests on all available OS targets.
- [ ] Commit `feat: add safe workspace operations`.

### Task 9: Conversation, modes, sessions, and commands

**Files:** Create `src/core/session.rs`, `src/core/agent.rs`, `src/cli/commands.rs`, `tests/commands.rs`; Modify `src/tui/app.rs`, `src/core/mod.rs`.

- [ ] Test `/new`, `/sessions`, `/connect`, `/models`, `/models refresh`, `/exit`, PLAN/BUILD switching, cancel, and resume.
- [ ] Connect TUI input to core orchestration, provider registry, workspace policy, diff review, and persistence.
- [ ] Add OpenCode-style agents, commands, permissions, and themes loaders.
- [ ] Run scripted PTY flow.
- [ ] Commit `feat: add interactive coding workflow`.

### Task 10: OS adapters, notifications, hardening, release

**Files:** Create `src/notify/platform/mod.rs`, `src/notify/platform/windows.rs`, `src/notify/platform/linux.rs`, `src/notify/platform/macos.rs`, `src/platform/mod.rs`, tests under `tests/platform/`; Create `README.md`, release workflow.

- [ ] Test notification/sound failure isolation, shell discovery, clipboard, credential-store fallback, and portable interface.
- [ ] Implement OS-specific adapters with localized conditional compilation only inside platform modules.
- [ ] Add profiling, fuzz/property tests, security review, packaging, docs, and full acceptance checklist.
- [ ] Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, benchmarks, and cross-platform smoke tests.
- [ ] Commit `release: prepare clawcode mvp`.
