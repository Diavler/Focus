# 8. UI and daemon supervision, and start at login

- Status: accepted
- Date: 2026-10-24

## Context

The design's bypass table says: if you kill the daemon, the UI and daemon restart each other, and both start at login. Step 6 also adds a `start_at_login` setting that the daemon applies itself, and since turning it off loosens protection, it goes through cooling-off. Everything stays per user, with no admin, and inside the portable install (ADR 0004).

## Decision

**Who watches whom.**
- **The UI watches the daemon.** After 3 status polls in a row find nothing listening (about 3 s), the UI starts `focus-daemon` from its own folder. Other failures (a slow or busy daemon, another protocol version, another user's pipe) don't count, since another daemon wouldn't help. A daemon that is running after all exits by itself, because of `data/daemon.lock`.
- **The daemon watches the UI, during sessions only.** The UI holds `data/ui.lock` while it runs; checking that lock is how the daemon knows whether a UI is up. (Over IPC the UI can't be told apart from the extension, which also asks for `Status`.) Every 5 s during a session, if no UI holds it, the daemon starts `focus-ui --minimized`, at most once every 15 s. A clock that jumps back doesn't hold this up. `UiWatch` is a core port with a fake and a contract. So killing or closing the UI during a session brings it back minimised. Outside sessions, closing the UI closes it.
- **Single UI:** a second launch writes `data/ui.show` and exits. The running UI notices it within a second, then un-minimises and focuses its window.
- **Locks:** only the UI takes its lock with `acquire_patiently`, which retries for 0.5 s so the daemon's momentary check isn't mistaken for a running UI. Nothing checks the daemon lock that way, so the daemon takes it once.
- **No console windows:** both apps are windowed programs on Windows (`windows_subsystem = "windows"`). A login entry or a restart therefore never opens a console that could be closed to stop Focus. The daemon logs to `data/daemon.log`, which moves to `daemon.log.old` at startup once it's over 1 MiB.
- **Dry run:** `--dry-run` is passed along (daemon → UI → daemon), so a restarted daemon stays a dry run.

**`focus-install` crate** (new). It holds:
- the install layout (`Paths`, moved from the daemon);
- the instance locks (moved from the daemon);
- `spawn_sibling`, which starts another Focus executable detached. On Windows it uses `CREATE_NO_WINDOW`; on Unix a thread waits for the child, so it's reaped instead of left as a zombie.

Both apps use it, and the UI doesn't depend on the daemon crate. CLAUDE.md's platform-code rule now lists it.

**Start at login.**
- `start_at_login` is a top-level setting in `settings.toml`, `true` by default (the protective choice). `ConfigSource::load` now returns a `Settings` value (configs plus this flag).
- `focus_core::LoginSetting` applies switching **on** at once. Switching **off** waits out the cooling-off delay (24 h); asking again keeps the original time, and switching back on cancels it. It's persisted in `PersistedState`.
- A new `Autostart` port has a Windows adapter that writes the HKCU `Run` value `Focus` = `"<install>\focus-daemon.exe" --at-login`, using `RegSetKeyValueW` / `RegDeleteKeyValueW` (dry-run aware). On Linux it's a placeholder until step 9.
- The daemon registers once per start, so a moved install folder fixes itself, and again whenever the setting changes. A failed registration is retried every 5 s until it succeeds, and its reason is shown in the UI. On Linux the placeholder reports "not available until step 9" rather than claiming Focus starts at login.
- Started with `--at-login`, the daemon also opens the UI minimised.

**UI.**
- It shows which browsers are protected, and warns when an extension isn't allowed in incognito (that browser is closed during sessions, ADR 0007) or has stopped checking in.
- It shows whether Focus starts at login, and when a switch-off takes effect.

## Consequences

- Done-when verified on Windows in dry-run mode, with real processes: starting a Locked session opened the UI; killing the UI brought it back; killing the daemon brought it back with the session still running.
- During a session the UI can't be closed for good: it comes back minimised. That's deliberate, because the UI is the daemon's watchdog.
- Killing both processes within the same few seconds leaves no watchdog until the next login. That's a determined bypass, like uninstalling.
- A fresh install starts at login from its first run, via the starter settings. Removing the entry by hand (e.g. in Task Manager's Startup tab) is undone the next time the daemon starts, until `start_at_login = false` takes effect.
