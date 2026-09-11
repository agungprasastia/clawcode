# M8-01 Notification Implementation Plan

**Goal:** Add best-effort terminal, desktop, and sound notifications with isolated failures and no impact on core results.

**Architecture:** `notify` owns payload bounds, backend trait, deterministic dispatch, and platform adapters. TUI/conversation integration calls a non-blocking notifier after terminal turn state changes. Notification errors remain diagnostics/tracing only.

## Task 1: Notification contracts and fake backends

**Files:** `src/notify/mod.rs`, `tests/notify.rs`

- Add bounded `Notification` and `NotificationKind`.
- Add `Notifier` and `NotificationReport`.
- Add deterministic fan-out dispatch with per-backend failure isolation.
- Add fake backend tests for success/failure/disabled/bounds.
- Run focused tests; commit `feat: add notification contracts`.

## Task 2: Platform adapters

**Files:** `src/notify/platform.rs`, `src/notify/mod.rs`, `tests/notify.rs`

- Implement terminal bell through `crossterm` writer.
- Implement desktop/sound adapters with platform-specific code isolated in `platform.rs`.
- Avoid external native dependencies; unsupported adapters return report status.
- Test platform-independent behavior using fake backends.
- Run fmt/clippy/tests; commit `feat: add notification platform adapters`.

## Task 3: Conversation/TUI integration

**Files:** `src/tui/app.rs`, `src/tui/mod.rs`, `tests/notify.rs` or `tests/tui_conversation.rs`

- Inject optional notifier into lifecycle boundary, not render code.
- Notify only after terminal success/error/cancel state is applied.
- Ensure notification failure cannot change status, metrics, persistence, or command result.
- Keep startup path local and non-blocking.
- Add integration test for successful result with failing notifier.
- Update `docs/TODO.md` M8-01.
- Run all gates and commit `feat: complete notification milestone`.
