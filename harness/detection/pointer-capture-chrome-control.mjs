/* eslint-disable no-await-in-loop -- CDP input and evaluation steps are sequential by design. */
// Capture the committed pointer capture / pointer lock cases in an independently installed
// headless Chrome, driving trusted mouse input with CDP Input.dispatchMouseEvent at element
// centers. Self-contained CDP over --remote-debugging-pipe: no Playwright, no turbo-surf.
// Usage: node pointer-capture-chrome-control.mjs --chrome /path/to/chrome --out fixture.json
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));
const { values } = parseArgs({ options: { chrome: { type: "string" }, out: { type: "string" } } });
if (!values.chrome || !values.out) throw Error("Pass --chrome /path/to/chrome --out fixture.json");
const files = ["pointer-capture-probe.js", "pointer-capture-cases.json"].map((name) =>
  join(here, name),
);
const hash = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const probe = readFileSync(files[0], "utf8").trim();
const { cases } = JSON.parse(readFileSync(files[1], "utf8"));
const names = Object.keys(cases);
const server = createServer((request, response) => {
  const item = cases[names[Number(request.url.slice(1))]];
  response.setHeader("Content-Type", "text/html");
  response.end(`<!doctype html><html><head></head><body>${item ? item.html : ""}</body></html>`);
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const profile = mkdtempSync(join(tmpdir(), "pointer-capture-chrome-"));
const flags = [
  "--headless=new",
  "--remote-debugging-pipe",
  `--user-data-dir=${profile}`,
  "--no-first-run",
  "--no-default-browser-check",
  "--disable-extensions",
  "--disable-component-extensions-with-background-pages",
  "about:blank",
];
const chrome = spawn(resolve(values.chrome), flags, {
  stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"],
});
const [, , , input, output] = chrome.stdio;
let buffer = "",
  sequence = 0;
const pending = new Map();
output.on("data", (chunk) => {
  buffer += chunk;
  let end;
  while ((end = buffer.indexOf("\0")) >= 0) {
    const message = JSON.parse(buffer.slice(0, end));
    buffer = buffer.slice(end + 1);
    const waiter = message.id && pending.get(message.id);
    if (!waiter) continue;
    pending.delete(message.id);
    if (message.error) waiter.reject(Error(JSON.stringify(message.error)));
    else waiter.resolve(message.result);
  }
});
const call = (method, params = {}, sessionId) =>
  new Promise((resolveCall, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve: resolveCall, reject });
    input.write(`${JSON.stringify({ id, method, params, sessionId })}\0`);
  });
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
try {
  const version = await call("Browser.getVersion");
  const result = {};
  for (const [index, name] of names.entries()) {
    const item = cases[name];
    const { targetId } = await call("Target.createTarget", {
      url: `http://127.0.0.1:${server.address().port}/${index}`,
    });
    const { sessionId } = await call("Target.attachToTarget", { targetId, flatten: true });
    await call("Runtime.enable", {}, sessionId);
    // The same async-body evaluation the offline regression uses: JSON of the returned value.
    const evaluate = async (body) => {
      const evaluated = await call(
        "Runtime.evaluate",
        {
          expression: `(async()=>{${body}\n})().then(value=>JSON.stringify(value===undefined?null:value))`,
          awaitPromise: true,
          returnByValue: true,
        },
        sessionId,
      );
      if (evaluated.exceptionDetails) throw Error(JSON.stringify(evaluated.exceptionDetails));
      return JSON.parse(evaluated.result.value);
    };
    for (let attempt = 0; attempt < 100; attempt++) {
      if ((await evaluate("return document.readyState")) === "complete") break;
      await pause(20);
    }
    await evaluate(`(${probe})(${JSON.stringify(item.options || {})});${item.setup || ""}`);
    const center = (selector) =>
      evaluate(
        `const box=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return {x:box.left+box.width/2,y:box.top+box.height/2}`,
      );
    const mouse = (type, point, buttons) =>
      call(
        "Input.dispatchMouseEvent",
        {
          type,
          ...point,
          button: type === "mouseMoved" && !buttons ? "none" : "left",
          buttons,
          // A physical mouse reports no force, which Chrome maps to 0.5 while pressed; CDP
          // defaults force to 0, so supply the hardware-equivalent pressure explicitly.
          force: buttons ? 0.5 : 0,
          clickCount: type === "mouseMoved" ? 0 : 1,
        },
        sessionId,
      );
    const evaluations = [];
    let buttons = 0,
      marker = 0;
    for (const step of item.steps) {
      if (step.eval !== undefined) {
        evaluations.push(await evaluate(step.eval));
        continue;
      }
      const kind = Object.keys(step)[0];
      const label = `--${marker++} ${kind}`;
      if (!step.quiet) await evaluate(`__log.push(${JSON.stringify(label)})`);
      if (kind === "move") await mouse("mouseMoved", await center(step.move.selector), buttons);
      if (kind === "down" || kind === "click") {
        buttons = 1;
        await mouse("mousePressed", await center(step.down || step.click), buttons);
      }
      if (kind === "up" || kind === "click") {
        buttons = 0;
        await mouse("mouseReleased", await center(step.up || step.click), buttons);
      }
    }
    const log = await evaluate("await sleep(100);return __log");
    result[name] = { evaluations, log };
    await call("Target.closeTarget", { targetId });
  }
  const control = {
    browser: version.product,
    v8: version.jsVersion,
    probeSha256: hash(files[0]),
    casesSha256: hash(files[1]),
    collectorSha256: hash(fileURLToPath(import.meta.url)),
    flags: flags.map((flag) =>
      flag.startsWith("--user-data-dir=") ? "--user-data-dir=<fresh-profile>" : flag,
    ),
    result,
  };
  writeFileSync(resolve(values.out), `${JSON.stringify(control, null, 2)}\n`);
  console.log(
    JSON.stringify({ browser: control.browser, cases: names.length, out: resolve(values.out) }),
  );
  await call("Browser.close").catch(() => {});
} finally {
  const exited =
    chrome.exitCode === null && chrome.signalCode === null
      ? new Promise((done) => chrome.once("exit", done))
      : null;
  chrome.kill();
  await exited;
  await new Promise((done) => server.close(done));
  rmSync(profile, { recursive: true, force: true, maxRetries: 10 });
}
