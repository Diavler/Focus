// The browser-specific module. The build points `./platform` at `chromium.ts` or `firefox.ts`
// (see build.mjs); this file stands in for the typechecker and the tests.

export { NEW_TAB_URL, browserName } from "./chromium";
