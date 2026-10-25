# All-in-One Productivity App — Design Doc

Source of truth: https://claude.ai/artifact/A9cRRGT9fDuKQ4yaXMxTbn (this file is an export; diagrams are rendered as text below).

## Overview

A native, offline-first focus app for Windows and Linux. It knows what you planned (Google Calendar, Todoist) and protects that time by blocking distracting sites and apps. It is designed around two failure modes: rabbit holes and bypassing blockers.

Goals:

- Start focus sessions from your plan, not only by hand.
- Block by URL pattern, not just domain: a single Reddit post is fine, the Reddit feed is not.
- Make every bypass slow and deliberate, without admin rights.

## Focus Mode

A session is a block configuration plus a strictness level plus a duration. It starts by hand, on a schedule, from a calendar event keyword, or from a Todoist task or label (e.g. `@deep`).

**Block configurations** are named, reusable rule sets in the TOML config. Each has a mode (block what's listed, or allow only what's listed), site and URL-pattern rules, apps to close and notification settings. Shared lists such as *Social media* (sites and apps) can be included by several configurations; the window calls configurations *profiles*.

```toml
[lists.social]
block = ["x.com", "instagram.com", "tiktok.com"]
apps = ["discord"]

[configs.coding]
mode = "blocklist"
include = ["social", "video"]
feed_only = ["reddit.com", "youtube.com"]  # hide feeds, allow single posts
allow = ["reddit.com/r/rust/comments/*", "docs.rs", "github.com"]
apps = ["discord", "steam"]

[configs.writing]
mode = "allowlist"
allow = ["docs.google.com", "dictionary.com"]
```

| Strictness | Ending early | Overrides |
| --- | --- | --- |
| Light | One click | Allowed |
| Deep | 30-second wait + typed reason | From the daily unlock budget |
| Locked | Not until the timer ends | None |

