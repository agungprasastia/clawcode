# Clawcode Runtime Task List

## Review gate

- [ ] Review `tasks/plan.md` and this checklist.
- [ ] Confirm first implementation is durable one-call local-tool settlement, not `session_input`.
- [ ] Resolve first-slice identity and transaction open questions.
- [ ] Record approval before any Rust source, migration, test, or configuration edit starts.

## Phase 1: First vertical slice, durable local-tool settlement

### Task 1: Define durable tool-call identity contract

- [ ] Description: Require committed assistant `messages.id` before local side effect; carry `assistant_message_id`, `generation_id` correlation, provider `call_id`, tool name, and bounded arguments.
- [ ] Acceptance: Contract distinguishes assistant message ID from generation ID and requires assistant message ID for side effects.
- [ ] Acceptance: One provider call ID maps to one owning assistant message within a generation.
- [ ] Acceptance: Lifecycle covers durable creation, running, completed, failed, and cancelled without crash recovery.
- [ ] Focused verification: Review contract against current `Generation`, `Message`, `RuntimeEvent`, provider events, and tool entry points. No source implementation before approval.
- [ ] Dependencies: Plan review approval.
- [ ] Likely files: `src/persistence/db.rs`, `src/runtime/client.rs`, `src/runtime/mod.rs`, `src/conversation/tools.rs`, `tests/runtime.rs`.
- [ ] Scope: 5 files, medium.

### Task 2: Add persistence and writer atomic call-settlement support

- [ ] Description: Add schema/API support for returned assistant message IDs, durable tool-call identity, atomic settlement, durable event append, and existing output bounds.
- [ ] Acceptance: Assistant tool-call message insertion returns committed `messages.id` before call enters `running`.
- [ ] Acceptance: Settlement atomically records status, bounded result/error, owning assistant message ID, and durable event sequence.
- [ ] Acceptance: Transaction failure leaves no successful settlement or false completion event.
- [ ] Focused verification: Focused persistence tests for message IDs, migration, atomic success/failure, ownership, and output limits.
- [ ] Dependencies: Task 1.
- [ ] Likely files: `src/persistence/schema.rs`, `src/persistence/db.rs`, `src/persistence/writer.rs`, `src/persistence/mod.rs`, `tests/persistence.rs`.
- [ ] Scope: 5 files, medium.

### Task 3: Run one-call settlement with thread, join, and reload

- [ ] Description: After provider stream closure, durably insert assistant tool-call message, obtain exact ID, persist call identity, execute existing local tool in `std::thread`, join, settle atomically, reload history once, and end without continuation.
- [ ] Acceptance: Tool side effect starts only after committed assistant message ID and call identity; events never substitute generation ID for assistant message ID.
- [ ] Acceptance: Runtime joins child, persists one settlement, reloads durable history once, and leaves no detached worker.
- [ ] Acceptance: Provider count is one; tool failure, cancellation, and panic-safe join paths report explicit settlement status.
- [ ] Focused verification: Fake provider and deterministic tool/filesystem seam; assert message ID before side-effect latch, join, one provider request, durable settlement, and reloaded projection.
- [ ] Dependencies: Task 2.
- [ ] Likely files: `src/runtime/client.rs`, `src/conversation/tools.rs`, `src/persistence/db.rs`, `src/persistence/writer.rs`, `tests/runtime.rs`.
- [ ] Scope: 5 files, medium.

### Task 4: Add focused first-slice regression tests

- [ ] Description: Lock exact identity, ordering, join, settlement, cancellation, one provider request, and one reload with deterministic local tests.
- [ ] Acceptance: Side-effect code observes committed assistant `messages.id` and fails if only generation ID is supplied.
- [ ] Acceptance: Success and failure settlements are atomic and linked to exact assistant message.
- [ ] Acceptance: One provider request, one joined local call, one settlement, and one durable reload are proven.
- [ ] Focused verification: Run only focused persistence/runtime targets after implementation. No full suite, formatter, linter, or build in this task.
- [ ] Dependencies: Task 3.
- [ ] Likely files: `tests/persistence.rs`, `tests/runtime.rs`, `src/runtime/client.rs`.
- [ ] Scope: 3 files, small to medium.

