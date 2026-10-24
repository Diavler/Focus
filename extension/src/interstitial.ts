// The page Ask, Delay and Block send you to: `interstitial.html?a=<action>#<original url>`.

import browser from "webextension-polyfill";

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
  const count = document.getElementById("countdown") as HTMLElement;
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

(document.getElementById("site") as HTMLElement).textContent = target;
for (const back of document.querySelectorAll("[data-back]")) back.addEventListener("click", goBack);

if (action === "ask") {
  show("ask")
    .querySelector("[data-continue]")
    ?.addEventListener("click", () => void continueToSite());
} else if (action === "delay") {
  void startDelay();
} else {
  show("block");
}
