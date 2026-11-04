// Record the @font-face / CSS Font Loading control (turbo-surf#130) in an independently
// installed Chrome: font-face-probe.js on a plain and a CSP `font-src 'self'` page, and
// the embedded-font Range probe over pages that load IBM Plex Mono with @font-face.
// node harness/detection/font-face-chrome-control.mjs --runner $PWD/harness/detection/chrome-cdp-trace.mjs \
//   --chrome <chrome> --out harness/detection/fixtures/font-face-chrome156.json
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = join(here, "fixtures");
const cases = JSON.parse(readFileSync(join(fixtures, "font-face-cases.json"), "utf8"));
const { values } = parseArgs({
  options: { runner: { type: "string" }, out: { type: "string" }, chrome: { type: "string" } },
});
if (!values.runner || !values.out) throw Error("Pass --runner and --out");

const fontFiles = {
  "/fonts/plex.ttf": ["IBMPlexMono-Regular.ttf", "font/ttf"],
  "/fonts/plex.woff": ["IBMPlexMono-Regular.woff", "font/woff"],
  "/fonts/plex.woff2": ["IBMPlexMono-Regular.woff2", "font/woff2"],
};
const serveFont = (request, response, cors) => {
  const path = request.url.split("?")[0];
  const file = fontFiles[path];
  if (cors) response.setHeader("Access-Control-Allow-Origin", "*");
  if (!file) {
    response.statusCode = 404;
    response.end("missing");
    return;
  }
  response.setHeader("Content-Type", file[1]);
  response.end(readFileSync(join(fixtures, "fonts/ibm-plex-mono", file[0])));
};
const cross = createServer((request, response) =>
  serveFont(request, response, request.url.includes("cors=1")),
);
await new Promise((done) => cross.listen(0, "127.0.0.1", done));
const crossOrigin = `http://127.0.0.1:${cross.address().port}`;
const server = createServer((request, response) => {
  if (request.url.startsWith("/fonts/")) return serveFont(request, response, false);
  const item = cases.pages.find((page) => `/${page.name}` === request.url);
  if (!item) {
    response.statusCode = 404;
    response.end();
    return;
  }
  for (const [name, value] of item.headers || []) response.setHeader(name, value);
  response.setHeader("Content-Type", "text/html");
  response.end(item.html.replace("{cross}", crossOrigin));
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const directory = mkdtempSync(join(tmpdir(), "turbo-font-face-oracle-"));
const hash = (name) =>
  createHash("sha256")
    .update(readFileSync(join(here, name)))
    .digest("hex");
let complete = false;
try {
  const result = {};
  let trace;
  for (const page of cases.pages) {
    const config = join(directory, `${page.name}.json`);
    writeFileSync(
      config,
      JSON.stringify({
        url: `http://127.0.0.1:${server.address().port}/${page.name}`,
        viewport: { width: 1280, height: 800 },
        deviceScaleFactor: 2,
        waitMs: 25,
        disableExtensions: true,
        foregroundFrames: true,
        steps: [],
        probes: page.probes.map((name) => join(here, name)),
        ...(values.chrome ? { chromePath: resolve(values.chrome) } : {}),
      }),
    );
    const output = join(directory, page.name);
    const child = spawn(process.execPath, [resolve(values.runner), config, output], {
      stdio: "inherit",
    });
    const status = await new Promise((done) => child.once("exit", done));
    if (status !== 0) throw Error(`Chrome capture exited ${status}`);
    trace = JSON.parse(readFileSync(join(output, "trace.json"), "utf8"));
    if (trace.probes.some((probe) => probe.error || !probe.result))
      throw Error(`Chrome control failed for ${page.name}; inspect ${output}`);
    result[page.name] = trace.probes.at(-1).result;
  }
  const probes = [...new Set(cases.pages.flatMap((page) => page.probes))];
  const oracle = {
    browser: trace.browserVersion,
    v8: trace.browserV8Version,
    recordedAt: new Date().toISOString(),
    fixtures: Object.fromEntries(
      [...probes, "fixtures/font-face-cases.json"].map((name) => [name, hash(name)]),
    ),
    result,
  };
  writeFileSync(resolve(values.out), `${JSON.stringify(oracle, null, 2)}\n`);
  console.log(JSON.stringify({ browser: oracle.browser, out: resolve(values.out) }));
  complete = true;
} finally {
  await new Promise((done) => server.close(done));
  await new Promise((done) => cross.close(done));
  if (complete) rmSync(directory, { recursive: true, force: true });
}
