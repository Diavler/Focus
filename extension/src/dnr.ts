// Turns a block configuration into declarativeNetRequest rules for top-level page loads.
//
// The daemon decides with "most specific pattern wins, stricter verdict breaks a tie"
// (crates/focus-core/src/matcher.rs). DNR picks the matching rule with the highest priority, so
// every rule gets a priority that sorts the same way, and every verdict gets a rule: Allow and
// Hide-feed become DNR `allow` (the page loads; the content script hides feeds), Ask, Delay and
// Block redirect to the interstitial. Pattern parsing mirrors crates/focus-core/src/pattern.rs.

import type { BlockConfig, RuleAction, Verdict } from "./protocol";

/** Chrome's limit on regex rules per extension. */
export const MAX_REGEX_RULES = 1000;

/** Above every config rule, for temporary "continue" grants. */
export const GRANT_PRIORITY = 2_000_000_000;

/** A DNR rule as passed to updateDynamicRules. */
export interface DnrRule {
  id: number;
  priority: number;
  action: { type: "allow" } | { type: "redirect"; redirect: { regexSubstitution: string } };
  condition: {
    regexFilter: string;
    isUrlFilterCaseSensitive: false;
    resourceTypes: ["main_frame"];
  };
}

/** A pattern split like the core's UrlPattern: lower-cased host and normalised path glob. */
export interface PatternParts {
  host: string;
  path: string | null;
}

/** Parses `host` or `host/path-glob` the way the core does; null for what the core rejects. */
export function parsePattern(input: string): PatternParts | null {
  const source = input.trim();
  if (source === "" || /[?#]/.test(source)) return null;
  const rest = stripScheme(source);
  const slash = rest.indexOf("/");
  const rawHost = slash === -1 ? rest : rest.slice(0, slash);
  const host = rawHost.replace(/\.$/, "").toLowerCase();
  const validHost = host.split(".").every((label) => /^[\p{L}\p{N}-]+$/u.test(label));
  if (host === "" || !validHost) return null;
  if (slash === -1) return { host, path: null };
  const path = rest.slice(slash).replace(/\*+/g, "*");
  return { host, path: normalizePath(path) };
}

function stripScheme(input: string): string {
  const at = input.indexOf("://");
  return at !== -1 && !/[/?#]/.test(input.slice(0, at)) ? input.slice(at + 3) : input;
}

function normalizePath(path: string): string {
  if (path === "" || path === "/") return "/";
  return (path.endsWith("/") ? path.slice(0, -1) : path).toLowerCase();
}

/** How strict each verdict is; breaks ties between equally specific rules. */
const STRICTNESS: Record<RuleAction | "allow", number> = {
  allow: 0,
  ask: 1,
  delay: 2,
  hide_feed: 3,
  block: 4,
};

/**
 * A DNR priority that sorts like the core's (host labels, literal path bytes, strictness).
 * Starts at 2 so the allowlist catch-all (priority 1) is always below.
 */
export function priority(parts: PatternParts, verdict: Verdict): number {
  const labels = parts.host.split(".").length;
  const literal = parts.path === null ? 0 : new TextEncoder().encode(parts.path.replaceAll("*", "")).length;
  const rank = STRICTNESS[verdict === "allow" ? "allow" : verdict.restrict];
  return 2 + labels * 1_000_000 + Math.min(literal, 99_999) * 10 + rank;
}

function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
}

/**
 * A regex matching every URL the pattern matches, applied case-insensitively.
 *
 * The core compares the glob against the URL path with one trailing `/` removed, so the regex
 * matches "normalised path, then an optional `/`". A trailing `*` must then not end in `/`
 * itself, and after a `/` it must match at least one character (`/a/*` doesn't match `/a/`).
 */
export function patternRegex(parts: PatternParts): string {
  const prefix = `^[a-z][a-z0-9+.-]*://(?:[^/?#@]*@)?(?:[^/?#@:]*\\.)?${escapeRegex(parts.host)}\\.?(?::[0-9]*)?`;
  const end = "(?:[?#].*)?$";
  if (parts.path === null) return `${prefix}(?:[/?#].*)?$`;
  if (parts.path === "/") return `${prefix}/?${end}`;
  const pieces = parts.path.split("*");
  let body = "";
  pieces.forEach((piece, index) => {
    body += escapeRegex(piece);
    const isLast = index === pieces.length - 1;
    if (isLast) return;
    const trailing = index === pieces.length - 2 && pieces[pieces.length - 1] === "";
    if (!trailing) body += "[^?#]*";
    else if (piece.endsWith("/")) body += "[^?#]*[^?#/]";
    else body += "(?:[^?#]*[^?#/])?";
  });
  return `${prefix}${body}/?${end}`;
}

/** Where Ask, Delay and Block send the page: `<base>interstitial.html?a=<action>#<url>`. */
export function interstitialUrl(base: string, action: RuleAction, url: string): string {
  return `${base}interstitial.html?a=${action}#${url}`;
}

/** The DNR rules for a session running `config`; `base` is the extension's root URL. */
export function toDnrRules(config: BlockConfig, base: string): DnrRule[] {
  const rules: DnrRule[] = [];
  for (const rule of config.rules) {
    const parts = parsePattern(rule.pattern);
    if (!parts || rules.length >= MAX_REGEX_RULES - 1) continue;
    rules.push(dnrRule(rules.length + 1, priority(parts, rule.verdict), patternRegex(parts), rule.verdict, base));
  }
  if (config.mode === "allowlist") {
    rules.push(dnrRule(rules.length + 1, 1, "^https?://.*$", { restrict: "block" }, base));
  }
  return rules;
}

function dnrRule(id: number, priority: number, regexFilter: string, verdict: Verdict, base: string): DnrRule {
  const condition = {
    regexFilter,
    isUrlFilterCaseSensitive: false as const,
    resourceTypes: ["main_frame"] as ["main_frame"],
  };
  const action = verdict === "allow" ? null : verdict.restrict;
  if (action === null || action === "hide_feed") {
    return { id, priority, action: { type: "allow" }, condition };
  }
  return {
    id,
    priority,
    action: { type: "redirect", redirect: { regexSubstitution: interstitialUrl(base, action, "\\0") } },
    condition,
  };
}