Each rule takes one action: **Ask** ("Is this part of *Finish the auth PR*?"), **Delay** (a countdown that grows each visit), **Hide feed** (the site works, feeds and recommendations don't) or **Block**. During a session notifications are silenced, and a parking-lot hotkey sends stray thoughts to the Todoist inbox instead of a new tab.

## Staying on track

Catch rabbit holes early and make bypasses slow. Without admin rights a determined bypass is always possible, so the aim is that it takes longer than the urge lasts.

**Rabbit holes**

- **Drift prompt:** after several hops away from the session's starting sites, show the trail and offer a way back.
- **Later button:** saves the link to Todoist and closes the tab.
- **Tab cap:** during focus, opening one more tab than the limit asks you to close one first.

**Bypass countermeasures**

| Bypass | Countermeasure |
| --- | --- |
| Quit the app or end the session | Closing the UI never ends a session; Locked has no stop button |
| Kill the daemon | UI and daemon restart each other, and both start at login |
| Loosen rules mid-session | Rules freeze during a session; changes apply after a cooling-off delay (e.g. 24 h) |
| Disable the extension, use incognito or another browser | During focus, a browser without a live extension heartbeat (including one whose extension isn't allowed in private windows) is closed |
| Override "just this once" | A typed reason, a growing wait, or spending the daily unlock budget |
| Uninstall | Can't be prevented without admin, but it takes several deliberate steps |

## Integrations

| Service | Used for | Connection | When |
| --- | --- | --- | --- |
| Google Calendar | Today's events; auto-start focus on tagged events | Private iCal address (read-only) | MVP |
| Todoist | Today's tasks; focus from a task; parking-lot capture | Personal API token | MVP |
| Notifications | Silence during sessions | KDE: `Notifications.Inhibit`; Windows: no official toggle API, so prompt to turn on Do Not Disturb (to investigate) | MVP |
| Calendar write-back, GitHub, Slack | Focus blocks on the calendar, PRs as tasks, status | Your own OAuth client or tokens | Later |

Tokens, and the private iCal address, are stored in the OS keychain (`keyring-core`). Full OAuth waits because an open-source app can't keep a Google client secret, and unverified test-mode tokens expire after 7 days.

## Architecture

A per-user daemon owns sessions and enforcement. The UI and the browser's native-messaging host talk to it over local IPC (named pipes on Windows, Unix sockets on Linux), so closing the UI never ends a session.

```
Runtime (processes)
  focus-ui  <--IPC-->  focus-daemon  <--sync-->  Google Calendar (iCal), Todoist
                           ^   |
          native messaging |   +--> OS enforcement: close apps, silence notifications
                           |   +--> local files: TOML settings, JSON state
  Chromium extension <--> focus-nmh
  (sees URL + page; interstitials, feed hiding, blocking)
```

The code follows ports and adapters: `focus-core` holds all the logic and declares traits; each adapter crate implements one, and only the daemon wires them together.

```
Dependencies (arrows point to what a crate depends on)
  focus-ui ──IPC── focus-daemon ──IPC── focus-nmh
                        │ uses
                        ▼
  focus-core: domain logic (sessions, rules, configs); no OS calls, no network, no disk
              ports (traits): CalendarSource, TaskSource, Secrets, Enforcer, Notifier,
                              ConfigSource, SettingsEditor, WindowedApps, StateStore, History, Clock
                        ▲ each implements a port
  config-toml · state-json · integrations · enforce-windows · enforce-linux-kde
  (later, optional: storage-sqlite, classifier, focus-online)
```

```
focus/
├─ crates/
│  ├─ focus-core/          # domain + port traits, no I/O
│  ├─ ipc-proto/           # versioned messages: ui, daemon, nmh
│  ├─ config-toml/         # loads + validates settings
│  ├─ state-json/          # active session, pending changes
│  ├─ integrations/        # ical, todoist
│  ├─ enforce-windows/
│  └─ enforce-linux-kde/
├─ apps/                   # focus-daemon, focus-ui, focus-nmh
├─ extension/              # TypeScript, MV3, Chromium first
└─ docs/adr/
```

Rules:

- `focus-core` depends on no OS, network or other workspace crate; CI checks this.
- The UI talks only to the daemon, never to files or the network.
- Every port has a fake for tests. The `History` port ships as a no-op until the Insights module.
- The extension stays browser-neutral: shared WebExtensions APIs via `webextension-polyfill`, Chromium-only code in one `platform/` module, one manifest per browser. `focus-nmh` needs no changes for Firefox.

**SOLID, applied to Rust.** Rust has traits instead of class inheritance, so each principle maps to a concrete rule:

| Principle | Rule in this codebase |
| --- | --- |
| Single responsibility | One reason to change per crate and module: rule matching, session state, scheduling and IPC each live apart |
| Open/closed | New behaviour arrives as a new adapter crate or feature flag (GNOME, Firefox, Insights), never as edits to `focus-core` |
| Liskov substitution | Every implementation of a port, fakes included, passes the same shared contract test suite |
| Interface segregation | Small, focused traits (`CalendarSource`, `TaskSource`, `Enforcer`, `Notifier`), not one large `Integration` trait |
| Dependency inversion | `focus-core` owns the traits and depends only on them; the daemon builds concrete adapters and injects them at startup |

| Need | Windows | KDE Plasma (Wayland) |
| --- | --- | --- |
| Start at login | `HKCU` Run key | systemd user service |
| Active window | `GetForegroundWindow` | Bundled KWin script (`workspace.windowActivated`) reporting over D-Bus |
| Close blocked apps | Terminate your own processes | `/proc` + signals |
| Global hotkey | `RegisterHotKey` | XDG portal GlobalShortcuts |
| Tray icon | `tray-icon` | `ksni` |
| Native messaging host | Registered under `HKCU` | `~/.config/<browser>/NativeMessagingHosts` |

GNOME, X11 and macOS later become new adapter crates; nothing in the core changes.

## Tech stack

| Area | Choice |
| --- | --- |
| Language and build | Rust, one Cargo workspace |
| UI | Iced |
| Async, HTTP | `tokio`; `ureq` for HTTP (synchronous like the ports; see ADR 0009) |
| Settings and state | `serde` + `toml` for settings; `serde_json` for the state file (written via temp file and rename) |
| Secrets | `keyring` |
| IPC | `interprocess` |
| OS access | `windows` (windows-rs); `zbus`, `nix` on Linux |
| Diagnostics | `tracing`, for developers only |
| Extension | TypeScript, Manifest V3 |

Ruled out: Tauri and Electron (web wrappers) and Slint (license friction with MIT/Apache). Python is used only for the optional classifier.

## Optional modules

Each is a Cargo feature, off by default; the core never depends on any of them.

| Module | Adds | Notes |
| --- | --- | --- |
| Insights (`insights`) | Session history, bypass log, planned vs actual, weekly review | SQLite via `storage-sqlite`, plugged into the `History` port |
| Content classifier (`classifier`) | Judges a page by its relevance to the session's intent | Train in Python, run an ONNX model with `ort`; URL rules cover the MVP |
| Online and social (`online`) | Accountability partner, body doubling, cross-device sync, community block lists | After v1.0; self-hostable server; end-to-end encrypted sync; never sends browsing history |

## Development process

| Stage | Practice |
| --- | --- |
| Plan | GitHub Issues with acceptance criteria; one ADR per big decision in `docs/adr/` |
| Build | Feature branches and pull requests, conventional commits, `rustfmt`, `clippy` |
| Test | Unit tests against fakes for every port, `proptest` for rule matching, IPC integration tests |
| CI | GitHub Actions on Windows and Ubuntu: format, lint, test, `cargo-deny` |
| Release | Semantic versioning, changelog, `cargo-dist`: portable zip, MSI, `.deb`/`.rpm` |

Test enforcement in a Windows VM and a KDE Plasma VM, skip Flatpak and Snap (their sandboxes block native messaging and process control), and dogfood from week one.

## Roadmap

Ship a usable offline blocker first; optional modules come only once you rely on it daily.

1. **Prototype:** Windows + Linux, manual focus sessions, URL rules + extension, interstitial prompt. Gate: you use it daily for 2 weeks.
2. **MVP:** Calendar + Todoist, focus from task/event, saved block configs, bypass friction. Gate: sync works offline, no data loss.
3. **Insights module:** drift detection, planned vs actual, unlock budget, weekly review. Gate: v1.0 release, you rely on it daily.
4. **Optional, later:** content classifier, macOS port, GNOME / X11 adapters, online/social module.

Ideas for later: auto-timeboxing tasks into free calendar slots, start and shutdown rituals, break reminders.

## Key decisions

| Topic | Decision |
| --- | --- |
| Scope | Personal, open source (MIT / Apache-2.0), offline-first, no accounts or server |
| Platforms | Windows and KDE Plasma on Wayland first; GNOME, X11 and macOS later |
| Language and UI | Rust, Iced |
| Privileges | None: everything runs per user; a portable zip is the whole install |
| Browser | Extension required; Chromium first, Firefox-ready |
| Persistence | TOML settings and a JSON state file; SQLite only with Insights |
| Classifier | Optional module; URL rules in the MVP |
| Online features | Optional module after v1.0 |
