# Clawcode Session Runtime Plan

## Review gate

Planning artifacts only. No implementation starts until this plan and `tasks/todo.md` are reviewed and approved. Approval must cover first-slice identity, transaction boundaries, phase order, open questions, and deferred scope. Until approval, do not edit Rust source, migrations, tests, or configuration.

## Overview

First vertical slice follows the repository roadmap and OpenCode v2's next reviewed runner slice:

```text
one provider turn
  -> complete local tool call
  -> durable assistant tool-call message with exact message ID
  -> local tool side effect in std::thread
  -> join
  -> durable tool settlement
  -> reload projected history
```

The first slice is **not** `session_input` prompt admission. Prompt admission remains a later phase. This slice hardens the existing `src/runtime/client.rs` tool seam before changing TUI admission or adding a coordinator.

Clawcode remains a Rust synchronous binary. Use `std::thread`, `std::sync::mpsc`, `Mutex`, SQLite WAL, and `WriterHandle`. Keep existing `RuntimeClient`, `ClientCommand`, `EventBus`, `generation_events`, `ProviderStream`, workspace tool execution, and TUI loop. OpenCode references supply behavior vocabulary and boundaries only, not Effect, Bun, HTTP, SDK, or service-container architecture.

## Grounded current state

- `src/runtime/client.rs::run_generation` streams provider events, assembles tool calls, persists an assistant text message before executing each call, executes tools inline, appends tool messages in memory, and may issue more provider turns.
- Existing assistant message persistence through `WriterHandle` returns only `Result<(), String>`, so runtime cannot prove the durable `messages.id` before a tool side effect.
- Existing `generation_id` is a generation lifecycle identifier. It must remain correlation metadata, never tool invocation identity.
- `src/persistence/db.rs` has `messages`, `generations`, and append-only per-session `generation_events`; `Db::append_message` can return `Message` with its SQLite ID.
- `src/persistence/writer.rs` batches message and event appends but has no message-ID return path or atomic tool-call settlement operation.
- `src/conversation/tools.rs` already exposes bounded local tool behavior used by the runtime.
- Provider streaming is synchronous. `ProviderStream` has cancellation support. The current runtime uses a generation thread, but tool execution itself is not isolated and joined as a dedicated local execution step.
- `src/tui/app.rs::submit_user_prompt` still appends user messages directly. That remains a later prompt-admission cutover, not part of this first slice.

## OpenCode references used as semantic input

- `opencode/specs/v2/todo.md`: next reviewed slice requires each complete local tool call to be durably recorded before child execution, child execution to start immediately, every settlement to be awaited after provider-stream closure, and projected history to reload once before continuation. It also keeps provider-attempt recovery, backpressure hardening, and broad continuation separate.
- `opencode/specs/v2/session.md`: projected tool identity belongs to the owning assistant message; prompt admission and execution are separate later responsibilities; local execution coordination is process-local.
- `opencode/CONTEXT.md`: Context Epoch, Context Source, baseline, snapshot, and safe provider-turn boundary vocabulary for a later phase.
- `opencode/specs/v2/tools.md`: local invocation context and durable identity principles. Clawcode uses existing Rust tool APIs, not the Effect registry.
- `opencode/specs/v2/provider-policy.md`: provider configuration and provider-use policy stay separate. No policy layer enters this slice.
- `opencode/specs/v2/instructions.md`: instruction discovery and persistence belong to a narrow instruction boundary. No plugin boot or Effect service graph enters this repository.

## Architecture decisions

1. **First slice is durable local-tool settlement.** Do not make `session_input` the first implementation. Prove tool-call identity and settlement against current runtime seams first.
2. **Assistant message ID is mandatory before side effect.** Persist the assistant message that owns the tool call, wait for its committed `messages.id`, then create the durable tool-call record and only then execute the tool. `generation_id` may be included for correlation but cannot substitute for `assistant_message_id`.
3. **One provider request, one local tool call, one settlement.** The first slice handles one complete local call from one provider stream. It does not start a second provider request. Multiple calls, continuation policy, and tool-call limits remain explicit follow-ups.
4. **Use a real child thread and join.** Execute the local tool in `std::thread`; join it after provider stream closure; persist settlement only from the joined result. No async executor or detached side-effect thread.
5. **Use writer-owned atomic boundaries.** Writer APIs must return durable message IDs and atomically publish tool-call lifecycle and settlement facts. No event may claim an identity before the corresponding row is committed.
6. **Reload from durable projection once.** After settlement, reload projected messages/history from SQLite before any future continuation decision. In this slice, reload proves correctness and then the generation ends without another provider call.
7. **Keep durable events and live events distinct.** Existing `generation_events` remains source of truth. Live-only control events keep `seq = 0`; durable event payloads carry `assistant_message_id` where tool identity matters.
8. **Preserve root concurrency rules.** Only `std::thread`, bounded `mpsc`, and `Mutex`; SQLite WAL and `WriterHandle`; no Effect, Bun, Tokio, HTTP server, SDK, or copied OpenCode architecture.
9. **Later phases stay separate.** Prompt admission, per-session coordinator/wake/interrupt, replay cursor, and Context Epoch/instruction source start only after the first settlement checkpoint.

