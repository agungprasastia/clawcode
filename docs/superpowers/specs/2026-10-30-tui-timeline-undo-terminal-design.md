# Clawcode Sub-Project 2: TUI Session Timeline, Workspace Undo/Redo + Diff Dialog, and Embedded Terminal Pane Design Spec

- **Date**: 2026-10-30
- **Target Reference**: OpenCode v2.1.3 (`@opencode/cli` 2.1.3)
- **Scope**: Sub-project 2 of OpenCode Behavioral & UX Parity (`docs/OPENCODE_PARITY.md`)
- **Visual Constraint**: Preserve Clawcode's existing TUI layout geometry, header banner (`CLAWCODE · [MODE]`), home mascot, rounded border styling, and color themes (`src/tui/theme.rs`, `src/tui/home.rs`).

## 1. Goals

Close the three remaining core TUI workbench gaps identified in `docs/OPENCODE_PARITY.md` plus server revert file restoration:

1. **TUI Session Timeline (`src/tui/dialogs/timeline.rs`)**:
   - Interactive turn timeline modal opened via `Ctrl+X g`, `/timeline`, or Command Palette (`Session timeline`).
   - Displays all user turns in the active session with turn index, prompt preview, assistant response summary, tool call count, modified file count, session fork lineage (`parent_id`), child fork badges, and revert markers.
   - Supports jumping the transcript scroll position to any turn (`Enter`), forking the session at a turn (`f` or `Ctrl+F`), reverting the session to a turn (`r`, `u`, or `Ctrl+R`), and live text filtering.

2. **TUI Session Undo/Redo, File Snapshot Restore & Diff Dialog (`src/tui/dialogs/diff_review.rs`, `src/conversation/tools.rs`, `src/cli/server.rs`)**:
   - Capture before/after file snapshots in tool call metadata (`filediff`) whenever `write_file` or `edit_file` mutates workspace files.
   - Implement `/undo` (and `/revert`, Timeline revert) to revert the latest turn (or selected turn), restore the user prompt into the composer, roll back modified workspace files on disk, and record the turn on a per-session redo stack.
   - Implement `/redo` (and `/unrevert`, `Ctrl+X r`) to re-apply the last reverted turn and restore its workspace file changes.
   - Wire HTTP `/api/session/:id/revert/stage` and `/api/session/:id/revert/commit` to report affected files and restore file snapshots on commit.
   - Add `/diff` (and Command Palette `View session diff`, `Ctrl+X d`) opening an interactive Session Diff Dialog with Unified and Side-by-Side (`Split`) diff modes across session file snapshots and Git working tree changes.

3. **Embedded TUI Terminal Pane (`src/tui/terminal_pane.rs`)**:
   - Dockable bottom terminal pane toggled via `Ctrl+X t`, `/terminal` (`/term`), and Command Palette (`Toggle terminal pane`).
   - Focus switching between the chat composer and terminal pane via `Ctrl+X Up`, `Ctrl+X Down`, `Esc`, or mouse click.
   - Stateful working directory (`cd`, `pwd`, `clear`, `exit`) and non-blocking command execution using `std::process::Command` + stdlib reader threads streaming stdout/stderr into a bounded 64 KiB / 500-line scrollback buffer with process interrupt support.

## 2. Architecture & Data Flow

### 2.1 Tool Call File Snapshot Metadata (`filediff`)
- In `src/conversation/tools.rs`, `write_file` and `edit_file` inspect the target file before mutation (`before_exists`, `before` UTF-8 text bounded to 256 KiB) and after mutation (`after_exists`, `after` UTF-8 text), compute line additions/deletions, and attach a `"filediff"` object to `ToolExecution.metadata`.
- SQLite `tool_calls.metadata_json` and TUI `ToolRow.metadata` automatically persist and replay this metadata.
- Helper functions `restore_filediff_before` and `restore_filediff_after` apply the snapshot atomically within the workspace root.

### 2.2 Session Timeline Dialog (`src/tui/dialogs/timeline.rs`)
- `TimelineDialogState` holds `items: Vec<TimelineTurnItem>`, `selected: usize`, `filter: String`, `search_active: bool`, `scroll_offset: usize`, and optional `parent_session: Option<(i64, String)>`.
- `App::open_timeline_dialog` builds `TimelineTurnItem` entries from durable session messages and tool calls when available, falling back to in-memory `stream_parts` and `tool_rows`.
- Actions:
  - `Enter`: scrolls chat transcript so the selected turn's `StreamPart::User` is in view and closes the dialog.
  - `f` / `Ctrl+F`: forks the session via `CommandService::fork_session_before` (or creates a branched in-memory session when running without SQLite) and switches to the new session tab.
  - `r` / `u` / `Ctrl+R`: reverts the session from the selected turn onward via `App::revert_session_to_turn`.

### 2.3 Session Undo/Redo & Diff Review Dialog (`src/tui/dialogs/diff_review.rs`)
- `App` and `ClientSessionState` maintain `reverted_turns: Vec<RevertedTurnSnapshot>`, where each entry stores the reverted user prompt, assistant text, tool rows (including `filediff` metadata), and stream parts.
- `App::undo_last_turn` (`/undo`, `/revert`) cancels any active turn, pops the latest turn, restores `before` file states on disk, stages/commits the revert in SQLite, places the user prompt into `self.prompt`, and pushes the snapshot onto `reverted_turns`.
- `App::redo_last_turn` (`/redo`, `/unrevert`, `Ctrl+X r`) pops from `reverted_turns`, re-applies `after` file states on disk, re-appends messages to SQLite and `stream_parts`, and restores the transcript.
- `DiffReviewDialogState` aggregates session `filediff` snapshots (earliest `before` to latest `after` per path) and merges uncommitted Git working tree diffs, rendering a file list + diff viewport with Unified and Side-by-Side (`Split`) toggle (`v` or `Tab`).

### 2.4 Embedded Terminal Pane (`src/tui/terminal_pane.rs`)
- `TerminalPaneState` manages:
  - `visible: bool`, `focused: bool`
  - `cwd: PathBuf`
  - `input: String`, `cursor: usize`, `history: Vec<String>`, `history_index: Option<usize>`
  - `lines: VecDeque<TerminalLine>` bounded to `MAX_TERMINAL_LINES = 500` and `MAX_TERMINAL_BYTES = 65_536`
  - `scroll_offset: u16`
  - `active_child: Arc<Mutex<Option<std::process::Child>>>`
  - `event_rx: Option<mpsc::Receiver<TerminalEvent>>`
  - `last_exit_code: Option<i32>`, `running_command: Option<String>`
- Polled each tick via `App::poll_runtime` -> `terminal_pane.poll()`.
- Rendered at the bottom of `workspace_area` in `src/tui/render.rs` when `terminal_pane.visible` is true.
