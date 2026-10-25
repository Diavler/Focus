// Messages exchanged with the daemon through focus-nmh. These mirror the serde shapes in
// crates/ipc-proto (ADR 0004) plus the `id` envelope field that focus-nmh adds (ADR 0005).

/** The protocol version this extension speaks; must match ipc-proto's PROTOCOL_VERSION. */
export const PROTOCOL_VERSION = 1;

/** The native messaging host's registered name. */
export const HOST_NAME = "dev.focus.nmh";

/** Rule actions, from least to most strict. */
export type RuleAction = "ask" | "delay" | "hide_feed" | "block";

/** What happens to a page. */
export type Verdict = "allow" | { restrict: RuleAction };

/** One rule of a block configuration. */
export interface Rule {
  pattern: string;
  verdict: Verdict;
}

/** A block configuration as the daemon sends it. */
export interface BlockConfig {
  name: string;
  mode: "blocklist" | "allowlist";
  rules: Rule[];
  apps: string[];
}

/** A running session; times are milliseconds since the Unix epoch. */
export interface RunningSession {
  spec: { config: BlockConfig; strictness: "light" | "deep" | "locked" };
  started_at: number;
  ends_at: number;
}

/** The daemon's session state. */
export type SessionState = "idle" | { running: RunningSession } | { ended: unknown };

/** The friction limits in effect (crates/focus-core/src/friction.rs). 0 turns a cap off. */
export interface Friction {
  unlocks_per_day: number;
  unlock_minutes: number;
  tab_cap: number;
  drift_hops: number;
}

/** The defaults, used until the daemon has answered once. */
export const DEFAULT_FRICTION: Friction = { unlocks_per_day: 3, unlock_minutes: 10, tab_cap: 10, drift_hops: 4 };

/** Requests the extension may send (focus-nmh refuses the rest). */
export type Request =
  | { type: "status" }
  | { type: "heartbeat"; browser: string; incognito_allowed: boolean }
  | { type: "rules_for"; url: string; config: string | null }
  | { type: "unlock"; url: string; reason: string }
  | { type: "delay_visit"; url: string }
  | { type: "park"; text: string; url: string | null };

/** The daemon's answers. */
export type Response =
  | { type: "status"; now: number; session: SessionState; friction?: Friction; unlocks_left?: number }
  | { type: "verdict"; verdict: Verdict; config: string | null }
  | { type: "unlocked"; until: number }
  | { type: "delay"; seconds: number }
  | { type: "done" }
  | { type: "error"; error: { kind: string; message?: string; error?: unknown } };

/** The envelope sent to focus-nmh. */
export interface Outgoing {
  id: number;
  version: number;
  message: Request;
}

/** The envelope focus-nmh sends back. */
export interface Incoming {
  id: number;
  version: number;
  message: Response;
}

/** The running session in a state, if there is one. */
export function runningSession(state: SessionState): RunningSession | null {
  return typeof state === "object" && "running" in state ? state.running : null;
}

/**
 * The session a status answer reports: the running one, null for none, or undefined when the
 * daemon couldn't say (keep using the last one known, so killing the daemon doesn't lift blocks).
 * A "stopped" error means Focus was stopped with its kill switch, which ends the session.
 */
export function sessionFromStatus(response: Response): RunningSession | null | undefined {
  if (response.type === "status") return runningSession(response.session);
  if (isStopped(response)) return null;
  return undefined;
}

/** The friction limits and unlocks left in a status answer; undefined for any other answer. */
export function frictionFromStatus(response: Response): { friction: Friction; unlocksLeft: number } | undefined {
  if (response.type !== "status") return undefined;
  const friction = response.friction ?? DEFAULT_FRICTION;
  return { friction, unlocksLeft: response.unlocks_left ?? friction.unlocks_per_day };
}

/** Why an unlock was refused, in words for the block page. */
export function unlockRefusal(response: Response): string {
  if (response.type !== "error") return "Focus couldn't unlock this page.";
  if (response.error.kind === "unreachable") return "Unlocking needs the Focus app running.";
  const reason = response.error.error;
  if (reason === "forbidden") return "Locked sessions can't be unlocked.";
  if (reason === "reason_required") return "Type a reason first.";
  if (reason === "not_blocked") return "This page isn't blocked any more; try reloading.";
  if (reason === "no_session") return "No session is running.";
  if (typeof reason === "object" && reason !== null && "budget_spent" in reason) {
    const nextAt = (reason as { budget_spent: { next_at: number | null } }).budget_spent.next_at;
    return nextAt === null
      ? "No unlocks left: your budget is 0."
      : `No unlocks left. The next one comes back at ${new Date(nextAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}.`;
  }
  return response.error.message ?? "Focus couldn't unlock this page.";
}

/** Why parking failed, in words for the page or the toolbar button; null when it worked. */
export function parkRefusal(response: Response): string | null {
  if (response.type === "done") return null;
  if (response.type === "error" && response.error.kind === "unreachable") return "Saving for later needs the Focus app running.";
  if (response.type === "error" && response.error.message) return response.error.message;
  if (response.type === "error" && response.error.kind === "park") {
    const reason = response.error.error;
    if (reason === "empty") return "There's nothing to save.";
    if (typeof reason === "object" && reason !== null && "full" in reason) return "Focus's parking lot is full; clear some in the Focus window.";
    if (typeof reason === "object" && reason !== null && "too_long" in reason) return "That link is too long to save.";
  }
  return "Focus couldn't save this for later.";
}

/** Whether the answer says Focus was stopped with its kill switch (no session, no daemon). */
export function isStopped(response: Response): boolean {
  return response.type === "error" && response.error.kind === "stopped";
}

/** The restricting action of a verdict, or null for Allow. */
export function actionOf(verdict: Verdict): RuleAction | null {
  return verdict === "allow" ? null : verdict.restrict;
}

/** Whether `value` looks like an answer from focus-nmh. */
export function isIncoming(value: unknown): value is Incoming {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.id === "number" &&
    typeof candidate.message === "object" &&
    candidate.message !== null
  );
}
