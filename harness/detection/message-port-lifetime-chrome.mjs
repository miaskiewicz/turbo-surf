// Host-triggered GC control over a normal installed Chrome; no page GC API.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { createServer as createHttpServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { cdpConnection } from "./cdp-connection.mjs";
const directory = dirname(fileURLToPath(import.meta.url));
const output = resolve(
  process.argv[2] || join(directory, "artifacts/message-port-lifetime-chrome"),
);
mkdirSync(output, { recursive: true });
const profile = mkdtempSync(join(tmpdir(), "turbo-port-gc-control-"));
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const server = createServer();
const pageServer = createHttpServer((_request, response) => {
  response.setHeader("Content-Type", "text/html");
  response.end("<!doctype html><body></body>");
});
let chrome, socket, connection;
const killOwnedChrome = (signal) => {
  if (!chrome?.pid) return;
  try {
    // detached:true gives this task its own process group; never kill by name.
    process.kill(-chrome.pid, signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
};
const listen = (listener) =>
  new Promise((resolve, reject) => {
    listener.once("error", reject);
    listener.listen(0, "127.0.0.1", () => {
      listener.removeListener("error", reject);
      resolve();
    });
  });
const closeServer = async (listener) => {
  if (!listener.listening) return;
  listener.closeAllConnections?.();
  await new Promise((resolve) => listener.close(resolve));
};
try {
  await listen(server);
  const port = server.address().port;
  await closeServer(server);
  await listen(pageServer);
  const pageUrl = `http://127.0.0.1:${pageServer.address().port}/`;
  chrome = spawn(
    process.env.TURBO_SURF_CHROME_PATH ||
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    [
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      "about:blank",
    ],
    { detached: true, stdio: ["ignore", "ignore", "pipe"] },
  );
  let spawnError;
  chrome.once("error", (error) => {
    spawnError = error;
  });
  let stderr = "";
  chrome.stderr.on("data", (chunk) => {
    if (stderr.length < 1048576) stderr += chunk;
  });
  const deadline = Date.now() + 30000;
  while (!stderr.includes("DevTools listening on")) {
    if (spawnError) throw spawnError;
    if (chrome.exitCode !== null || Date.now() > deadline) throw new Error("Chrome startup failed");
    await pause(50);
  }
  const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  writeFileSync(
    join(output, "version.json"),
    JSON.stringify({ browser: version.Browser, v8: version["V8-Version"] }, null, 2),
  );
  socket = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = reject;
  });
  const consoles = [];
  connection = cdpConnection(
    socket,
    (event) => {
      if (event.method === "Runtime.consoleAPICalled") {
        const row = event.params.args.map((arg) => arg.value);
        consoles.push(row);
        writeFileSync(join(output, "stage.json"), JSON.stringify(row, null, 2));
        writeFileSync(join(output, "console.json"), JSON.stringify(consoles, null, 2));
      }
    },
    10000,
  );
  const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
  await connection.call("Target.activateTarget", { targetId });
  const { sessionId } = await connection.call("Target.attachToTarget", { targetId, flatten: true });
  await connection.call("Runtime.enable", {}, sessionId);
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
  await connection.call("Page.enable", {}, sessionId);
  await connection.call("Page.navigate", { url: pageUrl }, sessionId);
  const readyDeadline = Date.now() + 10000;
  while ((await evaluate("document.readyState")) !== "complete") {
    if (Date.now() > readyDeadline) throw new Error("Control document did not load");
    await pause(20);
  }
  const probePath = process.argv[3]
    ? resolve(process.argv[3])
    : join(directory, "message-port-lifetime-probe.js");
  const hash = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
  writeFileSync(
    join(output, "source-pins.json"),
    JSON.stringify(
      {
        browser: version.Browser,
        v8: version["V8-Version"],
        mode: process.argv[4] === "--gc" || !process.argv[3] ? "host-gc" : "ordinary",
        files: [
          fileURLToPath(import.meta.url),
          join(directory, "cdp-connection.mjs"),
          probePath,
        ].map((path) => ({ path, sha256: hash(path) })),
        origin: "authored localhost",
        freshProfile: true,
        pageGcExposed: false,
      },
      null,
      2,
    ),
  );
  const gcProbe = process.argv[4] === "--gc";
  if (process.argv[3] && !gcProbe) {
    let result;
    try {
      const modes = process.argv[4] ? JSON.parse(process.argv[4]) : undefined;
      const prefix = modes ? "globalThis.__portGetterModes=" + JSON.stringify(modes) + ";\n" : "";
      result = await evaluate(prefix + readFileSync(resolve(process.argv[3]), "utf8"));
    } catch (error) {
      const stage = {
        lastConsole: existsSync(join(output, "stage.json"))
          ? JSON.parse(readFileSync(join(output, "stage.json"), "utf8"))
          : null,
      };
      writeFileSync(
        join(output, "failure.json"),
        JSON.stringify({ stage, error: String(error) }, null, 2),
      );
      throw error;
    }
    writeFileSync(
      join(output, "result.json"),
      JSON.stringify({ browser: version.Browser, v8: version["V8-Version"], result }, null, 2),
    );
    console.log(JSON.stringify({ output, result }));
  } else {
    await evaluate(
      readFileSync(
        gcProbe ? resolve(process.argv[3]) : join(directory, "message-port-lifetime-probe.js"),
        "utf8",
      ),
    );
    // Separate protocol tasks avoid WeakRef's current-job keep-alive guarantee.
    for (let index = 0; index < 3; index++)
      await connection.call("HeapProfiler.collectGarbage", {}, sessionId);
    const first = await evaluate("__readPortLifetime()");
    await evaluate("__portLifetimeState.retained = null");
    for (let index = 0; index < 3; index++)
      await connection.call("HeapProfiler.collectGarbage", {}, sessionId);
    const afterRelease = await evaluate("__readPortLifetime()");
    writeFileSync(
      join(output, "result.json"),
      JSON.stringify(
        { browser: version.Browser, v8: version["V8-Version"], first, afterRelease },
        null,
        2,
      ),
    );
    console.log(JSON.stringify({ output, first, afterRelease }));
  }
} finally {
  try {
    connection?.close();
  } finally {
    try {
      socket?.close();
    } finally {
      killOwnedChrome("SIGTERM");
      if (chrome?.pid && chrome.exitCode === null)
        await Promise.race([new Promise((resolve) => chrome.once("exit", resolve)), pause(5000)]);
      killOwnedChrome("SIGKILL");
      await closeServer(server);
      await closeServer(pageServer);
      rmSync(profile, { recursive: true, force: true });
      writeFileSync(
        join(output, "cleanup.json"),
        JSON.stringify(
          {
            ownedProcessGroup: chrome?.pid ?? null,
            terminated: true,
            profileRemoved: !existsSync(profile),
            serversClosed: !server.listening && !pageServer.listening,
          },
          null,
          2,
        ),
      );
    }
  }
}
