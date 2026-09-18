# Repository Guidelines

## Project Overview
- Root package: `clawcode` (Rust edition 2024, Rust 1.98.0 toolchain pinned via `rust-toolchain.toml`).
- Output: Single binary providing terminal UI (TUI) and non-interactive command-line interface (CLI).
- Note: Nested directory `crabcode/` is a separate reference codebase and not the root crate. Do not mix dependencies or tooling between root and `crabcode/`.

## Architecture & Data Flow
- **TUI Frontend**: Built with `ratatui` and `crossterm` using a 16ms event polling loop (~60 FPS) for responsive terminal input, rendering, and animations.
- **Worker Runtime**: Synchronous worker thread runtime coordinating events via `std::sync::mpsc` channels (`RuntimeClient`, `EventBus`). **No async runtime** (no `tokio` or `async fn` in root crate).
- **Persistence Layer**: SQLite 3-connection pool in WAL mode (`DatabaseConnectionPool`) with an append-only generation event log and bounded batch processing via `WriterHandle`.
- **Workspace Sandbox**: Enforces strict `PLAN` (read-only) and `BUILD` (mutation allowed) policies. File mutations use temporary sibling file atomic replacement with automatic rollback on failure.
- **Provider Subsystem**: `JsonProvider` streaming implementation consuming HTTP endpoints via synchronous `ureq` and processing chunks through a streaming normalizer.

## Key Directories
- `src/cli/`: Command-line interface entry points, subcommands, and non-interactive execution handlers.
- `src/tui/`: Ratatui terminal application state machine, rendering loops, input handling, and dialog modals.
- `src/runtime/`: Synchronous worker thread coordinator, runtime client, and event bus routing.
- `src/provider/`: Provider traits, model discovery, stream normalizers, registry, and usage metrics.
- `src/adapters/`: Specific LLM provider integrations (e.g. Anthropic, Ollama, OpenAI-compatible).
- `src/workspace/`: Sandbox filesystem operations, root resolution, execution policies, snapshotting, and shell execution.
- `src/conversation/`: Tool definitions, prompt formatting, context assembly, and conversation loop execution.
- `src/persistence/`: SQLite connection management, migrations, schema definitions, and background batch writer.
- `src/config/`: JSONC configuration parsing, schema validation, secrets resolution, and compatibility shims.
- `src/platform/`: Platform-specific utilities (clipboard, Git integration, credential storage, shell spawners).
- `src/core/`: Shared types, error definitions, and diagnostic categories.
- `src/notify/`: System notifications and desktop alerting integrations.
- `tests/`: Integration test suites covering runtime, workspace policies, adapters, TUI, and persistence.
- `benches/`: Criterion performance and first-frame rendering benchmarks.
- `scripts/`: Packaging and release automation scripts for Windows and POSIX environments.

## Development Commands
- **Build**: `cargo build --release --locked`
- **Run (TUI)**: `cargo run`
- **Run (CLI)**: `cargo run -- <command>` (e.g. `cargo run -- /models`, `cargo run -- /plan`, `cargo run -- --version`)
- **Lint**: `cargo clippy --all-targets --all-features --locked -- -D warnings`
- **Format**: `cargo fmt --all -- --check`
- **Test**: `cargo test --all-targets --all-features --locked`
- **Benchmarks**:
  - `cargo bench --bench performance`
  - `cargo bench --bench first_frame`
- **Packaging**:
  - Windows: `pwsh -File scripts/package.ps1`
  - POSIX: `sh scripts/package.sh`

## Code Conventions & Common Patterns
- **Error Handling**: Use explicit `Result<T, E>` types. Domain errors report structured diagnostics (`Diagnostic` in `src/core/error.rs`, `ProviderError`, `PlatformError`). Configuration parse errors must report 1-based line and column coordinates.
- **Concurrency & Threading**: Use Rust standard library primitives exclusively (`std::thread`, `std::sync::{Arc, Mutex}`, `std::sync::mpsc::sync_channel`). Never introduce `tokio`, `async fn`, or external async executors into root crate.
- **Dependency Injection**: Design systems against traits for modularity and testability (`Provider`, `Transport`, `FileSystem`, `ClipboardBackend`, `DiscoverySource`, `Notifier`).
- **State Management**: Model transitions as explicit mutable state machines (`App`, `TurnState`, `WorkspaceHistory` with undo/redo stack).
- **Buffer & Resource Limits**: Bound all buffers and stream collectors (e.g. 256 KiB transcript limit, 64 KiB provider stream coalescing buffer, truncation strictly on valid UTF-8 character boundaries).

## Important Files
- **Application Entry**: `src/main.rs`, `src/lib.rs`
- **CLI & TUI**: `src/cli/mod.rs`, `src/tui/app.rs`, `src/tui/render.rs`
- **Runtime**: `src/runtime/client.rs`, `src/runtime/mod.rs`
- **Provider & Adapters**: `src/provider/traits.rs`, `src/adapters/mod.rs`
- **Workspace & Tools**: `src/workspace/mod.rs`, `src/conversation/tools.rs`
- **Persistence**: `src/persistence/db.rs`, `src/persistence/writer.rs`
- **Configuration**: `config/schema.json`, `src/config/mod.rs`

## Runtime/Tooling Preferences
- **Toolchain**: Pinned Rust 1.98.0 (`rust-toolchain.toml`).
- **Package Management**: Use `cargo` as the sole package and build manager for the root project.
- **No Node/Bun in Root**: Never invoke `node`, `npm`, `bun`, or Javascript package managers in the root crate. Bun runtime scripts belong strictly to nested `crabcode/`.

## Testing & QA
- **Suite Execution**: Run `cargo test --all-targets --all-features --locked` for integration (`tests/*.rs`) and unit tests.
- **Strict Linting**: Clippy runs with `-D warnings`. Never add `#[allow(...)]` attributes to bypass warnings; fix the root cause instead.
- **Deterministic Testing**: Use in-memory SQLite instances (`:memory:`) and mock trait implementations (`MockFileSystem`, `MockTransport`) rather than touching live external systems or network endpoints.

<!-- antislop:start -->
## antislop
For UI, copy, people, mobile layout, or code comments work, load the antislop skill for the task:
- Core filter, always on: `antislop`
- Copy & text: `antislop-copywriting`
- People: `antislop-human`
- Code comments: `antislop-code`
- UI / visual: `antislop-ui`
Before starting, ask the user when antislop applies: during the work, or after it is done.
<!-- antislop:end -->
