// Firefox-only details. Same exports as chromium.ts; the build picks one per browser.

/**
 * Where a capped tab goes when the page it was opening isn't known. Firefox doesn't let
 * extensions open its own new tab page, so it's a blank page.
 */
export const NEW_TAB_URL = "about:blank";

/** This browser, for the daemon's heartbeat list. */
export function browserName(): string {
  return "firefox";
}
