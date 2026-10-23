# 1. Rust, Iced, and ports and adapters

- Status: accepted
- Date: 2026-10-23

## Context

Focus blocks sites and apps on Windows and KDE Plasma (Wayland) without admin rights, works fully offline, and has to resist casual bypassing: closing the UI must not end a session. It talks to several outside things: the browser, the OS, Google Calendar, Todoist and local files. More will come later (GNOME, X11, macOS, Firefox, SQLite insights). Those need to be added without rewriting the logic, and the logic needs to be testable without a real OS or network.

## Decision

- **Language: Rust**, in one Cargo workspace (edition 2024, stable toolchain). It gives a single native binary per process, no runtime to install, and good Windows (`windows-rs`) and Linux (`zbus`, `nix`) bindings.
- **UI: Iced.** It's native and pure Rust. Tauri and Electron are ruled out because they're web wrappers. Slint is ruled out because of licence friction with MIT/Apache-2.0.
- **Processes:** a per-user `focus-daemon` owns sessions and enforcement. `focus-ui` and the browser's native messaging host `focus-nmh` talk to it over local IPC (named pipes / Unix sockets). This is why the UI can close without ending a session.
- **Ports and adapters:** `focus-core` holds all domain logic and declares the port traits (`CalendarSource`, `TaskSource`, `Enforcer`, `Notifier`, `ConfigSource`, `StateStore`, `History`, `Clock`).
  - Each adapter crate (`config-toml`, `state-json`, `integrations`, `enforce-windows`, `enforce-linux-kde`) implements one or more ports.
  - Only `focus-daemon`, the composition root, constructs adapters and injects them.
- **Core purity is enforced, not just intended.** CI runs `scripts/check-core-deps.sh`, which fails if `focus-core` has:
  - a path or git (workspace) dependency,
  - a direct dependency missing from `scripts/focus-core-allowed-deps.txt`, or
  - any known I/O crate (`tokio`, `reqwest`, `windows`, ...) anywhere in its tree.

## Consequences

- Core logic is tested with fakes and a fake `Clock`. Every port implementation, fakes included, must pass a shared contract test suite.
- New platforms and services arrive as new crates or Cargo features. `focus-core` doesn't change for them.
- Adding a dependency to `focus-core` takes a deliberate allowlist edit plus an ADR.
- Some indirection: the daemon carries the wiring, and a feature may touch a port trait, an adapter and the daemon.
- Iced is pre-1.0, so expect API churn between releases.
