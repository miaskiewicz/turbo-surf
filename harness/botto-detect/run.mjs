// botto-vs-turbo-surf detection harness.
//
// Runs botto's REAL in-page signal collector (harness/tools/browser-probe.inject.js +
// the Botguard integrity VM + feature probe) inside turbo-surf's live V8+rtdom render
// isolate — the same isolate the engine hydrates pages in — then feeds the collected
// facts to botto's own `liveFacts()` + `runChecks()`. botto is the oracle: it tells us
// exactly which STRONG/WEAK anti-bot tells turbo-surf's synthetic browser trips.
//
// This is the measurement loop behind the surface-fidelity shim work: run it before a
// shim (baseline) and after (did the tell clear, did we regress a real-browser check?).
//
// Usage:  node harness/botto-detect/run.mjs [--json] [--botto <path-to-botto-repo>]
// Skips cleanly if the napi addon isn't built or the botto repo isn't beside turbo-surf.

import { readFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

// ── locate the botto repo (sibling of turbo-surf by default) ──
const argBotto = (() => {
  const i = process.argv.indexOf("--botto");
  return i >= 0 ? process.argv[i + 1] : null;
})();
const BOTTO = resolve(argBotto || join(here, "../../../botto"));
const JSON_OUT = process.argv.includes("--json");

function skip(msg) {
  console.log(`botto-detect: skipping — ${msg}`);
  process.exit(0);
}

if (!existsSync(join(BOTTO, "harness/tools/browser-probe.inject.js"))) {
  skip(`botto repo not found at ${BOTTO} (pass --botto <path>)`);
}

// ── load the turbo-surf native addon (the live V8 render isolate) ──
let addon;
try {
  addon = require("../../rust/crates/turbo-surf-napi/index.js");
} catch (e) {
  skip(`turbo-surf addon not built (${e.message}) — run: cargo build -p turbo-surf-napi`);
}
for (const fn of ["liveOpen", "liveEval", "liveClose"]) {
  if (typeof addon[fn] !== "function") skip(`addon missing ${fn}() — rebuild the addon`);
}

// ── botto oracle (imported cross-repo) ──
const { liveFacts } = await import(pathToFileURL(join(BOTTO, "harness/env-collect.js")));
const { runChecks } = await import(pathToFileURL(join(BOTTO, "src/surface/checks.js")));
const { WEB_BASELINE } = await import(pathToFileURL(join(BOTTO, "harness/web-baseline.js")));
const { scanWindowSurfaceSource } = await import(
  pathToFileURL(join(BOTTO, "harness/tools/inject-window-surface.js"))
);

// ── botto's injectable probe sources (same files its own Playwright harness injects) ──
const bh = (p) => readFileSync(join(BOTTO, p), "utf8");
const FEATURE_INJECT = bh("harness/tools/feature-probe.inject.js");
const INJECT = bh("harness/tools/browser-probe.inject.js");
// Single-source the Botguard VM the way botto does: strip ESM `export`s and expose window.__bottoVM,
// so the client checksum and the backend re-run use byte-identical code (no drift false-positive).
const VM_SRC = bh("src/vm/bytecode-vm.js").replace(/^export /gm, "");
const VM_INJECT = `${VM_SRC}\nwindow.__bottoVM={attest,deriveSession};`;

// botto's real-content probe page (layout/paint/intersection have something to act on).
const PAGE_HTML =
  "<!doctype html><html><head><style>" +
  "body{margin:0;font:16px sans-serif}.g{height:80px;background:linear-gradient(45deg,#f04,#04f)}" +
  "</style></head><body><main><h1>botto attestation</h1>" +
  '<div class="g"></div><p>render probe target</p>' +
  "<ul>" +
  Array.from({ length: 30 }, (_, i) => `<li>row ${i}</li>`).join("") +
  "</ul></main></body></html>";
const BASE_URL = "https://probe.turbo-surf.test/";

// ── isolate helpers ──
// liveEval returns String(globalThis.__RESULT); wrap each probe so its (possibly async)
// value lands in __RESULT as JSON, then parse it back on the node side.
async function evalJson(id, expr) {
  const script =
    "globalThis.__RESULT = '__PENDING__';" +
    `Promise.resolve().then(() => (${expr})).then((v) => { globalThis.__RESULT = JSON.stringify(v); })` +
    ".catch((e) => { globalThis.__RESULT = JSON.stringify({ __error: String(e && e.message || e) }); });";
  const raw = await addon.liveEval(id, script);
  if (raw === "__PENDING__" || raw === "undefined" || raw == null) {
    return { __pending: true };
  }
  try {
    return JSON.parse(raw);
  } catch {
    return { __unparseable: String(raw).slice(0, 200) };
  }
}

async function run(id, src) {
  // Definitions script: no __RESULT expected.
  await addon.liveEval(id, src);
}

async function main() {
  const id = await addon.liveOpen(PAGE_HTML, BASE_URL);

  // botto's inject attaches to `window`; ensure the alias exists in the isolate (harness glue —
  // turbo-surf's global may not be named `window`). Recorded so we don't mistake it for a shim.
  await run(id, "globalThis.window = globalThis.window || globalThis;");

  // Inject the three probe sources (feature probe, integrity VM, main collector).
  await run(id, FEATURE_INJECT);
  await run(id, VM_INJECT);
  await run(id, INJECT);

  // Collect the env-fact vector exactly as botto's browser-collect.js does.
  const envFacts = await evalJson(id, "window.__bottoFacts()");
  const audio = await evalJson(id, "window.__bottoAudio()");
  const storage = await evalJson(id, "window.__bottoStorage()");
  const perms = await evalJson(id, "window.__bottoPermissions()");
  const clock = await evalJson(id, "window.__bottoClock()");
  const gpuAdapter = await evalJson(id, "window.__bottoGpuAdapter()");
  const attestation = await evalJson(id, "window.__bottoAttest()");
  Object.assign(envFacts, audio, storage, perms, clock, gpuAdapter, attestation);

  // Window-surface scan (source-string expression → the surface map).
  const scanExpr = scanWindowSurfaceSource(3);
  const surface = await evalJson(id, scanExpr);

  addon.liveClose(id);

  // Feed botto's own enrichment + check pipeline.
  const facts = liveFacts(envFacts, surface && surface.__error ? {} : surface, WEB_BASELINE);
  const verdict = runChecks(facts);

  if (JSON_OUT) {
    console.log(JSON.stringify({ envFacts, verdict }, null, 2));
    return;
  }

  const tells = verdict.tells || [];
  const strong = tells.filter((t) => t.severity === "strong");
  console.log("=== botto verdict on turbo-surf render isolate ===");
  console.log(
    `fake=${verdict.fake}  suspicion=${verdict.suspicion.toFixed(3)}  tells=${tells.length}`,
  );
  console.log("");
  console.log("STRONG tells (any one sets fake=true):");
  for (const t of strong) console.log(`  ✖ ${t.id}  (w${t.weight ?? "?"})`);
  if (!strong.length) console.log("  (none)");
  console.log("");
  console.log("all tells:");
  for (const t of tells) console.log(`  - ${t.id}  (w${t.weight ?? "?"})`);
  if (envFacts.__error) console.log(`\nenvFacts error: ${envFacts.__error}`);
}

main().catch((e) => {
  console.error("botto-detect: harness error —", e && e.stack ? e.stack : e);
  process.exitCode = 1;
});
