# Build plan

One step per Claude Code session. Each step ends with green CI and a short summary. Steps 0–6 are the **Prototype**; 7–10 complete the **MVP**. See `docs/DESIGN.md` for the why.

**Windows first.** Steps 5–8 build and manually test the Windows side only. Linux keeps building and passing every automated test in CI (`ubuntu-latest`) throughout, but the KDE Plasma adapters and all manual Linux testing come together in step 9, once Windows is fully working.

## Prototype

### 0. Repository skeleton
Cargo workspace with empty crates from the design's folder layout (`focus-core`, `ipc-proto`, `config-toml`, `state-json`, `integrations`, `enforce-windows`, `enforce-linux-kde`, `apps/focus-daemon`, `apps/focus-ui`, `apps/focus-nmh`), `extension/` stub, `LICENSE-MIT`, `LICENSE-APACHE`, README, `docs/adr/0001-rust-iced-ports-and-adapters.md`.
GitHub Actions on `windows-latest` and `ubuntu-latest`: fmt, clippy `-D warnings`, test, `cargo deny check`, plus a check that `focus-core` has no workspace or I/O dependencies.
**Done when:** CI is green on both OSes and `cargo tree -p focus-core` shows only allowed crates.

### 1. Core domain and ports
In `focus-core`: `BlockConfig`, `SharedList`, `Rule`, `RuleAction` (Ask, Delay, HideFeed, Block), `Mode` (Blocklist, Allowlist), `Strictness` (Light, Deep, Locked), `Session` state machine (Idle → Running → Ended, with override and end-early rules per strictness), URL-pattern matcher. Port traits: `ConfigSource`, `StateStore`, `Enforcer`, `Notifier`, `Clock`, `History` (+ `NoopHistory`). Fakes for every port and contract-test suites.
**Done when:** the matcher has `proptest` coverage (e.g. `reddit.com/r/rust/comments/*` allowed while `reddit.com/` is hidden), and session transitions are unit-tested with a fake clock.

### 2. Settings and state adapters
`config-toml`: parse the TOML from the design into core types, resolve `include` lists, return clear validation errors (unknown list, empty allowlist, bad pattern). Cooling-off: edits to an active or scheduled config are queued with an apply-after time.
`state-json`: persist active session + pending changes atomically.
**Done when:** both pass their port contract tests; a corrupt state file is recovered from safely.

### 3. IPC and daemon
`ipc-proto`: versioned, serde-tagged messages (`StartSession`, `EndSession`, `Status`, `ListConfigs`, extension `Heartbeat`, `RulesFor(url)`). `focus-daemon`: composition root, single instance per user, serves IPC over `interprocess` local sockets, resumes a running session after restart.
**Done when:** an integration test starts the daemon in-process, starts a session over IPC, restarts the daemon and sees the session still running, and `focus-ui` no longer runs sessions in-process (retire the demo from ADR 0002).

### 4. Native messaging host and Chromium extension
`focus-nmh`: stdin/stdout length-prefixed JSON ↔ daemon IPC. Per-user host registration (HKCU on Windows, `~/.config/<browser>/NativeMessagingHosts` on Linux) via a `focus-nmh --register` command. Extension (TypeScript, MV3, `webextension-polyfill`, Chromium-only code in `src/platform/`): heartbeat every few seconds; `declarativeNetRequest` rules for Block; interstitial page for Ask and Delay; content scripts that hide feeds on Reddit and YouTube; reports whether it's allowed in incognito.
**Done when:** with a session running, a blocked site shows the interstitial, a single Reddit post loads, and the Reddit home feed is hidden (manual test steps written in the PR).

### 5. Enforcers (Windows)
`enforce-windows`: close blocked apps (current user's processes only), close browsers with no live heartbeat during focus, `GetForegroundWindow` for the active app, Do Not Disturb prompt.
**Done when:** contract tests pass with dry-run mode, and a manual checklist is verified on Windows.

### 6. Minimal Iced UI
Pick a config, strictness and duration; start a session; show status and time left. Respect strictness (no end button in Locked, wait + typed reason in Deep). UI and daemon restart each other if killed; both register for autostart at login (HKCU Run key; the systemd user service comes in step 9), controlled by a `start_at_login` setting in the TOML that the daemon applies itself. Turning it off loosens protection, so that change goes through cooling-off like any other.
**Done when:** you can run a full Locked session end to end, and killing either process brings it back.

## MVP

### 7. Integrations
`integrations`: `CalendarSource` from the private iCal URL and `TaskSource` from Todoist (personal token in `keyring`). Cache the last fetch in the state file. Start sessions from a calendar keyword or Todoist label mapped to a config.
**Done when:** a tagged calendar event starts the mapped config automatically, offline start uses cached data, and both adapters pass contract tests against recorded fixtures.

### 7b. Settings in the window
Everything in `settings.toml` editable from the UI, through the daemon's `SettingsEditor` (the file stays the source of truth and stays hand-editable, ADR 0009):
- Block configurations: mode, site rules (allow, ask, delay, hide feed, block) with a URL-pattern helper, included shared lists, apps to close (picked from running apps).
- Shared lists.
- Start at login (switching off still cools off).
Hover help on every control, and plain-language validation messages.

**Kill switch:** stops the daemon and the UI (not the browser's native messaging host), ending any running session and block. It needs friction: a wait, then an "Are you sure?" confirmation. Decide while planning:
- whether it works during a Locked session, and with what wait;
- whether Focus still starts at the next login;
- how to stop the UI and daemon restarting each other (ADR 0008).

**Done when:** a new user can set up configurations, rules and connections without opening `settings.toml`, and the kill switch works with its wait and confirmation (manual test notes for Windows).

### 8. Bypass friction and rabbit-hole tools
Unlock budget for Deep, growing Delay, drift prompt (hops away from the starting sites), Later button (sends link to Todoist inbox), tab cap, parking-lot hotkey (`RegisterHotKey`; the XDG GlobalShortcuts portal comes in step 9). Windows Do Not Disturb: a small button in the UI that opens the notification settings, instead of opening them by itself at session start. Keep it per platform, since KDE can silence notifications itself.
**Done when:** each behaviour has unit tests in the core and a manual test note for the platform parts.

### 9. Linux (KDE Plasma) parity
Bring KDE Plasma on Wayland up to the Windows feature set:
- `enforce-linux-kde`: `/proc` process control, a bundled KWin script (`workspace.windowActivated`) reporting over D-Bus, and `org.freedesktop.Notifications` `Inhibit` via `zbus` 6.
- Autostart as a systemd user service, and the parking-lot hotkey via the XDG GlobalShortcuts portal.
- Endpoint security for the `data/daemon.sock` socket, matching the Windows pipe hardening (ADR 0004).
- The manual checks deferred from earlier steps: UI, daemon socket, native messaging registration under `~/.config` or `$XDG_CONFIG_HOME`, and the extension.

**Done when:** the manual checklists of steps 3–8 pass on a KDE Plasma Wayland VM.

### 10. Release
`cargo-dist`: portable zip for both OSes, MSI, `.deb`/`.rpm` (or portable only; see ADR 0004); changelog; `v0.1.0` tag.
**Done when:** a fresh Windows VM and a fresh KDE Plasma VM can install from the release and run a session without admin rights.
