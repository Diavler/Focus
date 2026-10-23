# CLAUDE.md

Offline-first focus app (Rust, Iced) for Windows and KDE Plasma on Wayland. It blocks distracting sites and apps during focus sessions driven by Google Calendar and Todoist.

- Design: `docs/DESIGN.md` (read it before any non-trivial change; it wins over assumptions)
- Work plan: `docs/BUILD_PLAN.md` (do one step at a time; don't start the next step unless asked)
- Decisions: `docs/adr/` (add an ADR when you make or change an architectural decision)

## Hard constraints

- **No admin or root, ever.** Everything runs as the current user. Never write to HKLM, `/etc`, system services or the hosts file.
- **Offline-first.** The app must work fully with no network. No accounts, no server.
- **Native UI only.** Iced. No webview, Tauri or Electron.
- **`focus-core` has no I/O.** No OS calls, network, filesystem, `tokio` or other workspace crates in it. It owns the port traits.
- **Optional modules stay optional.** Insights, classifier and online code live behind Cargo features and never become dependencies of the core.
- **Browser extension is required** and must stay browser-neutral (Chromium first, Firefox later).

## SOLID in this codebase

- **S:** one reason to change per crate and module. Rule matching, session state, scheduling and IPC live apart. If a module needs "and" to describe it, split it.
- **O:** add behaviour with a new adapter crate or feature flag, not by editing `focus-core`. Prefer `enum` + `match` inside the core for closed sets (rule actions, strictness) and traits at the boundary for open sets (platforms, services).
- **L:** every implementation of a port, fakes included, must pass the shared contract tests for that port (exposed from `focus-core` behind a `testing` Cargo feature and run from each adapter's tests).
- **I:** keep traits small and focused (`CalendarSource`, `TaskSource`, `Enforcer`, `Notifier`, `ConfigSource`, `StateStore`, `History`, `Clock`). Don't add methods a caller doesn't need; add a new trait instead.
- **D:** the core depends only on its traits. Concrete adapters are constructed in `focus-daemon` (the composition root) and injected. No global state, no singletons, no `lazy_static` config.

## Rust conventions

- Edition 2024, stable toolchain. `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass before you say a task is done.
- Errors: `thiserror` enums in library crates, `anyhow` only in binaries. No `unwrap()`/`expect()` outside tests unless the invariant is commented.
- Time comes from the `Clock` port, never `SystemTime::now()` in the core, so session logic is testable.
- Persisted files (state JSON) are written atomically: temp file, then rename.
- Public items get a one-line doc comment. Keep functions small; prefer pure functions in the core.
- Platform code uses `#[cfg(windows)]` / `#[cfg(target_os = "linux")]` only inside the `enforce-*` crates and the daemon's wiring.
- Don't add a dependency without saying why in your summary; prefer well-maintained crates already in `docs/DESIGN.md`.

## Testing

- Core logic: unit tests against fakes; `proptest` for URL-pattern matching.
- Each port: one contract test suite, run against every implementation.
- IPC: integration tests that start the daemon in-process.
- Anything that closes processes, touches KWin or registers native messaging hosts: write it so it can be dry-run, and say which steps need manual testing on a real Windows or KDE Plasma (Wayland) machine.

## Workflow

- Start each task by restating the goal and listing the files you'll touch; ask if the step in `docs/BUILD_PLAN.md` is ambiguous.
- Small, reviewable changes. Conventional commit messages (`feat(core): ...`, `fix(nmh): ...`).
- Commit only when asked, on a feature branch, never directly on `main`.
- At the end of a task, summarise what changed, what's tested, what needs manual testing, and anything you deferred.
- The developer is a CS student learning Rust: briefly explain non-obvious Rust (lifetimes, trait objects, async, `Send`/`Sync`) in your summary when you use it.

## Commands

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo deny check
cd extension && npm test && npm run build
```
