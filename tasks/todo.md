# Clawcode Runtime Task List

## Review gate

- [x] Review `tasks/plan.md` and this checklist.
- [x] Confirm first implementation is durable one-call local-tool settlement, not `session_input`.
- [x] Resolve first-slice identity and transaction open questions.
- [x] Record approval before any Rust source, migration, test, or configuration edit starts.

## Phase 1: First vertical slice, durable local-tool settlement

### Task 1: Define durable tool-call identity contract

- [x] Description: Require committed assistant `messages.id` before local side effect; carry `assistant_message_id`, `generation_id` correlation, provider `call_id`, tool name, and bounded arguments.
- [x] Acceptance: Contract distinguishes assistant message ID from generation ID and requires assistant message ID for side effects.
- [x] Acceptance: One provider call ID maps to one owning assistant message within a generation.
- [x] Acceptance: Lifecycle covers durable creation, running, completed, failed, and cancelled without crash recovery.
- [x] Focused verification: Review contract against current `Generation`, `Message`, `RuntimeEvent`, provider events, and tool entry points. No source implementation before approval.
- [x] Dependencies: Plan review approval.
- [x] Likely files: `src/persistence/db.rs`, `src/runtime/client.rs`, `src/runtime/mod.rs`, `src/conversation/tools.rs`, `tests/runtime.rs`.
- [x] Scope: 5 files, medium.

### Task 2: Add persistence and writer atomic call-settlement support

- [x] Description: Add schema/API support for returned assistant message IDs, durable tool-call identity, atomic settlement, durable event append, and existing output bounds.
- [x] Acceptance: Assistant tool-call message insertion returns committed `messages.id` before call enters `running`.
- [x] Acceptance: Settlement atomically records status, bounded result/error, owning assistant message ID, and durable event sequence.
- [x] Acceptance: Transaction failure leaves no successful settlement or false completion event.
- [x] Focused verification: Focused persistence tests for message IDs, migration, atomic success/failure, ownership, and output limits.
- [x] Dependencies: Task 1.
- [x] Likely files: `src/persistence/schema.rs`, `src/persistence/db.rs`, `src/persistence/writer.rs`, `src/persistence/mod.rs`, `tests/persistence.rs`.
- [x] Scope: 5 files, medium.

### Task 3: Run one-call settlement with thread, join, and reload

- [x] Description: After provider stream closure, durably insert assistant tool-call message, obtain exact ID, persist call identity, execute existing local tool in `std::thread`, join, settle atomically, reload history once, and end without continuation.
- [x] Acceptance: Tool side effect starts only after committed assistant message ID and call identity; events never substitute generation ID for assistant message ID.
- [x] Acceptance: Runtime joins child, persists one settlement, reloads durable history once, and leaves no detached worker.
- [x] Acceptance: Provider count is one; tool failure, cancellation, and panic-safe join paths report explicit settlement status.
- [x] Focused verification: Fake provider and deterministic tool/filesystem seam; assert message ID before side-effect latch, join, one provider request, durable settlement, and reloaded projection.
- [x] Dependencies: Task 2.
- [x] Likely files: `src/runtime/client.rs`, `src/conversation/tools.rs`, `src/persistence/db.rs`, `src/persistence/writer.rs`, `tests/runtime.rs`.
- [x] Scope: 5 files, medium.

### Task 4: Add focused first-slice regression tests

- [x] Description: Lock exact identity, ordering, join, settlement, cancellation, one provider request, and one reload with deterministic local tests.
- [x] Acceptance: Side-effect code observes committed assistant `messages.id` and fails if only generation ID is supplied.
- [x] Acceptance: Success and failure settlements are atomic and linked to exact assistant message.
- [x] Acceptance: One provider request, one joined local call, one settlement, and one durable reload are proven.
- [x] Focused verification: Run only focused persistence/runtime targets after implementation. No full suite, formatter, linter, or build in this task.
- [x] Dependencies: Task 3.
- [x] Likely files: `tests/persistence.rs`, `tests/runtime.rs`, `src/runtime/client.rs`.
- [x] Scope: 3 files, small to medium.

### Checkpoint 1: First vertical slice complete

- [x] Plan approval recorded before source edits.
- [x] Exact durable assistant message ID exists before each local tool side effect.
- [x] `generation_id` is correlation only, never tool identity.
- [x] One provider request produces one local call, one joined execution, one settlement, and one durable reload.
- [x] Focused persistence/runtime checks pass.
- [x] No prompt admission, coordinator, replay cursor, or Context Epoch implementation entered this slice.
- [x] Review checkpoint approves Phase 2.

