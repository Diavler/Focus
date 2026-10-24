# 9. Calendar and Todoist integrations

- Status: accepted
- Date: 2026-10-23

## Context

Step 7 lets sessions start from the plan: a Google Calendar event whose title has a keyword, or a Todoist task with a label, runs a mapped configuration. It must keep working offline (design: "sync works offline, no data loss"), without accounts or a server, and without admin rights. The design rules out full OAuth for now, so Google is read through the calendar's private iCal address and Todoist through a personal API token. The step's done-when: a tagged calendar event starts the mapped config automatically, offline starts use cached data, and both adapters pass contract tests against recorded fixtures.

## Decision

**In the core (pure):**
- `CalendarEvent` (one occurrence; its ID is the event's UID plus its original start) and `Task` (labels, an optional due *time*, an optional duration).
- `AutoStartRule` from `[[auto_start]]` in `settings.toml`. Each rule names a config and a strictness (default Light), plus `calendar_keywords` (matched in the title, ignoring case) and/or `task_labels` (ignoring case and a leading `@`). `task_minutes` (default 50) is the length for tasks without a duration.
- `planned_starts` turns the cached plan into `PlannedStart`s, sorted by start. The first matching rule wins.
- `AutoStartLog`, persisted, remembers which planned sessions already started until they end. So ending an automatic Light session early doesn't start it again.
- A planned session starts when it's due, no session is running and at least a minute is left. It runs until the event ends (capped at 12 h). If a manual session is running, the planned one starts when it ends, for whatever time is left. A rule naming a config that isn't in effect is logged and marked, so it isn't retried every second.
- Edits to a config with a planned session inside the cooling-off window (24 h) now cool off too, as ADR 0003 planned for this step. A daily recurring event therefore keeps its config protected all the time.
- New ports, each with a fake and a contract suite:
  - `CalendarSource::events(from, to)` (overlapping, sorted, unique IDs);
  - `TaskSource::tasks()`;
  - `Secrets` (`load` / `store`, keyed by the closed `SecretKey` enum: `CalendarUrl`, `TodoistToken`).
- `SourceError` tells *not set up*, *unreachable* (keep the cache) and *rejected* (bad token or address, or unreadable data) apart.

**Cache:** `PersistedState.plan` holds the last successful fetch of each source, with its time. A failed fetch keeps the cache, so automatic starts keep working offline for up to 7 days ahead. A source that's no longer set up (credential removed) clears its cache, so stale events don't keep starting sessions.

**Sync (daemon):**
- A separate task asks the service every 2 s whether a fetch is due: at startup, every 5 min, and right after a credential changes.
- The fetch runs on the blocking pool *without* the service's lock, because the network can take up to 30 s and requests, ticks and enforcement must keep going.
- The result is then folded in under the lock.
- The window is 12 h back (so an event already running counts) to 7 days ahead.
- Automatic starts are checked every 1 s tick.

**`integrations` crate:**
- HTTP sits behind a tiny `HttpGet` trait, so the adapters themselves run the contract suites against recorded responses.
- **iCal:**
  - Parsed with `icalendar`'s parser.
  - Recurrences are expanded with `rrule`, honouring `EXDATE` and `RECURRENCE-ID` overrides (moved or cancelled occurrences).
  - `TZID`s are IANA names (`chrono-tz`). Floating times, and Windows zone names from Outlook, are read in the computer's local zone.
  - All-day and cancelled events are skipped.
  - `webcal://` becomes `https://`; anything other than HTTPS is refused, since the address grants access to the calendar.
- **Todoist:**
  - API v1 `GET /api/v1/tasks/filter?query=overdue | next 7 days`, 200 per page, following `next_cursor`. REST v2 has been retired and now answers 410.
  - `due.date` may be a day, a floating local time or a UTC time. Only timed tasks can start sessions.
- **HTTP client: `ureq` instead of the design's `reqwest`.**
  - The ports are synchronous, and `ureq` is too, so there's no runtime inside the daemon's runtime.
  - It uses rustls with *ring* rather than aws-lc (no C build tooling or OpenSSL-licensed code).
  - It verifies certificates with the OS store (`rustls-platform-verifier`), so antivirus or company proxies that add a root still work.
  - Mozilla's root list (`webpki-roots`, CDLA-Permissive-2.0) comes along as data; `deny.toml` allows that licence for those two crates only.

**Credentials (`secrets-keyring` crate):**
- On Windows, Credential Manager entries via `keyring-core` + `windows-native-keyring-store`, under the service name `Focus`. Entries are built straight from a store object, not keyring's global default store (no global state).
- Linux is a placeholder until step 9: storing fails with a reason and nothing is found, so the sources report *not set up*.
- The private iCal address counts as a credential too: it's stored in the keychain, not in `settings.toml`.
- The UI sends credentials with a new `SetSecret` request. A blank value removes one. Status never contains secrets, only whether each source works.
- The native messaging host doesn't relay `SetSecret`, so a web page can't swap the calendar.

**UI:** a Plan section shows each source's state, which is one of:
- synced N min ago;
- offline, using cached data from N h ago;
- not set up;
- a problem to fix, in red.

It also lists the next planned sessions and has hidden inputs (Save / Remove) for the iCal address and the Todoist token. The page now scrolls.

## Consequences

- Done-when verified by tests:
  - The daemon's in-process IPC test starts a Locked session from a tagged event.
  - After a restart with the calendar offline, the cached plan still starts it on time.
  - Both adapters pass their contracts against recorded Google and Todoist responses.
  - The keychain adapter passes its contract against the real Credential Manager (an ignored test, run locally).
- Triggers themselves don't cool off. Removing an `[[auto_start]]` rule applies at once, but deleting the event from the calendar is just as easy, so a delay would add nothing.
- The Todoist fixtures were written from Todoist's API reference and SDK models, not captured from a live account. The first run against a real token is a manual check.
- Not yet: starting a session from a task by hand, showing today's tasks, the Later button and parking lot (step 8), and calendar write-back (later, per the design).
