# Sub-Project 1: TUI Streaming, Transcript UX, & Settings Parity Design

- **Date**: 2026-10-23
- **Target Reference**: OpenCode CLI v2.1.3 (`@opencode/cli` standalone binary, SHA-256 `59cfe4e54c06778ec501a4566690d4ce0e8e1b45cf4bb3b9d3e49f5c8601f104`)
- **REA Evidence**:
  - `ev_eed620c2e37069f3206fd7ee582ca2a20b8773ec14e94d7171a314e3eb963090` (PE version/resources)
  - `ev_4ad92efb893c5c903bfbaa402decbd42ec75636d5ec0947593bf4ce66ee2555b` (PE `.bun` layout summary)
  - `ev_33cf25d95b7d8f2d1d997460e46ca44bce0906e7e2dfed0e7bd883ee2fecab57` (`opencode.js` entrypoint module graph)
  - `ev_ebf6fef4196f24a01345f595560aa91cb497eb01f9d14266022ea73ada084088` (`chunk-y7zg1ax8.js`, `chunk-yefbqffp.js`, `chunk-zn6m6fcp.js`, `chunk-xbw999b3.js`)
  - `ev_229a673446727054d973e2421d0cf6967fd186cbd54bc579afdd6d329502b50c` (`chunk-s68yxfjh.js`, `chunk-wmrptpbf.js`, `chunk-jy9gzhth.js`, `chunk-0y4s5x8e.js`)

## 1. Goal & Non-Goals

### Goal
Deliver 100% behavioral, streaming, transcript UX, settings, prompt-queue/steer, blocking-tool backgrounding (`Ctrl+B`), and model variant/favorites/recent parity with OpenCode v2.1.3 inside Clawcode's synchronous Rust architecture.

### Strict Non-Goals & Constraints
1. **Preserve Clawcode's Current Visual Design**: Never alter Clawcode's existing layout geometry, header banner (`CLAWCODE · [MODE]`), home mascot, palette colors, or rounded dialog border aesthetic in `src/tui/render.rs`, `src/tui/home.rs`, `src/tui/chat.rs`, and `src/tui/theme.rs`.
2. **Synchronous Rust Only**: Use `std::thread`, `std::sync::{Arc, Mutex}`, and `std::sync::mpsc` exclusively. Zero `tokio` or `async fn`.
3. **Strict Linting & Bounds**: Pass `cargo clippy --all-targets --all-features --locked -- -D warnings` without `#[allow(...)]`. Bound all buffers on valid UTF-8 character boundaries.
4. **Anti-Slop Copy & Code Hygiene**: Zero em dashes (U+2014) in any UI copy, CLI output, code comments, or documentation. No dead controls or unimplemented stubs.

---

## 2. Configuration (`/settings`) & SQLite Dynamic State (`kv_state`)

### 2.1 `TuiSettingsConfig` in `src/config/mod.rs`
Extend `Config` with `tui: TuiSettingsConfig`, accepting both top-level OpenCode v2.1.3 `cli.json` keys (`animations`, `session.*`, `scroll.*`, `mouse`, `leader.*`, `debug.*`) and nested `tui.*` keys in `clawcode.jsonc` without triggering `ConfigDiagnostic::UnknownField`:

