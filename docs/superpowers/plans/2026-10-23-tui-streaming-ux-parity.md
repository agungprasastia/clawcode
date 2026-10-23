# TUI Streaming, Transcript UX, & Settings Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver OpenCode v2.1.3 behavioral parity for TUI streaming, transcript verbosity/grouping/thinking/markdown/TPS controls, `/settings`, queued/steered prompts, prompt stash, `Ctrl+B` blocking-tool backgrounding, and model variants/favorites/recent while preserving Clawcode's visual design.

**Architecture:** Extend `Config` with atomic JSONC persistence for `/settings` and bump SQLite to v10 (`kv_state` + inbox steer/discard helpers). Wire `StreamRequest.variant`, inbox steering, and non-blocking `ToolWorkers` backgrounding through `RuntimeClient`, then connect TUI state, keybindings, transcript rendering, and modal dialogs without altering Clawcode's theme, banner, home mascot, or layout geometry.

**Tech Stack:** Rust 1.98.0 (edition 2024), `ratatui`, `crossterm`, `rusqlite` (WAL), `serde_json`, `std::thread` + `std::sync::mpsc` (synchronous runtime only).

**Spec:** `docs/superpowers/specs/2026-10-23-tui-streaming-ux-parity-design.md`

## Global Constraints

- Preserve Clawcode's existing layout geometry, header banner (`CLAWCODE · [MODE]`), home mascot, palette colors, and rounded dialog border aesthetic in `src/tui/render.rs`, `src/tui/home.rs`, `src/tui/chat.rs`, and `src/tui/theme.rs`.
- Use Rust standard library concurrency (`std::thread`, `std::sync::{Arc, Mutex}`, `std::sync::mpsc`) exclusively; zero `tokio` or `async fn`.
- Pass `cargo clippy --all-targets --all-features --locked -- -D warnings` with zero `#[allow(...)]` attributes.
- Bound all buffers and string previews on valid UTF-8 character boundaries.
- Zero em dashes (U+2014) in any UI copy, CLI output, code comments, or docs; zero dead controls or stubs.

## Review Focus

1. **JSONC comments, trailing commas, and `env:` secret references on `/settings` writeback:** Saving TUI settings onto an existing `clawcode.jsonc` with comments and `"api_key": "env:OPENAI_API_KEY"` must preserve existing provider/secret fields and re-parse cleanly without `PlaintextSecret` or `UnknownField` errors (tested in Task 1).
2. **Steer vs Esc interrupt semantics on active sessions:** Steering a pending prompt must cancel only the active generation while keeping `drain_session_loop` alive to promote the steered prompt immediately, whereas discarding an already-promoted input must return `Ok(None)` safely (tested in Tasks 2 and 4).
3. **`ToolWorkers::Drop` non-blocking exit after `Ctrl+B` backgrounding:** Backgrounding a slow shell tool must detach it from `ToolWorkers` wait accounting so `run_generation` advances immediately without deadlocking on `Drop` (tested in Task 4).
4. **Viewport stability when scrolled up (`chat_scroll > 0`) during streaming:** Appending deltas while the user reads history must maintain reading position and expose `Jump to latest ↓` without snapping to bottom (tested in Task 5).
5. **Multi-byte UTF-8 truncation and cross-session tab isolation:** Queued prompt previews, stash entries, and per-session pending queues must never panic on multi-byte characters (emoji/CJK) or leak across `Ctrl+Tab` session switches (tested in Task 6).

---

### Task 1: Config `TuiSettingsConfig`, Model Variants Config, & Atomic JSONC Persistence

**Files:**
- Modify: `src/config/mod.rs`
- Modify: `config/schema.json`

**Interfaces:**
- Consumes: `crate::config::jsonc` parser and `Config::parse_str`.
- Produces:
  - `ThinkingDisplay::{Hide, Show}`, `ToolGrouping::{Auto, None}`, `TranscriptVerbosity::{Low, Medium, High}`, `MarkdownMode::{Rendered, Source}`, `TurnTokensMode::{Off, Compact, Verbose}`
  - `TuiSettingsConfig` on `Config::tui` (defaulting to OpenCode v2.1.3 defaults: `animations: true`, `thinking: Hide`, `grouping: Auto`, `verbosity: Medium`, `markdown: Rendered`, `tps: true`, `scrollbar: true`, `scroll_speed: 3`, `scroll_acceleration: false`, `mouse: true`, `leader_timeout_ms: 2000`, `turn_tokens: Off`)
  - `CustomModelConfig::variants: Vec<String>`
  - `Config::save_tui_settings(path: &Path, settings: &TuiSettingsConfig) -> Result<(), String>`

