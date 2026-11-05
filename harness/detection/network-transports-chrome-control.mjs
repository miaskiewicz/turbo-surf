// Capture network-transports-probe.js (WebSocket, WebSocketStream, EventSource,
// WebTransport) and network-transports-csp-probe.js (connect-src) in an independently
// installed Chrome over a raw CDP pipe, against network-transports-server.mjs on 127.0.0.1.
// Each probe gets a fresh server (fresh request logs) and a fresh target. Nothing leaves
// loopback.
// Usage: node harness/detection/network-transports-chrome-control.mjs --out FIXTURE.json [--chrome PATH]
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { startTransportServer } from "./network-transports-server.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const { values } = parseArgs({ options: { out: { type: "string" }, chrome: { type: "string" } } });
if (!values.out) throw Error("Pass --out FIXTURE.json [--chrome /path/to/Chrome]");
// The native test serves these exact documents.
const pages = {
  "/": "<!doctype html><html><head></head><body></body></html>",
  "/csp": `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="connect-src 'self'"></head><body></body></html>`,
};
const probes = [
  ["network-transports-probe.js", "/"],
  ["network-transports-csp-probe.js", "/csp"],
];
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const executable =
  values.chrome || "/Applications/User Apps/Google Chrome.app/Contents/MacOS/Google Chrome";
const profile = mkdtempSync(join(tmpdir(), "turbo-network-transports-"));
const flags = [
  "--remote-debugging-pipe",
  "--user-data-dir=<fresh-profile>",
  "--no-first-run",
  "--no-default-browser-check",
  "--use-mock-keychain",
  "--password-store=basic",
  "--disable-extensions",
  "--disable-component-extensions-with-background-pages",
  "about:blank",
];
// One server for every probe, started before Chrome: a listener created after the spawn
// left the first probe's renderer unresponsive on macOS.
const transport = await startTransportServer(pages);
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

async function capture(name, path) {
  transport.reset();
  const { targetId } = await call("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await call("Target.attachToTarget", { targetId, flatten: true });
  await call("Page.enable", {}, sessionId);
  await call("Page.navigate", { url: `http://127.0.0.1:${transport.port}${path}` }, sessionId);
  await pause(500);
  const probe = readFileSync(join(here, name), "utf8");
  const evaluated = await call(
    "Runtime.evaluate",
    { expression: probe, returnByValue: true, awaitPromise: true },
    sessionId,
  );
  await call("Target.closeTarget", { targetId });
  return evaluated;
}

const errors = [],
  result = {};
let version;
try {
  version = await call("Browser.getVersion");
  for (const [name, path] of probes) {
    const evaluated = await capture(name, path);
    if (evaluated.exceptionDetails) errors.push({ probe: name, error: evaluated.exceptionDetails });
    result[name] = evaluated.result.value;
  }
} finally {
  const exited = new Promise((done) => chrome.once("exit", done));
  chrome.kill();
  await exited;
  await transport.stop();
  rmSync(profile, { recursive: true, force: true, maxRetries: 10 });
}
const oracle = {
  browser: version.product,
  v8: version.jsVersion,
  userAgent: version.userAgent,
  collectorSha256: sha(readFileSync(fileURLToPath(import.meta.url))),
  serverSha256: sha(readFileSync(join(here, "network-transports-server.mjs"))),
  fixtures: Object.fromEntries(probes.map(([name]) => [name, sha(readFileSync(join(here, name)))])),
  flags,
  errors,
  result,
};
writeFileSync(resolve(values.out), `${JSON.stringify(oracle, null, 2)}\n`);
console.log(
  JSON.stringify({ browser: oracle.browser, errors: errors.length, out: resolve(values.out) }),
);
