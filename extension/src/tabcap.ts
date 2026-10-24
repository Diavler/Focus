// The tab cap: during a session, opening one more tab than the limit asks you to close one
// first. Tabs the browser restores at startup don't count as opened.

/** How long after the browser starts new tabs are taken as restored, not opened. */
export const STARTUP_GRACE_MS = 15_000;

/** Whether a tab just opened, making `openTabs`, goes over `cap` (0 is no cap). */
export function overCap(openTabs: number, cap: number, browserStartedAt: number, now: number): boolean {
  return cap > 0 && openTabs > cap && now - browserStartedAt >= STARTUP_GRACE_MS;
}

/** Where Continue takes the capped tab: the page it was opening, or a new tab page. */
export function continueTarget(intended: string): string {
  return /^https?:\/\//i.test(intended) ? intended : "chrome://newtab/";
}
