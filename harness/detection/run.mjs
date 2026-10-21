// Reusable Chrome/native differential. Live network is explicit; native never imports
// the Chrome jar. All raw artifacts are local, mode 0600, and excluded from Git.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import {
  chmodSync,
  closeSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  statSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { checkoutProvenance } from "./checkout-provenance.mjs";
import { inputCadenceMs, searchSelector, searchSteps } from "./search-plan.mjs";
import { validateSteps } from "./validate-steps.mjs";
import { superviseChild } from "./child-supervision.mjs";
import { observationMilliseconds } from "./observation-window.mjs";
const here = fileURLToPath(new URL(".", import.meta.url));
const root = resolve(here, "../..");
const argv = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = argv.indexOf(name);
  return i < 0 ? fallback : argv[i + 1];
};
if (!argv.includes("--live")) {
  console.log("Opt in with --live. See harness/detection/README.md for build and trace commands.");
  process.exit(0);
}
const out = resolve(
  flag("--out", join(here, "artifacts", new Date().toISOString().replaceAll(":", "-"))),
);
const binary = resolve(flag("--binary", join(root, "rust/target/debug/examples/detection_trace")));
if (!existsSync(binary))
  throw new Error(`Build current detection_trace with all features first: ${binary}`);
const query = flag("--query", "rust lang");
const selector = flag("--selector", searchSelector);
const targetUrl = flag("--url", "https://www.google.com/");
const config = {
  url: targetUrl,
  requireNativeFormNavigation: argv.includes("--require-native-form-navigation"),
  disableFormFallback: argv.includes("--no-form-fallback"),
  dismissSelector: flag(
    "--dismiss-selector",
    new URL(targetUrl).hostname === "www.google.com" ? "#W0wltc" : "",
  ),
  locale: "en-US",
  chromeBackend: flag("--chrome-backend", "cdp"),
  chromeIncognito: argv.includes("--chrome-incognito"),
  chromePath: flag("--chrome-path", process.env.TURBO_SURF_CHROME_PATH),
  languages: ["en-US", "en"],
  viewport: { width: Number(flag("--width", "1280")), height: Number(flag("--height", "800")) },
  screen: { width: 1920, height: 1080 },
  deviceScaleFactor: 2,
  timezone: flag("--tz", "America/Los_Angeles"),
  waitMs: Number(flag("--wait", "2500")),
  observationMs: Number(flag("--observation-ms", flag("--wait", "2500"))),
  inputCadenceMs,
  pageBudgetMs: Number(flag("--page-budget", "20000")),
  interfaceScript: resolve(flag("--interface-script", join(here, "interface-trace.js"))),
  successSelector: flag("--success-selector", "#rso a h3, #search a h3"),
  readySelector: flag(
    "--ready-selector",
    argv.includes("--no-interaction") || flag("--steps") ? "" : selector,
  ),
  probes: argv.includes("--probes")
    ? [
        join(here, "capabilities.js"),
        join(here, "api-shape.js"),
        join(root, "scripts/browser-sidecar/probes/botguard-probe.js"),
        join(root, "scripts/browser-sidecar/probes/webgl-fp.js"),
      ]
    : [],
  steps: argv.includes("--no-interaction") ? [] : searchSteps(query, selector),
};
config.outerWindow = { width: config.viewport.width, height: config.viewport.height + 88 };
for (const [index, argument] of argv.entries()) {
  if (argument !== "--probe") continue;
  const path = argv[index + 1];
  if (!path || path.startsWith("--")) throw new Error("--probe requires a JavaScript file path");
  config.probes.push(resolve(path));
}
for (const value of Object.values(config.viewport))
  if (!Number.isInteger(value) || value < 1)
    throw new Error("Viewport dimensions must be positive integers");
if (
  config.outerWindow.width > config.screen.width ||
  config.outerWindow.height > config.screen.height
)
  throw new Error("Configured window must fit the shared screen");
if (flag("--steps")) config.steps = JSON.parse(readFileSync(flag("--steps"), "utf8"));
if (config.chromeIncognito && config.chromeBackend !== "cdp")
  throw new Error("--chrome-incognito currently requires --chrome-backend cdp");
const captureTimeoutMs = Number(flag("--capture-timeout", "180000"));
if (!Number.isInteger(config.waitMs) || config.waitMs < 0 || config.waitMs > 60000)
  throw new Error("--wait must be an integer in 0..60000 milliseconds");
