// Capture the window-interface probes (History/Location, VisualViewport/Screen/ScreenOrientation,
// interface descriptor shapes) in an independently installed Chrome over a raw CDP pipe, with the
// same 1280x800 viewport, device scale 2 and 1920x1080 screen the native PageSession uses.
// Each probe runs in a fresh target on a localhost page. No call timeouts: a loaded machine
// only slows the capture down.
// Usage: node harness/detection/window-interfaces-chrome-control.mjs --out FIXTURE.json [--chrome PATH]
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));
const { values } = parseArgs({ options: { out: { type: "string" }, chrome: { type: "string" } } });
if (!values.out) throw Error("Pass --out FIXTURE.json [--chrome /path/to/Chrome]");
const probeNames = [
  "history-location-probe.js",
  "viewport-orientation-probe.js",
  "interface-shapes-probe.js",
  "observers-probe.js",
  "transports-probe.js",
];
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const executable =
  values.chrome || "/Applications/User Apps/Google Chrome.app/Contents/MacOS/Google Chrome";
const profile = mkdtempSync(join(tmpdir(), "turbo-window-interfaces-"));
const flags = [
  "--headless=new",
  "--remote-debugging-pipe",
  "--user-data-dir=<fresh-profile>",
  "--no-first-run",
  "--no-default-browser-check",
  "--disable-extensions",
  "--disable-component-extensions-with-background-pages",
  "about:blank",
];
const html = "<!doctype html><html><head></head><body></body></html>";
// transports-probe.js reads these from the page's own origin (the native test serves the same).
const routes = {
  "/xhr/json": [
    200,
    "OK",
    { "Content-Type": "application/json", "X-Probe": "one", "Set-Cookie": "probe=1" },
    '{"a":1,"b":"\u00e9"}',
  ],
  "/xhr/text": [200, "OK", { "Content-Type": "text/plain; charset=utf-8" }, "h\u00e9llo"],
  "/xhr/missing": [404, "Not Found", { "Content-Type": "text/plain" }, "missing"],
};
const server = createServer((request, response) => {
  const route = routes[request.url];
  if (route) {
    response.writeHead(route[0], route[1], {
      ...route[2],
      "Content-Length": Buffer.byteLength(route[3]),
    });
    response.end(route[3]);
    return;
  }
  response.setHeader("Content-Type", "text/html");
  response.end(html);
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const chrome = spawn(
  executable,
  flags.map((flag) =>
    flag === "--user-data-dir=<fresh-profile>" ? `--user-data-dir=${profile}` : flag,
  ),
  { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
);
const pending = new Map();
let nextId = 1,
  buffer = Buffer.alloc(0);
chrome.stdio[4].on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (let end = buffer.indexOf(0); end >= 0; end = buffer.indexOf(0)) {
    const message = JSON.parse(buffer.subarray(0, end).toString("utf8"));
    buffer = buffer.subarray(end + 1);
    const waiter = message.id && pending.get(message.id);
    if (!waiter) continue;
    pending.delete(message.id);
    if (message.error) waiter.reject(Error(JSON.stringify(message.error)));
    else waiter.resolve(message.result);
  }
});
const call = (method, params = {}, sessionId) =>
  new Promise((done, fail) => {
    const id = nextId++;
    pending.set(id, { resolve: done, reject: fail });
    chrome.stdio[3].write(
      `${JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })}\0`,
    );
  });
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const errors = [],
  result = {};
let version;
try {
  version = await call("Browser.getVersion");
  for (const name of probeNames) {
    const probe = readFileSync(join(here, name), "utf8");
    const { targetId } = await call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await call("Target.attachToTarget", { targetId, flatten: true });
    await call("Page.enable", {}, sessionId);
    await call(
      "Emulation.setDeviceMetricsOverride",
      {
        width: 1280,
        height: 800,
        deviceScaleFactor: 2,
        mobile: false,
        screenWidth: 1920,
        screenHeight: 1080,
      },
      sessionId,
    );
    await call("Page.navigate", { url: `http://127.0.0.1:${server.address().port}/` }, sessionId);
    for (let tries = 0; tries < 2000; tries++) {
      const state = await call(
        "Runtime.evaluate",
        { expression: "document.readyState", returnByValue: true },
        sessionId,
      );
      if (state.result.value === "complete") break;
      await pause(10);
    }
    const evaluated = await call(
      "Runtime.evaluate",
      { expression: probe, returnByValue: true, awaitPromise: true },
      sessionId,
    );
    if (evaluated.exceptionDetails) errors.push({ probe: name, error: evaluated.exceptionDetails });
    result[name] = evaluated.result.value;
    await call("Target.closeTarget", { targetId });
  }
} finally {
  const exited = new Promise((done) => chrome.once("exit", done));
  chrome.kill();
  await exited;
  await new Promise((done) => server.close(done));
  rmSync(profile, { recursive: true, force: true, maxRetries: 10 });
}
const oracle = {
  browser: version.product,
  v8: version.jsVersion,
  collectorSha256: sha(readFileSync(fileURLToPath(import.meta.url))),
  fixtures: Object.fromEntries(
    probeNames.map((name) => [name, sha(readFileSync(join(here, name)))]),
  ),
  flags,
  errors,
  result,
};
writeFileSync(resolve(values.out), `${JSON.stringify(oracle, null, 2)}\n`);
console.log(
  JSON.stringify({ browser: oracle.browser, errors: errors.length, out: resolve(values.out) }),
);
