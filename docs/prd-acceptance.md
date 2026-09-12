# PRD Acceptance Checklist

Date: 2026-09-12
Status: **BLOCKED — not release-ready**

Checklist follows `docs/PRD.md` acceptance criteria. An item is complete only when its evidence is explicit and current.

| PRD criterion | Evidence | Status |
|---|---|---|
| TUI warm/local startup ≤100 ms; no discovery on critical path | `benches/first_frame.rs`, `benches/performance.rs`, `docs/performance-baseline.md`; benchmark is local and network-free | Pass — baseline document now uses `1.36 ms` consistently. |
| PLAN does not change filesystem | `tests/workspace_policy.rs`: `plan_mutation_is_denied_without_change`, `plan_read_only_and_cancellation_are_isolated`; `src/workspace/policy.rs` | Pass |
| BUILD creates snapshot and diff before apply | `tests/conversation_tools.rs`: `build_diff_precedes_approval_and_rejection_preserves_workspace`; transactional implementation in `src/workspace/mod.rs` | Pass |
| Cancellation always produces `Cancelled` and stops request/tool within bounds | `tests/provider_contract.rs`, `tests/conversation_runtime.rs`, `tests/ui_conversation.rs` | Pass |
| Provider failure independent; cached/static models remain selectable | `tests/adapters.rs`, `tests/provider_contract.rs`, provider discovery tests for stale cache/backoff | Pass |
| Config diagnostics identify error location | `tests/config.rs`: malformed and unknown-field location tests; `docs/diagnostics.md` | Pass |
| Render loop not blocked by SQLite/network/notification | persistence async writer tests, discovery non-blocking tests, notification isolation tests, `benches/performance.rs` | Pass |
| Retention and size limits tested | `tests/persistence.rs`, `tests/conversation_runtime.rs`, `tests/m8_property.rs`, snapshot limit tests | Pass |
| Windows/Linux/macOS build/test matrix runs | `.github/workflows/release.yml` | Pass — checksum step uses `sha256sum` or macOS-compatible `shasum -a 256`. |

## Required release blockers

All listed evidence blockers are resolved. Final validation must still be rerun after these documentation and workflow changes.

## Current validation evidence

Latest local Windows validation passed:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `cargo test --all-targets --all-features --locked -j 1`
- `cargo run --quiet -- --version`
- Windows package smoke test and same-environment SHA-256 comparison
- `git diff --check`

Local Windows evidence does not prove Linux/macOS workflow execution. The workflow is configured for all three runners; release publication still requires a successful CI run.
