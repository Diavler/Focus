# 14. Release on Windows first; Linux parity after

- Status: accepted
- Date: 2026-10-24

## Context

The build plan had Linux (KDE Plasma) parity as step 9 and a release for both OSes as step 10 (ADR 0004 left the Linux packaging question to step 10). The developer works on Windows and has no KDE machine to hand. They asked to move Linux support after the first release and to spend the time before it on polish, keeping Linux in mind.

## Decision

- Order: polish (step P, sub-steps P1, P2, ...), then the release (step 10, Windows only: portable zip and MSI), then Linux parity (step 9).
- Step numbers stay as they are, since code, error messages and ADRs refer to "step 9" for Linux.
- Linux stays supported in the code: platform code stays behind the ports, the `PendingEnforcer`, `focus-hotkey` and keyring placeholders remain, and Linux keeps building and passing CI on every PR.
- The Linux packaging choice in ADR 0004 (`.deb`/`.rpm` against the portable layout) moves to step 9.

## Consequences

- The first release says it is Windows only. Linux users can build from source but get no enforcement outside the browser and no autostart, hotkey or keyring until step 9.
- The deferred KDE manual checks (steps 3–8, plus 8b's Firefox and portal checks) wait for step 9.
- Polish work must not add Windows-only assumptions to the UI. Platform differences go through ports (as with `Silencing` in ADR 0012).
