# Performance Baseline

Date: 2026-09-12
Platform: Windows development machine
Command: `cargo bench --bench performance -- --noplot`

## Measurements

| Benchmark | Baseline |
| --- | ---: |
| `startup_local_app_and_first_render` | 470.4 µs median |
| `stream_coalescing_1024_deltas` | 72.4 µs median |
| `sqlite_writer_batch_64_messages` | 747.8 µs median |
| `redraw_120x40_with_bounded_stream` | 1.70 ms median |

## Scope

- Startup benchmark is local only: `App::default()` plus first `TestBackend` render. It performs no network discovery.
- Stream benchmark exercises bounded provider delta coalescing for 1,024 deltas.
- SQLite benchmark exercises 64 messages through the asynchronous writer and in-memory SQLite.
- Redraw benchmark processes 2,048 synthetic deltas through `UiEventQueue` and renders at `120x40`.
- Memory acceptance is represented by existing hard limits: UI coalesced stream bytes are capped at 64 KiB and message payloads at 256 KiB.

## Acceptance Notes

The PRD startup target is ≤100 ms for warm/local startup. This local render benchmark measured 470.4 µs. Criterion output remains the source of truth for startup measurement on each baseline machine. Network discovery is not part of benchmark or startup critical path. Provider-side pending delta accumulation still needs an explicit bound before claiming full stream memory-budget compliance.

Run the benchmark after changes affecting TUI, provider streaming, persistence, or event queues. Compare Criterion reports rather than treating one machine's absolute numbers as portable across operating systems.
