# 8. UI and daemon supervision, and start at login

- Status: accepted
- Date: 2026-10-24

## Context

The design's bypass table says: if you kill the daemon, the UI and daemon restart each other, and both start at login. Step 6 also adds a `start_at_login` setting that the daemon applies itself, and since turning it off loosens protection, it goes through cooling-off. Everything stays per user, with no admin, and inside the portable install (ADR 0004).

## Decision

**Who watches whom.**
- **The UI watches the daemon.** After 3 failed status polls in a row (about 3 s), the UI starts `focus-daemon` from its own folder. A daemon that's only slow exits by itself, because of `data/daemon.lock`, so this is always safe.
- **The daemon watches the UI, during sessions only.** The UI holds `data/ui.lock` while it runs; checking that lock is how the daemon knows whether a UI is up. Every 5 s during a session, if no UI holds it, the daemon starts `focus-ui --minimized`, at most once every 15 s. So killing or closing the UI during a session brings it back minimised. Outside sessions, closing the UI closes it.
- **Single UI:** a second UI exits straight away.
- **Locks:** `InstanceLock::acquire` retries for 0.5 s, so another process's momentary lock check isn't mistaken for a running instance.
- **Dry run:** `--dry-run` is passed along (daemon → UI → daemon), so a restarted daemon stays a dry run.

**`focus-install` crate** (new). It holds:
- the install layout (`Paths`, moved from the daemon);
- the instance locks (moved from the daemon);
- `spawn_sibling`, which starts another Focus executable detached and, on Windows, with `CREATE_NO_WINDOW` so no console window appears.

Both apps use it, and the UI doesn't depend on the daemon crate. CLAUDE.md's platform-code rule now lists it.

**Start at login.**
- `start_at_login` is a top-level setting in `settings.toml`, `true` by default (the protective choice). `ConfigSource::load` now returns a `Settings` value (configs plus this flag).
- `focus_core::LoginSetting` applies switching **on** at once. Switching **off** waits out the cooling-off delay (24 h); asking again keeps the original time, and switching back on cancels it. It's persisted in `PersistedState`.
- A new `Autostart` port has a Windows adapter that writes the HKCU `Run` value `Focus` = `"<install>\focus-daemon.exe" --at-login`, using `RegSetKeyValueW` / `RegDeleteKeyValueW` (dry-run aware). On Linux it's a placeholder until step 9.
- The daemon re-applies the setting on every start, so a moved install folder fixes itself. Started with `--at-login`, it also opens the UI minimised.

**UI.**
- It shows which browsers are protected, and warns when an extension isn't allowed in incognito (that browser is closed during sessions, ADR 0007) or has stopped checking in.
- It shows whether Focus starts at login, and when a switch-off takes effect.

## Consequences

- Done-when verified on Windows in dry-run mode, with real processes: starting a Locked session opened the UI; killing the UI brought it back; killing the daemon brought it back with the session still running.
- During a session the UI can't be closed for good: it comes back minimised. That's deliberate, because the UI is the daemon's watchdog.
- Killing both processes within the same few seconds leaves no watchdog until the next login. That's a determined bypass, like uninstalling.
- A fresh install starts at login from its first run, via the starter settings. Removing the entry by hand (e.g. in Task Manager's Startup tab) is undone the next time the daemon starts, until `start_at_login = false` takes effect.
