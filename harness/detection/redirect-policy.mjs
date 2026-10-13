import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs"; // Real two-origin redirect credential stripping and filtered/error-mode oracle.
import { createServer } from "node:http";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { differences } from "./api-shape.mjs";
import { run } from "./script-credentials.mjs";
const here = fileURLToPath(new URL(".", import.meta.url));
const html = readFileSync(join(here, "fixtures/redirect-policy.html"), "utf8");
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
export function redirectErrors(value, requests) {
  const errors = [];
  if (value?.same !== true || value?.cross !== false || value?.back !== false)
    errors.push("authorization origin transition");
  if (
    value?.manual?.type !== "opaqueredirect" ||
    value.manual.status !== 0 ||
    value.manual.ok !== false ||
    value.manual.url !== "/manual" ||
    value.manual.headers.length ||
    value.manual.text !== ""
  )
    errors.push("manual filtered response");
  if (value?.errorRejected !== true) errors.push("error mode rejection");
  if (value?.unsafeAllowed !== true || value?.unsafeDenied !== true)
    errors.push("redirected preflight authorization");
  if (requests.some((row) => row.path === "/unsafe-blocked" && row.method !== "OPTIONS"))
    errors.push("denied unsafe request was sent");
  for (const path of ["/unsafe-step", "/unsafe-final", "/unsafe-blocked"])
    if (!requests.some((row) => row.path === path && row.method === "OPTIONS"))
      errors.push(path + " preflight absent");
  for (const path of ["/manual-final", "/error-final"])
    if (requests.some((row) => row.path === path)) errors.push(path + " was followed");
  return errors;
}
export async function captureRedirectPolicy({ binary, out, chromeOnly = false }) {
  if (existsSync(join(out, "comparison.json"))) throw Error("Use a fresh output directory");
  mkdirSync(out, { recursive: true, mode: 0o700 });
  let frozen, build;
  if (!chromeOnly) {
    frozen = join(out, "native-executable");
    copyFileSync(binary, frozen);
    chmodSync(frozen, 0o700);
    build = JSON.parse(execFileSync(frozen, ["--build-info"], { encoding: "utf8" }));
  }
  let mode, source, cross;
  const requests = [];
  const makeServer = (isSource) =>
    createServer((req, res) => {
      const path = new URL(req.url, "http://fixture.test").pathname;
      requests.push({
        mode,
        path,
        method: req.method,
        origin: req.headers.origin || "",
        source: isSource,
        authorization: !!req.headers.authorization,
      });
      res.setHeader("Access-Control-Allow-Origin", req.headers.origin || source);
      res.setHeader("Content-Type", "application/json");
      if (path === "/") {
        res.setHeader("Content-Type", "text/html");
        res.end(html);
        return;
      }
      if (req.method === "OPTIONS") {
        res.setHeader("Access-Control-Allow-Methods", "PUT");
        if (path !== "/unsafe-blocked") res.setHeader("Access-Control-Allow-Headers", "x-hop");
        res.end("");
        return;
      }
      const targets = {
        "/unsafe-allowed": cross + "/unsafe-step",
        "/unsafe-step": source + "/unsafe-final",
        "/unsafe-denied": cross + "/unsafe-blocked",
        "/same": source + "/same-final",
        "/cross": cross + "/cross-final",
        "/back": cross + "/back-step",
        "/back-step": source + "/back-final",
        "/manual": source + "/manual-final",
        "/error": source + "/error-final",
      };
      if (targets[path]) {
        res.writeHead(path.startsWith("/unsafe-") ? 307 : 302, { Location: targets[path] });
        res.end("redirect");
        return;
      }
      res.end(
        JSON.stringify({
          authorization: !!req.headers.authorization,
          method: req.method,
          hop: !!req.headers["x-hop"],
          origin: req.headers.origin || "",
        }),
      );
    });
  const servers = [makeServer(true), makeServer(false)];
  try {
    for (const server of servers)
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
    source = `http://127.0.0.1:${servers[0].address().port}`;
    cross = `http://127.0.0.1:${servers[1].address().port}`;
    const config = {
      url: source + "/",
      interfaces: false,
      waitUntil: "load",
      readySelector: "body[data-browser-fixture='complete']",
      successSelector: "body",
      waitMs: 0,
      steps: [],
      probes: [],
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
    const values = traces.map((trace) => trace.pages[0].browserFixture);
    const comparison = {
      build,
      binaryHash: frozen ? hash(readFileSync(frozen)) : null,
      sourceHash: hash(html),
      chromeVersion: traces[0].browserVersion,
      chromeV8Version: traces[0].browserV8Version,
      chrome: values[0],
      native: values[1],
      requests,
      errors: values.map((value, index) =>
        redirectErrors(
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
  const out = resolve(flag("--out", join(here, "artifacts", `redirect-policy-${Date.now()}`)));
  const result = await captureRedirectPolicy({
    binary: resolve(flag("--binary", "rust/target/debug/examples/detection_trace")),
    out,
    chromeOnly: process.argv.includes("--chrome-only"),
  });
  console.log(JSON.stringify({ out, errors: result.errors, differences: result.differences }));
  if (result.errors.some((errors) => errors.length)) process.exitCode = 1;
}
