# Conversation UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect TUI, provider streaming, sessions, commands, workspace review, and metrics into bounded conversation UX.

**Architecture:** Add a conversation orchestration layer that consumes existing `Provider` streams and emits bounded UI events. Keep rendering in `tui`, persistence behind existing DB/writer APIs, commands in `cli`, and workspace safety behind `Workspace`.

**Tech Stack:** Rust 2024, Ratatui 0.30, Crossterm 0.29, rusqlite 0.37, existing provider/workspace abstractions.

## Global Constraints

- PLAN is read-only.
- BUILD mutation order remains `validate → snapshot → diff → policy decision → approval → apply`.
- UI, prompt, transcript, tool arguments, diagnostics, and persisted messages stay bounded.
- Streaming deltas are not persisted as individual rows.
- Provider/network/SQLite work must not block render loop.
- No `#[allow(...)]`.
- Every task ends with focused tests and project gates.

---

### Task 1: Conversation runtime contract

**Files:**
- Create: `src/conversation/mod.rs`
- Modify: `src/lib.rs`
- Test: `tests/conversation_runtime.rs`

**Interfaces:**
- Consumes: existing `Provider`, `StreamRequest`, `StreamEvent`, `ProviderStream`.
- Produces: `ConversationRuntime`, `ConversationEvent`, bounded turn state, assembled assistant output.

- [ ] **Step 1: Add failing tests** for text assembly, usage/finish propagation, provider error, and terminal cancellation.
- [ ] **Step 2: Run focused tests and verify failure.**
- [ ] **Step 3: Implement runtime using existing provider stream and bounded channel; coalesce text only through existing queue limits. Persist no deltas.**
- [ ] **Step 4: Run `cargo test --test conversation_runtime`.**
- [ ] **Step 5: Commit `feat: add conversation runtime`.**

### Task 2: TUI conversation state

**Files:**
- Modify: `src/tui/app.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/render.rs`
- Test: `tests/tui_conversation.rs`

**Interfaces:**
- Consumes: `ConversationEvent`.
- Produces: App state transitions for prompt submission, streaming, finish, error, cancellation, mode, and selected model.

- [ ] **Step 1: Add failing tests** for event application, cancellation priority, bounded transcript, and PLAN mode mutation rejection.
- [ ] **Step 2: Run focused tests and verify failure.**
- [ ] **Step 3: Extend `App` with minimal conversation state and event mapping; preserve existing queue behavior.**
- [ ] **Step 4: Render active-turn, mode, provider/model, and diagnostic state without unbounded allocations.**
- [ ] **Step 5: Run focused tests and `cargo test --all-targets --all-features`.**
- [ ] **Step 6: Commit `feat: connect conversation state to tui`.**

### Task 3: Session and slash-command flow

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/persistence/db.rs`
- Modify: `src/persistence/writer.rs`
- Test: `tests/conversation_commands.rs`

**Interfaces:**
- Consumes: `Db`, existing `CommandService`, `DiscoveryService`.
- Produces: command parsing/results for `/new`, `/sessions`, `/connect`, `/models`, `/models refresh`, `/exit`, and PLAN/BUILD mode switching.

- [ ] **Step 1: Add failing tests** for command parsing, session creation/list/select, mode switching, unknown-command diagnostics, and non-blocking refresh.
- [ ] **Step 2: Run focused tests and verify failure.**
- [ ] **Step 3: Add only missing command/result state; route session writes through existing writer boundary where available.**
- [ ] **Step 4: Enforce bounded session titles and actionable errors.**
- [ ] **Step 5: Run focused tests and project gates.**
- [ ] **Step 6: Commit `feat: add conversation session commands`.**

### Task 4: Tool lifecycle and diff review

**Files:**
- Create: `src/conversation/tools.rs`
- Modify: `src/workspace/mod.rs` only where integration requires existing public APIs.
- Modify: `src/tui/app.rs`
- Test: `tests/conversation_tools.rs`

**Interfaces:**
- Consumes: `Workspace`, `Operation`, `PolicyDecision`, snapshot/diff APIs.
- Produces: requested/running/completed/failed/cancelled/awaiting-approval lifecycle and reviewable diff state.

- [ ] **Step 1: Add failing tests** for read-only PLAN tools, BUILD diff-before-apply, approval rejection, cancellation, and failure isolation.
- [ ] **Step 2: Run focused tests and verify failure.**
- [ ] **Step 3: Implement lifecycle state machine; call workspace APIs only after validation and policy decision.**
- [ ] **Step 4: Keep diff visible before approval/apply and leave workspace unchanged on rejection.**
- [ ] **Step 5: Run focused tests and project gates.**
- [ ] **Step 6: Commit `feat: add workspace tool review lifecycle`.**

### Task 5: Metrics and persistence integration

**Files:**
- Modify: `src/provider/metrics.rs`
- Modify: `src/conversation/mod.rs`
- Modify: `src/persistence/writer.rs`
- Modify: `src/tui/render.rs`
- Test: `tests/conversation_metrics.rs`

**Interfaces:**
- Consumes: stream timestamps, `Usage`, `Finish`, provider/model identity.
- Produces: bounded TTFT, latency, duration, usage, finish status, and assembled-message persistence.

- [ ] **Step 1: Add failing tests** for TTFT, duration, usage presence/absence, finish status, and one final assembled message write.
- [ ] **Step 2: Run focused tests and verify failure.**
- [ ] **Step 3: Implement metrics updates at event boundaries and enqueue final message only after terminal success.**
- [ ] **Step 4: Render metrics without doing database or provider work.**
- [x] **Step 5: Run focused tests and project gates.**
- [x] **Step 6: Commit `feat: add conversation metrics and persistence`.**

Task 5 follow-up (2026-09-11): runtime metrics now emit only after successful terminal
finish (`Stop`, `Length`, or `ToolCall`); error/cancelled streams emit no metrics. New
prompt submission clears previous UI metrics. `TurnState::metrics()` is optional and
returns no metrics for failed, cancelled, or incomplete runs. `App` stores metrics only
for matching successful `Finished` status. Persistence success/error/cancellation
behavior remains covered by `tests/conversation_metrics.rs`; UI status gating is covered
by `tests/tui_conversation.rs`.

### Task 6: Scripted UX gate

**Files:**
- Modify: `src/tui/input.rs` if key mappings are missing.
- Modify: `src/tui/mod.rs` for injectable runtime seams.
- Create: `tests/conversation_flow.rs`
- Modify: `docs/TODO.md`

**Interfaces:**
- Consumes: completed runtime, commands, tool lifecycle, metrics, and TUI state.
- Produces: deterministic scripted flow coverage and M7 checklist status.

- [ ] **Step 1: Add failing scripted tests** covering `/new`, `/sessions`, `/connect`, `/models`, PLAN, BUILD, cancel, and `/exit`.
- [ ] **Step 2: Run focused tests and verify failure.**
- [ ] **Step 3: Add minimal test seams; do not introduce a second runtime or production-only bypass.**
- [ ] **Step 4: Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets --all-features`, and `git diff --check`.**
- [ ] **Step 5: Mark `P0-M7-01` through `P0-M7-03` complete only when covered.**
- [ ] **Step 6: Commit `feat: complete conversation UX milestone`.**
