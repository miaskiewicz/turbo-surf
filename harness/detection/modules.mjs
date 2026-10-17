// Shared offline module-graph/lifecycle oracle. No search-engine requests or sidecar.
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { differences } from "./api-shape.mjs";
const here = fileURLToPath(new URL(".", import.meta.url)),
  root = resolve(here, "../..");
const fixture = readFileSync(join(here, "fixtures/modules.html"));
export const moduleSources = Object.fromEntries(
  [
    "entry.mjs",
    "dep.mjs",
    "async.mjs",
    "late-dep.mjs",
    "wrong-mime.mjs",
    "tla.mjs",
    "runtime-error.mjs",
    "parse-error.mjs",
    "repeat.mjs",
  ].map((name) => [
    "/" + name,
    readFileSync(
      join(here, "fixtures/modules", name === "parse-error.mjs" ? "parse-error.source" : name),
      "utf8",
    ),
  ]),
);

Object.assign(
  moduleSources,
  Object.fromEntries(
    ["frame-child.html", "frame-entry.mjs", "frame-dep.mjs"].map((name) => [
      "/modules/" + name,
      readFileSync(join(here, "fixtures/modules", name), "utf8"),
    ]),
  ),
);
export function frameModuleErrors(value) {
  const errors = [];
  if (value?.parent !== "parent" || !value?.unpolluted || value?.frames?.length !== 2)
    errors.push("parent realm isolation");
  for (const frame of value?.frames || []) {
    if (
      frame.title !== "module child" ||
      frame.result?.owned !== "child" ||
      frame.inline?.owned !== "child" ||
      !frame.ownArray
    )
      errors.push("native child realm");
    if (frame.depRuns !== 1 || frame.entryRuns !== 1) errors.push("independent module graph cache");
    if (
      !frame.result?.current ||
      !frame.inline?.current ||
      frame.result.url !== "/modules/frame-entry.mjs" ||
      frame.inline.url !== "/modules/frame-child.html"
    )
      errors.push("module source and currentScript");
    if (JSON.stringify(frame.events) !== JSON.stringify(["module", "dcl", "load"]))
      errors.push("frame module lifecycle");
  }
  return errors;
}
export function moduleErrors(value) {
  const errors = [];
  if (!value?.dcl?.deferred || value.dcl.async || !value.dcl.currentScript)
    errors.push("DOMContentLoaded module ordering");
  for (const [id, type] of [
    ["deferred", "load"],
    ["async", "load"],
    ["missing", "error"],
    ["mime", "error"],
  ]) {
    const events = value?.scriptEvents?.[id];
    if (
      events?.length !== 1 ||
      events[0].type !== type ||
      !events[0].trusted ||
      !events[0].target ||
      !events[0].currentScript
    )
      errors.push(id + " module completion");
  }
  if (value?.scriptEvents?.inline || value?.scriptEvents?.inline2)
    errors.push("inline module load event");
  if (
    value?.inlineSources?.length !== 2 ||
    value.inlineSources.some(
      (source) => !source.url || !source.stack || source.local !== "independent",
    )
  )
    errors.push("inline module source identity");
  if (
    value?.inline !== true ||
    value?.deferred?.currentScript !== true ||
    value.deferred.source.currentScript !== true ||
    value?.async?.currentScript !== true
  )
    errors.push("module currentScript");
  if (
    value?.deferred?.source?.url != "/dep.mjs" ||
    value.deferred.entry != "/entry.mjs" ||
    !value.deferred.source.stack ||
    value.deferred.value !== "linked"
  )
    errors.push("module graph/source");
  const log = value?.events || [];
  if (
    log.indexOf("classic:loading") > log.indexOf("deferred:eval") ||
    log.indexOf("deferred:eval") > log.indexOf("dcl") ||
    log.indexOf("async:eval") < log.indexOf("dcl") ||
    log.indexOf("async:eval") > log.indexOf("window:load")
  )
    errors.push("module lifecycle ordering");
  return errors;
}
export function evaluationErrors(value) {
  const errors = [];
  if (!value?.dcl?.started || value.dcl.done || !value?.load?.started || value.load.done)
    errors.push("top-level await blocks document lifecycle");
  if (
    !value?.tla?.currentScript ||
    value.tla.readyState !== "complete" ||
    value.tla.url !== "/tla.mjs"
  )
    errors.push("top-level await source/currentScript");
  if (value?.repeat !== 1) errors.push("module graph cache");
  for (const id of ["tla", "runtime", "parse", "repeat1", "repeat2"]) {
    const events = value?.scriptEvents?.[id];
    if (
      events?.length !== 1 ||
      events[0].type !== "load" ||
      !events[0].trusted ||
      !events[0].target ||
      !events[0].currentScript
    )
      errors.push(id + " module evaluation completion");
  }
  for (const [name, filename] of [
    ["RangeError", "/runtime-error.mjs"],
    ["SyntaxError", "/parse-error.mjs"],
  ]) {
    const event = value?.errors?.find((x) => x.name === name);
    if (
      !event ||
      event.filename !== filename ||
      !event.trusted ||
      !event.target ||
      !event.currentScript ||
      !event.line ||
      event.brand !== "[object ErrorEvent]"
    )
      errors.push(name + " module Window error");
  }
  if (value?.errors?.length !== 2) errors.push("module error delivery count");
  return errors;
}
const run = (command, args, out) =>
  new Promise((done, fail) => {
    const child = spawn(command, args, {
      cwd: root,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, TURBO_SURF_BROWSER_CMD: "", TURBO_SURF_BROWSER_FETCH_CMD: "" },
    });
    let stdout = "",
      stderr = "";
    child.stdout.on("data", (x) => (stdout += x));
    child.stderr.on("data", (x) => (stderr += x));
    const timer = setTimeout(() => child.kill("SIGKILL"), 30000);
    child.on("error", fail);
    child.on("close", (code) => {
      clearTimeout(timer);
      writeFileSync(join(out, "stdout.log"), stdout);
      writeFileSync(join(out, "stderr.log"), stderr);
      code === 0 ? done() : fail(Error(stderr || String(code)));
    });
  });
