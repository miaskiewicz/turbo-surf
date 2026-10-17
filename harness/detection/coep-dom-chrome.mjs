// Collect a source-pinned authored DOM control in a fresh, normal installed Chrome.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";

const [outputArgument] = process.argv.slice(2);
const probeArgument = new URL("./coep-dom-probe.js", import.meta.url).pathname;
if (!probeArgument || !outputArgument)
  throw new Error("Usage: node coep-dom-chrome.mjs output.json");
const probePath = resolve(probeArgument),
  output = resolve(outputArgument);
const probe = readFileSync(probePath, "utf8"),
  profile = mkdtempSync(join(tmpdir(), "turbo-dom-oracle-"));
const requests = [];
let crossOrigin, sourceOrigin;
const htmlFixture = readFileSync(new URL("./fixtures/coep-dom.html", import.meta.url), "utf8");
const cross = createServer((request, response) => {
  requests.push({ path: request.url, cookie: request.headers.cookie || "" });
  const headers = [
    ["Access-Control-Allow-Origin", sourceOrigin],
    ["Access-Control-Allow-Credentials", "true"],
  ];
  let body;
  if (request.url.endsWith(".css")) {
    headers.push(["Content-Type", "text/css"]);
    body = "sentinel { color: rgb(12, 34, 56) }";
  } else if (request.url === "/pixel.png") {
    headers.push(["Content-Type", "image/png"]);
    body = readFileSync(new URL("./fixtures/pixel.png", import.meta.url));
  } else {
    headers.push(["Content-Type", "text/javascript"]);
    body =
      request.url === "/plain.js"
        ? "globalThis.coepPlain=(globalThis.coepPlain||0)+1"
        : "globalThis.coepAuthorized=(globalThis.coepAuthorized||0)+1";
  }
  if (request.url === "/authorized.js")
    headers.push(["Cross-Origin-Resource-Policy", "cross-origin"]);
  response.writeHead(200, headers.flat());
  response.end(body);
});
const server = createServer((request, response) => {
  const policy = new URL(request.url, "http://localhost").searchParams.get("policy");
  response.setHeader("Content-Type", "text/html");
  response.setHeader("Cache-Control", "no-store");
  response.setHeader("Set-Cookie", "seed=local; HttpOnly; Path=/");
  if (policy !== "unsafe-none") response.setHeader("Cross-Origin-Embedder-Policy", policy);
  response.end(htmlFixture.replaceAll("{{CROSS}}", crossOrigin));
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
  await new Promise((done) => cross.listen(0, "127.0.0.1", done));
  crossOrigin = `http://127.0.0.1:${cross.address().port}`;
  sourceOrigin = `http://127.0.0.1:${server.address().port}`;
  const url = sourceOrigin + "/";
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
  for (const policy of ["unsafe-none", "require-corp", "credentialless"]) {
    requests.length = 0;
    await connection.call("Page.navigate", { url: url + "?policy=" + policy }, sessionId);
    const readyDeadline = Date.now() + 10000;
    while (
      (await evaluate("document.readyState==='complete' && typeof resourceEvents==='object'")) !==
      true
    ) {
      if (Date.now() > readyDeadline) throw new Error("Authored COEP page did not finish loading");
      await pause(25);
    }
    const rows = await evaluate(probe);
    result.push({
      policy,
      rows,
      requests: [...requests].sort((a, b) => a.path.localeCompare(b.path)),
    });
  }
  const control = {
    htmlSha256: createHash("sha256").update(htmlFixture).digest("hex"),
    pixelSha256: createHash("sha256")
      .update(readFileSync(new URL("./fixtures/pixel.png", import.meta.url)))
      .digest("hex"),
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
  cross.closeAllConnections();
  await new Promise((done) => cross.close(done));
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  rmSync(profile, { recursive: true, force: true });
}
