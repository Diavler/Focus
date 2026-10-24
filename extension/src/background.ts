// The service worker: wiring only. It keeps the native connection, sends heartbeats, mirrors the
// running session's rules into declarativeNetRequest, checks in-page (SPA) navigations, keeps
// feed hiding up to date, and grants "continue" for Ask and Delay.
//
// Fail-closed: when the daemon can't be asked, verdicts come from the last session seen (kept in
// storage.local) until that session would have ended, so stopping the daemon isn't a bypass.

import browser, { type Runtime } from "webextension-polyfill";

import { type DnrRule, decide, interstitialUrl, toDnrRules } from "./dnr";
import { FEED_MATCHES } from "./feeds";
import { delaySeconds } from "./friction";
import { type Grant, delayKey, delayOver, grantRule, isGranted, makeGrant } from "./grants";
import { NativeClient, type PortLike } from "./native";
import { HOST_NAME, actionOf, runningSession, type RunningSession, type Verdict } from "./protocol";
import { browserName } from "./platform/chromium";

/** How often to send a heartbeat and refresh rules while the worker is awake. */
const HEARTBEAT_MS = 5000;

const client = new NativeClient(() => browser.runtime.connectNative(HOST_NAME) as unknown as PortLike);
const base = browser.runtime.getURL("");
let computedRules = "";
let refreshing: Promise<void> | null = null;

/** The last running session seen, or null once its time is up. */
async function knownSession(): Promise<RunningSession | null> {
  const { session } = (await browser.storage.local.get("session")) as { session?: RunningSession | null };
  return session && Date.now() < session.ends_at ? session : null;
}

/** Heartbeat plus rules refresh, one at a time: a call while one runs waits for it instead. */
function tick(): Promise<void> {
  refreshing ??= refresh()
    .catch((error: unknown) => console.warn("focus: refresh failed", error))
    .finally(() => {
      refreshing = null;
    });
  return refreshing;
}

async function refresh(): Promise<void> {
  let session: RunningSession | null;
  try {
    const incognito = await browser.extension.isAllowedIncognitoAccess();
    await client.request({ type: "heartbeat", browser: browserName(), incognito_allowed: incognito });
    const status = await client.request({ type: "status" });
    if (status.type !== "status") throw new Error(`status: ${JSON.stringify(status)}`);
    session = runningSession(status.session);
    await browser.storage.local.set({ session });
  } catch {
    session = await knownSession();
  }
  if (await syncRules(session)) await pushFeedVerdicts(session);
}

/** Replaces the dynamic rules with the session's if they changed; true if they did. */
async function syncRules(session: RunningSession | null): Promise<boolean> {
  const rules = session ? toDnrRules(session.spec.config, base) : [];
  const key = JSON.stringify(rules);
  if (key === computedRules) return false;
  const existing = await browser.declarativeNetRequest.getDynamicRules();
  await browser.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: existing.map((rule) => rule.id),
    addRules: (await supportedOnly(rules)) as never,
  });
  computedRules = key;
  return true;
}

/** Drops rules whose regex Chrome can't compile, so one bad pattern doesn't sink the rest. */
async function supportedOnly(rules: DnrRule[]): Promise<DnrRule[]> {
  const kept: DnrRule[] = [];
  for (const rule of rules) {
    const { isSupported, reason } = await browser.declarativeNetRequest.isRegexSupported({
      regex: rule.condition.regexFilter,
      isCaseSensitive: false,
    });
    if (isSupported) kept.push(rule);
    else console.warn(`focus: skipping a rule Chrome can't use (${reason}): ${rule.condition.regexFilter}`);
  }
  return kept;
}

/** The verdict for `url`: the daemon's, or the last session's rules if the daemon can't answer. */
async function verdictFor(url: string): Promise<Verdict> {
  try {
    const response = await client.request({ type: "rules_for", url, config: null });
    if (response.type === "verdict") return response.verdict;
  } catch {
    // Fall through to the local decision.
  }
  const session = await knownSession();
  return session ? decide(session.spec.config, url).verdict : "allow";
}

