// Off-thread ESM loader (the `module.register()` API, node >=20.6) for the
// Playwright-shim drop-in. node >=22 uses the synchronous in-thread
// `registerHooks` instead (see register.mjs); this file backs the older path so
// the drop-in works on node 20 too. The redirect itself is identical — bare
// `@playwright/test` etc. rewrite to the turbo-surf shim.
export { resolveHook as resolve, loadHook as load } from "./loader-hooks.mjs";
