# 4. IPC protocol, transport and daemon structure

- Status: accepted
- Date: 2026-10-23

## Context

ADR 0001 puts sessions in a per-user daemon that the UI and the native messaging host reach over local IPC. Build step 3 needs:

- a wire format the UI, the daemon and later `focus-nmh` agree on, which can evolve;
- a per-user endpoint that other users can't reach;
- a single daemon per user;
- a session that survives a daemon restart.

## Decision

**Messages (`ipc-proto`, no I/O).**
- Each message is one line of JSON: `{"version":1,"message":{"type":"status"}}`.
- A client sends a `Request` and reads exactly one `Response`, in order, on the same connection.
- `version` is checked before the message is parsed. A different version gets `UnsupportedVersion` instead of "malformed", and any incompatible change bumps `PROTOCOL_VERSION`.
- Requests: `Status`, `ListConfigs`, `StartSession`, `EndSession` plus `ConfirmEndSession` / `CancelEndSession` (Deep needs a second step), `Heartbeat` and `RulesFor`.
- Core types (`SessionState`, `BlockConfig`, `Verdict`) cross the wire as their serde form, so their serde shape is part of the protocol.

**Transport: a new crate, `ipc-transport`.** It holds endpoints, line framing (max 1 MiB per line) and a typed `Client`. It's separate so `ipc-proto` stays pure and the UI and NMH don't depend on the daemon.

The endpoint comes from `Endpoint::for_current_user()`, with no `#[cfg]`:
- If `XDG_RUNTIME_DIR` is set (Linux desktops), a socket file in that directory, which only the user can open.
- Otherwise (Windows), the named pipe `focus-daemon-<user>`.

**Single instance.** `bind` refuses if a connect to the endpoint already succeeds. On Windows the first pipe instance is also exclusive. On Linux a stale socket file left by a crash is replaced. The daemon binds before it reads any state, so a second daemon exits without touching anything.

**Daemon.**
- It runs on tokio. All connections and a 1-second ticker share one `Service` behind a `std::sync::Mutex`, which is held only for the synchronous handling of a request, never across an `.await`.
- Every change is applied to a copy of `PersistedState`, saved through `StateStore`, and only then kept, so a failed save leaves nothing half-applied.
- The settings file is re-read every 5 seconds, on `ListConfigs` and before `StartSession`. If it's invalid, the last good configs stay in effect and the problem is reported in `Status`.
- File locations come from `APPDATA`/`LOCALAPPDATA` (Windows) or `XDG_CONFIG_HOME`/`XDG_STATE_HOME`, falling back to `~/.config` and `~/.local/state` (Linux).
- On first run the daemon writes a starter `settings.toml` and never overwrites an existing one.

**UI.** `focus-ui` is now an IPC client. It opens one connection per call and polls `Status` every second, so a restarted daemon is picked up without reconnect logic. This supersedes ADR 0002.

## Consequences

- The build plan's done-when holds: an in-process integration test starts a session over IPC, restarts the daemon and finds the session still running.
- Closing or killing the UI never ends a session.
- On Windows, the named pipe's default security lets other local users open the pipe for reading. Requests need write access, which they don't get. Tightening the pipe's security descriptor is left for hardening.
- The workspace gains `ipc-transport`, which `docs/DESIGN.md`'s folder layout doesn't list.
- Dependencies added: `interprocess` (local sockets), `tokio`, `tracing` and `tracing-subscriber` (developer diagnostics), and `anyhow` (binaries only). `deny.toml` now allows `0BSD`, which `interprocess`'s helper crates use. `directories` was dropped because it pulls in an MPL-2.0 crate.
- Polling once a second costs two local connections a second while the window is open. Push updates can replace this if it ever matters.
