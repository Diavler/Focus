// The page Ask, Delay, Block, the drift prompt and the tab cap send you to:
// `interstitial.html?a=<action>#<original url>`.

import browser from "webextension-polyfill";

import type { Friction } from "./protocol";
import { siteOf } from "./sites";

const params = new URLSearchParams(location.search);
const action = params.get("a") ?? "block";
/** After an in-page navigation the blocked URL is the previous history entry, so skip it too. */
const stepsBack = params.get("spa") === "1" ? 2 : 1;
const target = location.hash.slice(1);

function show(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (!element) throw new Error(`interstitial.html has no #${id}`);
  element.hidden = false;
  return element;
}

function element<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

function goBack(): void {
  if (history.length > stepsBack) history.go(-stepsBack);
  else window.close();
}

async function continueToSite(): Promise<void> {
  const granted = await browser.runtime.sendMessage({ kind: "continue", url: target });
  if (!granted) show("refused");
}

async function startDelay(): Promise<void> {
  const button = show("delay").querySelector("[data-continue]") as HTMLButtonElement;
  const count = element("countdown");
  let left = (await browser.runtime.sendMessage({ kind: "delay", url: target })) as number;
  count.textContent = String(left);
  const timer = setInterval(() => {
    left -= 1;
    count.textContent = String(Math.max(left, 0));
    if (left <= 0) {
      clearInterval(timer);
      button.disabled = false;
    }
  }, 1000);
  button.addEventListener("click", () => void continueToSite());
}

interface UnlockInfo {
  strictness: "light" | "deep" | "locked";
  unlocksLeft: number;
  friction: Friction;
}

/** Offers an unlock on the block page: free in Light, from the budget with a reason in Deep. */
async function offerUnlock(): Promise<void> {
  // An extension updated on disk but not reloaded runs an older worker, which doesn't answer.
  const info = (await browser.runtime
    .sendMessage({ kind: "unlock-info" })
    .catch(() => undefined)) as UnlockInfo | null | undefined;
  if (info === undefined) {
    element("locked-note").textContent =
      "To unlock pages, reload the Focus extension on your browser's extensions page.";
    show("locked-note");
    return;
  }
  if (info === null) {
    element("locked-note").textContent = "Unlocking needs the Focus app running.";
    show("locked-note");
    return;
  }
  if (info.strictness === "locked") {
    show("locked-note");
    return;
  }
  show("unlock");
  const button = element<HTMLButtonElement>("unlock-button");
  const reason = element<HTMLInputElement>("unlock-reason");
  const note = element("unlock-note");
  const minutes = info.friction.unlock_minutes;
  const deep = info.strictness === "deep";
  if (deep) {
    reason.hidden = false;
    button.textContent = `Use an unlock for ${minutes} min`;
    note.textContent = `${info.unlocksLeft} of ${info.friction.unlocks_per_day} unlocks left in the last 24 hours.`;
    const update = () => {
      button.disabled = info.unlocksLeft === 0 || reason.value.trim() === "";
    };
    reason.addEventListener("input", update);
    update();
  } else {
    button.textContent = `Unlock for ${minutes} min`;
  }
  button.addEventListener("click", async () => {
    button.disabled = true;
    const refusal = (await browser.runtime.sendMessage({
      kind: "unlock",
      url: target,
      reason: reason.value,
    })) as string | null;
    if (refusal !== null) {
      note.textContent = refusal;
      button.disabled = false;
    }
  });
}

/** The drift prompt: the trail away from the session's sites, and a way back. */
function showDrift(): void {
  show("drift");
  const trail = (params.get("trail") ?? "").split(" ").filter(Boolean);
  element("trail").textContent = trail.join(" → ");
  const back = params.get("back") ?? "";
  const home = back ? siteOf(back) : null;
  const homeButton = element<HTMLButtonElement>("drift-back");
  homeButton.textContent = home ? `Back to ${home}` : "Go back";
  homeButton.addEventListener("click", () => {
    if (back) location.href = back;
    else goBack();
  });
  element("drift-continue").addEventListener("click", () => {
    void browser.runtime.sendMessage({ kind: "drift-continue", url: target });
  });
}

interface OpenTab {
  id: number;
  title: string;
  url: string;
}

/** The tab cap: lists the other tabs to close one, then opens the page this tab was opening. */
async function showTabCap(): Promise<void> {
  show("tabs");
  const cap = (await browser.runtime.sendMessage({ kind: "tab-cap" })) as number;
  element("cap").textContent = String(cap);
  const list = element("tab-list");
  const proceed = element<HTMLButtonElement>("tab-continue");
  const render = async () => {
    const tabs = (await browser.runtime.sendMessage({ kind: "tabs" })) as OpenTab[];
    list.replaceChildren(
      ...tabs.map((tab) => {
        const item = document.createElement("li");
        const label = document.createElement("span");
        label.textContent = tab.title || siteOf(tab.url) || tab.url;
        const close = document.createElement("button");
        close.textContent = "Close";
        close.addEventListener("click", async () => {
          await browser.runtime.sendMessage({ kind: "close-tab", id: tab.id });
          await render();
        });
        item.append(label, close);
        return item;
      }),
    );
    // This tab counts too.
    proceed.disabled = cap > 0 && tabs.length + 1 > cap;
  };
  proceed.addEventListener("click", () => {
    void browser.runtime.sendMessage({ kind: "tab-continue", url: target });
  });
  element("close-self").addEventListener("click", async () => {
    const tab = await browser.tabs.getCurrent();
    if (tab?.id !== undefined) await browser.tabs.remove(tab.id);
  });
  await render();
}

element("site").textContent = target;
for (const back of document.querySelectorAll("[data-back]")) back.addEventListener("click", goBack);

if (action === "ask") {
  show("ask")
    .querySelector("[data-continue]")
    ?.addEventListener("click", () => void continueToSite());
} else if (action === "delay") {
  void startDelay();
} else if (action === "drift") {
  showDrift();
} else if (action === "tabs") {
  void showTabCap();
} else {
  show("block");
  void offerUnlock();
}
