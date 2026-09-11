# M7 Conversation UX Design

## Goal

Connect the existing TUI, provider contract, persistence, command service, and workspace safety flow into one bounded conversation experience.

## Scope

M7 delivers four slices:

1. Conversation runtime: submit prompt, stream normalized provider events, render transcript, persist assembled messages, and cancel safely.
2. Conversation commands: `/new`, `/sessions`, `/exit`, mode switching, provider connect, and model selection; existing model discovery commands remain non-blocking.
3. Tool lifecycle and review: expose tool start/progress/result, workspace diff review, and BUILD approval without allowing PLAN mutation.
4. Metrics and scripted UX gate: show turn/provider metrics and validate representative PTY flows.

## Architecture

`ConversationRuntime` owns turn orchestration. It accepts a prompt and selected provider/model, invokes the existing `Provider` trait, maps `StreamEvent` values into bounded `UiEvent` values, and emits one assembled assistant message to the async persistence writer after terminal completion. It does not own rendering or direct filesystem mutation.

`App` owns presentation state: mode, selected session/provider/model, prompt, transcript, active turn state, tool lifecycle state, diff-review state, metrics, and pending cancellation. The TUI event loop drains priority quit/cancel events before ordinary events and renders only bounded state.

`CommandService` parses and executes slash commands. Session commands use `Db` and the async writer boundary; provider/model commands reuse existing discovery and registry APIs. Command results become UI events/state changes rather than direct terminal output during an interactive session.

Workspace operations remain behind `Workspace`. Tool lifecycle requests carry operation metadata and return a reviewable diff. BUILD mutation requires policy approval; PLAN rejects mutation and mutative shell operations before apply.

## Data flow

```text
prompt
  -> command/runtime dispatcher
  -> Provider::stream(StreamRequest)
  -> bounded StreamEvent channel
  -> ConversationRuntime
  -> bounded UiEvent queue
  -> App state + renderer
  -> assembled message -> async persistence writer
```

Cancellation has priority over normal input and must reach the provider stream. A completed cancellation produces terminal `StreamEvent::Cancelled` and no partial assistant row. Provider errors become diagnostics and end the turn without corrupting existing transcript state.

## Commands and state

Interactive commands:

- `/new`: create and select bounded-title session.
- `/sessions`: open/list selectable sessions.
- `/connect`: select/connect configured provider.
- `/models`: list cached models for selected provider.
- `/models refresh`: start non-blocking refresh.
- `/exit`: terminate cleanly.
- mode switch: select PLAN or BUILD; PLAN visibly disables mutation actions.

Unknown or malformed commands produce actionable diagnostics and do not start provider work.

## Tool and diff review

Tool lifecycle states are explicit: requested, running, completed, failed, cancelled, or awaiting approval. Read-only tools may execute in PLAN when within workspace boundary. Mutation and mutative shell requests in BUILD pass the existing transaction order: validate, snapshot, diff, policy decision, approval, apply. Diff review precedes apply. Rejected or cancelled approval leaves workspace unchanged.

## Metrics

Turn state records TTFT, total duration, finish status, input/output usage when provider supplies usage, and provider/model identity. Metrics are bounded and render-safe. Missing provider usage remains unavailable rather than fabricated.

## Error handling and limits

- UI event queue and coalesced deltas retain existing bounded limits.
- Prompt, tool arguments, transcript, diagnostics, and persisted messages enforce existing size limits.
- Provider failure is isolated to current turn.
- Persistence failure is surfaced as diagnostic; successful provider output remains visible.
- No network discovery runs on startup critical path.
- No streaming delta is persisted as an individual database row.

## Testing and acceptance

Add focused unit tests for runtime event mapping, cancellation, assembled-message persistence, command parsing/state transitions, session switching, mode safety, tool lifecycle, diff approval, and metrics. Add scripted TUI/PTY coverage for `/new`, `/sessions`, `/connect`, `/models`, PLAN, BUILD, cancel, and `/exit`.

M7 passes when all tests and project gates pass, scripted flows complete without hangs, PLAN cannot mutate, BUILD shows diff before apply, cancellation produces terminal `Cancelled`, and the render loop does not block on provider or SQLite work.

## Explicit non-goals

No new provider protocol, daemon, ACP, MCP, plugin system, OAuth flow, desktop UI, or durable multi-file transaction semantics.