- [ ] **Step 1: Write the failing unit tests in `src/config/mod.rs`**

Add unit tests verifying:
1. `parses_opencode_and_tui_settings_fields`: parses both OpenCode top-level style (`animations`, `mouse`, `session: { thinking, grouping, verbosity, markdown, tps, scrollbar }`, `scroll: { speed, acceleration }`, `leader: { timeout }`, `debug: { turn_tokens }`) and `provider.<id>.models.<m>.variants` without `UnknownField`.
2. `save_tui_settings_preserves_existing_fields_and_roundtrips`: writes a temp `clawcode.jsonc` with `// comment`, trailing comma, and `"api_key": "env:TEST_KEY"`, calls `Config::save_tui_settings`, and asserts `Config::load_file` returns both `SecretRef::Env("TEST_KEY")` and the updated `TuiSettingsConfig`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::parses_opencode_and_tui_settings_fields --locked`
Expected: FAIL (missing `TuiSettingsConfig` / `UnknownField`).

- [ ] **Step 3: Implement `TuiSettingsConfig`, `CustomModelConfig::variants`, and `Config::save_tui_settings` in `src/config/mod.rs` and update `config/schema.json`**

Add the typed enums with `as_str()` / `from_str()` / `cycle(delta: isize)` helpers, register top-level fields (`tui`, `animations`, `session`, `scroll`, `mouse`, `leader`, `debug`) in `KNOWN_FIELDS`, parse `variants` (array of strings or object keys) in `parse_models`, merge `tui` settings across layers, and implement atomic sibling-temp-file writeback in `Config::save_tui_settings`.

- [ ] **Step 4: Run config unit tests to verify they pass**

Run: `cargo test --lib config:: --locked`
Expected: PASS (all `config::` tests green).

- [ ] **Step 5: Commit**

```powershell
git add src/config/mod.rs config/schema.json
git commit -m "feat(config): add TUI settings, model variants, and atomic JSONC save"
```

---

### Task 2: SQLite Schema v10 (`kv_state`) & Session Inbox Steer/Discard Primitives

**Files:**
- Modify: `src/persistence/schema.rs`
- Modify: `src/persistence/db.rs`
- Modify: `src/persistence/writer.rs`
- Modify: `src/persistence/mod.rs`

**Interfaces:**
- Consumes: `Db`, `WriterHandle`, `SessionInput`, `InputDelivery`.
- Produces:
  - `SCHEMA_VERSION = 10` with `kv_state (key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at TEXT NOT NULL)`
  - `Db::get_kv(&self, key: &str) -> Result<Option<String>, rusqlite::Error>`
  - `Db::set_kv(&self, key: &str, value_json: &str) -> Result<(), rusqlite::Error>`
  - `Db::pending_session_inputs(&self, session_id: i64) -> Result<Vec<SessionInput>, rusqlite::Error>`
  - `Db::update_input_delivery(&self, session_id: i64, input_id: i64, delivery: InputDelivery) -> Result<Option<(SessionInput, i64)>, rusqlite::Error>` (emits `prompt_updated`)
  - `Db::discard_pending_input(&self, session_id: i64, input_id: i64) -> Result<Option<(SessionInput, i64)>, rusqlite::Error>` (emits `prompt_discarded`)
  - `WriterHandle::set_kv`, `WriterHandle::update_input_delivery`, `WriterHandle::discard_pending_input`

- [ ] **Step 1: Write the failing persistence tests**

Add unit tests in `src/persistence/db.rs` (and schema migration test in `src/persistence/schema.rs`):
1. `kv_state_roundtrips_and_overwrites`: tests `get_kv` / `set_kv` on `:memory:` DB.
2. `steer_prioritized_ahead_of_queued_inputs_and_discard_skips_promoted`: admits 3 `Queue` inputs, updates #3 to `Steer`, asserts `next_pending_input` returns #3 first, promotes #3, asserts `discard_pending_input` on promoted #3 returns `Ok(None)`, and `discard_pending_input` on pending #1 marks it `discarded` and emits `prompt_discarded`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib persistence:: --locked`
Expected: FAIL (missing `get_kv`, `update_input_delivery`, `discard_pending_input`).

