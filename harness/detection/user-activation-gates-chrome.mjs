/* eslint-disable no-await-in-loop -- CDP input and evaluation steps are sequential by design. */
// Collect activation-gated behavior (showPicker, fullscreen, close watchers, light
// dismiss, MessageEvent.userActivation) in a fresh installed Chrome, driving REAL
// trusted input through CDP Input.dispatchMouseEvent / Input.dispatchKeyEvent.
// Each case runs in its own fresh tab; `eval` steps never carry a user gesture.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";

const [casesArgument, outputArgument] = process.argv.slice(2);
if (!casesArgument || !outputArgument)
  throw new Error("Usage: node user-activation-gates-chrome.mjs cases.json output.json");
const output = resolve(outputArgument),
  casesText = readFileSync(resolve(casesArgument), "utf8"),
  { prelude, cases } = JSON.parse(casesText);
const headless = process.env.TURBO_SURF_CHROME_HEADLESS === "1";
const only = process.env.TURBO_SURF_CASES ? process.env.TURBO_SURF_CASES.split(",") : null;
const profile = mkdtempSync(join(tmpdir(), "turbo-activation-gates-"));
// The page is the case's HTML after the shared prelude, exactly as the offline test composes it.
const server = createServer((request, response) => {
  const index = Number(new URL(request.url, "http://localhost").pathname.slice(1));
  const item = cases[index];
  if (!item) {
    response.writeHead(404);
    response.end();
    return;
  }
  response.writeHead(200, { "Content-Type": "text/html", "Cache-Control": "no-store" });
  response.end(`<!doctype html><script>${prelude}</script>${item.html}`);
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
  const origin = `http://127.0.0.1:${server.address().port}`;
  const flags = [
    "--remote-debugging-port=0",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    ...chromeExtensionArgs({ url: origin, disableExtensions: true, foregroundFrames: true }),
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
  const result = [];
  for (let index = 0; index < cases.length; index++) {
    const item = cases[index];
    if (only && !only.includes(item.id)) continue;
    const { targetId } = await connection.call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await connection.call("Target.attachToTarget", {
      targetId,
      flatten: true,
    });
    const call = (method, params = {}) => connection.call(method, params, sessionId);
    await call("Runtime.enable");
    await call("Page.enable");
    // A native file chooser would block the oracle; interception is invisible to the page.
    await call("Page.setInterceptFileChooserDialog", { enabled: true });
    await call("Page.bringToFront");
    const evaluate = async (expression) => {
      const evaluated = await call("Runtime.evaluate", {
        expression,
        returnByValue: true,
        awaitPromise: true,
        userGesture: false,
      });
      if (evaluated.exceptionDetails)
        throw new Error(
          `${item.id}: ${evaluated.exceptionDetails.exception?.description || evaluated.exceptionDetails.text}`,
        );
      return evaluated.result.value;
    };
    const target = `${origin}/${index}`;
    await call("Page.navigate", { url: target });
    const ready = Date.now() + 10000;
    while (
      await evaluate(
        `document.readyState !== 'complete' || location.href !== ${JSON.stringify(target)}`,
      )
    ) {
      if (Date.now() > ready) throw new Error(`Case ${item.id} did not load`);
      await pause(25);
    }
    const values = [];
    for (const step of item.steps) {
      if (step.eval != null) values.push(await evaluate(step.eval));
      else if (step.click != null) {
        const point = await evaluate(
          `(()=>{const r=document.querySelector(${JSON.stringify(step.click)}).getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`,
        );
        for (const type of ["mousePressed", "mouseReleased"])
          await call("Input.dispatchMouseEvent", { type, ...point, button: "left", clickCount: 1 });
      } else if (step.press != null) {
        const keyCode = { Escape: 27, Enter: 13 }[step.press];
        const key = { key: step.press, code: step.press, windowsVirtualKeyCode: keyCode };
        await call("Input.dispatchKeyEvent", { type: "rawKeyDown", ...key });
        await call("Input.dispatchKeyEvent", { type: "keyUp", ...key });
      } else if (step.wait != null) await pause(step.wait);
      else throw new Error(`${item.id}: unsupported step`);
    }
    result.push({ id: item.id, values });
    await connection.call("Target.closeTarget", { targetId });
  }
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    casesSha256: createHash("sha256").update(casesText).digest("hex"),
    collectorSha256: createHash("sha256")
      .update(readFileSync(new URL(import.meta.url)))
      .digest("hex"),
    headless,
    flags: flags.map((flag) =>
      flag.startsWith("--user-data-dir=") ? "--user-data-dir=<fresh-profile>" : flag,
    ),
    errors,
    result,
  };
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(control, null, 2)}\n`);
  if (errors.length) throw new Error(`Chrome control reported ${errors.length} exception(s)`);
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
