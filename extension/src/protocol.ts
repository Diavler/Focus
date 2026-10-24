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

/** Requests the extension may send (focus-nmh refuses the rest). */
export type Request =
  | { type: "status" }
  | { type: "heartbeat"; browser: string; incognito_allowed: boolean }
  | { type: "rules_for"; url: string; config: string | null };

/** The daemon's answers. */
export type Response =
  | { type: "status"; now: number; session: SessionState }
  | { type: "verdict"; verdict: Verdict; config: string | null }
  | { type: "done" }
  | { type: "error"; error: { kind: string; message?: string } };

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
