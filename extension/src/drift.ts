// The drift prompt: after several hops away from the session's starting sites, the tab shows
// the trail and offers a way back. The starting ("home") sites are the ones visited in the first
// minutes of the session, plus those the configuration allows. A hop is a page load in a tab on
// a new site outside home; coming back home clears the tab's trail.

import { parsePattern } from "./dnr";
import type { BlockConfig } from "./protocol";
import { siteOf, within } from "./sites";

/** How long after a session starts the sites visited count as home. */
export const HOME_WINDOW_MS = 5 * 60 * 1000;

/** One tab's hops since it left home. */
export interface TabTrail {
  /** Sites visited since leaving home, oldest first. */
  trail: string[];
  /** The last home page the tab was on, for "Back to …". */
  back: string | null;
}

/** Drift tracking for one session. */
export interface DriftState {
  /** When the session it belongs to started. */
  session: number;
  /** Sites visited in the session's first minutes. */
  home: string[];
  /** Trails by tab id. */
  tabs: Record<string, TabTrail>;
}

/** Fresh tracking for the session that started at `startedAt`. */
export function freshDrift(startedAt: number): DriftState {
  return { session: startedAt, home: [], tabs: {} };
}

/** The hosts a configuration allows, which always count as home. */
export function allowedSites(config: BlockConfig): string[] {
  return config.rules
    .filter((rule) => rule.verdict === "allow")
    .map((rule) => parsePattern(rule.pattern)?.host)
    .filter((host): host is string => host !== undefined)
    .map((host) => (host.startsWith("www.") ? host.slice(4) : host));
}

/**
 * Records a page load in `tabId`. Returns the trail when it has reached `hops` (time for the
 * drift prompt), otherwise null. `hops` of 0 turns the prompt off but still tracks.
 */
export function recordVisit(
  state: DriftState,
  tabId: number,
  url: string,
  now: number,
  hops: number,
  allowed: string[],
): string[] | null {
  const site = siteOf(url);
  if (site === null) return null;
  const tab = (state.tabs[tabId] ??= { trail: [], back: null });
  if (now < state.session + HOME_WINDOW_MS && !state.home.includes(site)) state.home.push(site);
  const home = [...state.home, ...allowed].some((homeSite) => within(site, homeSite));
  if (home) {
    tab.trail = [];
    tab.back = url;
    return null;
  }
  if (tab.trail.at(-1) === site) return null;
  tab.trail.push(site);
  return hops > 0 && tab.trail.length >= hops ? [...tab.trail] : null;
}

/** A tab opened from another starts with the other's trail. */
export function inheritTrail(state: DriftState, fromTabId: number, toTabId: number): void {
  const from = state.tabs[fromTabId];
  if (from) state.tabs[toTabId] = { trail: [...from.trail], back: from.back };
}

/** "Keep going": the prompt comes back only after another full set of hops. */
export function acknowledge(state: DriftState, tabId: number): void {
  const tab = state.tabs[tabId];
  if (tab) tab.trail = [];
}

/** Forgets a closed tab. */
export function forgetTab(state: DriftState, tabId: number): void {
  delete state.tabs[tabId];
}
