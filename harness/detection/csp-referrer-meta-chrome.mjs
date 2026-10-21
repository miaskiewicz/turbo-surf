// Collect a source-pinned authored DOM control in a fresh, normal installed Chrome.
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
  throw new Error("Usage: node csp-referrer-meta-chrome.mjs cases.json probe.js output.json");
const probePath = resolve(probeArgument),
  output = resolve(outputArgument);
const probe = readFileSync(probePath, "utf8"),
  profile = mkdtempSync(join(tmpdir(), "turbo-dom-oracle-"));
const casesPath = resolve(casesArgument),
  casesText = readFileSync(casesPath, "utf8"),
  cases = JSON.parse(casesText);
const requests = [];
const server = createServer((request, response) => {
  const parsed = new URL(request.url, "http://localhost");
  const id = Number(parsed.searchParams.get("case") || 0),
    item = cases[id];
  requests.push({ url: request.url, referrer: request.headers.referer || "" });
  if (parsed.pathname === "/start") {
    response.writeHead(200, {
      "Content-Type": "text/html",
      "Referrer-Policy": item.policy || "strict-origin-when-cross-origin",
      "Content-Security-Policy": "style-src 'none'",
    });
    response.end(item.html || "<!doctype html><title>start</title>");
    return;
  }
  if (parsed.pathname === "/redirect") {
    response.writeHead(302, { Location: "/page?case=" + id });
    response.end();
    return;
  }
  response.writeHead(item.status, {
    "Content-Type": "text/html",
    "Cache-Control": "no-store",
    "Content-Security-Policy": "style-src 'none'",
  });
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
  const errors = [],
    contexts = new Map();
  connection = cdpConnection(
    socket,
    (event) => {
      if (event.method === "Runtime.exceptionThrown") errors.push(event.params.exceptionDetails);
      if (
        event.method === "Runtime.executionContextCreated" &&
        event.params.context.auxData?.isDefault
      )
        contexts.set(event.params.context.auxData.frameId, {
          id: event.params.context.id,
          sessionId: event.sessionId,
        });
    },
    30000,
  );
  const version = await connection.call("Browser.getVersion");
  const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await connection.call("Target.attachToTarget", { targetId, flatten: true });
  await connection.call("Runtime.enable", {}, sessionId);
  await connection.call("Page.enable", {}, sessionId);
  await connection.call("Page.navigate", { url }, sessionId);
  const evaluate = async (expression, context) => {
    const result = await connection.call(
      "Runtime.evaluate",
      {
        expression,
        returnByValue: true,
        awaitPromise: true,
        allowUnsafeEvalBlockedByCSP: false,
        ...(context ? { contextId: context.id } : {}),
      },
      context?.sessionId || sessionId,
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
  const normalize = (value) =>
    JSON.parse(
      JSON.stringify(value)
        .replaceAll(url, "http://metadata.test/")
        .replaceAll(`http://localhost:${server.address().port}/`, "http://cross.metadata.test/"),
    );
  const result = [];
  for (let index = 0; index < cases.length; index++) {
    const item = cases[index],
      target =
        url +
        (item.redirect ? "redirect" : "page") +
        "?case=" +
        index +
        (item.fragment ? "#authored" : "");
    if (item.frame) {
      const source = url + "start?case=" + index + "&private=authored#fragment";
      await connection.call("Page.navigate", { url: source }, sessionId);
      const deadline = Date.now() + 10000;
      while (
        await evaluate(
          "document.readyState!=='complete'||location.pathname!=='/start'||location.search!==" +
            JSON.stringify("?case=" + index + "&private=authored"),
        )
      ) {
        if (Date.now() > deadline) throw Error("Frame owner did not load");
        await pause(25);
      }
      if (item.steps) await evaluate(item.steps);
      const child =
        (item.child === "cross" ? `http://localhost:${server.address().port}/` : url) +
        "page?case=" +
        index +
        (item.fragment ? "#authored" : "");
      const options = { kind: item.frame, url: child, framePolicy: item.framePolicy };
      await evaluate(
        "(()=>{const options=" +
          JSON.stringify(options) +
          ";const frame=document.createElement('iframe');if(options.framePolicy!==undefined)frame.setAttribute('referrerpolicy',options.framePolicy);Object.defineProperty(frame,'referrerPolicy',{get(){throw new Error('guest referrerPolicy getter')}});if(options.kind==='srcdoc')frame.srcdoc='<!doctype html><head></head><body></body>';else if(options.kind==='data')frame.src='data:text/html,%3Chead%3E%3C%2Fhead%3E%3Cbody%3E%3C%2Fbody%3E';else if(options.kind==='http')frame.src=options.url;document.body.append(frame);})()",
      );
      let contextId;
      while (!contextId) {
        const tree = await connection.call("Page.getFrameTree", {}, sessionId);
        contextId = contexts.get(tree.frameTree.childFrames?.[0]?.frame.id);
        if (!contextId) {
          const targets = await connection.call("Target.getTargets");
          const target = targets.targetInfos.find(
            (target) =>
              target.type === "iframe" &&
              target.url ===
                (item.frame === "data"
                  ? "data:text/html,%3Chead%3E%3C%2Fhead%3E%3Cbody%3E%3C%2Fbody%3E"
                  : child),
          );
          if (target) {
            contextId = contexts.get(target.targetId);
            if (!contextId) {
              const attached = await connection.call("Target.attachToTarget", {
                targetId: target.targetId,
                flatten: true,
              });
              await connection.call("Runtime.enable", {}, attached.sessionId);
              contextId = contexts.get(target.targetId);
            }
          }
        }
        if (Date.now() > deadline) {
          writeFileSync(
            output + ".diagnostic.json",
            JSON.stringify(
              {
                item,
                tree,
                contexts: [...contexts],
                targets: await connection.call("Target.getTargets"),
              },
              null,
              2,
            ),
          );
          throw Error("Frame context missing: " + item.id);
        }
        if (!contextId) await pause(25);
      }
      while (await evaluate("document.readyState!=='complete'", contextId)) {
        if (Date.now() > deadline) throw Error("Frame incomplete");
        await pause(25);
      }
      result.push({
        id: item.id,
        result: normalize(await evaluate(probe, contextId)),
        request: normalize(
          requests.filter((request) => request.url === "/page?case=" + index).at(-1) || null,
        ),
      });
      continue;
    }
    if (item.source) {
      const source =
        (item.source === "cross" ? `http://localhost:${server.address().port}/` : url) +
        "start?case=" +
        index +
        "&private=authored#fragment";
      await connection.call("Page.navigate", { url: source }, sessionId);
      const deadline = Date.now() + 10000;
      while (
        await evaluate(
          "document.readyState!=='complete'||location.pathname!=='/start'||location.search!==" +
            JSON.stringify("?case=" + index + "&private=authored"),
        )
      ) {
        if (Date.now() > deadline) throw Error("Source did not load");
        await pause(25);
      }
      await evaluate("location.href=" + JSON.stringify(target));
    } else await connection.call("Page.navigate", { url: target }, sessionId);
    const deadline = Date.now() + 10000;
    while (
      await evaluate(
        "document.readyState!=='complete'||location.pathname!=='/page'||location.search!==" +
          JSON.stringify("?case=" + index),
      )
    ) {
      if (Date.now() > deadline) throw Error("Metadata page did not load");
      await pause(25);
    }
    result.push({
      id: item.id,
      result: normalize(await evaluate(probe)),
      request: normalize(
        requests.filter((request) => request.url === "/page?case=" + index).at(-1),
      ),
    });
  }
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    casesSha256: createHash("sha256").update(casesText).digest("hex"),
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
