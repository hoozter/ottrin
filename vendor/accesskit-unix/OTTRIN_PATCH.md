# Ottrin AT-SPI bus resilience

Origin: crates.io accesskit_unix 0.17.2, AccessKit commit
`5801c4a49ff9b2c9ec3f77b3c31272d0da4c5cd3`, checksum
`301e55b39cfc15d9c48943ce5f572204a551646700d0e8efa424585f94fec528`.

Keep a missing, stale, or disconnecting D-Bus/AT-SPI bus from aborting the
process. Upstream `src/context.rs` ended the worker with
`run_event_loop(...).await.unwrap()`, so any connection-level error that was
not exactly `BrokenPipe` — for example `zbus::Error::InputOutput` errno 2
(ENOENT, "socket missing") when the configured AT-SPI bus socket does not
exist — aborted the whole application (SIGABRT) at startup or on disconnect.
The event loop now reports these errors on stderr, disables the affected bus
(`atspi_bus = None`, adapters deactivated), and keeps running while the session
status connection remains available. A later `IsEnabled` change then retries
the normal registration path. Session-bus address, connection and status-proxy
initialization failures log and end the worker cleanly. A closed status stream
also deactivates adapters and ends the worker rather than spinning; restoring
accessibility after that terminal session connection failure requires an
application restart. This patch does not implement session-bus reconnection.
Adapter channel sends already ignore a closed
receiver (`try_send` for `async-io`, `send` for `tokio` in `adapter.rs`),
so an exited worker cannot panic later callers through channel sends, and the
unbounded channel is drained (bus operations no-op while disabled) so it does
not grow. No retry timers or reconnect framework were added;
`map_or_ignoring_broken_pipe` semantics are unchanged.

Focused regression tests in the vendored crate cover the
ENOENT-vs-BrokenPipe classification and message processing with the bus
disabled.

Remove this patch when the supported egui dependency graph upgrades to an
accesskit_unix release that hardens these paths. The 0.18+ releases require
the newer accesskit core APIs and are incompatible with the pinned
accesskit 0.21.1 / egui 0.33 graph, and `accesskit_winit` 0.29.2 pins
`accesskit_unix = "0.17.2"` exactly. The crate is the sole Linux AT-SPI
backend.
