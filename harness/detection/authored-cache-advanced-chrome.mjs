// Collect a source-pinned authored DOM control in a fresh, normal installed Chrome.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";

const [probeArgument, outputArgument] = process.argv.slice(2);
if (!probeArgument || !outputArgument)
  throw new Error("Usage: node authored-dom-chrome.mjs probe.js output.json");
const probePath = resolve(probeArgument),
  output = resolve(outputArgument);
const probe = readFileSync(probePath, "utf8"),
  profile = mkdtempSync(join(tmpdir(), "turbo-dom-oracle-"));
const counts = new Map();
const requests = [];
const nextCount = (key) => {
  const count = (counts.get(key) || 0) + 1;
  counts.set(key, count);
  return count;
};
const sendHtml = (response, body) => {
  response.setHeader("Content-Type", "text/html");
  response.end(body);
};
const resourceBodies = {
  "/resource/classic": (count) => [
    "text/javascript",
    `globalThis.resourceClassic=(globalThis.resourceClassic||[]);globalThis.resourceClassic.push(${count});`,
  ],
  "/resource/module": (count) => ["text/javascript", `export const value=${count};`],
  "/resource/style": (count) => ["text/css", `body{margin-left:${count}px}`],
};
const serveResource = (request, response, target) => {
  const count = nextCount(request.url);
  response.setHeader("Cache-Control", "max-age=3600");
  const [type, body] = resourceBodies[target.pathname]?.(count) || [
    "image/gif",
    Buffer.from("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7", "base64"),
  ];
  response.setHeader("Content-Type", type);
  response.end(body);
};
const serveStats = (_request, response, target) => {
  response.setHeader("Content-Type", "application/json");
  response.setHeader("Cache-Control", "no-store");
  response.end(JSON.stringify(counts.get(target.searchParams.get("path")) || 0));
};
const serveModuleFrame = (_request, response) =>
  sendHtml(
    response,
    `<!doctype html><script type="module">import {value} from '/resource/module';top.postMessage({kind:'resource-module',key:new URL(location.href).searchParams.get('key'),value},'*')</script>`,
  );
const serveFrameDirect = (_request, response) =>
  sendHtml(
    response,
    `<!doctype html><script>(async()=>{const response=await fetch('http://127.0.0.1:${server.address().port}/cache/nested');top.postMessage({kind:'cache-ancestor',key:new URL(location.href).searchParams.get('key'),count:response.headers.get('x-count')},'*')})()</script>`,
  );
const serveFrameParent = (_request, response) =>
  sendHtml(
    response,
    `<!doctype html><iframe src="http://127.0.0.1:${server.address().port}/frame-direct?key=nested"></iframe>`,
  );
const rangeBytes = Buffer.from("abcdefghij");
// Resolve a single `bytes=start-end` / `bytes=-suffix` range against the fixed body.
const rangeBounds = (match) => {
  const start = match[1] ? Number(match[1]) : Math.max(0, rangeBytes.length - Number(match[2]));
  const end =
    match[1] && match[2]
      ? Math.min(Number(match[2]), rangeBytes.length - 1)
      : rangeBytes.length - 1;
  return [start, end];
};
const serveRangeSlice = (response, match) => {
  const [start, end] = rangeBounds(match);
  if (start > end || start >= rangeBytes.length) {
    response.writeHead(416, { "Content-Range": `bytes */${rangeBytes.length}` });
    response.end();
    return;
  }
  response.writeHead(206, { "Content-Range": `bytes ${start}-${end}/${rangeBytes.length}` });
  response.end(rangeBytes.subarray(start, end + 1));
};
const serveRange = (request, response) => {
  requests.push({
    url: request.url,
    range: request.headers.range || null,
    ifRange: request.headers["if-range"] || null,
  });
  const count = nextCount(request.url);
  response.setHeader("Cache-Control", "max-age=3600");
  response.setHeader("ETag", '"range"');
  response.setHeader("X-Count", String(count));
  response.setHeader("Content-Type", "text/plain");
  response.setHeader("Accept-Ranges", "bytes");
  const match = /^bytes=(\d*)-(\d*)$/.exec(request.headers.range || "");
  if (match) serveRangeSlice(response, match);
  else response.end(rangeBytes);
};
const serveClear = (_request, response, target) => {
  response.writeHead(200, {
    "Clear-Site-Data": target.searchParams.get("types") || '"cache"',
    "Cache-Control": "no-store",
  });
  response.end("cleared");
};
const servePage = (_request, response) =>
  sendHtml(response, "<!doctype html><body>HTTP cache control</body>");