| Setting Path | Type & Values | Default | Behavior |
|---|---|---|---|
| `animations` | `bool` | `true` | Enables typewriter pacing and animated wave spinner; when `false`, flushes deltas immediately and renders static `...` / `⋯`. |
| `session.thinking` | `ThinkingDisplay` (`hide`, `show`) | `hide` | Controls whether reasoning blocks render collapsed (`Thought · N steps · Xs`) or expanded inline. |
| `session.grouping` | `ToolGrouping` (`auto`, `none`) | `auto` | Collapses consecutive exploration tools (`read`, `glob`, `grep`, `webfetch`, `websearch`) into an expandable `Exploring` / `Explored` summary when `auto`. |
| `session.verbosity` | `TranscriptVerbosity` (`low`, `medium`, `high`) | `medium` | `low` groups all non-text parts (reasoning + tools) into a compact expandable `Activity` row; `medium` uses standard grouping; `high` auto-expands exploration and instruction details. |
| `session.markdown` | `MarkdownMode` (`rendered`, `source`) | `rendered` | Switches assistant and reasoning markdown between styled spans (`rendered`) and raw markdown text (`source`). |
| `session.tps` | `bool` | `true` | Shows live `tok/s` during active streaming and final `tok/s` in the assistant turn footer. |
| `session.scrollbar` | `bool` | `true` | Toggles vertical scrollbar rendering in `render_chat`. |
| `scroll.speed` | `u16` (`1..=10`) | `3` | Lines scrolled per mouse wheel / line-scroll step. |
| `scroll.acceleration` | `bool` | `false` | Accelerates rapid consecutive scroll steps within 80ms windows. |
| `mouse` | `bool` | `true` | Enables or ignores mouse click/scroll events in TUI input handling. |
| `leader.timeout` | `u64` (`250..=10000` ms) | `2000` | Timeout before `leader_pending` (`Ctrl+X`) auto-cancels on tick. |
| `debug.turn_tokens` | `TurnTokensMode` (`off`, `compact`, `verbose`) | `off` | Appends a collapsible step-token summary (`Tokens: N steps · latest: X new · Y cached · Z total`) after completed turns; `verbose` lists per-step tool names. |

### 2.2 Atomic Config Persistence
Add `Config::save_tui_settings(path: &Path, settings: &TuiSettingsConfig) -> Result<(), String>`:
- Reads existing JSONC text if `path` exists (or initializes `{}`), updates/inserts the modified setting keys while preserving unrelated top-level JSONC sections, validates via `Config::parse_str`, writes to `<filename>.<pid>.tmp` in the same directory, and atomically renames over `path`.

### 2.3 SQLite Schema v10 (`kv_state`)
Bump `SCHEMA_VERSION` from `9` to `10` in `src/persistence/schema.rs`:
```sql
CREATE TABLE IF NOT EXISTS kv_state (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
```
Add `Db::get_kv(&self, key: &str) -> Result<Option<String>, rusqlite::Error>` and `Db::set_kv(&self, key: &str, value_json: &str) -> Result<(), rusqlite::Error>` (plus `WriterHandle::set_kv` so the TUI thread never blocks on SQLite writes).

Keys stored in `kv_state`:
- `model.favorites`: JSON array of `"provider/model"` strings.
- `model.recent`: JSON array of `"provider/model"` strings (bounded to 10 most recent, MRU first).
- `model.variants`: JSON object mapping `"provider/model"` to active variant string (e.g., `"high"`).
- `prompt.stash`: JSON array of `{ "text": String, "timestamp_ms": u64 }` (bounded to 50 entries, each text bounded to `MAX_MESSAGE_BYTES` on valid UTF-8 boundary).

---

## 3. Queued Prompts (`Queue` & `Steer`) and Prompt Stash

### 3.1 Persistence & Runtime Inbox Operations
`session_inputs` already stores `(id, session_id, content, delivery, status, ...)`.
1. Update `Db::next_pending_input(session_id)` to prioritize `steer` over `queue`:
   `ORDER BY CASE WHEN delivery = 'steer' THEN 0 ELSE 1 END, id ASC LIMIT 1`.
2. Add `Db::pending_session_inputs(session_id) -> Result<Vec<SessionInput>, rusqlite::Error>`.
3. Add `Db::update_input_delivery(session_id: i64, input_id: i64, delivery: InputDelivery) -> Result<(Option<SessionInput>, i64), rusqlite::Error>` (emits durable `prompt_updated` event when a pending row changes).
4. Add `Db::discard_pending_input(session_id: i64, input_id: i64) -> Result<(Option<SessionInput>, i64), rusqlite::Error>` (sets `status = 'discarded'` on a `pending` row and emits durable `prompt_discarded` event).
5. Expose `WriterHandle::update_input_delivery` and `WriterHandle::discard_pending_input`.
6. Extend `SessionCoordinator` with `steer(&self, session_id: i64) -> Option<i64>`:
   - Unlike `interrupt()`, which sets `cleaning_up = true` and halts the drain loop, `steer()` sets `wake_pending = true` (leaving `cleaning_up = false`) and returns `active_generation_id` so the worker calls `db.request_cancel(generation_id)`. When `run_generation` exits from cancellation, `drain_session_loop` immediately continues (`complete_turn() == true`) and promotes the `steer` input.
