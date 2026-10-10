// Reusable Chrome/native differential. Live network is explicit; native never imports
// the Chrome jar. All raw artifacts are local, mode 0600, and excluded from Git.
import { spawn, execFileSync } from "node:child_process";
import {
  mkdirSync,
  writeFileSync,
  existsSync,
  readFileSync,
  openSync,
  closeSync,
  writeSync,
  statSync,
  copyFileSync,
  chmodSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
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
const selector = flag("--selector", "textarea[name=q],input[name=q]");
const config = {
  url: flag("--url", "https://www.google.com/"),
  locale: "en-US",
  timezone: flag("--tz", "America/Los_Angeles"),
  waitMs: Number(flag("--wait", "2500")),
  interfaceScript: join(here, "interface-trace.js"),
  successSelector: flag("--success-selector", "#rso a h3, #search a h3"),
  probes: argv.includes("--probes")
    ? [
        join(here, "capabilities.js"),
        join(root, "scripts/browser-sidecar/probes/botguard-probe.js"),
        join(root, "scripts/browser-sidecar/probes/webgl-fp.js"),
      ]
    : [],
  steps: argv.includes("--no-interaction")
    ? []
    : [
        { move: { selector } },
        { click: selector },
        { focus: selector },
        { type: query },
        { wait: 350 },
        { press: "Enter" },
      ],
};
if (flag("--steps")) config.steps = JSON.parse(readFileSync(flag("--steps"), "utf8"));
if (
  !Array.isArray(config.steps) ||
  config.steps.some(
    (s) =>
      !s ||
      Object.keys(s).length !== 1 ||
      !["move", "click", "focus", "type", "press", "wait"].includes(Object.keys(s)[0]),
  )
)
  throw new Error("Each step must have one supported action");
const run = (cmd, args, dest, env = {}) =>
  new Promise((done) => {
    const logs = ["stdout", "stderr"].map((name) =>
      openSync(join(dest, `${name}.log`), "w", 0o600),
    );
    const startedAt = new Date().toISOString();
    let error,
      timedOut = false;
    const child = spawn(cmd, args, {
      cwd: root,
      stdio: ["ignore", "pipe", "pipe"],
      detached: process.platform !== "win32",
      env: { ...process.env, ...env },
    });
    for (const [i, stream] of [child.stdout, child.stderr].entries())
      stream.on("data", (data) => {
        writeSync(logs[i], data);
        (i ? process.stderr : process.stdout).write(data);
      });
    let forceTimer;
    const kill = (signal) => {
      try {
        if (process.platform === "win32") child.kill(signal);
        else process.kill(-child.pid, signal);
      } catch {}
    };
    const timer = setTimeout(() => {
      timedOut = true;
      kill("SIGTERM");
      forceTimer = setTimeout(() => kill("SIGKILL"), 5000);
    }, 180_000);
    child.on("error", (e) => {
      error = String(e);
    });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      clearTimeout(forceTimer);
      for (const fd of logs) closeSync(fd);
      done({ startedAt, finishedAt: new Date().toISOString(), code, signal, timedOut, error });
    });
  });
mkdirSync(out, { recursive: true, mode: 0o700 });
if (existsSync(join(out, "manifest.json")))
  throw new Error("Output directory already has a run; use a fresh directory");
const sha = (s) => createHash("sha256").update(s).digest("hex");
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
const manifest = {
  createdAt: new Date().toISOString(),
  commit: git("rev-parse", "HEAD"),
  branch: git("branch", "--show-current"),
  dirtyDiffHash: sha(git("diff", "HEAD")),
  binary,
  binaryModifiedAt: statSync(binary).mtime.toISOString(),
  binaryHash: sha(readFileSync(binary)),
  harnessHashes: Object.fromEntries(
    [
      "run.mjs",
      "chrome-trace.mjs",
      "interface-trace.js",
      "capabilities.js",
      "compare.mjs",
      "protocol-headers.mjs",
    ].map((f) => [f, sha(readFileSync(join(here, f)))]),
  ),
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
saveManifest();
const repeats = Number(flag("--repeat", "1"));
if (!Number.isInteger(repeats) || repeats < 1 || repeats > 10)
  throw new Error("--repeat must be 1..10");
for (let repeat = 0; repeat < repeats; repeat++) {
  const runOut = repeats === 1 ? out : join(out, `repeat-${repeat}`);
  mkdirSync(runOut, { recursive: true, mode: 0o700 });
  for (const instrumented of [false, true]) {
    config.interfaces = instrumented;
    config.debugger = instrumented && argv.includes("--debugger");
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
    }
  }
  if (config.probes.length) {
    const path = join(runOut, "probe-config.json");
    writeFileSync(
      path,
      JSON.stringify(
        {
          ...config,
          url: "about:blank",
          steps: [],
          interfaces: false,
          debugger: false,
          probeOnly: true,
        },
        null,
        2,
      ),
      { mode: 0o600 },
    );
    for (const runtime of ["chrome", "native"]) {
      const dest = join(runOut, `${runtime}-probes`);
      mkdirSync(dest, { mode: 0o700 });
      const status =
        runtime === "chrome"
          ? await run(process.execPath, [join(here, "chrome-trace.mjs"), path, dest], dest)
          : await run(frozenBinary, [path, dest], dest, { TURBO_SURF_TZ: config.timezone });
      manifest.runs.push({ repeat, runtime, pass: "probes", ...status });
      saveManifest();
    }
  }
  const comparisonStatus = await run(process.execPath, [join(here, "compare.mjs"), runOut], runOut);
  manifest.runs.push({ repeat, runtime: "comparison", ...comparisonStatus });
  saveManifest();
  console.log(`Saved differential: ${join(runOut, "comparison.json")}`);
}
if (manifest.runs.some((r) => r.code !== 0 || r.error)) process.exitCode = 1;
