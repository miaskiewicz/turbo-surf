/* eslint-disable no-await-in-loop -- CDP commands and navigation readiness checks are sequential by design. */
// Collect the document response-hook control (lastModified, document.domain, agent
// clustering, fullscreen/picture-in-picture policy) in a fresh installed Chrome.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";

const [casesArgument, probeArgument, outputArgument] = process.argv.slice(2);
if (!casesArgument || !probeArgument || !outputArgument)
  throw new Error("Usage: node document-hooks-chrome.mjs cases.json probe.js output.json");
const probePath = resolve(probeArgument),
  output = resolve(outputArgument);
const probe = readFileSync(probePath, "utf8"),
  profile = mkdtempSync(join(tmpdir(), "turbo-document-hooks-"));
const casesPath = resolve(casesArgument),
  casesText = readFileSync(casesPath, "utf8"),
  cases = JSON.parse(casesText);
const report = readFileSync(new URL("./document-hooks-report.js", import.meta.url), "utf8");
const headless = process.env.TURBO_SURF_CHROME_HEADLESS === "1";
let port = 0;
// Each case is served under /<index>/; {{cross}} names the same case on the other
// loopback host, so a child is cross-origin (and cross-site) to its top page.
const origin = (item, cross) => {
  const top = item.host || "127.0.0.1";
  const host = cross ? (top === "127.0.0.1" ? "localhost" : "127.0.0.1") : top;
  return `http://${host}:${port}`;
};
const server = createServer((request, response) => {
  const path = new URL(request.url, "http://localhost").pathname;
  if (path === "/report.js") {
    response.writeHead(200, { "Content-Type": "text/javascript", "Cache-Control": "no-store" });
    response.end(report);
    return;
  }
  const [, index, name] = /^\/(\d+)\/(.*)$/.exec(path) || [];
  const item = cases[Number(index)];
  const resource = item && (name ? item.resources?.[name] : item);
  if (!resource) {
    response.writeHead(404);
    response.end();
    return;
  }
  response.writeHead(
    200,
    [["Content-Type", "text/html"], ["Cache-Control", "no-store"], ...resource.headers].flat(),
  );
  response.end(resource.html.replaceAll("{{cross}}", `${origin(item, true)}/${index}/`));
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
  port = server.address().port;
  const url = `http://127.0.0.1:${port}/`;
  const flags = [
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    ...chromeExtensionArgs({ url, disableExtensions: true }),
    `--host-resolver-rules=MAP *.localhost 127.0.0.1`,
    ...(headless ? ["--headless=new"] : []),
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
    socket.addEventListener("open", done, { once: true });
    socket.addEventListener("error", fail, { once: true });
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
      { expression, returnByValue: true, awaitPromise: true, allowUnsafeEvalBlockedByCSP: false },
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
  for (let index = 0; index < cases.length; index++) {
    const target = `${origin(cases[index], false)}/${index}/`;
    await connection.call("Page.navigate", { url: target }, sessionId);
    const deadline = Date.now() + 10000;
    const expected = cases[index].messages || 0;
    while (
      await evaluate(
        `document.readyState !== 'complete' || location.href !== ${JSON.stringify(target)} || (window.__messages || []).length < ${expected}`,
      )
    ) {
      if (Date.now() > deadline) throw Error(`Case ${cases[index].id} did not load`);
      await pause(25);
    }
    result.push({ id: cases[index].id, result: await evaluate(probe) });
  }
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    casesSha256: createHash("sha256").update(casesText).digest("hex"),
    probeSha256: createHash("sha256").update(probe).digest("hex"),
    reportSha256: createHash("sha256").update(report).digest("hex"),
    headless,
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
