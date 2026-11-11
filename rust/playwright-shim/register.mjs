// Opt-in drop-in installer: run a Playwright suite on the napi-backed Rust engine
// with NO @playwright/test or Chromium —
//
//   node --import ./register.mjs --test 'e2e/**/*.spec.mjs'
//
// Every `import … from "@playwright/test"` then resolves to the turbo-surf shim.
// Toggle off without dropping the flag: TURBO_PLAYWRIGHT_SHIM=0.

import * as nodeModule from "node:module";
import { loadHook, resolveHook } from "./loader-hooks.mjs";

if (process.env.TURBO_PLAYWRIGHT_SHIM !== "0") {
  // `registerHooks` (synchronous, in-thread) lands in node 22; on node 20 fall
  // back to the off-thread `module.register()` loader (node >=20.6) so the
  // drop-in resolves `@playwright/test` → the shim on both. Same redirect either way.
  if (typeof nodeModule.registerHooks === "function") {
    nodeModule.registerHooks({ resolve: resolveHook, load: loadHook });
  } else {
    nodeModule.register("./loader.mjs", import.meta.url);
  }
}
