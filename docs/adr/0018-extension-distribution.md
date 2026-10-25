# 18. Ship the extension as an unpacked Chromium zip and an unlisted signed Firefox add-on

- Status: accepted
- Date: 2026-10-25

## Context

The extension is required (ADR 0005), so each release needs a version users can install for Chromium browsers and for Firefox. The native messaging host only accepts the fixed IDs in its manifests (`hjeodjhhiiaojdakfafaebijlmdckebo`, `focus@focus-app.local`). Store listings mean accounts, fees, review queues and a public page for an app that has no server and no accounts.

## Decision

- **Chromium:** `Focus-<version>-chromium-extension.zip`, loaded with **Load unpacked**. The manifest `key` keeps the ID fixed wherever the folder lives. No Chrome Web Store listing for now.
- **Firefox:** `Focus-<version>-firefox-extension.xpi`, signed by AMO on the **unlisted** channel (automated review, no public page). Release Firefox only installs signed add-ons, so this is the only lasting install there.
- `release.yml` builds both on every packaging PR and lints the Firefox build with `web-ext lint`. Only a `v*` tag signs, since each signed version number can be uploaded once. Signing uses `web-ext` (pinned) with the repository secrets `AMO_JWT_ISSUER` and `AMO_JWT_SECRET`, an API key from a free AMO developer account.
- The extension's version (`package.json`, both manifests) must equal `[workspace.package]` in `Cargo.toml`; the workflow fails otherwise.

## Consequences

- Neither build updates itself: users install each release by hand (unzip over the old folder and click reload, or open the new `.xpi`). An `update_url` or store listing can come later.
- Chromium shows its usual developer-mode notices for unpacked extensions.
- A bump means editing the version in `Cargo.toml`, `extension/package.json` (and its lock file) and both manifests; the check catches a missed one.
