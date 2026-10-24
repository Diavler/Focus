// The service worker: wiring only. It keeps the native connection, sends heartbeats, mirrors the
// running session's rules into declarativeNetRequest, checks in-page (SPA) navigations, keeps
// feed hiding up to date, grants "continue" for Ask and Delay and "unlock" for Block, and runs
// the drift prompt and the tab cap.
//
// Fail-closed: when the daemon can't be asked, verdicts come from the last session seen (kept in
// storage.local) until that session would have ended, so stopping the daemon isn't a bypass.

import browser, { type Runtime } from "webextension-polyfill";

import { type DnrRule, decide, interstitialUrl, toDnrRules } from "./dnr";
import {
  type DriftState,
  acknowledge,
  allowedSites,
  forgetTab,
  freshDrift,
  inheritTrail,
  recordVisit,
} from "./drift";
import { FEED_MATCHES } from "./feeds";
import { delaySeconds } from "./friction";
import { type Grant, delayKey, delayOver, grantRule, isGranted, makeGrant, makeUnlockGrant } from "./grants";
import { NativeClient, type PortLike } from "./native";
import {
  DEFAULT_FRICTION,
  HOST_NAME,
  actionOf,
  frictionFromStatus,
  isStopped,
  sessionFromStatus,
  unlockRefusal,
  type Friction,
  type RunningSession,
  type Verdict,
} from "./protocol";
import { browserName } from "./platform/chromium";
import { siteOf } from "./sites";
import { continueTarget, overCap } from "./tabcap";

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

/** The friction limits and unlocks left, as last reported by the daemon. */
interface KnownFriction {
  friction: Friction;
  unlocksLeft: number;
}

async function knownFriction(): Promise<KnownFriction> {
  const { friction } = (await browser.storage.local.get("friction")) as { friction?: KnownFriction };
  return friction ?? { friction: DEFAULT_FRICTION, unlocksLeft: DEFAULT_FRICTION.unlocks_per_day };
}

