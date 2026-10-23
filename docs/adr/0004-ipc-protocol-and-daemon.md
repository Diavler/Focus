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
- **Portable install:** all files live in a `data` folder next to the executables (`data/settings.toml`, `data/state.json` and its backups), not in `%APPDATA%`, `%LOCALAPPDATA%` or `~/.config`. The install folder must be writable without admin, e.g. a zip unpacked in the user's folder or `%LOCALAPPDATA%\Programs\Focus`. If it isn't (e.g. Program Files), the daemon refuses to start and says why.
- On first run the daemon writes a starter `settings.toml` and never overwrites an existing one.

**UI.** `focus-ui` is now an IPC client. It opens one connection per call and polls `Status` every second, so a restarted daemon is picked up without reconnect logic. This supersedes ADR 0002.

## Consequences

- The build plan's done-when holds: an in-process integration test starts a session over IPC, restarts the daemon and finds the session still running.
- Closing or killing the UI never ends a session.
- On Windows, the named pipe's default security lets other local users open the pipe for reading. Requests need write access, which they don't get. Tightening the pipe's security descriptor is left for hardening.
- The workspace gains `ipc-transport`, which `docs/DESIGN.md`'s folder layout doesn't list.
- Dependencies added: `interprocess` (local sockets), `tokio`, `tracing` and `tracing-subscriber` (developer diagnostics), and `anyhow` (binaries only). `deny.toml` now allows `0BSD`, which `interprocess`'s helper crates use. No platform-directories crate is needed because of the portable layout.
- Upgrading means replacing the executables and keeping `data/`. Installers in step 9 must preserve it.
- Linux `.deb`/`.rpm` packages install to read-only `/usr/bin`, which conflicts with the portable layout. Step 9 decides between per-user data for those packages only and shipping only the portable archive.
- A few things can't live in the folder: the named pipe (not a file), and the registry entries for autostart (step 6) and native messaging (step 4) under `HKCU` on Windows. The native-messaging manifest file itself stays in the install folder on Windows. Linux browsers only look in their own config folder, so step 4 adds one small manifest there.
- When run with `cargo run`, the files land in `target/<profile>/data`, and `cargo clean` wipes them.
- Polling once a second costs two local connections a second while the window is open. Push updates can replace this if it ever matters.
