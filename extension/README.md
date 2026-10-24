# Focus browser extension

Manifest V3, TypeScript, Chromium first. It talks to the Focus daemon through the `focus-nmh` native messaging host (ADR 0005); design notes are in ADR 0006.

## What it does

- Sends a heartbeat every 5 seconds, including whether it may run in incognito.
- Turns the running session's rules into `declarativeNetRequest` rules: Block, Ask and Delay send the page to an interstitial; Allow and Hide feed let it load.
- Checks page changes inside single-page apps (Reddit, YouTube) with the daemon, since those never reach `declarativeNetRequest`.
- Hides feeds and recommendations on Reddit and YouTube when the verdict is Hide feed.

## Layout

- `src/protocol.ts`, `src/native.ts`: messages and the `id`-matched native client.
- `src/dnr.ts`: config → DNR rules (mirrors the core's matcher).
- `src/friction.ts`, `src/feeds.ts`: Delay timing and feed selectors.
- `src/background.ts`, `src/interstitial.ts`, `src/content/feeds.ts`: entry points (wiring only).
- `src/platform/`: browser-specific code. Only `chromium.ts` for now.
- `manifests/<browser>.json`: one manifest per browser. The `key` fixes the Chromium extension ID at `hjeodjhhiiaojdakfafaebijlmdckebo`.

## Develop

```sh
npm ci
npm test        # typecheck + unit tests
npm run build   # bundles into dist/chromium/
```

## Try it in Chrome or Edge

1. Build the Rust side and register the host for your user: `cargo build`, then `target/debug/focus-nmh --register` (add `--dry-run` to see what it would change first).
2. Start the daemon: `cargo run -p focus-daemon`.
3. Open `chrome://extensions`, turn on Developer mode, choose **Load unpacked** and pick `extension/dist/chromium`.
4. Start a session in `cargo run -p focus-ui`, then visit a blocked site.

To undo the registration: `target/debug/focus-nmh --unregister`.
