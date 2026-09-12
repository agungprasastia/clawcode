# M8-04 Property-Test Design

## Goal

Exercise trust-boundary invariants from PRD without adding a fuzzing toolchain or dependency.

## Scope

Use deterministic generated inputs in integration/unit tests:

- Config/JSONC parser: malformed arbitrary inputs must return diagnostics or errors, never panic.
- `ToolCallAssembler`: accumulated arguments never exceed `MAX_TOOL_ARGUMENT_BYTES`.
- `ProviderStream` and `UiEventQueue`: coalesced stream memory remains bounded at 64 KiB.
- Workspace path policy: traversal and outside-root paths remain rejected.

## Test Strategy

A small deterministic generator produces varied byte/string inputs from fixed seeds. Tests assert invariants, not exact parser output. Existing public APIs and limits are reused. No production behavior changes except where an uncovered bound is required.

## Gate

`cargo fmt --all -- --check`, strict Clippy, `cargo test --all-targets --all-features`, and `git diff --check` must pass. Mark `P1-M8-04` complete only after all invariants pass.
