# 12. Parking lot, Later, the parking-lot shortcut, and Do Not Disturb on request

- Status: accepted
- Date: 2026-10-24

## Context

Step 8b (the second half of step 8, see ADR 0011) covers capture and notifications:
- a parking-lot hotkey that sends stray thoughts to the Todoist inbox instead of a new tab;
- a Later button that saves a link to Todoist and closes the tab;
- Do Not Disturb that no longer opens Windows Settings by itself at session start.

The developer chose that captures always land in a **local** parking lot that syncs to Todoist when it's connected and online (offline-first, no account needed). They also asked that Do Not Disturb stay per platform, since KDE can silence notifications by itself.

## Decision

**Parking lot (core + daemon).**
- `ParkingLot` lives in the saved state. Each `ParkedItem` has text, an optional link, and when it was parked and sent. Limits: 500 items, 2000 characters each.
- New requests:
  - `Park { text, url }`, which the browser may send for Later; blank text with a link parks the link;
  - `ParkingLot`, `Unpark { id }` and `ClearSentParked`, which only the window may send.
- A new `TaskInbox` port is kept apart from the read-only `TaskSource` (interface segregation). The `integrations` crate implements it as `TodoistInbox`:
  - `POST /api/v1/tasks` with no project, so tasks land in the Inbox;
  - the link goes in the description;
  - an `X-Request-Id` comes from the item, so a send repeated after a lost answer isn't added twice.
- HTTP writes go through a new `HttpPost` trait, so read-only adapters can't post.
- The daemon sends waiting items on its own task (`send_parked_forever`), outside the service's lock, like the plan fetch:
  - soon after parking, or after a credential changes;
  - one minute after a failure, or ten minutes when Todoist isn't set up;
  - it stops at the first failure.
- `Status.parking_lot` reports the total, how many are waiting, and why the last send failed.

**Later (extension).**
- The block, ask, delay and drift pages, and the toolbar button (`action`), send `Park` with the page's link (and title from the toolbar), then close the tab.
- If the daemon can't be reached, nothing is closed. The page says why; the toolbar button shows a "!" badge with the reason as its tooltip.

**Shortcut (UI + new `focus-hotkey` crate).**
- The UI listens, not the daemon. Windows lets the process that receives a hotkey bring its own window to the front, so the window can come up and focus the box at once. The UI runs during every session, because the daemon relaunches it.
- A new `GlobalHotkey` port registers a shortcut and returns a guard; dropping the guard unregisters it.
- `focus-hotkey` picks the platform adapter internally, as `secrets-keyring` does, so the UI has no `#[cfg]`:
  - **Windows:** `RegisterHotKey` on a thread with its own message loop; dropping the guard posts `WM_QUIT` and joins the thread.
  - **Linux:** reports unsupported until the XDG GlobalShortcuts portal in step 9.
- The shortcut is a closed set (`Hotkey`: Ctrl+Alt+Space by default, Ctrl+Shift+Space, Ctrl+Alt+P, off). It's chosen from a pick list in Settings → General and written as `parking_hotkey` in `settings.toml`. It isn't a protection, so changes apply at once, without cooling off.
- An iced subscription keyed by the shortcut owns the guard, so changing the setting re-registers it.
- Pressing the shortcut restores and focuses the window on Sessions, with the parking-lot box focused. Enter parks the thought and minimises the window again.

**Do Not Disturb (amends ADR 0007).**
- `Notifier::silence` now answers `Silencing::{Silenced, AskUser, Unsupported}`, and a new `NotificationSettings` port opens the platform's settings.
- On Windows, silencing answers `AskUser` and does nothing else. While a session runs, the session card shows "Turn on Do Not Disturb", which sends `OpenNotificationSettings` to the daemon to open `ms-settings:notifications`.
- KDE will answer `Silenced` after `Notifications.Inhibit` (step 9), so no button appears there; until then the placeholder answers `Unsupported`.
- The daemon now also silences when a running session resumes after a restart.

## Consequences

- New crate `focus-hotkey`, added to CLAUDE.md's list of crates allowed platform code. No new external dependencies: it uses the workspace's `windows-sys` with one more feature (`Win32_UI_Input_KeyboardAndMouse`).
- The protocol grew again: rebuild `focus-nmh` and reload the extension (in Helium, on its extensions page).
- With the UI closed outside a session, the shortcut does nothing. The parking lot still works from the window and the browser.
- Sent items stay listed as "In Todoist" until cleared, so the user can see what went where.
- Manual tests (Windows):
  - the shortcut from another app, then Enter;
  - a shortcut another app already uses (the window explains);
  - Later from each page and from the toolbar;
  - parking offline, then online with Todoist connected, checking the Todoist Inbox;
  - the Do Not Disturb button during a session, and that nothing opens at session start.