- [ ] **Step 3: Implement Schema v10, `Db` methods, and `WriterHandle` commands**

Update `SCHEMA_VERSION` to `10` in `src/persistence/schema.rs` (plus update any existing version assertion test from `9` to `10`), update `Db::next_pending_input` ordering (`CASE WHEN delivery = 'steer' THEN 0 ELSE 1 END, id ASC`), implement `get_kv`, `set_kv`, `pending_session_inputs`, `update_input_delivery`, and `discard_pending_input`, and expose batched write commands on `WriterHandle`.

- [ ] **Step 4: Run persistence tests to verify they pass**

Run: `cargo test --lib persistence:: --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/persistence/schema.rs src/persistence/db.rs src/persistence/writer.rs src/persistence/mod.rs
git commit -m "feat(persistence): add kv_state v10 table and inbox steer/discard ops"
```

---

### Task 3: Provider `StreamRequest` Variant Wiring & Model Variant Resolver

**Files:**
- Modify: `src/provider/traits.rs`
- Modify: `src/provider/mod.rs`
- Modify: `src/adapters/openai_compatible.rs`
- Modify: `src/adapters/anthropic.rs`

**Interfaces:**
- Consumes: `CustomModelConfig::variants` from Task 1.
- Produces:
  - `StreamRequest::variant: Option<String>` and `StreamRequest::with_variant(mut self, variant: Option<String>) -> Self`
  - `crate::provider::available_variants_for_model(provider: &str, model: &str, config: Option<&crate::config::Config>) -> Vec<String>`

- [ ] **Step 1: Write the failing adapter and variant resolver tests**

Add unit tests verifying:
1. `openai_adapter_serializes_reasoning_effort_when_variant_set`: `StreamRequest::new("o3-mini", "hi", 4096).with_variant(Some("high".into()))` produces `"reasoning_effort":"high"` in request JSON, and omits it when `None` or `"default"`.
2. `anthropic_adapter_serializes_thinking_budget_when_variant_set`: `StreamRequest::new("claude-sonnet-4-0", "hi", 4096).with_variant(Some("medium".into()))` produces `"thinking":{"type":"enabled","budget_tokens":4096}` and ensures `max_tokens > budget_tokens`.
3. `available_variants_resolves_custom_and_reasoning_models`: custom config variants override defaults; reasoning model IDs return `["default", "low", "medium", "high"]`; standard non-reasoning models return `["default"]`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib adapters:: --locked`
Expected: FAIL (`with_variant` / `available_variants_for_model` undefined).

- [ ] **Step 3: Implement `StreamRequest::variant`, adapter request fields, and `available_variants_for_model`**

Add `pub variant: Option<String>` (default `None`) to `StreamRequest` (updating struct literals in `src/runtime/client.rs` and tests if needed), wire `reasoning_effort` in `src/adapters/openai_compatible.rs` and `thinking` budget in `src/adapters/anthropic.rs`, and implement `available_variants_for_model` in `src/provider/mod.rs`.

- [ ] **Step 4: Run provider and adapter tests to verify they pass**

Run: `cargo test --lib provider:: --locked && cargo test --lib adapters:: --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/provider/traits.rs src/provider/mod.rs src/adapters/openai_compatible.rs src/adapters/anthropic.rs src/runtime/client.rs
git commit -m "feat(provider): wire model variants to OpenAI and Anthropic requests"
```

---

### Task 4: Runtime Worker Steer/Discard Inbox & `Ctrl+B` Blocking-Tool Backgrounding

**Files:**
- Modify: `src/runtime/coordinator.rs`
- Modify: `src/runtime/client.rs`
- Create: `tests/runtime_queue_and_background.rs`

**Interfaces:**
- Consumes: Task 2 (`WriterHandle::update_input_delivery`, `WriterHandle::discard_pending_input`, `Db::pending_session_inputs`, `Db::get_kv`/`WriterHandle::set_kv`) and Task 3 (`StreamRequest.variant`).
- Produces:
  - `SessionConfig::variant: Option<String>` and `GenerationRequest::variant: Option<&'a str>`
  - `SessionCoordinator::steer(&self, session_id: i64) -> Option<i64>`
  - `SessionCoordinator::request_background(&self, session_id: i64) -> bool` (triggers per-session `Arc<AtomicBool>`)
  - `RuntimeClient::steer_input(&self, session_id: i64, input_id: i64) -> Result<bool, String>`
  - `RuntimeClient::discard_input(&self, session_id: i64, input_id: i64) -> Result<Option<String>, String>`
  - `RuntimeClient::pending_inputs(&self, session_id: i64) -> Result<Vec<crate::persistence::SessionInput>, String>`
  - `RuntimeClient::background_tools(&self, session_id: i64) -> Result<bool, String>`
  - `RuntimeClient::get_kv(&self, key: &str) -> Result<Option<String>, String>` and `RuntimeClient::set_kv(&self, key: &str, value_json: &str) -> Result<(), String>`
  - Runtime bus events: `prompt_updated`, `prompt_discarded`, `tool_background_finished`

