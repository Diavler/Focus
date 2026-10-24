// Bundles the extension into dist/<browser>/, ready for "Load unpacked" (Chromium) or
// about:debugging (Firefox). Only `./platform` and the manifest differ between browsers.
import { build } from "esbuild";
import { cpSync, mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";

/** Each browser, with the JavaScript its oldest supported version runs. */
const BROWSERS = { chromium: "chrome120", firefox: "firefox128" };

/** Points `./platform` at the browser's own module (src/platform/<browser>.ts). */
function platform(browser) {
  return {
    name: "platform",
    setup(bundle) {
      bundle.onResolve({ filter: /^\.\/platform$/ }, () => ({ path: resolve(`src/platform/${browser}.ts`) }));
    },
  };
}

for (const [browser, target] of Object.entries(BROWSERS)) {
  const out = `dist/${browser}`;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  await build({
    entryPoints: {
      background: "src/background.ts",
      interstitial: "src/interstitial.ts",
      feeds: "src/content/feeds.ts",
    },
    bundle: true,
    format: "iife",
    target,
    outdir: out,
    logLevel: "warning",
    plugins: [platform(browser)],
  });
  cpSync(`manifests/${browser}.json`, `${out}/manifest.json`);
  cpSync("static", out, { recursive: true });
  console.log(`built ${out}`);
}