export async function captureModules({
  binary,
  out,
  chromeOnly = false,
  evaluation = false,
  frames = false,
}) {
  const html = frames
    ? readFileSync(join(here, "fixtures/frame-modules.html"))
    : evaluation
      ? readFileSync(join(here, "fixtures/modules-evaluation.html"))
      : fixture;
  if (existsSync(join(out, "comparison.json"))) throw Error("Use a fresh output directory");
  mkdirSync(out, { recursive: true, mode: 0o700 });
  let build = null,
    frozen;
  if (!chromeOnly) {
    frozen = join(out, "native-executable");
    copyFileSync(binary, frozen);
    chmodSync(frozen, 0o700);
    build = JSON.parse(execFileSync(frozen, ["--build-info"], { encoding: "utf8" }));
  }
  const requests = [];
  let release, gate;
  const resetGate = () => {
    gate = new Promise((done) => (release = done));
  };
  resetGate();
  const server = createServer((request, response) => {
    requests.push(request.url);
    if (request.url === "/module-gate" || request.url === "/evaluation-gate") {
      release();
      response.end("ready");
      return;
    }
    const reply = () => {
      const code = moduleSources[request.url];
      response.statusCode = request.url === "/missing.mjs" ? 404 : 200;
      response.setHeader(
        "Content-Type",
        request.url === "/" || request.url.endsWith(".html")
          ? "text/html"
          : request.url === "/wrong-mime.mjs"
            ? "text/plain"
            : "application/javascript",
      );
      response.end(request.url === "/" ? html : code || "missing");
    };
    if (request.url === "/await-gate") gate.then(() => setTimeout(reply, 100));
    else if (request.url === "/late-dep.mjs") gate.then(reply);
    else reply();
  });
  try {
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    const config = {
      url: `http://127.0.0.1:${server.address().port}/`,
      interfaces: false,
      waitUntil: "load",
      readySelector: "body[data-browser-fixture='complete']",
      successSelector: "body",
      waitMs: 0,
      probes: [],
      steps: [],
    };
    const configPath = join(out, "config.json");
    writeFileSync(configPath, JSON.stringify(config));
    const traces = [];
    for (const name of chromeOnly ? ["chrome"] : ["chrome", "native"]) {
      resetGate();
      const dest = join(out, name);
      mkdirSync(dest, { mode: 0o700 });
      await run(
        name === "chrome" ? process.execPath : frozen,
        name === "chrome" ? [join(here, "chrome-trace.mjs"), configPath, dest] : [configPath, dest],
        dest,
      );
      traces.push(JSON.parse(readFileSync(join(dest, "trace.json"))));
    }
    const results = traces.map((trace) => trace.pages[0].browserFixture);
    const comparison = {
      schemaVersion: 1,
      build,
      fixtureHash: createHash("sha256").update(html).digest("hex"),
      sourceHashes: Object.fromEntries(
        Object.entries(moduleSources).map(([path, source]) => [
          path,
          createHash("sha256").update(source).digest("hex"),
        ]),
      ),
      binaryHash: frozen ? createHash("sha256").update(readFileSync(frozen)).digest("hex") : null,
      chromeVersion: traces[0].browserVersion,
      chrome: results[0],
      native: results[1],
      errors: results.map(
        frames ? frameModuleErrors : evaluation ? evaluationErrors : moduleErrors,
      ),
      differences: results.length === 2 ? differences(results[0], results[1]) : [],
      requests,
    };
    writeFileSync(join(out, "comparison.json"), JSON.stringify(comparison, null, 2));
    return comparison;
  } finally {
    server.closeAllConnections();
    await new Promise((done) => server.close(done));
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const flag = (name, fallback) => {
    const i = process.argv.indexOf(name);
    return i < 0 ? fallback : process.argv[i + 1];
  };
  if (!process.argv.includes("--run"))
    console.log("Use --run for the offline Chrome/native module lifecycle differential.");
  else {
    const out = resolve(flag("--out", join(here, "artifacts", `modules-${Date.now()}`)));
    const result = await captureModules({
      binary: resolve(flag("--binary", join(root, "rust/target/debug/examples/detection_trace"))),
      out,
      chromeOnly: process.argv.includes("--chrome-only"),
      evaluation: process.argv.includes("--evaluation"),
      frames: process.argv.includes("--frames"),
    });
    console.log(JSON.stringify({ out, errors: result.errors, differences: result.differences }));
    if (result.errors.some((x) => x.length) || result.differences.length) process.exitCode = 1;
  }
}
