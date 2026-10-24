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
import {
  type Grant,
  delayKey,
  delayOver,
  grantRule,
  isGranted,
  makeGrant,
  makeUnlockGrant,
  staleGrants,
} from "./grants";
import { NativeClient, type PortLike } from "./native";
import {
  DEFAULT_FRICTION,
  HOST_NAME,
  actionOf,
  frictionFromStatus,
  isStopped,
  parkRefusal,
  sessionFromStatus,
  unlockRefusal,
  type Friction,
  type RunningSession,
  type Verdict,
} from "./protocol";
import { NEW_TAB_URL, browserName } from "./platform/chromium";
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
  await serially(() => dropStaleGrants(session));
  if (await syncRules(session)) await pushFeedVerdicts(session);
}

/** Ends grants from an earlier session, so an unlock or Continue never carries over. */
async function dropStaleGrants(session: RunningSession | null): Promise<void> {
  const { grants: stored } = (await browser.storage.session.get("grants")) as { grants?: Grant[] };
  const stale = staleGrants(stored ?? [], session?.started_at ?? null);
  if (stale.length === 0) return;
  const ids = stale.map((grant) => grant.id);
  await browser.declarativeNetRequest.updateSessionRules({ removeRuleIds: ids });
  await browser.storage.session.set({ grants: (stored ?? []).filter((grant) => !ids.includes(grant.id)) });
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
  if (session === null) return false;
  const decision = decide(session.spec.config, url);
  return isGranted(await grants(), tabId, url, decision, Date.now(), session.started_at);
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
  const decision = decide(session.spec.config, url);
  const grantUntil = (id: number, until: number) =>
    makeUnlockGrant(id, tabId, session.started_at, decision, url, until);
  // Checked before asking, so a Deep unlock is never spent on a page that can't be opened.
  if (!grantUntil(0, 0)) return "This page can't be unlocked here; try reloading it.";
  let response;
  try {
    response = await client.request({ type: "unlock", url, reason });
  } catch {
    return "Unlocking needs the Focus app running.";
  }
  if (response.type !== "unlocked") return unlockRefusal(response);
  const until = response.until;
  if (!(await addGrant((id) => grantUntil(id, until)))) return "Focus couldn't open this page.";
  await browser.tabs.update(tabId, { url });
  void tick();
  return null;
}

/**
 * Adds the grant `make` builds with a free rule id. Queued with every other change to grants, so
 * two at once never share an id or overwrite each other's stored list. False if `make` gives null.
 */
function addGrant(make: (id: number) => Grant | null): Promise<boolean> {
  return serially(async () => {
    const existing = await browser.declarativeNetRequest.getSessionRules();
    const grant = make(Math.max(0, ...existing.map((rule) => rule.id)) + 1);
    if (!grant) return false;
    await browser.declarativeNetRequest.updateSessionRules({ addRules: [grantRule(grant)] as never });
    await browser.storage.session.set({ grants: [...(await grants()), grant] });
    browser.alarms.create(`grant:${grant.id}`, { when: grant.expires });
    return true;
  });
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
  if (!(await addGrant((id) => makeGrant(id, tabId, session.started_at, decision, now)))) return false;
  await browser.tabs.update(tabId, { url });
  return true;
}

/**
 * Later: parks `url` (with `title`, if known) in Focus's parking lot, which sends it to the
 * Todoist inbox when it can, then closes the tab. Returns why it failed, or null.
 */
async function later(tabId: number, url: string, title: string): Promise<string | null> {
  if (siteOf(url) === null) return "Only web pages can be saved for later.";
  let response;
  try {
    response = await client.request({ type: "park", text: title, url });
  } catch {
    return "Saving for later needs the Focus app running.";
  }
  const refusal = parkRefusal(response);
  if (refusal === null) await browser.tabs.remove(tabId).catch(() => undefined);
  return refusal;
}

/** Shows on the toolbar button why Later failed, for a few seconds. */
async function flagLaterFailure(tabId: number, why: string): Promise<void> {
  await browser.action.setBadgeText({ tabId, text: "!" });
  await browser.action.setTitle({ tabId, title: why });
  setTimeout(() => {
    void browser.action.setBadgeText({ tabId, text: "" }).catch(() => undefined);
    void browser.action.setTitle({ tabId, title: LATER_TITLE }).catch(() => undefined);
  }, 6000);
}

/** The toolbar button's tooltip. */
const LATER_TITLE = "Later: save this page to Focus's parking lot and close it";

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

function endGrant(id: number): Promise<void> {
  return serially(async () => {
    await browser.declarativeNetRequest.updateSessionRules({ removeRuleIds: [id] });
    await browser.storage.session.set({ grants: (await grants()).filter((grant) => grant.id !== id) });
  });
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
  | { kind: "tab-continue"; url: string }
  | { kind: "later"; url: string };

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
    case "later":
      return tabId === undefined
        ? Promise.resolve("No tab to close.")
        : later(tabId, request.url, "").catch(() => "Focus couldn't save this for later.");
    case "tab-cap":
      return knownFriction().then(({ friction }) => friction.tab_cap);
    case "tab-continue":
      if (tabId === undefined) return undefined;
      return browser.tabs
        .update(tabId, { url: continueTarget(request.url, NEW_TAB_URL) })
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

browser.action.onClicked.addListener(async (tab) => {
  if (tab.id === undefined) return;
  const why = await later(tab.id, tab.url ?? "", tab.title ?? "").catch(() => "Focus couldn't save this for later.");
  if (why !== null) await flagLaterFailure(tab.id, why);
});

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