for (const [value, maximum] of [
  [config.pageBudgetMs, 120000],
  [captureTimeoutMs, 600000],
])
  if (!Number.isInteger(value) || value < 1 || value > maximum)
    throw new Error(
      `Page budget / capture timeout must be an integer in 1..${maximum} milliseconds`,
    );
observationMilliseconds(config);
validateSteps(config.steps);
const captureAbort = new AbortController();
for (const signal of ["SIGTERM", "SIGINT"])
  process.on(signal, () => {
    process.exitCode = signal === "SIGINT" ? 130 : 143;
    captureAbort.abort(new Error(`Differential interrupted by ${signal}`));
  });
const run = async (cmd, args, dest, env = {}) => {
  const logs = Object.fromEntries(
    ["stdout", "stderr"].map((name) => [name, openSync(join(dest, `${name}.log`), "w", 0o600)]),
  );
  const startedAt = new Date().toISOString();
  try {
    const {
      stdout: _stdout,
      stderr: _stderr,
      ...status
    } = await superviseChild(cmd, args, {
      signal: captureAbort.signal,
      timeoutMs: captureTimeoutMs,
      spawnOptions: { cwd: root, env: { ...process.env, ...env } },
      onOutput(name, bytes) {
        writeSync(logs[name], bytes);
        process[name].write(bytes);
      },
    });
    return { startedAt, finishedAt: new Date().toISOString(), ...status };
  } finally {
    for (const fd of Object.values(logs)) closeSync(fd);
  }
};
mkdirSync(out, { recursive: true, mode: 0o700 });
if (existsSync(join(out, "manifest.json")))
  throw new Error("Output directory already has a run; use a fresh directory");
const sha = (s) => createHash("sha256").update(s).digest("hex");
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
const manifest = {
  createdAt: new Date().toISOString(),
  commit: git("rev-parse", "HEAD"),
  branch: git("branch", "--show-current"),
  ...(await checkoutProvenance(root)),
  binary,
  captureTimeoutMs,
  binaryModifiedAt: statSync(binary).mtime.toISOString(),
  binaryHash: sha(readFileSync(binary)),
  harnessHashes: Object.fromEntries(
    [
      "run.mjs",
      "chrome-trace.mjs",
      "chrome-cdp-trace.mjs",
      "cdp-connection.mjs",
      "input-state.js",
      "search-plan.mjs",
      "validate-steps.mjs",
      "observation-window.mjs",
      "main-navigation.mjs",
      "collector-lifecycle.mjs",
      "effective-input.mjs",
      "child-supervision.mjs",
      "freeze-chrome.mjs",
      "interface-trace.js",
      "capabilities.js",
      "api-shape.js",
      "compare.mjs",
      "summarize-run.mjs",
      "protocol-headers.mjs",
      "chrome-network-metadata.mjs",
      "checkout-provenance.mjs",
    ].map((f) => [f, sha(readFileSync(join(here, f)))]),
  ),
  probeHashes: Object.fromEntries(config.probes.map((path) => [path, sha(readFileSync(path))])),
  note: "Binary hash/mtime records the executable; it does not prove that it matches checkout. Rebuild first.",
  runs: [],
};
const saveManifest = () =>
  writeFileSync(join(out, "manifest.json"), JSON.stringify(manifest, null, 2), { mode: 0o600 });
saveManifest();
// cargo test/build can overwrite target/debug/examples while Chrome is running.
// Freeze the executable used by this run before starting any captures.
const frozenBinary = join(out, "native-executable");
copyFileSync(binary, frozenBinary);
chmodSync(frozenBinary, 0o700);
manifest.frozenBinaryHash = sha(readFileSync(frozenBinary));
// Check the frozen executable before either runtime reaches the target URL.
manifest.nativeBuildInfo = JSON.parse(
  execFileSync(frozenBinary, ["--build-info"], { encoding: "utf8" }),
);
const missingFeatures = ["impersonate", "trustAnchors", "gpuMetal", "coretext"].filter(
  (feature) => !manifest.nativeBuildInfo.features?.[feature],
);
saveManifest();
if (missingFeatures.length && !argv.includes("--allow-reduced"))
  throw new Error(
    `Native features missing: ${missingFeatures.join(", ")}; rebuild with all features`,
  );
const repeats = Number(flag("--repeat", "1"));
if (!Number.isInteger(repeats) || repeats < 1 || repeats > 10)
  throw new Error("--repeat must be 1..10");
