// "Sites" as Focus counts them for unlocks, Delay visits and drift: a web page's host without a
// leading `www.` (crates/focus-core/src/delay.rs counts Delay visits the same way).

/** The site of a web page, or null for anything that isn't http(s) (new tab, extension pages). */
export function siteOf(url: string): string | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
  const host = parsed.hostname.toLowerCase().replace(/\.$/, "");
  return host.startsWith("www.") ? host.slice(4) : host;
}

/** Whether `site` is `home` or one of its subdomains. */
export function within(site: string, home: string): boolean {
  return site === home || site.endsWith(`.${home}`);
}
