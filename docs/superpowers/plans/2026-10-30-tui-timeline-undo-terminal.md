# TUI Session Timeline, Workspace Undo/Redo + Diff Dialog, and Embedded Terminal Pane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement OpenCode v2.1.3 parity for the TUI Session Timeline (`Ctrl+X g`, `/timeline`), Session Undo/Redo + File Snapshot Restore + Diff Review Dialog (`/undo`, `/redo`, `Ctrl+X r`, `/diff`, HTTP revert file restore), and Embedded Terminal Pane (`Ctrl+X t`, `/terminal`, `Ctrl+X Up/Down`) without altering Clawcode's visual identity.

**Architecture:** Extend `write_file`/`edit_file` tool execution to emit `filediff` before/after snapshots in `metadata_json`, expose `fork_session`, `stage_revert`, `clear_revert`, `commit_revert`, and `session_summaries` on `CommandService` and `RuntimeClient`, add `timeline` and `diff_review` dialog modals under `src/tui/dialogs/`, and add a non-blocking stdlib-threaded `TerminalPaneState` under `src/tui/terminal_pane.rs`.

**Tech Stack:** Rust 1.98.0 (Edition 2024), Ratatui, Crossterm, rusqlite, `std::process::Command`, `std::sync::mpsc`.

---

### Task 1: Tool `filediff` Metadata, Persistence Helpers & Server File Restore on Revert

**Files:**
- Modify: `src/conversation/tools.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/runtime/client.rs`
- Modify: `src/cli/server.rs`

- [ ] Capture `before_exists`, `before`, `after_exists`, `after`, `additions`, and `deletions` in `write_file` and `edit_file` (`src/conversation/tools.rs`) and attach `"filediff"` to `*metadata`.
- [ ] Add ` restore_filediff_snapshot` helper to apply `before` or `after` file states safely inside a workspace directory.
- [ ] Expose `fork_session_before`, `stage_revert`, `clear_revert`, `commit_revert`, and `session_summaries` on `CommandService` (`src/cli/mod.rs`) and `RuntimeClient` (`src/runtime/client.rs`).
- [ ] Update `stage_revert` and `commit_revert` in `src/cli/server.rs` to report modified file paths and restore `before` file snapshots from `tool_calls.metadata_json` on commit.

### Task 2: TUI Session Timeline Dialog (`src/tui/dialogs/timeline.rs`)

**Files:**
- Create: `src/tui/dialogs/timeline.rs`
- Modify: `src/tui/dialogs/mod.rs`
- Modify: `src/tui/dialogs/palette.rs`
- Modify: `src/tui/app/suggestions.rs`
- Modify: `src/tui/app/dialogs.rs`
- Modify: `src/tui/render.rs`

- [ ] Create `TimelineTurnItem`, `TimelineDialogState`, and `render_timeline_dialog` in `src/tui/dialogs/timeline.rs`.
- [ ] Wire `Ctrl+X g`, `/timeline`, and `PaletteAction::SessionTimeline` to `App::open_timeline_dialog`.
- [ ] Support `Enter` (jump to turn), `f` / `Ctrl+F` (fork session at turn and switch tab), `r` / `u` / `Ctrl+R` (revert session from turn), and search filtering.

### Task 3: TUI Session Undo/Redo & Diff Review Dialog (`src/tui/dialogs/diff_review.rs`)

**Files:**
- Create: `src/tui/dialogs/diff_review.rs`
- Modify: `src/tui/dialogs/mod.rs`
- Modify: `src/tui/dialogs/palette.rs`
- Modify: `src/tui/app/mod.rs`
- Modify: `src/tui/app/session_sync.rs`
- Modify: `src/tui/app/dialogs.rs`
- Modify: `src/tui/app/input.rs`
- Modify: `src/tui/render.rs`

- [ ] Create `SessionFileDiff`, `DiffViewMode` (`Unified`, `Split`), `DiffReviewDialogState`, and `render_diff_review_dialog` in `src/tui/dialogs/diff_review.rs`.
- [ ] Implement `App::undo_last_turn`, `App::revert_session_to_turn`, and `App::redo_last_turn` with `RevertedTurnSnapshot` stack per session and disk snapshot restoration.
- [ ] Wire `/undo`, `/revert`, `/redo`, `/unrevert`, `/diff`, `Ctrl+X r`, `Ctrl+X d`, and Palette actions (`UndoLastTurn`, `RedoLastTurn`, `ViewSessionDiff`).

### Task 4: Embedded TUI Terminal Pane (`src/tui/terminal_pane.rs`)

**Files:**
- Create: `src/tui/terminal_pane.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app/mod.rs`
- Modify: `src/tui/app/dialogs.rs`
- Modify: `src/tui/app/input.rs`
- Modify: `src/tui/app/runtime_bridge.rs`
- Modify: `src/tui/render.rs`

- [ ] Implement `TerminalPaneState` and `render_terminal_pane` in `src/tui/terminal_pane.rs` with bounded scrollback, built-in `cd`/`clear`/`pwd`/`exit`, background command execution with live line streaming, and `Ctrl+C` process termination.
- [ ] Wire `Ctrl+X t`, `Ctrl+X Up`, `Ctrl+X Down`, `/terminal`, `/term`, and `PaletteAction::ToggleTerminalPane`.
- [ ] Route keyboard and mouse scroll events to the terminal pane when focused while keeping global shortcuts (`Input::Leader`, `Input::Palette`, `Input::CycleAgent`, `Esc`) responsive.

### Task 5: Comprehensive Tests, Parity Doc Update & CI Verification

**Files:**
- Modify: `tests/tui.rs`
- Modify: `docs/OPENCODE_PARITY.md`

- [ ] Add integration tests in `tests/tui.rs` covering Session Timeline (jump, filter, fork, revert), Session Undo/Redo + file restoration + `/diff` dialog (Unified & Split), and Embedded Terminal Pane (toggle, focus, command execution, `cd`, `clear`, scrollback).
- [ ] Update `docs/OPENCODE_PARITY.md` status rows for `TUI terminal pane`, `TUI session timeline`, `TUI undo/redo`, and `Server and HTTP API`.
- [ ] Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, and `cargo test --all-targets --all-features --locked`.
