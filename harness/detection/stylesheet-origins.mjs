// Local CSS authorization oracle: native DOM, private policy and CSSOM mutation routes.
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
const html = readFileSync(join(here, "fixtures/stylesheet-origins.html"), "utf8");
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
export function stylesheetErrors(value) {
  const errors = [];
  for (const id of ["same", "cors", "include"])
    if (!value?.[id]?.sheet || value[id].rules !== 1 || value[id].insert !== "allowed")
      errors.push(id);
  for (const id of ["opaque", "redirect", "denied", "mime", "same-mime"])
    if (
      !value?.[id]?.sheet ||
      value[id].rules !== "SecurityError" ||
      value[id].insert !== "SecurityError"
    )
      errors.push(id);

  if (!value?.imported?.sheet || value.imported.rules !== "SecurityError")
    errors.push("import origin");
  const expectedEvents = [
    ["cors", "load", true],
    ["denied", "error", true],
    ["include", "load", true],
    ["mime", "error", true],
    ["opaque", "load", true],
    ["redirect", "load", true],
    ["same", "load", true],
    ["same-mime", "error", true],
  ];
  if (JSON.stringify(value?.events) !== JSON.stringify(expectedEvents))
    errors.push("parser resource events");
  for (const key of ["opaqueApplies", "corsApplies", "importApplies"])
    if (value?.[key] !== true) errors.push(key);
  return errors;
}
export async function captureStylesheetOrigins({
  binary,
  out,
  chromeOnly = false,
  nativeOnly = false,
}) {
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
  let sourceOrigin;
  const target = createServer((request, response) => {
    const path = new URL(request.url, "http://fixture.test").pathname;
    requests.push({
      mode,
      path,
      origin: request.headers.origin || null,
      cookie: !!request.headers.cookie,
    });
    response.setHeader("Content-Type", path === "/mime.css" ? "text/plain" : "text/css");
    if (path === "/mime.css") response.setHeader("X-Content-Type-Options", "nosniff");
    if (path === "/cors.css") response.setHeader("Access-Control-Allow-Origin", "*");
    if (path === "/include.css") {
      response.setHeader("Access-Control-Allow-Origin", sourceOrigin);
      response.setHeader("Access-Control-Allow-Credentials", "true");
    }
    response.end(
      path === "/opaque.css"
        ? "#opaque-target {opacity:0.25}"
        : path === "/cors.css"
          ? "#cors-target {opacity:0.5}"
          : path === "/import.css"
            ? "#import-target {opacity:0.75}"
            : "sentinel {}",
    );
  });
  const server = createServer((request, response) => {
    const path = new URL(request.url, "http://fixture.test").pathname;
    requests.push({ mode, path });
    if (path === "/redirect.css") {
      response.writeHead(302, {
        Location: `http://127.0.0.1:${target.address().port}/redirected.css`,
      });
      response.end();
      return;
    }
    response.setHeader(
      "Content-Type",
      path === "/" ? "text/html" : path === "/same-mime.css" ? "text/plain" : "text/css",
    );
    if (path === "/same-mime.css") response.setHeader("X-Content-Type-Options", "nosniff");
    response.setHeader("Set-Cookie", "style-cookie=present; Path=/; SameSite=Lax");
    response.end(
      path === "/"
        ? html.replaceAll("TARGET", `http://127.0.0.1:${target.address().port}`)
        : "sentinel {}",
    );
  });
  try {
    await new Promise((done, fail) => {
      target.once("error", fail);
      target.listen(0, "127.0.0.1", done);
    });
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
    sourceOrigin = `http://127.0.0.1:${server.address().port}`;
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
    for (const name of chromeOnly ? ["chrome"] : nativeOnly ? ["native"] : ["chrome", "native"]) {
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

      return value;
    });
    const comparison = {
      schemaVersion: 1,
      build,
      binaryHash: frozen ? hash(readFileSync(frozen)) : null,
      fixtureHash: hash(html),
      chromeVersion: nativeOnly ? null : traces[0].browserVersion,
      chrome: nativeOnly ? null : values[0],
      native: nativeOnly ? values[0] : values[1],
      requests,
      errors: values.map(stylesheetErrors),
      differences: values.length === 2 ? differences(values[0], values[1]) : [],
    };
    writeFileSync(join(out, "comparison.json"), JSON.stringify(comparison, null, 2));
    return comparison;
  } finally {
    target.closeAllConnections();
    await new Promise((done) => target.close(done));
    server.closeAllConnections();
    await new Promise((done) => server.close(done));
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const flag = (name, fallback) => {
    const index = process.argv.indexOf(name);
    return index < 0 ? fallback : process.argv[index + 1];
  };
  const out = resolve(flag("--out", join(here, "artifacts", `stylesheet-origins-${Date.now()}`)));
  const result = await captureStylesheetOrigins({
    binary: resolve(flag("--binary", "rust/target/debug/examples/detection_trace")),
    out,
    chromeOnly: process.argv.includes("--chrome-only"),
    nativeOnly: process.argv.includes("--native-only"),
  });
  console.log(JSON.stringify({ out, errors: result.errors, differences: result.differences }));
  if (result.errors.some((errors) => errors.length)) process.exitCode = 1;
}
