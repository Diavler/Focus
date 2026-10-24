# Focus

An offline-first focus app for Windows and KDE Plasma (Wayland), written in Rust with a native [Iced](https://iced.rs) UI. It blocks distracting sites and apps during focus sessions, which you start by hand or from Google Calendar and Todoist, and it makes every bypass slow and deliberate. It needs no admin rights, no account and no server.

**Status:** prototype complete on Windows (build step 6; KDE Plasma parity is step 9). The daemon runs sessions, starts at login, and restarts the UI during sessions; the UI restarts the daemon if it's killed. The browser extension blocks sites and hides feeds, and on Windows the daemon closes blocked apps and browsers without the extension. Add `--dry-run` to the daemon to see what it would do without doing it.

Try it:

```sh
cargo run -p focus-daemon   # first run writes starter settings to target/debug/data/
cargo run -p focus-ui       # in another terminal
```

On Windows both are windowed programs, so the daemon prints nothing to the terminal; its log is `target/debug/data/daemon.log`.

## Docs

- [Design](docs/DESIGN.md): what and why
- [Build plan](docs/BUILD_PLAN.md): the steps, in order
- [Architecture decisions](docs/adr/)

## Layout

```
crates/
  focus-core/         domain logic + port traits, no I/O
  ipc-proto/          versioned messages: ui, daemon, nmh
  ipc-transport/      per-user local sockets, framing, client
  config-toml/        loads + validates settings
  state-json/         active session, pending changes
  integrations/       iCal, Todoist
  enforce-windows/    Windows enforcement
  enforce-linux-kde/  KDE Plasma (Wayland) enforcement (placeholder until step 9)
  win-user/           Windows user/SID helpers
  focus-install/      portable install layout, instance locks, starting sibling processes
apps/
  focus-daemon/       per-user daemon, composition root
  focus-ui/           Iced UI
  focus-nmh/          browser native messaging host
extension/            browser extension (TypeScript, MV3)
scripts/              CI helpers
```

## Development

You need the stable Rust toolchain (on Windows, the MSVC target with the VS Build Tools "Desktop development with C++" workload) and `cargo install cargo-deny --locked`.

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo deny check
bash scripts/check-core-deps.sh   # focus-core stays free of I/O and workspace deps
cd extension && npm test && npm run build
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
