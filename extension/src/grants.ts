// "Continue" on an Ask or Delay page: a short-lived allow for that tab, scoped to the rule that
// asked, never to the whole site, so stricter or more specific rules on the same site still apply.

import type { Decision, DnrRule } from "./dnr";
import { actionOf } from "./protocol";

/** How long Continue lasts. */
export const GRANT_MS = 5 * 60 * 1000;

/** An allow for one rule in one tab, until `expires` (ms since the epoch). */
export interface Grant {
  id: number;
  tabId: number;
  regexFilter: string;
  priority: number;
  expires: number;
}

/** A grant for the rule behind `decision`, or null if that verdict can't be granted (Block). */
export function makeGrant(id: number, tabId: number, decision: Decision, now: number): Grant | null {
  const action = actionOf(decision.verdict);
  if ((action !== "ask" && action !== "delay") || decision.regexFilter === null) return null;
  return { id, tabId, regexFilter: decision.regexFilter, priority: decision.priority, expires: now + GRANT_MS };
}

/**
 * The session rule for a grant: allows what its rule matches, in that tab, at that rule's own
 * priority. Chrome prefers `allow` on a priority tie, so it beats the rule it was granted for,
 * while more specific or stricter rules (higher priority) still win.
 */
export function grantRule(grant: Grant): DnrRule {
  return {
    id: grant.id,
    priority: grant.priority,
    action: { type: "allow" },
    condition: {
      regexFilter: grant.regexFilter,
      isUrlFilterCaseSensitive: false,
      resourceTypes: ["main_frame"],
      tabIds: [grant.tabId],
    },
  };
}

/** Whether a live grant lets `url` load in `tabId`, the same way the grant's DNR rule would. */
export function isGranted(grants: Grant[], tabId: number, url: string, decision: Decision, now: number): boolean {
  return grants.some(
    (grant) =>
      grant.tabId === tabId &&
      grant.expires > now &&
      grant.priority >= decision.priority &&
      new RegExp(grant.regexFilter, "i").test(url),
  );
}

/** Where a Delay wait is tracked: per tab and per rule. */
export function delayKey(tabId: number, regexFilter: string): string {
  return `delay:${tabId}:${regexFilter}`;
}

/** Whether a Delay wait that ends at `readyAt` is over; no recorded wait means it never started. */
export function delayOver(readyAt: number | undefined, now: number): boolean {
  return readyAt !== undefined && now >= readyAt;
}
