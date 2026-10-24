import { describe, expect, it } from "vitest";

import { decide } from "../src/dnr";
import { GRANT_MS, delayKey, delayOver, grantRule, isGranted, makeGrant } from "../src/grants";
import type { BlockConfig } from "../src/protocol";

/** YouTube is Ask, Shorts are Block, Reddit is Delay. */
const CONFIG: BlockConfig = {
  name: "coding",
  mode: "blocklist",
  rules: [
    { pattern: "youtube.com", verdict: { restrict: "ask" } },
    { pattern: "youtube.com/shorts/*", verdict: { restrict: "block" } },
    { pattern: "reddit.com", verdict: { restrict: "delay" } },
  ],
  apps: [],
};
const NOW = 1_000_000;
const TAB = 7;

function grantFor(url: string) {
  const grant = makeGrant(1, TAB, decide(CONFIG, url), NOW);
  if (!grant) throw new Error(`no grant for ${url}`);
  return grant;
}

function allowed(grantUrl: string, url: string, tab = TAB, now = NOW + 1) {
  return isGranted([grantFor(grantUrl)], tab, url, decide(CONFIG, url), now);
}

describe("grants", () => {
  it("lets the asked-about site through in that tab", () => {
    expect(allowed("https://www.youtube.com/watch?v=1", "https://www.youtube.com/watch?v=2")).toBe(true);
  });

  it("never unblocks a stricter, more specific rule on the same site", () => {
    expect(allowed("https://www.youtube.com/watch?v=1", "https://www.youtube.com/shorts/abc")).toBe(false);
    const grant = grantFor("https://www.youtube.com/watch?v=1");
    const shorts = decide(CONFIG, "https://www.youtube.com/shorts/abc");
    expect(grantRule(grant).priority).toBeLessThan(shorts.priority);
  });

  it("stays in its tab and expires", () => {
    expect(allowed("https://www.youtube.com/", "https://www.youtube.com/", TAB + 1)).toBe(false);
    expect(allowed("https://www.youtube.com/", "https://www.youtube.com/", TAB, NOW + GRANT_MS)).toBe(false);
  });

  it("can't be made for Block, Allow or unmatched pages", () => {
    for (const url of ["https://www.youtube.com/shorts/x", "https://example.org/"]) {
      expect(makeGrant(1, TAB, decide(CONFIG, url), NOW), url).toBeNull();
    }
  });

  it("becomes a tab-scoped allow rule at the deciding rule's priority", () => {
    const grant = grantFor("https://reddit.com/r/all");
    const rule = grantRule(grant);
    expect(rule.action).toEqual({ type: "allow" });
    expect(rule.condition.tabIds).toEqual([TAB]);
    expect(rule.priority).toBe(decide(CONFIG, "https://reddit.com/r/all").priority);
  });
});

describe("delay waits", () => {
  it("are only over once started and elapsed", () => {
    expect(delayOver(undefined, NOW)).toBe(false);
    expect(delayOver(NOW + 1, NOW)).toBe(false);
    expect(delayOver(NOW, NOW)).toBe(true);
    expect(delayKey(3, "^x$")).toBe("delay:3:^x$");
  });
});
