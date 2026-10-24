import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { FEED_MATCHES, hideFeedCss } from "../src/feeds";
import { BASE_DELAY, MAX_DELAY, delaySeconds } from "../src/friction";
import { sessionFromStatus, type RunningSession } from "../src/protocol";

describe("delaySeconds", () => {
  it("grows with each visit up to the cap", () => {
    expect([0, 1, 2].map(delaySeconds)).toEqual([BASE_DELAY, 2 * BASE_DELAY, 3 * BASE_DELAY]);
    expect(delaySeconds(1000)).toBe(MAX_DELAY);
    expect(delaySeconds(-5)).toBe(BASE_DELAY);
  });
});

describe("hideFeedCss", () => {
  it("covers Reddit and YouTube, including subdomains, and nothing else", () => {
    expect(hideFeedCss("www.reddit.com")).toContain("shreddit-feed");
    expect(hideFeedCss("old.reddit.com")).toContain("body.listing-page #siteTable");
    expect(hideFeedCss("m.youtube.com")).toContain("#related");
    expect(hideFeedCss("notreddit.com")).toBeNull();
    expect(hideFeedCss("docs.rs")).toBeNull();
  });
});

describe("chromium manifest", () => {
  const manifest = JSON.parse(readFileSync("manifests/chromium.json", "utf8"));

  it("has the key that fixes the extension ID focus-nmh registers by default", () => {
    // Chrome's ID: SHA-256 of the public key, first 32 hex digits mapped 0-f to a-p.
    const hex = createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest("hex").slice(0, 32);
    const id = [...hex].map((digit) => String.fromCharCode(97 + parseInt(digit, 16))).join("");
    const register = readFileSync("../apps/focus-nmh/src/register.rs", "utf8");
    expect(register).toContain(`DEFAULT_EXTENSION_ID: &str = "${id}"`);
  });

  it("lets pages be redirected to the interstitial", () => {
    expect(manifest.permissions).toEqual(
      expect.arrayContaining(["nativeMessaging", "declarativeNetRequestWithHostAccess", "webNavigation"]),
    );
    expect(manifest.web_accessible_resources[0].resources).toContain("interstitial.html");
  });

  it("runs the feed script exactly where the background expects it", () => {
    expect(manifest.content_scripts[0].matches).toEqual(FEED_MATCHES);
  });
});

describe("sessionFromStatus", () => {
  const running: RunningSession = {
    spec: { config: { name: "coding", mode: "blocklist", rules: [], apps: [] }, strictness: "locked" },
    started_at: 0,
    ends_at: 1000,
  };

  it("reads the running session, or none", () => {
    expect(sessionFromStatus({ type: "status", now: 1, session: { running } })).toEqual(running);
    expect(sessionFromStatus({ type: "status", now: 1, session: "idle" })).toBeNull();
  });

  it("drops the session when Focus was stopped, but keeps it when the daemon is just gone", () => {
    expect(sessionFromStatus({ type: "error", error: { kind: "stopped" } })).toBeNull();
    expect(sessionFromStatus({ type: "error", error: { kind: "unreachable" } })).toBeUndefined();
  });
});
