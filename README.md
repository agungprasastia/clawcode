# Clawcode

```text
  ____ _                         _      
 / ___| | __ ___      _____ ___   __| | ___ 
| |   | |/ _` \ \ /\ / / __/ _ \ / _` |/ _ \
| |___| | (_| |\ V  V / (_| (_) | (_| |  __/
 \____|_|\__,_| \_/\_/ \___\___/ \__,_|\___|
```

[![Rust](https://img.shields.io/badge/rust-1.98.0%2B-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue.svg?style=flat-square)](https://doc.rust-lang.org/edition-guide/)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen.svg?style=flat-square&logo=githubactions)](.github/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-informational.svg?style=flat-square)]()
[![Runtime](https://img.shields.io/badge/runtime-sync%20worker%20(zero--async)-success.svg?style=flat-square)]()
[![License](https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue.svg?style=flat-square)](LICENSE)

**Clawcode** is a high-performance terminal AI coding assistant built in Rust. Designed for developers with a keyboard-first philosophy, zero-async runtime overhead, safe transactional file mutations, side-by-side visual diff inspection, and session persistence powered by SQLite (WAL mode).

---

## Terminal UI Preview

```text
┌───────────────────────────────────────────────────────────────────────────┐
│ clawcode v0.1.0 │ git:main │ provider:anthropic │ model:claude-3-7-sonnet │
├─────────────────────────────────────────────┬─────────────────────────────┤
│ TRANSCRIPT                                  │ TASK PLAN                   │
│                                             │                             │
│ User: Update authentication middleware      │ [x] Inspect src/auth.rs     │
│                                             │ [>] Validate token expiry   │
│ Assistant: Adding token verification...     │ [ ] Verify unit tests       │
│                                             │                             │
│ • Edit src/auth.rs (+2 -1)                  │                             │
│      12   fn verify_token(t: &str) -> bool {│                             │
│      13 -     t.len() > 10                  │                             │
│      13 +     let exp = parse_expiry(t)?;   │                             │
│      14 +     exp > Utc::now().timestamp()  │                             │
│      15   }                                 │                             │
├─────────────────────────────────────────────┴─────────────────────────────┤
│ [BUILD] > Enter prompt or /command (Ctrl+X: WhichKey)       Tokens: 4.2k│
└───────────────────────────────────────────────────────────────────────────┘
```

---

## Table of Contents

- [Terminal UI Preview](#terminal-ui-preview)
- [System Architecture](#system-architecture)
- [Key Features](#key-features)
- [System Requirements](#system-requirements)
- [Installation & Quick Start](#installation--quick-start)
  - [Building from Source](#building-from-source)
  - [API Key Configuration](#api-key-configuration)
  - [Running the Application](#running-the-application)
- [System Configuration (`clawcode.jsonc`)](#system-configuration-clawcodejsonc)
- [Navigation & TUI Commands](#navigation--tui-commands)
  - [Slash Commands](#slash-commands)
  - [Keyboard Shortcuts](#keyboard-shortcuts)
  - [WhichKey Quick Menu (`Ctrl+X`)](#whichkey-quick-menu-ctrlx)
  - [Built-in Agent Profiles](#built-in-agent-profiles)
  - [Built-in Themes](#built-in-themes)
- [12 Built-in AI Agent Tools Specification](#12-built-in-ai-agent-tools-specification)
- [BUILD Mode Transactional Workflow](#build-mode-transactional-workflow)
  - [Side-by-Side Visual Diff](#side-by-side-visual-diff)
- [Quality Assurance & Benchmarks](#quality-assurance--benchmarks)
- [Packaging & Release Distribution](#packaging--release-distribution)
- [Architecture Scope & Non-Goals](#architecture-scope--non-goals)
- [License](#license)

---

## System Architecture

Clawcode is optimized with zero external async runtime dependencies (zero `tokio` or `async-std`). All system coordination runs on a synchronous worker thread using standard Rust communication channels (`std::sync::mpsc`) and a thread-safe event bus.

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

Core Architectural Principles:
- **Zero-Async Overhead**: Eliminates coroutine/async runtime overhead for minimal latency and a compact memory footprint.
- **Atomic File Transactions**: File modifications use sibling temporary files with atomic replacement and instant rollback on interruption.
- **Strict Bounded Resources**: Enforces bounds on transcript memory (256 KiB) and stream buffers (64 KiB), strictly truncating on valid UTF-8 character boundaries.

---

## Key Features

- **Fast Initialization & Offline-First**: Instant TUI startup (≤100 ms target) without blocking network calls. Persistent local model metadata cache with stale-while-revalidate strategy and exponential backoff handling.
- **Dual Execution Modes**:
  - `PLAN`: Read-only investigation and planning mode. Safe exploration with no risk of accidental code changes or dangerous shell command execution.
  - `BUILD`: Active transactional mutation pipeline (`validate` → `snapshot` → `diff` → `policy` → `apply`) with checkpoint rollback support and SHA-256 hash validation.
- **Visual Side-by-Side Diff**: Two-column split-view diff presentation directly in the transcript for code edits (`edit_file`), complete with original line numbering and contrasting color markers.
- **Provider Agnostic**:
  - Native Anthropic Messages API adapter (`/v1/messages`).
  - OpenAI & OpenAI-compatible providers (`/chat/completions`: OpenRouter, DeepSeek, vLLM, Groq, Mistral).
  - Local offline model execution via Ollama (`/api/chat`).
- **Quiet Editorial TUI**: Ratatui-based interface focused on code readability, resize-aware rendering, and WhichKey modal navigation (`Ctrl+X`).
- **12 Built-in AI Agent Tools**: Autonomous tool suite for file hierarchy inspection, surgical text replacement, real-time web search, sandboxed shell execution, and multi-step plan tracking.
- **Protected Workspace Sandboxing**: Canonical path resolution sandboxing prevents directory traversal attacks and symlink breakouts.
- **SQLite Session Persistence**: Managed SQLite connection pool in WAL (Write-Ahead Logging) mode persists conversation transcripts, token usage metrics, and recovery snapshots.

---

## System Requirements

- **Rust**: Version `1.98.0` or newer (pinned via `rust-toolchain.toml`).
- **Operating System**: Windows 10/11, Linux (major distributions with glibc or musl), or macOS (Apple Silicon / Intel).

---

## Installation & Quick Start

### Building from Source

```bash
# 1. Clone the repository
git clone https://github.com/owner/clawcode.git
cd clawcode

# 2. Build optimized release binary
cargo build --release --locked
```

The compiled binary will be located at `target/release/clawcode` (or `target\release\clawcode.exe` on Windows).

### API Key Configuration

Configure environment variables for your chosen LLM provider:

```bash
# OpenAI or OpenAI-compatible endpoint
export OPENAI_API_KEY="sk-..."

# Anthropic Claude
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenRouter
export OPENROUTER_API_KEY="sk-or-..."

# Ollama Endpoint (optional, default: http://localhost:11434)
export OLLAMA_HOST="http://localhost:11434"
```

### Running the Application

```bash
# Launch interactive TUI
cargo run

# Check application release version
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

Clawcode supports layered configuration in JSONC format (JSON with comments and trailing commas):

1. **Global Configuration**: `~/.config/clawcode/clawcode.json` (system-wide user defaults).
2. **Project Configuration**: `.clawcode/clawcode.json` (workspace root, overrides global preferences).

### Example `clawcode.jsonc`

```jsonc
{
  "$schema": "https://clawcode.dev/schema/v1.json",
  "schema_version": 1,
  "model": "claude-3-7-sonnet-20250219",
  "endpoint": "https://api.anthropic.com/v1",

  // Plaintext API keys are rejected by the config validator.
  // Use 'env:VAR_NAME' or 'credential:KEYRING_ID' prefix
  "api_key": "env:ANTHROPIC_API_KEY",

  // Active TUI color scheme
  "theme": "clawcode-dark",

  // Custom LLM provider endpoint configuration
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
  }
}
```

> **Precise Diagnostics**: Syntax errors in configuration files report 1-based line and column coordinates for quick troubleshooting.

---

## Navigation & TUI Commands

### Slash Commands

| Command | Description |
| :--- | :--- |
| `/plan` | Switch to **PLAN** mode (read-only, safe exploration without mutations). |
| `/build` | Switch to **BUILD** mode (active transactional mutation execution). |
| `/connect [provider]` | Connect to default provider or a specific provider (`openai`, `anthropic`, `ollama`). |
| `/model <id>` | Change active model (or open model picker dialog if no parameter is provided). |
| `/models` | Open model selection dialog. |
| `/models refresh` | Force model rediscovery from provider servers. |
| `/agents` | Open agent profile selection dialog (`plan`, `build`, `review`, `compact`). |
| `/themes` | Open theme picker dialog. |
| `/theme <name>` | Switch theme directly (e.g. `/theme catppuccin`). |
| `/new <title>` | Create a new session with the specified title. |
| `/sessions` | Open session history dialog. |
| `/clear` / `/home` | Clear active transcript display on screen. |
| `/compact` | Compact conversation history transcript to conserve context tokens. |
| `/copy` | Copy full transcript of active session to system clipboard. |
| `/status` | Show runtime metrics, connection diagnostics, and token usage details. |
| `/keys` | Open keyboard shortcuts cheatsheet (`WhichKey`). |
| `/help` | Display quick reference guide for all supported commands. |
| `/exit` | Save active session to SQLite database and quit application. |

### Keyboard Shortcuts

| Key Combination | Action |
| :--- | :--- |
| `Tab` / `Shift+Tab` | Instant toggle between **PLAN** and **BUILD** modes. |
| `Ctrl+C` | Interrupt active model streaming or cancel running tool process. |
| `Ctrl+L` | Clear transcript display on screen. |
| `Ctrl+X` | Open quick shortcut menu (**WhichKey**). |
| `Ctrl+V` | Paste text from clipboard into prompt input. |
| `Esc` / `q` | Close active modal dialog / cancel current action. |
| `PageUp` / `PageDown` | Scroll transcript one full page up / down. |
| `Shift+Up` / `Shift+Down` | Scroll transcript line by line. |
| `Up` / `Down` | Navigate prompt history or selection items in modal dialogs. |
| `Enter` | Submit prompt instruction / confirm dialog selection. |

### WhichKey Quick Menu (`Ctrl+X`)

Pressing `Ctrl+X` opens the quick action menu:

| Key | Action / Dialog |
| :---: | :--- |
| `a` | Open **Agents** dialog |
| `m` | Open **Models** dialog |
| `t` | Open **Themes** dialog |
| `s` | Open runtime **Status** summary dialog |
| `p` | Switch mode to **PLAN** |
| `b` | Switch mode to **BUILD** |
| `c` | Clear chat transcript on screen |

### Built-in Agent Profiles

| Agent Profile | Default Mode | Description & Focus |
| :--- | :---: | :--- |
| **Plan Agent** | `PLAN` | Safe repository exploration, architectural analysis, and task planning. |
| **Build Agent** | `BUILD` | Autonomous code editing, tool execution, and verified mutations. |
| **Review Agent** | `PLAN` | Source code quality audits, security review, and PR evaluations. |
| **Compact Agent** | `BUILD` | Token-efficient execution with terse responses and low latency. |

### Built-in Themes

Clawcode provides 8 built-in color schemes:

| Theme ID | Theme Name | Visual Characteristics |
| :--- | :--- | :--- |
| `clawcode-dark` | **Clawcode Dark** *(Default)* | High-contrast amber & teal accents over deep charcoal background. |
| `crabcode-orange` | **Crabcode Orange** | Warm orange ember and coral reef gradients. |
| `catppuccin` | **Catppuccin Mocha** | Soothing pastel palette of mauve, sapphire, and mocha. |
| `dracula` | **Dracula** | High-contrast nocturnal purple, pink, and cyan accents. |
| `nord` | **Nord** | Cool Arctic blue, teal, and polar night palette. |
| `gruvbox` | **Gruvbox Dark** | Retro warm dark wood, cream, and orange accents. |
| `tokyo-night` | **Tokyo Night** | Metropolitan night vibes with indigo and neon blue tones. |
| `monokai` | **Monokai** | Legendary charcoal gray palette with green, yellow, and magenta accents. |

---

## 12 Built-in AI Agent Tools Specification

Clawcode's autonomous system provides 12 core tools with isolated access permissions per mode:

| Tool Name | Allowed Modes | Scope & Function |
| :--- | :---: | :--- |
| `read_file` | PLAN / BUILD | Read file contents with precise line selectors (`offset`, `limit`, inline `path:10-50`). |
| `list_dir` | PLAN / BUILD | List directory and file structure within workspace. |
| `glob_search` | PLAN / BUILD | Search files matching glob patterns (e.g. `**/*.rs`, `src/**/*.json`). |
| `grep_search` | PLAN / BUILD | Fast case-insensitive text pattern search across project files. |
| `write_file` | BUILD | Create new file or overwrite entire file content from scratch. |
| `edit_file` | BUILD | Surgical file modification via exact literal string replacement. |
| `bash` | BUILD | Execute shell commands in workspace root within sandbox bounds. |
| `websearch` | PLAN / BUILD | Real-time web search via DuckDuckGo Instant Answers. |
| `webfetch` | PLAN / BUILD | Fetch web content over HTTP/HTTPS and strip markup tags to clean text. |
| `skill` | PLAN / BUILD | Load domain-specific instruction module from `skills/` directory. |
| `question` | PLAN / BUILD | Prompt user with structured clarifying questions and predefined options. |
| `update_plan` | PLAN / BUILD | Update multi-step execution plan checklist (*pending*, *in_progress*, *completed*). |

---

## BUILD Mode Transactional Workflow

Every mutation in **BUILD** mode passes through an atomic verification pipeline:

```text
Mutation Request ──► Validate ──► Snapshot ──► Diff Review ──► Policy Check ──► Apply / Revert
```

1. **Validate**: Verifies canonical target file path to prevent directory traversal and symlink escapes outside the workspace.
2. **Snapshot**: Saves current file state into SQLite database alongside SHA-256 checksum hash to support undo/redo rollbacks.
3. **Diff Review**: Computes line-by-line modification delta before applying changes.
4. **Policy Check**: Evaluates security policies. Destructive operations or mutations outside the workspace require explicit user confirmation.
5. **Apply / Revert**: Writes modifications using a sibling temporary file and replaces atomically. If an error occurs, automatic rollback triggers immediately.

### Side-by-Side Visual Diff

When `edit_file` runs, the transcript displays a two-column split-view diff:

```text
• Edit src/main.rs (+1 -1)
      10   fn main() {               │   10   fn main() {
      11 -     // old implementation │   11 +     println!("hello world");
      12   }                         │   12   }
```

---

## Quality Assurance & Benchmarks

The entire repository is validated against strict quality standards:

```bash
# 1. Check standard code formatting
cargo fmt --all -- --check

# 2. Static linting (strict mode, zero warnings allowed)
cargo clippy --all-targets --all-features --locked -- -D warnings

# 3. Run full unit and integration test suite
cargo test --all-targets --all-features --locked

# 4. Run performance benchmarks (render latency & first-frame throughput)
cargo bench --bench performance
cargo bench --bench first_frame
```

---

## Packaging & Release Distribution

Automated build scripts compile release binary artifacts and generate SHA-256 checksum files:

- **Windows (PowerShell)**:
  ```powershell
  pwsh -File scripts/package.ps1
  ```
- **Linux / macOS (POSIX Shell)**:
  ```bash
  sh scripts/package.sh
  ```

Complete documentation on reproducible build procedures is available in [`docs/reproducible-builds.md`](docs/reproducible-builds.md).

---

## Architecture Scope & Non-Goals

Clawcode is intentionally designed as a standalone, portable, instant terminal binary. The following are deliberate non-goals:
- Separate background daemon processes.
- Third-party runtime plugin systems.
- Web client interfaces or desktop GUI wrappers.
- Custom adapter drivers for endpoints already compliant with the OpenAI protocol specification.

---

## License

Distributed under dual [MIT](LICENSE) or Apache-2.0 license terms. See `LICENSE` for details.
