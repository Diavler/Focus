// The service worker: wiring only. It keeps the native connection, sends heartbeats, mirrors the
// running session's rules into declarativeNetRequest, checks in-page (SPA) navigations, answers
// the content script and interstitial, and grants "continue" for Ask and Delay.

import browser, { type Runtime } from "webextension-polyfill";

import { GRANT_PRIORITY, interstitialUrl, toDnrRules } from "./dnr";
import { delaySeconds } from "./friction";
import { NativeClient, type PortLike } from "./native";
import { HOST_NAME, actionOf, runningSession, type RunningSession, type Verdict } from "./protocol";
import { browserName } from "./platform/chromium";

/** How often to send a heartbeat and refresh rules while the worker is awake. */
const HEARTBEAT_MS = 5000;
/** How long "continue" on an Ask or Delay page lets that site load in that tab. */
const GRANT_MS = 5 * 60 * 1000;

const client = new NativeClient(() => browser.runtime.connectNative(HOST_NAME) as unknown as PortLike);
const base = browser.runtime.getURL("");
let appliedRules = "";

/** One heartbeat plus a rules refresh. If the daemon can't be reached, the last rules stay until
 * the session they belong to would have ended, so stopping the daemon isn't a way around them. */
async function tick(): Promise<void> {
  try {
    const incognito = await browser.extension.isAllowedIncognitoAccess();
    await client.request({ type: "heartbeat", browser: browserName(), incognito_allowed: incognito });
    const status = await client.request({ type: "status" });
    if (status.type !== "status") throw new Error(`status: ${JSON.stringify(status)}`);
    const session = runningSession(status.session);
    await browser.storage.local.set({ sessionEndsAt: session?.ends_at ?? 0 });
    await syncRules(session);
  } catch {
    const { sessionEndsAt } = (await browser.storage.local.get("sessionEndsAt")) as { sessionEndsAt?: number };
    if (!sessionEndsAt || Date.now() >= sessionEndsAt) await syncRules(null);
  }
}

/** Replaces the dynamic rules with the session's, if they changed. */
async function syncRules(session: RunningSession | null): Promise<void> {
  const rules = session ? toDnrRules(session.spec.config, base) : [];
  const key = JSON.stringify(rules);
  if (key === appliedRules) return;
  const existing = await browser.declarativeNetRequest.getDynamicRules();
  await browser.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: existing.map((rule) => rule.id),
    addRules: rules as never,
  });
  appliedRules = key;
}

async function verdictFor(url: string): Promise<Verdict> {
  const response = await client.request({ type: "rules_for", url, config: null });
  return response.type === "verdict" ? response.verdict : "allow";
}

interface Grant {
  id: number;
  tabId: number;
  host: string;
  expires: number;
}

async function grants(): Promise<Grant[]> {
  const { grants } = (await browser.storage.session.get("grants")) as { grants?: Grant[] };
  return (grants ?? []).filter((grant) => grant.expires > Date.now());
}

async function isGranted(tabId: number, url: string): Promise<boolean> {
  const host = new URL(url).hostname;
  return (await grants()).some((grant) => grant.tabId === tabId && (host === grant.host || host.endsWith(`.${grant.host}`)));
}

/** Lets `url`'s site load in `tabId` for a while, then navigates there. Block can't be granted. */
async function grantAndGo(tabId: number, url: string): Promise<boolean> {
  const action = actionOf(await verdictFor(url));
  if (action !== "ask" && action !== "delay") return false;
  const host = new URL(url).hostname;
  const existing = await browser.declarativeNetRequest.getSessionRules();
  const id = Math.max(0, ...existing.map((rule) => rule.id)) + 1;
  await browser.declarativeNetRequest.updateSessionRules({
    addRules: [
      {
        id,
        priority: GRANT_PRIORITY,
        action: { type: "allow" },
        condition: { requestDomains: [host], tabIds: [tabId], resourceTypes: ["main_frame"] },
      },
    ] as never,
  });
  const grant: Grant = { id, tabId, host, expires: Date.now() + GRANT_MS };
  await browser.storage.session.set({ grants: [...(await grants()), grant] });
  browser.alarms.create(`grant:${id}`, { when: grant.expires });
  await browser.tabs.update(tabId, { url });
  return true;
}

async function endGrant(id: number): Promise<void> {
  await browser.declarativeNetRequest.updateSessionRules({ removeRuleIds: [id] });
  await browser.storage.session.set({ grants: (await grants()).filter((grant) => grant.id !== id) });
}

/** The Delay wait for `url`'s site; each call counts as one more visit this browser session. */
async function nextDelay(url: string): Promise<number> {
  const key = `visits:${new URL(url).hostname}`;
  const visits = ((await browser.storage.session.get(key)) as Record<string, number>)[key] ?? 0;
  await browser.storage.session.set({ [key]: visits + 1 });
  return delaySeconds(visits);
}

type Message =
  | { kind: "verdict"; url: string }
  | { kind: "delay"; url: string }
  | { kind: "continue"; url: string };

browser.runtime.onMessage.addListener((message: unknown, sender: Runtime.MessageSender) => {
  const request = message as Message;
  switch (request.kind) {
    case "verdict":
      return verdictFor(request.url).then(
        (verdict) => actionOf(verdict) === "hide_feed",
        () => false,
      );
    case "delay":
      return nextDelay(request.url);
    case "continue": {
      const tabId = sender.tab?.id;
      return tabId === undefined ? Promise.resolve(false) : grantAndGo(tabId, request.url).catch(() => false);
    }
    default:
      return undefined;
  }
});

// Single-page apps (Reddit, YouTube) change pages without a load, which DNR never sees.
browser.webNavigation.onHistoryStateUpdated.addListener(async ({ tabId, frameId, url }) => {
  if (frameId !== 0 || (await isGranted(tabId, url))) return;
  const action = actionOf(await verdictFor(url).catch((): Verdict => "allow"));
  if (action === "ask" || action === "delay" || action === "block") {
    await browser.tabs.update(tabId, { url: interstitialUrl(base, action, url) });
  }
});

browser.alarms.onAlarm.addListener(({ name }) => {
  if (name.startsWith("grant:")) void endGrant(Number(name.slice("grant:".length)));
  else if (name === "tick") void tick();
});

// The interval runs while the worker is awake (the open native port keeps it awake); the alarm
// wakes it again if the browser suspended it.
browser.alarms.create("tick", { periodInMinutes: 0.5 });
setInterval(() => void tick(), HEARTBEAT_MS);
void tick();
