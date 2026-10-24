// Runs on Reddit and YouTube: hides feeds while the session's verdict for the page is Hide feed.
// It asks once when the page loads; after that the background tells it when the verdict changes
// (in-page navigation, or a session starting or ending).

import browser from "webextension-polyfill";

import { hideFeedCss } from "../feeds";

const STYLE_ID = "focus-hide-feed";
const css = hideFeedCss(location.hostname);
/** Retries for the first check, e.g. while the service worker is starting. */
const FIRST_CHECK_ATTEMPTS = 5;

function setHidden(hidden: boolean): void {
  const existing = document.getElementById(STYLE_ID);
  if (!hidden || !css) {
    existing?.remove();
    return;
  }
  if (existing) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = css;
  (document.head ?? document.documentElement).append(style);
}

async function firstCheck(): Promise<void> {
  for (let attempt = 0; attempt < FIRST_CHECK_ATTEMPTS; attempt++) {
    try {
      const hide = await browser.runtime.sendMessage({ kind: "verdict", url: location.href });
      setHidden(hide === true);
      return;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
  }
}

browser.runtime.onMessage.addListener((message: unknown) => {
  const update = message as { kind?: string; hide?: boolean };
  if (update.kind === "hide") setHidden(update.hide === true);
  return undefined;
});

void firstCheck();
