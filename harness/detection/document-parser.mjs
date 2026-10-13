// Offline parsed-document differential; both engines load the same saved HTML.
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
const fixture = readFileSync(join(here, "fixtures/document-parser.html"));
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
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
export function parserErrors(value) {
  const errors = [];
  for (const key of ["distinct", "parent", "created"]) if (!value?.[key]) errors.push(key);
  for (const key of ["dom", "unsafe", "safe"]) {
    const doc = value?.[key];
    if (
      !doc?.view ||
      !doc.location ||
      !doc.inert ||
      !doc.rootOwned ||
      !doc.bodyOwned ||
      !doc.headOwned ||
      !doc.documentCtor ||
      !doc.documentProto ||
      (key === "dom" ? !doc.url?.startsWith("http://127.0.0.1:") : doc.url !== "about:blank") ||
      doc.cookie !== "" ||
      doc.ready !== "complete"
    )
      errors.push(key + " independent document");
  }
  if (
    value?.safe?.head !== "<title>parsed</title>" ||
    value?.safe?.body !== "<p><a>link</a></p><svg><a>vector</a></svg><math><mi>x</mi></math>"
  )
    errors.push("native default sanitizer");
  if (
    value?.import?.owned !== true ||
    value.import.original !== true ||
    value.import.copied !== true ||
    value.import.text !== "link"
  )
    errors.push("native import");
  if (value?.invalid?.some((error) => error !== "TypeError")) errors.push("MIME validation");
  return errors;
}
export async function captureParser({ binary, out, chromeOnly = false }) {
  if (existsSync(join(out, "comparison.json"))) throw new Error("Use a fresh output directory");
  mkdirSync(out, { recursive: true, mode: 0o700 });
  let frozen = null,
    build = null;
  if (!chromeOnly) {
    frozen = join(out, "native-executable");
    copyFileSync(binary, frozen);
    chmodSync(frozen, 0o700);
    build = JSON.parse(execFileSync(frozen, ["--build-info"], { encoding: "utf8" }));
  }
  const requests = [];
  const server = createServer((req, res) => {
    requests.push(req.url);
    res.setHeader("Content-Type", "text/html");
    res.end(req.url === "/" ? fixture : "inert resource");
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
    for (const runtime of chromeOnly ? ["chrome"] : ["chrome", "native"]) {
      const dest = join(out, runtime);
      mkdirSync(dest, { mode: 0o700 });
      await run(
        runtime === "chrome" ? process.execPath : frozen,
        runtime === "chrome"
          ? [join(here, "chrome-trace.mjs"), configPath, dest]
          : [configPath, dest],
        dest,
      );
      traces.push(JSON.parse(readFileSync(join(dest, "trace.json"))));
    }
    const values = traces.map((trace) => trace.pages[0].browserFixture);
    const comparison = {
      schemaVersion: 1,
      build,
      binaryHash: frozen ? hash(readFileSync(frozen)) : null,
      fixtureHash: hash(fixture),
      chromeVersion: traces[0].browserVersion,
      chrome: values[0],
      native: values[1],
      errors: values.map(parserErrors),
      differences: values.length === 2 ? differences(values[0], values[1]) : [],
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
    console.log("Use --run for the localhost Chrome/native parsed-document differential.");
  else {
    const out = resolve(flag("--out", join(here, "artifacts", `document-parser-${Date.now()}`)));
    const result = await captureParser({
      binary: resolve(flag("--binary", join(root, "rust/target/debug/examples/detection_trace"))),
      out,
      chromeOnly: process.argv.includes("--chrome-only"),
    });
    console.log(JSON.stringify({ out, errors: result.errors, differences: result.differences }));
    if (result.errors.some((errors) => errors.length) || result.differences.length)
      process.exitCode = 1;
  }
}