7. Add `ClientCommand::SteerInput { session_id: i64, input_id: i64, reply: mpsc::Sender<Result<bool, String>> }` and `ClientCommand::DiscardInput { session_id: i64, input_id: i64, reply: mpsc::Sender<Result<Option<String>, String>> }` on `RuntimeClient`.

### 3.2 TUI Composer & `Queued prompts` Dialog
- **Submit While Active**: When `status == ConversationStatus::Active` (or runtime is active for the session), `submit_prompt()` does **not** cancel the active generation; it submits via `runtime.start_generation(...)` (`InputDelivery::Queue`), clears the composer, and records the pending entry from `prompt_admitted` (or local optimistic state reconciled by `prompt_admitted` / `prompt_promoted` / `prompt_discarded`).
- **Empty Submit Steers First Queued Prompt**: Pressing `Enter` when `prompt.trim().is_empty()` while `queued_prompts` is non-empty steers the first queued prompt (`SteerInput`).
- **Pending Strip**: When `queued_prompts` is non-empty, the 1-line gap row directly above the input card displays `N queued · <single-line preview> (Ctrl+X q to manage)`. Clicking it or pressing `Ctrl+X q` / `/queued` opens `QueuedPromptsDialogState`.
- **Queued Prompts Dialog (`src/tui/dialogs/queued_prompts.rs`)**:
  - Searchable modal listing pending prompts (`1 of N`).
  - `Enter`: steers selected prompt immediately and closes modal.
  - `Ctrl+D`: deletes/discards selected prompt (auto-closes modal if list becomes empty).
  - `Ctrl+Z` (undo): discards selected prompt from inbox, restores its text into composer input (appending with newline if composer already has text), and closes modal.
- **Prompt Stash (`src/tui/dialogs/stash.rs`, `/stash`)**:
  - Slash commands & palette actions: `/stash` (opens `Stash` dialog if composer is empty, or stashes current non-empty prompt and clears composer), `Stash prompt`, `Pop stashed prompt`, `View prompt stash`.
  - `StashDialogState`: `Enter` pops selected entry into composer; `Ctrl+D` pressed twice confirms deletion (`Press Ctrl+D again to confirm`).

---

## 4. `Ctrl+B` Blocking-Tool Backgrounding

### 4.1 Runtime Detach Mechanics
- Add a per-session background trigger channel or atomic flag in `SharedGenerationCtx` (`background_requested: Arc<AtomicBool>`) registered on `SessionCoordinator` (or worker map keyed by `session_id`), and `ClientCommand::BackgroundTools { session_id: i64 }`.
- In `ToolWorkers` (`src/runtime/client.rs`):
  - Poll `rx.recv_timeout(Duration::from_millis(50))` instead of blocking indefinitely on `rx.recv()`.
  - Track which spawned tasks are backgroundable blocking tools (`bash`, `sh`, `shell`) and have not yet finished.
  - When `background_requested` becomes `true` (and `cancel_requested` is `false`), detach all currently running shell tasks:
    1. Immediately return `LocalToolOutcome::Backgrounded { shell_id, partial_output }` to `run_generation` for each running shell task so `run_generation` settles the tool call in SQLite with `ToolCallStatus::Completed`, output `"Command moved to background (output continues streaming)"`, and `metadata: { "background": true, "status": "running", "shell_id": call_id }`.
    2. Remove detached task indices from `ToolWorkers::Drop` blocking wait so `run_generation` immediately proceeds to the next LLM turn without waiting for the child shell process.
    3. A lightweight monitor closure on the detached shell thread continues emitting `tool_call_progress` events (`seq = 0`) and, upon process exit, updates tool call metadata (`"status": "completed"` or `"failed"`) and emits `tool_background_finished`.

