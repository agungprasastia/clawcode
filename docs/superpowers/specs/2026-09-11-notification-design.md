# M8-01 Notification Design

## Goal

Provide terminal bell, desktop notification, and sound as best-effort side effects. Notification failure must never alter conversation, tool, or persistence results.

## Architecture

Add `notify` module with a small `Notifier` trait, `Notification` payload, `NotificationKind`, and `NotificationReport`. `BestEffortNotifier` dispatches enabled backends independently. Terminal, desktop, and sound implementations live behind platform adapters; `cfg(target_os)` stays inside `src/notify/platform.rs`.

The default path is non-blocking from the TUI/render loop: callers enqueue or spawn notification work, and backend failures are reported through tracing and `NotificationReport`, not propagated into the successful operation. Tests use deterministic fake backends.

## Behavior

- Terminal bell writes the terminal bell sequence when enabled.
- Desktop and sound adapters are best-effort and may report unsupported/failure.
- One backend failure does not prevent other backends from running.
- Notification payloads are bounded and contain no secret/provider data.
- Conversation result remains unchanged for all notification outcomes.
- No notification work runs during startup critical path.

## Testing

Tests cover all backends succeeding, each backend failing independently, bounded payloads, disabled backends, and integration proving a successful conversation remains successful when notification fails.
