# Focus

An offline-first focus app for Windows and KDE Plasma (Wayland), written in Rust with a native [Iced](https://iced.rs) UI. It blocks distracting sites and apps during focus sessions, which you start by hand or from Google Calendar and Todoist, and it makes every bypass slow and deliberate. It needs no admin rights, no account and no server.

**Status:** early prototype, at build step 2. The core rules, session logic, TOML settings and state file work, and a demo window lets you run a session and try URLs against the example configs: `cargo run -p focus-ui`. Nothing is blocked yet (see [ADR 0002](docs/adr/0002-temporary-in-process-demo-ui.md)).

## Docs

- [Design](docs/DESIGN.md): what and why
- [Build plan](docs/BUILD_PLAN.md): the steps, in order
- [Architecture decisions](docs/adr/)

## Layout

```
crates/
  focus-core/         domain logic + port traits, no I/O
  ipc-proto/          versioned messages: ui, daemon, nmh
  config-toml/        loads + validates settings
  state-json/         active session, pending changes
  integrations/       iCal, Todoist
  enforce-windows/    Windows enforcement
  enforce-linux-kde/  KDE Plasma (Wayland) enforcement
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
