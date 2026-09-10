# PRD — Clawcode

## 1. Ringkasan

Clawcode adalah AI coding assistant berbasis Rust untuk terminal. Produk membuka cepat, memakai TUI editorial yang tenang, mendukung percakapan streaming, planning read-only, build transactional, banyak session, dan konfigurasi JSON/JSONC yang familiar bagi pengguna OpenCode.

MVP menargetkan Windows, Linux, dan macOS pada architecture level. Feature parity platform-specific boleh bertahap.

## 2. Tujuan

- Membuka TUI warm/local secara cepat; target benchmark baseline machine ≤100 ms, tanpa network pada startup critical path.
- Memberi alur aman: PLAN untuk memahami dan merencanakan; BUILD untuk perubahan terkontrol.
- Menjaga core tidak bergantung pada protokol provider.
- Menyediakan pengalaman provider-agnostic melalui generic OpenAI-compatible, native Anthropic, native Ollama, custom endpoint, registry, discovery, dan capability flags.
- Menyediakan kompatibilitas konfigurasi JSON/JSONC dan pola command/agent/permission yang familiar.

## 3. Non-goals MVP

Daemon, ACP, MCP, plugin system, desktop app, remote client, provider OAuth bespoke, dan provider adapter terpisah untuk endpoint yang sudah OpenAI-compatible.

## 4. Pengguna dan use cases

- Developer membuka assistant di project lalu memilih model.
- Developer meminta analisis tanpa risiko perubahan.
- Developer meminta implementasi, meninjau diff, lalu menerapkan perubahan.
- Developer berpindah session dan melanjutkan pekerjaan.
- Developer memakai config global/project, custom commands, agents, permissions, dan themes.

## 5. Fitur MVP

### TUI dan command

Quiet editorial: hierarchy jelas, whitespace terukur, diff dominan, keyboard-first, resize-aware. Commands: `/new`, `/sessions`, `/connect`, `/models`, `/models refresh`, `/exit`. Model/session switcher dan status bar metrics tersedia.

### Modes

PLAN read-only: tidak ada mutation dan shell mutatif. BUILD: transactional mutation `validate → snapshot → diff → policy decision → apply`. Diff selalu tersedia. Policy `allow` tidak meminta approval; operasi luar project dan operasi berisiko wajib approval.

### Provider

Trait provider internal, registry, model discovery cache stale-while-revalidate, TTL/backoff, timeout, cancellation, dan custom endpoint. Adapter: OpenAI-compatible, Anthropic, Ollama. Capability flags mencakup streaming, reasoning, tools, usage, vision bila didukung.

Normalized stream events: `TextDelta`, `ReasoningDelta`, `ToolCallStart`, `ToolCallDelta`, `ToolCallEnd`, `ToolResult`, `Usage`, `Finish`, `Error`, `Cancelled`. Bounded channel, delta coalescing, render batching. Cancellation punya jalur prioritas. Tool arguments incremental memiliki size limit.

### Persistence

SQLite untuk runtime/session state, messages assembled, metrics, provider metadata, dan model cache. Snapshot patch internal untuk undo/redo; tidak membuat internal Git repository. Streaming delta tidak disimpan sebagai row. SQLite writes asynchronous dari render loop. Semua history, output, snapshot, cache punya size/retention limit.

### Config

JSONC global dan project; project override global; `schema_version` untuk migration. Unknown fields menjadi diagnostic. Parse errors memuat file, line, column. Schema lokal boleh dibundel. Secrets memakai environment variable atau credential store, bukan plaintext config.

### Workspace dan safety

Canonical path boundary mencegah traversal/symlink escape. Read/edit/shell otomatis hanya dalam project bila policy mengizinkan. Operasi luar project, delete massal, overwrite sensitif, reset/clean Git, install dependency, network mutation, dan privilege escalation memerlukan approval. Snapshot restore atomik dan checksum-validated.

### Metrics dan notification

TTFT, latency, duration, input/output/cached tokens, throughput, finish status. Notification desktop/terminal dan sound best-effort, non-blocking; failure tidak menggagalkan successful turn.

## 6. Arsitektur modular satu crate

`main.rs` tipis sebagai composition root. Modules: `core`, `provider`, `adapters`, `config`, `workspace`, `persistence`, `tui`, `notify`, `cli`. Core memakai trait; TUI tidak HTTP/filesystem mutation langsung; adapters tidak menyentuh session/database. Implementasi platform-specific berada di submodule/adapter OS masing-masing untuk Windows, Linux, dan macOS; `cfg(target_os)` tidak disebarkan ke seluruh codebase.

## 7. Persyaratan non-fungsional

Responsif saat streaming/tool execution. Memory bounded untuk stream, arguments, output. Portable core pada Windows/Linux/macOS. Startup network-independent. Diagnostics actionable. Recovery tidak merusak workspace. Test coverage untuk policy, config, normalized events, persistence, transactional mutation, dan provider adapters.

## 8. Acceptance criteria MVP

- TUI warm/local baseline benchmark mencapai ≤100 ms dan tidak menunggu discovery.
- PLAN tidak mengubah filesystem.
- BUILD menghasilkan snapshot dan diff sebelum apply.
- Cancellation selalu menghasilkan `Cancelled` dan menghentikan request/tool sesuai batas.
- Provider failure independen; cached/static model tetap dapat dipilih.
- Config diagnostic menyebut lokasi kesalahan.
- Render loop tidak terblokir SQLite/network/notification.
- Batas retention dan ukuran diuji.
- Windows, Linux, macOS build/test matrix berjalan.
