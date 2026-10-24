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

The endpoint comes from `Endpoint::for_install(install_folder)`, with no `#[cfg]`, so every process of an install agrees on it regardless of environment variables:
- On Windows, the named pipe `focus-daemon-<user>`.
- Elsewhere, the socket file `<install>/data/daemon.sock`, inside the portable install. Abstract sockets, which have no permissions, are used only in tests.

Connects wait at most 3 s and requests at most 5 s, so a stuck daemon shows up as an error instead of hanging the UI.

**Single instance.** Before binding, the daemon takes an exclusive lock on `data/daemon.lock` (`File::try_lock`, which needs Rust 1.89). The OS releases it when the process dies, so there's never a stale lock. Only the lock holder binds, and `bind` replaces a socket file nobody answers on (left by a crash). Without the lock, two daemons starting at the same moment could both treat a live socket as stale. A second daemon exits before binding, reading or writing anything.

**Daemon.**
- It runs on tokio. All connections and a 1-second ticker share one `Service` behind a `std::sync::Mutex`. The service does blocking file I/O, so it only runs inside `spawn_blocking`, never on the async worker threads, and the lock is never held across an `.await`. On shutdown the service is taken out of the mutex, so a blocking call that was already queued can't save after `serve` returns.
- `StartSession` is capped at 12 hours (`SessionState::MAX_DURATION`), so a bad client can't lock the user in indefinitely.
- A tick that has nothing due returns without copying the state.
- Every change is applied to a copy of `PersistedState`, saved through `StateStore`, and only then kept, so a failed save leaves nothing half-applied.
- The settings file is re-read every 5 seconds, on `ListConfigs` and before `StartSession`. If it's invalid, the last good configs stay in effect and the problem is reported in `Status`.
- **Portable install:** all files live in a `data` folder next to the executables (`data/settings.toml`, `data/state.json` and its backups), not in `%APPDATA%`, `%LOCALAPPDATA%` or `~/.config`. The install folder must be writable without admin, e.g. a zip unpacked in the user's folder or `%LOCALAPPDATA%\Programs\Focus`. If it isn't (e.g. Program Files), the daemon refuses to start and says why.
- On first run the daemon writes a starter `settings.toml` and never overwrites an existing one.

**UI.** `focus-ui` is now an IPC client. It opens one connection per call and polls `Status` every second, so a restarted daemon is picked up without reconnect logic. This supersedes ADR 0002.

## Consequences

- The build plan's done-when holds: an in-process integration test starts a session over IPC, restarts the daemon and finds the session still running.
- Closing or killing the UI never ends a session.
- **Windows pipe hardening** (added after step 4). Pipe names are machine-wide, so another local user could create `\\.\pipe\focus-daemon-<you>` before your daemon starts. Two defences, in `ipc-transport/src/windows_security.rs` and the endpoint code that calls it (the only place outside the `enforce-*` crates and the daemon's wiring allowed `#[cfg(windows)]`):
  - The daemon creates the pipe with a protected DACL that grants only SYSTEM and the current user's SID, so other users can neither connect to it nor create instances of it.
  - Every client checks who serves the pipe: the server's process ID (`GetNamedPipeServerProcessId`, via `interprocess`), then that process's user SID. Another user's pipe is refused with `PermissionDenied`, and `bind` reports `TakenByAnotherUser` instead of mistaking it for our own daemon.
  - The pipe is named `focus-daemon-<user SID>`, so accounts with the same user name don't collide. SIDs aren't secret, so another local user can still deliberately take the name first: that's denial of service, not impersonation, and it's reported clearly. A random name kept in `data/` would prevent it; not worth it for a single-user machine.
  - If the server's owner can't be read (for example, the daemon was started as administrator), the client refuses with a message saying so. Focus never needs admin.
  - It uses `windows-sys` and `widestring`, at the versions `interprocess` already depends on. The Linux socket gets the equivalent in step 9.
  - Manual check (needs a second Windows account): start `focus-daemon` from a copy of the build as the other user first. Your own daemon should exit with "held by another user's process", and your UI should report the same.
- The workspace gains `ipc-transport`, which `docs/DESIGN.md`'s folder layout doesn't list.
- Dependencies added: `interprocess` (local sockets), `tokio`, `tracing` and `tracing-subscriber` (developer diagnostics), and `anyhow` (binaries only). `deny.toml` now allows `0BSD`, which `interprocess`'s helper crates use. No platform-directories crate is needed because of the portable layout.
- Upgrading means replacing the executables and keeping `data/`. Installers in step 10 must preserve it.
- Linux `.deb`/`.rpm` packages install to read-only `/usr/bin`, which conflicts with the portable layout. Step 10 decides between per-user data for those packages only and shipping only the portable archive.
- A few things can't live in the folder: the named pipe (not a file), and the registry entries for autostart (step 6) and native messaging (step 4) under `HKCU` on Windows. The native-messaging manifest file itself stays in the install folder on Windows. Linux browsers only look in their own config folder, so step 4 adds one small manifest there.
- When run with `cargo run`, the files land in `target/<profile>/data`, and `cargo clean` wipes them.
- Polling once a second costs two local connections a second while the window is open. Push updates can replace this if it ever matters.