### Checkpoint 1: First vertical slice complete

- [ ] Plan approval recorded before source edits.
- [ ] Exact durable assistant message ID exists before each local tool side effect.
- [ ] `generation_id` is correlation only, never tool identity.
- [ ] One provider request produces one local call, one joined execution, one settlement, and one durable reload.
- [ ] Focused persistence/runtime checks pass.
- [ ] No prompt admission, coordinator, replay cursor, or Context Epoch implementation entered this slice.
- [ ] Review checkpoint approves Phase 2.

## Phase 2: Durable prompt admission, later separate slice

### Task 5: Add `session_input` admission and promotion contract

- [ ] Description: Add durable prompt inbox after local-tool settlement is stable; admission and promotion remain separate durable transitions.
- [ ] Acceptance: Admission survives restart and does not make input model-visible.
- [ ] Acceptance: Promotion atomically consumes one eligible row and appends one user message plus promotion event.
- [ ] Acceptance: Admission errors create no partial durable input.
- [ ] Focused verification: In-memory SQLite tests for migration, pending/promoted state, restart, ordering, limits, and atomic failure.
- [ ] Dependencies: Checkpoint 1.
- [ ] Likely files: `src/persistence/schema.rs`, `src/persistence/db.rs`, `src/persistence/writer.rs`, `src/persistence/mod.rs`, `tests/persistence.rs`.
- [ ] Scope: 5 files, medium.

### Task 6: Route TUI prompt submission through admission

- [ ] Description: Replace runtime-backed direct user-message append in `App::submit_user_prompt` with admission and promotion events.
- [ ] Acceptance: Runtime-backed TUI has no direct user-message SQLite append.
- [ ] Acceptance: Rejected admission is not shown as committed transcript content.
- [ ] Acceptance: Successful admission/promotion renders one user turn.
- [ ] Focused verification: Targeted TUI tests for admission success, rejection, event filtering, and duplicate prevention.
- [ ] Dependencies: Task 5.
- [ ] Likely files: `src/tui/app.rs`, `src/runtime/client.rs`, `tests/tui.rs`.
- [ ] Scope: 3 files, medium.

### Checkpoint 2: Prompt admission

- [ ] Durable admission and promotion are proven independently from local-tool settlement.
- [ ] TUI no longer directly writes user messages in runtime-backed mode.
- [ ] Review checkpoint approves coordinator work.

## Phase 3: Per-session coordinator, wake, and interrupt

### Task 7: Add process-local per-session coordinator and wake

- [x] Description: Add session-keyed coordinator; serialize one local drain, allow different sessions concurrently, coalesce wakes, and schedule admitted inputs at safe boundaries.
- [x] Acceptance: One session has at most one active drain and one follow-up wake.
- [x] Acceptance: Different sessions can run concurrently.
- [x] Acceptance: Repeated wakes do not duplicate promotions or provider calls.
- [x] Focused verification: Deterministic fake-provider tests for serialization, cross-session overlap, wake coalescing, and active cleanup.
- [ ] Dependencies: Checkpoint 2.
- [ ] Likely files: `src/runtime/coordinator.rs`, `src/runtime/client.rs`, `src/runtime/mod.rs`, `src/persistence/db.rs`, `tests/runtime.rs`.
- [ ] Scope: 5 files, medium.

### Task 8: Add process-local interrupt cleanup

- [x] Description: Cancel current provider stream, join and unregister drain, clear coalesced wake, and preserve pending input.
- [x] Acceptance: Active interrupt emits one terminal cancellation and leaves coordinator idle.
- [x] Acceptance: Idle or unknown interrupt is idempotent and preserves pending input.
- [x] Acceptance: Wake during cleanup cannot start a second drain.
- [x] Focused verification: Blocking fake-provider cancellation test for cleanup, pending input, stale registry, and duplicate prevention.
- [ ] Dependencies: Task 7.
- [ ] Likely files: `src/runtime/coordinator.rs`, `src/runtime/client.rs`, `src/persistence/db.rs`, `tests/runtime.rs`.
- [ ] Scope: 4 files, medium.

### Checkpoint 3: Local session control

