# 2. Temporary in-process demo UI

- Status: accepted (temporary; superseded once build step 3 rewires the UI)
- Date: 2026-10-23

## Context

The build plan puts the UI at step 6, after the settings and state adapters (step 2) and the daemon with its IPC (step 3). ADR 0001 says the UI talks only to `focus-daemon`, so that closing the UI never ends a session.

After step 1 the session logic exists only in `focus-core`. We want a basic UI now, to check by hand that the strictness rules feel right and that URL patterns behave as expected.

## Decision

`focus-ui` temporarily depends on `focus-core` directly and runs the session state machine in-process:

- `apps/focus-ui/src/demo.rs` holds the only stand-ins: a `SystemClock` (a `Clock` adapter) and the example configurations from `docs/DESIGN.md`, built in code instead of loaded from TOML.
- The window says it is a demo. Nothing is enforced, and closing the window ends the session.
- The rest of the UI calls only the same `SessionState` operations the daemon will expose over IPC (start, tick, request/confirm/cancel end early) plus `BlockConfig::verdict`. Swapping them for IPC calls is a local change.
- Iced is built without its `linux-theme-detection` feature, because that pulls in zbus 5, whose `zvariant` crates are flagged unmaintained (zbus 6 absorbed them). The cost is that the window doesn't follow the KDE dark/light setting.

## Consequences

- The UI can be tried out after step 1 instead of step 6.
- Until step 3, the UI breaks ADR 0001's "UI talks only to the daemon" rule. Step 3 must:
  - move `SystemClock` and the configs (by then loaded through `ConfigSource`) into `focus-daemon`,
  - replace the in-process `SessionState` with IPC calls,
  - remove `demo.rs` and the direct `focus-core` dependency if the UI no longer needs it (the UI may still use `focus-core` types that `ipc-proto` re-exports),
  - mark this ADR superseded.
- Step 6 builds on this UI rather than starting from scratch.
