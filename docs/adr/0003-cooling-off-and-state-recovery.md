# 3. Cooling-off rules and state-file recovery

- Status: accepted
- Date: 2026-10-23

## Context

The design says rules freeze during a session and that edits apply only after a cooling-off delay (e.g. 24 h), so rules can't be loosened on impulse. A running session already copies its configuration when it starts, so edits can't loosen *that* session. The risk is a looser configuration being ready for the *next* session, especially after ending a Light or Deep session early.

The daemon must also survive crashes and restarts without losing a running session (ADR 0001). Its state is a JSON file, and the app can't use admin rights to protect it.

## Decision

**Cooling-off is pure core logic** (`focus_core::ConfigState::sync`). The settings file is only the *desired* configuration. What's in effect, plus the queue of pending edits, lives in the persisted state:

- A new configuration, or an edit to a configuration that is neither protected nor already cooling off, applies immediately.
- An edit or deletion of a **protected** configuration (for now, the running session's; scheduled ones join in step 7) is queued until `now + cooling_off`. The default is 24 h.
- A configuration with a change already queued stays guarded after protection ends. Ending a session therefore never rushes an edit through.
- Re-reading the same edit keeps its original time. A different edit starts the wait again. Editing back to what's in effect cancels the queued change.
- Every edit waits, including ones that only tighten the rules. Judging automatically whether an edit is stricter is hard to get right, and a running session already uses its own copy of the rules.

**The state file** (`state-json`) is written atomically, keeps one backup, and quarantines corrupt files:

- **Save:** write `state.json.tmp` and `fsync` it, rename `state.json` → `state.json.bak`, then rename the temp file → `state.json`. Rust's `fs::rename` replaces an existing file on both Windows and Linux.
- **Load:** read `state.json`, or `state.json.bak` if the main file is missing or corrupt.
  - A corrupt file is renamed to `state.json.corrupt[-N]`, never deleted or read again.
  - If no readable copy remains, load returns `Corrupt` once, and the caller starts fresh.
- **Versioning:** the file carries a `version`. A file from a newer version is left untouched and reported as unavailable, so a downgrade can't destroy it.

**A missing settings file means "no configurations".** It isn't an error.

## Consequences

- Corrupting the main state file doesn't end a running session: the backup brings it back.
- Without admin rights, someone who deletes or corrupts *both* state files loses the session and the cooling-off queue. The next sync then applies the settings file directly. This sits with the "uninstall" row of the design's bypass table: possible, but it takes several deliberate steps.
- Tightening edits to a protected configuration also wait 24 h. Revisit if this annoys in daily use.
- The cooling-off delay is a parameter of `sync` but isn't in the settings file yet. If it ever is, changes to it must go through cooling-off too.
- The daemon (step 3) has to call `sync` whenever it reloads settings, passing the running session's config as protected, and persist the result.