const cacheHeaders = (request, response, target, count) => {
  response.setHeader("Content-Type", "application/json");
  response.setHeader(
    "Cache-Control",
    request.headers["x-response-control"] || target.searchParams.get("control") || "max-age=3600",
  );
  response.setHeader("ETag", '"stable"');
  response.setHeader("X-Count", String(count));
  response.setHeader("X-Merged", count === 1 ? "initial" : "validated");
  if (target.searchParams.has("vary")) response.setHeader("Vary", target.searchParams.get("vary"));
};
// Terminal cache responses short of the JSON body: a redirect or a 304 revalidation.
const cacheShortCircuit = (request, response, target) => {
  if (target.searchParams.has("redirect")) {
    response.writeHead(Number(target.searchParams.get("redirect")) || 301, {
      Location: "/cache/redirect-target",
    });
    response.end();
    return true;
  }
  if (request.headers["if-none-match"] !== '"stable"') return false;
  response.writeHead(304);
  response.end();
  return true;
};
const serveCache = (request, response, target) => {
  const count = nextCount(request.url);
  cacheHeaders(request, response, target, count);
  if (cacheShortCircuit(request, response, target)) return;
  if (target.searchParams.has("status"))
    response.statusCode = Number(target.searchParams.get("status"));
  response.end(
    JSON.stringify({
      count,
      conditional: request.headers["if-none-match"] || null,
      cacheControl: request.headers["cache-control"] || null,
      pragma: request.headers.pragma || null,
      vary: request.headers["x-flavor"] || null,
      referer: !!request.headers.referer,
    }),
  );
};
const exact = (path) => (pathname) => pathname === path;
const prefix = (path) => (pathname) => pathname.startsWith(path);
const routes = [
  [prefix("/resource/"), serveResource],
  [exact("/module-frame"), serveModuleFrame],
  [exact("/stats"), serveStats],
  [exact("/frame-direct"), serveFrameDirect],
  [exact("/frame-parent"), serveFrameParent],
  [prefix("/range/"), serveRange],
  [exact("/clear"), serveClear],
  [(pathname) => !pathname.startsWith("/cache/"), servePage],
];
const server = createServer((request, response) => {
  response.setHeader("Connection", "close");
  response.setHeader("Date", new Date(Date.now() - 60000).toUTCString());
  const target = new URL(request.url, "http://localhost");
  const route = routes.find(([matches]) => matches(target.pathname));
  (route ? route[1] : serveCache)(request, response, target);
});
let chrome, socket, connection;
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const kill = (signal) => {
  if (!chrome?.pid || chrome.exitCode !== null || chrome.signalCode !== null) return;
  try {
    process.kill(-chrome.pid, signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
};
try {
  await new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  const url = `http://127.0.0.1:${server.address().port}/`;
  const flags = [
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    ...chromeExtensionArgs({ url, disableExtensions: true }),
    "about:blank",
  ];
  chrome = spawn(
    process.env.TURBO_SURF_CHROME_PATH ||
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    flags,
    { detached: true, stdio: ["ignore", "ignore", "pipe"] },
  );
  let stderr = "",
    spawnError;
  chrome.once("error", (error) => {
    spawnError = error;
  });
  chrome.stderr.on("data", (chunk) => {
    if (stderr.length < 1048576) stderr += String(chunk);
  });
  const deadline = Date.now() + 30000;
  let endpoint;
  while (!(endpoint = /DevTools listening on (ws:\/\/[^\s]+)/.exec(stderr)?.[1])) {
    if (spawnError) throw spawnError;
    if (chrome.exitCode !== null || Date.now() > deadline) throw new Error("Chrome startup failed");
    await pause(50);
  }
  socket = new WebSocket(endpoint);
  await new Promise((done, fail) => {
    socket.onopen = done;
    socket.onerror = fail;
  });
  const errors = [];
  connection = cdpConnection(
    socket,
    (event) => {
      if (event.method === "Runtime.exceptionThrown") errors.push(event.params.exceptionDetails);
    },
    30000,
  );
  const version = await connection.call("Browser.getVersion");
  const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await connection.call("Target.attachToTarget", { targetId, flatten: true });
  await connection.call("Runtime.enable", {}, sessionId);
  await connection.call("Page.enable", {}, sessionId);
  await connection.call("Page.navigate", { url }, sessionId);
  const evaluate = async (expression) => {
    const result = await connection.call(
      "Runtime.evaluate",
      { expression, returnByValue: true, awaitPromise: true },
      sessionId,
    );
    if (result.exceptionDetails)
      throw new Error(
        result.exceptionDetails.exception?.description || result.exceptionDetails.text,
      );
    return result.result.value;
  };
  const readyDeadline = Date.now() + 10000;
  while ((await evaluate("document.readyState")) !== "complete") {
    if (Date.now() > readyDeadline) throw new Error("Authored page did not finish loading");
    await pause(25);
  }
  const result = await evaluate(probe);
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    probeSha256: createHash("sha256").update(probe).digest("hex"),
    collectorSha256: createHash("sha256")
      .update(readFileSync(new URL(import.meta.url)))
      .digest("hex"),
    flags: flags.map((flag) =>
      flag.startsWith("--user-data-dir=") ? "--user-data-dir=<fresh-profile>" : flag,
    ),
    errors,
    result,
    requests,
  };
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(control, null, 2)}\n`);
  if (errors.length)
    throw new Error(`Authored Chrome control reported ${errors.length} exception(s)`);
} finally {
  connection?.close();
  socket?.close();
  kill("SIGTERM");
  if (chrome && chrome.exitCode === null && chrome.signalCode === null)
    await Promise.race([new Promise((done) => chrome.once("exit", done)), pause(1000)]);
  if (chrome?.exitCode === null && chrome.signalCode === null) kill("SIGKILL");
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  rmSync(profile, { recursive: true, force: true });
}
