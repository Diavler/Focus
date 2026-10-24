import { describe, expect, it } from "vitest";

import { type DnrRule, decide as decideLocally, parsePattern, patternRegex, priority, toDnrRules } from "../src/dnr";
import type { BlockConfig, Rule, Verdict } from "../src/protocol";

const BASE = "chrome-extension://hjeodjhhiiaojdakfafaebijlmdckebo/";
const HIDDEN: Verdict = { restrict: "hide_feed" };
const BLOCKED: Verdict = { restrict: "block" };

function config(rules: [string, Verdict][], mode: BlockConfig["mode"] = "blocklist"): BlockConfig {
  return { name: "test", mode, rules: rules.map(([pattern, verdict]): Rule => ({ pattern, verdict })), apps: [] };
}

/** The `coding` configuration from docs/DESIGN.md, as in crates/focus-core/src/matcher.rs. */
const CODING = config([
  ["x.com", BLOCKED],
  ["instagram.com", BLOCKED],
  ["tiktok.com", BLOCKED],
  ["reddit.com", HIDDEN],
  ["youtube.com", HIDDEN],
  ["reddit.com/r/rust/comments/*", "allow"],
  ["docs.rs", "allow"],
  ["github.com", "allow"],
]);

/**
 * What Chrome would do: the highest-priority rule whose regex matches (case-insensitively) wins.
 * Returns that rule's original verdict, so results compare directly with the core's.
 */
function decide(cfg: BlockConfig, url: string): Verdict | "no match" {
  const rules = toDnrRules(cfg, BASE);
  const matching = rules.filter((rule) => new RegExp(rule.condition.regexFilter, "i").test(url));
  if (matching.length === 0) return "no match";
  const winner = matching.reduce((best, rule) => (rule.priority > best.priority ? rule : best));
  if (winner.priority === 1) return BLOCKED;
  return cfg.rules[winner.id - 1]!.verdict;
}

describe("toDnrRules reproduces the core's verdicts", () => {
  it.each<[string, Verdict | "no match"]>([
    ["https://www.reddit.com/", HIDDEN],
    ["https://www.reddit.com/?feed=home", HIDDEN],
    ["https://www.reddit.com/r/rust/comments/1abc/borrowck_question/", "allow"],
    ["https://old.reddit.com/R/Rust/Comments/1abc", "allow"],
    ["https://www.reddit.com/r/rust/comments/", HIDDEN],
    ["https://www.reddit.com/r/rustjerk/comments/1abc", HIDDEN],
    ["https://x.com/home", BLOCKED],
    ["https://X.COM.:443/home#top", BLOCKED],
    ["https://docs.rs/iced", "allow"],
    ["https://notreddit.com/", "no match"],
    ["https://example.org", "no match"],
  ])("%s", (url, expected) => {
    expect(decide(CODING, url)).toEqual(expected);
  });

  it("allows only what an allowlist lists", () => {
    const writing = config([["docs.google.com", "allow"]], "allowlist");
    expect(decide(writing, "https://docs.google.com/d/1")).toEqual("allow");
    expect(decide(writing, "https://news.ycombinator.com")).toEqual(BLOCKED);
  });

  it("lets the stricter rule win a tie", () => {
    const tie = config([
      ["a.com/x/*", "allow"],
      ["a.com/x/*", BLOCKED],
    ]);
    expect(decide(tie, "https://a.com/x/1")).toEqual(BLOCKED);
  });

  it("matches the core on random Rust post URLs", () => {
    for (let i = 0; i < 200; i++) {
      const id = Math.random().toString(36).slice(2, 8);
      const slug = Math.random().toString(36).slice(2);
      const slash = i % 2 === 0 ? "/" : "";
      const query = i % 3 === 0 ? "?sort=new" : "";
      expect(decide(CODING, `https://www.reddit.com/r/rust/comments/${id}/${slug}${slash}${query}`)).toEqual("allow");
    }
  });
});

describe("rule shapes", () => {
  const rules = toDnrRules(CODING, BASE);

  it("redirects Block, Ask and Delay to the interstitial with the original URL", () => {
    const block = rules[0] as DnrRule;
    expect(block.action).toEqual({
      type: "redirect",
      redirect: { regexSubstitution: `${BASE}interstitial.html?a=block#\\0` },
    });
    expect(block.condition.resourceTypes).toEqual(["main_frame"]);
    const ask = toDnrRules(config([["a.com", { restrict: "ask" }]]), BASE)[0] as DnrRule;
    expect(JSON.stringify(ask.action)).toContain("a=ask#");
  });

  it("lets Allow and Hide feed pages load", () => {
    expect(rules[3]?.action).toEqual({ type: "allow" });
    expect(rules[5]?.action).toEqual({ type: "allow" });
  });

  it("keeps every rule above the allowlist catch-all", () => {
    for (const rule of rules) expect(rule.priority).toBeGreaterThan(1);
  });

  it("decides locally exactly as the DNR rules do", () => {
    const writing = config([["docs.google.com", "allow"]], "allowlist");
    for (const [cfg, url] of [
      [CODING, "https://www.reddit.com/"],
      [CODING, "https://www.reddit.com/r/rust/comments/a/b"],
      [CODING, "https://x.com/home"],
      [CODING, "https://example.org/"],
      [writing, "https://docs.google.com/d"],
      [writing, "https://news.ycombinator.com"],
    ] as const) {
      const expected = decide(cfg, url);
      expect(decideLocally(cfg, url).verdict, url).toEqual(expected === "no match" ? "allow" : expected);
    }
  });
});

describe("parsePattern and priority", () => {
  it("rejects what the core rejects", () => {
    for (const bad of ["", "  ", "*.x.com", "localhost:3000", "a..b/x", "google.com/search?q=x"]) {
      expect(parsePattern(bad), bad).toBeNull();
    }
  });

  it("orders like the core's specificity", () => {
    const of = (pattern: string) => priority(parsePattern(pattern)!, "allow");
    expect(of("old.reddit.com")).toBeGreaterThan(of("reddit.com/r/rust/*"));
    expect(of("reddit.com/r/rust/*")).toBeGreaterThan(of("reddit.com/"));
    expect(of("reddit.com/")).toBeGreaterThan(of("reddit.com"));
  });

  it("builds anchored regexes", () => {
    const regex = patternRegex(parsePattern("HTTPS://YouTube.com/Shorts/*")!);
    expect(regex.startsWith("^")).toBe(true);
    expect(regex.endsWith("$")).toBe(true);
    expect(new RegExp(regex, "i").test("https://m.youtube.com/SHORTS/x")).toBe(true);
  });
});