- [ ] **Step 1: Write failing integration tests in `tests/runtime_queue_and_background.rs`**

Write 3 deterministic tests using `:memory:` SQLite + custom `Provider`:
1. `submitting_while_active_queues_and_drains_in_order`: starts generation 1 (blocked on gate), admits prompt 2 and prompt 3, releases gate, verifies all 3 complete sequentially without cancelling generation 1.
2. `steering_queued_prompt_interrupts_current_turn_and_runs_steered_first`: starts slow generation 1, queues prompt 2 and prompt 3, calls `client.steer_input(session_id, input_3_id)`, verifies generation 1 cancels cleanly and prompt 3 promotes before prompt 2; also verifies `discard_input` returns prompt 2's content and prevents its execution.
3. `background_tools_unblocks_generation_turn_without_deadlocking_drop`: model emits a `bash` tool call (`sleep` / short wait command), client calls `background_tools(session_id)`, verifies tool call settles immediately with `metadata_json` containing `"background":true` and `"status":"running"`, generation finishes its follow-up assistant turn immediately, and `tool_background_finished` arrives once the command exits.

- [ ] **Step 2: Run integration test to verify it fails**

Run: `cargo test --test runtime_queue_and_background --locked`
Expected: FAIL (missing `steer_input`, `discard_input`, `background_tools`).

- [ ] **Step 3: Implement `steer`, `discard`, `kv` helpers, and non-blocking `ToolWorkers` backgrounding**

1. Add `steer` and per-session `background_flag: Arc<AtomicBool>` to `SessionCoordinator` in `src/runtime/coordinator.rs`.
2. Add `SteerInput`, `DiscardInput`, `BackgroundTools` to `ClientCommand` in `src/runtime/client.rs`, plus `pending_inputs`, `get_kv`, `set_kv` methods on `RuntimeClient`.
3. Update `ToolWorkers` in `src/runtime/client.rs` to poll `rx.recv_timeout(Duration::from_millis(25))`, inspect `background_flag`, detach active `bash`/`sh`/`shell` tasks from `expected` count when triggered, return `LocalToolOutcome::Backgrounded` so `run_generation` settles the call with `{"background": true, "status": "running", "shell_id": call_id}`, and spawn a lightweight completion observer that emits `tool_background_finished` when the detached thread finishes.

- [ ] **Step 4: Run integration and unit tests to verify they pass**

Run: `cargo test --test runtime_queue_and_background --locked && cargo test --lib runtime:: --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/runtime/coordinator.rs src/runtime/client.rs tests/runtime_queue_and_background.rs
git commit -m "feat(runtime): support prompt steer/discard and Ctrl+B tool backgrounding"
```

---

### Task 5: TUI Streaming UX, `/settings` Dialog, Transcript Modes, & Sticky Scroll

**Files:**
- Create: `src/tui/dialogs/settings.rs`
- Modify: `src/tui/dialogs/mod.rs`
- Modify: `src/tui/dialogs/palette.rs`
- Modify: `src/tui/typewriter.rs`
- Modify: `src/tui/input.rs`
- Modify: `src/tui/chat.rs`
- Modify: `src/tui/render.rs`
- Modify: `src/tui/app/mod.rs`
- Modify: `src/tui/app/input.rs`
- Modify: `src/tui/app/dialogs.rs`
- Modify: `src/tui/app/suggestions.rs`

