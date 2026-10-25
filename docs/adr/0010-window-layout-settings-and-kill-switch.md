# 10. Window layout, settings in the window, and the kill switch

- Status: accepted (amended 2026-10-25: no wait when no session is running)
- Date: 2026-10-24

## Context

After step 7 the developer found the window "very boring & bare", and the app not intuitive: too much depended on hand-written strings in `settings.toml`. Step 7b asks for:
- a proper layout with three sections (Sessions, Calendar, Todoist);
- a Settings button in Sessions for the rules and everything else editable;
- a more interesting colour scheme;
- a kill switch that stops Focus (not the browser's host) and ends any block, with friction.

The developer decided:
- a sidebar with pages;
- the kill switch works during Locked sessions, but with a longer wait and a typed reason;
- Focus starts again at the next login.

## Decision

**Layout:**
- A sidebar shows the brand, what's running, the pages (Sessions, Calendar, Todoist) and each source's sync status. Polish step P1 added Overview as the first page and the one the window opens on: today's calendar as an hour grid beside overdue and today's tasks.
- The main area scrolls and shows one page.
- Settings is a page opened from the Sessions header, with tabs: Automatic starts, Configurations, Lists, General.
- Each page is an `impl App` block in its own module (`sessions.rs`, `plan_pages.rs`, `settings.rs`, `configs.rs`, `rules.rs`), with shared styling in `style.rs`.

**Colours:**
- Focus has its own palette in two versions, following the system's light or dark mode (`iced::system::theme` and `theme_changes`):
  - **night:** deep navy, a violet accent, mint, amber, rose;
  - **day:** pale lavender with deeper versions of the same accents.
- The running session sits in a violet-to-cyan gradient card, and the sidebar has a faint violet tint.
- Every widget style takes its colours from the theme's palette, so both versions work everywhere.

**Settings in the window:**
- `SettingsEditor` changes from `set_auto_start` to whole-settings editing:
  - `load_editable` returns `EditableSettings`: the settings as written, with configurations keeping their own patterns and the names of the lists they include, plus a revision (a hash of the text).
  - `save_editable` writes everything back with `toml_edit`. Lists and configurations are updated in place, so comments around them stay; unchanged arrays aren't rewritten; new entries go at the end and removed ones disappear.
- A save from an old revision is refused (`ConfigSourceError::Changed`, shown as "reload and make the change again"), so hand edits made while Settings is open aren't overwritten.
- Invalid results are refused by the normal parser. Nothing is written in either case.
- The daemon then re-reads the file as for a hand edit, so cooling-off is unchanged and `settings.toml` stays the single source of truth.
- The window edits a draft and has one save bar: Save / Discard, with Reload after a refused save.
- Patterns are checked as they're added (`UrlPattern::parse`), and "Try a URL" checks the *unsaved* draft locally (`EditableSettings::resolve`).
- Deleting a configuration drops the rules that start it; deleting a list removes it from the configurations that include it.
- Apps to close can be picked from the apps with a window open, through a new `WindowedApps` port:
  - On Windows it reads the visible top-level windows, keeps the current user's processes and leaves out Focus, the shell and a few helper hosts.
  - On Linux it's a placeholder until step 9.

**Kill switch:**
- Core `KillSwitch` and `StopPolicy`:
  - Asking starts a wait of 1 minute, or 5 minutes plus a typed reason during a Locked session. With no session running there's nothing to protect, so there's no wait (amended 2026-10-25, so the tray's "Close Focus" closes at once; it warns about skipped automatic starts).
  - The policy in force when confirming applies, and the wait never counts time from before the running session started. So a session that starts during the wait restarts it (5 minutes for a Locked one), and a request left pending while idle can't shorten a later session's wait.
  - It's not persisted: a restart just starts a fresh wait.
- `SessionState::stop` ends any session with `EndReason::Stopped { reason }`.
- New requests: `RequestStop`, `ConfirmStop { reason }` and `CancelStop`. `Status` shows `stop_allowed_at`, so the window can count down.
- On confirm, the daemon saves the ended session and answers `Done`. The server then shuts down: `Service::take_stop` reports the stop, the service is taken out of the shared slot under the same lock (so no tick can start a session or save after the stop), and a `Notify` wakes `serve`. The window exits when it sees `Done`.
- Neither process restarts the other:
  - The daemon only relaunches the UI during a session, and there isn't one any more.
  - The UI that would relaunch the daemon has exited.
- Focus starts again at the next login (the HKCU Run entry is untouched) or when the user opens it.
- **Lifting the browser's blocks.** The extension deliberately keeps enforcing its last-known session while the daemon is unreachable, so killing the daemon isn't a way out of a session (ADR 0006).
  - When the daemon can't be reached, `focus-nmh` now looks at the saved state with `JsonStateStore::peek`, which reads only, never moving or rewriting a file.
  - If the saved state has no running session, it answers `DaemonError::Stopped`, and the extension drops its cached session and blocks. A missing or unreadable state file counts as a session (blocks stay), since the kill switch always saves the stopped state before exiting.
  - Killing the processes by hand leaves the session saved as running, so blocks stay, and so does deleting the state file. Faking "stopped" means writing a state file, which ADR 0003 already counts as deliberate tampering.

## Consequences

- Tested:
  - the kill switch's waits and reasons in the core and daemon;
  - an IPC test where `ConfirmStop` makes the real server shut itself down with the session saved as stopped;
  - the host answering `stopped` when no session is saved, and the extension's `sessionFromStatus`;
  - TOML round trips that keep comments, add, change and remove, refuse invalid saves, and refuse old revisions;
  - the editor contract against real files;
  - `WindowsWindowedApps` live.
- Manual (Windows):
  - the layout and colours in light and dark mode;
  - every Settings tab saving into `settings.toml`;
  - the kill switch during an idle, Deep and Locked session, with the browser's blocks lifting afterwards and Focus returning at the next login.
- Renaming a configuration or list isn't supported (create a new one and delete the old); the tooltip says so.
- The URL check that used the daemon's running session is replaced by the configuration editor's local "Try a URL".
