# Focus browser extension

Stub. Implemented in build step 4 (`docs/BUILD_PLAN.md`).

Planned layout:

- TypeScript, Manifest V3, shared WebExtensions APIs via `webextension-polyfill`.
- Chromium-only code lives in `src/platform/`; one manifest per browser, so Firefox support needs no changes elsewhere.
- Talks to the daemon through the `focus-nmh` native messaging host: heartbeats, `RulesFor(url)`, interstitials for Ask and Delay, `declarativeNetRequest` for Block, content scripts that hide feeds.
