// The tab cap: during a session, opening one more tab than the limit asks you to close one
// first. Tabs the browser restores at startup don't count as opened.

/** How long after the browser starts new tabs are taken as restored, not opened. */
export const STARTUP_GRACE_MS = 15_000;

/** Whether `openTabs` (counting the one just opened) goes over `cap` (0 is no cap). */
export function overCap(openTabs: number, cap: number): boolean {
  return cap > 0 && openTabs > cap;
}

/** Whether a tab created at `now` is one the browser restored at startup, not one opened. */
export function restoredAtStartup(browserStartedAt: number, now: number): boolean {
  return now - browserStartedAt < STARTUP_GRACE_MS;
}

/** Where Continue takes the capped tab: the page it was opening, or `newTab` (the platform's). */
export function continueTarget(intended: string, newTab: string): string {
  return /^https?:\/\//i.test(intended) ? intended : newTab;
}