- [x] Per-session serialization, cross-session concurrency, wake coalescing, and interrupt cleanup pass focused checks.
- [x] `active()` remains process-local.
- [x] Crash recovery, retry, and clustered ownership remain absent.

## Phase 4: Durable event replay cursor

### Task 9: Add replay-plus-tail session cursor

- [ ] Description: Add per-session exclusive `after_seq` replay over `generation_events`; register live tail before history read; deduplicate; exclude `seq = 0` controls.
- [ ] Acceptance: Cursor `N` returns every durable event with `seq > N` in ascending order, including handoff races.
- [ ] Acceptance: Live-only controls never advance cursor.
- [ ] Acceptance: Queue closure recovers from last committed sequence without durable event loss.
- [ ] Focused verification: SQLite WAL race, cursor boundary, `seq = 0` filter, deduplication, and bounded recovery tests.
- [ ] Dependencies: Checkpoint 3.
- [ ] Likely files: `src/runtime/mod.rs`, `src/runtime/client.rs`, `src/persistence/db.rs`, `src/tui/app.rs`, `tests/runtime.rs`.
- [ ] Scope: 5 files, medium.

### Task 10: Make TUI restore cursor-driven

- [ ] Description: Restore session display from durable events after `ClientSessionState::loaded_until_seq`; ignore ephemeral deltas during replay.
- [ ] Acceptance: Reconnect and session switch do not duplicate durable events.
- [ ] Acceptance: Disconnected durable assistant/tool events replay; live-only fragments do not become history.
- [ ] Acceptance: Cursor never advances on sequence zero or failed durable commit.
- [ ] Focused verification: Seeded TUI replay tests for reconnect, transcript output, cursor values, and duplicate suppression.
- [ ] Dependencies: Task 9.
- [ ] Likely files: `src/tui/app.rs`, `src/tui/mod.rs`, `tests/tui.rs`.
- [ ] Scope: 3 files, medium.

### Checkpoint 4: Replay contract

- [ ] Replay/live handoff race passes.
- [ ] TUI restore uses durable cursor without treating fragments as history.
- [ ] Review checkpoint approves Context Epoch work.

## Phase 5: Context Epoch and instruction source

### Task 11: Add durable Context Epoch boundary

- [ ] Description: Persist epoch ID, exact baseline system text, and JSON source snapshot; initialize and reconcile at safe boundaries.
- [ ] Acceptance: Baseline is exact and reusable after restart.
- [ ] Acceptance: Unavailable initial context blocks future promotion; unchanged source emits none; changed source advances snapshot atomically with one system message.
- [ ] Acceptance: Epoch replacement remains out of scope.
- [ ] Focused verification: In-memory tests for initialization, source states, atomic update, restart reuse, and request assembly.
- [ ] Dependencies: Checkpoint 4.
- [ ] Likely files: `src/persistence/schema.rs`, `src/persistence/db.rs`, `src/runtime/client.rs`, `src/conversation/context.rs`, `tests/runtime.rs`.
- [ ] Scope: 5 files, medium.

### Task 12: Add first `InstructionSource`

- [ ] Description: Read bounded `AGENTS.md` files from workspace root and parent chain; feed one deterministic aggregate into existing `SystemPromptComposer` and Context Epoch.
- [ ] Acceptance: Same files produce stable source key, snapshot, and baseline bytes.
- [ ] Acceptance: Missing file is valid absence; read failure is unavailable and does not erase prior value.
- [ ] Acceptance: Changes appear only at next safe boundary as one aggregate update.
- [ ] Focused verification: Temporary-workspace tests for ordering, missing files, size limits, read failure, changes, and provider messages.
- [ ] Dependencies: Task 11.
- [ ] Likely files: `src/conversation/context.rs`, `src/conversation/prompt.rs`, `src/runtime/client.rs`, `src/persistence/db.rs`, `tests/runtime.rs`.
- [ ] Scope: 5 files, medium.

### Checkpoint 5: Context boundary

- [ ] Context baseline and snapshot survive restart exactly.
- [ ] Instruction changes reconcile only at safe boundaries.
- [ ] Initial unavailable instruction context leaves future admitted prompt retryable.
- [ ] No Effect, Bun, plugin boot, watcher, or remote-source architecture entered root crate.
- [ ] Final review checkpoint passes.

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
