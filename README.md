<div align="center">

![Clawcode](assets/clawcode-logo.svg)

[![Rust](https://img.shields.io/badge/rust-1.98.0%2B-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue.svg?style=flat-square)](https://doc.rust-lang.org/edition-guide/)
[![CI](https://github.com/agungprasastia/clawcode/actions/workflows/ci.yml/badge.svg)](https://github.com/agungprasastia/clawcode/actions/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-informational.svg?style=flat-square)](#system-requirements)
[![Runtime](https://img.shields.io/badge/runtime-fully%20synchronous-success.svg?style=flat-square)](#system-architecture)
[![License](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](LICENSE)

**A terminal-native AI coding assistant, built in Rust.**

</div>

Clawcode pairs a keyboard-first interface with a fully synchronous runtime, transactional file mutations with automatic rollback, an inline side-by-side diff viewer, and durable session history backed by SQLite in WAL mode.

---

## System Architecture

Clawcode carries no external async runtime dependency. Every subsystem is coordinated from a single-threaded UI event loop plus a pool of synchronous worker threads, communicating exclusively through standard-library channels (`std::sync::mpsc`) and a lightweight, thread-safe event bus.

```text
┌─────────────────────────────────────────────────────────────┐
│                    Terminal Frontend (TUI)                  │
│       Ratatui + Crossterm  •  16ms Event Polling (~60 FPS)   │
└──────────────────────────────┬──────────────────────────────┘
                               │ EventBus / std::sync::mpsc
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                   Synchronous Worker Runtime                │
│       RuntimeClient  •  State Machine  •  Task Coordinator   │
└──────────────┬───────────────────────┬──────────────────────┘
               │                       │
      ┌────────┴────────┐     ┌────────┴────────┐     ┌───────────────────────┐
      │  Workspace      │     │  Persistence    │     │  Provider Subsystem   │
      │  Sandbox        │     │  Layer          │     │  (Sync ureq + SSE)    │
      ├─────────────────┤     ├─────────────────┤     ├───────────────────────┤
      │ • Path Guard    │     │ • SQLite WAL    │     │ • OpenAI / OpenRouter │
      │ • SHA-256 Snap  │     │ • 3-Conn Pool   │     │ • Anthropic Native    │
      │ • Atomic Replace│     │ • Batch Writer  │     │ • Ollama Local        │
      │ • Policy Engine │     │ • Event Log     │     │ • Stream Normalizer   │
      └─────────────────┘     └─────────────────┘     └───────────────────────┘
```

Core architectural principles:
- **Zero-async overhead**: no coroutine or async-executor scheduling cost, keeping latency low and memory usage predictable.
- **Atomic file transactions**: every write goes through a sibling temporary file and an atomic rename, with automatic rollback if the write is interrupted.
- **Isolated persistence connections**: the runtime, background writer, and CLI command service each own a dedicated SQLite connection opened against the same WAL-mode database file, avoiding lock contention on the render loop.
- **Strict bounded resources**: transcript memory and provider stream buffers are hard-capped, with all truncation performed on valid UTF-8 character boundaries.

---

## Key Features

- **Fast, offline-first startup**: the TUI is designed to render its first frame quickly, with no blocking network calls at launch.
- **Dual execution modes**:
  - `PLAN`: read-only investigation and planning. File writes are rejected outright, and shell commands that would mutate the workspace are denied.
  - `BUILD`: the active mutation pipeline (validate → snapshot → diff → policy → apply), with checkpointed undo/redo and SHA-256 integrity checks on every snapshot.
- **Inline side-by-side diff**: file edits made through `edit_file` render as a two-column split view directly in the transcript, with original line numbers and clear add/remove markers.
- **Provider-agnostic by design**: a native Anthropic Messages API adapter, an OpenAI-compatible adapter usable with any endpoint that speaks the `/chat/completions` protocol (OpenAI, OpenRouter, Groq, DeepSeek, and others), and a local Ollama adapter for fully offline inference.
- **Quiet, editorial interface**: a Ratatui-based UI focused on code readability, resize-aware layout, and a discoverable `WhichKey` shortcut menu (`Ctrl+X`).
- **Built-in autonomous tool suite**: file inspection, surgical text editing, live web search and fetch, sandboxed shell execution, a project skill library, and multi-step plan tracking, all callable by the assistant.
- **Sandboxed workspace access**: canonical path resolution rejects directory traversal and symlink escapes outside the project root.
- **Session persistence and recovery**: an SQLite database in WAL mode stores conversation transcripts, tool call history, and generation state, and automatically recovers sessions that were interrupted mid-turn.
- **Interactive Git panel**: a `/git` dialog surfaces working-tree status and diffs without leaving the TUI.
- **Skill library**: reusable, project-scoped instruction snippets can be loaded into the conversation with `/skill <name>`.

---

## System Requirements

- **Rust**: version `1.98.0` or newer (pinned via `rust-toolchain.toml`).
- **Operating system**: Windows 10/11, Linux (major distributions, glibc or musl), or macOS (Apple Silicon or Intel).

---

## Installation & Quick Start

### Building from Source

```bash
# 1. Clone the repository
git clone https://github.com/agungprasastia/clawcode.git
cd clawcode

# 2. Build the optimized release binary
cargo build --release --locked
```

The compiled binary is placed at `target/release/clawcode` (or `target\release\clawcode.exe` on Windows).

### API Key Configuration

Set the environment variable that matches your chosen provider:

```bash
# OpenAI or an OpenAI-compatible endpoint
export OPENAI_API_KEY="sk-..."

# Anthropic Claude
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenRouter
export OPENROUTER_API_KEY="sk-or-..."

# Ollama endpoint (optional, default: http://localhost:11434)
export OLLAMA_HOST="http://localhost:11434"
```

### Running the Application

```bash
# Launch the interactive TUI
cargo run

# Print the release version
cargo run -- --version

# Run non-interactive commands directly (CLI mode)
cargo run -- /models
cargo run -- /models refresh
cargo run -- /plan
cargo run -- /build
cargo run -- /connect anthropic
cargo run -- /new "Refactor Middleware"
```

---

## System Configuration (`clawcode.jsonc`)

Clawcode reads layered configuration in JSONC format (JSON with comments):

1. **Global configuration**: `~/.config/clawcode/config.jsonc` (user-wide defaults, resolved through the platform's XDG config directory).
2. **Project configuration**: `.clawcode/config.jsonc` at the workspace root, which overrides matching global values.

### Example `clawcode.jsonc`

```jsonc
{
  "schema_version": 1,
  "model": "claude-3-7-sonnet-20250219",
  "endpoint": "https://api.anthropic.com/v1",

  // Plaintext API keys are rejected by the config validator.
  // Use an 'env:VAR_NAME' or 'credential:KEYRING_ID' prefix instead.
  "api_key": "env:ANTHROPIC_API_KEY",

  // Custom or additional provider endpoints
  "providers": {
    "openrouter": {
      "base_url": "https://openrouter.ai/api/v1",
      "api_key": "env:OPENROUTER_API_KEY",
      "models": {
        "anthropic/claude-3.5-sonnet": {
          "context_window": 200000,
          "max_output_tokens": 8192
        }
      }
    }
  },

  // Per-agent overrides (plan, build, review, compact)
  "agents": {
    "review": {
      "model": "claude-3-7-sonnet-20250219",
      "temperature": 0
    }
  }
}
```

> **Precise diagnostics**: configuration syntax errors report 1-based line and column coordinates to make troubleshooting fast.

---

## Navigation & TUI

### Slash Commands

| Command | Description |
| :--- | :--- |
| `/plan` | Switch to **PLAN** mode (read-only, safe exploration). |
| `/build` | Switch to **BUILD** mode (active transactional execution). |
| `/connect [provider]` | Connect to the default provider or a specific one (`openai`, `anthropic`, `ollama`, `openrouter`, `groq`, `deepseek`, `gemini`). |
| `/model <id>` | Change the active model, with autocomplete suggestions as you type. |
| `/models` | Open the model selection dialog. |
| `/models refresh` | Force model rediscovery from the provider. |
| `/agents` | Open the agent profile dialog (`plan`, `build`, `review`, `compact`). |
| `/themes` | Open the theme picker dialog. |
| `/theme <name>` | Switch theme directly (e.g. `/theme catppuccin`). |
| `/git` | Open the interactive Git status and diff dialog. |
| `/skills` | Open the skill library picker. |
| `/skill <name> [prompt]` | Load a named skill's instructions into the conversation, optionally with an extra prompt. |
| `/new <title>` | Create a new session with the given title. |
| `/sessions` | Open the session history dialog. |
| `/clear` / `/home` | Clear the transcript on screen. |
| `/compact` | Compact the conversation transcript to conserve context tokens. |
| `/copy` | Copy the active transcript (or current status, if empty) to the system clipboard. |
| `/status` | Show runtime metrics, connection diagnostics, and token usage. |
| `/keys` | Open the keyboard shortcuts cheatsheet (`WhichKey`). |
| `/help` | Display a quick reference of all supported commands. |
| `/exit` | Save the active session and quit the application. |

### Keyboard Shortcuts

| Key Combination | Action |
| :--- | :--- |
| `Tab` / `Shift+Tab` | Toggle between **PLAN** and **BUILD** modes. |
| `Ctrl+C` | Cancel the active turn or running tool call. |
| `Ctrl+L` | Clear the transcript on screen. |
| `Ctrl+X` | Toggle the quick shortcut menu (**WhichKey**). |
| `Ctrl+V` | Paste text from the clipboard into the prompt input. |
| `Esc` | Dismiss the active dialog or panel (with no modal open, this quits the app). |
| `PageUp` / `PageDown` | Scroll the transcript one page up / down. |
| `Shift+Up` / `Shift+Down` (or `Ctrl+`/`Alt+` + arrow) | Scroll the transcript line by line. |
| `Up` / `Down` | Navigate prompt history, or move the selection inside dialogs. |
| `Home` / `End` | Jump to the start / end of the input line. |
| `Enter` | Submit the prompt, or confirm the selected dialog item. |

### WhichKey Quick Menu (`Ctrl+X`)

Pressing `Ctrl+X` opens a quick action popup; the next key press is routed straight to the matching action.

| Key | Action |
| :---: | :--- |
| `a` | Open the **Agents** dialog. |
| `t` | Open the **Themes** dialog. |
| `m` | Open the **Models** dialog. |
| `s` | Open the runtime **Status** dialog. |
| `r` | Open the **Sessions** dialog. |
| `p` | Switch mode to **PLAN**. |
| `b` | Switch mode to **BUILD**. |
| `c` | Clear the chat transcript on screen. |

### Built-in Agent Profiles

| Agent Profile | Default Mode | Focus |
| :--- | :---: | :--- |
| **Plan Agent** | `PLAN` | Safe repository exploration, architectural analysis, and task planning. |
| **Build Agent** | `BUILD` | Autonomous code editing, tool execution, and verified mutations. |
| **Review Agent** | `PLAN` | Source code quality audits, security review, and PR readiness checks. |
| **Compact Agent** | `BUILD` | Terse output, minimal token usage, and low-latency execution. |

### Built-in Themes

| Theme ID | Theme Name | Visual Characteristics |
| :--- | :--- | :--- |
| `clawcode-dark` | **Clawcode Dark** *(Default)* | Amber and teal accents on a deep charcoal background. |
| `catppuccin` | **Catppuccin Mocha** | Pastel mauve, sapphire, and mocha surface tones. |
| `dracula` | **Dracula** | Vibrant purple, pink, and cyan gothic palette. |
| `nord` | **Nord** | Arctic frost blue, teal, and polar night tones. |
| `gruvbox` | **Gruvbox Dark** | Retro warm yellow, aqua, and earthy brown accents. |
| `tokyo-night` | **Tokyo Night** | Midnight blue with neon cyan and magenta highlights. |
| `monokai` | **Monokai** | Vivid yellow, green, and retro charcoal palette. |

---

## Built-in AI Agent Tools

Clawcode exposes 12 tools to the assistant, with access to mutating tools gated by the active mode:

| Tool Name | Allowed Modes | Scope & Function |
| :--- | :---: | :--- |
| `read_file` | PLAN / BUILD | Read file contents with pagination or inline selectors (`path:10-50`, `path:10+20`, `path:-40`, `path:raw`). |
| `list_dir` | PLAN / BUILD | List directory contents within the workspace. |
| `glob_search` | PLAN / BUILD | Find files matching a glob pattern (e.g. `**/*.rs`). |
| `grep_search` | PLAN / BUILD | Case-insensitive text search across project files. |
| `write_file` | BUILD | Create a new file. Blocked in PLAN mode. |
| `edit_file` | BUILD | Modify an existing file via exact literal substring replacement. Blocked in PLAN mode. |
| `bash` | PLAN / BUILD | Run a shell command in the workspace root; read-only commands are allowed in PLAN, mutating commands require BUILD mode and, depending on policy, explicit approval. |
| `websearch` | PLAN / BUILD | Search the web for documentation, references, or error solutions. |
| `webfetch` | PLAN / BUILD | Fetch a web page by URL and return readable content. |
| `skill` | PLAN / BUILD | Load a named instruction module from the project's skill library. |
| `question` | PLAN / BUILD | Ask the user a clarifying question, optionally with predefined options. |
| `update_plan` | PLAN / BUILD | Update the structured multi-step execution plan (`pending`, `in_progress`, `completed`). |

---

## BUILD Mode Transactional Workflow

Every mutation submitted in **BUILD** mode passes through an atomic pipeline:

```text
Mutation Request ──► Validate ──► Snapshot ──► Diff Review ──► Policy Check ──► Apply / Revert
```

1. **Validate**: resolves the target path against the workspace root, rejecting directory traversal and symlink escapes.
2. **Snapshot**: captures the file's current and proposed state along with a SHA-256 checksum, recorded on an undo/redo stack.
3. **Diff Review**: computes the line-by-line change before anything is written.
4. **Policy Check**: evaluates the operation; overwriting an existing file or running a mutating shell command may require explicit user approval, and disallowed operations are denied outright.
5. **Apply / Revert**: writes through a sibling temporary file and an atomic rename. If any mutation in the batch fails, previously applied changes in that batch are rolled back automatically.

### Side-by-Side Visual Diff

When `edit_file` runs, the transcript renders a two-column split view of the change:

```text
• Edit src/main.rs (+1 -1)
      10   fn main() {               │   10   fn main() {
      11 -     // old implementation │   11 +     println!("hello world");
      12   }                         │   12   }
```

---

## Quality Assurance & Benchmarks

The repository is validated against the following commands:

```bash
# 1. Check code formatting
cargo fmt --all -- --check

# 2. Static linting (strict mode, zero warnings allowed)
cargo clippy --all-targets --all-features --locked -- -D warnings

# 3. Run the full unit and integration test suite
cargo test --all-targets --all-features --locked

# 4. Run performance benchmarks (render latency & first-frame throughput)
cargo bench --bench performance
cargo bench --bench first_frame
```

---

## Packaging & Release Distribution

Build scripts compile release binary artifacts and generate SHA-256 checksum files:

- **Windows (PowerShell)**:
  ```powershell
  pwsh -File scripts/package.ps1
  ```
- **Linux / macOS (POSIX shell)**:
  ```bash
  sh scripts/package.sh
  ```

Reproducible build documentation is available in [`docs/reproducible-builds.md`](docs/reproducible-builds.md).

---

## Architecture Scope & Non-Goals

Clawcode is intentionally designed as a standalone, portable terminal binary. The following are deliberate non-goals:
- A separate background daemon process.
- A third-party runtime plugin system.
- A web client interface or desktop GUI wrapper.
- Custom adapters for endpoints that are already compliant with the OpenAI protocol; the generic OpenAI-compatible adapter handles those.

---

## License

Clawcode is distributed under the [MIT License](LICENSE).