## Ordered phases and tasks

### Phase 0: Review gate

No source work starts until plan review approval is recorded. The first implementation target is Task 1 below, not `session_input`.

### Phase 1: First vertical slice, durable local-tool settlement

#### Task 1: Define durable tool-call identity contract

**Description:** Specify the Rust and persistence contract for one local tool call. The owning assistant message must be durable first and expose its exact `messages.id`. The durable call record and every related runtime event must carry `session_id`, `generation_id` for correlation, `assistant_message_id` for ownership, provider `call_id`, tool name, and bounded arguments. Define lifecycle states and make `assistant_message_id` non-optional for local calls.

**Acceptance criteria:**
- [ ] Contract distinguishes `assistant_message_id` from `generation_id`; tool side effects require the former.
- [ ] One provider `call_id` can be correlated to exactly one owning assistant message within a generation.
- [ ] Lifecycle states cover durable call creation, running, completed, failed, and cancelled without inventing crash recovery.

**Focused verification:** Review the contract against current `Generation`, `Message`, `RuntimeEvent`, provider tool-call events, and workspace tool entry points. Add no source code during this planning gate.

**Dependencies:** Plan review approval.

**Likely files:**
- `src/persistence/db.rs`
- `src/runtime/client.rs`
- `src/runtime/mod.rs`
- `src/conversation/tools.rs`
- `tests/runtime.rs`

**Scope:** 5 files, medium.

#### Task 2: Add persistence and writer atomic call-settlement support

**Description:** Add the smallest additive schema/API needed to persist tool-call ownership and settlement. Extend writer operations to return the committed assistant message ID. Add an atomic operation for durable call creation and an atomic settlement update plus durable event append. Bound arguments and output using existing repository limits. Keep SQLite writes serialized through `WriterHandle`.

**Acceptance criteria:**
- [ ] Assistant tool-call message insertion returns its committed `messages.id` before the call record can enter `running`.
- [ ] Settlement atomically records status, bounded result/error, owning assistant message ID, and durable event sequence.
- [ ] Transaction failure leaves no successful settlement or false completion event.

**Focused verification:** Run focused persistence tests after implementation for returned message IDs, v2-to-current migration, atomic success/failure, ownership foreign keys or equivalent checks, and output limits. No project-wide test run in this task.

**Dependencies:** Task 1.

**Likely files:**
- `src/persistence/schema.rs`
- `src/persistence/db.rs`
- `src/persistence/writer.rs`
- `src/persistence/mod.rs`
- `tests/persistence.rs`

**Scope:** 5 files, medium.

#### Task 3: Run one-call settlement with thread, join, and durable reload

**Description:** Refactor the existing runtime tool branch. After provider stream closure and complete call assembly, durably insert the assistant tool-call message and obtain its exact message ID. Persist the call identity before side effect. Spawn one `std::thread` for existing local tool execution, join it, settle success or failure atomically, then reload projected history once. End generation after this one provider request and one local call; do not silently issue continuation.

**Acceptance criteria:**
- [ ] Tool side effect starts only after committed assistant message ID and durable call identity exist; all events carry `assistant_message_id`, never generation ID as a substitute.
- [ ] Runtime joins child execution, persists exactly one settlement, reloads durable history once, and leaves no detached worker.
- [ ] Provider call count is one; tool failure, cancellation, and panic-safe join paths produce explicit durable settlement status without claiming success.

**Focused verification:** Use a fake provider and deterministic tool/filesystem seam. Assert message ID exists before side-effect latch, child join completes, one provider request occurs, settlement is durable, and reloaded history contains the expected assistant/tool projection.

**Dependencies:** Task 2.

**Likely files:**
- `src/runtime/client.rs`
- `src/conversation/tools.rs`
- `src/persistence/db.rs`
- `src/persistence/writer.rs`
- `tests/runtime.rs`

**Scope:** 5 files, medium.

#### Task 4: Add focused first-slice regression tests

