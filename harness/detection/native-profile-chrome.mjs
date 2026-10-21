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
  throw new Error("Usage: node native-profile-chrome.mjs probe.js output.json");
const probePath = resolve(probeArgument),
  output = resolve(outputArgument);
const probe = readFileSync(probePath, "utf8"),
  profile = mkdtempSync(join(tmpdir(), "turbo-dom-oracle-"));
let cachedRequests = 0;
const server = createServer((request, response) => {
  response.setHeader("Connection", "close");
  if (request.url === "/cached") {
    cachedRequests++;
    response.setHeader("Cache-Control", "max-age=600");
    response.end("cached");
    return;
  }
  if (request.url === "/clear") response.setHeader("Clear-Site-Data", '"storage"');
  response.setHeader("Content-Type", "text/html");
  response.end("<!doctype html><body>Profile control</body>");
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
  const evaluate = async (sessionId, expression) => {
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
  const navigate = async (sessionId, targetUrl) => {
    await connection.call("Page.navigate", { url: targetUrl }, sessionId);
    const limit = Date.now() + 10000;
    while ((await evaluate(sessionId, "document.readyState")) !== "complete") {
      if (Date.now() > limit) throw new Error("Profile page did not load");
      await pause(25);
    }
  };
  const tab = async () => {
    const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await connection.call("Target.attachToTarget", {
      targetId,
      flatten: true,
    });
    await connection.call("Runtime.enable", {}, sessionId);
    await connection.call("Page.enable", {}, sessionId);
    await navigate(sessionId, url);
    return { targetId, sessionId };
  };
  const run = (tab, operation) =>
    evaluate(tab.sessionId, `(${probe.trim().replace(/;$/, "")})(${JSON.stringify(operation)})`);
  const a = await tab(),
    b = await tab();
  const result = {};
  result.first = await run(a, { local: "first", session: "alpha", cache: true });
  result.otherBefore = await run(b, { cache: true });
  result.otherWrite = await run(b, { local: "second", session: "beta" });
  result.firstAfter = await run(a, {});
  await navigate(a.sessionId, url + "next");
  result.navigation = await run(a, { cache: true });
  await connection.call("Target.closeTarget", { targetId: a.targetId });
  const c = await tab();
  result.freshTab = await run(c, { cache: true });
  await run(b, { clear: true });
  result.clearedPeer = await run(c, {});
  result.clearedSource = await run(b, {});
  result.cachedRequests = cachedRequests;
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
