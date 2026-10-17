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
// Shared localhost resource/lifecycle fixture. No search engine or cookie sidecar.
import { createServer } from "node:http";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { differences } from "./api-shape.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const root = resolve(here, "../..");
const fixture = readFileSync(join(here, "fixtures/lifecycle.html"));
const image = readFileSync(join(here, "fixtures/pixel.png"));
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");

export function lifecycleErrors(result) {
  const errors = [];
  const expected = [
    "classic:loading",
    "body:loading",
    "state:interactive",
    "defer:interactive",
    "dcl:interactive",
    "image:load",
    "image:error",
    "state:complete",
    "load:complete",
    "pageshow",
  ];
  if (JSON.stringify(result.events) !== JSON.stringify(expected)) errors.push("lifecycle ordering");
  if (result.imageAtDcl !== false) errors.push("image delayed DOMContentLoaded");
  if (JSON.stringify(result.image) !== JSON.stringify({ complete: true, width: 3, height: 2 }))
    errors.push("decoded image dimensions");
  if (JSON.stringify(result.broken) !== JSON.stringify({ complete: true, width: 0 }))
    errors.push("invalid image load semantics");
  if (
    result.linked?.imported !== "em" ||
    JSON.stringify(result.linked?.rules) !== JSON.stringify(["import", "sentinel"])
  )
    errors.push("stylesheet import graph");
  if (result.dynamic?.imageWidth !== 3 || result.dynamic?.styleRule !== "dynamic-sentinel")
    errors.push("dynamic resources");
  if (result.control !== "parsed-token") errors.push("parsed hidden control");
  if (
    result.hostEvents?.length !== 5 ||
    result.hostEvents.some(
      (event) => !event.trusted || event.phase !== 2 || !event.timestampValid,
    ) ||
    result.completedEventStates?.length !== 5 ||
    result.completedEventStates.some(
      (event, index) =>
        !event.trusted ||
        (index < 3
          ? event.currentTarget !== null || event.phase !== 0
          : event.currentTarget !== "window" || event.phase !== 2 || event.pathLength !== 1),
    ) ||
    result.redispatchStates?.length !== 2 ||
    result.redispatchStates.some((event) => event.error !== "InvalidStateError" || !event.trusted)
  )
    errors.push("host event dispatch state");
  return errors;
}

async function run(command, args, dest, timezone) {
  return new Promise((done, fail) => {
    const child = spawn(command, args, {
      cwd: root,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        TURBO_SURF_TZ: timezone,
        TURBO_SURF_BROWSER_CMD: "",
        TURBO_SURF_BROWSER_FETCH_CMD: "",
      },
    });
    let stdout = "",
      stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    const timer = setTimeout(() => child.kill("SIGKILL"), 60_000);
    child.on("error", fail);
    child.on("close", (code) => {
      clearTimeout(timer);
      writeFileSync(join(dest, "stdout.log"), stdout, { mode: 0o600 });
      writeFileSync(join(dest, "stderr.log"), stderr, { mode: 0o600 });
      if (code === 0) done();
      else fail(new Error(`${command} exited ${code}: ${stderr}`));
    });
  });
}

