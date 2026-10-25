import { describe, expect, it } from "vitest";

import {
  HOME_WINDOW_MS,
  acknowledge,
  allowedSites,
  forgetTab,
  freshDrift,
  inheritTrail,
  recordVisit,
} from "../src/drift";
import type { BlockConfig } from "../src/protocol";
import { siteOf, within } from "../src/sites";
import { STARTUP_GRACE_MS, continueTarget, overCap, restoredAtStartup } from "../src/tabcap";

const START = 1_000_000;
const LATER = START + HOME_WINDOW_MS + 1;
const TAB = 3;

describe("sites", () => {
  it("are hosts without www., only for web pages", () => {
    expect(siteOf("https://www.Docs.rs/serde")).toBe("docs.rs");
    expect(siteOf("http://news.ycombinator.com/item?id=1")).toBe("news.ycombinator.com");
    expect(siteOf("chrome://newtab/")).toBeNull();
    expect(siteOf("not a url")).toBeNull();
    expect(within("blog.rust-lang.org", "rust-lang.org")).toBe(true);
    expect(within("notrust-lang.org", "rust-lang.org")).toBe(false);
  });
});

describe("drift", () => {
  it("prompts after enough hops away from the first minutes' sites", () => {
    const state = freshDrift(START);
    expect(recordVisit(state, TAB, "https://github.com/me/repo", START + 1000, 3, [])).toBeNull();
    expect(state.home).toEqual(["github.com"]);
    expect(recordVisit(state, TAB, "https://en.wikipedia.org/wiki/Rust", LATER, 3, [])).toBeNull();
    expect(recordVisit(state, TAB, "https://en.wikipedia.org/wiki/Iron", LATER, 3, [])).toBeNull();
    expect(recordVisit(state, TAB, "https://youtube.com/watch?v=1", LATER, 3, [])).toBeNull();
    expect(recordVisit(state, TAB, "https://reddit.com/r/all", LATER, 3, [])).toEqual([
      "en.wikipedia.org",
      "youtube.com",
      "reddit.com",
    ]);
    expect(state.tabs[TAB]?.back).toBe("https://github.com/me/repo");
  });

  it("clears the trail when the tab comes home, and counts allowed sites as home", () => {
    const state = freshDrift(START);
    const allowed = ["docs.rs"];
    recordVisit(state, TAB, "https://a.example/", LATER, 2, allowed);
    expect(recordVisit(state, TAB, "https://docs.rs/tokio", LATER, 2, allowed)).toBeNull();
    expect(state.tabs[TAB]?.trail).toEqual([]);
    expect(recordVisit(state, TAB, "https://b.example/", LATER, 2, allowed)).toBeNull();
    expect(recordVisit(state, TAB, "https://c.example/", LATER, 2, allowed)).toEqual(["b.example", "c.example"]);
  });

  it("doesn't count going back to a site already on the trail", () => {
    const state = freshDrift(START);
    for (const url of ["https://so.example/", "https://docs.example/", "https://so.example/q", "https://docs.example/b"]) {
      expect(recordVisit(state, TAB, url, LATER, 3, [])).toBeNull();
    }
    expect(state.tabs[TAB]?.trail).toEqual(["so.example", "docs.example"]);
  });

  it("keeps going after an acknowledgement, and 0 hops never prompts", () => {
    const state = freshDrift(START);
    recordVisit(state, TAB, "https://a.example/", LATER, 2, []);
    recordVisit(state, TAB, "https://b.example/", LATER, 2, []);
    acknowledge(state, TAB);
    expect(recordVisit(state, TAB, "https://c.example/", LATER, 2, [])).toBeNull();
    const off = freshDrift(START);
    for (const site of ["a", "b", "c", "d", "e"]) {
      expect(recordVisit(off, TAB, `https://${site}.example/`, LATER, 0, [])).toBeNull();
    }
  });

  it("passes a trail to tabs opened from it and forgets closed tabs", () => {
    const state = freshDrift(START);
    recordVisit(state, TAB, "https://a.example/", LATER, 2, []);
    inheritTrail(state, TAB, 9);
    expect(recordVisit(state, 9, "https://b.example/", LATER, 2, [])).toEqual(["a.example", "b.example"]);
    forgetTab(state, TAB);
    expect(state.tabs[TAB]).toBeUndefined();
  });

  it("takes the configuration's allowed hosts as home", () => {
    const config: BlockConfig = {
      name: "coding",
      mode: "blocklist",
      rules: [
        { pattern: "reddit.com/r/rust/comments/*", verdict: "allow" },
        { pattern: "www.docs.rs", verdict: "allow" },
        { pattern: "x.com", verdict: { restrict: "block" } },
      ],
      apps: [],
    };
    expect(allowedSites(config)).toEqual(["reddit.com", "docs.rs"]);
  });
});

describe("tab cap", () => {
  it("applies over the cap, and not when off", () => {
    expect(overCap(11, 10)).toBe(true);
    expect(overCap(10, 10)).toBe(false);
    expect(overCap(30, 0)).toBe(false);
  });

  it("leaves tabs restored at browser start alone", () => {
    expect(restoredAtStartup(START, START + STARTUP_GRACE_MS - 1)).toBe(true);
    expect(restoredAtStartup(START, START + STARTUP_GRACE_MS)).toBe(false);
  });

  it("continues to the page, or a new tab page", () => {
    expect(continueTarget("https://docs.rs/", "about:newtab")).toBe("https://docs.rs/");
    expect(continueTarget("", "about:newtab")).toBe("about:newtab");
    expect(continueTarget("chrome://newtab/", "about:newtab")).toBe("about:newtab");
  });
});