### 4.2 TUI `Ctrl+B` Key Resolution & 3-Second Hint
- In `src/tui/input.rs`, map `Ctrl+B` to `Input::BackgroundOrLeft`.
- In `App::handle_input`:
  - If `prompt.is_empty()` and the active session has a running blocking tool (`bash` / `sh` / `shell`), `Input::BackgroundOrLeft` calls `request_background_tools()` and sets diagnostic/status feedback (`"Moved running command to background."`).
  - Otherwise, `Input::BackgroundOrLeft` behaves identically to `Input::Left` (cursor left).
  - Command Palette (`PaletteAction::BackgroundTools`) and `/background` always trigger `request_background_tools()` regardless of composer text.
- **3-Second Hint**: `ToolRow` records `started_at: Instant` when entering `ToolRowState::Running`. When a foreground shell tool has been running for `>= 3s` in `render_chat`, append a muted hint line below the tool row: `Press Ctrl+B to move running work to the background`.

---

## 5. Streaming UX, Transcript Controls, & Navigation

### 5.1 Typewriter, Live TPS, & Turn Footer
- `App` syncs `TuiSettingsConfig` into `TypewriterState` (`set_animations_enabled(bool)`): when disabled, `push()` advances `displayed_bytes = buffer.len()` immediately and `is_typing()` reflects live stream state without artificial drain lag.
- `status_label(app)` in `src/tui/render.rs`: when `is_working && app.tui_settings().session_tps` and `typewriter.tokens_per_second() > 0.0`, append ` · {:.1} tok/s`.
- Assistant turn footer in `src/tui/chat.rs`:
  - Displays `▣ [MODE] · <provider/model>[ · <variant>] · <duration>` plus ` · {:.1} tok/s` (only when `session_tps == true`) and ` · interrupted` when `status == ConversationStatus::Cancelled`.
  - When `debug_turn_tokens != Off` and usage metrics exist, renders an expandable token line (`+ Tokens: N steps · latest: X new · Y total`) below the turn footer.

### 5.2 Thinking, Exploration Grouping, Verbosity, & Markdown Source
- `session.thinking`: sets default expansion state for reasoning parts (`hide` -> collapsed `Thought` header, `show` -> expanded reasoning text). Clicking a reasoning header or running `Toggle thinking` (`PaletteAction` / `/thinking`) toggles it.
- `session.grouping`: when `none`, `append_stream_parts` renders every exploration tool (`read`, `glob`, `grep`, `webfetch`, `websearch`) as an individual tool row instead of collapsing consecutive exploration rows.
- `session.verbosity`:
  - `low`: collapses consecutive non-text parts (reasoning + all tools) into a single expandable `+ N commands, M reads` activity summary row unless manually expanded.
  - `medium`: standard behavior (`session.thinking` + `session.grouping`).
  - `high`: auto-expands exploration groups and tool output previews by default.
- `session.markdown`: when `source`, bypasses markdown conceal/span formatting in `chat.rs` and renders raw markdown lines with standard assistant indentation and word wrapping.

### 5.3 Sticky Scroll, `Jump to latest ↓`, & Message Navigation
- When `chat_scroll > 0`, incoming streaming deltas preserve the user's viewport anchor by incrementing `chat_scroll` by the added visual lines (clamped to `u16::MAX`), preventing text jumps while reading earlier messages.
- When `chat_scroll > 0`, `render_chat` renders a right-aligned `Jump to latest ↓` badge on the bottom transcript line (clickable via mouse and bound to `End` / `Ctrl+Alt+G`), resetting `chat_scroll = 0`.
- Add OpenCode message navigation bindings in `src/tui/input.rs`:
  - `Ctrl+Alt+B` (`PageUp`), `Ctrl+Alt+F` (`PageDown`), `Ctrl+Alt+U` (half-page up), `Ctrl+Alt+D` (half-page down), `Ctrl+Alt+Y` (line up), `Ctrl+Alt+E` (line down), `Ctrl+G` / `Alt+Home` (first message), `Ctrl+Alt+G` (last message), `Alt+End` (jump to last user message), `Ctrl+X y` / `/copy` (copy last assistant response to clipboard via `ClipboardBackend`).

---

## 6. Model Variants (`Ctrl+T`, `/variant`) & Favorites / Recent (`F2`)

