# Focus

> **Note:** this code was written with an AI assistant (Claude). It's a personal project, provided as is, with no guarantees that it works, keeps working or blocks everything you expect. Use it at your own risk.

Focus helps you get deep work done by blocking distracting websites and apps while you focus. It runs entirely on your own computer: no account, no server, no admin rights, and it keeps working offline.

Start a focus session by hand, or let your Google Calendar and Todoist start one for you. During a session, the sites and apps you've chosen are blocked or slowed down, and getting around them takes long enough that the urge usually passes first.

Windows today; KDE Plasma (Wayland) is planned.

## Features

**Focus sessions**
- Start a session by hand, on a schedule, from a calendar event or from a Todoist task.
- Pick a configuration (what to block) and a strictness level:
  - **Light:** end any time with one click.
  - **Deep:** ending early takes a 30-second wait and a typed reason; overrides come from a small daily budget.
  - **Locked:** no stopping until the timer ends.
- An **Emergency exit** in Settings for when you really need out.

**Blocking**
- Block by page, not just by site: one Reddit post is fine, the Reddit front page isn't.
- Each rule can **Block**, **Delay** (a countdown that grows each visit), **Ask** ("Is this part of your task?") or **Hide feed** (the site works, the endless feed doesn't).
- Closes distracting apps during a session.
- Browsers without the Focus extension (or private windows it can't see) are closed during a session, so switching browsers doesn't get around it.

**Staying on track**
- **Drift prompt:** after a few hops away from what you were doing, shows the trail and offers a way back.
- **Later button:** saves the link to Todoist and closes the tab.
- **Tab cap:** asks you to close a tab before opening one more than your limit.
- **Parking lot:** a keyboard shortcut sends a stray thought to your Todoist inbox instead of a new tab.
- Do Not Disturb during sessions.

**Hard to quit on a whim**
- Closing the window doesn't end a session; Focus keeps running in the tray.
- Rules are frozen during a session, and loosening them only takes effect after a cooling-off delay.
- Focus starts at login, and its parts restart each other if one is closed.

**Your data stays with you**
- Calendar (via its private iCal link) and Todoist sync when you're online and keep working from the last sync when you're not.
- Settings and state live in a `data` folder next to the app. Credentials (Todoist token, calendar link) are kept in the Windows Credential Manager.

## Install (Windows)

From a [release](https://github.com/Diavler/FocusApp/releases), take either:

- **MSI** (`Focus-<version>-windows-x64.msi`): installs for your user only, into `%LOCALAPPDATA%\Programs\Focus`, with no admin prompt. It adds a Start menu entry and registers the browser extension's native messaging host. Uninstall from Settings → Apps.
- **Zip** (`Focus-<version>-windows-x64.zip`): unzip anywhere you can write to, run `focus-nmh.exe --register` once, then open `focus-ui.exe`. To remove it, run `focus-nmh.exe --unregister`, turn off "Start at login" and delete the folder.

Either way, Focus keeps its settings and state in `data\` beside the exes, and uninstalling leaves that folder in place. Close Focus from the tray (**Close Focus…**) before you upgrade or uninstall. Then install the browser extension from the same release: `Focus-<version>-chromium-extension.zip` for Chromium browsers (load unpacked) or `Focus-<version>-firefox-extension.xpi` for Firefox. Steps are in [extension/README.md](extension/README.md#install-from-a-release).

## Development

**Status:** MVP on Windows; v0.1.0 is the first release. KDE Plasma parity is build step 9. Add `--dry-run` to the daemon to see what it would do without doing it.

### Build from source

```sh
cargo run -p focus-daemon   # first run writes starter settings to target/debug/data/
cargo run -p focus-ui       # in another terminal
```

On Windows both are windowed programs, so the daemon prints nothing to the terminal; its log is `target/debug/data/daemon.log`.

### Docs

- [Design](docs/DESIGN.md): what and why
- [Build plan](docs/BUILD_PLAN.md): the steps, in order
- [Architecture decisions](docs/adr/)

### Layout

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

### Checks

You need the stable Rust toolchain (on Windows, the MSVC target with the VS Build Tools "Desktop development with C++" workload) and `cargo install cargo-deny --locked`.

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo deny check
bash scripts/check-core-deps.sh   # focus-core stays free of I/O and workspace deps
cd extension && npm test && npm run build
```

### Making a release

`scripts/package-windows.ps1` builds the Windows zip and MSI into `target/release-artifacts/` (`-SkipMsi` if you don't have the WiX v5 tool: `dotnet tool install --global wix --version 5.0.2`). To publish, bump `version` in `[workspace.package]` of `Cargo.toml` and in `extension/package.json`, `package-lock.json` and both `extension/manifests/*.json`, add a `## [x.y.z]` section to `CHANGELOG.md` (it becomes the release notes), merge, then push a matching tag (`git tag v0.1.0 && git push origin v0.1.0`). `.github/workflows/release.yml` builds the zip, the MSI, the Chromium extension zip and the Firefox `.xpi` (signed as unlisted on AMO; needs the `AMO_JWT_ISSUER` and `AMO_JWT_SECRET` repository secrets) and attaches them to a **draft** GitHub release, which you check and publish by hand. See ADRs 0017 and 0018.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
