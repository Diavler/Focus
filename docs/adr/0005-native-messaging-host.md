# 5. Native messaging host

- Status: accepted
- Date: 2026-10-23

## Context

The browser extension has to reach the daemon (ADR 0001, ADR 0004). Chromium browsers let an extension talk to a local program only through native messaging: the browser starts a registered host and exchanges length-prefixed JSON with it over stdin/stdout. Registration has to be per user, without admin, and should keep files inside the portable install where the OS allows it.

## Decision

**Relay protocol.**
- `focus-nmh` reads Chrome's framing (4-byte native-endian length, then UTF-8 JSON) and relays to the daemon over the normal IPC client.
- Messages use the daemon's envelope plus an `id`: `{"id":7,"version":1,"message":<Request>}`, answered with `{"id":7,"version":1,"message":<Response>}`. The extension matches answers to requests by `id`. Unreadable input is answered with `id` 0.
- Only `Heartbeat`, `RulesFor` and `Status` are relayed. The extension, and any page that compromises it, can't start or end sessions.
- If the daemon is down, the host answers `Unreachable` (a new `DaemonError` variant). It reconnects on the next message, so a restarted daemon is picked up without restarting the browser.
- Incoming messages are capped at 1 MiB, and outgoing ones at Chrome's 1 MiB limit.

**Registration.** `focus-nmh --register --extension-id <id>` and `--unregister` build a plan of actions, then carry it out. With `--dry-run` they only print it, and the plan itself is unit-tested.
- **Windows:** one manifest at `<install>/native-messaging/dev.focus.nmh.json`, plus one key per browser at `HKCU\Software\<browser>\NativeMessagingHosts\dev.focus.nmh` pointing at it. The keys are written with the built-in `reg.exe`, so no registry crate is needed. HKCU needs no admin.
- **Linux:** a manifest in `~/.config/<browser>/NativeMessagingHosts/` for each supported browser whose config folder exists. Browsers only look there, so this is the one file outside the install folder.
- **Browsers:** Chrome, Chromium, Edge and Brave.
- **Extension ID:** passed explicitly for now. Step 4b gives the extension a fixed `key` in its manifest so the ID is stable, and makes it the default.

## Consequences

- The relay is tested end to end in-process: a pretend browser on an in-memory pipe, the real host code and an in-process daemon. Real browser registration is a manual test, done with the extension in step 4b.
- Moving the install folder breaks registration until `--register` is run again, because the manifest stores absolute paths.
- Firefox uses a different manifest key (`allowed_extensions`) and passes different arguments. Supporting it is a later addition to the registration plan, and the relay doesn't change.
