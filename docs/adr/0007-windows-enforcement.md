# 7. Windows enforcement

- Status: accepted
- Date: 2026-10-24

## Context

Step 5 makes sessions bite outside the browser on Windows. The build plan now does Windows first (Linux in step 9). The design asks for: closing blocked apps (only the current user's), closing browsers that have no live extension heartbeat during focus (including one whose extension isn't allowed in private windows), knowing the foreground app, and a Do Not Disturb prompt, because Windows offers apps no supported way to turn it on.

## Decision

**Policy in the core, effects in the adapter.**
- `focus_core::BrowserWatch::browsers_to_close` decides which running browsers to close. A browser is kept if a heartbeat from its extension arrived within 15 s with incognito access allowed. Otherwise it's kept only during a 20 s grace period after it's first seen, so a newly opened browser's extension can start. A browser that stops running is forgotten, so a relaunch gets a fresh grace period.
- `apps_to_close` merges these browsers with the config's `apps`. App names match case-insensitively, with or without `.exe`.
- Two small new ports keep `Enforcer` focused:
  - `ProcessList` ("which of these apps are running?");
  - `ForegroundApp` ("which app is in front?").
- Both come with fakes and contract suites.

**`enforce-windows`** (`windows-sys` only):
- **Closing an app:** the first time, it posts `WM_CLOSE` to the app's visible top-level windows, as clicking ✕ would, so browsers keep their tabs. If the app is still running 4 s later, the next check terminates it. An app with no windows is terminated straight away.
- **Whose processes:** only the current user's (an owner SID check), never this process. Focus itself and the shell's own processes (`explorer`, `dwm`, `sihost`, ...) are protected, whatever the config says.
- **Browser catalogue:** each browser's executable and the heartbeat names that vouch for it. Chromium also runs as `chrome.exe`, and Vivaldi reports itself as Chromium. Firefox has no extension yet, so it's always closed during a session.
- **Foreground app:** `GetForegroundWindow`, then the owning process's image name. Reported in `Status` as `foreground_app`, ready for drift detection later.
- **Do Not Disturb:** at session start, open `ms-settings:notifications` once (via `%SystemRoot%\explorer.exe`, by full path). Ending the session can't turn Do Not Disturb off again for the user.
- **`Mode::DryRun`** logs every action instead. `focus-daemon --dry-run` uses it.

**Shared Windows identity code** moved to a new `win-user` crate (current user SID, process owner, owned handles), used by `ipc-transport` and `enforce-windows`. CLAUDE.md's platform-code rule now lists it.

**Daemon:**
- Enforces every 5 s during a session.
- Silences notifications when a session starts and restores them when it ends. The prompt isn't repeated when a running session resumes after a daemon restart.
- On other platforms it uses `enforce-linux-kde`'s `PendingEnforcer` placeholder until step 9, so Linux CI keeps building and testing everything.

## Consequences

- With the extension unreachable (daemon down, or a stale `focus-nmh` after an upgrade), every browser is closed after the grace period. That's the intended fail-closed behaviour, but `focus-nmh` must be rebuilt and the extension reloaded whenever the protocol changes.
- A Chromium browser whose extension isn't allowed in incognito is closed during sessions. The UI should point this out (step 6).
- Heartbeats identify a browser family, not a process, so a browser that shares a heartbeat name with another (e.g. Chromium and Chrome) can be vouched for by the other. That's acceptable for a single-user tool.
- Terminating is abrupt for apps that ignore `WM_CLOSE` within 4 s.
- Tested automatically on Windows: listing processes, closing a spawned test process (live), dry run leaving it alone, protected apps, every contract. Manual: real apps and browsers, the grace period, the Do Not Disturb prompt.
