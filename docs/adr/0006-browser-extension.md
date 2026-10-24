# 6. Browser extension

- Status: accepted
- Date: 2026-10-24

## Context

The design needs URL-level rules: a single Reddit post is fine, the Reddit feed isn't. Only the browser sees URLs and page contents, so the extension is required (ADR 0001). In Manifest V3, extensions can't hold back requests while asking anyone: blocking `webRequest` is gone. They can only declare rules up front (`declarativeNetRequest`, DNR) or react after a navigation has started. The daemon stays the source of truth for verdicts (ADR 0004 and 0005).

## Decision

- **Stack:** TypeScript, bundled with esbuild, tested with vitest. Shared WebExtensions APIs go through `webextension-polyfill`; Chromium-only code lives in `src/platform/`, with one manifest per browser.
- **Stable ID:** `manifests/chromium.json` carries a public `key`, which fixes the ID at `hjeodjhhiiaojdakfafaebijlmdckebo`. `focus-nmh --register` uses it by default, and a test checks the two agree. The private key wasn't kept: unpacked loading doesn't need it, and the Chrome Web Store assigns its own.
- **Page loads go through DNR, built from the running session's own copy of the rules (from `Status`).**
  - Each rule becomes a DNR regex rule whose priority sorts like the core: host labels, then literal path length, then strictness. Allow and Hide-feed rules get a DNR `allow`, so a more specific Allow still beats a broader Block. Ask, Delay and Block redirect to `interstitial.html?a=<action>#<url>`.
  - An allowlist adds a lowest-priority catch-all that blocks everything else.
  - The regexes reproduce the core's URL normalisation: case, port, user info, query and trailing slash. They're tested against the same examples as `focus-core`'s matcher.
- **In-page navigation** (single-page apps): `webNavigation.onHistoryStateUpdated` asks the daemon (`RulesFor`) and redirects if needed.
- **Hide feed:** a content script on Reddit and YouTube asks the background for the page's verdict and adds or removes a stylesheet when the URL changes.
- **Continue on Ask or Delay:** adds a DNR session rule allowing that site in that tab for 5 minutes, then navigates. Block can't be granted. Delay starts at 10 s and grows by 10 s per visit, up to 2 minutes; the count lives in `storage.session`, so it resets when the browser restarts.
- **Fail-closed while the daemon is unreachable:** the last rules stay until the session they came from would have ended. The end time is kept in `storage.local`, so it survives a browser restart.
- **Heartbeat:** every 5 s while the service worker is awake. An open native port keeps it awake, and a 30-second alarm wakes it if Chrome suspended it. The heartbeat reports the browser and whether incognito access is allowed.

## Consequences

- The URL matching exists twice, in Rust and in TypeScript. The tests pin both to the same examples, and changing the matcher means changing both.
- At most 1000 regex rules per session (Chrome's limit); more are dropped.
- Only top-level pages are affected. Embedded frames (e.g. a YouTube embed) aren't.
- Feed selectors depend on each site's markup and will need updates when the sites change; they're checked by hand.
- Firefox needs its own manifest (`browser_specific_settings`, `allowed_extensions` in the host manifest) and a `platform/firefox.ts`. The rest of the code is shared.
- Manual tests needed (real browser): host registration, the interstitial for a blocked site, a single Reddit post loading while the feed is hidden, and heartbeats appearing in the daemon's status.
