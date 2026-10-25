# 17. Own release workflow and a per-user MSI instead of cargo-dist

- Status: accepted
- Date: 2026-10-25

## Context

The design named `cargo-dist` for releases. Focus ships three exes from three packages (`focus-daemon`, `focus-ui`, `focus-nmh`) that must sit in one folder. cargo-dist makes one archive and one MSI per package, so a release would be three zips and three MSIs. Its default MSI also installs per machine into Program Files, which needs admin rights and is read-only, so `data/` couldn't sit beside the exes (ADR 0004).

## Decision

- `.github/workflows/release.yml` builds on a `v*` tag and attaches the files to a **draft** GitHub release. Pull requests that touch the packaging build the same files without releasing.
- `scripts/package-windows.ps1` builds the three exes and makes `Focus-<version>-windows-x64.zip` (one folder: exes, `LICENSE-MIT`, `LICENSE-APACHE`, the fonts' `OFL.txt`) and an MSI from the same folder.
- `installer/windows/focus.wxs` (WiX v5, pinned to 5.0.2) is a **per-user** MSI: it installs to `%LOCALAPPDATA%\Programs\Focus` with no UAC prompt and adds a Start menu shortcut. Install runs `focus-nmh --register`, since the extension IDs are fixed. Uninstall runs `--unregister` and deletes the HKCU `Run` value `Focus`.
- The MSI never removes `data/`, because it didn't install it, so settings and state survive upgrades and uninstalls.

## Consequences

- No cargo-dist installers, updater or Homebrew/npm channels. They aren't needed for a Windows-only first release.
- Windows Installer requires an HKCU key path for per-user components (`HKCU\Software\Focus\Installer`, removed on uninstall). That's the only registry entry besides autostart and native messaging.
- Upgrading or uninstalling while Focus runs hits files in use, so the user closes Focus from the tray first. Windows Installer's own prompt covers the case where they don't.
- Linux packaging (step 9) adds its own job to the same workflow.
