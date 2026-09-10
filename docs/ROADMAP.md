# ROADMAP — Clawcode

Roadmap memakai milestone kecil. Setiap milestone harus melewati validation gate sebelum milestone berikutnya dimulai.

## M0 — Foundation

Buat Cargo package, module boundary, error model, tracing, CLI entry, dan CI matrix Windows/Linux/macOS.

**Gate:** `cargo check`, `cargo test`, lint, dan binary exit code tervalidasi pada semua target yang tersedia.

## M1 — TUI shell

Bangun Ratatui event loop, Quiet editorial layout, input editor, resize, help, quit, dan render benchmark harness.

**Gate:** first frame warm/local di baseline benchmark; render loop tetap responsif saat synthetic stream.

## M2 — Config

Implement JSONC global/project merge, `schema_version`, migration, local schema, environment secret references, dan diagnostics file/line/column.

**Gate:** fixture valid/invalid/unknown field/override/migration lulus; parsing tidak crash.

## M3 — Persistence dan sessions

Implement SQLite schema, session CRUD, assembled message recovery, metrics, model cache, retention/size limits, async batched writes.

**Gate:** restart recovery, concurrent render simulation, retention enforcement, dan migration tests lulus.

## M4 — Provider contract

Implement provider trait, normalized events, bounded channels, coalescing, render batching, priority cancellation, capability flags, dan usage metrics.

**Gate:** event contract tests membuktikan core bebas provider; cancellation tidak tersumbat; argument size limit bekerja.

## M5 — Provider adapters

Implement generic OpenAI-compatible, native Anthropic, native Ollama, custom endpoint, registry, discovery cache stale-while-revalidate, TTL/backoff, timeout, dan independent failure.

**Gate:** mocked streaming fixtures untuk setiap adapter; cache fallback dan `/models refresh` lulus tanpa blocking startup.

## M6 — PLAN/BUILD workspace

Implement project root, canonical boundary, read tools, transactional file mutation + policy-gated shell execution, patch snapshots, diff, policy, approval, atomic restore, dan risk classification.

**Gate:** PLAN mutation test gagal aman; BUILD transaction/order, outside-root, symlink, dangerous command, approval, undo/redo lulus.

## M7 — Conversation UX

Hubungkan prompt ke core, slash commands, session switcher, provider/model picker, tool lifecycle, diagnostics, diff review, dan metrics status.

**Gate:** scripted PTY flows untuk new/session/connect/models/plan/build/cancel/exit lulus.

## M8 — Notifications dan hardening

Tambah terminal bell, desktop notification, sound, clipboard/shell discovery fallback, performance profiling, fuzz/property tests, security review, packaging. Letakkan notification, clipboard, shell discovery, dan credential store dalam submodule/adapter OS masing-masing; hindari penyebaran `cfg(target_os)` ke codebase.

**Gate:** notification failure tidak mengubah result; startup/stream/memory budgets dan cross-platform smoke tests lulus.

## M9 — Release candidate

Dokumentasi, config examples, migration notes, release binaries, reproducible build checks, dan manual usability pass.

**Gate:** acceptance criteria PRD 100% terpetakan dan tidak ada blocker severity tinggi.

## Ditunda setelah MVP

ACP, MCP, daemon, plugin ecosystem, remote client, desktop app, OAuth provider khusus, dan feature parity platform-specific lanjutan.
