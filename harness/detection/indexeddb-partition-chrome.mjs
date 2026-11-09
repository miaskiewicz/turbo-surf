// Collect the IndexedDB storage-partition control (#148) in a fresh, normal installed
// Chrome: indexeddb-partition-probe.js on a localhost and then a 127.0.0.1 top-level
// page, each embedding indexeddb-partition-widget.js at 127.0.0.1 directly and through
// a localhost frame. One profile serves both runs, as one browser would.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";

const [outputArgument] = process.argv.slice(2);
if (!outputArgument) throw new Error("Usage: node indexeddb-partition-chrome.mjs output.json");
const output = resolve(outputArgument);
const source = (name) => readFileSync(new URL(name, import.meta.url), "utf8");
const probe = source("./indexeddb-partition-probe.js"),
  widget = source("./indexeddb-partition-widget.js"),
  profile = mkdtempSync(join(tmpdir(), "turbo-dom-oracle-"));
const server = createServer((request, response) => {
  response.setHeader("Connection", "close");
  response.setHeader("Content-Type", "text/html");
  const query = request.url.split("?")[1] ?? "";
  if (request.url.startsWith("/idb-middle?")) {
    const port = request.headers.host.split(":").at(-1);
    response.end(
      `<iframe src="http://127.0.0.1:${port}/idb-widget?${query}"></iframe><script>addEventListener('message',event=>parent.postMessage(event.data,'*'))</script>`,
    );
    return;
  }
  if (request.url.startsWith("/idb-widget?")) {
    response.end(`<!doctype html><script>${widget}</script>`);
    return;
  }
  response.end("<!doctype html><body>IndexedDB partition control");
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
    "--use-mock-keychain",
    "--password-store=basic",
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
  const result = [];
  for (const host of ["localhost", "127.0.0.1"]) {
    await connection.call(
      "Page.navigate",
      { url: `http://${host}:${server.address().port}/` },
      sessionId,
    );
    const loaded = Date.now() + 10000;
    while ((await evaluate("document.readyState")) !== "complete") {
      if (Date.now() > loaded) throw new Error("Partition page did not load");
      await pause(25);
    }
    result.push(await evaluate(probe));
  }
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
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  rmSync(profile, { recursive: true, force: true });
}