/** Runs `task` after every task queued before it, so read-modify-write of storage doesn't race. */
let queue: Promise<unknown> = Promise.resolve();
function serially<T>(task: () => Promise<T>): Promise<T> {
  const run = queue.then(task, task);
  queue = run.catch(() => undefined);
  return run;
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
    const reported = sessionFromStatus(status);
    if (reported === undefined) throw new Error(`status: ${JSON.stringify(status)}`);
    session = reported;
    await browser.storage.local.set({ session, friction: frictionFromStatus(status) ?? (await knownFriction()) });
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
    if (isStopped(response)) return "allow";
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

/**
 * The Delay wait for `url` in `tabId`; each call counts as one more visit to the site. The daemon
 * counts visits for the whole session; a count kept here covers a daemon that can't be asked.
 */
async function startDelay(tabId: number, url: string): Promise<number> {
  const session = await knownSession();
  const regexFilter = session ? decide(session.spec.config, url).regexFilter : null;
  const visitsKey = `visits:${siteOf(url) ?? url}`;
  const visits = ((await browser.storage.session.get(visitsKey)) as Record<string, number>)[visitsKey] ?? 0;
  await browser.storage.session.set({ [visitsKey]: visits + 1 });
  let seconds = delaySeconds(visits);
  try {
    const response = await client.request({ type: "delay_visit", url });
    if (response.type === "delay") seconds = Math.max(seconds, response.seconds);
  } catch {
    // Keep the local count's wait.
  }
  if (regexFilter) await browser.storage.session.set({ [delayKey(tabId, regexFilter)]: Date.now() + seconds * 1000 });
  return seconds;
}

/** What the block page needs to offer an unlock; null when the daemon can't be asked. */
async function unlockInfo(): Promise<{ strictness: string; unlocksLeft: number; friction: Friction } | null> {
  try {
    const status = await client.request({ type: "status" });
    const session = sessionFromStatus(status);
    const friction = frictionFromStatus(status);
    if (!session || !friction) return null;
    return { strictness: session.spec.strictness, ...friction };
  } catch {
    return null;
  }
}

/**
 * Asks the daemon to unlock `url`, then allows its site in `tabId` until the unlock ends and opens
 * it. Returns why it was refused, or null when it worked.
 */
async function unlock(tabId: number, url: string, reason: string): Promise<string | null> {
  const session = await knownSession();
  if (!session) return "No session is running.";
  let response;
  try {
    response = await client.request({ type: "unlock", url, reason });
  } catch {
    return "Unlocking needs the Focus app running.";
  }
  if (response.type !== "unlocked") return unlockRefusal(response);
  const existing = await browser.declarativeNetRequest.getSessionRules();
  const id = Math.max(0, ...existing.map((rule) => rule.id)) + 1;
  const grant = makeUnlockGrant(id, tabId, decide(session.spec.config, url), url, response.until);
  if (!grant) return "This page isn't blocked any more; try reloading.";
  await addGrant(grant);
  await browser.tabs.update(tabId, { url });
  void tick();
  return null;
}

async function addGrant(grant: Grant): Promise<void> {
  await browser.declarativeNetRequest.updateSessionRules({ addRules: [grantRule(grant)] as never });
  await browser.storage.session.set({ grants: [...(await grants()), grant] });
  browser.alarms.create(`grant:${grant.id}`, { when: grant.expires });
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
  await addGrant(grant);
  await browser.tabs.update(tabId, { url });
  return true;
}

/** Drift tracking for the running session (fresh when the session changed). */
async function driftState(session: RunningSession): Promise<DriftState> {
  const { drift } = (await browser.storage.session.get("drift")) as { drift?: DriftState };
  return drift && drift.session === session.started_at ? drift : freshDrift(session.started_at);
}

/** Records a top-level page load and shows the drift prompt when the tab has hopped too far. */
async function trackDrift(tabId: number, url: string): Promise<void> {
  const session = await knownSession();
  if (!session) return;
  const state = await driftState(session);
  const { friction } = await knownFriction();
  const trail = recordVisit(state, tabId, url, Date.now(), friction.drift_hops, allowedSites(session.spec.config));
  await browser.storage.session.set({ drift: state });
  if (!trail) return;
  const back = state.tabs[tabId]?.back ?? "";
  const params = new URLSearchParams({ a: "drift", trail: trail.join(" "), back });
  await browser.tabs.update(tabId, { url: `${base}interstitial.html?${params.toString()}#${url}` });
}

/** Changes drift tracking for the running session, if there is one. */
async function changeDrift(change: (state: DriftState) => void): Promise<void> {
  const session = await knownSession();
  if (!session) return;
  const state = await driftState(session);
  change(state);
  await browser.storage.session.set({ drift: state });
}

/** When the browser started (storage.session is cleared on every browser start). */
async function browserStartedAt(): Promise<number> {
  const { startedAt } = (await browser.storage.session.get("startedAt")) as { startedAt?: number };
  if (startedAt !== undefined) return startedAt;
  const now = Date.now();
  await browser.storage.session.set({ startedAt: now });
  return now;
}

/** Sends a tab opened over the cap to the tab-cap page, which asks to close one first. */
async function capTab(tab: { id?: number; pendingUrl?: string; url?: string }): Promise<void> {
  if (tab.id === undefined || !(await knownSession())) return;
  const { friction } = await knownFriction();
  const open = (await browser.tabs.query({})).length;
  if (!overCap(open, friction.tab_cap, await browserStartedAt(), Date.now())) return;
  const intended = tab.pendingUrl || tab.url || "";
  await browser.tabs.update(tab.id, { url: `${base}interstitial.html?a=tabs#${intended}` });
}

/** The other open tabs, for the tab-cap page. */
async function otherTabs(tabId: number): Promise<{ id: number; title: string; url: string }[]> {
  const tabs = await browser.tabs.query({});
  return tabs
    .filter((tab) => tab.id !== undefined && tab.id !== tabId)
    .map((tab) => ({ id: tab.id as number, title: tab.title ?? "", url: tab.url ?? "" }));
}

async function endGrant(id: number): Promise<void> {
  await browser.declarativeNetRequest.updateSessionRules({ removeRuleIds: [id] });
  await browser.storage.session.set({ grants: (await grants()).filter((grant) => grant.id !== id) });
}

type Message =
  | { kind: "verdict"; url: string }
  | { kind: "delay"; url: string }
  | { kind: "continue"; url: string }
  | { kind: "unlock-info" }
  | { kind: "unlock"; url: string; reason: string }
  | { kind: "drift-continue"; url: string }
  | { kind: "tabs" }
  | { kind: "close-tab"; id: number }
  | { kind: "tab-cap" }
  | { kind: "tab-continue"; url: string };

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
    case "unlock-info":
      return unlockInfo();
    case "unlock":
      return tabId === undefined
        ? Promise.resolve("No tab to unlock.")
        : unlock(tabId, request.url, request.reason).catch(() => "Focus couldn't unlock this page.");
    case "drift-continue":
      if (tabId === undefined) return undefined;
      return serially(() => changeDrift((state) => acknowledge(state, tabId)))
        .then(() => browser.tabs.update(tabId, { url: request.url }))
        .then(() => true);
    case "tabs":
      return tabId === undefined ? Promise.resolve([]) : otherTabs(tabId);
    case "close-tab":
      return browser.tabs.remove(request.id).then(
        () => true,
        () => false,
      );
    case "tab-cap":
      return knownFriction().then(({ friction }) => friction.tab_cap);
    case "tab-continue":
      if (tabId === undefined) return undefined;
      return browser.tabs
        .update(tabId, { url: continueTarget(request.url) })
        .catch(() => browser.tabs.update(tabId, { url: "about:blank" }))
        .then(() => true);
    default:
      return undefined;
  }
});

// Single-page apps (Reddit, YouTube) change pages without a load, which DNR never sees.
browser.webNavigation.onHistoryStateUpdated.addListener(async ({ tabId, frameId, url }) => {
  if (frameId !== 0) return;
  const action = actionOf(await verdictFor(url));
  const stopped = action === "ask" || action === "delay" || action === "block";
  // Grants cover Continue (Ask, Delay) and unlocks (Block).
  if (stopped && !(await isGrantedFor(tabId, url))) {
    await browser.tabs.update(tabId, { url: interstitialUrl(base, action, url, true) });
    return;
  }
  await tellTab(tabId, action === "hide_feed");
});

browser.webNavigation.onCommitted.addListener(({ tabId, frameId, url }) => {
  if (frameId === 0) void serially(() => trackDrift(tabId, url));
});

browser.webNavigation.onCreatedNavigationTarget.addListener(({ sourceTabId, tabId }) => {
  void serially(() => changeDrift((state) => inheritTrail(state, sourceTabId, tabId)));
});

browser.tabs.onRemoved.addListener((tabId) => {
  void serially(() => changeDrift((state) => forgetTab(state, tabId)));
});

browser.tabs.onCreated.addListener((tab) => void capTab(tab));

browser.alarms.onAlarm.addListener(({ name }) => {
  if (name.startsWith("grant:")) void endGrant(Number(name.slice("grant:".length)));
  else if (name === "tick") void tick();
});

// The interval runs while the worker is awake (the open native port keeps it awake); the alarm
// wakes it again if the browser suspended it.
browser.alarms.create("tick", { periodInMinutes: 0.5 });
setInterval(() => void tick(), HEARTBEAT_MS);
void browserStartedAt();
void tick();
