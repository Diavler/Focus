// Bundles the extension into dist/<browser>/, ready for "Load unpacked".
import { build } from "esbuild";
import { cpSync, mkdirSync, rmSync } from "node:fs";

const browser = "chromium";
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
  target: "chrome120",
  outdir: out,
  logLevel: "warning",
});
cpSync(`manifests/${browser}.json`, `${out}/manifest.json`);
cpSync("static", out, { recursive: true });
console.log(`built ${out}`);
