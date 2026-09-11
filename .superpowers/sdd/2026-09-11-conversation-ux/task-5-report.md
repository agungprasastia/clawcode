# Task 5 report

Status: fixed review findings.

Implemented bounded turn metrics and persistence integration:

- Removed false TTFT metric; duration is reported only because provider contract has no first-output event boundary.
- Runtime metrics propagate through `ConversationEvent::Metrics` into `App`; render uses cached metrics only.
- Provider/model identity values are bounded to 256 bytes at runtime and TUI boundaries.
- Added `run_and_persist`; writes one assembled assistant message only for `Stop`, `Length`, or `ToolCall`.
- `FinishReason::Error`, cancellation, and incomplete streams never persist assistant output.
- No stream delta rows are written.
- Exposed cached metrics through `App`; render formats cached values only. No DB/provider work in render.
- Added regression tests for error finish, cancellation, and App metrics propagation.
- TurnState now gives Error and Cancelled terminal precedence over later Finish events; added TextDelta+Error+Finish and TextDelta+Cancelled+Finish persistence regressions.
- Fixed remaining review finding: `App` now bounds/sanitizes provider and model identities when receiving `ConversationEvent::Metrics`, with UTF-8-safe truncation and unchanged duration, usage, and finish values. Added direct UI regression coverage.

Gates passed:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- `git diff --check`

Concern: exact first-output timing remains unavailable without changing provider contract; no fake TTFT is exposed.
Concern: none.
