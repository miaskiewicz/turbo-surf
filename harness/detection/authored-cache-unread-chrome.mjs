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
const server = createServer((request, response) => {
  response.setHeader("Connection", "close");
  response.setHeader("Date", new Date(Date.now() - 60000).toUTCString());
  const target = new URL(request.url, "http://localhost");
  if (target.pathname.startsWith("/unread/")) {
    const bytes = Buffer.alloc(Number(target.searchParams.get("bytes")) || 16384, 97);
    response.writeHead(200, {
      ...(target.searchParams.get("control") === "absent"
        ? {}
        : { "Cache-Control": target.searchParams.get("control") || "max-age=3600" }),
      "Content-Type": "text/plain",
      "Content-Length": bytes.length,
    });
    response.write(bytes.subarray(0, 1));
    response.on("close", () => {
      counts.set(request.url + ":canceled", !response.writableFinished);
    });
    setTimeout(() => response.end(bytes.subarray(1)), 200);
    return;
  }
  if (target.pathname === "/unread-hold") {
    response.writeHead(200, { "Content-Length": "8", "Content-Type": "application/octet-stream" });
    response.flushHeaders();
    response.on("close", () => counts.set("/unread-hold:closed", true));
    return;
  }
  if (target.pathname === "/unread-frame") {
    response.setHeader("Content-Type", "text/html");
    response.end(
      `<!doctype html><script>(async()=>{await fetch('/unread/retire');top.postMessage({kind:'unread-frame'},'*')})()</script>`,
    );
    return;
  }
  if (target.pathname.startsWith("/resource/")) {
    const count = (counts.get(request.url) || 0) + 1;
    counts.set(request.url, count);
    response.setHeader("Cache-Control", "max-age=3600");
    if (target.pathname === "/resource/classic") {
      response.setHeader("Content-Type", "text/javascript");
      response.end(
        `globalThis.resourceClassic=(globalThis.resourceClassic||[]);globalThis.resourceClassic.push(${count});`,
      );
      return;
    }
    if (target.pathname === "/resource/module") {
      response.setHeader("Content-Type", "text/javascript");
      response.end(`export const value=${count};`);
      return;
    }
    if (target.pathname === "/resource/style") {
      response.setHeader("Content-Type", "text/css");
      response.end(`body{margin-left:${count}px}`);
      return;
    }
    response.setHeader("Content-Type", "image/gif");
    response.end(Buffer.from("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7", "base64"));
    return;
  }
  if (target.pathname === "/module-frame") {
    response.setHeader("Content-Type", "text/html");
    response.end(
      `<!doctype html><script type="module">import {value} from '/resource/module';top.postMessage({kind:'resource-module',key:new URL(location.href).searchParams.get('key'),value},'*')</script>`,
    );
    return;
  }
  if (target.pathname === "/stats") {
    response.setHeader("Content-Type", "application/json");
    response.setHeader("Cache-Control", "no-store");
    response.end(JSON.stringify(counts.get(target.searchParams.get("path")) || 0));
    return;
  }
  if (target.pathname === "/frame-direct") {
    response.setHeader("Content-Type", "text/html");
    response.end(
      `<!doctype html><script>(async()=>{const response=await fetch('http://127.0.0.1:${server.address().port}/cache/nested');top.postMessage({kind:'cache-ancestor',key:new URL(location.href).searchParams.get('key'),count:response.headers.get('x-count')},'*')})()</script>`,
    );
    return;
  }
  if (target.pathname === "/frame-parent") {
    response.setHeader("Content-Type", "text/html");
    response.end(
      `<!doctype html><iframe src="http://127.0.0.1:${server.address().port}/frame-direct?key=nested"></iframe>`,
    );
    return;
  }
  if (target.pathname.startsWith("/range/")) {
    requests.push({
      url: request.url,
      range: request.headers.range || null,
      ifRange: request.headers["if-range"] || null,
    });
    const count = (counts.get(request.url) || 0) + 1;
    counts.set(request.url, count);
    response.setHeader("Cache-Control", "max-age=3600");
    response.setHeader("ETag", '"range"');
    response.setHeader("X-Count", String(count));
    response.setHeader("Content-Type", "text/plain");
    response.setHeader("Accept-Ranges", "bytes");
    const bytes = Buffer.from("abcdefghij");
    const range = /^bytes=(\d*)-(\d*)$/.exec(request.headers.range || "");
    if (range) {
      const start = range[1] ? Number(range[1]) : Math.max(0, bytes.length - Number(range[2]));
      const end =
        range[1] && range[2] ? Math.min(Number(range[2]), bytes.length - 1) : bytes.length - 1;
      if (start > end || start >= bytes.length) {
        response.writeHead(416, { "Content-Range": `bytes */${bytes.length}` });
        response.end();
        return;
      }
      response.writeHead(206, { "Content-Range": `bytes ${start}-${end}/${bytes.length}` });
      response.end(bytes.subarray(start, end + 1));
      return;
    }
    response.end(bytes);
    return;
  }
  if (target.pathname === "/clear") {
    response.writeHead(200, {
      "Clear-Site-Data": target.searchParams.get("types") || '"cache"',
      "Cache-Control": "no-store",
    });
    response.end("cleared");
    return;
  }
  if (!target.pathname.startsWith("/cache/")) {
    response.setHeader("Content-Type", "text/html");
    response.end("<!doctype html><body>HTTP cache control</body>");
    return;
  }
  const key = request.url;
  const count = (counts.get(key) || 0) + 1;
  counts.set(key, count);
  response.setHeader("Content-Type", "application/json");
  response.setHeader(
    "Cache-Control",
    request.headers["x-response-control"] || target.searchParams.get("control") || "max-age=3600",
  );
  response.setHeader("ETag", '"stable"');
  response.setHeader("X-Count", String(count));
  response.setHeader("X-Merged", count === 1 ? "initial" : "validated");
  if (target.searchParams.has("vary")) response.setHeader("Vary", target.searchParams.get("vary"));
  if (target.searchParams.has("redirect")) {
    response.writeHead(Number(target.searchParams.get("redirect")) || 301, {
      Location: "/cache/redirect-target",
    });
    response.end();
    return;
  }
  if (request.headers["if-none-match"] === '"stable"') {
    response.writeHead(304);
    response.end();
    return;
  }
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
  await evaluate(probe);
  await evaluate("(async()=>{globalThis.unreadHeld=await fetch('/unread/held')})()");
  await pause(500);
  const held = await evaluate("globalThis.unreadControl.check('/unread/held')");
  await evaluate("(async()=>{await fetch('/unread/gc')})()");
  await connection.call("HeapProfiler.collectGarbage", {}, sessionId);
  await pause(500);
  const gc = await evaluate("globalThis.unreadControl.check('/unread/gc')");
  await evaluate(
    "(async()=>{const response=await fetch('/unread/cancel');await response.body.cancel()})()",
  );
  await pause(500);
  const cancel = await evaluate("globalThis.unreadControl.check('/unread/cancel')");
  await evaluate(
    "(async()=>{const controller=new AbortController();await fetch('/unread/abort',{signal:controller.signal});controller.abort()})()",
  );
  await pause(500);
  const abort = await evaluate("globalThis.unreadControl.check('/unread/abort')");
  await evaluate("globalThis.unreadControl.retire()");
  await pause(500);
  const retire = await evaluate("globalThis.unreadControl.check('/unread/retire')");
  const result = { held, gc, cancel, abort, retire };
  for (const bytes of [65536, 262144, 1048576, 2097152]) {
    const heldUrl = "/unread/size-held?bytes=" + bytes;
    await evaluate(`(async()=>{globalThis.unreadHeld=await fetch(${JSON.stringify(heldUrl)})})()`);
    await pause(500);
    result["held-" + bytes] = await evaluate(`unreadControl.check(${JSON.stringify(heldUrl)})`);
    const gcUrl = "/unread/size-gc?bytes=" + bytes;
    await evaluate(`(async()=>{await fetch(${JSON.stringify(gcUrl)})})()`);
    await connection.call("HeapProfiler.collectGarbage", {}, sessionId);
    await pause(500);
    result["gc-" + bytes] = await evaluate(`unreadControl.check(${JSON.stringify(gcUrl)})`);
  }
  for (const control of ["absent", "no-cache", "no-store"]) {
    const controlUrl = "/unread/control?control=" + control;
    await evaluate(`(async()=>{await fetch(${JSON.stringify(controlUrl)})})()`);
    await connection.call("HeapProfiler.collectGarbage", {}, sessionId);
    await pause(500);
    result["gc-" + control] = await evaluate(`unreadControl.check(${JSON.stringify(controlUrl)})`);
  }
  await evaluate("(async()=>{await fetch('/unread-hold')})()");
  await connection.call("HeapProfiler.collectGarbage", {}, sessionId);
  await pause(500);
  result.holdClosed = await evaluate(
    "(async()=>await(await fetch('/stats?path=%2Funread-hold%3Aclosed')).json())()",
  );
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