**Interfaces:**
- Consumes: Task 1 (`TuiSettingsConfig`, `Config::save_tui_settings`).
- Produces:
  - `SettingsDialogState` and `render_settings_dialog` in `src/tui/dialogs/settings.rs`
  - `TypewriterState::set_animations_enabled(&mut self, enabled: bool)`
  - `App::tui_settings(&self) -> &TuiSettingsConfig`, `App::with_config_settings(&mut self, config: Config, config_path: Option<PathBuf>)`
  - Transcript rendering support in `render_chat` for `thinking` (`Hide`/`Show`), `grouping` (`Auto`/`None`), `verbosity` (`Low`/`Medium`/`High`), `markdown` (`Rendered`/`Source`), `tps`, `scrollbar`, `turn_tokens`, sticky scroll anchor when `chat_scroll > 0`, and clickable `Jump to latest ↓` badge.
  - OpenCode message navigation bindings (`Ctrl+Alt+B/F/U/D/Y/E`, `Ctrl+G`, `Ctrl+Alt+G`, `Alt+End`, `Ctrl+X y` / `/copy`).

- [ ] **Step 1: Write failing TUI tests for settings, typewriter animations, transcript modes, and sticky scroll**

Add unit tests in `src/tui/typewriter.rs` and `src/tui/app/tests.rs`:
1. `typewriter_animations_disabled_flushes_immediately`: `set_animations_enabled(false)` displays full text on `push()` with zero tick delay.
2. `settings_dialog_cycles_values_and_persists_to_config`: opens `/settings`, filters `"verbosity"`, presses `Right`, verifies `app.tui_settings().verbosity == TranscriptVerbosity::High` and temp `clawcode.jsonc` updated.
3. `transcript_respects_verbosity_grouping_markdown_and_sticky_scroll`: verifies `MarkdownMode::Source` shows raw `**bold**`, `ToolGrouping::None` separates consecutive `read` rows, `TranscriptVerbosity::Low` collapses tools into an activity summary, and streaming deltas while `chat_scroll > 0` keep viewport scrolled up and render `Jump to latest ↓`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib tui:: --locked`
Expected: FAIL (missing `SettingsDialogState`, `set_animations_enabled`, `tui_settings`).

- [ ] **Step 3: Implement `/settings` dialog, typewriter toggle, transcript modes, sticky scroll, and navigation shortcuts**

1. Add `set_animations_enabled` to `TypewriterState` and gate spinner animation in `render.rs` / `chat.rs` (`⋯` when `animations == false`).
2. Create `src/tui/dialogs/settings.rs` using Clawcode's rounded `theme.teal` / `theme.panel` frame (`Up`/`Down` navigate, `Left`/`Right`/`Enter` cycle, search filter) and register `/settings`, `/thinking`, `/copy` in `AVAILABLE_COMMANDS` and `PALETTE_ITEMS`.
3. Update `src/tui/chat.rs` and `src/tui/render.rs` for live `tok/s` in `status_label`, `session.tps` + `· interrupted` + `debug.turn_tokens` in turn footer, `thinking`/`grouping`/`verbosity`/`markdown`/`scrollbar` toggles, sticky scroll compensation on delta arrival, and clickable `Jump to latest ↓`.
4. Add OpenCode navigation keybindings in `src/tui/input.rs` and configurable `leader_timeout_ms` + `scroll_speed` / `mouse` handling in `App`.

- [ ] **Step 4: Run TUI unit tests to verify they pass**

Run: `cargo test --lib tui:: --locked`
Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add src/tui/
git commit -m "feat(tui): add /settings dialog, streaming UX toggles, live TPS, and sticky scroll"
```

---

### Task 6: TUI Queued Prompts, Prompt Stash, `Ctrl+B` Hint, & Models Favorites/Recent/Variants

**Files:**
- Create: `src/tui/dialogs/queued_prompts.rs`
- Create: `src/tui/dialogs/stash.rs`
- Create: `src/tui/dialogs/variants.rs`
- Modify: `src/tui/dialogs/mod.rs`
- Modify: `src/tui/dialogs/models.rs`
- Modify: `src/tui/dialogs/palette.rs`
- Modify: `src/tui/app/mod.rs`
- Modify: `src/tui/app/input.rs`
- Modify: `src/tui/app/dialogs.rs`
- Modify: `src/tui/app/runtime_bridge.rs`
- Modify: `src/tui/app/session_sync.rs`
- Modify: `src/tui/app/suggestions.rs`
- Modify: `src/tui/app/tool_rows.rs`
- Modify: `src/tui/chat.rs`
- Modify: `src/tui/render.rs`
- Modify: `src/main.rs`
- Modify: `docs/OPENCODE_PARITY.md`