### 6.1 Model Variants End-to-End
- `CustomModelConfig` in `src/config/mod.rs`: add `variants: Vec<String>` (parsed from `"variants"` array or object keys in `clawcode.jsonc`).
- Built-in variant resolver `available_variants_for_model(provider: &str, model: &str, config: &Config) -> Vec<String>`:
  - Returns configured `variants` if non-empty (prefixed with `"default"` if absent).
  - For reasoning-capable models (`o1`, `o3`, `o4`, `gpt-5`, `claude-3-7`, `claude-sonnet-4`, `claude-opus-4`, `deepseek-r1`, `qwq`, `gemini-2.5`), returns `["default", "low", "medium", "high"]`.
  - Otherwise returns `["default"]`.
- `StreamRequest` (`src/provider/traits.rs`): add `pub variant: Option<String>` (`None` when `"default"`).
- Adapter wiring:
  - `src/adapters/openai_compatible.rs`: when `request.variant` is `Some("low" | "medium" | "high")`, include `"reasoning_effort": variant` in the JSON request body.
  - `src/adapters/anthropic.rs`: when `request.variant` is `Some(v)`, include `"thinking": { "type": "enabled", "budget_tokens": budget }` (`low` = `1024`, `medium` = `4096`, `high` = `16000`, clamped below `max_tokens` or bumping `max_tokens` accordingly).
- TUI controls:
  - `Ctrl+T` (`Input::CycleVariant`): cycles through `available_variants_for_model`, persists choice in `kv_state` (`model.variants`), and updates `identity_label` (`provider/model · <variant>` when non-default). If only `"default"` exists, displays `"Model uses default variant only."`.
  - `/variant` command & `VariantsDialogState` (`src/tui/dialogs/variants.rs`): modal selector for active model's variants.

### 6.2 Models Dialog Favorites, Recent, & Provider Filter
- Upgrade `ModelsDialogState` (`src/tui/dialogs/models.rs`):
  - Categorizes rows into `Favorites` (starred `★`), `Recent`, and provider groups (`provider_id`).
  - `Ctrl+F`: toggles favorite status for the highlighted model and persists `model.favorites` to `kv_state`.
  - `Ctrl+A`: cycles provider filter (`All providers` -> each distinct provider).
  - Selecting any model updates `model.recent` (MRU, max 10) in `kv_state`.
- Global shortcuts `F2` (`Input::CycleModel`) and `Shift+F2` (`Input::CycleModelReverse`): cycle through `model.recent` (falling back to `available_models` when recent list has `< 2` entries).

---

## 7. Settings Dialog (`src/tui/dialogs/settings.rs`)
- Opened via `/settings` slash command or Command Palette (`Settings`).
- Uses Clawcode's standard rounded `theme.teal` / `theme.panel` modal frame.
- Search bar in top row filters settings by title, category (`Appearance`, `Session`, `Input`, `Debug`), and keywords.
- `Up`/`Down` (or `Ctrl+P`/`Ctrl+N`) navigates items; `Left`/`Right` or `Enter` cycles/steps the active setting value, immediately applies it to `App`, and persists it atomically to `clawcode.jsonc`.
- Footer displays `↑↓ navigate · ←/→/Enter change · Esc close`.

---

## 8. Error Handling & Edge Cases
- **Read-only or invalid config path on `/settings` change**: In-memory setting still applies immediately for the active session, and `app.diagnostic` surfaces a clear non-crashing message (`"Applied in session; could not save clawcode.jsonc: <reason>"`).
- **Steer or Undo Race Condition**: If a queued prompt was already promoted by the worker millisecond before the user pressed `Delete` or `Undo`, `discard_pending_input` returns `Ok(None)` and TUI displays `"Prompt already started"` without duplicating or losing composer text.
- **Session Switch Isolation**: Pending prompt lists, scroll offsets, and expanded group toggles are stored per-session in `ClientSessionState` so switching tabs (`Ctrl+Tab`) never leaks queue badges or scroll positions across sessions.
- **Leader Timeout Configurable**: `App::tick_animations` checks `self.tui_settings.leader_timeout_ms` so `leader_pending` automatically expires after the configured timeout instead of hanging indefinitely if no second key is pressed.