## Phase 2: Durable prompt admission, later separate slice

### Task 5: Add `session_input` admission and promotion contract

- [x] Description: Add durable prompt inbox after local-tool settlement is stable; admission and promotion remain separate durable transitions.
- [x] Acceptance: Admission survives restart and does not make input model-visible.
- [x] Acceptance: Promotion atomically consumes one eligible row and appends one user message plus promotion event.
- [x] Acceptance: Admission errors create no partial durable input.
- [x] Focused verification: In-memory SQLite tests for migration, pending/promoted state, restart, ordering, limits, and atomic failure.
- [x] Dependencies: Checkpoint 1.
- [x] Likely files: `src/persistence/schema.rs`, `src/persistence/db.rs`, `src/persistence/writer.rs`, `src/persistence/mod.rs`, `tests/persistence.rs`.
- [x] Scope: 5 files, medium.

### Task 6: Route TUI prompt submission through admission

- [x] Description: Replace runtime-backed direct user-message append in `App::submit_user_prompt` with admission and promotion events.
- [x] Acceptance: Runtime-backed TUI has no direct user-message SQLite append.
- [x] Acceptance: Rejected admission is not shown as committed transcript content.
- [x] Acceptance: Successful admission/promotion renders one user turn.
- [x] Focused verification: Targeted TUI tests for admission success, rejection, event filtering, and duplicate prevention.
- [x] Dependencies: Task 5.
- [x] Likely files: `src/tui/app.rs`, `src/runtime/client.rs`, `tests/tui.rs`.
- [x] Scope: 3 files, medium.

### Checkpoint 2: Prompt admission

- [x] Durable admission and promotion are proven independently from local-tool settlement.
- [x] TUI no longer directly writes user messages in runtime-backed mode.
- [x] Review checkpoint approves coordinator work.

## Phase 3: Per-session coordinator, wake, and interrupt

### Task 7: Add process-local per-session coordinator and wake

- [x] Description: Add session-keyed coordinator; serialize one local drain, allow different sessions concurrently, coalesce wakes, and schedule admitted inputs at safe boundaries.
- [x] Acceptance: One session has at most one active drain and one follow-up wake.
- [x] Acceptance: Different sessions can run concurrently.
- [x] Acceptance: Repeated wakes do not duplicate promotions or provider calls.
- [x] Focused verification: Deterministic fake-provider tests for serialization, cross-session overlap, wake coalescing, and active cleanup.
- [x] Dependencies: Checkpoint 2.
- [x] Likely files: `src/runtime/coordinator.rs`, `src/runtime/client.rs`, `src/runtime/mod.rs`, `src/persistence/db.rs`, `tests/runtime.rs`.
- [x] Scope: 5 files, medium.

### Task 8: Add process-local interrupt cleanup

- [x] Description: Cancel current provider stream, join and unregister drain, clear coalesced wake, and preserve pending input.
- [x] Acceptance: Active interrupt emits one terminal cancellation and leaves coordinator idle.
- [x] Acceptance: Idle or unknown interrupt is idempotent and preserves pending input.
- [x] Acceptance: Wake during cleanup cannot start a second drain.
- [x] Focused verification: Blocking fake-provider cancellation test for cleanup, pending input, stale registry, and duplicate prevention.
- [x] Dependencies: Task 7.
- [x] Likely files: `src/runtime/coordinator.rs`, `src/runtime/client.rs`, `src/persistence/db.rs`, `tests/runtime.rs`.
- [x] Scope: 4 files, medium.

### Checkpoint 3: Local session control

- [x] Per-session serialization, cross-session concurrency, wake coalescing, and interrupt cleanup pass focused checks.
- [x] `active()` remains process-local.
- [x] Crash recovery, retry, and clustered ownership remain absent.

## Phase 4: Durable event replay cursor

### Task 9: Add replay-plus-tail session cursor

- [x] Description: Add per-session exclusive `after_seq` replay over `generation_events`; register live tail before history read; deduplicate; exclude `seq = 0` controls.
- [x] Acceptance: Cursor `N` returns every durable event with `seq > N` in ascending order, including handoff races.
- [x] Acceptance: Live-only controls never advance cursor.
- [x] Acceptance: Queue closure recovers from last committed sequence without durable event loss.
- [x] Focused verification: SQLite WAL race, cursor boundary, `seq = 0` filter, deduplication, and bounded recovery tests.
- [x] Dependencies: Checkpoint 3.
- [x] Likely files: `src/runtime/mod.rs`, `src/runtime/client.rs`, `src/persistence/db.rs`, `src/tui/app.rs`, `tests/runtime.rs`.
- [x] Scope: 5 files, medium.

