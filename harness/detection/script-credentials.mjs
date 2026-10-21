// Two-origin localhost script/module credentials and redirect oracle.
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
const here = fileURLToPath(new URL(".", import.meta.url));
const sources = Object.fromEntries(
  ["page.html", "entry.mjs", "dep.mjs"].map((name) => [
    name,
    readFileSync(join(here, "fixtures/script-credentials", name), "utf8"),
  ]),
);
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
export const run = (command, args, out) =>
  new Promise((done, fail) => {
    const child = spawn(command, args, {
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, TURBO_SURF_BROWSER_CMD: "", TURBO_SURF_BROWSER_FETCH_CMD: "" },
    });
    let stdout = "",
      stderr = "";
    child.stdout.on("data", (part) => (stdout += part));
    child.stderr.on("data", (part) => (stderr += part));
    const timer = setTimeout(() => child.kill("SIGKILL"), 30000);
    child.on("error", fail);
    child.on("close", (code) => {
      clearTimeout(timer);
      writeFileSync(join(out, "stdout.log"), stdout);
      writeFileSync(join(out, "stderr.log"), stderr);
      code === 0 ? done() : fail(Error(stderr || String(code)));
    });
  });
export function credentialErrors(value, requests) {
  const errors = [];
  if (
    value?.module?.value !== "credentialed dependency" ||
    !value.module.current ||
    value.module.url !== "/entry.mjs"
  )
    errors.push("credentialed module graph");
  if (!value?.anonymous || !value?.classic) errors.push("script completion");
  for (const key of ["legacy", "nosniff", "json"])
    if (value?.[key] !== true) errors.push(`${key} MIME policy`);
  if (value?.cookie !== "") errors.push("HttpOnly visibility");
  if (value?.redirect?.value !== "redirect dependency" || value.redirect.url !== "/final/entry.mjs")
    errors.push("redirect source/import identity");
  const events = new Map(
    (value?.events || []).map(([id, type, trusted]) => [id, { type, trusted }]),
  );
  for (const id of ["module", "anonymous", "classic", "redirect", "legacy"])
    if (events.get(id)?.type !== "load" || !events.get(id).trusted) errors.push(`${id} event`);
  if (events.get("denied")?.type !== "error" || !events.get("denied").trusted)
    errors.push("denied redirect event");
  for (const id of ["nosniff", "json"])
    if (events.get(id)?.type !== "error" || !events.get(id).trusted)
      errors.push(`${id} MIME event`);
  for (const path of ["/entry.mjs", "/dep.mjs", "/classic.js"])
    if (!requests.some((row) => row.path === path && row.cookie && row.origin === "source"))
      errors.push(`${path} credentials/origin`);
  if (requests.some((row) => row.path === "/anonymous.mjs" && row.cookie))
    errors.push("anonymous module credentials");
  if (requests.some((row) => row.path === "/must-not-request.mjs"))
    errors.push("denied redirect was followed");
  if (
    !requests.some((row) => row.path === "/final/entry.mjs" && row.origin === "null" && row.cookie)
  )
    errors.push("redirect-tainted origin");
  return errors;
}
export async function captureCredentials({ binary, out, chromeOnly = false }) {
  if (existsSync(join(out, "comparison.json"))) throw Error("Use a fresh output directory");
  mkdirSync(out, { recursive: true, mode: 0o700 });
  let frozen, build;
  if (!chromeOnly) {
    frozen = join(out, "native-executable");
    copyFileSync(binary, frozen);
    chmodSync(frozen, 0o700);
    build = JSON.parse(execFileSync(frozen, ["--build-info"], { encoding: "utf8" }));
  }
  const requests = [];
  let sourceOrigin, crossOrigin, mode;
  const makeServer = (isSource) =>
    createServer((request, response) => {
      const path = new URL(request.url, "http://fixture.test").pathname;
      requests.push({
        mode,
        path,
        server: isSource ? "source" : "cross",
        origin: request.headers.origin === sourceOrigin ? "source" : request.headers.origin || "",
        cookie: (request.headers.cookie || "").includes("secret=session"),
      });
      response.setHeader(
        "Content-Type",
        path === "/"
          ? "text/html"
          : ["/nosniff.js", "/legacy.js"].includes(path)
            ? "text/plain"
            : path === "/json.mjs"
              ? "application/json"
              : "text/javascript",
      );
      if (path === "/nosniff.js") response.setHeader("X-Content-Type-Options", "nosniff");
      if (isSource && path === "/") {
        response.setHeader("Set-Cookie", "secret=session; HttpOnly; Path=/");
        response.end(sources["page.html"].replaceAll("{{CROSS}}", crossOrigin));
        return;
      }
      if (!isSource && path === "/denied") {
        response.writeHead(302, { Location: `${sourceOrigin}/must-not-request.mjs` });
        response.end();
        return;
      }
      if (!isSource && path === "/redirect") {
        response.writeHead(302, {
          "Access-Control-Allow-Origin": sourceOrigin,
          "Access-Control-Allow-Credentials": "true",
          Location: `${sourceOrigin}/final/entry.mjs`,
        });
        response.end();
        return;
      }
      const body =
        path === "/nosniff.js"
          ? "globalThis.nosniffExecuted=true;"
          : path === "/legacy.js"
            ? "globalThis.legacyMime=true;"
            : path === "/json.mjs"
              ? "globalThis.jsonExecuted=true;"
              : path === "/entry.mjs"
                ? sources["entry.mjs"]
                : path === "/dep.mjs"
                  ? sources["dep.mjs"]
                  : path === "/classic.js"
                    ? "globalThis.credentialClassic=true;"
                    : path === "/anonymous.mjs"
                      ? "globalThis.anonymousModule=true;"
                      : path === "/final/entry.mjs"
                        ? "import {value} from './dep.mjs';globalThis.redirectModule={value,url:new URL(import.meta.url).pathname};"
                        : path === "/final/dep.mjs"
                          ? "export const value='redirect dependency';"
                          : null;
      if (!isSource) {
        response.setHeader(
          "Access-Control-Allow-Origin",
          ["/anonymous.mjs", "/json.mjs"].includes(path) ? "*" : sourceOrigin,
        );
        response.setHeader("Access-Control-Allow-Credentials", "true");
      }
      if (path === "/final/entry.mjs") {
        response.setHeader("Access-Control-Allow-Origin", "null");
        response.setHeader("Access-Control-Allow-Credentials", "true");
      }
      response.statusCode = body === null ? 404 : 200;
      response.end(body || "missing");
    });
  const source = makeServer(true),
    cross = makeServer(false),
    servers = [source, cross];
  try {
    for (const server of servers)
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
    sourceOrigin = `http://127.0.0.1:${source.address().port}`;
    crossOrigin = `http://127.0.0.1:${cross.address().port}`;
    const config = {
      url: `${sourceOrigin}/`,
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
      value?.events?.sort((a, b) => a[0].localeCompare(b[0]));
      return value;
    });
    const comparison = {
      schemaVersion: 1,
      build,
      binaryHash: frozen ? hash(readFileSync(frozen)) : null,
      sourceHashes: Object.fromEntries(
        Object.entries(sources).map(([name, source]) => [name, hash(source)]),
      ),
      chromeVersion: traces[0].browserVersion,
      chrome: values[0],
      native: values[1],
      requests,
      errors: values.map((value, index) =>
        credentialErrors(
          value,
          requests.filter((row) => row.mode === (index ? "native" : "chrome")),
        ),
      ),
      differences: values.length === 2 ? differences(values[0], values[1]) : [],
    };
    writeFileSync(join(out, "comparison.json"), JSON.stringify(comparison, null, 2));
    return comparison;
  } finally {
    for (const server of servers) {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
    }
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const flag = (name, fallback) => {
    const index = process.argv.indexOf(name);
    return index < 0 ? fallback : process.argv[index + 1];
  };
  const out = resolve(flag("--out", join(here, "artifacts", `script-credentials-${Date.now()}`)));
  const result = await captureCredentials({
    binary: resolve(flag("--binary", "rust/target/debug/examples/detection_trace")),
    out,
    chromeOnly: process.argv.includes("--chrome-only"),
  });
  console.log(JSON.stringify({ out, errors: result.errors, differences: result.differences }));
  if (result.errors.some((errors) => errors.length)) process.exitCode = 1;
}