**Description:** Lock the behavioral contract with narrow tests, not broad architecture tests. Cover exact assistant identity, durable call ordering, joined execution, settlement failure, cancellation, one provider request, and one reload. Keep tests deterministic and local; do not touch network or live workspace state.

**Acceptance criteria:**
- [ ] Test proves side-effect code observes a committed assistant `messages.id` and fails if only `generation_id` is supplied.
- [ ] Test proves success and failure settlements are atomic and linked to the exact assistant message.
- [ ] Test proves one provider request, one joined local call, one settlement, and one durable reload path.

**Focused verification:** Run only the focused persistence/runtime test targets selected for this slice. Do not run full suite, formatter, linter, or build as part of this task.

**Dependencies:** Task 3.

**Likely files:**
- `tests/persistence.rs`
- `tests/runtime.rs`
- `src/runtime/client.rs`

**Scope:** 3 files, small to medium.

### Checkpoint 1: First vertical slice complete

- [ ] Plan review approval was recorded before source edits.
- [ ] Exact durable assistant message ID exists before every local tool side effect.
- [ ] `generation_id` is correlation only, never tool identity.
- [ ] One provider request produces one local call, one joined execution, one settlement, and one durable reload.
- [ ] Focused persistence and runtime checks pass.
- [ ] No prompt-admission, coordinator, replay-cursor, or Context Epoch implementation was pulled into this slice.
- [ ] Review checkpoint approves Phase 2.

### Phase 2: Durable prompt admission, later separate slice

#### Task 5: Add `session_input` admission and promotion contract

**Description:** Add an additive inbox for prompts only after local-tool settlement is stable. Admission creates a pending row and durable `prompt_admitted`; promotion appends the visible user message and `prompt_promoted` atomically. Keep delivery mode and idempotency choices explicit in review.

**Acceptance criteria:**
- [ ] Admission survives restart and does not make input model-visible.
- [ ] Promotion atomically consumes one eligible row and appends one user message plus promotion event.
- [ ] Admission errors never create partial durable input.

**Focused verification:** In-memory SQLite tests for migration, pending/promoted state, restart, ordering, limits, and atomic failure.

**Dependencies:** Checkpoint 1.

**Likely files:**
- `src/persistence/schema.rs`
- `src/persistence/db.rs`
- `src/persistence/writer.rs`
- `src/persistence/mod.rs`
- `tests/persistence.rs`

**Scope:** 5 files, medium.

#### Task 6: Route TUI prompt submission through admission

**Description:** Replace runtime-backed direct user-message append in `App::submit_user_prompt` with runtime admission and promotion events. Preserve existing TUI rendering and diagnostics; do not redesign session execution here.

**Acceptance criteria:**
- [ ] Runtime-backed TUI has no direct user-message SQLite append.
- [ ] Rejected admission is not shown as committed transcript content.
- [ ] Successful admission/promotion renders one user turn.

**Focused verification:** Targeted TUI tests for admission success, rejection, event filtering, and duplicate prevention.

**Dependencies:** Task 5.

**Likely files:**
- `src/tui/app.rs`
- `src/runtime/client.rs`
- `tests/tui.rs`

**Scope:** 3 files, medium.

### Checkpoint 2: Prompt admission

- [ ] Durable admission and promotion are proven independently from local-tool settlement.
- [ ] TUI no longer directly writes user messages in runtime-backed mode.
- [ ] Review checkpoint approves coordinator work.

### Phase 3: Per-session coordinator, wake, and interrupt

#### Task 7: Add process-local per-session coordinator and wake

**Description:** Add a small `SessionCoordinator` keyed by session ID. Serialize one local drain, allow different sessions concurrently, coalesce repeated wakes, and schedule admitted inputs at safe boundaries. Keep execution ownership process-local and non-durable.

**Acceptance criteria:**
- [ ] One session has at most one active drain and one follow-up wake.
- [ ] Different sessions can run concurrently.
- [ ] Repeated wakes do not duplicate promotions or provider calls.

**Focused verification:** Deterministic fake-provider tests for serialization, cross-session overlap, wake coalescing, and active registry cleanup.

**Dependencies:** Checkpoint 2.

**Likely files:**
- `src/runtime/coordinator.rs`
- `src/runtime/client.rs`
- `src/runtime/mod.rs`
- `src/persistence/db.rs`
- `tests/runtime.rs`

**Scope:** 5 files, medium.

#### Task 8: Add process-local interrupt cleanup

**Description:** Route interrupt through the coordinator. Cancel current provider stream, join and unregister local drain, clear coalesced follow-up wake, and preserve pending durable inputs. Do not infer crash recovery or retry safety.