export async function captureLifecycle({ binary, out }) {
  if (!existsSync(binary)) throw new Error("Build the detection_trace example first");
  if (existsSync(join(out, "comparison.json"))) throw new Error("Use a fresh output directory");
  mkdirSync(out, { recursive: true, mode: 0o700 });
  const frozen = join(out, "native-executable");
  copyFileSync(binary, frozen);
  chmodSync(frozen, 0o700);
  const build = JSON.parse(execFileSync(frozen, ["--build-info"], { encoding: "utf8" }));
  const requests = [];
  let releaseResources;
  let resourceGate = Promise.resolve();
  const server = createServer((request, response) => {
    requests.push({ path: request.url, headers: request.headers });
    if (request.url === "/")
      resourceGate = new Promise((done) => {
        releaseResources = done;
      });
    if (request.url === "/resources-ready") {
      releaseResources();
      response.end("ready");
      return;
    }
    const routes = {
      "/": ["text/html", fixture, 0],
      "/main.css": ["text/css", "@import '/import.css'; sentinel { color: blue; }", 0],
      "/import.css": ["text/css", "em { color: red; }", 0],
      "/dynamic.css": ["text/css", "dynamic-sentinel {}", 0],
      "/defer.js": ["application/javascript", "fixtureLog('defer:'+document.readyState);", 0],
      "/pixel.png": ["image/png", image, 150],
      "/broken.png": ["image/png", "not an image", 200],
      "/dynamic.png": ["image/png", image, 250],
    };
    const [mime, body, delay] = routes[request.url] || ["text/plain", "missing", 0];
    const respond = () =>
      setTimeout(() => {
        response.setHeader("Content-Type", mime);
        response.end(body);
      }, delay);
    if (request.url.endsWith(".png")) resourceGate.then(respond);
    else respond();
  });
  try {
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    const config = {
      url: `http://127.0.0.1:${server.address().port}/`,
      viewport: { width: 1280, height: 800 },
      screen: { width: 1920, height: 1080 },
      outerWindow: { width: 1280, height: 888 },
      deviceScaleFactor: 2,
      languages: ["en-US", "en"],
      locale: "en-US",
      timezone: "America/Los_Angeles",
      waitMs: 0,
      waitUntil: "load",
      readySelector: "body[data-browser-fixture='complete']",
      interfaces: false,
      steps: [],
      successSelector: "body",
    };
    const configPath = join(out, "config.json");
    writeFileSync(configPath, JSON.stringify(config, null, 2), { mode: 0o600 });
    const traces = [];
    for (const runtime of ["chrome", "native"]) {
      const dest = join(out, runtime);
      mkdirSync(dest, { mode: 0o700 });
      if (runtime === "chrome")
        await run(
          process.execPath,
          [join(here, "chrome-trace.mjs"), configPath, dest],
          dest,
          config.timezone,
        );
      else await run(frozen, [configPath, dest], dest, config.timezone);
      traces.push(JSON.parse(readFileSync(join(dest, "trace.json"))));
    }
    const rows = traces.map((trace) => trace.pages[0]);
    const comparison = {
      schemaVersion: 1,
      build,
      binaryHash: hash(readFileSync(frozen)),
      fixtureHashes: { html: hash(fixture), image: hash(image) },
      harnessHashes: Object.fromEntries(
        [
          "lifecycle.mjs",
          "chrome-trace.mjs",
          "capabilities.js",
          "api-shape.js",
          "api-shape.mjs",
        ].map((file) => [file, hash(readFileSync(join(here, file)))]),
      ),
      chromeVersion: traces[0].browserVersion,
      chrome: rows[0].browserFixture,
      native: rows[1].browserFixture,
      environment: { chrome: rows[0].environment, native: rows[1].environment },
      fixtureErrors: rows.map((row) => lifecycleErrors(row.browserFixture || {})),
      differences: differences(rows[0].browserFixture, rows[1].browserFixture),
      environmentDifferences: differences(rows[0].environment, rows[1].environment),
      requests,
    };
    writeFileSync(join(out, "comparison.json"), JSON.stringify(comparison, null, 2), {
      mode: 0o600,
    });
    return comparison;
  } finally {
    server.closeAllConnections();
    await new Promise((done) => server.close(done));
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2),
    flag = (name, fallback) => {
      const index = args.indexOf(name);
      return index < 0 ? fallback : args[index + 1];
    };
  if (!args.includes("--run"))
    console.log("Use --run for the localhost Chrome/native lifecycle and resource differential.");
  else {
    const out = resolve(flag("--out", join(here, "artifacts", `lifecycle-${Date.now()}`)));
    const result = await captureLifecycle({
      binary: resolve(flag("--binary", join(root, "rust/target/debug/examples/detection_trace"))),
      out,
    });
    console.log(
      JSON.stringify({
        out,
        fixtureErrors: result.fixtureErrors,
        differences: result.differences,
        environmentDifferences: result.environmentDifferences,
      }),
    );
    if (
      result.fixtureErrors.some((errors) => errors.length) ||
      result.differences.length ||
      result.environmentDifferences.length
    )
      process.exitCode = 1;
  }
}
