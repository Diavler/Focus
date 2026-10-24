// What "hide feed" hides on each site: feeds and recommendations, never the page you came for.
// Sites change their markup, so these selectors are best effort and checked by hand.

/** The pages the feed content script runs on; must match the manifests' content_scripts. */
export const FEED_MATCHES = ["*://*.reddit.com/*", "*://*.youtube.com/*"];

const SITES: { domain: string; selectors: string[] }[] = [
  {
    domain: "reddit.com",
    selectors: [
      // New Reddit: front page, subreddit and popular listings, sidebars, "related" posts.
      "shreddit-feed",
      "#right-sidebar-container",
      "reddit-sidebar-nav",
      "shreddit-gallery-carousel",
      "[slot='related-posts']",
      "pdp-right-rail",
      // Old Reddit: listings only (comment pages keep the post).
      "body.listing-page #siteTable",
      "body.listing-page .side",
    ],
  },
  {
    domain: "youtube.com",
    selectors: [
      // Home feed, Shorts shelves, the recommendations beside and after a video.
      "ytd-browse[page-subtype='home']",
      "ytd-reel-shelf-renderer",
      "ytd-rich-shelf-renderer[is-shorts]",
      "#related",
      ".ytp-endscreen-content",
      "ytd-watch-next-secondary-results-renderer",
    ],
  },
];

/** CSS that hides the feeds on `hostname`, or null for a site without known feeds. */
export function hideFeedCss(hostname: string): string | null {
  const host = hostname.toLowerCase();
  const site = SITES.find(({ domain }) => host === domain || host.endsWith(`.${domain}`));
  return site ? `${site.selectors.join(",\n")} { display: none !important; }` : null;
}
