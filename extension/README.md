# Focus browser extension

Manifest V3, TypeScript, for Chromium browsers and Firefox (ADR 0013). It talks to the Focus daemon through the `focus-nmh` native messaging host (ADR 0005); design notes are in ADR 0006.

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
- `src/platform/`: browser-specific code, `chromium.ts` and `firefox.ts`; the build points `./platform` at one of them.
- `manifests/<browser>.json`: one manifest per browser. The `key` fixes the Chromium extension ID at `hjeodjhhiiaojdakfafaebijlmdckebo`; Firefox's ID is `focus@focus-app.local`.

## Develop

```sh
npm ci
npm test        # typecheck + unit tests
npm run build   # bundles into dist/chromium/ and dist/firefox/
```

## Try it in Chrome or Edge

With the Windows MSI installed, the host is already registered; skip step 1 and `cargo build`, and just load the extension (step 3).

1. Build the Rust side and register the host for your user: `cargo build`, then `target/debug/focus-nmh --register` (add `--dry-run` to see what it would change first).
2. Start the daemon: `cargo run -p focus-daemon`.
3. Open `chrome://extensions`, turn on Developer mode, choose **Load unpacked** and pick `extension/dist/chromium`.
4. Start a session in `cargo run -p focus-ui`, then visit a blocked site.

To undo the registration: `target/debug/focus-nmh --unregister`.

## Try it in Firefox (128 or later)

1. Register the host as above; `--register` covers Firefox too (a second manifest, and `HKCU\Software\Mozilla\NativeMessagingHosts` on Windows or `~/.mozilla/native-messaging-hosts` on Linux).
2. Start the daemon.
3. Open `about:debugging#/runtime/this-firefox`, choose **Load Temporary Add-on** and pick `extension/dist/firefox/manifest.json`. A temporary add-on is gone after Firefox restarts; a lasting install needs the signed `.xpi` from step 10b.
4. In `about:addons` → Focus, allow **Run in Private Windows**, or Focus closes Firefox during sessions.
5. Start a session and visit a blocked site.