captureMatrix: for (let repeat = 0; repeat < repeats; repeat++) {
  const runOut = repeats === 1 ? out : join(out, `repeat-${repeat}`);
  mkdirSync(runOut, { recursive: true, mode: 0o700 });
  for (const instrumented of [false, true]) {
    config.interfaces = instrumented;
    config.nativeCpuProfile = instrumented && argv.includes("--native-cpu-profile");
    config.debugger = instrumented && (argv.includes("--debugger") || config.nativeCpuProfile);
    const path = join(runOut, instrumented ? "instrumented-config.json" : "baseline-config.json");
    writeFileSync(path, JSON.stringify(config, null, 2), { mode: 0o600 });
    const order = argv.includes("--native-first") ? ["native", "chrome"] : ["chrome", "native"];
    for (const runtime of order) {
      const pass = instrumented ? "interfaces" : "baseline";
      const dest = join(runOut, `${runtime}-${pass}`);
      mkdirSync(dest, { recursive: true, mode: 0o700 });
      console.log(`Capturing ${runtime} (${pass}, repeat ${repeat})`);
      const status =
        runtime === "chrome"
          ? await run(process.execPath, [join(here, "chrome-trace.mjs"), path, dest], dest)
          : await run(frozenBinary, [path, dest], dest, {
              TURBO_SURF_TZ: config.timezone,
              TURBO_SURF_BROWSER_CMD: "",
              TURBO_SURF_BROWSER_FETCH_CMD: "",
              TURBO_SURF_ENID_CACHE: join(dest, "unused-cookie-cache.json"),
              TURBO_SURF_JS_PROFILE: config.nativeCpuProfile ? "1" : "",
            });
      if (runtime === "native" && existsSync(join(dest, "trace.json"))) {
        const trace = JSON.parse(readFileSync(join(dest, "trace.json"), "utf8"));
        const missing = ["impersonate", "trustAnchors", "gpuMetal", "coretext"].filter(
          (f) => !trace.features?.[f],
        );
        if (missing.length && !argv.includes("--allow-reduced")) {
          status.error = `Native features missing: ${missing.join(", ")}; rebuild with all features`;
          console.error(status.error);
        }
      }
      manifest.runs.push({ repeat, runtime, pass, ...status });
      saveManifest();
      if (captureAbort.signal.aborted) break captureMatrix;
    }
  }
  if (config.probes.length) {
    // Fresh about:blank is insecure in Chrome. Use the same secure loopback
    // origin for both collectors so secure-only API enumeration is comparable.
    const fixtureHtml = "<!doctype html><html><head></head><body></body></html>";
    const probeServer = createServer((_request, response) => {
      response.setHeader("content-type", "text/html");
      response.end(fixtureHtml);
    });
    await new Promise((done, fail) => {
      probeServer.once("error", fail);
      probeServer.listen(0, "127.0.0.1", done);
    });
    const probeUrl = `http://127.0.0.1:${probeServer.address().port}/`;
    const path = join(runOut, "probe-config.json");
    writeFileSync(
      path,
      JSON.stringify(
        {
          ...config,
          url: probeUrl,
          fixtureHtml,
          fixtureHash: sha(fixtureHtml),
          dismissSelector: "",
          readySelector: "",
          steps: [],
          interfaces: false,
          debugger: false,
          nativeCpuProfile: false,
          probeOnly: true,
        },
        null,
        2,
      ),
      { mode: 0o600 },
    );
    try {
      for (const runtime of ["chrome", "native"]) {
        const dest = join(runOut, `${runtime}-probes`);
        mkdirSync(dest, { mode: 0o700 });
        const status =
          runtime === "chrome"
            ? await run(process.execPath, [join(here, "chrome-trace.mjs"), path, dest], dest)
            : await run(frozenBinary, [path, dest], dest, { TURBO_SURF_TZ: config.timezone });
        manifest.runs.push({ repeat, runtime, pass: "probes", ...status });
        saveManifest();
        if (captureAbort.signal.aborted) break captureMatrix;
      }
    } finally {
      probeServer.closeAllConnections();
      await new Promise((done) => probeServer.close(done));
    }
  }
  const comparisonStatus = await run(process.execPath, [join(here, "compare.mjs"), runOut], runOut);
  manifest.runs.push({ repeat, runtime: "comparison", ...comparisonStatus });
  saveManifest();
  console.log(`Saved differential: ${join(runOut, "comparison.json")}`);
}
if (manifest.runs.some((r) => r.code !== 0 || r.error || r.timedOut || r.aborted))
  process.exitCode ||= 1;
