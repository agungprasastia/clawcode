# M8-02 Platform Adapters Design

## Goal

Add minimal cross-platform adapters required by the PRD: clipboard access and shell executable discovery. Keep credential-store integration explicit but unsupported in MVP.

## Scope

- Clipboard read/write contract.
- Shell discovery through `PATH`; discovery never executes a command.
- Credential-store contract returning an explicit unsupported diagnostic.
- OS-specific implementations isolated under one platform adapter boundary.
- Bounded inputs and actionable errors.
- Tests use fakes and temporary executable files; tests do not access real credentials.

## Architecture

Add a platform adapter module with three focused interfaces:

- `Clipboard`: `read` and `write` operations returning bounded text or a platform error.
- `ShellDiscovery`: resolve an executable name from supplied `PATH` entries and return its path without execution.
- `CredentialStore`: lookup contract reserved for a later milestone; MVP returns `Unsupported`.

The public adapter contracts remain platform-neutral. `cfg(target_os)` stays inside the platform implementation module. The rest of the crate depends only on the contracts and errors.

Clipboard implementations use native OS commands available on the target platform where practical. Process creation remains outside the render loop and failures are returned, never panicked. Clipboard payloads use a fixed maximum size.

Shell discovery uses `PATH` parsing and filesystem metadata only. It accepts executable names, rejects empty or path-traversal-like discovery requests, and checks candidate files without launching them. Windows executable extensions follow the host `PATHEXT` behavior; Unix candidates must be regular executable files.

Credential-store lookup returns `Unsupported` with a stable diagnostic. No secret is read, logged, or persisted.

## Error Handling

All adapter failures use a typed error with categories for invalid input, unavailable backend, unsupported platform, and I/O/process failure. Callers can display diagnostics without exposing payloads or secrets.

Clipboard read rejects output beyond the configured byte limit. Clipboard write rejects oversized input before spawning a backend process. Shell discovery returns `NotFound` when no candidate resolves.

## Testing

- Contract tests for clipboard size limits and fake success/failure behavior.
- Shell discovery tests with temporary directories and executable marker files; verify no command execution.
- Credential-store test verifies explicit `Unsupported` result.
- Platform command construction tests avoid invoking real desktop services.
- Full validation remains `cargo fmt`, strict Clippy, all tests, and `git diff --check`.

## Acceptance Criteria

- Clipboard and shell discovery APIs compile on Windows, Linux, and macOS targets.
- Shell discovery never executes discovered binaries.
- Oversized clipboard payloads fail before backend invocation.
- Credential store returns explicit unsupported status without handling secrets.
- Adapter failures do not panic or alter unrelated conversation results.
- OS-specific code is isolated in the platform adapter module.