/** Tells open Reddit and YouTube tabs whether to hide feeds, after the rules changed. */
async function pushFeedVerdicts(session: RunningSession | null): Promise<void> {
  for (const tab of await browser.tabs.query({ url: FEED_MATCHES })) {
    if (tab.id === undefined || !tab.url) continue;
    const verdict = session ? decide(session.spec.config, tab.url).verdict : "allow";
    await tellTab(tab.id, actionOf(verdict) === "hide_feed");
  }
}

async function tellTab(tabId: number, hide: boolean): Promise<void> {
  await browser.tabs.sendMessage(tabId, { kind: "hide", hide }).catch(() => undefined);
}

async function grants(): Promise<Grant[]> {
  const { grants } = (await browser.storage.session.get("grants")) as { grants?: Grant[] };
  return (grants ?? []).filter((grant) => grant.expires > Date.now());
}

async function isGrantedFor(tabId: number, url: string): Promise<boolean> {
  const session = await knownSession();
  return session !== null && isGranted(await grants(), tabId, url, decide(session.spec.config, url), Date.now());
}

/** The Delay wait for `url` in `tabId`; each call counts as one more visit to the site. */
async function startDelay(tabId: number, url: string): Promise<number> {
  const session = await knownSession();
  const regexFilter = session ? decide(session.spec.config, url).regexFilter : null;
  const visitsKey = `visits:${new URL(url).hostname}`;
  const visits = ((await browser.storage.session.get(visitsKey)) as Record<string, number>)[visitsKey] ?? 0;
  const seconds = delaySeconds(visits);
  await browser.storage.session.set({ [visitsKey]: visits + 1 });
  if (regexFilter) await browser.storage.session.set({ [delayKey(tabId, regexFilter)]: Date.now() + seconds * 1000 });
  return seconds;
}

/** Grants Continue for the rule that stopped `url` and navigates there. Block, or a Delay whose
 * wait isn't over, is refused. */
async function grantAndGo(tabId: number, url: string): Promise<boolean> {
  const session = await knownSession();
  if (!session) return false;
  const decision = decide(session.spec.config, url);
  const now = Date.now();
  if (actionOf(decision.verdict) === "delay" && decision.regexFilter) {
    const key = delayKey(tabId, decision.regexFilter);
    const readyAt = ((await browser.storage.session.get(key)) as Record<string, number>)[key];
    if (!delayOver(readyAt, now)) return false;
  }
  const existing = await browser.declarativeNetRequest.getSessionRules();
  const grant = makeGrant(Math.max(0, ...existing.map((rule) => rule.id)) + 1, tabId, decision, now);
  if (!grant) return false;
  await browser.declarativeNetRequest.updateSessionRules({ addRules: [grantRule(grant)] as never });
  await browser.storage.session.set({ grants: [...(await grants()), grant] });
  browser.alarms.create(`grant:${grant.id}`, { when: grant.expires });
  await browser.tabs.update(tabId, { url });
  return true;
}

async function endGrant(id: number): Promise<void> {
  await browser.declarativeNetRequest.updateSessionRules({ removeRuleIds: [id] });
  await browser.storage.session.set({ grants: (await grants()).filter((grant) => grant.id !== id) });
}

type Message =
  | { kind: "verdict"; url: string }
  | { kind: "delay"; url: string }
  | { kind: "continue"; url: string };

browser.runtime.onMessage.addListener((message: unknown, sender: Runtime.MessageSender) => {
  const request = message as Message;
  const tabId = sender.tab?.id;
  switch (request.kind) {
    case "verdict":
      return verdictFor(request.url).then((verdict) => actionOf(verdict) === "hide_feed");
    case "delay":
      return tabId === undefined ? Promise.resolve(0) : startDelay(tabId, request.url);
    case "continue":
      return tabId === undefined ? Promise.resolve(false) : grantAndGo(tabId, request.url).catch(() => false);
    default:
      return undefined;
  }
});

// Single-page apps (Reddit, YouTube) change pages without a load, which DNR never sees.
browser.webNavigation.onHistoryStateUpdated.addListener(async ({ tabId, frameId, url }) => {
  if (frameId !== 0) return;
  const action = actionOf(await verdictFor(url));
  const stopped = action === "ask" || action === "delay" || action === "block";
  if (stopped && (action === "block" || !(await isGrantedFor(tabId, url)))) {
    await browser.tabs.update(tabId, { url: interstitialUrl(base, action, url, true) });
    return;
  }
  await tellTab(tabId, action === "hide_feed");
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
