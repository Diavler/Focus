# Changelog

All notable changes to Focus. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-25

First release, Windows only (KDE Plasma comes later). Everything runs as your user: no admin rights, no account, no network needed.

### Added

- Focus sessions that block distracting sites (browser extension) and close blocked apps and browsers that don't run the extension.
- Configurations with block and allow lists, three strictness levels and an Emergency exit.
- Automatic starts from Google Calendar (iCal link) and Todoist, kept working offline from the last sync.
- Unlock budget, growing delays, a drift prompt and a tab cap to make bypassing deliberate.
- Parking lot with a system-wide shortcut, plus a Do Not Disturb button.
- Overview, Sessions, Calendar and Todoist pages; Rules and Settings overlays; System, Light and Dark themes; tray icon.
- Start at login. The daemon keeps the UI running during sessions, and the UI restarts the daemon if it's killed.
- Windows portable zip and per-user MSI; Chromium extension zip (load unpacked) and a signed Firefox `.xpi`.

[0.1.0]: https://github.com/Diavler/FocusApp/releases/tag/v0.1.0