**Acceptance criteria:**
- [ ] Active interrupt emits one terminal cancellation and leaves coordinator idle.
- [ ] Idle or unknown interrupt is idempotent and preserves pending input.
- [ ] Wake during cleanup cannot start a second drain.

**Focused verification:** Blocking fake-provider cancellation test covering cleanup, pending input, stale registry, and duplicate prevention.

**Dependencies:** Task 7.

**Likely files:**
- `src/runtime/coordinator.rs`
- `src/runtime/client.rs`
- `src/persistence/db.rs`
- `tests/runtime.rs`

**Scope:** 4 files, medium.

### Checkpoint 3: Local session control

- [ ] Per-session serialization, cross-session concurrency, wake coalescing, and interrupt cleanup pass focused checks.
- [ ] `active()` remains process-local.
- [ ] Crash recovery, retry, and clustered ownership remain absent.

### Phase 4: Durable event replay cursor

#### Task 9: Add replay-plus-tail session cursor

**Description:** Expose per-session exclusive `after_seq` replay over existing `generation_events`. Register live subscription before history read, deduplicate durable sequences, and keep `seq = 0` controls out of cursor progress.

**Acceptance criteria:**
- [ ] Cursor `N` returns every durable event with `seq > N` in ascending order, including handoff races.
- [ ] Live-only control events never advance cursor.
- [ ] Queue closure recovers by replaying from last committed sequence.

**Focused verification:** SQLite WAL race, cursor exclusivity, `seq = 0` filtering, deduplication, and bounded subscriber recovery tests.

**Dependencies:** Checkpoint 3.

**Likely files:**
- `src/runtime/mod.rs`
- `src/runtime/client.rs`
- `src/persistence/db.rs`
- `src/tui/app.rs`
- `tests/runtime.rs`

**Scope:** 5 files, medium.

#### Task 10: Make TUI restore cursor-driven

**Description:** Restore session display from durable events after `ClientSessionState::loaded_until_seq`; ignore ephemeral deltas during replay and preserve existing transcript bounds.

**Acceptance criteria:**
- [ ] Reconnect and session switch do not duplicate durable events.
- [ ] Disconnected durable assistant/tool events replay; live-only fragments do not become history.
- [ ] Cursor never advances on sequence zero or failed durable commit.

**Focused verification:** Seeded TUI event-log replay tests for reconnect, transcript output, cursor values, and duplicate suppression.

**Dependencies:** Task 9.

**Likely files:**
- `src/tui/app.rs`
- `src/tui/mod.rs`
- `tests/tui.rs`

**Scope:** 3 files, medium.

### Checkpoint 4: Replay contract

- [ ] Replay/live handoff race passes.
- [ ] TUI restore uses durable cursor without treating fragments as history.
- [ ] Review checkpoint approves Context Epoch work.

### Phase 5: Context Epoch and instruction source

#### Task 11: Add durable Context Epoch boundary

**Description:** Persist active epoch ID, exact baseline system text, and JSON source snapshot. Initialize before future prompt promotion; reconcile source changes after promotion and before a provider request as one chronological system message plus snapshot update.

**Acceptance criteria:**
- [ ] First epoch baseline is exact and reusable after restart.
- [ ] Unavailable initial context blocks future promotion; unchanged source emits nothing; changed source advances snapshot atomically with one system message.
- [ ] Epoch replacement remains out of this task.

**Focused verification:** In-memory tests for initialization, unavailable/unchanged/changed source, atomic update, restart reuse, and request assembly.

**Dependencies:** Checkpoint 4.

**Likely files:**
- `src/persistence/schema.rs`
- `src/persistence/db.rs`
- `src/runtime/client.rs`
- `src/conversation/context.rs`
- `tests/runtime.rs`

**Scope:** 5 files, medium.

#### Task 12: Add first `InstructionSource`

**Description:** Read bounded `AGENTS.md` files from workspace root and parent chain in deterministic order. Feed one aggregate source into existing `SystemPromptComposer` and Context Epoch. Keep global, URL, remote, watcher, and plugin sources deferred.

**Acceptance criteria:**
- [ ] Same files produce stable source key, snapshot, and baseline bytes.
- [ ] Missing file is valid absence; read failure is unavailable and does not erase prior value.
- [ ] Changes appear only at the next safe provider boundary as one aggregate update.

**Focused verification:** Temporary-workspace tests for ordering, missing files, size limits, read failure, changes, and provider messages.

**Dependencies:** Task 11.

