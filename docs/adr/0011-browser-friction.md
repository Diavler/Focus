# 11. Browser friction: unlocks, growing Delay, drift prompt and tab cap

- Status: accepted
- Date: 2026-10-24

## Context

Step 8 makes bypasses slow and rabbit holes visible. The design (`docs/DESIGN.md`) gives Deep sessions overrides "from the daily unlock budget", a Delay that grows each visit, a drift prompt after several hops away from the session's starting sites, and a tab cap. Before this step:
- `OverridePolicy` (Free / SpendUnlock / Forbidden) existed in the core but nothing used it, and Block pages had no way through.
- Growing Delay lived only in the extension, counted in `storage.session`, so restarting the browser reset it.

Step 8 was split in two (build plan): this ADR covers 8a. The Later button, parking-lot hotkey and Do Not Disturb button are 8b.

## Decision

**Limits, with cooling-off.**
- A new core `Friction` (`unlocks_per_day` 3, `unlock_minutes` 10, `tab_cap` 10, `drift_hops` 4; 0 turns the cap and the prompt off) is written as an optional `[friction]` table in `settings.toml`. It's edited from Settings → General with sliders and hover help, through the existing `SettingsEditor`.
- The persisted `FrictionSetting` applies every limit a change tightens at once. If the change loosens any limit, all of it waits the cooling-off delay (24 h), like start-at-login (ADR 0008). Asking for the same loosening again keeps its time; asking for something else restarts it.
- Out-of-range values are settings problems (`Friction::problems`, shared by the TOML adapter and the fake).

**Unlocks.**
- The block page offers Unlock. The extension sends `Unlock { url, reason }`; the daemon checks that a session is running and that the page is really blocked.
- It then applies `UnlockLedger::unlock` under the session's `OverridePolicy`:
  - Light is free;
  - Deep needs a reason and spends one unlock;
  - Locked is refused.
- The answer is `Unlocked { until }`.
- The budget is a rolling 24 hours, not "per day":
  - it needs no time zone in the core;
  - it can't be doubled across midnight;
  - an unlock "in the future" (clock turned back) still counts.
- The ledger is persisted, so a restart doesn't refill it.
- The extension then adds a tab-scoped DNR allow until `until`. It covers the blocked page's *site* (host and subdomains, without `www.`), at the deciding rule's priority:
  - an allowlist's catch-all has no narrower rule to scope to, and scoping to it would open every site in the tab;
  - stricter, more specific rules on the same site still block.
- The extension checks it can make the grant *before* asking, so a Deep unlock is never spent on a page it can't open.
- Every grant (unlocks and Continue) records the session it was made in. Grants from another session are removed on the next refresh and never count, so an unlock can't carry over into a later (even Locked) session.
- Grant ids are allocated, and the stored grant list changed, through one queue, so simultaneous grants can't collide.
- In-page (SPA) navigations now honour grants for Block too.

**Growing Delay.**
- The daemon counts visits per site per session (`DelayLadder`, persisted): 10 s, +10 s per visit, at most 120 s. A new session starts again. Only pages behind a Delay rule are counted (and saved); anything else waits 0.
- The extension asks with `DelayVisit { url }` and uses the longer of the daemon's wait and its own local count, which stays as the fallback when the daemon can't be asked (keep the constants in sync, as with the matcher in ADR 0006).

**Drift prompt and tab cap stay in the extension.**
- Both work on tabs and navigations, which only the browser sees.
- Both are nudges, not enforcement, so they don't need a Rust copy to fail closed.
- They're pure TypeScript modules (`drift.ts`, `tabcap.ts`) with unit tests. The limits come from the daemon in `Status` (`friction`, `unlocks_left`, `friction_loosen_after`), and the last values seen are kept in `storage.local`.
- How drift works:
  - Home sites are those visited in the first 5 minutes of the session, plus hosts the configuration allows.
  - A hop is a top-level page load in a tab on a site outside home that isn't on the tab's trail yet, so bouncing between two sites isn't drift. Coming home clears the tab's trail, and tabs opened from a tab inherit its trail.
  - At `drift_hops`, the tab goes to the interstitial with the trail, "Back to <last home site>" and "Keep going". Keep going clears the trail, so the prompt comes back only after another full set of hops.
- How the tab cap works:
  - A tab opened during a session that makes more tabs than `tab_cap` goes to the interstitial. It lists the other tabs with Close buttons and opens the intended page once you're at the cap (or the browser's new tab page, which comes from `platform/`).
  - Tabs created in the first 15 s after the browser starts are taken as restored, not opened.

**Protocol.** `focus-nmh` now also relays `Unlock` and `DelayVisit` from the browser (ADR 0005). Both only work within the running session's limits. The UI shows the unlocks left in the session card, and a pending loosening under "Cooling off".

## Consequences

- No new dependencies. The protocol grew, so `focus-nmh` must be rebuilt and the extension reloaded (ADR 0007).
- With the daemon down, Unlock is refused (fail closed), while Delay, the drift prompt and the tab cap keep working from the last values seen.
- First-time setup: a `[friction]` written looser than the defaults before Focus first runs still waits 24 hours, like any loosening.
- A tab's drift trail and the tab cap's startup grace live in `storage.session`, so they reset with the browser. That's acceptable for nudges.
- Manual tests (Windows, Chrome): unlocking in Light, Deep and Locked sessions; the budget running out; the Delay wait growing across a browser restart; the drift prompt; the tab cap, including a browser restart with many tabs restored.
