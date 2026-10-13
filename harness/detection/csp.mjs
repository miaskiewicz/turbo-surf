// Enforced header/meta CSP subset on genuine scripts and network requests.
import { execFileSync } from "node:child_process";
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
import { run } from "./script-credentials.mjs";
const here = fileURLToPath(new URL(".", import.meta.url));
const html = readFileSync(join(here, "fixtures/csp.html"), "utf8");
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
export const policy = `script-src 'nonce-good' 'sha256-${createHash("sha256").update("globalThis.hashAllowed=true;").digest("base64")}'; connect-src 'none'; style-src 'none'; img-src 'none'; frame-src 'none'`;
export function cspErrors(value, requests) {
  const errors = [];
  for (const key of [
    "inlineBlocked",
    "hashAllowed",
    "nonceAllowed",
    "evalBlocked",
    "connectBlocked",
    "classic",
    "module",
    "externalBlocked",
  ])
    if (value?.[key] !== true) errors.push(key);
  for (const path of ["/allowed.js", "/allowed.mjs", "/dep.mjs"])
    if (!requests.includes(path)) errors.push(`${path} allowed request`);
  for (const path of [
    "/blocked.js",
    "/blocked-connect",
    "/blocked.css",
    "/blocked.png",
    "/blocked-frame",
  ])
    if (requests.includes(path)) errors.push(`${path} blocked request`);
  return errors;
}
export async function captureCsp({ binary, out, chromeOnly = false }) {
  if (existsSync(join(out, "comparison.json"))) throw Error("Use a fresh output directory");
  mkdirSync(out, { recursive: true, mode: 0o700 });
  let frozen, build;
  if (!chromeOnly) {
    frozen = join(out, "native-executable");
    copyFileSync(binary, frozen);
    chmodSync(frozen, 0o700);
    build = JSON.parse(execFileSync(frozen, ["--build-info"], { encoding: "utf8" }));
  }
  let mode;
  const requests = [];
  const server = createServer((request, response) => {
    const path = new URL(request.url, "http://fixture.test").pathname;
    requests.push({ mode, path });
    response.setHeader("Content-Type", path === "/" ? "text/html" : "text/javascript");
    if (path === "/") {
      response.setHeader("Content-Security-Policy", policy);
      response.end(html);
      return;
    }
    const body =
      path === "/allowed.js"
        ? "globalThis.allowedClassic=true;"
        : path === "/allowed.mjs"
          ? "import {allowed} from './dep.mjs';globalThis.allowedModule=allowed;"
          : path === "/dep.mjs"
            ? "export const allowed=true;"
            : path === "/blocked.js"
              ? "globalThis.blockedExternal=true;"
              : null;
    response.statusCode = body === null ? 404 : 200;
    response.end(body || "missing");
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
      mode = name;
      const dest = join(out, name);
      mkdirSync(dest, { mode: 0o700 });
      await run(
        name === "chrome" ? process.execPath : frozen,
        name === "chrome"
          ? [join(here, "chrome-cdp-trace.mjs"), configPath, dest]
          : [configPath, dest],
        dest,
      );
      traces.push(JSON.parse(readFileSync(join(dest, "trace.json"))));
    }
    const values = traces.map((trace) => {
      const value = trace.pages[0].browserFixture;
      if (value) value.events.sort((a, b) => a[0].localeCompare(b[0]));
      return value;
    });
    const comparison = {
      schemaVersion: 1,
      build,
      binaryHash: frozen ? hash(readFileSync(frozen)) : null,
      fixtureHash: hash(html),
      policy,
      chromeVersion: traces[0].browserVersion,
      chrome: values[0],
      native: values[1],
      requests,
      errors: values.map((value, index) =>
        cspErrors(
          value,
          requests
            .filter((row) => row.mode === (index ? "native" : "chrome"))
            .map((row) => row.path),
        ),
      ),
      differences: values.length === 2 ? differences(values[0], values[1]) : [],
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
    const index = process.argv.indexOf(name);
    return index < 0 ? fallback : process.argv[index + 1];
  };
  const out = resolve(flag("--out", join(here, "artifacts", `csp-${Date.now()}`)));
  const result = await captureCsp({
    binary: resolve(flag("--binary", "rust/target/debug/examples/detection_trace")),
    out,
    chromeOnly: process.argv.includes("--chrome-only"),
  });
  console.log(JSON.stringify({ out, errors: result.errors, differences: result.differences }));
  if (result.errors.some((errors) => errors.length)) process.exitCode = 1;
}