**Likely files:**
- `src/conversation/context.rs`
- `src/conversation/prompt.rs`
- `src/runtime/client.rs`
- `src/persistence/db.rs`
- `tests/runtime.rs`

**Scope:** 5 files, medium.

### Checkpoint 5: Context boundary

- [ ] Context baseline and snapshot survive restart exactly.
- [ ] Instruction changes reconcile only at safe boundaries.
- [ ] Initial unavailable instruction context leaves future admitted prompt retryable.
- [ ] No Effect, Bun, plugin boot, watcher, or remote-source architecture entered root crate.
- [ ] Final review checkpoint passes.

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Runtime uses generation ID as tool identity | High | Make committed assistant `messages.id` a required argument to call creation and settlement; test side-effect latch before execution. |
| Assistant message row and tool-call row commit separately | High | Writer transaction returns message ID first, then atomically creates call identity and durable lifecycle event; settlement has its own atomic update/event boundary. |
| Side effect runs before durable identity | High | Do not spawn child thread until assistant message and call record acknowledgements arrive. |
| Detached or panicked tool thread | High | Spawn with `std::thread`, catch panic into failed settlement, always `join`, never detach. |
| Tool output exceeds storage bound | Medium | Reuse existing bounded output policy; settlement failure remains explicit and cannot claim successful full output. |
| Existing multi-turn loop silently continues | High | First-slice runtime test asserts provider call count equals one; continuation is explicitly deferred. |
| Writer and runtime DB connections observe stale data | High | Await writer acknowledgements before tool execution and reload; retain SQLite WAL and busy timeout. |
| Prompt admission scope leaks into first slice | Medium | Keep `session_input` in Phase 2 and checkpoint it separately. |
| Event cursor confuses live controls with durable rows | High | Keep `seq = 0` non-advancing and replay only committed positive/current durable sequences. |
| Context source blocks valid prompts on transient read failure | Medium | Distinguish initial unavailable from successful absence and stale prior value; keep recovery policy explicit. |
| OpenCode architecture gets copied | High | Review every addition against root constraints; reject Effect/Bun/HTTP/plugin-container designs. |

## Open questions

1. **Tool-call storage shape:** Use dedicated `tool_calls` plus settlement columns, or one call table with status and result? Recommendation: one durable call identity row with settlement fields if it preserves atomic transitions and clear ownership; avoid a second table unless history queries require it.
2. **Multiple provider tool calls:** First slice handles one complete local call. Decide later whether same-stream multiple calls settle eagerly in parallel or sequentially; do not broaden Task 3.
3. **Tool-call ID reuse:** Provider `call_id` may repeat across turns. Recommendation: uniqueness is scoped by assistant message ID, not generation ID alone.
4. **Prompt admission idempotency:** Decide when a non-TUI caller exists. It is not part of first slice.
5. **Settlement retry after process loss:** Deferred. A durable `running` call must not be retried automatically without an explicit recovery policy.
6. **Global instruction path and precedence:** Task 12 starts with workspace root and parent `AGENTS.md`; settle user-global paths separately.
7. **Context Epoch replacement:** Compaction and workspace movement remain deferred; this plan only initializes and reconciles one epoch.

## Explicit deferred scope

- `session_input` is not first slice; prompt admission and TUI cutover begin only after durable local-tool settlement checkpoint.
- Effect, Bun, Tokio, async runtime, fibers, HTTP server, generated SDK, and OpenCode service/container architecture.
- Durable crash recovery, ambiguous provider-dispatch retry, backoff, retry budgets, stale-owner fencing, clustered execution, and automatic settlement retry.
- More than one provider request in first slice, multi-call policy, tool continuation, automatic/manual compaction, provider context-window recovery, native continuation metadata, and provider-specific pruning.
- Background jobs, subagents, MCP, plugin tools, permission registry redesign, and remote tool execution.
- Global, URL, remote, nested watcher-backed, plugin-defined, and hot-reloadable instruction sources.
- Provider-use policy implementation. Existing provider configuration and `ConfiguredRouter` remain authoritative.
- Cross-session/global event aggregation; replay cursor remains one session at a time.
- Durable execution identity for local drains; coordinator remains process-local.
- Broad inbox growth controls beyond existing bounded channels and message limits.
- Production rollout, migration cleanup, formatting, linting, and project-wide validation before plan approval and implementation completion.

## Completion definition

The plan is complete when the first checkpoint proves exact assistant message identity before tool side effect, atomic durable settlement, one joined local call, one provider request, and one durable reload. Later checkpoints then prove prompt admission, local session control, replay cursor, and Context Epoch/instruction behavior independently. All source implementation waits for explicit plan approval.
