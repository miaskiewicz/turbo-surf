/* eslint-disable no-await-in-loop -- CDP commands and navigation readiness checks are sequential by design. */
// Collect multipart/form-data wire bytes from a fresh installed Chrome: fetch() with a
// FormData body, a FormData built from a form with file controls, and a native form
// submission, for a UTF-8 and a windows-1252 document.
//   node harness/detection/form-multipart-chrome.mjs harness/detection/form-multipart-probe.js \
//     harness/detection/fixtures/form_multipart_chrome156.json
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";

const [probeArgument, outputArgument] = process.argv.slice(2);
if (!probeArgument || !outputArgument)
  throw new Error("Usage: node form-multipart-chrome.mjs probe.js output.json");
const probe = readFileSync(resolve(probeArgument), "utf8"),
  output = resolve(outputArgument),
  profile = mkdtempSync(join(tmpdir(), "turbo-form-multipart-"));
const charsets = ["utf-8", "windows-1252"];
const server = createServer(async (request, response) => {
  const url = new URL(request.url, "http://127.0.0.1");
  if (url.pathname === "/wire") {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const wire = {
      method: request.method,
      contentType: request.headers["content-type"] ?? null,
      bodyHex: Buffer.concat(chunks).toString("hex"),
    };
    if (url.searchParams.get("kind") === "submit") {
      response.setHeader("Content-Type", "text/html; charset=utf-8");
      response.end(
        `<body>${JSON.stringify(wire).replaceAll("&", "&amp;").replaceAll("<", "&lt;")}</body>`,
      );
    } else {
      response.setHeader("Content-Type", "application/json");
      response.end(JSON.stringify(wire));
    }
    return;
  }
  const charset = charsets[Number(url.pathname.slice(1))] || "utf-8";
  response.setHeader("Content-Type", `text/html; charset=${charset}`);
  response.end("<!doctype html><html><head></head><body></body></html>");
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
  const flags = [
    "--headless=new",
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-extensions",
    "--disable-component-extensions-with-background-pages",
    "about:blank",
  ];
  chrome = spawn(
    process.env.TURBO_SURF_CHROME_PATH ||
      "/Applications/User Apps/Google Chrome.app/Contents/MacOS/Google Chrome",
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
  const result = [];
  for (let index = 0; index < charsets.length; index++) {
    const url = `http://127.0.0.1:${server.address().port}/${index}`;
    await connection.call("Page.navigate", { url }, sessionId);
    const ready = Date.now() + 10000;
    while (
      await evaluate(
        `document.readyState !== 'complete' || location.href !== ${JSON.stringify(url)}`,
      )
    ) {
      if (Date.now() > ready) throw Error(`Case ${charsets[index]} did not load`);
      await pause(25);
    }
    result.push({
      id: charsets[index],
      characterSet: await evaluate("document.characterSet"),
      result: await evaluate(probe),
    });
  }
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
