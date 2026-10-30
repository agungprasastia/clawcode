# OpenCode v2.1.3 parity

Reference source: tag `v2.1.3` (superset of `v2.1.2` commit `a517687fde98a7329ada3ae644a01a9fc6c4e99e`).
Reference binary: `@opencode/cli` 2.1.3 (reverse-engineered via REA).

Parity means matching OpenCode behavior. clawcode keeps its own visual design.

## Status

| Area | Status | Implemented | Remaining |
| --- | --- | --- | --- |
| CLI command tree and help | complete | Subcommands, global flags, argument parsing, help layout, completions, invalid-flag diagnostics | Re-check new commands as they land |
| `run`, `mini`, default TUI launch | partial | Prompt execution, model selection, JSON output, directory/session/continue flags, streaming events | Attach/continue semantics, full headless parity |
| `models`, `stats`, `debug`, `session *` | complete | JSON/text output, session IDs, import/export, config/agents/paths | Minor output-shape checks |
| Auth CLI | partial | List/logout/export/import/switch over configuration-backed credentials | Interactive provider login, OAuth device flow, credential persistence |
| MCP CLI | partial | List, add, remove, enable/disable, status | OAuth login/logout, live connection handshake |
| MCP runtime | partial | Top-level and nested config shapes, lazy stdio and streamable HTTP clients, paginated tool discovery, OpenCode-style tool names, runtime schema advertisement and tool execution, per-session ACP registries, status snapshots (enabled/connected/tool count/error), explicit probe and disconnect | OAuth, server notifications/list-changed, automatic reconnect, image results |
| Plugin CLI | partial | List, add, remove from configuration | Update/check against registry versions |
| Server and HTTP API | partial | `serve`, `serve --stdio`, health, config, v1 session CRUD/message list, v2 `/api/session` CRUD/message list, prompt admission, interrupt, session fork with before/through boundaries, revert stage/clear/commit with workspace file snapshot restoration, permission request/saved routes with pending events, SSE `/api/event` with runtime event translation, OpenAPI, CORS, local web page, config MCP servers | File/image prompt parts, config-rule enforcement during tool execution, provider/model management, durable event replay; v2.1.2 defines no session share route |
| ACP | partial | NDJSON JSON-RPC transport; initialize/auth; session new/load/list/delete/resume/close/fork; workspace activation; prompt streaming; text, reasoning, and tool-call updates; cancellation; history replay; per-session modes/models; stdio and HTTP MCP attachments with advertised capabilities | Permission/form requests, image prompts, child-session updates |
| Service lifecycle | partial | Status and config get/set/unset | Start/restart/stop and background server supervision |
| Pairing | partial | Pair command and URL/config plumbing | Device-code exchange and secure credential storage |
| TUI keybindings | mostly complete | Leader, palette, escape interrupt, Ctrl+C/D context rules, Ctrl+B running-shell backgrounding, Ctrl+T variant cycling, F2/Shift+F2 recent-model cycling, Ctrl+Alt+B/F/U/D/Y/E/G + Ctrl+G + Alt+Home/End + leader u transcript scrolling, agents, sessions, tabs, pinning, rename, delete, word editing, multiline input with grow/scroll, newline bindings, paste normalization, input undo/redo | Selection bindings, external editor |
| TUI session tabs | complete | Open/switch/cycle/close/reopen, quick slots, cached titles, tab bar for multiple tabs, rename propagation | Unread markers, tab history |
| TUI terminal pane | complete | Embedded bottom shell pane (`Ctrl+X t`, `/terminal`, `/term <cmd>`), focus cycling (`Ctrl+X Up/Down`, `Esc`, mouse click), built-in `cd`/`pwd`/`clear`/`exit`, non-blocking stdout/stderr streaming, `Ctrl+C` command interrupt, bounded 500-line / 64 KiB scrollback | Full VT100 interactive TUI programs inside pane |
| TUI session timeline | complete | Interactive `/timeline` & `Ctrl+X g` modal with turn search, `Enter` jump-to-turn, `f` fork-at-turn into a new session tab, `r` revert-to-turn, parent fork lineage header, and child fork (`⑂`) branch badges | Visual ASCII tree graph across all sessions |
| TUI undo/redo | complete | `/undo` (`/revert`) and `/redo` (`/unrevert`, `Ctrl+X r`) with prompt restoration, atomic workspace file snapshot rollback/re-apply (`write_file` / `edit_file` metadata), and `/diff` (`Ctrl+X d`) session diff review dialog with Unified/Split view toggle | Multi-branch redo tree |
| TUI model variants | complete | Built-in and custom model variant discovery, OpenAI `reasoning_effort` + Anthropic `thinking` budget serialization, `Ctrl+T` / `/variant` cycling & picker, `Favorites` (`Ctrl+F`) + `Recent` (`F2`) + provider filter (`Ctrl+A`) in `/models`, SQLite `kv_state` persistence | Provider-specific custom effort presets |
| TUI MCP/settings dialogs | mostly complete | `/mcps` and palette MCP dialog (searchable server list, eager startup probes, connect/disconnect/retry, failed-server error details with copy); `/settings` interactive editor with atomic comment-preserving persistence to `clawcode.jsonc` (`animations`, `thinking`, `grouping`, `verbosity`, `markdown`, `tps`, `turn_tokens`, `timestamps`, `scrollbar`, `scroll_speed`, `scroll_acceleration`, `mouse`, `leader_timeout_ms`) | MCP OAuth sign-in (`needs_auth`) |
| TUI images | missing | Text transcript and tool cards | Attach, preview, and image tool parts |
| TUI queued prompts / background tools | complete | Non-cancelling queue-on-submit while active, empty-submit steer promotion, `/queued` dialog (steer, undo-to-composer, discard), `/stash` prompt stash with UTF-8 bounds and double `Ctrl+D` delete confirmation, `Ctrl+B` / `/background` non-blocking foreground shell detachment with 3-second hint and `tool_background_finished` reconciliation | Multi-part attachment queueing |
| Streaming and tool cards | complete | Grouped exploration, foreground/background shell, durable parts, replay, malformed-stream handling, live `tok/s` status + configurable turn token footer, sticky scroll compensation while scrolled up + clickable `Jump to latest ↓` badge, configurable thinking/markdown/verbosity modes | New provider-specific part types |
| Providers | partial | OpenAI-compatible, Anthropic, Ollama, discovery, normalization | Full provider catalog and provider-specific auth |
| LSP | missing | Configuration passthrough only | Diagnostics, symbols, navigation, edit integration |
| Plugin runtime | missing | Configuration-backed CLI list/add/remove | Plugin loading, hooks, OpenCode plugin API |
| Web UI | minimal | Local sessions page | OpenCode v2 web application |
| Notifications | complete | Desktop, terminal bell, sound behind `Notifier` | Platform-specific polish |

## Verification

- `cargo test --all-targets --all-features --locked`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `cargo fmt --all -- --check`
- Compare help text against the installed OpenCode binary for each command family.
- Use REA on the shipped binary when source and observed behavior disagree.

Last full test run: 276 library tests and 113 TUI tests passing, plus all other integration targets. Strict Clippy and formatting checks pass.