### Task 10: Make TUI restore cursor-driven

- [x] Description: Restore session display from durable events after `ClientSessionState::loaded_until_seq`; ignore ephemeral deltas during replay.
- [x] Acceptance: Reconnect and session switch do not duplicate durable events.
- [x] Acceptance: Disconnected durable assistant/tool events replay; live-only fragments do not become history.
- [x] Acceptance: Cursor never advances on sequence zero or failed durable commit.
- [x] Focused verification: Seeded TUI replay tests for reconnect, transcript output, cursor values, and duplicate suppression.
- [x] Dependencies: Task 9.
- [x] Likely files: `src/tui/app.rs`, `src/tui/mod.rs`, `tests/tui.rs`.
- [x] Scope: 3 files, medium.

### Checkpoint 4: Replay contract

- [x] Replay/live handoff race passes.
- [x] TUI restore uses durable cursor without treating fragments as history.
- [x] Review checkpoint approves Context Epoch work.

## Phase 5: Context Epoch and instruction source

### Task 11: Add durable Context Epoch boundary

- [x] Description: Persist epoch ID, exact baseline system text, and JSON source snapshot; initialize and reconcile at safe boundaries.
- [x] Acceptance: Baseline is exact and reusable after restart.
- [x] Acceptance: Unavailable initial context blocks future promotion; unchanged source emits none; changed source advances snapshot atomically with one system message.
- [x] Acceptance: Epoch replacement remains out of scope.
- [x] Focused verification: In-memory tests for initialization, source states, atomic update, restart reuse, and request assembly.
- [x] Dependencies: Checkpoint 4.
- [x] Likely files: `src/persistence/schema.rs`, `src/persistence/db.rs`, `src/runtime/client.rs`, `src/conversation/context.rs`, `tests/runtime.rs`.
- [x] Scope: 5 files, medium.

### Task 12: Add first `InstructionSource`

- [x] Description: Read bounded `AGENTS.md` files from workspace root and parent chain; feed one deterministic aggregate into existing `SystemPromptComposer` and Context Epoch.
- [x] Acceptance: Same files produce stable source key, snapshot, and baseline bytes.
- [x] Acceptance: Missing file is valid absence; read failure is unavailable and does not erase prior value.
- [x] Acceptance: Changes appear only at next safe boundary as one aggregate update.
- [x] Focused verification: Temporary-workspace tests for ordering, missing files, size limits, read failure, changes, and provider messages.
- [x] Dependencies: Task 11.
- [x] Likely files: `src/conversation/context.rs`, `src/conversation/prompt.rs`, `src/runtime/client.rs`, `src/persistence/db.rs`, `tests/runtime.rs`.
- [x] Scope: 5 files, medium.

### Checkpoint 5: Context boundary

- [x] Context baseline and snapshot survive restart exactly.
- [x] Instruction changes reconcile only at safe boundaries.
- [x] Initial unavailable instruction context leaves future admitted prompt retryable.
- [x] No Effect, Bun, plugin boot, watcher, or remote-source architecture entered root crate.
- [x] Final review checkpoint passes.

## Deferred scope

- [ ] `session_input` is not first slice.
- [ ] No Effect, Bun, Tokio, async runtime, fibers, HTTP server, generated SDK, or OpenCode service/container architecture.
- [ ] No durable crash recovery, ambiguous-dispatch retry, backoff, retry budget, stale-owner fencing, clustered execution, or automatic settlement retry.
- [ ] No more than one provider request in first slice, multi-call policy, tool continuation, compaction, provider context-window recovery, native continuation metadata, or provider-specific pruning.
- [ ] No background jobs, subagents, MCP, plugin tools, permission registry redesign, or remote tool execution.
- [ ] No global, URL, remote, watcher-backed, plugin-defined, or hot-reload instruction sources.
- [ ] No provider-use policy implementation.
- [ ] No cross-session/global event aggregation.
- [ ] No durable execution identity for local drains.
- [ ] No broad inbox growth controls beyond existing bounded channels and message limits.
- [ ] No production rollout, migration cleanup, formatting, linting, or project-wide validation before plan approval and implementation completion.
