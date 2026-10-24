// Runs on Reddit and YouTube: hides feeds while the session's verdict for the page is Hide feed.
// Both sites change pages without reloading, so the URL is re-checked when it changes.

import browser from "webextension-polyfill";

import { hideFeedCss } from "../feeds";

const STYLE_ID = "focus-hide-feed";
const css = hideFeedCss(location.hostname);
let checkedUrl = "";

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

async function check(): Promise<void> {
  const url = location.href;
  if (url === checkedUrl) return;
  checkedUrl = url;
  const hide = await browser.runtime.sendMessage({ kind: "verdict", url }).catch(() => false);
  if (location.href === url) setHidden(hide === true);
}

void check();
setInterval(() => void check(), 500);
