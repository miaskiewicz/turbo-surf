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
const wire = [];
const widget = readFileSync(resolve(dirname(probePath), "storage-clear-widget.js"), "utf8");
const handler = (request, response) => {
  const url = new URL(request.url, "http://" + request.headers.host);
  wire.push({
    host: request.headers.host.replace(/:[0-9]+$/, ":PORT"),
    path: url.pathname,
    case: url.searchParams.get("case"),
    cookie: request.headers.cookie ?? null,
  });
  response.setHeader("Content-Type", "text/html");
  if (url.pathname === "/resource-clear") {
    response.setHeader("Clear-Site-Data", '"storage"');
    const kind = url.searchParams.get("kind");
    response.setHeader(
      "Content-Type",
      kind === "style" ? "text/css" : kind === "image" ? "image/svg+xml" : "text/javascript",
    );
    response.end(
      kind === "style"
        ? "body { color: red; }"
        : kind === "image"
          ? '<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"></svg>'
          : "globalThis.__resourceClear = true;",
    );
    return;
  }
  if (url.pathname === "/clear") {
    response.setHeader("Clear-Site-Data", url.searchParams.get("header"));
    response.setHeader("Cache-Control", "max-age=60");
    if (url.searchParams.get("cors") === "allow") {
      response.setHeader("Access-Control-Allow-Origin", request.headers.origin);
      response.setHeader("Access-Control-Allow-Credentials", "true");
    }
    if (url.searchParams.has("corp"))
      response.setHeader("Cross-Origin-Resource-Policy", url.searchParams.get("corp"));
    response.end("clear");
    return;
  }
  if (url.pathname === "/widget") {
    if (url.searchParams.has("clearstorage")) response.setHeader("Clear-Site-Data", '"storage"');
    response.end(`<!doctype html><body><script>${widget}</script>`);
    return;
  }
  response.end("<!doctype html><html><head></head><body></body></html>");
};
const server = createServer(handler),
  secondary = createServer(handler);

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
  await new Promise((done, fail) => {
    secondary.once("error", fail);
    secondary.listen(0, "127.0.0.1", done);
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
  await evaluate("globalThis.__storageClearSecondPort=" + secondary.address().port);
  const result = await evaluate(probe);
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    widgetSha256: createHash("sha256").update(widget).digest("hex"),
    probeSha256: createHash("sha256").update(probe).digest("hex"),
    collectorSha256: createHash("sha256")
      .update(readFileSync(new URL(import.meta.url)))
      .digest("hex"),
    flags: flags.map((flag) =>
      flag.startsWith("--user-data-dir=") ? "--user-data-dir=<fresh-profile>" : flag,
    ),
    errors,
    wire,
    result,
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
  secondary.closeAllConnections();
  await new Promise((done) => secondary.close(done));
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  rmSync(profile, { recursive: true, force: true });
}
