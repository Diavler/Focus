# 13. The extension on Firefox

- Status: accepted
- Date: 2026-10-24

## Context

The extension is required and must stay browser-neutral, Chromium first (CLAUDE.md). ADR 0006 planned the Firefox port: its own manifest, `allowed_extensions` in the host manifest, and a `platform/firefox.ts`. Until now Firefox had no extension, so the daemon closed it during every session (ADR 0007). The developer asked for the port alongside step 8b.

## Decision

**One codebase, two builds.**
- `build.mjs` bundles `dist/chromium` and `dist/firefox`. They differ only in:
  - the manifest;
  - the `./platform` module, which an esbuild plugin points at `src/platform/<browser>.ts`;
  - the JavaScript target (Chrome 120, Firefox 128).
- `src/platform/index.ts` re-exports the Chromium module for the typechecker and the tests. A test checks that both modules export the same names.

**Firefox manifest** (`manifests/firefox.json`), Manifest V3:
- `background.scripts` instead of a service worker, which Firefox doesn't run for extensions.
- `browser_specific_settings.gecko`:
  - the fixed ID `focus@focus-app.local`, which the host trusts and a test checks against `register.rs`;
  - `strict_min_version` 128, for `declarativeNetRequest` with regex rules and `storage.session`;
  - `data_collection_permissions: none`.
- Otherwise the same permissions, content scripts, interstitial and toolbar button as Chromium, which a test checks. There's no Chrome-only `key` or `minimum_chrome_version`.

**Differences handled in shared code:**
- `declarativeNetRequest.isRegexSupported` is used only where the browser has it.
- **Tab cap:** Firefox gives no `pendingUrl` for a new tab, so a tab opened over the cap is held and sent to the tab-cap page on its first page load. A blank new tab is caught when it navigates.
- When a capped tab's page isn't known, Firefox opens `about:blank` (`NEW_TAB_URL` in `platform/`), since extensions can't open its new tab page.
- Closing the tab from an interstitial uses `tabs.remove`, because Firefox may refuse `window.close()`.

**Native messaging host:**
- Firefox starts the host with the host manifest's path and the add-on ID, not a `chrome-extension://` origin. `host::started_by_browser` accepts either, but only Focus's own Firefox ID.
- `--register` now also writes a Firefox manifest (with `allowed_extensions`):
  - **Windows:** `<install>/native-messaging/dev.focus.nmh.firefox.json`, pointed to by `HKCU\Software\Mozilla\NativeMessagingHosts\dev.focus.nmh`;
  - **Linux:** `~/.mozilla/native-messaging-hosts/dev.focus.nmh.json` when `~/.mozilla` exists.
- `--unregister` undoes both.

**Daemon.** On Windows, `firefox` now accepts the `firefox` heartbeat, so Firefox with a live extension (allowed in private windows) is no longer closed during sessions. Linux's browser list comes in step 9.

## Consequences

- No new dependencies. `web-ext lint` (run on demand with `npx`, not a project dependency) passes on `dist/firefox`.
- A temporary add-on (about:debugging → Load Temporary Add-on) is removed when Firefox restarts. For a lasting install, the add-on needs Mozilla's signing (an unlisted AMO upload) or a Firefox build that allows unsigned add-ons. Packaging belongs to the release step (10).
- Firefox must be told to run the extension in private windows (about:addons → Focus → "Run in Private Windows"). Otherwise the heartbeat reports it as not allowed, and the daemon closes Firefox during sessions, as with Chromium.
- Protocol changes still need `focus-nmh` rebuilt and the extension reloaded, now in both browsers.
- Manual tests (Windows, Firefox 128+):
  - registration, loading the add-on, and the heartbeat ("Firefox: protected");
  - each interstitial and Unlock;
  - Later (page and toolbar);
  - drift and the tab cap;
  - private windows;
  - Firefox staying open during a session while the extension is live.
- The Linux checks join step 9's list.