**Interfaces:**
- Consumes: Task 3 (`available_variants_for_model`), Task 4 (`steer_input`, `discard_input`, `pending_inputs`, `background_tools`, `get_kv`, `set_kv`), and Task 5 (`App` dialog/settings infrastructure).
- Produces:
  - `QueuedPromptsDialogState`, `StashDialogState`, `VariantsDialogState`
  - Upgraded `ModelsDialogState` with `Favorites` (`Ctrl+F`), `Recent` (`F2` / `Shift+F2`), and provider filter (`Ctrl+A`)
  - Non-cancelling queue-on-submit while active, empty-submit steer, pending prompt indicator strip, `Ctrl+B` contextual backgrounding + 3-second hint, and `Ctrl+T` / `/variant` cycling
  - Updated `docs/OPENCODE_PARITY.md` reflecting completed Sub-Project 1 parity items against OpenCode v2.1.3

- [ ] **Step 1: Write failing TUI tests for prompt queue/steer/undo, stash UTF-8 safety, `Ctrl+B`, and model variants/favorites/recent**

Add tests in `src/tui/app/tests.rs`:
1. `submit_while_active_queues_instead_of_cancelling_and_undo_restores_prompt`: verifies submitting while `Active` enqueues without setting `cancellation_pending`, renders `1 queued · ...` with multi-byte emoji/CJK safely, and `Undo` in `QueuedPromptsDialogState` restores text to composer; also verifies session tab switching isolates queued prompts per session.
2. `ctrl_b_backgrounds_running_shell_when_prompt_empty_else_moves_cursor`: verifies `Ctrl+B` moves cursor left when `prompt` is non-empty, and triggers `background_tools` when `prompt` is empty and a `bash` tool row is `Running` (plus 3-second hint rendering).
3. `cycle_variant_and_model_favorites_recent_persist_in_kv_state`: verifies `Ctrl+T` cycles `default -> low -> medium -> high` for `o3-mini` (updating `identity_label`), `Ctrl+F` pins a favorite in `ModelsDialogState`, and `F2` / `Shift+F2` cycles recent models via SQLite `kv_state`.
4. `prompt_stash_pushes_pops_and_confirms_delete_with_utf8_bounds`: verifies `/stash` push/pop and double `Ctrl+D` delete confirmation.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib tui::app::tests:: --locked`
Expected: FAIL (missing dialogs and queue/variant/background handlers).

- [ ] **Step 3: Implement dialogs, runtime bridge wiring, startup config/state hydration, and `docs/OPENCODE_PARITY.md` update**

1. Create `queued_prompts.rs`, `stash.rs`, and `variants.rs` in `src/tui/dialogs/` and upgrade `models.rs` with `Favorites` (`★`), `Recent`, and `Ctrl+A` provider filter.
2. Update `App::submit_prompt` so active sessions enqueue instead of cancelling, empty `Enter` steers the first queued prompt, and `runtime_bridge.rs` reconciles `prompt_admitted`, `prompt_promoted`, `prompt_updated`, `prompt_discarded`, and `tool_background_finished` per session (`ClientSessionState`).
3. Wire `Input::BackgroundOrLeft` (`Ctrl+B`) + 3-second hint in `chat.rs`, `Input::CycleVariant` (`Ctrl+T`) + `/variant`, `F2`/`Shift+F2` recent model cycle, and `/queued` + `/stash` commands, and hydrate `TuiSettingsConfig` + `kv_state` on TUI startup in `src/main.rs`.
4. Update `docs/OPENCODE_PARITY.md` to mark completed Sub-Project 1 items (`Ctrl+B` backgrounding, `/settings`, live TPS/turn metrics, queued/steered prompts, prompt stash, model variants/favorites/recent, and OpenCode v2.1.3 reference).

- [ ] **Step 4: Run full test suite, strict clippy, and rustfmt verification**

Run:
```powershell
cargo test --all-targets --all-features --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
```
Expected: PASS with 0 warnings and 0 formatting diffs.

- [ ] **Step 5: Commit**

```powershell
git add src/ docs/OPENCODE_PARITY.md
git commit -m "feat(tui): add queued prompts, stash, Ctrl+B backgrounding, and model variants"
```
