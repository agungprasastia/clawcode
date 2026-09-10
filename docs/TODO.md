# TODO — Clawcode MVP

Prioritas: P0 wajib MVP, P1 penting sebelum release candidate, P2 sesudah MVP.

## P0 — Foundation dan UI

- [x] `P0-M0-01` Buat Cargo package dan module boundary; gate `cargo check`.
- [x] `P0-M0-02` Tambah error taxonomy dan tracing; gate unit tests.
- [x] `P0-M0-03` Siapkan CI Windows/Linux/macOS; gate matrix hijau.
- [x] `P0-M1-01` Implement TUI event loop dan clean shutdown; gate automated shutdown test (PTY smoke test ditunda sampai scripted flow Task 9).
- [x] `P0-M1-02` Implement Quiet editorial layout, input, resize, help; gate automated input/resize/render tests.
- [x] `P0-M1-03` Tambah synthetic streaming harness; gate bounded memory/redraw, priority cancel/quit, dan in-process first-frame benchmark.

## P0 — Config dan state

- [x] `P0-M2-01` Parse JSONC dengan lokasi error file/line/column; gate malformed fixtures.
- [x] `P0-M2-02` Merge global → project; gate precedence fixtures.
- [x] `P0-M2-03` Tambah `schema_version` migration dan unknown-field diagnostics; gate migration tests.
- [x] `P0-M2-04` Implement env/credential references; gate secret non-plaintext test.
- [x] `P0-M3-01` Rancang SQLite schema dan migration; gate fresh/upgrade DB tests.
- [x] `P0-M3-02` Implement session CRUD dan assembled message recovery; gate restart test.
- [x] `P0-M3-03` Implement async batched writes; gate render loop non-blocking test.
- [x] `P0-M3-04` Terapkan limits tool output, snapshots, model cache, history; gate retention tests. (history limits di Task 5; tool output/snapshots/model cache menyusul di Task 7–8)

## P0 — Provider dan streaming

- [ ] `P0-M4-01` Definisikan normalized event enum dan finish/usage types.
- [ ] `P0-M4-02` Implement bounded stream channel dan delta coalescing.
- [ ] `P0-M4-03` Implement priority cancellation dan terminal `Cancelled`.
- [ ] `P0-M4-04` Implement incremental tool argument assembler dengan size cap.
- [ ] `P0-M4-05` Implement provider trait, registry, capability flags, metrics.
- [ ] `P0-M5-01` Implement OpenAI-compatible adapter dan custom endpoint.
- [ ] `P0-M5-02` Implement native Anthropic adapter.
- [ ] `P0-M5-03` Implement native Ollama adapter.
- [ ] `P0-M5-04` Implement cached stale-while-revalidate discovery, TTL/backoff, timeout.
- [ ] `P0-M5-05` Tambah `/connect`, `/models`, `/models refresh`; gate mocked provider tests.

## P0 — Workspace dan agent modes

- [ ] `P0-M6-01` Detect project root dan canonical path boundary.
- [ ] `P0-M6-02` Implement read tools dengan output limit.
- [ ] `P0-M6-03` Implement transactional edit: validate → snapshot → diff → policy → apply.
- [ ] `P0-M6-04` Implement shell risk classification dan project boundary.
- [ ] `P0-M6-05` Implement approval, atomic restore, checksum, undo/redo.
- [ ] `P0-M6-06` Enforce PLAN read-only; gate forbidden mutation tests.
- [ ] `P0-M7-01` Hubungkan conversation, tool lifecycle, diff review, metrics.
- [ ] `P0-M7-02` Implement `/new`, `/sessions`, `/exit`, mode switch, model/session picker.
- [ ] `P0-M7-03` Load OpenCode-style agents, commands, permissions, themes; gate compatibility fixtures.
- [ ] `P0-M8-01` Implement terminal/desktop notification dan sound best-effort; gate failure isolation.
- [ ] `P0-M8-02` Add clipboard/shell discovery/credential-store OS adapters dengan portable fallback; gate OS adapter tests.

## P1 — Release hardening sebelum MVP release

- [ ] `P1-M8-03` Profil startup, stream, SQLite, memory, redraw CPU.
- [ ] `P1-M8-04` Add fuzz/property tests config, JSONC, event assembler, path policy.
- [ ] `P1-M8-05` Security review trust boundary dan dangerous operations.
- [ ] `P1-M9-01` Package Windows/Linux/macOS binaries dan reproducible build check.
- [ ] `P1-M9-02` Tulis README, config examples, diagnostics, migration docs.
- [ ] `P1-M9-03` Run full PRD acceptance checklist.

## P2 — Sesudah MVP

- [ ] ACP editor integration.
- [ ] MCP resources/tools.
- [ ] Daemon/remote client.
- [ ] Plugin system.
- [ ] Desktop app.
- [ ] OAuth provider-specific integrations.
